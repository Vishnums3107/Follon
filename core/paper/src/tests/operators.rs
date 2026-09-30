//! Operator-attributed kill-switch and order operations.

use super::*;

#[test]
fn an_authenticated_submitter_is_journaled_and_owns_the_retry() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-{}.ndjson",
        std::process::id(),
        "combo-submitted-by"
    ));
    let _ = fs::remove_file(&journal_path);
    let account = account();
    let open = || {
        PaperTradingService::open_durable(
            account.clone(),
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap()
    };
    let mut service = open();
    let intent = combo_intent("combo-000027", "2026-01-02T14:30:00Z");
    let market = combo_market("2026-01-02T14:30:00Z");
    let decided_at = "2026-01-02T14:30:02Z";
    let outcome = service
        .submit_combo_intent_as(
            intent.clone(),
            market.clone(),
            decided_at,
            Some("user.trader"),
        )
        .unwrap();
    assert!(outcome.decision.approved);
    let decision_id = outcome.decision.decision_id.clone();
    assert_eq!(
        service
            .combo_risk_evidence(&decision_id)
            .unwrap()
            .submitted_by
            .as_deref(),
        Some("user.trader")
    );
    // Neither another operator nor an unattributed caller may claim it.
    for other in [Some("user.other"), None] {
        assert!(service
            .submit_combo_intent_as(intent.clone(), market.clone(), decided_at, other)
            .is_err());
    }
    assert!(service
        .submit_combo_intent_as(
            combo_intent("combo-000028", "2026-01-02T14:30:00Z"),
            market.clone(),
            decided_at,
            Some("User Trader"),
        )
        .is_err());
    // The original submitter's retry is the idempotent original.
    let retry = service
        .submit_combo_intent_as(intent, market.clone(), decided_at, Some("user.trader"))
        .unwrap();
    assert_eq!(retry.order_id, outcome.order_id);
    // A direct, unattributed submission journals no `submitted_by` key at
    // all, keeping the earlier serialization byte for byte.
    service
        .submit_combo_intent(
            combo_intent("combo-000029", "2026-01-02T14:30:00Z"),
            market,
            decided_at,
        )
        .unwrap();
    drop(service);

    // The attribution survives a restart.
    let reopened = open();
    assert_eq!(
        reopened
            .combo_risk_evidence(&decision_id)
            .unwrap()
            .submitted_by
            .as_deref(),
        Some("user.trader")
    );
    drop(reopened);
    let journal = fs::read_to_string(&journal_path).unwrap();
    let last: serde_json::Value = serde_json::from_str(journal.lines().last().unwrap()).unwrap();
    fn find<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        match value {
            serde_json::Value::Object(map) => map
                .get(key)
                .or_else(|| map.values().find_map(|child| find(child, key))),
            serde_json::Value::Array(items) => items.iter().find_map(|child| find(child, key)),
            _ => None,
        }
    }
    let attributed = find(&last, "paper-combo-risk-combo-000027").unwrap();
    assert_eq!(attributed["submitted_by"], "user.trader");
    let unattributed = find(&last, "paper-combo-risk-combo-000029").unwrap();
    assert!(
        unattributed.get("submitted_by").is_none(),
        "an unattributed decision must not carry submitted_by"
    );
    let _ = fs::remove_file(&journal_path);
}

