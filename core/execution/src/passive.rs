//! Passive order repricing (cancel/replace) planning.

use follon_domain::{price_deviation_bps, Decimal, Side};

use crate::*;

/// Strict bounded policy for a passive cancel-and-replace sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassiveRepricePolicy {
    /// Initial passive working price, distinct from the parent's hard collar.
    pub initial_limit_price: Decimal,
    /// Exact venue tick size used to reject unrouteable prices.
    pub tick_size: Decimal,
    /// Maximum adverse movement from the initial price in basis points.
    pub maximum_chase_bps: u32,
    /// Maximum number of replacement orders after the initial child.
    pub maximum_replacements: u32,
    /// Minimum elapsed schedule time between replacements.
    pub minimum_replace_interval_seconds: u64,
}

/// One deterministic top-of-book observation used by passive repricing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassiveMarketObservation {
    /// Schedule time relative to initial submission.
    pub observed_after_seconds: u64,
    /// Positive best bid.
    pub best_bid: Decimal,
    /// Positive best ask.
    pub best_ask: Decimal,
}

/// Atomic cancel-and-replace instruction; cancel confirmation precedes transmit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancelReplaceInstruction {
    /// Exact working child that must be cancelled and confirmed first.
    pub cancel_child_order_id: String,
    /// Replacement child; it restates the remaining quantity at a new limit.
    pub replacement: ChildInstruction,
}

/// Complete bounded passive-price plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassiveRepricePlan {
    /// Initial price-protected child.
    pub initial: ChildInstruction,
    /// Ordered replacements, each contingent on prior cancel confirmation.
    pub replacements: Vec<CancelReplaceInstruction>,
}

/// Plans a passive, monotonic, strictly collared cancel-and-replace sequence.
///
/// The plan never crosses the observed spread, exceeds the parent hard limit,
/// moves away from the market, or schedules simultaneous live child orders.
pub fn plan_passive_repricing(
    parent: &ParentOrder,
    policy: &PassiveRepricePolicy,
    observations: &[PassiveMarketObservation],
) -> Result<PassiveRepricePlan, ExecutionError> {
    parent.validate()?;
    if policy.initial_limit_price <= Decimal::ZERO
        || policy.tick_size <= Decimal::ZERO
        || policy.maximum_chase_bps > 10_000
        || policy.maximum_replacements > 10_000
        || policy.minimum_replace_interval_seconds == 0
        || policy.initial_limit_price.scaled() % policy.tick_size.scaled() != 0
        || observations.is_empty()
        || observations.len() > 100_000
    {
        return Err(ExecutionError(
            "invalid passive repricing policy".to_owned(),
        ));
    }
    if let Some(hard_limit) = parent.limit_price {
        let outside = match parent.side {
            Side::Buy => policy.initial_limit_price > hard_limit,
            Side::Sell => policy.initial_limit_price < hard_limit,
        };
        if outside {
            return Err(ExecutionError(
                "initial passive price violates the parent limit".to_owned(),
            ));
        }
    }

    let initial = ChildInstruction {
        child_order_id: format!("{}.passive.0000", parent.parent_order_id),
        scheduled_after_seconds: 0,
        venue: None,
        quantity: parent.quantity,
        kind: ChildOrderKind::Limit,
        limit_price: Some(policy.initial_limit_price),
        stop_price: None,
    };
    let mut current_id = initial.child_order_id.clone();
    let mut current_price = policy.initial_limit_price;
    let mut prior_observation = None;
    let mut last_replace_at = 0_u64;
    let mut replacements = Vec::new();
    for observation in observations {
        if observation.best_bid <= Decimal::ZERO
            || observation.best_ask <= observation.best_bid
            || observation.best_bid.scaled() % policy.tick_size.scaled() != 0
            || observation.best_ask.scaled() % policy.tick_size.scaled() != 0
            || (prior_observation.is_none()
                && match parent.side {
                    Side::Buy => policy.initial_limit_price >= observation.best_ask,
                    Side::Sell => policy.initial_limit_price <= observation.best_bid,
                })
            || prior_observation.is_some_and(|prior| observation.observed_after_seconds <= prior)
        {
            return Err(ExecutionError(
                "invalid passive market observation".to_owned(),
            ));
        }
        prior_observation = Some(observation.observed_after_seconds);
        if replacements.len() >= policy.maximum_replacements as usize
            || observation.observed_after_seconds
                < last_replace_at
                    .checked_add(policy.minimum_replace_interval_seconds)
                    .ok_or_else(|| ExecutionError("passive schedule overflowed".to_owned()))?
        {
            continue;
        }

        let mut candidate = match parent.side {
            Side::Buy => observation.best_bid,
            Side::Sell => observation.best_ask,
        };
        if let Some(hard_limit) = parent.limit_price {
            // Clamp to the most aggressive price on the tick grid that still
            // respects the hard limit. An operator-typed limit need not be on
            // the grid, and a venue rejects an off-grid replacement after the
            // working child's cancel has already been confirmed.
            let tick = policy.tick_size.scaled();
            let below = hard_limit.scaled() - hard_limit.scaled().rem_euclid(tick);
            candidate = match parent.side {
                Side::Buy => candidate.min(Decimal::from_scaled(below)),
                Side::Sell => {
                    candidate.max(Decimal::from_scaled(if below == hard_limit.scaled() {
                        below
                    } else {
                        below + tick
                    }))
                }
            };
        }
        let more_aggressive = match parent.side {
            Side::Buy => candidate > current_price && candidate < observation.best_ask,
            Side::Sell => candidate < current_price && candidate > observation.best_bid,
        };
        if !more_aggressive
            || price_deviation_bps(policy.initial_limit_price, candidate)?
                > Decimal::from_integer(i64::from(policy.maximum_chase_bps))?
        {
            continue;
        }
        let replacement_number = replacements.len() + 1;
        let replacement_id = format!("{}.passive.{replacement_number:04}", parent.parent_order_id);
        replacements.push(CancelReplaceInstruction {
            cancel_child_order_id: current_id,
            replacement: ChildInstruction {
                child_order_id: replacement_id.clone(),
                scheduled_after_seconds: observation.observed_after_seconds,
                venue: None,
                quantity: parent.quantity,
                kind: ChildOrderKind::Limit,
                limit_price: Some(candidate),
                stop_price: None,
            },
        });
        current_id = replacement_id;
        current_price = candidate;
        last_replace_at = observation.observed_after_seconds;
    }
    Ok(PassiveRepricePlan {
        initial,
        replacements,
    })
}
