//! Checked decimal helpers shared across the risk kernel.

use follon_domain::Decimal;

use crate::*;

pub(crate) fn ratio_bps(numerator: Decimal, denominator: Decimal) -> Result<Decimal, RiskError> {
    if numerator < Decimal::ZERO || denominator <= Decimal::ZERO {
        return Err(RiskError("risk ratio requires valid inputs".to_owned()));
    }
    Ok(numerator
        .checked_mul(Decimal::from_integer(10_000)?)?
        .checked_div(denominator)?)
}

pub(crate) fn absolute(value: Decimal) -> Result<Decimal, RiskError> {
    if value >= Decimal::ZERO {
        Ok(value)
    } else {
        negate(value)
    }
}

pub(crate) fn negate(value: Decimal) -> Result<Decimal, RiskError> {
    value
        .scaled()
        .checked_neg()
        .map(Decimal::from_scaled)
        .ok_or_else(|| RiskError("risk decimal negation overflowed".to_owned()))
}
