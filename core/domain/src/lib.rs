//! Stable, framework-independent contracts for the Follon trading kernel.
//!
//! Values that affect accounting are represented as fixed-point [`Decimal`]s;
//! broker and UI concerns intentionally do not appear in this crate.

use std::fmt;
use std::str::FromStr;

/// Fixed decimal precision used by quantities and monetary values.
pub const DECIMAL_SCALE: i128 = 100_000_000;

/// An exact decimal represented as a signed integer scaled by [`DECIMAL_SCALE`].
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Decimal(i128);

/// An error returned when a decimal cannot be represented exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalError(pub String);

impl fmt::Display for DecimalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DecimalError {}

impl Decimal {
    /// The additive identity.
    pub const ZERO: Self = Self(0);

    /// Creates a value from its exact scaled representation.
    pub const fn from_scaled(value: i128) -> Self {
        Self(value)
    }

    /// Creates an exact whole-number value.
    pub fn from_integer(value: i64) -> Result<Self, DecimalError> {
        let scaled = i128::from(value)
            .checked_mul(DECIMAL_SCALE)
            .ok_or_else(|| DecimalError("integer decimal overflow".to_owned()))?;
        Ok(Self(scaled))
    }

    /// Returns the scaled representation for storage and comparison.
    pub const fn scaled(self) -> i128 {
        self.0
    }

    /// Adds two decimals without losing precision.
    pub fn checked_add(self, other: Self) -> Result<Self, DecimalError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or_else(|| DecimalError("decimal addition overflow".to_owned()))
    }

    /// Subtracts two decimals without losing precision.
    pub fn checked_sub(self, other: Self) -> Result<Self, DecimalError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or_else(|| DecimalError("decimal subtraction overflow".to_owned()))
    }

    /// Multiplies two decimals, retaining the configured fixed precision.
    pub fn checked_mul(self, other: Self) -> Result<Self, DecimalError> {
        self.0
            .checked_mul(other.0)
            .and_then(|value| value.checked_div(DECIMAL_SCALE))
            .map(Self)
            .ok_or_else(|| DecimalError("decimal multiplication overflow".to_owned()))
    }

    /// Divides two decimals, retaining the configured fixed precision.
    pub fn checked_div(self, other: Self) -> Result<Self, DecimalError> {
        if other.0 == 0 {
            return Err(DecimalError("division by zero".to_owned()));
        }
        self.0
            .checked_mul(DECIMAL_SCALE)
            .and_then(|value| value.checked_div(other.0))
            .map(Self)
            .ok_or_else(|| DecimalError("decimal division overflow".to_owned()))
    }
}

impl FromStr for Decimal {
    type Err = DecimalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let source = value.trim();
        if source.is_empty() {
            return Err(DecimalError("decimal is empty".to_owned()));
        }

        let (negative, unsigned) = match source.as_bytes()[0] {
            b'-' => (true, &source[1..]),
            b'+' => (false, &source[1..]),
            _ => (false, source),
        };
        if unsigned.is_empty() || unsigned.matches('.').count() > 1 {
            return Err(DecimalError(format!("invalid decimal: {source}")));
        }

        let mut parts = unsigned.split('.');
        let whole_text = parts.next().unwrap_or("0");
        let fraction_text = parts.next().unwrap_or("");
        if whole_text.is_empty() && fraction_text.is_empty()
            || !whole_text.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction_text.bytes().all(|byte| byte.is_ascii_digit())
            || fraction_text.len() > 8
        {
            return Err(DecimalError(format!("invalid decimal: {source}")));
        }

