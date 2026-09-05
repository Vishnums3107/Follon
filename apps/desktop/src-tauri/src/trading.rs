//! Privileged desktop commands for declarative trading requests.
//!
//! This module is deliberately a command boundary, not a broker adapter. The
//! configured gateway must route every validated request through Risk and OMS
//! before any broker-facing operation occurs.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const MAX_RATIONALE_LENGTH: usize = 1_024;

/// A user- or strategy-originated request to trade.
///
/// Amounts stay as fixed-point decimal strings at the IPC boundary. This avoids
/// silently converting user input through binary floating point before the
/// domain contract validates it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrderIntent {
    /// Idempotency identity supplied by the caller.
    pub intent_id: String,
    /// Account to which the request applies.
    pub account_id: String,
    /// Originator identity, such as `desktop.manual`.
    pub strategy_id: String,
    /// Canonical instrument identity, never a display ticker.
    pub instrument_id: String,
    /// Causal-chain identity for the request.
    pub correlation_id: String,
    /// Requested buy or sell direction.
    pub side: OrderSide,
    /// Positive fixed-point quantity with at most eight decimal places.
    pub quantity: String,
    /// Requested order kind.
    pub order_type: OrderType,
    /// Required for a limit order and forbidden for a market order.
    pub limit_price: Option<String>,
    /// Requested lifetime for the order.
    pub time_in_force: TimeInForce,
    /// Human-readable operator rationale or signal reference.
    pub rationale: String,
    /// Canonical, second-precision UTC creation time.
    pub created_at: String,
    /// Immutable strategy or operator workflow version.
    pub strategy_version: String,
    /// Immutable risk/configuration version selected by the caller.
    pub configuration_version: String,
    /// Requested execution environment.
    pub environment: ExecutionEnvironment,
    /// Optional parent intent for a bracket or basket.
    pub parent_intent_id: Option<String>,
}

impl OrderIntent {
    /// Validates the IPC representation before a gateway receives it.
    pub fn validate(&self) -> Result<(), TradingCommandError> {
        for (name, value) in [
            ("intent_id", self.intent_id.as_str()),
            ("account_id", self.account_id.as_str()),
            ("strategy_id", self.strategy_id.as_str()),
            ("instrument_id", self.instrument_id.as_str()),
            ("correlation_id", self.correlation_id.as_str()),
            ("strategy_version", self.strategy_version.as_str()),
            ("configuration_version", self.configuration_version.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if let Some(parent_intent_id) = &self.parent_intent_id {
            validate_canonical_id("parent_intent_id", parent_intent_id)?;
        }
        validate_positive_decimal("quantity", &self.quantity)?;
        match (&self.order_type, &self.limit_price) {
            (OrderType::Limit, Some(limit_price)) => {
                validate_positive_decimal("limit_price", limit_price)?;
            }
            (OrderType::Limit, None) => {
                return Err(TradingCommandError::validation(
                    "a LIMIT order requires limit_price",
                ));
            }
            (OrderType::Market, Some(_)) => {
                return Err(TradingCommandError::validation(
                    "a MARKET order must not include limit_price",
                ));
            }
            (OrderType::Market, None) => {}
        }
        if self.rationale.trim().is_empty() || self.rationale.len() > MAX_RATIONALE_LENGTH {
            return Err(TradingCommandError::validation(
                "rationale must be non-empty and at most 1024 characters",
            ));
        }
        if !is_canonical_utc_second(&self.created_at) {
            return Err(TradingCommandError::validation(
                "created_at must be canonical second-precision UTC",
            ));
        }
        require_paper_environment(&self.environment)?;
        Ok(())
    }
}

/// Requested order direction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum OrderSide {
    /// Buy the instrument.
    #[serde(rename = "BUY", alias = "Buy", alias = "buy")]
    Buy,
    /// Sell the instrument.
    #[serde(rename = "SELL", alias = "Sell", alias = "sell")]
    Sell,
}

/// Requested order type.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum OrderType {
    /// Execute according to the configured market-order policy.
    #[serde(rename = "MARKET", alias = "Market", alias = "market")]
    Market,
    /// Execute only at the requested protected price or better.
    #[serde(rename = "LIMIT", alias = "Limit", alias = "limit")]
    Limit,
}

