//! Interactive Brokers paper-gateway configuration and normalized adapter contract.
//!
//! The adapter deliberately permits only documented paper ports. It translates
//! no strategy or risk decision: the core submits normalized requests only after
//! its own OMS and risk controls accept them. A TWS/Gateway transport plugs into
//! [`IbkrPaperGatewayTransport`] and is continuously exercised by the core's
//! deterministic in-memory paper model and fault-injection suite.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use follon_commercial::{
    verify_release_artifacts, verify_release_signature, ReleaseManifest, ReleaseSignature,
    TrustedReleaseKey,
};
use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, OrderState};
use follon_live::{
    LiveBrokerAccountSnapshot, LiveBrokerAdapter, LiveBrokerEvent, LiveBrokerOrderRequest,
    LiveBrokerReplaceRequest, LiveBrokerSubmitResult, LiveError,
};
use follon_paper::{
    BrokerAccountSnapshot, BrokerCancelRequest, BrokerComboRequest, BrokerEvent,
    BrokerOrderRequest, BrokerOrderSnapshot, BrokerPositionSnapshot, BrokerSubmitResult,
    PaperBrokerAdapter, PaperError,
};
use follon_secrets::SecretMaterial;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const BRIDGE_PROTOCOL_VERSION: u32 = 1;

/// IBKR TWS/Gateway endpoint allowed for paper operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IbkrPaperGatewayConfiguration {
    /// Canonical account identity selected for the paper session.
    pub account_id: String,
    /// Local TWS/Gateway hostname; public and live endpoints are refused.
    pub host: String,
    /// IBKR paper TWS (7497) or Gateway (4002) port.
    pub port: u16,
    /// Must be the literal value `PAPER`.
    pub environment: String,
}