#[test]
fn an_operator_kill_switch_change_is_journaled_and_survives_reopen() {
    let journal = |label: &str| {
        let path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{label}.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        path
    };
    let account = account();
    let open = |path: &Path| {
        PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            path,
        )
        .unwrap()
    };
    // The journal is exclusively locked while its service is open, so it
    // is read only after the service is dropped.
    let last_state = |path: &Path| {
        let journal = fs::read_to_string(path).unwrap();
        let record: serde_json::Value =
            serde_json::from_str(journal.lines().last().unwrap()).unwrap();
        record["state"].clone()
    };

    // A local, unattributed change journals no operator record at all,
    // keeping the earlier serialization byte for byte.
    let local_path = journal("kill-switch-local");
    let mut local = open(&local_path);
    assert!(local.activate_kill_switch(KillSwitchScope::Global).unwrap());
    drop(local);
    assert!(last_state(&local_path)
        .get("kill_switch_operations")
        .is_none());
    let _ = fs::remove_file(&local_path);

    let journal_path = journal("kill-switch-operator");
    let mut service = open(&journal_path);
    let spy = KillSwitchScope::Instrument("inst.us_equity.spy".to_owned());
    assert!(service
        .activate_kill_switch_as(spy.clone(), "user.risk", "2026-01-02T14:29:00Z")
        .unwrap());
    // A repeat changes nothing and journals nothing.
    assert!(!service
        .activate_kill_switch_as(spy.clone(), "user.risk", "2026-01-02T14:29:30Z")
        .unwrap());
    // The switch binds: an order on that instrument is refused.
    let refused = service
        .submit_intent(
            intent("intent-kill-operator-001", "2026-01-02T14:30:00Z"),
            market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:01Z",
        )
        .unwrap();
    assert_eq!(
        refused.decision.reason_codes,
        vec!["KILL_SWITCH_INSTRUMENT_INST.US_EQUITY.SPY".to_owned()]
    );
    // A malformed operator or time changes nothing.
    assert!(service
        .release_kill_switch_as(spy.clone(), "User Risk", "2026-01-02T14:31:00Z")
        .is_err());
    assert!(service
        .release_kill_switch_as(spy.clone(), "user.risk", "yesterday")
        .is_err());
    assert_eq!(
        service.kill_switches().active_keys(),
        vec!["instrument:inst.us_equity.spy".to_owned()]
    );
    assert!(service
        .release_kill_switch_as(spy, "user.risk.second", "2026-01-02T14:31:00Z")
        .unwrap());
    let expected = vec![
        KillSwitchOperation {
            scope: "instrument:inst.us_equity.spy".to_owned(),
            action: KillSwitchAction::Activate,
            operator: "user.risk".to_owned(),
            operated_at: "2026-01-02T14:29:00Z".to_owned(),
        },
        KillSwitchOperation {
            scope: "instrument:inst.us_equity.spy".to_owned(),
            action: KillSwitchAction::Release,
            operator: "user.risk.second".to_owned(),
            operated_at: "2026-01-02T14:31:00Z".to_owned(),
        },
    ];
    assert_eq!(service.kill_switch_operations(), expected.as_slice());
    drop(service);

    // The journal names each operator, and a reopen restores the record.
    let state = last_state(&journal_path);
    assert_eq!(state["kill_switch_operations"][0]["operator"], "user.risk");
    assert_eq!(state["kill_switch_operations"][1]["action"], "RELEASE");
    let reopened = open(&journal_path);
    assert_eq!(reopened.kill_switch_operations(), expected.as_slice());
    assert!(reopened.kill_switches().active_keys().is_empty());
    drop(reopened);
    let _ = fs::remove_file(&journal_path);
}

#[test]
fn a_persisted_kill_switch_operation_must_be_well_formed() {
    let mut attributed = service();
    attributed
        .activate_kill_switch_as(KillSwitchScope::Global, "user.risk", "2026-01-02T14:29:00Z")
        .unwrap();
    let valid = attributed.persistent_state();
    let mut restored = service();
    restored.restore(valid.clone()).unwrap();
    assert_eq!(restored.kill_switch_operations().len(), 1);
    let corruptions: [fn(&mut PersistentKillSwitchOperation); 4] = [
        |operation| operation.scope = "everything".to_owned(),
        |operation| operation.action = "TOGGLE".to_owned(),
        |operation| operation.operator = "User Risk".to_owned(),
        |operation| operation.operated_at = "yesterday".to_owned(),
    ];
    for corrupt in corruptions {
        let mut state = valid.clone();
        corrupt(&mut state.kill_switch_operations[0]);
        assert!(service().restore(state).is_err());
    }
}

