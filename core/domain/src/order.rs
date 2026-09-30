//! Order side, type, time-in-force and the strategy `OrderIntent`.

use crate::*;

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

/// Whether a trade moves one instrument's position strictly toward flat,
/// without passing through it: a long position sold down that stays long or
/// reaches zero, or a short position bought back the same way.
///
/// This is the one definition of a risk-reducing trade. A trade that opens a
/// position, adds to one, reverses one through zero, or changes nothing is not
/// one. PAPER and controlled LIVE both use it when equity is not positive and
/// no aggregate ratio can be computed, to let an underwater account close
/// exposure and nothing else (delivery state E7.4b).
pub fn reduces_position(current: Decimal, projected: Decimal) -> bool {
    (current > Decimal::ZERO && projected >= Decimal::ZERO && projected < current)
        || (current < Decimal::ZERO && projected <= Decimal::ZERO && projected > current)
}

/// [`reduces_position`], once the orders already working in the instrument are counted.
///
/// `working_reduction` is the quantity those orders will still take off this position, in the
/// direction of the trade being judged. Two orders that each sell the whole of a long position
/// are each a reduction alone, and filled together they reverse it. So a trade is a reduction
/// only if it and everything already working fit inside the position that is held. Nothing
/// working in the other direction is counted: an order that adds exposure may never fill, and
/// assuming it does would let a reduction be refused for something that may not happen.
pub fn reduces_position_with_working(
    current: Decimal,
    projected: Decimal,
    working_reduction: Decimal,
) -> Result<bool, DecimalError> {
    if !reduces_position(current, projected) {
        return Ok(false);
    }
    let claimed = magnitude(projected.checked_sub(current)?)?.checked_add(working_reduction)?;
    Ok(claimed <= magnitude(current)?)
}

fn magnitude(value: Decimal) -> Result<Decimal, DecimalError> {
    if value < Decimal::ZERO {
        Decimal::ZERO.checked_sub(value)
    } else {
        Ok(value)
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
        validate_utc_timestamp("intent created_at", &self.created_at)?;
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