impl IbkrPaperGatewayConfiguration {
    /// Rejects any endpoint outside an explicit local IBKR paper gateway.
    pub fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("IBKR paper account_id", &self.account_id)?;
        if !matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "::1")
            || !matches!(self.port, 7497 | 4002)
            || self.environment != "PAPER"
        {
            return Err(PaperError(
                "IBKR adapter accepts only a local PAPER TWS (7497) or Gateway (4002) endpoint"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// Minimal transport implemented by a pinned, audited IBKR TWS/Gateway client.
///
/// This deliberate seam keeps vendor wire parsing outside the OMS. A transport
/// must preserve the pre-generated `client_order_id` as the adapter idempotency
/// key and return normalized evidence; it has no permission to create orders,
/// bypass a kill switch, or alter risk decisions.
pub trait IbkrPaperGatewayTransport {
    /// Sends one normalized paper order request.
    fn submit_paper_order(
        &mut self,
        request: &BrokerOrderRequest,
    ) -> Result<BrokerSubmitResult, PaperError>;
    /// Sends one normalized paper combination request (BAG order).
    fn submit_paper_combo(
        &mut self,
        _request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        Err(PaperError(
            "IBKR transport does not implement BAG orders".to_owned(),
        ))
    }
    /// Requests cancellation of one pre-generated client order identity.
    fn cancel_paper_order(&mut self, client_order_id: &str) -> Result<(), PaperError>;
    /// Drains normalized IBKR order/execution evidence.
    fn poll_paper_events(&mut self) -> Result<Vec<BrokerEvent>, PaperError>;
    /// Retrieves an independent IBKR paper account snapshot.
    fn paper_account_snapshot(
        &mut self,
        account_id: &str,
    ) -> Result<BrokerAccountSnapshot, PaperError>;
    /// Reconnects to the configured paper endpoint; reconciliation follows in the core.
    fn reconnect_paper(&mut self) -> Result<(), PaperError>;
}

/// Fixed process boundary for the official-API IBKR gateway bridge.
///
/// The program path must be absolute. The child receives normalized requests on stdin and emits
/// bounded protocol-v1 JSON lines on stdout. No shell, credential argument, or public socket is
/// involved. A timed-out or malformed session is poisoned until `reconnect_paper` restarts it.
#[derive(Clone)]
pub struct IbkrPaperBridgeProcessConfiguration {
    /// Absolute bridge executable, normally the approved Python interpreter or packaged bridge.
    pub executable: PathBuf,
    /// Fixed non-sensitive arguments selecting the reviewed bridge and PAPER configuration.
    pub arguments: Vec<String>,
    /// Maximum time to wait for each bridge response.
    pub request_timeout: Duration,
    /// Maximum UTF-8 bytes accepted for one response line.
    pub max_response_bytes: usize,
}

impl IbkrPaperBridgeProcessConfiguration {
    /// Validates the process and resource boundary before a child can start.
    pub fn validate(&self) -> Result<(), PaperError> {
        if !self.executable.is_absolute()
            || !self.executable.is_file()
            || self.arguments.len() > 64
            || self
                .arguments
                .iter()
                .any(|argument| argument.len() > 4_096 || argument.contains('\0'))
            || self.request_timeout < Duration::from_millis(10)
            || self.request_timeout > Duration::from_secs(60)
            || !(256..=1024 * 1024).contains(&self.max_response_bytes)
        {
            return Err(PaperError(
                "invalid IBKR paper bridge process configuration".to_owned(),
            ));
        }
        Ok(())
    }
}

struct BridgeProcessSession {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<Result<String, String>>,
}

impl Drop for BridgeProcessSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Production process transport for a pinned local IBKR TWS/Gateway bridge.
pub struct IbkrPaperBridgeProcessTransport {
    configuration: IbkrPaperBridgeProcessConfiguration,
    session: BridgeProcessSession,
    next_request_id: u64,
    healthy: bool,
}

impl IbkrPaperBridgeProcessTransport {
    /// Starts the fixed bridge process and waits for requests through the normalized protocol.
    pub fn start(configuration: IbkrPaperBridgeProcessConfiguration) -> Result<Self, PaperError> {
        configuration.validate()?;
        let session = Self::start_session(&configuration)?;
        Ok(Self {
            configuration,
            session,
            next_request_id: 1,
            healthy: true,
        })
    }

    fn start_session(
        configuration: &IbkrPaperBridgeProcessConfiguration,
    ) -> Result<BridgeProcessSession, PaperError> {
        let mut child = Command::new(&configuration.executable)
            .args(&configuration.arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| PaperError("IBKR paper bridge process could not start".to_owned()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PaperError("IBKR paper bridge stdin is unavailable".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PaperError("IBKR paper bridge stdout is unavailable".to_owned()))?;
        let max_response_bytes = configuration.max_response_bytes;
        let (sender, responses) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let read_result = reader
                    .by_ref()
                    .take((max_response_bytes + 1) as u64)
                    .read_until(b'\n', &mut bytes);
                match read_result {
                    Ok(0) => {
                        let _ = sender.send(Err("IBKR paper bridge closed its output".to_owned()));
                        break;
                    }
                    Ok(_) if bytes.len() > max_response_bytes || !bytes.ends_with(b"\n") => {
                        let _ = sender.send(Err(
                            "IBKR paper bridge emitted an oversized response".to_owned()
                        ));
                        break;
                    }
                    Ok(_) => {
                        bytes.pop();
                        if bytes.ends_with(b"\r") {
                            bytes.pop();
                        }
                        match String::from_utf8(bytes) {
                            Ok(line) => {
                                if sender.send(Ok(line)).is_err() {
                                    break;
                                }
                            }
                            Err(_) => {
                                let _ = sender.send(Err(
                                    "IBKR paper bridge emitted non-UTF-8 output".to_owned(),
                                ));
                                break;
                            }
                        }
                    }
                    Err(_) => {
                        let _ = sender
                            .send(Err("IBKR paper bridge output could not be read".to_owned()));
                        break;
                    }
                }
            }
        });
        Ok(BridgeProcessSession {
            child,
            stdin,
            responses,
        })
    }

    fn restart(&mut self) -> Result<(), PaperError> {
        self.session = Self::start_session(&self.configuration)?;
        self.next_request_id = 1;
        self.healthy = true;
        Ok(())
    }

    fn request(&mut self, operation: &str, payload: Value) -> Result<Value, PaperError> {
        if !self.healthy {
            return Err(PaperError(
                "IBKR paper bridge is unhealthy; reconnect is required".to_owned(),
            ));
        }
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or_else(|| PaperError("IBKR bridge request counter overflowed".to_owned()))?;
        let request = BridgeRequest {
            protocol_version: BRIDGE_PROTOCOL_VERSION,
            request_id,
            operation,
            payload,
        };
        let serialized = serde_json::to_string(&request)
            .map_err(|error| PaperError(format!("IBKR bridge request is invalid: {error}")))?;
        if self
            .session
            .stdin
            .write_all(serialized.as_bytes())
            .and_then(|_| self.session.stdin.write_all(b"\n"))
            .and_then(|_| self.session.stdin.flush())
            .is_err()
        {
            self.healthy = false;
            return Err(PaperError(
                "IBKR paper bridge request outcome is unknown".to_owned(),
            ));
        }
        let line = match self
            .session
            .responses
            .recv_timeout(self.configuration.request_timeout)
        {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                self.healthy = false;
                return Err(PaperError(error));
            }
            Err(_) => {
                self.healthy = false;
                return Err(PaperError(
                    "IBKR paper bridge response timed out; outcome is unknown".to_owned(),
                ));
            }
        };
        let response: BridgeResponse = match serde_json::from_str(&line) {
            Ok(response) => response,
            Err(_) => {
                self.healthy = false;
                return Err(PaperError(
                    "IBKR paper bridge response is malformed".to_owned(),
                ));
            }
        };
        if response.protocol_version != BRIDGE_PROTOCOL_VERSION
            || response.request_id != request_id
            || response.ok == response.error.is_some()
            || response.ok != response.result.is_some()
        {
            self.healthy = false;
            return Err(PaperError(
                "IBKR paper bridge response does not match the request".to_owned(),
            ));
        }
        if let Some(error) = response.error {
            if validate_bridge_text("IBKR paper bridge error", &error).is_err() {
                self.healthy = false;
                return Err(PaperError(
                    "IBKR paper bridge error response is malformed".to_owned(),
                ));
            }
            return Err(PaperError(error));
        }
        response
            .result
            .ok_or_else(|| PaperError("IBKR paper bridge response has no result".to_owned()))
    }

    fn accept_bridge_contract<T>(
        &mut self,
        normalized: Result<T, PaperError>,
    ) -> Result<T, PaperError> {
        if normalized.is_err() {
            self.healthy = false;
        }
        normalized
    }
}

#[derive(Serialize)]
struct BridgeRequest<'a> {
    protocol_version: u32,
    request_id: u64,
    operation: &'a str,
    payload: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeResponse {
    protocol_version: u32,
    request_id: u64,
    ok: bool,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitBridgeResult {
    status: String,
    broker_order_id: Option<String>,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeEvent {
    event_type: String,
    execution_id: Option<String>,
    client_order_id: String,
    broker_order_id: Option<String>,
    quantity: Option<String>,
    price: Option<String>,
    fee: Option<String>,
    executed_at: Option<String>,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeSnapshot {
    orders: Vec<BridgeOrderSnapshot>,
    positions: Vec<BridgePositionSnapshot>,
    cash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeOrderSnapshot {
    client_order_id: String,
    broker_order_id: String,
    state: String,
    filled_quantity: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgePositionSnapshot {
    instrument_id: String,
    quantity: String,
}

impl IbkrPaperGatewayTransport for IbkrPaperBridgeProcessTransport {
    fn submit_paper_order(
        &mut self,
        request: &BrokerOrderRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        for (name, value) in [
            ("IBKR client_order_id", request.client_order_id.as_str()),
            ("IBKR account_id", request.account_id.as_str()),
            ("IBKR instrument_id", request.instrument_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if request.quantity <= Decimal::ZERO
            || request
                .limit_price
                .is_some_and(|price| price <= Decimal::ZERO)
        {
            return Err(PaperError("invalid IBKR paper order request".to_owned()));
        }
        let value = self.request(
            "submit",
            json!({
                "client_order_id": request.client_order_id,
                "account_id": request.account_id,
                "instrument_id": request.instrument_id,
                "side": request.side.as_str(),
                "quantity": request.quantity.to_string(),
                "limit_price": request.limit_price.map(|price| price.to_string()),
            }),
        )?;
        let normalized = serde_json::from_value::<SubmitBridgeResult>(value)
            .map_err(|_| PaperError("IBKR submit response is malformed".to_owned()))
            .and_then(|result| match result.status.as_str() {
                "ACKNOWLEDGED" => {
                    let broker_order_id = result.broker_order_id.ok_or_else(|| {
                        PaperError("IBKR acknowledgement has no broker order ID".to_owned())
                    })?;
                    validate_canonical_id("IBKR broker_order_id", &broker_order_id)?;
                    if result.reason.is_some() {
                        return Err(PaperError(
                            "IBKR acknowledgement unexpectedly contains a reason".to_owned(),
                        ));
                    }
                    Ok(BrokerSubmitResult::Acknowledged { broker_order_id })
                }
                "REJECTED" => Ok(BrokerSubmitResult::Rejected {
                    reason: required_bridge_reason(result.reason)?,
                }),
                "UNKNOWN" => Ok(BrokerSubmitResult::Unknown {
                    reason: required_bridge_reason(result.reason)?,
                }),
                _ => Err(PaperError(
                    "IBKR submit response has an unknown status".to_owned(),
                )),
            });
        self.accept_bridge_contract(normalized)
    }

    fn submit_paper_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        validate_canonical_id("IBKR client_order_id", &request.client_order_id)?;
        validate_canonical_id("IBKR account_id", &request.account_id)?;
        for leg in &request.legs {
            validate_canonical_id("IBKR leg instrument_id", &leg.instrument_id)?;
            if leg.ratio == 0 {
                return Err(PaperError(
                    "IBKR paper combo leg ratio must be positive".to_owned(),
                ));
            }
        }
        let legs = request
            .legs
            .iter()
            .map(|leg| {
                json!({
                    "instrument_id": leg.instrument_id,
                    "side": leg.side.as_str(),
                    "ratio": leg.ratio,
                })
            })
            .collect::<Vec<_>>();

        let value = self.request(
            "submit_combo",
            json!({
                "client_order_id": request.client_order_id,
                "account_id": request.account_id,
                "legs": legs,
                "limit_price": request.limit_price.map(|price| price.to_string()),
            }),
        )?;

        let normalized = serde_json::from_value::<SubmitBridgeResult>(value)
            .map_err(|_| PaperError("IBKR submit response is malformed".to_owned()))
            .and_then(|result| match result.status.as_str() {
                "ACKNOWLEDGED" => {
                    let broker_order_id = result.broker_order_id.ok_or_else(|| {
                        PaperError("IBKR acknowledgement has no broker order ID".to_owned())
                    })?;
                    validate_canonical_id("IBKR broker_order_id", &broker_order_id)?;
                    Ok(BrokerSubmitResult::Acknowledged { broker_order_id })
                }
                "REJECTED" => Ok(BrokerSubmitResult::Rejected {
                    reason: required_bridge_reason(result.reason)?,
                }),
                _ => Err(PaperError(
                    "IBKR submit response has an unknown status".to_owned(),
                )),
            });
        self.accept_bridge_contract(normalized)
    }

    fn cancel_paper_order(&mut self, client_order_id: &str) -> Result<(), PaperError> {
        validate_canonical_id("IBKR cancellation client_order_id", client_order_id)?;
        let value = self.request("cancel", json!({ "client_order_id": client_order_id }))?;
        let normalized = expect_empty_bridge_result("IBKR cancel", &value);
        self.accept_bridge_contract(normalized)
    }

    fn poll_paper_events(&mut self) -> Result<Vec<BrokerEvent>, PaperError> {
        let value = self.request("poll", json!({}))?;
        let normalized = serde_json::from_value::<Vec<BridgeEvent>>(value)
            .map_err(|_| PaperError("IBKR event response is malformed".to_owned()))
            .and_then(|events| events.into_iter().map(normalize_bridge_event).collect());
        self.accept_bridge_contract(normalized)
    }

    fn paper_account_snapshot(
        &mut self,
        account_id: &str,
    ) -> Result<BrokerAccountSnapshot, PaperError> {
        validate_canonical_id("IBKR snapshot account_id", account_id)?;
        let value = self.request("snapshot", json!({ "account_id": account_id }))?;
        let normalized = (|| {
            let snapshot: BridgeSnapshot = serde_json::from_value(value)
                .map_err(|_| PaperError("IBKR snapshot response is malformed".to_owned()))?;
            let orders = snapshot
                .orders
                .into_iter()
                .map(|order| {
                    validate_canonical_id("IBKR snapshot client_order_id", &order.client_order_id)?;
                    validate_canonical_id("IBKR snapshot broker_order_id", &order.broker_order_id)?;
                    let filled_quantity =
                        bridge_decimal("IBKR snapshot filled quantity", &order.filled_quantity)?;
                    if filled_quantity < Decimal::ZERO {
                        return Err(PaperError(
                            "IBKR snapshot filled quantity is negative".to_owned(),
                        ));
                    }
                    Ok(BrokerOrderSnapshot {
                        client_order_id: order.client_order_id,
                        broker_order_id: order.broker_order_id,
                        state: parse_bridge_order_state(&order.state)?,
                        filled_quantity,
                    })
                })
                .collect::<Result<Vec<_>, PaperError>>()?;
            let positions = snapshot
                .positions
                .into_iter()
                .map(|position| {
                    validate_canonical_id("IBKR snapshot instrument_id", &position.instrument_id)?;
                    Ok(BrokerPositionSnapshot {
                        instrument_id: position.instrument_id,
                        quantity: bridge_decimal("IBKR snapshot position", &position.quantity)?,
                    })
                })
                .collect::<Result<Vec<_>, PaperError>>()?;
            Ok(BrokerAccountSnapshot {
                orders,
                positions,
                cash: bridge_decimal("IBKR snapshot cash", &snapshot.cash)?,
            })
        })();
        self.accept_bridge_contract(normalized)
    }

    fn reconnect_paper(&mut self) -> Result<(), PaperError> {
        if !self.healthy {
            self.restart()?;
        }
        let value = self.request("reconnect", json!({}))?;
        let normalized = expect_empty_bridge_result("IBKR reconnect", &value);
        self.accept_bridge_contract(normalized)
    }
}

fn expect_empty_bridge_result(operation: &str, value: &Value) -> Result<(), PaperError> {
    if value.as_object().is_some_and(serde_json::Map::is_empty) {
        Ok(())
    } else {
        Err(PaperError(format!("{operation} response is malformed")))
    }
}

fn normalize_bridge_event(event: BridgeEvent) -> Result<BrokerEvent, PaperError> {
    validate_canonical_id("IBKR event client_order_id", &event.client_order_id)?;
    match event.event_type.as_str() {
        "ACKNOWLEDGED" => {
            let broker_order_id = event.broker_order_id.ok_or_else(|| {
                PaperError("IBKR acknowledgement event has no broker order ID".to_owned())
            })?;
            validate_canonical_id("IBKR event broker_order_id", &broker_order_id)?;
            Ok(BrokerEvent::Acknowledged {
                client_order_id: event.client_order_id,
                broker_order_id,
            })
        }
        "EXECUTION" => {
            let execution_id = event
                .execution_id
                .ok_or_else(|| PaperError("IBKR execution event has no execution ID".to_owned()))?;
            let broker_order_id = event.broker_order_id.ok_or_else(|| {
                PaperError("IBKR execution event has no broker order ID".to_owned())
            })?;
            validate_canonical_id("IBKR execution_id", &execution_id)?;
            validate_canonical_id("IBKR execution broker_order_id", &broker_order_id)?;
            let executed_at = event
                .executed_at
                .ok_or_else(|| PaperError("IBKR execution event has no timestamp".to_owned()))?;
            validate_utc_timestamp("IBKR execution time", &executed_at)?;
            Ok(BrokerEvent::Execution {
                execution_id,
                client_order_id: event.client_order_id,
                broker_order_id,
                quantity: bridge_decimal(
                    "IBKR execution quantity",
                    event.quantity.as_deref().ok_or_else(|| {
                        PaperError("IBKR execution event has no quantity".to_owned())
                    })?,
                )?,
                price: bridge_decimal(
                    "IBKR execution price",
                    event.price.as_deref().ok_or_else(|| {
                        PaperError("IBKR execution event has no price".to_owned())
                    })?,
                )?,
                fee: bridge_decimal(
                    "IBKR execution fee",
                    event
                        .fee
                        .as_deref()
                        .ok_or_else(|| PaperError("IBKR execution event has no fee".to_owned()))?,
                )?,
                executed_at,
            })
        }
        "CANCELLED" => Ok(BrokerEvent::Cancelled {
            client_order_id: event.client_order_id,
            reason: required_bridge_reason(event.reason)?,
        }),
        "REJECTED" => Ok(BrokerEvent::Rejected {
            client_order_id: event.client_order_id,
            reason: required_bridge_reason(event.reason)?,
        }),
        _ => Err(PaperError(
            "IBKR bridge emitted an unknown event type".to_owned(),
        )),
    }
}

fn required_bridge_reason(reason: Option<String>) -> Result<String, PaperError> {
    let reason = reason.ok_or_else(|| PaperError("IBKR response has no reason".to_owned()))?;
    validate_bridge_text("IBKR response reason", &reason)?;
    Ok(reason)
}

fn validate_bridge_text(name: &str, value: &str) -> Result<(), PaperError> {
    if value.trim().is_empty() || value.len() > 1_024 || value.contains(['\r', '\n']) {
        return Err(PaperError(format!(
            "{name} must contain 1 to 1024 single-line characters"
        )));
    }
    Ok(())
}

fn bridge_decimal(name: &str, value: &str) -> Result<Decimal, PaperError> {
    Decimal::from_str(value).map_err(|error| PaperError(format!("invalid {name}: {error}")))
}

fn parse_bridge_order_state(value: &str) -> Result<OrderState, PaperError> {
    match value {
        "PENDING_SUBMIT" => Ok(OrderState::PendingSubmit),
        "SUBMITTED" => Ok(OrderState::Submitted),
        "ACKNOWLEDGED" => Ok(OrderState::Acknowledged),
        "PARTIALLY_FILLED" => Ok(OrderState::PartiallyFilled),
        "FILLED" => Ok(OrderState::Filled),
        "PENDING_CANCEL" => Ok(OrderState::PendingCancel),
        "CANCELLED" => Ok(OrderState::Cancelled),
        "REJECTED" => Ok(OrderState::Rejected),
        "EXPIRED" => Ok(OrderState::Expired),
        "UNKNOWN" => Ok(OrderState::Unknown),
        _ => Err(PaperError(
            "IBKR snapshot contains an unknown order state".to_owned(),
        )),
    }
}

/// Concrete normalized adapter around one pinned IBKR TWS/Gateway paper transport.
pub struct IbkrPaperGatewayAdapter<T> {
    configuration: IbkrPaperGatewayConfiguration,
    transport: T,
}

impl<T: IbkrPaperGatewayTransport> IbkrPaperGatewayAdapter<T> {
    /// Creates an adapter only after the endpoint is proved paper-only and local.
    pub fn new(
        configuration: IbkrPaperGatewayConfiguration,
        transport: T,
    ) -> Result<Self, PaperError> {
        configuration.validate()?;
        Ok(Self {
            configuration,
            transport,
        })
    }

    /// Returns the immutable endpoint configuration for health/status projection.
    pub fn configuration(&self) -> &IbkrPaperGatewayConfiguration {
        &self.configuration
    }

    /// Releases the configured vendor transport during controlled shutdown.
    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: IbkrPaperGatewayTransport> PaperBrokerAdapter for IbkrPaperGatewayAdapter<T> {
    fn adapter_configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        if account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper adapter configuration account does not match request".to_owned(),
            ));
        }
        Ok(format!(
            "ibkr-paper-gateway-v1|{}|{}|{}|{}",
            self.configuration.account_id,
            self.configuration.host,
            self.configuration.port,
            self.configuration.environment
        ))
    }

    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        if request.account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper request account does not match configuration".to_owned(),
            ));
        }
        self.transport.submit_paper_order(request)
    }

    fn submit_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        if request.account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper request account does not match configuration".to_owned(),
            ));
        }
        self.transport.submit_paper_combo(request)
    }

    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        if request.account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper cancellation account does not match configuration".to_owned(),
            ));
        }
        self.transport.cancel_paper_order(&request.client_order_id)
    }

    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        if account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper poll account does not match configuration".to_owned(),
            ));
        }
        self.transport.poll_paper_events()
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        if account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper snapshot account does not match configuration".to_owned(),
            ));
        }
        self.transport.paper_account_snapshot(account_id)
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        if account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper reconnect account does not match configuration".to_owned(),
            ));
        }
        self.transport.reconnect_paper()
    }
}