fn scratch_journal(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-{label}.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    path
}

/// The journal's last recorded state, read once its service is dropped:
/// an open service holds the journal exclusively.
fn last_journal_state(path: &Path) -> serde_json::Value {
    let journal = fs::read_to_string(path).unwrap();
    let record: serde_json::Value = serde_json::from_str(journal.lines().last().unwrap()).unwrap();
    record["state"].clone()
}

#[test]
fn an_authenticated_order_submitter_is_journaled_and_owns_the_retry() {
    let journal_path = scratch_journal("order-submitted-by");
    let account = account();
    let open = || {
        PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap()
    };
    let mut service = open();
    let order = intent("paper-000041", "2026-01-02T14:30:00Z");
    let observation = market("2026-01-02T14:30:00Z");
    let decided_at = "2026-01-02T14:30:02Z";
    let outcome = service
        .submit_intent_as(
            order.clone(),
            observation.clone(),
            decided_at,
            Some("user.trader"),
        )
        .unwrap();
    assert!(outcome.decision.approved);
    let decision_id = outcome.decision.decision_id.clone();
    assert_eq!(
        service
            .risk_evidence(&decision_id)
            .unwrap()
            .submitted_by
            .as_deref(),
        Some("user.trader")
    );
    // Neither another operator nor an unattributed caller may claim it.
    for other in [Some("user.other"), None] {
        assert!(service
            .submit_intent_as(order.clone(), observation.clone(), decided_at, other)
            .is_err());
    }
    // A malformed submitter is refused before risk sees the intent.
    assert!(service
        .submit_intent_as(
            intent("paper-000042", "2026-01-02T14:30:00Z"),
            observation.clone(),
            decided_at,
            Some("User Trader"),
        )
        .is_err());
    assert!(service.risk_evidence("paper-risk-paper-000042").is_none());
    // The original submitter's retry is the idempotent original.
    let retry = service
        .submit_intent_as(order, observation.clone(), decided_at, Some("user.trader"))
        .unwrap();
    assert_eq!(retry.order_id, outcome.order_id);
    // A direct submission journals no `submitted_by` key at all, keeping
    // the earlier serialization byte for byte.
    service
        .submit_intent(
            intent("paper-000043", "2026-01-02T14:30:00Z"),
            observation,
            decided_at,
        )
        .unwrap();
    drop(service);

    let state = last_journal_state(&journal_path);
    assert_eq!(
        state["risk_evidence"]["paper-risk-paper-000041"]["submitted_by"],
        "user.trader"
    );
    assert!(
        state["risk_evidence"]["paper-risk-paper-000043"]
            .get("submitted_by")
            .is_none(),
        "an unattributed decision must not carry submitted_by"
    );
    // The attribution survives a restart.
    let reopened = open();
    assert_eq!(
        reopened
            .risk_evidence(&decision_id)
            .unwrap()
            .submitted_by
            .as_deref(),
        Some("user.trader")
    );
    drop(reopened);
    let _ = fs::remove_file(&journal_path);
}

/// The model, except that asking it to cancel stops the process, as a
/// crash just after the broker received the request would.
struct CrashOnCancelBroker(IbkrPaperAdapter);

impl PaperBrokerAdapter for CrashOnCancelBroker {
    fn adapter_configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        self.0.adapter_configuration_fingerprint(account_id)
    }

    fn configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        self.0.configuration_fingerprint(account_id)
    }

    fn permits_empty_journal(&self, account_id: &str) -> bool {
        self.0.permits_empty_journal(account_id)
    }

    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        self.0.submit(request)
    }

    fn cancel(&mut self, _request: &BrokerCancelRequest) -> Result<(), PaperError> {
        panic!("the process stopped while the broker held the cancellation")
    }

    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        self.0.poll(account_id)
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        self.0.snapshot(account_id)
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        self.0.reconnect(account_id)
    }
}