/// Requested time in force.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TimeInForce {
    /// Valid for the current session.
    #[serde(rename = "DAY", alias = "Day", alias = "day")]
    Day,
    /// Valid until an explicit cancellation.
    #[serde(rename = "GTC", alias = "Gtc", alias = "gtc")]
    GoodTilCancelled,
}

/// Requested execution environment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExecutionEnvironment {
    /// Deterministic simulation only.
    #[serde(rename = "SIMULATION", alias = "Simulation", alias = "simulation")]
    Simulation,
    /// Broker paper-trading environment.
    #[serde(rename = "PAPER", alias = "Paper", alias = "paper")]
    Paper,
    /// Controlled live-trading environment.
    #[serde(rename = "LIVE", alias = "Live", alias = "live")]
    Live,
}

/// A declarative request to cancel one OMS order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelOrderIntent {
    /// Idempotency identity for the cancellation request.
    pub request_id: String,
    /// Account that owns the OMS order.
    pub account_id: String,
    /// OMS order identity, not a broker-native identifier.
    pub order_id: String,
    /// Causal-chain identity for the request.
    pub correlation_id: String,
    /// Environment in which the order exists.
    pub environment: ExecutionEnvironment,
}

impl CancelOrderIntent {
    fn validate(&self) -> Result<(), TradingCommandError> {
        for (name, value) in [
            ("request_id", self.request_id.as_str()),
            ("account_id", self.account_id.as_str()),
            ("order_id", self.order_id.as_str()),
            ("correlation_id", self.correlation_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        require_paper_environment(&self.environment)?;
        Ok(())
    }
}

/// A declarative request to close one account position through Risk/OMS.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClosePositionIntent {
    /// Idempotency identity for the close request.
    pub request_id: String,
    /// Account containing the position.
    pub account_id: String,
    /// Canonical instrument identity of the position to close.
    pub instrument_id: String,
    /// Causal-chain identity for the request.
    pub correlation_id: String,
    /// Environment in which the position exists.
    pub environment: ExecutionEnvironment,
    /// Human-readable reason for the requested close.
    pub rationale: String,
}

impl ClosePositionIntent {
    fn validate(&self) -> Result<(), TradingCommandError> {
        for (name, value) in [
            ("request_id", self.request_id.as_str()),
            ("account_id", self.account_id.as_str()),
            ("instrument_id", self.instrument_id.as_str()),
            ("correlation_id", self.correlation_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.rationale.trim().is_empty() || self.rationale.len() > MAX_RATIONALE_LENGTH {
            return Err(TradingCommandError::validation(
                "rationale must be non-empty and at most 1024 characters",
            ));
        }
        require_paper_environment(&self.environment)?;
        Ok(())
    }
}

/// The result returned only by a configured Risk/OMS route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReceipt {
    /// Command kind that created this receipt.
    pub command: TradingCommandKind,
    /// Idempotency identity supplied in the request.
    pub request_id: String,
    /// The furthest authoritative state reached by the route.
    pub status: CommandStatus,
    /// OMS order identity when the route created or referenced one.
    pub order_id: Option<String>,
    /// User-displayable route outcome.
    pub message: String,
}

/// Supported native command kinds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradingCommandKind {
    /// Submit a new order intent.
    SubmitOrder,
    /// Cancel an OMS order.
    CancelOrder,
    /// Close an account position.
    ClosePosition,
}

/// Authoritative status returned by a Risk/OMS gateway.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandStatus {
    /// The route durably accepted the request for risk evaluation.
    AcceptedForRisk,
    /// Risk rejected the request before OMS order creation.
    RiskRejected,
    /// OMS accepted a broker-submission request.
    PendingSubmit,
    /// OMS accepted a cancellation request.
    PendingCancel,
    /// OMS accepted a position-close request.
    PendingPositionClose,
}

/// The only interface through which desktop commands can reach trading state.
pub trait RiskOmsGateway: Send + Sync {
    /// Routes a fully validated order intent through Risk and OMS.
    fn submit_order(&self, intent: OrderIntent) -> CommandResult;
    /// Routes a fully validated cancellation request through OMS.
    fn cancel_order(&self, intent: CancelOrderIntent) -> CommandResult;
    /// Routes a fully validated position-close request through Risk and OMS.
    fn close_position(&self, intent: ClosePositionIntent) -> CommandResult;
}