        let whole = if whole_text.is_empty() {
            0
        } else {
            whole_text
                .parse::<i128>()
                .map_err(|_| DecimalError(format!("decimal overflow: {source}")))?
        };
        let fraction = if fraction_text.is_empty() {
            0
        } else {
            let mut padded = fraction_text.to_owned();
            while padded.len() < 8 {
                padded.push('0');
            }
            padded
                .parse::<i128>()
                .map_err(|_| DecimalError(format!("decimal overflow: {source}")))?
        };
        let scaled = whole
            .checked_mul(DECIMAL_SCALE)
            .and_then(|whole_scaled| whole_scaled.checked_add(fraction))
            .ok_or_else(|| DecimalError(format!("decimal overflow: {source}")))?;
        Ok(Self(if negative { -scaled } else { scaled }))
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        if value < 0 {
            formatter.write_str("-")?;
        }
        let magnitude = value.unsigned_abs();
        write!(
            formatter,
            "{}.{:08}",
            magnitude / DECIMAL_SCALE as u128,
            magnitude % DECIMAL_SCALE as u128
        )
    }
}

/// Validates one canonical identifier used as a durable domain key.
pub fn validate_canonical_id(name: &str, value: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(DomainError(format!(
            "{name} must be a non-empty canonical ID"
        )));
    }
    Ok(())
}

/// A violation of an accepted Follon domain contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainError(pub String);

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DomainError {}

/// Side of an order intent or execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    /// Buy the instrument.
    Buy,
    /// Sell the instrument.
    Sell,
}

impl Side {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

/// Supported first-slice order types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderType {
    /// Execute at the deterministic simulated market price.
    Market,
    /// Execute only at a specified price or better.
    Limit,
}

impl OrderType {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Market => "MARKET",
            Self::Limit => "LIMIT",
        }
    }
}

/// Time-in-force for an order intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeInForce {
    /// Good for the current trading day.
    Day,
    /// Good until explicitly cancelled.
    GoodTilCancelled,
}

impl TimeInForce {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "DAY",
            Self::GoodTilCancelled => "GTC",
        }
    }
}

/// A normalized historical OHLCV bar with its exchange-local context retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bar {
    /// Canonical instrument identifier.
    pub instrument_id: String,
    /// Bar open price.
    pub open: Decimal,
    /// Highest traded price.
    pub high: Decimal,
    /// Lowest traded price.
    pub low: Decimal,
    /// Bar close price.
    pub close: Decimal,
    /// Exact traded volume.
    pub volume: Decimal,
    /// Bar interval in seconds.
    pub interval_seconds: u32,
    /// IANA exchange timezone, such as `America/New_York`.
    pub exchange_timezone: String,
}

impl Bar {
    /// Validates OHLC relationships and canonical identity before ingress.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_canonical_id("instrument_id", &self.instrument_id)?;
        if self.interval_seconds == 0 || self.volume < Decimal::ZERO {
            return Err(DomainError(
                "bar interval and volume must be positive".to_owned(),
            ));
        }
        if self.high < self.low
            || self.open < self.low
            || self.open > self.high
            || self.close < self.low
            || self.close > self.high
        {
            return Err(DomainError("bar OHLC values are inconsistent".to_owned()));
        }
        if self.exchange_timezone.is_empty() {
            return Err(DomainError("bar exchange timezone is required".to_owned()));
        }
        Ok(())
    }
}

/// A strategy request to trade. It is not a broker order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderIntent {
    /// Intent identity.
    pub intent_id: String,
    /// Target account identity.
    pub account_id: String,
    /// Originating strategy identity.
    pub strategy_id: String,
    /// Target canonical instrument identity.
    pub instrument_id: String,
    /// Correlates the resulting causal chain.
    pub correlation_id: String,
    /// Requested direction.
    pub side: Side,
    /// Requested exact quantity.
    pub quantity: Decimal,
    /// Requested order type.
    pub order_type: OrderType,
    /// Optional limit price for a limit order.
    pub limit_price: Option<Decimal>,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Human-readable strategy rationale or signal reference.
    pub rationale: String,
    /// UTC creation time supplied by the replay clock.
    pub created_at: String,
    /// Immutable strategy-bundle version.
    pub strategy_version: String,
    /// Immutable configuration version.
    pub configuration_version: String,
    /// Requested execution environment, initially `SIMULATION`.
    pub environment: String,
}