#[test]
fn an_operator_cancellation_is_durable_before_the_broker_is_asked() {
    let journal_path = scratch_journal("order-cancel-crash");
    let account = account();
    let mut service = PaperTradingService::open_durable(
        account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        CrashOnCancelBroker(IbkrPaperAdapter::new(&account).unwrap()),
        &journal_path,
    )
    .unwrap();
    let order_id = service
        .submit_intent_as(
            intent("paper-000051", "2026-01-02T14:30:00Z"),
            market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
            Some("user.trader"),
        )
        .unwrap()
        .order_id
        .unwrap();
    assert_eq!(
        service.order(&order_id).unwrap().oms.state,
        OrderState::Acknowledged
    );
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        service.cancel_order_as(&order_id, "user.risk", "2026-01-02T14:31:00Z")
    }));
    assert!(crashed.is_err(), "the broker was never asked to cancel");
    drop(service);

    // The restarted service knows the cancellation was requested, and by
    // whom, although the process stopped before it heard back.
    let reopened = PaperTradingService::open_durable(
        account.clone(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account).unwrap(),
        &journal_path,
    )
    .unwrap();
    assert_eq!(
        reopened.order(&order_id).unwrap().oms.state,
        OrderState::PendingCancel
    );
    assert_eq!(
        reopened.order_operations(),
        [OrderOperation {
            order_id: order_id.clone(),
            action: OrderOperationAction::CancelRequested,
            operator: "user.risk".to_owned(),
            operated_at: "2026-01-02T14:31:00Z".to_owned(),
        }]
        .as_slice()
    );
    drop(reopened);
    let _ = fs::remove_file(&journal_path);
}

