//! Atomic option-combination planning.

use follon_domain::{validate_canonical_id, Decimal, Side};

use crate::*;

/// Net-price protection for a synchronized listed-option combination.
///
/// Re-exported from `core/domain` rather than defined here: pre-trade risk
/// assesses a combination's protected net price before any plan exists, so the
/// contract has to sit below the planner. Call sites that imported it from this
/// crate are unaffected.
pub use follon_domain::ComboPriceLimit;

/// One ratio leg in a synchronized option combination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionComboLeg {
    /// Canonical option instrument identity.
    pub instrument_id: String,
    /// Economic side for this leg.
    pub side: Side,
    /// Positive integer contracts per combination unit.
    pub ratio: u32,
    /// Positive protected leg price used to prove the net price.
    pub limit_price: Decimal,
}

/// One adapter-neutral child of a synchronized option combination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComboLegInstruction {
    /// Deterministic child identity.
    pub child_order_id: String,
    /// Canonical option identity.
    pub instrument_id: String,
    /// Leg side.
    pub side: Side,
    /// Exact contract quantity after applying the ratio.
    pub quantity: Decimal,
    /// Protected leg price.
    pub limit_price: Decimal,
}

/// Exact synchronized option-combination plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionComboPlan {
    /// Stable combination identity.
    pub combo_id: String,
    /// Positive number of combination units.
    pub combo_quantity: Decimal,
    /// Signed protected net price: positive is debit, negative is credit.
    pub protected_net_price: Decimal,
    /// Complete set of ratio-conserving legs submitted as one atomic group.
    pub legs: Vec<ComboLegInstruction>,
}

/// Plans a synchronized, net-price-protected listed-option combination.
///
/// This does not degrade a combination into independently marketable legs.
/// An adapter must either support an atomic native combination or reject the
/// plan before any child is transmitted.
pub fn plan_option_combo(
    combo_id: &str,
    combo_quantity: Decimal,
    price_limit: ComboPriceLimit,
    legs: &[OptionComboLeg],
) -> Result<OptionComboPlan, ExecutionError> {
    use std::collections::BTreeSet;

    validate_canonical_id("combo_id", combo_id)?;
    if combo_quantity <= Decimal::ZERO || !(2..=16).contains(&legs.len()) {
        return Err(ExecutionError("invalid option combination".to_owned()));
    }
    let mut instruments = BTreeSet::new();
    let mut protected_net_price = Decimal::ZERO;
    let mut instructions = Vec::with_capacity(legs.len());
    for (index, leg) in legs.iter().enumerate() {
        validate_canonical_id("combo instrument_id", &leg.instrument_id)?;
        if !instruments.insert(leg.instrument_id.as_str())
            || leg.ratio == 0
            || leg.ratio > 10_000
            || leg.limit_price <= Decimal::ZERO
        {
            return Err(ExecutionError("invalid option combination leg".to_owned()));
        }
        let ratio = Decimal::from_integer(i64::from(leg.ratio))?;
        let leg_net = leg.limit_price.checked_mul(ratio)?;
        protected_net_price = match leg.side {
            Side::Buy => protected_net_price.checked_add(leg_net)?,
            Side::Sell => protected_net_price.checked_sub(leg_net)?,
        };
        instructions.push(ComboLegInstruction {
            child_order_id: format!("{combo_id}.leg.{:04}", index + 1),
            instrument_id: leg.instrument_id.clone(),
            side: leg.side,
            quantity: combo_quantity.checked_mul(ratio)?,
            limit_price: leg.limit_price,
        });
    }
    // One definition of the protection check, shared with the risk gate, so a
    // combination cannot pass one and fail the other.
    price_limit
        .check_net_price(protected_net_price)
        .map_err(|error| ExecutionError(error.0))?;
    Ok(OptionComboPlan {
        combo_id: combo_id.to_owned(),
        combo_quantity,
        protected_net_price,
        legs: instructions,
    })
}
