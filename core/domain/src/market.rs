//! Normalized market-data bars.

use crate::*;

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
        if self.interval_seconds == 0
            || self.open <= Decimal::ZERO
            || self.high <= Decimal::ZERO
            || self.low <= Decimal::ZERO
            || self.close <= Decimal::ZERO
            || self.volume < Decimal::ZERO
        {
            return Err(DomainError(
                "bar prices and interval must be positive and volume cannot be negative".to_owned(),
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