#[test]
fn an_operator_cancellation_is_journaled_once_for_a_single_order_or_a_combination() {
    let account = account();
    let open = |path: &Path| {
        PaperTradingService::open_durable(
            account.clone(),
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            path,
        )
        .unwrap()
    };

    // A direct, unattributed cancellation journals no operation record at
    // all, keeping the earlier serialization byte for byte.
    let local_path = scratch_journal("order-operations-local");
    let mut local = open(&local_path);
    let local_id = local
        .submit_intent(
            intent("paper-000060", "2026-01-02T14:30:00Z"),
            market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap()
        .order_id
        .unwrap();
    local.cancel_order(&local_id).unwrap();
    drop(local);
    let local_state = last_journal_state(&local_path);
    assert_eq!(
        local_state["orders"][local_id.as_str()]["state"],
        "PENDING_CANCEL"
    );
    assert!(local_state.get("order_operations").is_none());
    let _ = fs::remove_file(&local_path);

    let journal_path = scratch_journal("order-operations");
    let mut service = open(&journal_path);
    let submit = |service: &mut PaperTradingService<IbkrPaperAdapter>, intent_id: &str| {
        service
            .submit_intent(
                intent(intent_id, "2026-01-02T14:30:00Z"),
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap()
            .order_id
            .unwrap()
    };
    let order_id = submit(&mut service, "paper-000061");
    let direct_id = submit(&mut service, "paper-000062");
    let combo_id = submit_lifecycle_combo(&mut service, "operator-cancel");

    // A malformed operator or time changes nothing.
    assert!(service
        .cancel_order_as(&order_id, "User Risk", "2026-01-02T14:31:00Z")
        .is_err());
    assert!(service
        .cancel_order_as(&order_id, "user.risk", "yesterday")
        .is_err());
    assert_eq!(
        service.order(&order_id).unwrap().oms.state,
        OrderState::Acknowledged
    );
    assert!(service.order_operations().is_empty());

    service
        .cancel_order_as(&order_id, "user.risk", "2026-01-02T14:31:00Z")
        .unwrap();
    // A retry, even by another operator, changes nothing and journals nothing.
    service
        .cancel_order_as(&order_id, "user.risk.second", "2026-01-02T14:31:05Z")
        .unwrap();
    service
        .cancel_order_as(&combo_id, "user.risk.second", "2026-01-02T14:31:10Z")
        .unwrap();
    // A direct cancellation is not attributed.
    service.cancel_order(&direct_id).unwrap();
    let operation = |order_id: &str, operator: &str, operated_at: &str| OrderOperation {
        order_id: order_id.to_owned(),
        action: OrderOperationAction::CancelRequested,
        operator: operator.to_owned(),
        operated_at: operated_at.to_owned(),
    };
    let expected = vec![
        operation(&order_id, "user.risk", "2026-01-02T14:31:00Z"),
        operation(&combo_id, "user.risk.second", "2026-01-02T14:31:10Z"),
    ];
    assert_eq!(service.order_operations(), expected.as_slice());
    for id in [&order_id, &direct_id] {
        assert_eq!(
            service.order(id).unwrap().oms.state,
            OrderState::PendingCancel
        );
    }
    assert_eq!(
        service.combo_order(&combo_id).unwrap().oms.state,
        OrderState::PendingCancel
    );
    drop(service);

    let state = last_journal_state(&journal_path);
    assert_eq!(state["order_operations"].as_array().unwrap().len(), 2);
    assert_eq!(state["order_operations"][0]["action"], "CANCEL_REQUESTED");
    assert_eq!(state["order_operations"][1]["operator"], "user.risk.second");
    let reopened = open(&journal_path);
    assert_eq!(reopened.order_operations(), expected.as_slice());
    drop(reopened);
    let _ = fs::remove_file(&journal_path);
}

#[test]
fn a_persisted_order_attribution_must_be_well_formed() {
    let mut attributed = service();
    let order_id = attributed
        .submit_intent_as(
            intent("paper-000071", "2026-01-02T14:30:00Z"),
            market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
            Some("user.trader"),
        )
        .unwrap()
        .order_id
        .unwrap();
    attributed
        .cancel_order_as(&order_id, "user.risk", "2026-01-02T14:31:00Z")
        .unwrap();
    let valid = attributed.persistent_state();
    let mut restored = service();
    restored.restore(valid.clone()).unwrap();
    assert_eq!(restored.order_operations().len(), 1);
    assert_eq!(
        restored
            .risk_evidence("paper-risk-paper-000071")
            .unwrap()
            .submitted_by
            .as_deref(),
        Some("user.trader")
    );
    let corruptions: [fn(&mut PersistentPaperState); 5] = [
        |state| state.order_operations[0].order_id = "order-paper-000099".to_owned(),
        |state| state.order_operations[0].action = "CANCEL".to_owned(),
        |state| state.order_operations[0].operator = "User Risk".to_owned(),
        |state| state.order_operations[0].operated_at = "yesterday".to_owned(),
        |state| {
            state
                .risk_evidence
                .get_mut("paper-risk-paper-000071")
                .unwrap()
                .submitted_by = Some("User Trader".to_owned());
        },
    ];
    for corrupt in corruptions {
        let mut state = valid.clone();
        corrupt(&mut state);
        assert!(service().restore(state).is_err());
    }
}

#[test]
fn a_kill_switch_scope_parses_from_its_stable_key() {
    for scope in [
        KillSwitchScope::Global,
        KillSwitchScope::Account("acct.paper.001".to_owned()),
        KillSwitchScope::Strategy("strategy.paper.001".to_owned()),
        KillSwitchScope::Instrument("inst.us_equity.spy".to_owned()),
    ] {
        assert_eq!(KillSwitchScope::from_key(&scope.as_key()).unwrap(), scope);
    }
    for malformed in [
        "",
        "GLOBAL",
        "everything",
        "instrument:",
        "account:Not Canonical",
    ] {
        assert!(
            KillSwitchScope::from_key(malformed).is_err(),
            "accepted {malformed:?}"
        );
    }
}