/// Application state made available to the three Tauri commands.
#[derive(Clone)]
pub struct TradingCommandState {
    gateway: Arc<dyn RiskOmsGateway>,
    route_available: bool,
}

impl TradingCommandState {
    /// Creates a state object with a concrete application Risk/OMS gateway.
    pub fn with_gateway(gateway: Arc<dyn RiskOmsGateway>) -> Self {
        Self {
            gateway,
            route_available: true,
        }
    }

    /// Creates a state object which rejects commands until the application
    /// supplies a Risk/OMS route.
    pub fn unavailable() -> Self {
        Self {
            gateway: Arc::new(UnavailableGateway),
            route_available: false,
        }
    }

    fn gateway(&self) -> &dyn RiskOmsGateway {
        self.gateway.as_ref()
    }

    fn route_status(&self) -> TradingCommandRouteStatus {
        TradingCommandRouteStatus {
            route_available: self.route_available,
            message: if self.route_available {
                "A native PAPER Risk/OMS command route is configured.".to_owned()
            } else {
                "The desktop Risk/OMS command route is not configured; no trading action can be sent."
                    .to_owned()
            },
        }
    }
}

/// Read-only command-route capability advertised to the desktop UI.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradingCommandRouteStatus {
    /// Whether the native host has a concrete PAPER Risk/OMS command route.
    pub route_available: bool,
    /// A safe, user-displayable explanation of the current boundary.
    pub message: String,
}

struct UnavailableGateway;

impl RiskOmsGateway for UnavailableGateway {
    fn submit_order(&self, _: OrderIntent) -> CommandResult {
        Err(TradingCommandError::RouteUnavailable)
    }

    fn cancel_order(&self, _: CancelOrderIntent) -> CommandResult {
        Err(TradingCommandError::RouteUnavailable)
    }

    fn close_position(&self, _: ClosePositionIntent) -> CommandResult {
        Err(TradingCommandError::RouteUnavailable)
    }
}

/// Submit an order intent through the configured Risk/OMS gateway.
#[tauri::command]
pub fn submit_order(
    state: State<'_, TradingCommandState>,
    intent: OrderIntent,
) -> Result<CommandReceipt, String> {
    submit_order_through(state.gateway(), intent).map_err(|error| error.to_string())
}

/// Cancel an OMS order through the configured gateway.
#[tauri::command]
pub fn cancel_order(
    state: State<'_, TradingCommandState>,
    intent: CancelOrderIntent,
) -> Result<CommandReceipt, String> {
    cancel_order_through(state.gateway(), intent).map_err(|error| error.to_string())
}

/// Request a risk-governed close of one account position.
#[tauri::command]
pub fn close_position(
    state: State<'_, TradingCommandState>,
    intent: ClosePositionIntent,
) -> Result<CommandReceipt, String> {
    close_position_through(state.gateway(), intent).map_err(|error| error.to_string())
}

/// Return the native host's read-only Risk/OMS command capability.
#[tauri::command]
pub fn trading_command_status(
    state: State<'_, TradingCommandState>,
) -> TradingCommandRouteStatus {
    state.route_status()
}

fn submit_order_through(gateway: &dyn RiskOmsGateway, intent: OrderIntent) -> CommandResult {
    intent.validate()?;
    gateway.submit_order(intent)
}

fn cancel_order_through(gateway: &dyn RiskOmsGateway, intent: CancelOrderIntent) -> CommandResult {
    intent.validate()?;
    gateway.cancel_order(intent)
}

fn close_position_through(
    gateway: &dyn RiskOmsGateway,
    intent: ClosePositionIntent,
) -> CommandResult {
    intent.validate()?;
    gateway.close_position(intent)
}

/// A command-boundary error suitable for returning to the desktop UI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TradingCommandError {
    /// The IPC payload cannot become a valid domain request.
    Validation(String),
    /// The desktop host has no application Risk/OMS route configured.
    RouteUnavailable,
    /// This checked-in desktop boundary accepts PAPER requests only.
    EnvironmentUnavailable,
}