/// Proof that the exact capital-bearing adapter binary belongs to a signed,
/// artifact-verified release. It can be constructed only by
/// [`verify_capital_adapter_release`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCapitalAdapterRelease {
    release_id: String,
    manifest_hash: String,
    adapter_sha256: String,
}

impl VerifiedCapitalAdapterRelease {
    /// Signed release identity.
    pub fn release_id(&self) -> &str {
        &self.release_id
    }

    /// SHA-256 of the canonical signed manifest.
    pub fn manifest_hash(&self) -> &str {
        &self.manifest_hash
    }

    /// SHA-256 of the exact reviewed adapter artifact.
    pub fn adapter_sha256(&self) -> &str {
        &self.adapter_sha256
    }
}

/// Verifies an Ed25519 release signature and every manifest artifact before
/// selecting the exact `adapter.ibkr.live` binary for capital-bearing use.
pub fn verify_capital_adapter_release(
    manifest_source: &str,
    signature: &ReleaseSignature,
    trusted_key: &TrustedReleaseKey,
    artifact_root: &Path,
) -> Result<VerifiedCapitalAdapterRelease, LiveError> {
    let manifest = verify_release_signature(manifest_source, signature, trusted_key)
        .map_err(|error| LiveError(error.to_string()))?;
    verify_release_artifacts(&manifest, artifact_root)
        .map_err(|error| LiveError(error.to_string()))?;
    verified_capital_artifact(&manifest)
}

