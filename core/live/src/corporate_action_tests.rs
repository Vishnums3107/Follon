// Operator-attested corporate actions in controlled LIVE (delivery state E8.4b-2).
// Included into `tests` so these reuse its broker, secrets and fixtures.

fn live_split(action_id: &str, ratio: &str) -> follon_market_data::CorporateAction {
    follon_market_data::CorporateAction::Split {
        action_id: action_id.to_owned(),
        instrument_id: "inst.us_equity.spy".to_owned(),
        effective_at: "2026-01-05T13:30:00Z".to_owned(),
        ratio: amount(ratio),
    }
}

fn live_dividend(action_id: &str, amount_per_share: &str) -> follon_market_data::CorporateAction {
    follon_market_data::CorporateAction::CashDividend {
        action_id: action_id.to_owned(),
        instrument_id: "inst.us_equity.spy".to_owned(),
        effective_at: "2026-01-05T13:30:00Z".to_owned(),
        amount: amount(amount_per_share),
    }
}

fn live_action(action: follon_market_data::CorporateAction, held: &str) -> LiveCorporateAction {
    LiveCorporateAction {
        action,
        held_quantity: amount(held),
        applied_by: "operator.ops.001".to_owned(),
        applied_at: "2026-01-05T13:31:00Z".to_owned(),
    }
}

/// A connected canary service whose broker filled one SPY order of `quantity`
/// on `side` at 10 with no fee; the broker snapshot agrees with the OMS.
fn live_holding(
    label: &str,
    policy: LiveRiskPolicy,
    side: Side,
    quantity: &str,
) -> (LiveTradingService<TestBroker>, PathBuf) {
    let path = journal_path(label);
    let mut service = test_service_with_policy(LiveRunMode::Canary, &path, policy);
    let order_intent = OrderIntent {
        side,
        quantity: amount(quantity),
        ..intent("LIVE", "intent.live.ca.001")
    };
    let approval = approval_for(&service, &order_intent);
    service
        .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
        .expect("four-eyes approval");
    service
        .connect(&TestSecrets, "operator.approver.001", "2026-01-02T14:30:00Z")
        .expect("managed-secret connection");
    service
        .submit_canary_intent(
            order_intent,
            market(),
            "approval.live.001",
            "2026-01-02T14:30:00Z",
            "operator.requester.001",
        )
        .expect("bounded submission");
    let signed = match side {
        Side::Buy => amount(quantity),
        Side::Sell => Decimal::ZERO.checked_sub(amount(quantity)).unwrap(),
    };
    let broker = service.broker_mut();
    broker.events.push(LiveBrokerEvent::Execution {
        execution_id: "execution.live.ca.001".to_owned(),
        client_order_id: "order-intent.live.ca.001".to_owned(),
        broker_order_id: "broker-order-intent.live.ca.001".to_owned(),
        quantity: amount(quantity),
        price: amount("10"),
        fee: Decimal::ZERO,
        executed_at: "2026-01-02T14:31:00Z".to_owned(),
    });
    broker.snapshot.orders[0].state = OrderState::Filled;
    broker.snapshot.orders[0].filled_quantity = amount(quantity);
    broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
        instrument_id: "inst.us_equity.spy".to_owned(),
        quantity: signed,
    });
    broker.snapshot.cash = amount("1000")
        .checked_sub(signed.checked_mul(amount("10")).unwrap())
        .unwrap();
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
        .expect("broker event synchronization");
    (service, path)
}

/// The broker's own view after it applied an action: its position in SPY.
fn broker_holds(service: &mut LiveTradingService<TestBroker>, quantity: &str) {
    service.broker_mut().snapshot.positions[0].quantity = amount(quantity);
}

fn live_position(service: &LiveTradingService<TestBroker>) -> (String, String) {
    let row = service
        .monitoring_dashboard()
        .positions
        .into_iter()
        .find(|row| row.instrument_id == "inst.us_equity.spy")
        .expect("a SPY position");
    (row.quantity, row.average_cost)
}

/// Everything a refused action must leave exactly as it was.
fn live_books(service: &LiveTradingService<TestBroker>) -> (LiveMonitoringDashboard, Vec<TaxLot>, usize) {
    (
        service.monitoring_dashboard(),
        service.tax_lots("inst.us_equity.spy").to_vec(),
        service.corporate_actions().len(),
    )
}

