//! Property-based coverage for `settle_expired_option_position`: a second,
//! independently designed slice of the "comprehensive state-model/property
//! test program for every planned asset/order type... not complete" gap
//! recorded in docs/06-delivery/14-master-plan-conformance-audit.md
//! (Reliability and quality conformance), after the first slice covering the
//! OMS order-lifecycle state machine (`core/control-plane`).
//!
//! This checks economic invariants of the settlement function -- position
//! closure, sign consistency, cash/underlying conservation, and outcome
//! classification -- independently derived from the documented contract
//! (doc comments on `OptionLifecycleOutcome`/`OptionSettlementMethod`), not
//! copied from the function body.

use follon_domain::Decimal;
use follon_options::{
    settle_expired_option_position, OptionContract, OptionLifecycleOutcome, OptionRight,
    OptionSettlementMethod,
};
use proptest::prelude::*;

fn decimal_strategy(range: std::ops::RangeInclusive<i64>) -> impl Strategy<Value = Decimal> {
    range.prop_map(|value| Decimal::from_integer(value).unwrap())
}

fn nonzero_quantity_strategy() -> impl Strategy<Value = Decimal> {
    prop_oneof![decimal_strategy(-10..=-1), decimal_strategy(1..=10)]
}

fn right_strategy() -> impl Strategy<Value = OptionRight> {
    prop_oneof![Just(OptionRight::Call), Just(OptionRight::Put)]
}

fn method_strategy() -> impl Strategy<Value = OptionSettlementMethod> {
    prop_oneof![
        Just(OptionSettlementMethod::Cash),
        Just(OptionSettlementMethod::Physical),
    ]
}

fn fixture_contract(strike: Decimal, multiplier: Decimal, right: OptionRight) -> OptionContract {
    OptionContract {
        option_id: "option.proptest.001".to_owned(),
        underlying_instrument_id: "instrument.proptest".to_owned(),
        expiration_at: "2026-01-01T00:00:00Z".to_owned(),
        strike,
        right,
        multiplier,
        currency: "USD".to_owned(),
        reference_version: "ref-v1".to_owned(),
    }
}