impl OrderIntent {
    /// Validates fields required before risk can assess the intent.
    pub fn validate(&self) -> Result<(), DomainError> {
        for (name, value) in [
            ("intent_id", self.intent_id.as_str()),
            ("account_id", self.account_id.as_str()),
            ("strategy_id", self.strategy_id.as_str()),
            ("instrument_id", self.instrument_id.as_str()),
            ("correlation_id", self.correlation_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.quantity <= Decimal::ZERO || self.rationale.is_empty() || self.created_at.is_empty()
        {
            return Err(DomainError(
                "intent quantity, rationale, and creation time are required".to_owned(),
            ));
        }
        if matches!(self.order_type, OrderType::Limit) && self.limit_price.is_none()
            || matches!(self.order_type, OrderType::Market) && self.limit_price.is_some()
        {
            return Err(DomainError(
                "intent limit price does not match order type".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Result of a versioned pre-trade risk policy evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskDecision {
    /// Decision identity.
    pub decision_id: String,
    /// Intent evaluated by this decision.
    pub intent_id: String,
    /// Whether the intent may create an executable order.
    pub approved: bool,
    /// Machine-readable rule outcomes.
    pub reason_codes: Vec<String>,
    /// Stable policy identity and version.
    pub policy_version: String,
    /// UTC replay-clock decision time.
    pub decided_at: String,
    /// Workflow correlation identity.
    pub correlation_id: String,
    /// Identity of the evaluating actor.
    pub actor: String,
    /// Deterministic limit values used during the decision.
    pub evaluated_limits: String,
}

/// Complete OMS lifecycle state set, including the safety `UNKNOWN` state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderState {
    /// Order was created from an approved intent.
    Created,
    /// Awaiting risk assessment.
    PendingRisk,
    /// Risk rejected the intent.
    RiskRejected,
    /// Risk approved the intent.
    Approved,
    /// Waiting for adapter or simulator submission.
    PendingSubmit,
    /// Submission was sent.
    Submitted,
    /// External system acknowledged submission.
    Acknowledged,
    /// A portion of the quantity executed.
    PartiallyFilled,
    /// All requested quantity executed.
    Filled,
    /// Cancellation is in progress.
    PendingCancel,
    /// Cancellation completed.
    Cancelled,
    /// External system rejected the order.
    Rejected,
    /// Time in force elapsed.
    Expired,
    /// Submission outcome is ambiguous and needs reconciliation.
    Unknown,
}

impl OrderState {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::PendingRisk => "PENDING_RISK",
            Self::RiskRejected => "RISK_REJECTED",
            Self::Approved => "APPROVED",
            Self::PendingSubmit => "PENDING_SUBMIT",
            Self::Submitted => "SUBMITTED",
            Self::Acknowledged => "ACKNOWLEDGED",
            Self::PartiallyFilled => "PARTIALLY_FILLED",
            Self::Filled => "FILLED",
            Self::PendingCancel => "PENDING_CANCEL",
            Self::Cancelled => "CANCELLED",
            Self::Rejected => "REJECTED",
            Self::Expired => "EXPIRED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// A validated OMS state transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderStateChange {
    /// OMS order identity.
    pub order_id: String,
    /// State before the transition, if an order is newly created.
    pub previous_state: Option<OrderState>,
    /// New lifecycle state.
    pub new_state: OrderState,
    /// Reason for the state change.
    pub reason: String,
}

/// A normalized simulator or broker execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fill {
    /// Execution identity, unique per fill.
    pub execution_id: String,
    /// OMS order identity.
    pub order_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Executed direction.
    pub side: Side,
    /// Exact executed quantity.
    pub quantity: Decimal,
    /// Exact execution price.
    pub price: Decimal,
    /// Exact commission/fees in the position currency.
    pub fee: Decimal,
    /// UTC execution time.
    pub executed_at: String,
}

/// Rebuildable portfolio position projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionSnapshot {
    /// Account identity.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Signed position quantity.
    pub quantity: Decimal,
    /// Exact average cost.
    pub average_cost: Decimal,
    /// Exact realized P&L.
    pub realized_pnl: Decimal,
}

/// Rebuildable current P&L projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PnlSnapshot {
    /// Account identity.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Mark used for the valuation.
    pub mark_price: Decimal,
    /// Exact realized P&L.
    pub realized_pnl: Decimal,
    /// Exact unrealized P&L.
    pub unrealized_pnl: Decimal,
    /// Exact total P&L.
    pub total_pnl: Decimal,
}

/// Evidence linking a completed workflow to its primary events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditTrail {
    /// Workflow correlation identity.
    pub correlation_id: String,
    /// Stable ordered event IDs in the trail.
    pub event_ids: Vec<String>,
    /// Operator-readable statement of the completed transition.
    pub summary: String,
}

/// First-slice event families supported by the stable envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventPayload {
    /// A normalized historical bar.
    MarketBar(Bar),
    /// A canonical trading request.
    OrderIntent(OrderIntent),
    /// A risk approval or rejection.
    RiskDecision(RiskDecision),
    /// An OMS lifecycle transition.
    OrderState(OrderStateChange),
    /// A simulator execution.
    Fill(Fill),
    /// A position projection.
    Position(PositionSnapshot),
    /// A P&L projection.
    Pnl(PnlSnapshot),
    /// An immutable audit trail summary.
    Audit(AuditTrail),
}

impl EventPayload {
    /// Namespaced event type with an explicit compatibility version.
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::MarketBar(_) => "market.bar.v1",
            Self::OrderIntent(_) => "intent.created.v1",
            Self::RiskDecision(_) => "risk.decision.v1",
            Self::OrderState(_) => "order.state_changed.v1",
            Self::Fill(_) => "execution.fill.v1",
            Self::Position(_) => "portfolio.position_updated.v1",
            Self::Pnl(_) => "portfolio.pnl_updated.v1",
            Self::Audit(_) => "audit.trail.v1",
        }
    }

    fn canonical_json(&self) -> String {
        match self {
            Self::MarketBar(bar) => format!(
                "{{\"close\":\"{}\",\"exchange_timezone\":{},\"high\":\"{}\",\"instrument_id\":{},\"interval_seconds\":{},\"low\":\"{}\",\"open\":\"{}\",\"volume\":\"{}\"}}",
                bar.close, json_string(&bar.exchange_timezone), bar.high, json_string(&bar.instrument_id), bar.interval_seconds, bar.low, bar.open, bar.volume
            ),
            Self::OrderIntent(intent) => format!(
                "{{\"account_id\":{},\"configuration_version\":{},\"correlation_id\":{},\"created_at\":{},\"environment\":{},\"instrument_id\":{},\"intent_id\":{},\"limit_price\":{},\"order_type\":{},\"quantity\":\"{}\",\"rationale\":{},\"side\":{},\"strategy_id\":{},\"strategy_version\":{},\"time_in_force\":{}}}",
                json_string(&intent.account_id), json_string(&intent.configuration_version), json_string(&intent.correlation_id), json_string(&intent.created_at), json_string(&intent.environment), json_string(&intent.instrument_id), json_string(&intent.intent_id), option_decimal_json(intent.limit_price), json_string(intent.order_type.as_str()), intent.quantity, json_string(&intent.rationale), json_string(intent.side.as_str()), json_string(&intent.strategy_id), json_string(&intent.strategy_version), json_string(intent.time_in_force.as_str())
            ),
            Self::RiskDecision(decision) => format!(
                "{{\"actor\":{},\"approved\":{},\"correlation_id\":{},\"decided_at\":{},\"decision_id\":{},\"evaluated_limits\":{},\"intent_id\":{},\"policy_version\":{},\"reason_codes\":{},\"source\":\"risk\"}}",
                json_string(&decision.actor), decision.approved, json_string(&decision.correlation_id), json_string(&decision.decided_at), json_string(&decision.decision_id), json_string(&decision.evaluated_limits), json_string(&decision.intent_id), json_string(&decision.policy_version), json_strings(&decision.reason_codes)
            ),
            Self::OrderState(change) => format!(
                "{{\"new_state\":{},\"order_id\":{},\"previous_state\":{},\"reason\":{}}}",
                json_string(change.new_state.as_str()), json_string(&change.order_id), change.previous_state.map(|state| json_string(state.as_str())).unwrap_or_else(|| "null".to_owned()), json_string(&change.reason)
            ),
            Self::Fill(fill) => format!(
                "{{\"executed_at\":{},\"execution_id\":{},\"fee\":\"{}\",\"instrument_id\":{},\"order_id\":{},\"price\":\"{}\",\"quantity\":\"{}\",\"side\":{}}}",
                json_string(&fill.executed_at), json_string(&fill.execution_id), fill.fee, json_string(&fill.instrument_id), json_string(&fill.order_id), fill.price, fill.quantity, json_string(fill.side.as_str())
            ),
            Self::Position(position) => format!(
                "{{\"account_id\":{},\"average_cost\":\"{}\",\"instrument_id\":{},\"quantity\":\"{}\",\"realized_pnl\":\"{}\"}}",
                json_string(&position.account_id), position.average_cost, json_string(&position.instrument_id), position.quantity, position.realized_pnl
            ),
            Self::Pnl(pnl) => format!(
                "{{\"account_id\":{},\"instrument_id\":{},\"mark_price\":\"{}\",\"realized_pnl\":\"{}\",\"total_pnl\":\"{}\",\"unrealized_pnl\":\"{}\"}}",
                json_string(&pnl.account_id), json_string(&pnl.instrument_id), pnl.mark_price, pnl.realized_pnl, pnl.total_pnl, pnl.unrealized_pnl
            ),
            Self::Audit(audit) => format!(
                "{{\"correlation_id\":{},\"event_ids\":{},\"summary\":{}}}",
                json_string(&audit.correlation_id), json_strings(&audit.event_ids), json_string(&audit.summary)
            ),
        }
    }
}

