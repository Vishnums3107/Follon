//! Property tests for EMS scheduling legality.
//!
//! Every planner already calls `ExecutionPlan::validate_against`, which proves
//! quantity conservation and non-decreasing offsets. That self-check cannot see
//! whether an algorithm does its *own* job, so these properties check what it
//! cannot, against integer oracles written here in 1e-8 units:
//!
//! - TWAP: `min(slices, units)` children that differ by at most one unit, the
//!   larger ones first;
//! - VWAP: every child but the last is exactly the floor of its proportional
//!   share, the last absorbs the remainder, and a window that would round to
//!   zero is refused rather than silently dropped;
//! - participation: each child is exactly `min(cap, remaining)`, never above its
//!   window's cap, and only genuinely unavailable liquidity is left unallocated;
//! - arrival price: non-increasing children (front-loaded), equal at zero urgency;
//! - iceberg: every child is the display size except a final, smaller remainder;
//! - every algorithm: offsets are exactly `index * interval`, child identities
//!   are unique, and the parent limit (hence the child kind) is inherited.

use follon_domain::{Decimal, Side};
use follon_execution::{
    plan_execution, plan_iceberg_execution, ChildOrderKind, ExecutionAlgorithm, ExecutionPlan,
    ParentOrder,
};
use proptest::prelude::*;
use std::collections::BTreeSet;

const UNIT: i128 = 100_000_000;

fn parent(quantity_scaled: i128, limit: Option<i128>) -> ParentOrder {
    ParentOrder {
        parent_order_id: "parent.proptest".to_owned(),
        account_id: "acct.proptest".to_owned(),
        instrument_id: "inst.us_equity.proptest".to_owned(),
        side: Side::Buy,
        quantity: Decimal::from_scaled(quantity_scaled),
        limit_price: limit.map(Decimal::from_scaled),
    }
}

fn scaled(plan: &ExecutionPlan) -> Vec<i128> {
    plan.children
        .iter()
        .map(|child| child.quantity.scaled())
        .collect()
}

/// Properties every algorithm must satisfy beyond `validate_against`.
fn assert_common(
    plan: &ExecutionPlan,
    parent: &ParentOrder,
    interval: u64,
) -> Result<(), TestCaseError> {
    let mut ids = BTreeSet::new();
    let kind = if parent.limit_price.is_some() {
        ChildOrderKind::Limit
    } else {
        ChildOrderKind::Market
    };
    for (index, child) in plan.children.iter().enumerate() {
        prop_assert!(
            ids.insert(child.child_order_id.clone()),
            "duplicate child id"
        );
        prop_assert_eq!(child.scheduled_after_seconds, index as u64 * interval);
        prop_assert_eq!(child.limit_price, parent.limit_price);
        prop_assert_eq!(child.kind, kind);
        prop_assert!(child.quantity > Decimal::ZERO);
    }
    let total: i128 = scaled(plan).iter().sum::<i128>() + plan.unallocated_quantity.scaled();
    prop_assert_eq!(total, parent.quantity.scaled());
    Ok(())
}

fn quantity() -> impl Strategy<Value = i128> {
    prop_oneof![
        // Tiny quantities, where slices outnumber available units.
        1i128..=50,
        // Whole and fractional quantities up to 10,000.
        1i128..=10_000 * UNIT,
    ]
}

