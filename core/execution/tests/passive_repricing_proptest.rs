//! Property tests for passive cancel-and-replace repricing.
//!
//! `plan_passive_repricing` has one hand-written scenario test. These
//! properties check every rule it promises, over random buy and sell quote
//! paths, against arithmetic written here in 1e-8 units:
//!
//! - post-only: a buy replacement rests strictly below the ask it was planned
//!   against, a sell strictly above the bid;
//! - strictly monotonic toward the market, never away from it;
//! - never beyond the parent's hard limit;
//! - never further from the initial price than `maximum_chase_bps`, measured
//!   exactly as the shared `price_deviation_bps` truncates;
//! - at most `maximum_replacements`, each at least
//!   `minimum_replace_interval_seconds` after the previous one (or the start),
//!   scheduled at the observation it priced from;
//! - one live child at a time: each replacement cancels exactly the previous
//!   child, restates the full quantity, and is a limit order;
//! - every price is on the venue tick grid;
//! - complete: no observation that satisfied every rule was skipped.

use follon_domain::{Decimal, Side};
use follon_execution::{
    plan_passive_repricing, ChildOrderKind, ParentOrder, PassiveMarketObservation,
    PassiveRepricePlan, PassiveRepricePolicy,
};
use proptest::prelude::*;

const UNIT: i128 = 100_000_000;

#[derive(Clone, Debug)]
struct Scenario {
    side: Side,
    tick: i128,
    quantity: i128,
    initial: i128,
    hard_limit: Option<i128>,
    chase_bps: u32,
    maximum_replacements: u32,
    interval: u64,
    /// `(observed_after_seconds, best_bid, best_ask)`, strictly increasing in time.
    quotes: Vec<(u64, i128, i128)>,
}

impl Scenario {
    fn parent(&self) -> ParentOrder {
        ParentOrder {
            parent_order_id: "parent.passive".to_owned(),
            account_id: "acct.proptest".to_owned(),
            instrument_id: "inst.us_equity.proptest".to_owned(),
            side: self.side,
            quantity: Decimal::from_scaled(self.quantity),
            limit_price: self.hard_limit.map(Decimal::from_scaled),
        }
    }

    fn policy(&self) -> PassiveRepricePolicy {
        PassiveRepricePolicy {
            initial_limit_price: Decimal::from_scaled(self.initial),
            tick_size: Decimal::from_scaled(self.tick),
            maximum_chase_bps: self.chase_bps,
            maximum_replacements: self.maximum_replacements,
            minimum_replace_interval_seconds: self.interval,
        }
    }

    fn observations(&self) -> Vec<PassiveMarketObservation> {
        self.quotes
            .iter()
            .map(|(at, bid, ask)| PassiveMarketObservation {
                observed_after_seconds: *at,
                best_bid: Decimal::from_scaled(*bid),
                best_ask: Decimal::from_scaled(*ask),
            })
            .collect()
    }

    fn plan(&self) -> Result<PassiveRepricePlan, follon_execution::ExecutionError> {
        plan_passive_repricing(&self.parent(), &self.policy(), &self.observations())
    }

    /// Exactly `price_deviation_bps(initial, price) <= maximum_chase_bps`.
    fn within_chase(&self, price: i128) -> bool {
        let distance = (price - self.initial).abs();
        distance * 10_000 * UNIT / self.initial <= i128::from(self.chase_bps) * UNIT
    }

    /// Replacement prices a correct planner emits, as `(scheduled_at, price)`.
    fn expected(&self) -> Vec<(u64, i128)> {
        let buy = self.side == Side::Buy;
        let mut current = self.initial;
        let mut last_replace_at = 0_u64;
        let mut planned = Vec::new();
        for &(at, bid, ask) in &self.quotes {
            if planned.len() >= self.maximum_replacements as usize
                || at < last_replace_at + self.interval
            {
                continue;
            }
            let mut candidate = if buy { bid } else { ask };
            if let Some(limit) = self.hard_limit {
                // The most aggressive on-grid price inside the hard limit.
                candidate = if buy {
                    candidate.min(limit.div_euclid(self.tick) * self.tick)
                } else {
                    candidate.max((limit + self.tick - 1).div_euclid(self.tick) * self.tick)
                };
            }
            let toward_market = if buy {
                candidate > current && candidate < ask
            } else {
                candidate < current && candidate > bid
            };
            if toward_market && self.within_chase(candidate) {
                planned.push((at, candidate));
                current = candidate;
                last_replace_at = at;
            }
        }
        planned
    }
}