fn verified_capital_artifact(
    manifest: &ReleaseManifest,
) -> Result<VerifiedCapitalAdapterRelease, LiveError> {
    let artifact = manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == "adapter.ibkr.live")
        .ok_or_else(|| LiveError("signed release does not contain adapter.ibkr.live".to_owned()))?;
    Ok(VerifiedCapitalAdapterRelease {
        release_id: manifest.release_id.clone(),
        manifest_hash: manifest
            .fingerprint()
            .map_err(|error| LiveError(error.to_string()))?,
        adapter_sha256: artifact.sha256.clone(),
    })
}

/// Human review record bound to one already verified adapter artifact.
///
/// The two reviewer identities are evidence inputs; a deployment owner must
/// validate them against its change-management/IAM system before constructing
/// this adapter. This crate never manufactures an approval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapitalAdapterReview {
    /// Canonical review record ID.
    pub review_id: String,
    /// Release identity reviewed.
    pub release_id: String,
    /// Exact signed manifest fingerprint.
    pub manifest_hash: String,
    /// Exact adapter artifact hash.
    pub adapter_sha256: String,
    /// Primary security/code reviewer.
    pub primary_reviewer: String,
    /// Independent operational/risk reviewer.
    pub secondary_reviewer: String,
    /// Canonical UTC review completion time.
    pub reviewed_at: String,
}

