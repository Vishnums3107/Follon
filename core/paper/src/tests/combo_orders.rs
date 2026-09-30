//! Combination submission, working-order visibility, fault handling and durable recovery.

use super::*;

#[test]
fn combo_submission_creates_one_acknowledged_order_for_the_whole_group() {
    let mut service = service_permitting_shorts();
    let outcome = service
        .submit_combo_intent(
            combo_intent("combo-000020", "2026-01-02T14:30:00Z"),
            combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(
        outcome.decision.approved,
        "{:?}",
        outcome.decision.reason_codes
    );
    assert_eq!(
        outcome.order_id.as_deref(),
        Some("combo-order-combo-000020")
    );
    assert_eq!(outcome.state, Some(OrderState::Acknowledged));

    let order = service.combo_order("combo-order-combo-000020").unwrap();
    assert_eq!(order.oms.intent.legs.len(), 2);
    assert!(order.broker_order_id.is_some());
    // One broker order for the whole group, not one per leg.
    assert_eq!(order.broker_order_versions.len(), 1);
    // And one open order, not two.
    assert_eq!(service.dashboard().working_orders, 1);
    assert!(service.orders.is_empty());
}

#[test]
fn combo_submission_is_idempotent_and_refuses_a_changed_retry() {
    let mut service = service_permitting_shorts();
    let intent = combo_intent("combo-000021", "2026-01-02T14:30:00Z");
    let market = combo_market("2026-01-02T14:30:00Z");
    let first = service
        .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
        .unwrap();
    let replay = service
        .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
        .unwrap();
    assert_eq!(first.order_id, replay.order_id);
    assert_eq!(first.decision, replay.decision);
    assert_eq!(service.combo_orders.len(), 1);

    // The same identity with different economics is refused outright, not
    // silently treated as the original.
    let mut tampered = intent.clone();
    tampered.combo_quantity = decimal("units", "5").unwrap();
    assert!(service
        .submit_combo_intent(tampered, market.clone(), "2026-01-02T14:30:02Z")
        .is_err());

    // And a retry that re-prices the original is refused too: a retry is a
    // retry, not a new decision wearing an old identity.
    let mut moved = market;
    moved.marks[0].mark_price = decimal("mark", "7.51").unwrap();
    assert!(service
        .submit_combo_intent(intent, moved, "2026-01-02T14:30:02Z")
        .is_err());
}

#[test]
fn combo_refusal_records_evidence_and_creates_no_order() {
    let mut service = service_permitting_shorts();
    service
        .activate_kill_switch(KillSwitchScope::Global)
        .unwrap();
    let outcome = service
        .submit_combo_intent(
            combo_intent("combo-000022", "2026-01-02T14:30:00Z"),
            combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(!outcome.decision.approved);
    assert!(outcome.order_id.is_none());
    assert!(service.combo_orders.is_empty());
    // The refusal itself is durable evidence: a rejected combination is
    // still a decision that was made and has to be auditable.
    assert!(service
        .combo_risk_evidence("paper-combo-risk-combo-000022")
        .is_some());
}

#[test]
fn combo_transport_failure_leaves_the_group_unknown_and_disconnects() {
    // Until E5.1 the fault wrapper did not forward combinations, so this
    // passed on the trait's refusal and the scheduled fault was never
    // consumed. It now exercises the fault itself; the test below shows
    // an unfaulted combination reaches the model.
    let account = account();
    let adapter = IbkrPaperAdapter::new(&account).unwrap();
    let mut faulted = FaultInjectingBroker::new(adapter);
    faulted.inject(BrokerOperation::Submit, BrokerFault::Disconnect);
    let mut service = PaperTradingService::new(
        account,
        policy_permitting_shorts(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        faulted,
    )
    .unwrap();
    let result = service.submit_combo_intent(
        combo_intent("combo-000023", "2026-01-02T14:30:00Z"),
        combo_market("2026-01-02T14:30:00Z"),
        "2026-01-02T14:30:02Z",
    );
    // The outcome is genuinely unknown -- the request may or may not have
    // reached the venue -- so it is never reported as a clean rejection.
    assert!(result.is_err());
    let order = service.combo_order("combo-order-combo-000023").unwrap();
    assert_eq!(order.oms.state, OrderState::Unknown);
    // And an UNKNOWN combination blocks the next decision of either kind,
    // exactly as an UNKNOWN plain order does.
    assert!(service.has_unknown_order());
}

#[test]
fn the_fault_wrapper_forwards_a_combination_when_no_fault_is_scheduled() {
    let account = account();
    let faulted = FaultInjectingBroker::new(IbkrPaperAdapter::new(&account).unwrap());
    let mut service = PaperTradingService::new(
        account,
        policy_permitting_shorts(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        faulted,
    )
    .unwrap();
    let outcome = service
        .submit_combo_intent(
            combo_intent("combo-000025", "2026-01-02T14:30:00Z"),
            combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert_eq!(outcome.state, Some(OrderState::Acknowledged));
    assert!(!service.has_unknown_order());
}

#[test]
fn a_working_combination_is_visible_to_every_single_order_risk_counter() {
    let mut service = service_permitting_shorts();
    service
        .submit_combo_intent(
            combo_intent("combo-000024", "2026-01-02T14:30:00Z"),
            combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();

    // Open-order count, rate window, and reserved cash all see it.
    assert_eq!(service.working_order_count(), 1);
    assert_eq!(
        service.recent_order_count("2026-01-02T14:30:30Z").unwrap(),
        1
    );
    // 4 units * 2.50 net debit.
    assert_eq!(
        service.total_reserved_cash().unwrap(),
        decimal("reserved", "10").unwrap()
    );

    // And self-trade: the combination's short far-strike leg is a real
    // resting sell, so a plain buy on that instrument is a self-trade.
    assert!(service.conflicts_with_working_order("inst.us_option.spy.far", Side::Buy));
    assert!(!service.conflicts_with_working_order("inst.us_option.spy.far", Side::Sell));
    // The long near-strike leg is the mirror image.
    assert!(service.conflicts_with_working_order("inst.us_option.spy.near", Side::Sell));
}

#[test]
fn a_working_combination_reconciles_against_its_independent_broker_order() {
    let mut service = service_permitting_shorts();
    service
        .submit_combo_intent(
            combo_intent("combo-000025", "2026-01-02T14:30:00Z"),
            combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
    assert!(report.is_clean(), "{:?}", report.issues);
    service.broker.combos.clear();
    let missing = service.reconcile("2026-01-02T21:00:01Z").unwrap();
    assert!(missing
        .issues
        .iter()
        .any(|issue| issue.category == "MISSING_BROKER_ORDER"));
}

#[test]
fn a_combination_survives_a_durable_journal_reopen() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-{}.ndjson",
        std::process::id(),
        "combo-recovery"
    ));
    let _ = fs::remove_file(&journal_path);
    let account = account();
    let mut service = PaperTradingService::open_durable(
        account.clone(),
        policy_permitting_shorts(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account).unwrap(),
        &journal_path,
    )
    .unwrap();
    let intent = combo_intent("combo-000026", "2026-01-02T14:30:00Z");
    let market = combo_market("2026-01-02T14:30:00Z");
    service
        .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
        .unwrap();
    let before = service
        .combo_order("combo-order-combo-000026")
        .unwrap()
        .clone();
    drop(service);

    let reopened = PaperTradingService::open_durable(
        account.clone(),
        policy_permitting_shorts(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account).unwrap(),
        &journal_path,
    )
    .unwrap();
    let after = reopened.combo_order("combo-order-combo-000026").unwrap();
    // The whole structure comes back exactly: every leg, its ratio, its
    // protected price, the price-limit kind, and the per-leg observation
    // that priced it.
    assert_eq!(after.oms.intent, intent);
    assert_eq!(after.market, market);
    assert_eq!(after.oms.state, before.oms.state);
    assert_eq!(after.broker_order_id, before.broker_order_id);
    assert_eq!(
        after.oms.intent.price_limit.kind(),
        follon_domain::ComboPriceLimit::MaximumDebit(Decimal::ZERO).kind()
    );
    // And the reservation it implies survives with it, so a restart does
    // not free cash the combination still has committed.
    assert_eq!(
        reopened.total_reserved_cash().unwrap(),
        decimal("reserved", "10").unwrap()
    );
    let _ = fs::remove_file(&journal_path);
}

/// An absent combination field deserializes as "no combinations".
///
/// This checks the persisted *type*, not a reopened journal file, and the
/// distinction is real. `FilePaperJournal::open` additionally requires each
/// line to re-serialize byte-for-byte, so a file missing any field the
/// current serializer writes is rejected before `#[serde(default)]` can
/// apply. That is pre-existing behaviour, not a consequence of the
/// combination fields: deleting `tax_lots` -- which predates them -- from a
/// journal line fails exactly the same way, verified directly. So the
/// defaults make the type tolerant, which is what a future format change
/// needs, while whole-file compatibility across a schema change remains an
/// open question recorded in `docs/06-delivery/16-delivery-state.md`.
#[test]
fn an_absent_combination_field_deserializes_as_no_combinations() {
    let without_combinations = serde_json::json!({
        "configuration_fingerprint": "",
        "account_id": "acct.paper.001",
        "currency": "USD",
        "cash": "100000.00000000",
        "orders": {},
        "risk_evidence": {},
        "positions": {},
        "execution_ids": [],
        "active_kill_switches": [],
        "incidents": {},
        "last_reconciled_at": serde_json::Value::Null,
        "last_reconciliation_clean": serde_json::Value::Null,
        "paper_days": {},
        "next_reconciliation": 0,
    });
    let state: PersistentPaperState = serde_json::from_value(without_combinations).unwrap();
    assert!(state.combo_orders.is_empty());
    assert!(state.combo_risk_evidence.is_empty());
}