/// Valid buy and sell scenarios. The hard limit is sometimes deliberately
/// off the tick grid, as an operator-typed collar can be.
fn scenario() -> impl Strategy<Value = Scenario> {
    (
        prop_oneof![Just(Side::Buy), Just(Side::Sell)],
        prop_oneof![Just(UNIT / 100), Just(UNIT / 20), Just(UNIT / 4)],
        1i128..=1_000 * UNIT,
        200i128..=20_000,
        prop::collection::vec((-3i128..=3, 1i128..=5, 1u64..=30), 1..=40),
        0i128..=10,
        prop::option::of((0i128..=50, prop_oneof![Just(0i128), 1i128..UNIT / 100])),
        0u32..=500,
        0u32..=20,
        1u64..=60,
    )
        .prop_map(
            |(side, tick, quantity, base, steps, offset, limit, chase_bps, maximum, interval)| {
                let mut bid_ticks = base;
                let mut at = 0_u64;
                let quotes: Vec<(u64, i128, i128)> = steps
                    .iter()
                    .map(|(walk, spread, gap)| {
                        bid_ticks += walk;
                        at += gap;
                        (at, bid_ticks * tick, (bid_ticks + spread) * tick)
                    })
                    .collect();
                let (_, first_bid, first_ask) = quotes[0];
                // The initial child is passive against the first quote.
                let initial = match side {
                    Side::Buy => first_bid - offset * tick,
                    Side::Sell => first_ask + offset * tick,
                };
                let hard_limit = limit.map(|(extra_ticks, off_grid)| match side {
                    Side::Buy => initial + extra_ticks * tick + off_grid,
                    Side::Sell => initial - extra_ticks * tick - off_grid,
                });
                Scenario {
                    side,
                    tick,
                    quantity,
                    initial,
                    hard_limit,
                    chase_bps,
                    maximum_replacements: maximum,
                    interval,
                    quotes,
                }
            },
        )
}

fn price(child: &follon_execution::ChildInstruction) -> i128 {
    child
        .limit_price
        .expect("passive children are limit orders")
        .scaled()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn every_replacement_obeys_every_passive_rule(scenario in scenario()) {
        let plan = scenario.plan().expect("a valid scenario must plan");
        let buy = scenario.side == Side::Buy;
        prop_assert_eq!(&plan.initial.child_order_id, "parent.passive.passive.0000");
        prop_assert_eq!(price(&plan.initial), scenario.initial);
        prop_assert_eq!(plan.initial.quantity.scaled(), scenario.quantity);
        prop_assert!(plan.replacements.len() <= scenario.maximum_replacements as usize);

        let mut previous_id = plan.initial.child_order_id.clone();
        let mut previous_price = scenario.initial;
        let mut previous_at: Option<u64> = None;
        for (index, instruction) in plan.replacements.iter().enumerate() {
            let child = &instruction.replacement;
            let at = child.scheduled_after_seconds;
            let p = price(child);
            let &(_, bid, ask) = scenario
                .quotes
                .iter()
                .find(|(observed, _, _)| *observed == at)
                .expect("a replacement must be scheduled at an observation");

            prop_assert_eq!(&instruction.cancel_child_order_id, &previous_id, "one live child at a time");
            prop_assert_eq!(&child.child_order_id, &format!("parent.passive.passive.{:04}", index + 1));
            prop_assert_eq!(child.quantity.scaled(), scenario.quantity);
            prop_assert_eq!(child.kind, ChildOrderKind::Limit);
            if buy {
                prop_assert!(p < ask, "buy replacement {} crosses ask {}", p, ask);
                prop_assert!(p > previous_price, "buy replacement {} does not improve on {}", p, previous_price);
            } else {
                prop_assert!(p > bid, "sell replacement {} crosses bid {}", p, bid);
                prop_assert!(p < previous_price, "sell replacement {} does not improve on {}", p, previous_price);
            }
            if let Some(limit) = scenario.hard_limit {
                prop_assert!(if buy { p <= limit } else { p >= limit }, "price {} beyond hard limit {}", p, limit);
            }
            prop_assert!(scenario.within_chase(p), "price {} chases beyond {} bps", p, scenario.chase_bps);
            prop_assert!(at >= previous_at.unwrap_or(0) + scenario.interval, "replacement at {} too soon", at);
            prop_assert_eq!(p % scenario.tick, 0, "price {} is off the {} tick grid", p, scenario.tick);

            previous_id = child.child_order_id.clone();
            previous_price = p;
            previous_at = Some(at);
        }
    }

    #[test]
    fn no_eligible_reprice_is_skipped(scenario in scenario()) {
        let plan = scenario.plan().expect("a valid scenario must plan");
        let planned: Vec<(u64, i128)> = plan
            .replacements
            .iter()
            .map(|instruction| (instruction.replacement.scheduled_after_seconds, price(&instruction.replacement)))
            .collect();
        prop_assert_eq!(planned, scenario.expected());
    }

    #[test]
    fn malformed_inputs_are_refused(scenario in scenario(), which in 0usize..6) {
        let mut broken = scenario.clone();
        match which {
            // Initial price off the tick grid.
            0 => broken.initial += 1,
            // Initial price already marketable against the first quote.
            1 => {
                let (_, bid, ask) = broken.quotes[0];
                broken.initial = if broken.side == Side::Buy { ask } else { bid };
            }
            // Observation times that do not strictly increase.
            2 => {
                let first = broken.quotes[0];
                broken.quotes.insert(1, first);
            }
            // A locked or crossed quote.
            3 => {
                let last = broken.quotes.len() - 1;
                broken.quotes[last].2 = broken.quotes[last].1;
            }
            // Initial price beyond the parent's hard limit.
            4 => {
                broken.hard_limit = Some(if broken.side == Side::Buy {
                    broken.initial - broken.tick
                } else {
                    broken.initial + broken.tick
                });
            }
            // Zero replace interval.
            _ => broken.interval = 0,
        }
        prop_assume!(broken.initial > 0 && broken.hard_limit.is_none_or(|limit| limit > 0));
        prop_assert!(broken.plan().is_err(), "malformed input {} was planned", which);
    }
}