impl CapitalAdapterReview {
    fn validate(&self, release: &VerifiedCapitalAdapterRelease) -> Result<(), LiveError> {
        for (name, value) in [
            ("capital adapter review_id", self.review_id.as_str()),
            (
                "capital adapter primary reviewer",
                self.primary_reviewer.as_str(),
            ),
            (
                "capital adapter secondary reviewer",
                self.secondary_reviewer.as_str(),
            ),
        ] {
            validate_canonical_id(name, value).map_err(|error| LiveError(error.to_string()))?;
        }
        validate_utc_timestamp("capital adapter reviewed_at", &self.reviewed_at)
            .map_err(|error| LiveError(error.to_string()))?;
        if self.primary_reviewer == self.secondary_reviewer
            || self.release_id != release.release_id
            || self.manifest_hash != release.manifest_hash
            || self.adapter_sha256 != release.adapter_sha256
        {
            return Err(LiveError(
                "capital adapter review is not independent or does not bind the verified release"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// Explicit, narrowly bounded IBKR LIVE endpoint and canary envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IbkrLiveGatewayConfiguration {
    /// Canonical controlled-LIVE account ID.
    pub account_id: String,
    /// Only a colocated loopback TWS/Gateway is accepted.
    pub host: String,
    /// IBKR LIVE TWS (7496) or Gateway (4001) port.
    pub port: u16,
    /// Must be `LIVE`.
    pub environment: String,
    /// Maximum exact quantity accepted by this edge adapter.
    pub maximum_quantity: Decimal,
    /// Maximum limit-price notional accepted by this edge adapter.
    pub maximum_notional: Decimal,
    /// Explicit instrument canary allow-list.
    pub allowed_instruments: BTreeSet<String>,
}

impl IbkrLiveGatewayConfiguration {
    /// Validates the endpoint and a deliberately narrow canary envelope.
    pub fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("IBKR live account_id", &self.account_id)
            .map_err(|error| LiveError(error.to_string()))?;
        if !matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "::1")
            || !matches!(self.port, 7496 | 4001)
            || self.environment != "LIVE"
            || self.maximum_quantity <= Decimal::ZERO
            || self.maximum_notional <= Decimal::ZERO
            || self.allowed_instruments.is_empty()
        {
            return Err(LiveError(
                "IBKR LIVE adapter requires loopback LIVE port and positive canary limits"
                    .to_owned(),
            ));
        }
        for instrument in &self.allowed_instruments {
            validate_canonical_id("IBKR live allowed instrument", instrument)
                .map_err(|error| LiveError(error.to_string()))?;
        }
        Ok(())
    }
}

/// Vendor transport seam that must be implemented only by the reviewed IBKR
/// API package. Secret bytes are received for connection and must not be stored,
/// logged, or returned.
pub trait IbkrLiveGatewayTransport {
    /// Establishes the capital-bearing broker connection.
    fn connect_live(
        &mut self,
        configuration: &IbkrLiveGatewayConfiguration,
        credential: &[u8],
    ) -> Result<(), LiveError>;
    /// Sends one already approved normalized request idempotently.
    fn submit_live(
        &mut self,
        request: &LiveBrokerOrderRequest,
    ) -> Result<LiveBrokerSubmitResult, LiveError>;
    /// Requests cancellation.
    fn cancel_live(&mut self, client_order_id: &str) -> Result<(), LiveError>;
    /// Requests a price-only replacement.
    fn replace_live(&mut self, request: &LiveBrokerReplaceRequest) -> Result<(), LiveError>;
    /// Drains normalized asynchronous events.
    fn poll_live(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError>;
    /// Retrieves an independent account snapshot.
    fn live_account_snapshot(
        &mut self,
        account_id: &str,
    ) -> Result<LiveBrokerAccountSnapshot, LiveError>;
    /// Best-effort cancellation of every working order for emergency stop.
    fn cancel_all_live(&mut self, account_id: &str) -> Result<(), LiveError>;
    /// Closes the live connection and clears transport-held credentials.
    fn disconnect_live(&mut self);
}

/// Capital-bearing IBKR adapter that remains inert until signed-release and
/// four-eyes review evidence are both supplied.
pub struct IbkrControlledLiveAdapter<T> {
    configuration: IbkrLiveGatewayConfiguration,
    release: VerifiedCapitalAdapterRelease,
    review: CapitalAdapterReview,
    transport: T,
    connected: bool,
    initial_snapshot_observed: bool,
    emergency_stop: bool,
}

impl<T: IbkrLiveGatewayTransport> IbkrControlledLiveAdapter<T> {
    /// Constructs an inert adapter after checking release and review bindings.
    pub fn new(
        configuration: IbkrLiveGatewayConfiguration,
        release: VerifiedCapitalAdapterRelease,
        review: CapitalAdapterReview,
        transport: T,
    ) -> Result<Self, LiveError> {
        configuration.validate()?;
        review.validate(&release)?;
        Ok(Self {
            configuration,
            release,
            review,
            transport,
            connected: false,
            initial_snapshot_observed: false,
            emergency_stop: false,
        })
    }

    /// Returns the bound signed release evidence.
    pub fn verified_release(&self) -> &VerifiedCapitalAdapterRelease {
        &self.release
    }

    /// Returns the externally supplied independent review record.
    pub fn review(&self) -> &CapitalAdapterReview {
        &self.review
    }

    /// Irreversibly stops new submissions for this adapter instance, then
    /// attempts broker-wide cancellation. The stop remains active even when
    /// cancellation transport fails.
    pub fn activate_emergency_stop(&mut self) -> Result<(), LiveError> {
        self.emergency_stop = true;
        self.transport
            .cancel_all_live(&self.configuration.account_id)
    }

    /// Releases the reviewed transport during controlled shutdown.
    pub fn into_transport(mut self) -> T {
        self.transport.disconnect_live();
        self.transport
    }

    fn require_submission_ready(&self, request: &LiveBrokerOrderRequest) -> Result<(), LiveError> {
        if !self.connected || !self.initial_snapshot_observed || self.emergency_stop {
            return Err(LiveError(
                "IBKR LIVE adapter is disconnected, unreconciled, or emergency-stopped".to_owned(),
            ));
        }
        if request.account_id != self.configuration.account_id
            || !self
                .configuration
                .allowed_instruments
                .contains(&request.instrument_id)
            || request.quantity <= Decimal::ZERO
            || request.quantity > self.configuration.maximum_quantity
        {
            return Err(LiveError(
                "IBKR LIVE request exceeds the reviewed canary envelope".to_owned(),
            ));
        }
        let limit_price = request.limit_price.ok_or_else(|| {
            LiveError("IBKR LIVE canary requires a price-protected limit order".to_owned())
        })?;
        let notional = request.quantity.checked_mul(limit_price)?;
        if notional > self.configuration.maximum_notional {
            return Err(LiveError(
                "IBKR LIVE request exceeds maximum canary notional".to_owned(),
            ));
        }
        Ok(())
    }
}

impl<T: IbkrLiveGatewayTransport> LiveBrokerAdapter for IbkrControlledLiveAdapter<T> {
    fn connect(&mut self, account_id: &str, credential: &SecretMaterial) -> Result<(), LiveError> {
        if account_id != self.configuration.account_id || self.emergency_stop {
            return Err(LiveError(
                "IBKR LIVE connection account mismatch or emergency stop".to_owned(),
            ));
        }
        credential.expose_to(|bytes| self.transport.connect_live(&self.configuration, bytes))?;
        self.connected = true;
        self.initial_snapshot_observed = false;
        if self
            .transport
            .live_account_snapshot(&self.configuration.account_id)
            .is_err()
        {
            self.connected = false;
            self.transport.disconnect_live();
            return Err(LiveError(
                "IBKR LIVE initial account snapshot failed".to_owned(),
            ));
        }
        self.initial_snapshot_observed = true;
        Ok(())
    }

    fn submit(
        &mut self,
        request: &LiveBrokerOrderRequest,
    ) -> Result<LiveBrokerSubmitResult, LiveError> {
        self.require_submission_ready(request)?;
        self.transport.submit_live(request)
    }

    fn cancel(&mut self, client_order_id: &str) -> Result<(), LiveError> {
        validate_canonical_id("IBKR live cancel client_order_id", client_order_id)
            .map_err(|error| LiveError(error.to_string()))?;
        if !self.connected {
            return Err(LiveError("IBKR LIVE adapter is disconnected".to_owned()));
        }
        self.transport.cancel_live(client_order_id)
    }

    fn replace(&mut self, request: &LiveBrokerReplaceRequest) -> Result<(), LiveError> {
        if !self.connected || self.emergency_stop {
            return Err(LiveError("IBKR LIVE replacement is disabled".to_owned()));
        }
        self.transport.replace_live(request)
    }

    fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
        if !self.connected {
            return Err(LiveError("IBKR LIVE adapter is disconnected".to_owned()));
        }
        self.transport.poll_live()
    }

    fn snapshot(&mut self, account_id: &str) -> Result<LiveBrokerAccountSnapshot, LiveError> {
        if account_id != self.configuration.account_id || !self.connected {
            return Err(LiveError(
                "IBKR LIVE snapshot account mismatch or disconnected adapter".to_owned(),
            ));
        }
        let snapshot = self.transport.live_account_snapshot(account_id)?;
        self.initial_snapshot_observed = true;
        Ok(snapshot)
    }

    fn reconnect(
        &mut self,
        account_id: &str,
        credential: &SecretMaterial,
    ) -> Result<(), LiveError> {
        self.connected = false;
        self.initial_snapshot_observed = false;
        self.transport.disconnect_live();
        if self.emergency_stop {
            return Err(LiveError(
                "IBKR LIVE adapter cannot reconnect after emergency stop".to_owned(),
            ));
        }
        self.connect(account_id, credential)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use follon_domain::Side;
    use std::process::Command;

    fn python_executable() -> Option<PathBuf> {
        ["python", "python3"].into_iter().find_map(|candidate| {
            Command::new(candidate)
                .args([
                    "-c",
                    "import pathlib,sys;print(pathlib.Path(sys.executable).resolve())",
                ])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|output| PathBuf::from(output.trim()))
                .filter(|path| path.is_absolute() && path.is_file())
        })
    }

    #[test]
    fn paper_configuration_refuses_live_and_nonlocal_endpoints() {
        let configuration = IbkrPaperGatewayConfiguration {
            account_id: "acct.paper.001".to_owned(),
            host: "127.0.0.1".to_owned(),
            port: 7497,
            environment: "PAPER".to_owned(),
        };
        assert!(configuration.validate().is_ok());
        assert!(IbkrPaperGatewayConfiguration {
            environment: "LIVE".to_owned(),
            ..configuration.clone()
        }
        .validate()
        .is_err());
        assert!(IbkrPaperGatewayConfiguration {
            host: "api.ibkr.example".to_owned(),
            ..configuration
        }
        .validate()
        .is_err());
    }

    #[test]
    fn process_transport_round_trips_normalized_paper_evidence() {
        let Some(python) = python_executable() else {
            eprintln!("Python is unavailable; process transport fixture was skipped");
            return;
        };
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/ibkr/fake-paper-bridge.py")
            .canonicalize()
            .expect("fixture path");
        let mut transport =
            IbkrPaperBridgeProcessTransport::start(IbkrPaperBridgeProcessConfiguration {
                executable: python,
                arguments: vec![fixture.to_string_lossy().into_owned()],
                request_timeout: Duration::from_secs(2),
                max_response_bytes: 64 * 1024,
            })
            .expect("transport starts");
        let request = BrokerOrderRequest {
            client_order_id: "order.1".to_owned(),
            account_id: "acct.paper.001".to_owned(),
            instrument_id: "aapl.xnas".to_owned(),
            side: Side::Buy,
            quantity: Decimal::from_str("2").expect("quantity"),
            limit_price: Some(Decimal::from_str("101.25").expect("price")),
        };

        assert_eq!(
            transport.submit_paper_order(&request).expect("submit"),
            BrokerSubmitResult::Acknowledged {
                broker_order_id: "ibkr.41".to_owned(),
            }
        );
        let events = transport.poll_paper_events().expect("events");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], BrokerEvent::Execution { .. }));
        let snapshot = transport
            .paper_account_snapshot("acct.paper.001")
            .expect("snapshot");
        assert_eq!(snapshot.orders.len(), 1);
        assert_eq!(snapshot.positions.len(), 1);
        assert_eq!(snapshot.cash, Decimal::from_str("797.15").expect("cash"));
        transport.cancel_paper_order("order.1").expect("cancel");
        transport.reconnect_paper().expect("reconnect");
    }

    #[test]
    fn malformed_bridge_result_poisons_the_process_session() {
        let Some(python) = python_executable() else {
            eprintln!("Python is unavailable; malformed process fixture was skipped");
            return;
        };
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/ibkr/malformed-paper-bridge.py")
            .canonicalize()
            .expect("fixture path");
        let mut transport =
            IbkrPaperBridgeProcessTransport::start(IbkrPaperBridgeProcessConfiguration {
                executable: python,
                arguments: vec![fixture.to_string_lossy().into_owned()],
                request_timeout: Duration::from_secs(2),
                max_response_bytes: 64 * 1024,
            })
            .expect("transport starts");

        assert!(transport.poll_paper_events().is_err());
        let error = transport
            .cancel_paper_order("order.1")
            .expect_err("poisoned session must reject later work");
        assert!(error.0.contains("unhealthy"));
    }
}

