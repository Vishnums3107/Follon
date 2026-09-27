//! Property tests for algo-wheel allocation.
//!
//! `plan_algo_wheel_execution` ends with `ExecutionPlan::validate_against`,
//! which proves only that children plus unallocated quantity conserve the
//! parent and that offsets never decrease. The wheel's own job is more than
//! that, and these properties check it against an oracle written here:
//!
//! - the parent is split by weight exactly: every branch but the last gets the
//!   floor of `quantity * weight / 10000` in 1e-8 units and the last absorbs
//!   the remainder, and a branch whose share rounds to zero is refused;
//! - each branch is planned with its own algorithm and the parent's limit;
//! - children merge by offset, then branch order, then position within the
//!   branch, and are renumbered `<parent>.child.0001` onward;
//! - the unallocated quantity is the sum over branches;
//! - a single full-weight branch is transparent: identical to planning its
//!   algorithm directly, apart from the renumbering;
//! - weights that are zero or do not sum to exactly 10000, nested wheels and
//!   empty wheels are refused.
//!
//! The sub-algorithms themselves are covered by
//! `scheduling_legality_proptest.rs`, so the oracle may call `plan_execution`
//! for a branch.

use follon_domain::{Decimal, Side};
use follon_execution::{
    plan_execution, AlgoWheelAllocation, ExecutionAlgorithm, ExecutionPlan, ParentOrder,
};
use proptest::prelude::*;

const UNIT: i128 = 100_000_000;

fn parent(quantity_scaled: i128, limit: Option<i128>) -> ParentOrder {
    ParentOrder {
        parent_order_id: "parent.wheel".to_owned(),
        account_id: "acct.proptest".to_owned(),
        instrument_id: "inst.us_equity.proptest".to_owned(),
        side: Side::Buy,
        quantity: Decimal::from_scaled(quantity_scaled),
        limit_price: limit.map(Decimal::from_scaled),
    }
}

fn wheel(allocations: &[(ExecutionAlgorithm, u32)]) -> ExecutionAlgorithm {
    ExecutionAlgorithm::AlgoWheel {
        allocations: allocations
            .iter()
            .map(|(algorithm, weight_bps)| AlgoWheelAllocation {
                algorithm: algorithm.clone(),
                weight_bps: *weight_bps,
            })
            .collect(),
    }
}

/// The plan an algo wheel must produce, built without the wheel planner.
/// `None` means the wheel must refuse.
fn oracle(
    parent: &ParentOrder,
    allocations: &[(ExecutionAlgorithm, u32)],
) -> Option<ExecutionPlan> {
    let quantity = parent.quantity.scaled();
    let mut shares = Vec::with_capacity(allocations.len());
    let mut allocated = 0_i128;
    for (index, (_, weight)) in allocations.iter().enumerate() {
        let share = if index + 1 == allocations.len() {
            quantity - allocated
        } else {
            quantity * i128::from(*weight) / 10_000
        };
        if share <= 0 {
            return None;
        }
        allocated += share;
        shares.push(share);
    }

    let mut merged = Vec::new();
    let mut unallocated = 0_i128;
    for (branch, ((algorithm, _), share)) in allocations.iter().zip(shares).enumerate() {
        let branch_parent = ParentOrder {
            parent_order_id: format!("oracle.branch.{branch}"),
            quantity: Decimal::from_scaled(share),
            ..parent.clone()
        };
        let plan = plan_execution(&branch_parent, algorithm).ok()?;
        unallocated += plan.unallocated_quantity.scaled();
        for (position, child) in plan.children.into_iter().enumerate() {
            merged.push(((child.scheduled_after_seconds, branch, position), child));
        }
    }
    merged.sort_by_key(|(key, _)| *key);
    Some(ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "algo-wheel-v1".to_owned(),
        children: merged
            .into_iter()
            .enumerate()
            .map(|(index, (_, mut child))| {
                child.child_order_id = format!("{}.child.{:04}", parent.parent_order_id, index + 1);
                child
            })
            .collect(),
        unallocated_quantity: Decimal::from_scaled(unallocated),
    })
}

fn quantity() -> impl Strategy<Value = i128> {
    prop_oneof![
        // Tiny quantities, where a branch's share can round to zero.
        1i128..=50,
        // Whole and fractional quantities up to 10,000.
        1i128..=10_000 * UNIT,
    ]
}

fn limit() -> impl Strategy<Value = Option<i128>> {
    prop::option::of(1i128..=500 * UNIT)
}

fn interval() -> impl Strategy<Value = u64> {
    // Few distinct intervals, so branches often schedule children at the same
    // offset and the tie-break is exercised.
    prop_oneof![Just(30u64), Just(60u64), 1u64..=600]
}

fn volumes(minimum: i64) -> impl Strategy<Value = Vec<Decimal>> {
    prop::collection::vec(minimum..=1_000, 1..=8).prop_map(|volumes| {
        volumes
            .into_iter()
            .map(|volume| Decimal::from_integer(volume).unwrap())
            .collect()
    })
}