proptest! {
    /// Every settlement, regardless of right, settlement method, or
    /// direction, must close the position exactly.
    #[test]
    fn settlement_always_closes_the_position_exactly(
        strike in decimal_strategy(1..=500),
        multiplier in decimal_strategy(1..=100),
        right in right_strategy(),
        method in method_strategy(),
        signed_quantity in nonzero_quantity_strategy(),
        underlying_price in decimal_strategy(0..=1000),
        threshold in decimal_strategy(0..=50),
    ) {
        let contract = fixture_contract(strike, multiplier, right);
        let settlement = settle_expired_option_position(
            "lifecycle-proptest-001",
            &contract,
            signed_quantity,
            underlying_price,
            threshold,
            method,
            "2026-01-02T00:00:00Z",
        ).unwrap();

        prop_assert_eq!(
            settlement.option_quantity_delta,
            Decimal::ZERO.checked_sub(signed_quantity).unwrap()
        );
        prop_assert!(settlement.intrinsic_value >= Decimal::ZERO);
    }

    /// Cash settlement never moves the underlying, whatever the outcome.
    /// Physical settlement's cash and underlying deltas always have exactly
    /// the sign that pays for delivery (never free stock, never free cash).
    #[test]
    fn settlement_method_conservation_holds(
        strike in decimal_strategy(1..=500),
        multiplier in decimal_strategy(1..=100),
        right in right_strategy(),
        method in method_strategy(),
        signed_quantity in nonzero_quantity_strategy(),
        underlying_price in decimal_strategy(0..=1000),
    ) {
        // A zero threshold means only an exactly-zero intrinsic value expires
        // worthless, isolating this property from threshold-driven expiry.
        let contract = fixture_contract(strike, multiplier, right);
        let settlement = settle_expired_option_position(
            "lifecycle-proptest-002",
            &contract,
            signed_quantity,
            underlying_price,
            Decimal::ZERO,
            method,
            "2026-01-02T00:00:00Z",
        ).unwrap();

        if method == OptionSettlementMethod::Cash {
            prop_assert_eq!(settlement.underlying_quantity_delta, Decimal::ZERO);
        }
        if settlement.outcome == OptionLifecycleOutcome::Expired {
            prop_assert_eq!(settlement.underlying_quantity_delta, Decimal::ZERO);
            prop_assert_eq!(settlement.cash_delta, Decimal::ZERO);
        } else if method == OptionSettlementMethod::Physical {
            // Delivering (or receiving) the underlying and its matching cash
            // leg must always run in exactly opposite directions: nothing is
            // ever delivered and paid for at the same time, and nothing is
            // ever received for free.
            let delta = settlement.underlying_quantity_delta;
            let cash = settlement.cash_delta;
            prop_assert!(
                (delta > Decimal::ZERO && cash < Decimal::ZERO)
                    || (delta < Decimal::ZERO && cash > Decimal::ZERO),
                "physical settlement must pay for exactly what it delivers: \
                 underlying_quantity_delta={delta:?}, cash_delta={cash:?}",
            );
        }
    }

    /// The three lifecycle outcomes are mutually exclusive and exhaustive,
    /// and classify consistently against the caller's own inputs.
    #[test]
    fn outcome_classification_matches_inputs(
        strike in decimal_strategy(1..=500),
        multiplier in decimal_strategy(1..=100),
        right in right_strategy(),
        method in method_strategy(),
        signed_quantity in nonzero_quantity_strategy(),
        underlying_price in decimal_strategy(0..=1000),
        threshold in decimal_strategy(0..=50),
    ) {
        let contract = fixture_contract(strike, multiplier, right);
        let settlement = settle_expired_option_position(
            "lifecycle-proptest-003",
            &contract,
            signed_quantity,
            underlying_price,
            threshold,
            method,
            "2026-01-02T00:00:00Z",
        ).unwrap();

        let intrinsic = contract.intrinsic_value(underlying_price).unwrap();
        let expected = if intrinsic == Decimal::ZERO || intrinsic < threshold {
            OptionLifecycleOutcome::Expired
        } else if signed_quantity > Decimal::ZERO {
            OptionLifecycleOutcome::Exercised
        } else {
            OptionLifecycleOutcome::Assigned
        };
        prop_assert_eq!(settlement.outcome, expected);
    }

    /// Settlement is a pure function of its inputs: calling it twice with
    /// identical arguments must produce a byte-for-byte identical result,
    /// matching this codebase's deterministic-replay/reproducibility
    /// requirement documented across docs/06-delivery.
    #[test]
    fn settlement_is_deterministic(
        strike in decimal_strategy(1..=500),
        multiplier in decimal_strategy(1..=100),
        right in right_strategy(),
        method in method_strategy(),
        signed_quantity in nonzero_quantity_strategy(),
        underlying_price in decimal_strategy(0..=1000),
        threshold in decimal_strategy(0..=50),
    ) {
        let contract = fixture_contract(strike, multiplier, right);
        let first = settle_expired_option_position(
            "lifecycle-proptest-004",
            &contract,
            signed_quantity,
            underlying_price,
            threshold,
            method,
            "2026-01-02T00:00:00Z",
        ).unwrap();
        let second = settle_expired_option_position(
            "lifecycle-proptest-004",
            &contract,
            signed_quantity,
            underlying_price,
            threshold,
            method,
            "2026-01-02T00:00:00Z",
        ).unwrap();
        prop_assert_eq!(first, second);
    }
}

#[test]
fn settlement_before_expiration_is_rejected() {
    let contract = fixture_contract(
        Decimal::from_integer(100).unwrap(),
        Decimal::from_integer(1).unwrap(),
        OptionRight::Call,
    );
    let result = settle_expired_option_position(
        "lifecycle-early",
        &contract,
        Decimal::from_integer(1).unwrap(),
        Decimal::from_integer(150).unwrap(),
        Decimal::ZERO,
        OptionSettlementMethod::Cash,
        "2025-12-31T00:00:00Z",
    );
    assert!(result.is_err());
}

#[test]
fn zero_quantity_is_rejected() {
    let contract = fixture_contract(
        Decimal::from_integer(100).unwrap(),
        Decimal::from_integer(1).unwrap(),
        OptionRight::Call,
    );
    let result = settle_expired_option_position(
        "lifecycle-zero-qty",
        &contract,
        Decimal::ZERO,
        Decimal::from_integer(150).unwrap(),
        Decimal::ZERO,
        OptionSettlementMethod::Cash,
        "2026-01-02T00:00:00Z",
    );
    assert!(result.is_err());
}