#[cfg(test)]
mod live_adapter_tests {
    use super::*;
    use follon_domain::Side;
    use follon_live::{LiveBrokerOrderSnapshot, LiveBrokerPositionSnapshot};

    #[derive(Default)]
    struct FakeLiveTransport {
        connected: bool,
        submissions: usize,
        cancel_all_calls: usize,
    }

    impl IbkrLiveGatewayTransport for FakeLiveTransport {
        fn connect_live(
            &mut self,
            _configuration: &IbkrLiveGatewayConfiguration,
            credential: &[u8],
        ) -> Result<(), LiveError> {
            if credential != b"managed-secret" {
                return Err(LiveError("credential rejected".to_owned()));
            }
            self.connected = true;
            Ok(())
        }

        fn submit_live(
            &mut self,
            _request: &LiveBrokerOrderRequest,
        ) -> Result<LiveBrokerSubmitResult, LiveError> {
            self.submissions += 1;
            Ok(LiveBrokerSubmitResult::Acknowledged {
                broker_order_id: "ibkr.live.1".to_owned(),
            })
        }

        fn cancel_live(&mut self, _client_order_id: &str) -> Result<(), LiveError> {
            Ok(())
        }

        fn replace_live(&mut self, _request: &LiveBrokerReplaceRequest) -> Result<(), LiveError> {
            Ok(())
        }