#[test]
fn a_live_split_waits_for_the_broker_and_then_follows_the_position_and_its_lots() {
    let (mut service, path) = live_holding("ca-split", policy(), Side::Buy, "2");
    let before = live_books(&service);
    // The broker has not split yet, so the OMS must not run ahead of it: a
    // canary could otherwise sell shares the account does not hold.
    let refusal = service
        .apply_corporate_action(live_action(live_split("ca.split.spy.2026", "2"), "2"))
        .unwrap_err();
    assert!(
        refusal.0.contains("the broker holds 2.00000000 of inst.us_equity.spy, not the 4.00000000"),
        "{refusal}"
    );
    assert_eq!(live_books(&service), before);

    broker_holds(&mut service, "4");
    let receipt = service
        .apply_corporate_action(live_action(live_split("ca.split.spy.2026", "2"), "2"))
        .expect("the broker shows the split");
    assert_eq!(
        (receipt.quantity_before, receipt.quantity_after, receipt.cash_delta),
        (amount("2"), amount("4"), Decimal::ZERO)
    );
    assert_eq!(live_position(&service), ("4.00000000".to_owned(), "5.00000000".to_owned()));
    let lots = service.tax_lots("inst.us_equity.spy");
    assert_eq!((lots[0].remaining_quantity, lots[0].unit_cost), (amount("4"), amount("5")));
    assert!(service
        .reconcile("operator.approver.001", "2026-01-05T13:32:00Z")
        .unwrap()
        .is_clean());

    // The audit names the operator, and a restart restores the receipt.
    drop(service);
    let journal = fs::read_to_string(&path).unwrap();
    let applied = journal
        .lines()
        .find(|line| line.contains("\"event_type\":\"live.corporate_action.applied.v1\""))
        .expect("the action's audit record");
    assert!(applied.contains("\"actor\":\"operator.ops.001\""), "{applied}");
    assert!(applied.contains("\"occurred_at\":\"2026-01-05T13:31:00Z\""), "{applied}");
    assert!(
        applied.contains("\"correlation_id\":\"corr-corporate-action-ca.split.spy.2026\""),
        "{applied}"
    );
    let reopened = test_service(LiveRunMode::Canary, &path);
    assert_eq!(reopened.corporate_actions(), [receipt]);
    assert_eq!(live_position(&reopened), ("4.00000000".to_owned(), "5.00000000".to_owned()));
    drop(reopened);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_live_split_needs_a_connected_session() {
    let (service, path) = live_holding("ca-disconnected", policy(), Side::Buy, "2");
    drop(service);
    // A restart never assumes a live broker session.
    let mut reopened = test_service(LiveRunMode::Canary, &path);
    let refusal = reopened
        .apply_corporate_action(live_action(live_split("ca.split.spy.2026", "2"), "2"))
        .unwrap_err();
    assert!(refusal.0.contains("connect the session first"), "{refusal}");
    assert!(reopened.corporate_actions().is_empty());
    drop(reopened);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_live_split_is_refused_while_an_order_works_in_its_instrument() {
    let path = journal_path("ca-working");
    let mut service = test_service(LiveRunMode::Canary, &path);
    let order_intent = intent("LIVE", "intent.live.ca.001");
    let approval = approval_for(&service, &order_intent);
    service
        .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
        .expect("four-eyes approval");
    service
        .connect(&TestSecrets, "operator.approver.001", "2026-01-02T14:30:00Z")
        .expect("managed-secret connection");
    service
        .submit_canary_intent(
            order_intent,
            market(),
            "approval.live.001",
            "2026-01-02T14:30:00Z",
            "operator.requester.001",
        )
        .expect("a working order");
    let refusal = service
        .apply_corporate_action(live_action(live_split("ca.split.spy.2026", "2"), "0"))
        .unwrap_err();
    assert!(
        refusal.0.contains("working order order-intent.live.ca.001 cannot rest across"),
        "{refusal}"
    );
    assert!(service.corporate_actions().is_empty());
    drop(service);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_live_split_that_leaves_a_fraction_of_a_lot_is_refused() {
    let (mut service, path) = live_holding("ca-fraction", policy(), Side::Buy, "3");
    broker_holds(&mut service, "4.5");
    let before = live_books(&service);
    let refusal = service
        .apply_corporate_action(live_action(live_split("ca.split.spy.3-2", "1.5"), "3"))
        .unwrap_err();
    assert!(refusal.0.contains("not a whole number of its lots"), "{refusal}");
    assert_eq!(live_books(&service), before);
    drop(service);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_live_split_rebases_the_cached_mark_and_scales_the_strategy_attribution() {
    let mut risk_policy = policy();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: permissive_portfolio_risk_policy(),
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let (mut service, path) = live_holding("ca-mark", risk_policy, Side::Buy, "2");
    broker_holds(&mut service, "4");
    service
        .apply_corporate_action(live_action(live_split("ca.split.spy.2026", "2"), "2"))
        .expect("the broker shows the split");
    // A later QQQ decision marks SPY from the cache. Rebased, the split moves no
    // exposure: SPY 4 x 5 and the 2 x 10 QQQ candidate. Left at 10 it would read
    // 60, and an unscaled attribution would leave two SPY shares to no strategy.
    let candidate = OrderIntent {
        instrument_id: "inst.us_equity.qqq".to_owned(),
        ..intent("LIVE", "intent.live.ca.qqq")
    };
    let decision = service
        .evaluate_risk(
            &candidate,
            &LiveMarketData {
                instrument_id: "inst.us_equity.qqq".to_owned(),
                mark_price: amount("10"),
                observed_at: "2026-01-05T14:00:00Z".to_owned(),
            },
            "2026-01-05T14:00:00Z",
            false,
        )
        .expect("a later assessment");
    let limits = &decision.evaluated_limits;
    assert!(limits.contains("portfolio_gross_exposure=40.00000000"), "{limits}");
    assert!(!limits.contains("unattributed"), "{limits}");
    drop(service);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_live_split_that_would_round_the_cached_mark_to_nothing_is_refused() {
    let path = journal_path("ca-tiny-mark");
    let mut service = test_service(LiveRunMode::Canary, &path);
    service
        .connect(&TestSecrets, "operator.approver.001", "2026-01-02T14:30:00Z")
        .expect("managed-secret connection");
    service
        .evaluate_risk(
            &intent("LIVE", "intent.live.ca.tiny"),
            &LiveMarketData {
                mark_price: amount("0.00000001"),
                ..market()
            },
            "2026-01-02T14:30:00Z",
            false,
        )
        .expect("an assessment caches its observation");
    let refusal = service
        .apply_corporate_action(live_action(live_split("ca.split.spy.2026", "2"), "0"))
        .unwrap_err();
    assert!(refusal.0.contains("mark of 0.00000001 down to nothing"), "{refusal}");
    assert!(service.corporate_actions().is_empty());
    drop(service);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_live_dividend_credits_a_long_and_debits_a_short_and_reconciles() {
    let (mut long, long_path) = live_holding("ca-dividend-long", policy(), Side::Buy, "2");
    long.broker_mut().snapshot.cash = amount("981");
    let receipt = long
        .apply_corporate_action(live_action(live_dividend("ca.dividend.spy.2026", "0.5"), "2"))
        .unwrap();
    assert_eq!(receipt.cash_delta, amount("1"));
    assert_eq!(long.monitoring_dashboard().internal_cash, "981.00000000");
    assert!(long
        .reconcile("operator.approver.001", "2026-01-05T13:32:00Z")
        .unwrap()
        .is_clean());
    drop(long);
    let _ = fs::remove_file(&long_path);

    let (mut short, short_path) =
        live_holding("ca-dividend-short", policy_permitting_shorts(), Side::Sell, "2");
    short.broker_mut().snapshot.cash = amount("1019");
    let receipt = short
        .apply_corporate_action(live_action(live_dividend("ca.dividend.spy.2026", "0.5"), "-2"))
        .unwrap();
    assert_eq!(receipt.cash_delta, amount("-1"));
    assert!(short
        .reconcile("operator.approver.001", "2026-01-05T13:32:00Z")
        .unwrap()
        .is_clean());
    drop(short);
    let _ = fs::remove_file(&short_path);
}

#[test]
fn a_live_action_applies_only_to_the_stated_position_after_it_takes_effect_and_once() {
    let (mut service, path) = live_holding("ca-checks", policy(), Side::Buy, "2");
    let before = live_books(&service);
    let refusal = service
        .apply_corporate_action(live_action(live_dividend("ca.dividend.spy.2026", "0.5"), "3"))
        .unwrap_err();
    assert!(refusal.0.contains("the account holds 2.00000000 of"), "{refusal}");
    let early = LiveCorporateAction {
        applied_at: "2026-01-05T13:29:59Z".to_owned(),
        ..live_action(live_dividend("ca.dividend.spy.2026", "0.5"), "2")
    };
    let refusal = service.apply_corporate_action(early).unwrap_err();
    assert!(refusal.0.contains("cannot be applied at 2026-01-05T13:29:59Z"), "{refusal}");
    let unattributed = LiveCorporateAction {
        applied_by: "Operator Ops".to_owned(),
        ..live_action(live_dividend("ca.dividend.spy.2026", "0.5"), "2")
    };
    assert!(service.apply_corporate_action(unattributed).is_err());
    assert_eq!(live_books(&service), before);

    let first = service
        .apply_corporate_action(live_action(live_dividend("ca.dividend.spy.2026", "0.5"), "2"))
        .unwrap();
    let after = live_books(&service);
    let retry = LiveCorporateAction {
        applied_at: "2026-01-05T13:45:00Z".to_owned(),
        ..live_action(live_dividend("ca.dividend.spy.2026", "0.5"), "2")
    };
    assert_eq!(service.apply_corporate_action(retry).unwrap(), first);
    let refusal = service
        .apply_corporate_action(live_action(live_dividend("ca.dividend.spy.2026", "0.6"), "2"))
        .unwrap_err();
    assert!(refusal.0.contains("already applied with other terms"), "{refusal}");
    assert_eq!(live_books(&service), after);
    drop(service);
    let _ = fs::remove_file(&path);
}

#[test]
fn a_live_journal_without_an_action_does_not_mention_one() {
    let (service, path) = live_holding("ca-unused", policy(), Side::Buy, "2");
    drop(service);
    assert!(!fs::read_to_string(&path).unwrap().contains("corporate_actions"));
    let _ = fs::remove_file(&path);
}

#[test]
fn a_journaled_live_action_must_follow_from_its_action_and_apply_once() {
    let receipt = LiveCorporateActionReceipt {
        action: live_split("ca.split.spy.2026", "2"),
        applied_by: "operator.ops.001".to_owned(),
        applied_at: "2026-01-05T13:31:00Z".to_owned(),
        quantity_before: amount("2"),
        quantity_after: amount("4"),
        cash_delta: Decimal::ZERO,
    };
    let persisted = PersistentLiveCorporateAction::from(&receipt);
    assert_eq!(
        restore_live_corporate_actions(vec![persisted.clone()]).unwrap(),
        std::slice::from_ref(&receipt)
    );
    let dividend = PersistentLiveCorporateAction::from(&LiveCorporateActionReceipt {
        action: live_dividend("ca.dividend.spy.2026", "0.5"),
        quantity_after: amount("2"),
        cash_delta: amount("1"),
        ..receipt
    });
    assert_eq!(restore_live_corporate_actions(vec![dividend.clone()]).unwrap().len(), 1);
    let refusal = restore_live_corporate_actions(vec![persisted.clone(), persisted.clone()]).unwrap_err();
    assert_eq!(refusal.0, "live audit journal applies one corporate action twice");
    let edited = |document: &PersistentLiveCorporateAction, field: &str, value: &str| {
        let mut json = serde_json::to_value(document).unwrap();
        json[field] = serde_json::Value::String(value.to_owned());
        serde_json::from_value::<PersistentLiveCorporateAction>(json).unwrap()
    };
    for (name, refused) in [
        ("a split that moved cash", edited(&persisted, "cash_delta", "1")),
        ("a split that did not scale", edited(&persisted, "quantity_after", "2")),
        ("a dividend that moved the position", edited(&dividend, "quantity_after", "3")),
        ("a dividend that credited too much", edited(&dividend, "cash_delta", "2")),
        ("an unknown type", edited(&persisted, "action_type", "MERGER")),
        ("an early application", edited(&persisted, "applied_at", "2026-01-05T13:29:00Z")),
    ] {
        assert!(restore_live_corporate_actions(vec![refused]).is_err(), "{name}");
    }
}

#[test]
fn a_live_split_is_refused_while_a_combination_leg_works_in_its_instrument() {
    let path = journal_path("ca-working-combo");
    let combination = combo_intent("intent.live.combo.ca");
    let mut service = canary_ready(&path, &combination);
    service
        .submit_canary_combo_intent(
            combination,
            combo_market(),
            "approval.live.001",
            "2026-01-02T14:30:02Z",
            "operator.requester.001",
        )
        .expect("a working combination");
    let split = follon_market_data::CorporateAction::Split {
        action_id: "ca.split.near.2026".to_owned(),
        instrument_id: "inst.us_option.spy.near".to_owned(),
        effective_at: "2026-01-05T13:30:00Z".to_owned(),
        ratio: amount("2"),
    };
    let refusal = service
        .apply_corporate_action(live_action(split, "0"))
        .unwrap_err();
    assert!(
        refusal
            .0
            .contains("working order combo-order-intent.live.combo.ca cannot rest across"),
        "{refusal}"
    );
    assert!(service.corporate_actions().is_empty());
    drop(service);
    let _ = fs::remove_file(&path);
}