/// Immutable, append-only envelope around every significant trading event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventEnvelope {
    /// Globally unique immutable event identity.
    pub event_id: String,
    /// Namespaced semantic event name.
    pub event_type: String,
    /// Payload schema version.
    pub schema_version: u32,
    /// Source or logical event time in UTC.
    pub event_time: String,
    /// Local receipt or generation time in UTC.
    pub receive_time: String,
    /// Account when applicable; absence is explicit.
    pub account_id: Option<String>,
    /// Strategy when applicable; absence is explicit.
    pub strategy_id: Option<String>,
    /// Instrument when applicable; absence is explicit.
    pub instrument_id: Option<String>,
    /// Causal workflow identity.
    pub correlation_id: String,
    /// Direct cause event identity, if any.
    pub causation_id: Option<String>,
    /// Actor responsible for this event.
    pub actor: String,
    /// Source subsystem or provider.
    pub source: String,
    /// Validated event-specific payload.
    pub payload: EventPayload,
    /// Immutable engine build version.
    pub software_version: String,
    /// Immutable configuration version.
    pub configuration_version: String,
}

impl EventEnvelope {
    /// Validates stable envelope fields and payload compatibility at ingress.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_canonical_id("event_id", &self.event_id)?;
        validate_canonical_id("correlation_id", &self.correlation_id)?;
        if self.event_type != self.payload.event_type()
            || self.schema_version == 0
            || self.event_time.is_empty()
            || self.receive_time.is_empty()
            || self.actor.is_empty()
            || self.source.is_empty()
            || self.software_version.is_empty()
            || self.configuration_version.is_empty()
        {
            return Err(DomainError(
                "event envelope is missing required or compatible fields".to_owned(),
            ));
        }
        for (name, value) in [
            ("account_id", self.account_id.as_deref()),
            ("strategy_id", self.strategy_id.as_deref()),
            ("instrument_id", self.instrument_id.as_deref()),
        ] {
            if let Some(value) = value {
                validate_canonical_id(name, value)?;
            }
        }
        Ok(())
    }

    /// Produces stable JSON for persistence, replay comparisons, and tests.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"account_id\":{},\"actor\":{},\"causation_id\":{},\"configuration_version\":{},\"correlation_id\":{},\"event_id\":{},\"event_time\":{},\"event_type\":{},\"instrument_id\":{},\"payload\":{},\"receive_time\":{},\"schema_version\":{},\"software_version\":{},\"source\":{},\"strategy_id\":{}}}",
            option_string_json(&self.account_id), json_string(&self.actor), option_string_json(&self.causation_id), json_string(&self.configuration_version), json_string(&self.correlation_id), json_string(&self.event_id), json_string(&self.event_time), json_string(&self.event_type), option_string_json(&self.instrument_id), self.payload.canonical_json(), json_string(&self.receive_time), self.schema_version, json_string(&self.software_version), json_string(&self.source), option_string_json(&self.strategy_id)
        )
    }
}

fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use fmt::Write;
                write!(&mut escaped, "\\u{:04x}", character as u32)
                    .expect("string formatting cannot fail");
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn option_string_json(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(json_string)
        .unwrap_or_else(|| "null".to_owned())
}

fn option_decimal_json(value: Option<Decimal>) -> String {
    value
        .map(|decimal| format!("\"{decimal}\""))
        .unwrap_or_else(|| "null".to_owned())
}

fn json_strings(values: &[String]) -> String {
    let mut json = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str(&json_string(value));
    }
    json.push(']');
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_is_exact_and_stably_rendered() {
        let value = Decimal::from_str("12.5").unwrap();
        assert_eq!(value.to_string(), "12.50000000");
        assert_eq!(
            value
                .checked_mul(Decimal::from_str("2.0").unwrap())
                .unwrap()
                .to_string(),
            "25.00000000"
        );
    }

    #[test]
    fn identifiers_reject_display_symbols_and_whitespace() {
        assert!(validate_canonical_id("instrument_id", "inst.us_equity.spy").is_ok());
        assert!(validate_canonical_id("instrument_id", "SPY").is_err());
        assert!(validate_canonical_id("instrument_id", "inst spy").is_err());
    }

    #[test]
    fn envelope_serialization_is_stable() {
        let envelope = EventEnvelope {
            event_id: "evt-000001".to_owned(),
            event_type: "market.bar.v1".to_owned(),
            schema_version: 1,
            event_time: "2026-01-02T14:30:00Z".to_owned(),
            receive_time: "2026-01-02T14:30:00Z".to_owned(),
            account_id: None,
            strategy_id: None,
            instrument_id: Some("inst.us_equity.spy".to_owned()),
            correlation_id: "corr-market-000001".to_owned(),
            causation_id: None,
            actor: "market_data".to_owned(),
            source: "historical_import".to_owned(),
            payload: EventPayload::MarketBar(Bar {
                instrument_id: "inst.us_equity.spy".to_owned(),
                open: "100".parse().unwrap(),
                high: "101".parse().unwrap(),
                low: "99".parse().unwrap(),
                close: "100.5".parse().unwrap(),
                volume: "10".parse().unwrap(),
                interval_seconds: 60,
                exchange_timezone: "America/New_York".to_owned(),
            }),
            software_version: "dev".to_owned(),
            configuration_version: "cfg-1".to_owned(),
        };
        envelope.validate().unwrap();
        assert_eq!(envelope.canonical_json(), envelope.canonical_json());
        assert!(envelope
            .canonical_json()
            .contains("\"event_type\":\"market.bar.v1\""));
    }
}