        fn poll_live(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
            Ok(Vec::new())
        }

        fn live_account_snapshot(
            &mut self,
            _account_id: &str,
        ) -> Result<LiveBrokerAccountSnapshot, LiveError> {
            if !self.connected {
                return Err(LiveError("not connected".to_owned()));
            }
            Ok(LiveBrokerAccountSnapshot {
                orders: Vec::<LiveBrokerOrderSnapshot>::new(),
                positions: Vec::<LiveBrokerPositionSnapshot>::new(),
                cash: Decimal::from_integer(1_000).unwrap(),
            })
        }

        fn cancel_all_live(&mut self, _account_id: &str) -> Result<(), LiveError> {
            self.cancel_all_calls += 1;
            Ok(())
        }

        fn disconnect_live(&mut self) {
            self.connected = false;
        }
    }

    fn release() -> VerifiedCapitalAdapterRelease {
        VerifiedCapitalAdapterRelease {
            release_id: "release.canary".to_owned(),
            manifest_hash: "a".repeat(64),
            adapter_sha256: "b".repeat(64),
        }
    }

    fn review() -> CapitalAdapterReview {
        CapitalAdapterReview {
            review_id: "review.ibkr-live".to_owned(),
            release_id: "release.canary".to_owned(),
            manifest_hash: "a".repeat(64),
            adapter_sha256: "b".repeat(64),
            primary_reviewer: "user.security".to_owned(),
            secondary_reviewer: "user.risk".to_owned(),
            reviewed_at: "2026-08-24T10:00:00Z".to_owned(),
        }
    }

