//! Property/model-based coverage for the OMS order-lifecycle state machine.
//!
//! This test maintains its own independent edge-list model of legal
//! transitions (`legal_transitions`) rather than calling into the crate's
//! private `is_valid_transition`. A divergence between this model and the
//! real `OmsOrder::transition` is therefore a genuine regression signal, not
//! a tautological self-check. See docs/06-delivery/14-master-plan-conformance-audit.md
//! (Reliability and quality conformance: "A comprehensive state-model/
//! property test program for every planned asset/order type is not
//! complete") for the gap this closes a first slice of.

use follon_control_plane::OmsOrder;
use follon_domain::{Decimal, OrderIntent, OrderState, OrderType, RiskDecision, Side, TimeInForce};
use proptest::prelude::*;

const ALL_STATES: [OrderState; 15] = [
    OrderState::Created,
    OrderState::PendingRisk,
    OrderState::RiskRejected,
    OrderState::Approved,
    OrderState::PendingSubmit,
    OrderState::Submitted,
    OrderState::Acknowledged,
    OrderState::PartiallyFilled,
    OrderState::Filled,
    OrderState::PendingCancel,
    OrderState::PendingReplace,
    OrderState::Cancelled,
    OrderState::Rejected,
    OrderState::Expired,
    OrderState::Unknown,
];

/// Independent model of the documented OMS lifecycle graph. `OrderState` has
/// no `Ord` impl, so this is a small linear from -> allowed-to table rather
/// than a `BTreeMap`; 15 states makes the linear scan in `is_legal` trivial.
fn legal_transitions() -> Vec<(OrderState, Vec<OrderState>)> {
    use OrderState::{
        Acknowledged, Approved, Cancelled, Created, Expired, Filled, PartiallyFilled,
        PendingCancel, PendingReplace, PendingRisk, PendingSubmit, Rejected, RiskRejected,
        Submitted, Unknown,
    };
    Vec::from([
        (Created, vec![Approved, RiskRejected]),
        (Approved, vec![PendingSubmit]),
        (PendingSubmit, vec![Submitted, Unknown]),
        (
            Submitted,
            vec![Acknowledged, Cancelled, Rejected, Expired, Unknown],
        ),
        (
            Acknowledged,
            vec![
                PartiallyFilled,
                Filled,
                PendingCancel,
                PendingReplace,
                Cancelled,
                Rejected,
                Expired,
                Unknown,
            ],
        ),
        (
            PartiallyFilled,
            vec![
                Filled,
                PendingCancel,
                PendingReplace,
                Cancelled,
                Rejected,
                Expired,
                Unknown,
            ],
        ),
        (
            PendingCancel,
            vec![
                Acknowledged,
                PartiallyFilled,
                Filled,
                Cancelled,
                Rejected,
                Expired,
                Unknown,
            ],
        ),
        (
            PendingReplace,
            vec![
                Acknowledged,
                PartiallyFilled,
                Filled,
                Cancelled,
                Rejected,
                Expired,
                Unknown,
            ],
        ),
        (
            Unknown,
            vec![
                Acknowledged,
                PartiallyFilled,
                Filled,
                Cancelled,
                Rejected,
                Expired,
                PendingReplace,
            ],
        ),
        // Late authoritative broker evidence can still resolve an already
        // locally-terminal state through UNKNOWN rather than rewriting it.
        (Cancelled, vec![Unknown]),
        (Rejected, vec![Unknown]),
        (Expired, vec![Unknown]),
        // Absolute terminal: no legal outgoing edge at all, not even to
        // UNKNOWN. A cumulative fill is the OMS's most authoritative fact.
        (Filled, vec![]),
        // Never reached by `from_approved_intent`/`transition` in the
        // current implementation; carried here only so the model is total
        // over every declared `OrderState` variant.
        (RiskRejected, vec![]),
        (PendingRisk, vec![]),
    ])
}

fn is_legal(model: &[(OrderState, Vec<OrderState>)], from: OrderState, to: OrderState) -> bool {
    model
        .iter()
        .find(|(state, _)| *state == from)
        .is_some_and(|(_, edges)| edges.contains(&to))
}

fn state_strategy() -> impl Strategy<Value = OrderState> {
    (0..ALL_STATES.len()).prop_map(|index| ALL_STATES[index])
}

fn fixture_order() -> OmsOrder {
    let intent = OrderIntent {
        intent_id: "intent-proptest-001".to_owned(),
        account_id: "acct-proptest".to_owned(),
        strategy_id: "strategy-proptest".to_owned(),
        instrument_id: "instrument-proptest".to_owned(),
        correlation_id: "corr-proptest-001".to_owned(),
        side: Side::Buy,
        quantity: Decimal::from_integer(1).unwrap(),
        order_type: OrderType::Market,
        limit_price: None,
        time_in_force: TimeInForce::Day,
        rationale: "property test fixture".to_owned(),
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        strategy_version: "strategy-v1".to_owned(),
        configuration_version: "config-v1".to_owned(),
        environment: "PAPER".to_owned(),
    };
    let decision = RiskDecision {
        decision_id: "decision-proptest-001".to_owned(),
        intent_id: intent.intent_id.clone(),
        approved: true,
        reason_codes: vec![],
        policy_version: "policy-v1".to_owned(),
        decided_at: "2026-01-01T00:00:00Z".to_owned(),
        correlation_id: intent.correlation_id.clone(),
        actor: "risk-engine".to_owned(),
        evaluated_limits: String::new(),
    };
    OmsOrder::from_approved_intent(intent, &decision).unwrap()
}

proptest! {
    /// For every randomly generated sequence of attempted transitions, the
    /// real `OmsOrder::transition` must agree with the independent model on
    /// exactly which transitions are legal, must reach exactly the state the
    /// model predicts, must never mutate state on a rejected transition, and
    /// must never mutate order identity or the original intent.
    #[test]
    fn oms_lifecycle_matches_independent_model(
        candidates in prop::collection::vec(state_strategy(), 1..40),
    ) {
        let model = legal_transitions();
        let mut order = fixture_order();
        let original_order_id = order.order_id.clone();
        let original_intent = order.intent.clone();
        let mut expected_state = OrderState::Created;

        for candidate in candidates {
            let legal = is_legal(&model, expected_state, candidate);
            let state_before = order.state;
            let result = order.transition(candidate, "proptest-step");

            prop_assert_eq!(result.is_ok(), legal);
            if legal {
                expected_state = candidate;
            } else {
                // A rejected transition must never mutate state.
                prop_assert_eq!(state_before, expected_state);
            }
            prop_assert_eq!(order.state, expected_state);
            prop_assert_eq!(&order.order_id, &original_order_id);
            prop_assert_eq!(&order.intent, &original_intent);
        }
    }
}

#[test]
fn filled_is_absolute_terminal() {
    let mut order = fixture_order();
    order.transition(OrderState::Approved, "r").unwrap();
    order.transition(OrderState::PendingSubmit, "r").unwrap();
    order.transition(OrderState::Submitted, "r").unwrap();
    order.transition(OrderState::Acknowledged, "r").unwrap();
    order.transition(OrderState::Filled, "r").unwrap();

    for candidate in ALL_STATES {
        assert!(
            order.transition(candidate, "post-filled").is_err(),
            "Filled must have no legal outgoing transition, including to {candidate:?}",
        );
    }
}

#[test]
fn model_is_total_over_every_declared_state() {
    let model = legal_transitions();
    for state in ALL_STATES {
        assert!(
            model.iter().any(|(covered, _)| *covered == state),
            "the independent model must cover {state:?} even if its only edge set is empty",
        );
    }
}