fn sub_algorithm() -> impl Strategy<Value = ExecutionAlgorithm> {
    prop_oneof![
        Just(ExecutionAlgorithm::Immediate),
        (1u32..=12, interval()).prop_map(|(slice_count, interval_seconds)| {
            ExecutionAlgorithm::Twap {
                slice_count,
                interval_seconds,
            }
        }),
        (volumes(1), interval()).prop_map(|(forecast_market_volumes, interval_seconds)| {
            ExecutionAlgorithm::Vwap {
                forecast_market_volumes,
                interval_seconds,
            }
        }),
        (1u32..=10_000, volumes(0), interval()).prop_map(
            |(participation_bps, observed_market_volumes, interval_seconds)| {
                ExecutionAlgorithm::Participation {
                    participation_bps,
                    observed_market_volumes,
                    interval_seconds,
                }
            }
        ),
        (1u32..=12, interval(), 0u32..=10_000).prop_map(
            |(slice_count, interval_seconds, urgency_bps)| ExecutionAlgorithm::ArrivalPrice {
                slice_count,
                interval_seconds,
                urgency_bps,
            }
        ),
        (100i128 * UNIT..=5_000 * UNIT, interval()).prop_map(|(display, interval_seconds)| {
            ExecutionAlgorithm::Iceberg {
                display_quantity: Decimal::from_scaled(display),
                interval_seconds,
            }
        }),
    ]
}

/// One to eight positive weights summing to exactly 10000.
fn weights() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::btree_set(1u32..10_000, 0..=7).prop_map(|cuts| {
        let mut bounds = vec![0];
        bounds.extend(cuts);
        bounds.push(10_000);
        bounds.windows(2).map(|pair| pair[1] - pair[0]).collect()
    })
}

fn allocations() -> impl Strategy<Value = Vec<(ExecutionAlgorithm, u32)>> {
    weights().prop_flat_map(|weights| {
        let count = weights.len();
        prop::collection::vec(sub_algorithm(), count)
            .prop_map(move |algorithms| algorithms.into_iter().zip(weights.clone()).collect())
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn a_wheel_plan_is_exactly_the_merged_weighted_branch_plans(
        q in quantity(), limit in limit(), allocations in allocations(),
    ) {
        let parent = parent(q, limit);
        let planned = plan_execution(&parent, &wheel(&allocations)).ok();
        let expected = oracle(&parent, &allocations);
        prop_assert_eq!(
            planned.is_some(),
            expected.is_some(),
            "wheel and oracle disagree about refusing {:?}",
            allocations
        );
        if let (Some(planned), Some(expected)) = (planned, expected) {
            prop_assert_eq!(planned, expected);
        }
    }

    #[test]
    fn a_single_full_weight_branch_is_transparent(
        q in quantity(), limit in limit(), algorithm in sub_algorithm(),
    ) {
        let parent = parent(q, limit);
        let direct = plan_execution(&parent, &algorithm);
        let wheeled = plan_execution(&parent, &wheel(&[(algorithm, 10_000)]));
        prop_assert_eq!(direct.is_ok(), wheeled.is_ok());
        if let (Ok(direct), Ok(wheeled)) = (direct, wheeled) {
            prop_assert_eq!(wheeled.algorithm.as_str(), "algo-wheel-v1");
            prop_assert_eq!(wheeled.unallocated_quantity, direct.unallocated_quantity);
            prop_assert_eq!(wheeled.children.len(), direct.children.len());
            for (index, (wheeled, direct)) in wheeled.children.iter().zip(&direct.children).enumerate() {
                prop_assert_eq!(&wheeled.child_order_id, &format!("parent.wheel.child.{:04}", index + 1));
                prop_assert_eq!(wheeled.scheduled_after_seconds, direct.scheduled_after_seconds);
                prop_assert_eq!(wheeled.quantity, direct.quantity);
                prop_assert_eq!(wheeled.kind, direct.kind);
                prop_assert_eq!(wheeled.limit_price, direct.limit_price);
                prop_assert_eq!(wheeled.stop_price, direct.stop_price);
                prop_assert_eq!(&wheeled.venue, &direct.venue);
            }
        }
    }

    #[test]
    fn weights_that_do_not_sum_to_exactly_10000_are_refused(
        q in 1i128..=10_000 * UNIT, allocations in allocations(), delta in -500i64..=500,
    ) {
        prop_assume!(delta != 0);
        let mut skewed = allocations;
        let last = skewed.len() - 1;
        let adjusted = i64::from(skewed[last].1) + delta;
        prop_assume!((1..=10_000).contains(&adjusted));
        skewed[last].1 = adjusted as u32;
        prop_assert!(plan_execution(&parent(q, None), &wheel(&skewed)).is_err());
    }

    #[test]
    fn nested_empty_and_zero_weight_wheels_are_refused(
        q in 1i128..=10_000 * UNIT, allocations in allocations(), position in 0usize..8,
    ) {
        let parent = parent(q, None);
        let index = position % allocations.len();

        let mut nested = allocations.clone();
        nested[index].0 = wheel(&[(ExecutionAlgorithm::Immediate, 10_000)]);
        prop_assert!(plan_execution(&parent, &wheel(&nested)).is_err(), "a nested wheel was accepted");

        let mut zero = allocations.clone();
        zero.push((ExecutionAlgorithm::Immediate, 0));
        prop_assert!(plan_execution(&parent, &wheel(&zero)).is_err(), "a zero weight was accepted");

        prop_assert!(plan_execution(&parent, &wheel(&[])).is_err(), "an empty wheel was accepted");
    }
}