    fn configuration() -> IbkrLiveGatewayConfiguration {
        IbkrLiveGatewayConfiguration {
            account_id: "acct.live.001".to_owned(),
            host: "127.0.0.1".to_owned(),
            port: 7496,
            environment: "LIVE".to_owned(),
            maximum_quantity: Decimal::from_integer(2).unwrap(),
            maximum_notional: Decimal::from_integer(250).unwrap(),
            allowed_instruments: BTreeSet::from(["aapl.xnas".to_owned()]),
        }
    }

    fn request(quantity: i64, price: Option<i64>) -> LiveBrokerOrderRequest {
        LiveBrokerOrderRequest {
            client_order_id: "order.live.1".to_owned(),
            account_id: "acct.live.001".to_owned(),
            instrument_id: "aapl.xnas".to_owned(),
            side: Side::Buy,
            quantity: Decimal::from_integer(quantity).unwrap(),
            limit_price: price.map(|value| Decimal::from_integer(value).unwrap()),
        }
    }

    #[test]
    fn review_requires_two_distinct_people_and_exact_release_binding() {
        let mut invalid = review();
        invalid.secondary_reviewer = invalid.primary_reviewer.clone();
        assert!(IbkrControlledLiveAdapter::new(
            configuration(),
            release(),
            invalid,
            FakeLiveTransport::default(),
        )
        .is_err());
        let mut invalid = review();
        invalid.adapter_sha256 = "c".repeat(64);
        assert!(IbkrControlledLiveAdapter::new(
            configuration(),
            release(),
            invalid,
            FakeLiveTransport::default(),
        )
        .is_err());
    }

    #[test]
    fn live_adapter_requires_secret_snapshot_limit_price_and_canary_envelope() {
        let mut adapter = IbkrControlledLiveAdapter::new(
            configuration(),
            release(),
            review(),
            FakeLiveTransport::default(),
        )
        .unwrap();
        assert!(adapter.submit(&request(1, Some(100))).is_err());
        let secret = SecretMaterial::new(b"managed-secret".to_vec()).unwrap();
        adapter.connect("acct.live.001", &secret).unwrap();
        assert!(adapter.submit(&request(1, None)).is_err());
        assert!(adapter.submit(&request(3, Some(100))).is_err());
        assert!(matches!(
            adapter.submit(&request(2, Some(100))).unwrap(),
            LiveBrokerSubmitResult::Acknowledged { .. }
        ));
        adapter.activate_emergency_stop().unwrap();
        assert!(adapter.submit(&request(1, Some(100))).is_err());
        let transport = adapter.into_transport();
        assert_eq!(transport.submissions, 1);
        assert_eq!(transport.cancel_all_calls, 1);
    }

    #[test]
    fn live_configuration_rejects_paper_port_remote_host_and_empty_allow_list() {
        let mut invalid = configuration();
        invalid.port = 7497;
        assert!(invalid.validate().is_err());
        invalid = configuration();
        invalid.host = "api.example.com".to_owned();
        assert!(invalid.validate().is_err());
        invalid = configuration();
        invalid.allowed_instruments.clear();
        assert!(invalid.validate().is_err());
    }
}