impl TradingCommandError {
    fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

impl fmt::Display for TradingCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => write!(formatter, "invalid trading command: {message}"),
            Self::RouteUnavailable => formatter.write_str(
                "the desktop Risk/OMS command route is not configured; no trading action was sent",
            ),
            Self::EnvironmentUnavailable => formatter.write_str(
                "this desktop command boundary accepts PAPER requests only; no trading action was sent",
            ),
        }
    }
}

impl std::error::Error for TradingCommandError {}

/// Result type used by the route and command helpers.
pub type CommandResult = Result<CommandReceipt, TradingCommandError>;

fn validate_canonical_id(name: &str, value: &str) -> Result<(), TradingCommandError> {
    if value.is_empty()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(TradingCommandError::validation(format!(
            "{name} must be a non-empty canonical ID"
        )));
    }
    Ok(())
}

fn require_paper_environment(environment: &ExecutionEnvironment) -> Result<(), TradingCommandError> {
    if *environment == ExecutionEnvironment::Paper {
        Ok(())
    } else {
        Err(TradingCommandError::EnvironmentUnavailable)
    }
}

fn validate_positive_decimal(name: &str, value: &str) -> Result<(), TradingCommandError> {
    let value = value.trim();
    let mut pieces = value.split('.');
    let whole = pieces.next().unwrap_or_default();
    let fraction = pieces.next();
    if pieces.next().is_some()
        || (whole.is_empty() && fraction.unwrap_or_default().is_empty())
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|part| {
            part.is_empty() || part.len() > 8 || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
        || !value
            .bytes()
            .any(|byte| byte.is_ascii_digit() && byte != b'0')
    {
        return Err(TradingCommandError::validation(format!(
            "{name} must be a positive fixed-point decimal with at most eight decimal places"
        )));
    }
    Ok(())
}

fn is_canonical_utc_second(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes
            .iter()
            .enumerate()
            .filter(|(index, _)| !matches!(index, 4 | 7 | 10 | 13 | 16 | 19))
            .all(|(_, byte)| byte.is_ascii_digit())
        && OffsetDateTime::parse(value, &Rfc3339).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingGateway {
        requests: Mutex<Vec<String>>,
    }

    impl RiskOmsGateway for RecordingGateway {
        fn submit_order(&self, intent: OrderIntent) -> CommandResult {
            self.requests.lock().unwrap().push(intent.intent_id.clone());
            Ok(receipt(
                TradingCommandKind::SubmitOrder,
                intent.intent_id,
                CommandStatus::AcceptedForRisk,
            ))
        }

        fn cancel_order(&self, intent: CancelOrderIntent) -> CommandResult {
            self.requests.lock().unwrap().push(intent.order_id.clone());
            Ok(receipt(
                TradingCommandKind::CancelOrder,
                intent.request_id,
                CommandStatus::PendingCancel,
            ))
        }

        fn close_position(&self, intent: ClosePositionIntent) -> CommandResult {
            self.requests
                .lock()
                .unwrap()
                .push(intent.instrument_id.clone());
            Ok(receipt(
                TradingCommandKind::ClosePosition,
                intent.request_id,
                CommandStatus::PendingPositionClose,
            ))
        }
    }

    fn receipt(
        command: TradingCommandKind,
        request_id: String,
        status: CommandStatus,
    ) -> CommandReceipt {
        CommandReceipt {
            command,
            request_id,
            status,
            order_id: None,
            message: "routed".to_owned(),
        }
    }

    fn valid_intent() -> OrderIntent {
        OrderIntent {
            intent_id: "intent.desktop.1".to_owned(),
            account_id: "account.primary".to_owned(),
            strategy_id: "desktop.manual".to_owned(),
            instrument_id: "inst.us_equity.aapl".to_owned(),
            correlation_id: "correlation.desktop.1".to_owned(),
            side: OrderSide::Buy,
            quantity: "10".to_owned(),
            order_type: OrderType::Limit,
            limit_price: Some("123.45000000".to_owned()),
            time_in_force: TimeInForce::Day,
            rationale: "operator entry".to_owned(),
            created_at: "2026-09-03T12:30:00Z".to_owned(),
            strategy_version: "desktop.v1".to_owned(),
            configuration_version: "risk.v1".to_owned(),
            environment: ExecutionEnvironment::Paper,
            parent_intent_id: None,
        }
    }

    #[test]
    fn valid_order_intent_routes_to_the_gateway() {
        let gateway = RecordingGateway::default();

        let result = submit_order_through(&gateway, valid_intent()).unwrap();

        assert_eq!(result.status, CommandStatus::AcceptedForRisk);
        assert_eq!(
            gateway.requests.lock().unwrap().as_slice(),
            ["intent.desktop.1"]
        );
    }

    #[test]
    fn invalid_order_intent_never_reaches_the_gateway() {
        let gateway = RecordingGateway::default();
        let mut intent = valid_intent();
        intent.limit_price = None;

        let error = submit_order_through(&gateway, intent).unwrap_err();

        assert_eq!(
            error,
            TradingCommandError::Validation("a LIMIT order requires limit_price".to_owned())
        );
        assert!(gateway.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn market_order_cannot_include_a_limit_price() {
        let mut intent = valid_intent();
        intent.order_type = OrderType::Market;

        assert!(intent.validate().is_err());
    }

    #[test]
    fn impossible_calendar_time_is_rejected() {
        let mut intent = valid_intent();
        intent.created_at = "2026-02-31T12:30:00Z".to_owned();

        assert!(intent.validate().is_err());
    }

    #[test]
    fn unavailable_route_never_returns_a_submission_receipt() {
        let error = submit_order_through(&UnavailableGateway, valid_intent()).unwrap_err();

        assert_eq!(error, TradingCommandError::RouteUnavailable);
    }

    #[test]
    fn route_status_distinguishes_an_unconfigured_host_from_a_gateway_host() {
        let unavailable = TradingCommandState::unavailable().route_status();
        assert!(!unavailable.route_available);
        assert!(unavailable.message.contains("not configured"));

        let available = TradingCommandState::with_gateway(Arc::new(RecordingGateway::default())).route_status();
        assert!(available.route_available);
        assert!(available.message.contains("native PAPER Risk/OMS"));
    }

    #[test]
    fn cancel_and_close_requests_require_routeable_identity() {
        let gateway = RecordingGateway::default();
        let cancel = CancelOrderIntent {
            request_id: "request.cancel.1".to_owned(),
            account_id: "account.primary".to_owned(),
            order_id: "order.1".to_owned(),
            correlation_id: "correlation.cancel.1".to_owned(),
            environment: ExecutionEnvironment::Paper,
        };
        let close = ClosePositionIntent {
            request_id: "request.close.1".to_owned(),
            account_id: "account.primary".to_owned(),
            instrument_id: "inst.us_equity.aapl".to_owned(),
            correlation_id: "correlation.close.1".to_owned(),
            environment: ExecutionEnvironment::Paper,
            rationale: "operator close".to_owned(),
        };

        assert_eq!(
            cancel_order_through(&gateway, cancel).unwrap().status,
            CommandStatus::PendingCancel
        );
        assert_eq!(
            close_position_through(&gateway, close).unwrap().status,
            CommandStatus::PendingPositionClose
        );
    }

    #[test]
    fn non_paper_commands_never_reach_a_configured_gateway() {
        let gateway = RecordingGateway::default();
        let mut order = valid_intent();
        order.environment = ExecutionEnvironment::Live;
        assert_eq!(
            submit_order_through(&gateway, order).unwrap_err(),
            TradingCommandError::EnvironmentUnavailable
        );

        let cancel = CancelOrderIntent {
            request_id: "request.cancel.live.1".to_owned(),
            account_id: "account.primary".to_owned(),
            order_id: "order.1".to_owned(),
            correlation_id: "correlation.cancel.live.1".to_owned(),
            environment: ExecutionEnvironment::Live,
        };
        assert_eq!(
            cancel_order_through(&gateway, cancel).unwrap_err(),
            TradingCommandError::EnvironmentUnavailable
        );

        let close = ClosePositionIntent {
            request_id: "request.close.simulation.1".to_owned(),
            account_id: "account.primary".to_owned(),
            instrument_id: "inst.us_equity.aapl".to_owned(),
            correlation_id: "correlation.close.simulation.1".to_owned(),
            environment: ExecutionEnvironment::Simulation,
            rationale: "operator close".to_owned(),
        };
        assert_eq!(
            close_position_through(&gateway, close).unwrap_err(),
            TradingCommandError::EnvironmentUnavailable
        );
        assert!(gateway.requests.lock().unwrap().is_empty());
    }
}