fn limit() -> impl Strategy<Value = Option<i128>> {
    prop::option::of(1i128..=500 * UNIT)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn immediate_is_one_child_for_the_whole_parent(q in quantity(), limit in limit()) {
        let parent = parent(q, limit);
        let plan = plan_execution(&parent, &ExecutionAlgorithm::Immediate).unwrap();
        assert_common(&plan, &parent, 0)?;
        prop_assert_eq!(scaled(&plan), vec![q]);
    }

    #[test]
    fn twap_slices_are_equal_to_within_one_unit(
        q in quantity(), limit in limit(), slices in 1u32..=200, interval in 1u64..=600,
    ) {
        let parent = parent(q, limit);
        let plan = plan_execution(&parent, &ExecutionAlgorithm::Twap { slice_count: slices, interval_seconds: interval }).unwrap();
        assert_common(&plan, &parent, interval)?;
        let children = scaled(&plan);
        prop_assert_eq!(children.len() as i128, i128::from(slices).min(q));
        prop_assert_eq!(plan.unallocated_quantity, Decimal::ZERO);
        let (min, max) = (*children.iter().min().unwrap(), *children.iter().max().unwrap());
        prop_assert!(max - min <= 1, "TWAP slices {} and {} differ by more than one unit", min, max);
        prop_assert!(children.windows(2).all(|pair| pair[0] >= pair[1]), "larger slices must come first");
    }

    #[test]
    fn vwap_children_are_the_floor_of_their_proportional_share(
        q in quantity(), limit in limit(),
        volumes in prop::collection::vec(1i64..=1_000_000, 1..=40), interval in 1u64..=600,
    ) {
        let parent = parent(q, limit);
        let forecast: Vec<Decimal> = volumes.iter().map(|v| Decimal::from_integer(*v).unwrap()).collect();
        let result = plan_execution(&parent, &ExecutionAlgorithm::Vwap { forecast_market_volumes: forecast, interval_seconds: interval });
        let total: i128 = volumes.iter().map(|v| i128::from(*v)).sum();
        let floors: Vec<i128> = volumes.iter().map(|v| q * i128::from(*v) / total).collect();
        let last = q - floors[..floors.len() - 1].iter().sum::<i128>();
        let representable = floors[..floors.len() - 1].iter().all(|f| *f > 0) && last > 0;
        match result {
            Ok(plan) => {
                prop_assert!(representable, "a window that rounds to zero must be refused");
                assert_common(&plan, &parent, interval)?;
                let children = scaled(&plan);
                prop_assert_eq!(&children[..children.len() - 1], &floors[..floors.len() - 1]);
                prop_assert_eq!(*children.last().unwrap(), last);
            }
            Err(_) => prop_assert!(!representable, "a representable VWAP schedule was refused"),
        }
    }

    #[test]
    fn participation_never_exceeds_a_windows_cap(
        q in quantity(), limit in limit(), bps in 1u32..=10_000,
        volumes in prop::collection::vec(0i64..=100_000, 1..=40), interval in 1u64..=600,
    ) {
        let parent = parent(q, limit);
        let observed: Vec<Decimal> = volumes.iter().map(|v| Decimal::from_integer(*v).unwrap()).collect();
        let plan = plan_execution(&parent, &ExecutionAlgorithm::Participation {
            participation_bps: bps, observed_market_volumes: observed, interval_seconds: interval,
        }).unwrap();
        assert_common(&plan, &parent, interval)?;
        // Oracle: greedy, window by window, capped at volume * bps / 10000.
        let mut remaining = q;
        let mut expected = Vec::new();
        for volume in &volumes {
            if remaining == 0 { break; }
            let cap = i128::from(*volume) * UNIT * i128::from(bps) / 10_000;
            let take = cap.min(remaining);
            if take > 0 { expected.push(take); remaining -= take; }
        }
        prop_assert_eq!(scaled(&plan), expected);
        prop_assert_eq!(plan.unallocated_quantity.scaled(), remaining);
    }

    #[test]
    fn arrival_price_front_loads_and_is_flat_at_zero_urgency(
        q in quantity(), limit in limit(), slices in 1u32..=100, interval in 1u64..=600, urgency in 0u32..=10_000,
    ) {
        let parent = parent(q, limit);
        let result = plan_execution(&parent, &ExecutionAlgorithm::ArrivalPrice {
            slice_count: slices, interval_seconds: interval, urgency_bps: urgency,
        });
        // A slice that would round to zero is refused, exactly as for VWAP.
        prop_assume!(result.is_ok());
        let plan = result.unwrap();
        assert_common(&plan, &parent, interval)?;
        let children = scaled(&plan);
        prop_assert_eq!(children.len(), slices as usize);
        let body = &children[..children.len() - 1];
        prop_assert!(body.windows(2).all(|pair| pair[0] >= pair[1]), "arrival price must front-load: {:?}", body);
        if urgency == 0 {
            prop_assert!(body.windows(2).all(|pair| pair[0] == pair[1]), "zero urgency must be flat: {:?}", body);
        }
    }

    #[test]
    fn iceberg_children_are_the_display_size_until_the_remainder(
        q in quantity(), limit in limit(), display in 1i128..=5_000 * UNIT, interval in 1u64..=600,
    ) {
        prop_assume!((q + display - 1) / display <= 10_000);
        let parent = parent(q, limit);
        let kind = if limit.is_some() { ChildOrderKind::Limit } else { ChildOrderKind::Market };
        let via_algorithm = plan_execution(&parent, &ExecutionAlgorithm::Iceberg {
            display_quantity: Decimal::from_scaled(display), interval_seconds: interval,
        }).unwrap();
        let direct = plan_iceberg_execution(&parent, Decimal::from_scaled(display), interval, kind).unwrap();
        for plan in [&via_algorithm, &direct] {
            assert_common(plan, &parent, interval)?;
            let children = scaled(plan);
            prop_assert_eq!(children.len() as i128, (q + display - 1) / display);
            prop_assert!(children[..children.len() - 1].iter().all(|c| *c == display));
            let tail = *children.last().unwrap();
            prop_assert!(tail > 0 && tail <= display);
        }
        prop_assert_eq!(scaled(&via_algorithm), scaled(&direct));
    }
}
