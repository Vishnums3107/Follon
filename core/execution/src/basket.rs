//! Basket (multi-symbol) order planning.

use follon_domain::{validate_canonical_id, Decimal, Side};

use crate::*;

/// One notional-weighted basket leg.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BasketLeg {
    /// Instrument identity.
    pub instrument_id: String,
    /// Leg side.
    pub side: Side,
    /// Weight in basis points; all legs must sum to 10,000.
    pub weight_bps: u32,
    /// Positive reference price for converting notional to quantity.
    pub reference_price: Decimal,
}

/// Converts exact basket notional into independently auditable parent orders.
pub fn plan_basket(
    basket_id: &str,
    account_id: &str,
    total_notional: Decimal,
    legs: &[BasketLeg],
) -> Result<Vec<ParentOrder>, ExecutionError> {
    validate_canonical_id("basket_id", basket_id)?;
    validate_canonical_id("basket account_id", account_id)?;
    if total_notional <= Decimal::ZERO || legs.is_empty() || legs.len() > 1_000 {
        return Err(ExecutionError("invalid basket request".to_owned()));
    }
    let weight_total = legs.iter().try_fold(0_u32, |total, leg| {
        validate_canonical_id("basket instrument_id", &leg.instrument_id)?;
        if leg.weight_bps == 0 || leg.reference_price <= Decimal::ZERO {
            return Err(ExecutionError("invalid basket leg".to_owned()));
        }
        total
            .checked_add(leg.weight_bps)
            .ok_or_else(|| ExecutionError("basket weight overflowed".to_owned()))
    })?;
    if weight_total != 10_000 {
        return Err(ExecutionError(
            "basket weights must sum to 10000 basis points".to_owned(),
        ));
    }
    legs.iter()
        .enumerate()
        .map(|(index, leg)| {
            let leg_notional = total_notional
                .checked_mul(Decimal::from_integer(i64::from(leg.weight_bps))?)?
                .checked_div(Decimal::from_integer(10_000)?)?;
            let parent = ParentOrder {
                parent_order_id: format!("{basket_id}.leg.{:04}", index + 1),
                account_id: account_id.to_owned(),
                instrument_id: leg.instrument_id.clone(),
                side: leg.side,
                quantity: leg_notional.checked_div(leg.reference_price)?,
                limit_price: None,
            };
            parent.validate()?;
            Ok(parent)
        })
        .collect()
}
