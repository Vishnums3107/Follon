//! Interactive Brokers paper-gateway configuration and normalized adapter contract.
//!
//! The adapter deliberately permits only documented paper ports. It translates
//! no strategy or risk decision: the core submits normalized requests only after
//! its own OMS and risk controls accept them. A TWS/Gateway transport plugs into
//! [`IbkrPaperGatewayTransport`] and is continuously exercised by the core's
//! deterministic in-memory paper model and fault-injection suite.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, OrderState};
use follon_paper::{
    BrokerAccountSnapshot, BrokerEvent, BrokerOrderRequest, BrokerOrderSnapshot,
    BrokerPositionSnapshot, BrokerSubmitResult, PaperBrokerAdapter, PaperError,
};
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
    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        if request.account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper request account does not match configuration".to_owned(),
            ));
        }
        self.transport.submit_paper_order(request)
    }

    fn cancel(&mut self, client_order_id: &str) -> Result<(), PaperError> {
        self.transport.cancel_paper_order(client_order_id)
    }

    fn poll(&mut self) -> Result<Vec<BrokerEvent>, PaperError> {
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

    fn reconnect(&mut self) -> Result<(), PaperError> {
        self.transport.reconnect_paper()
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
