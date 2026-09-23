fn combo_decimal(value: &str) -> Decimal { Decimal::from_str(value).unwrap() }

fn submit_lifecycle_combo<B: PaperBrokerAdapter>(service: &mut PaperTradingService<B>, suffix: &str) -> String {
    service.submit_combo_intent(
        combo_intent(suffix, "2026-01-02T14:30:00Z"), combo_market("2026-01-02T14:30:00Z"), "2026-01-02T14:30:02Z",
    ).unwrap().order_id.unwrap()
}

fn combo_execution<B: PaperBrokerAdapter>(service: &PaperTradingService<B>, id: &str, receipt: &str, units: &str) -> BrokerComboExecution {
    let order = service.combo_order(id).unwrap();
    let units = combo_decimal(units);
    BrokerComboExecution {
        execution_id: receipt.to_owned(), client_order_id: id.to_owned(),
        broker_order_id: order.broker_order_id.clone().unwrap_or_else(|| "ibkr-paper-combo-00000001".to_owned()), units,
        legs: order.oms.intent.legs.iter().enumerate().map(|(index, leg)| BrokerComboExecutionLeg {
            execution_id: format!("{receipt}-leg-{index}"), instrument_id: leg.instrument_id.clone(), side: leg.side,
            quantity: units.checked_mul(Decimal::from_integer(i64::from(leg.ratio)).unwrap()).unwrap(),
            price: leg.limit_price, fee: combo_decimal("0.1"), executed_at: "2026-01-02T14:31:00Z".to_owned(),
        }).collect(),
    }
}

#[test]
fn combo_partial_and_full_fills_account_every_leg_fee_and_remaining_reservation() {
    let mut service = service_permitting_shorts();
    let id = submit_lifecycle_combo(&mut service, "lifecycle-fill");
    let first = combo_execution(&service, &id, "group-first", "1");
    service.broker.queue_combo_fill(first.clone()).unwrap();
    service.synchronize().unwrap();
    assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::PartiallyFilled);
    assert_eq!(service.combo_order(&id).unwrap().filled_quantity, combo_decimal("1"));
    assert_eq!(service.total_reserved_cash().unwrap(), combo_decimal("7.5"));
    assert_eq!(service.cash, account().initial_cash.checked_sub(combo_decimal("2.7")).unwrap());
    assert_eq!(service.portfolios["inst.us_option.spy.near"].position_snapshot().quantity, combo_decimal("1"));
    assert_eq!(service.portfolios["inst.us_option.spy.far"].position_snapshot().quantity, combo_decimal("-1"));
    assert_eq!(service.tax_lots.short_lots("inst.us_option.spy.far")[0].unit_proceeds, combo_decimal("4.9"));
    assert_eq!(service.strategy_attribution["inst.us_option.spy.far"]["strategy.paper.001"], combo_decimal("-1"));
    assert!(service.reconcile("2026-01-02T14:32:00Z").unwrap().is_clean());

    let before = serde_json::to_string(&service.persistent_state()).unwrap();
    let mut reordered = first;
    reordered.legs.reverse();
    service.broker.pending_events.push_back(BrokerEvent::ComboExecution(reordered));
    service.synchronize().unwrap();
    assert_eq!(serde_json::to_string(&service.persistent_state()).unwrap(), before);

    let last = combo_execution(&service, &id, "group-last", "3");
    service.broker.queue_combo_fill(last).unwrap();
    service.synchronize().unwrap();
    assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::Filled);
    assert_eq!(service.total_reserved_cash().unwrap(), Decimal::ZERO);
    assert_eq!(service.cash, account().initial_cash.checked_sub(combo_decimal("10.4")).unwrap());
    assert!(service.reconcile("2026-01-02T14:33:00Z").unwrap().is_clean());
}

#[test]
fn combo_ratio_two_fills_in_units_not_leg_contracts() {
    let mut service = service_permitting_shorts();
    let mut intent = combo_intent("ratio-fill", "2026-01-02T14:30:00Z");
    intent.legs[0].ratio = 2;
    intent.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(combo_decimal("10"));
    let id = service.submit_combo_intent(intent, combo_market("2026-01-02T14:30:00Z"), "2026-01-02T14:30:02Z").unwrap().order_id.unwrap();
    let fill = combo_execution(&service, &id, "ratio-group", "2");
    service.broker.queue_combo_fill(fill).unwrap();
    service.synchronize().unwrap();
    assert_eq!(service.combo_order(&id).unwrap().filled_quantity, combo_decimal("2"));
    assert_eq!(service.portfolios["inst.us_option.spy.near"].position_snapshot().quantity, combo_decimal("4"));
    assert_eq!(service.total_reserved_cash().unwrap(), combo_decimal("20"));
    assert!(service.reconcile("2026-01-02T14:32:00Z").unwrap().is_clean());
}

#[test]
fn combo_invalid_atomic_evidence_rolls_back_and_blocks_clean_reconciliation() {
    for defect in 0..10 {
        let mut service = service_permitting_shorts();
        let id = submit_lifecycle_combo(&mut service, "bad-group");
        let mut fill = combo_execution(&service, &id, "bad-receipt", "1");
        match defect {
            0 => { fill.legs.pop(); }
            1 => fill.legs[1] = fill.legs[0].clone(),
            2 => fill.legs[1].quantity = combo_decimal("2"),
            3 => fill.units = combo_decimal("0.5"),
            4 => fill.legs[1].side = Side::Buy,
            5 => fill.broker_order_id = "unrecognized-broker".to_owned(),
            6 => fill.legs[1].execution_id = fill.legs[0].execution_id.clone(),
            7 => fill.legs[1].instrument_id = "unapproved.instrument".to_owned(),
            8 => fill = combo_execution(&service, &id, "bad-receipt", "5"),
            // Far leg sorts first and succeeds; the second leg's arithmetic fails.
            9 => fill.legs[0].price = Decimal::from_scaled(i128::MAX),
            _ => unreachable!(),
        }
        service.broker.pending_events.push_back(BrokerEvent::ComboExecution(fill));
        assert!(service.synchronize().is_err(), "defect {defect}");
        assert!(service.portfolios.is_empty(), "defect {defect}");
        assert!(service.execution_ids.is_empty());
        assert!(service.tax_lots.short_lots("inst.us_option.spy.far").is_empty());
        assert!(service.strategy_attribution.is_empty());
        assert_eq!(service.cash, account().initial_cash);
        assert_eq!(service.combo_order(&id).unwrap().filled_quantity, Decimal::ZERO);
        assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::Unknown);
        // Status callbacks cannot erase missing execution evidence.
        service.broker.pending_events.push_back(BrokerEvent::Acknowledged {
            client_order_id: id.clone(), broker_order_id: "ibkr-paper-combo-00000001".to_owned(),
        });
        service.broker.pending_events.push_back(BrokerEvent::Cancelled { client_order_id: id.clone(), reason: "late-cancel".to_owned() });
        service.synchronize().unwrap();
        assert!(service.has_unknown_order());
        let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
        assert!(report.issues.iter().any(|i| i.category == "COMBINATION_EXECUTION_ANOMALY"));
    }
}

#[test]
fn combo_changed_duplicate_and_overlapping_leg_receipts_are_anomalies() {
    for changed_group_id in [false, true] {
        let mut service = service_permitting_shorts();
        let id = submit_lifecycle_combo(&mut service, "duplicate-group");
        let first = combo_execution(&service, &id, "original-group", "1");
        service.broker.queue_combo_fill(first.clone()).unwrap();
        service.synchronize().unwrap();
        let cash = service.cash;
        let mut changed = first;
        if changed_group_id { changed.execution_id = "overlapping-group".to_owned(); }
        else { changed.legs[0].fee = combo_decimal("0.2"); }
        service.broker.pending_events.push_back(BrokerEvent::ComboExecution(changed));
        assert!(service.synchronize().is_err());
        assert_eq!(service.cash, cash);
        assert_eq!(service.combo_order(&id).unwrap().filled_quantity, combo_decimal("1"));
        assert!(service.has_unknown_order());
    }
}

#[test]
fn combo_unidentified_leg_event_is_anomaly_but_later_orders_are_still_applied() {
    let mut service = service_permitting_shorts();
    let id = submit_lifecycle_combo(&mut service, "unidentified-leg");
    let other = submit_lifecycle_combo(&mut service, "later-valid-group");
    service.broker.pending_events.push_back(BrokerEvent::Execution {
        execution_id: "unidentified".to_owned(), client_order_id: id.clone(), broker_order_id: "ibkr-paper-combo-00000001".to_owned(),
        quantity: combo_decimal("1"), price: combo_decimal("7.5"), fee: Decimal::ZERO, executed_at: "2026-01-02T14:31:00Z".to_owned(),
    });
    let fill = combo_execution(&service, &other, "later-valid", "1");
    service.broker.queue_combo_fill(fill).unwrap();
    assert!(service.synchronize().is_err());
    assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::Unknown);
    assert_eq!(service.combo_order(&other).unwrap().filled_quantity, combo_decimal("1"));
}

#[test]
fn combo_cancellation_is_idempotent_and_partial_fill_preserves_pending_cancel() {
    let mut service = service_permitting_shorts();
    let id = submit_lifecycle_combo(&mut service, "cancel-race");
    service.cancel_order(&id).unwrap();
    service.cancel_order(&id).unwrap();
    assert_eq!(service.broker.pending_events.len(), 1);
    let cancel = service.broker.pending_events.pop_front().unwrap();
    let fill = combo_execution(&service, &id, "cancel-partial", "1");
    service.broker.queue_combo_fill(fill).unwrap();
    service.synchronize().unwrap();
    assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::PendingCancel);
    service.broker.pending_events.push_back(cancel);
    service.broker.combos.get_mut(&id).unwrap().state = OrderState::Cancelled;
    service.synchronize().unwrap();
    service.cancel_order(&id).unwrap();
    assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::Cancelled);
    assert_eq!(service.combo_order(&id).unwrap().filled_quantity, combo_decimal("1"));
    assert_eq!(service.total_reserved_cash().unwrap(), Decimal::ZERO);
    assert!(service.reconcile("2026-01-02T14:32:00Z").unwrap().is_clean());
}

#[test]
fn combo_full_fill_wins_cancel_race_and_late_execution_can_resolve_terminal_order() {
    for terminal_first in [false, true] {
        let mut service = service_permitting_shorts();
        let id = submit_lifecycle_combo(&mut service, "terminal-race");
        service.cancel_order(&id).unwrap();
        let cancel = service.broker.pending_events.pop_front().unwrap();
        if terminal_first { service.broker.pending_events.push_back(cancel.clone()); service.synchronize().unwrap(); }
        let fill = combo_execution(&service, &id, "full-race", "4");
        service.broker.queue_combo_fill(fill).unwrap();
        service.broker.pending_events.push_back(cancel);
        service.synchronize().unwrap();
        assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::Filled);
        assert!(service.reconcile("2026-01-02T14:32:00Z").unwrap().is_clean());
    }
}

#[test]
fn combo_cancel_rejection_restores_actual_partial_state_and_expiry_keeps_fills() {
    for terminal in [OrderState::Expired, OrderState::Rejected] {
        let mut service = service_permitting_shorts();
        let id = submit_lifecycle_combo(&mut service, "cancel-rejected");
        let fill = combo_execution(&service, &id, "before-rejection", "1");
        service.broker.queue_combo_fill(fill).unwrap(); service.synchronize().unwrap();
        service.cancel_order(&id).unwrap(); service.broker.pending_events.clear();
        service.broker.pending_events.push_back(BrokerEvent::CancelRejected { client_order_id: id.clone(), reason: "too-late".to_owned() });
        service.synchronize().unwrap();
        assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::PartiallyFilled);
        service.broker.pending_events.push_back(if terminal == OrderState::Expired {
            BrokerEvent::Expired { client_order_id: id.clone(), reason: "expired".to_owned() }
        } else { BrokerEvent::Rejected { client_order_id: id.clone(), reason: "rejected".to_owned() } });
        service.synchronize().unwrap();
        assert_eq!(service.combo_order(&id).unwrap().oms.state, terminal);
        assert_eq!(service.combo_order(&id).unwrap().filled_quantity, combo_decimal("1"));
    }
}

#[test]
fn combo_fill_before_acknowledgement_resolves_unknown_submission() {
    let mut service = service_permitting_shorts();
    let id = submit_lifecycle_combo(&mut service, "before-ack");
    let fill = combo_execution(&service, &id, "early-fill", "1");
    let order = service.combo_orders.get_mut(&id).unwrap();
    order.oms.transition(OrderState::Unknown, "TRANSPORT_UNKNOWN").unwrap();
    order.broker_order_id = None; order.broker_order_versions.clear();
    service.broker.queue_combo_fill(fill).unwrap(); service.synchronize().unwrap();
    service.broker.pending_events.push_back(BrokerEvent::Acknowledged { client_order_id: id.clone(), broker_order_id: "ibkr-paper-combo-00000001".to_owned() });
    service.synchronize().unwrap();
    assert_eq!(service.combo_order(&id).unwrap().oms.state, OrderState::PartiallyFilled);
    assert!(service.reconcile("2026-01-02T14:32:00Z").unwrap().is_clean());
}

#[test]
fn combo_reconciliation_checks_identity_units_state_cash_and_each_leg_position() {
    for defect in 0..5 {
        let mut service = service_permitting_shorts();
        let id = submit_lifecycle_combo(&mut service, "reconcile-evidence");
        let fill = combo_execution(&service, &id, "reconcile-fill", "1");
        service.broker.queue_combo_fill(fill).unwrap(); service.synchronize().unwrap();
        let expected = match defect {
            0 => { service.broker.combos.get_mut(&id).unwrap().broker_order_id = "unexpected-id".to_owned(); "BROKER_ORDER_ID_MISMATCH" }
            1 => { service.broker.combos.get_mut(&id).unwrap().filled_quantity = combo_decimal("2"); "FILLED_QUANTITY_MISMATCH" }
            2 => { service.broker.combos.get_mut(&id).unwrap().state = OrderState::Filled; "ORDER_STATE_MISMATCH" }
            3 => { service.broker.cash = combo_decimal("1"); "CASH_MISMATCH" }
            _ => { service.broker.positions.insert("inst.us_option.spy.far".to_owned(), Decimal::ZERO); "POSITION_QUANTITY_MISMATCH" }
        };
        let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
        assert!(report.issues.iter().any(|issue| issue.category == expected), "{defect}: {:?}", report.issues);
        assert_eq!(service.combo_order(&id).unwrap().filled_quantity, combo_decimal("1"));
    }
}

#[test]
fn combo_fills_and_short_tax_lots_recover_without_reapplying_duplicates() {
    let path = std::env::temp_dir().join(format!("follon-combo-fill-recovery-{}.ndjson", std::process::id()));
    let _ = fs::remove_file(&path);
    let mut service = PaperTradingService::open_durable(account(), policy_permitting_shorts(), KillSwitchRegistry::new("paper-kills-v1").unwrap(), IbkrPaperAdapter::new(&account()).unwrap(), &path).unwrap();
    let id = submit_lifecycle_combo(&mut service, "durable-fill");
    let fill = combo_execution(&service, &id, "durable-receipt", "1");
    service.broker.queue_combo_fill(fill.clone()).unwrap(); service.synchronize().unwrap();
    let cash = service.cash;
    service.journal.take();
    let mut reopened = PaperTradingService::open_durable(account(), policy_permitting_shorts(), KillSwitchRegistry::new("paper-kills-v1").unwrap(), service.broker, &path).unwrap();
    assert_eq!(reopened.combo_order(&id).unwrap().filled_quantity, combo_decimal("1"));
    assert_eq!(reopened.total_reserved_cash().unwrap(), combo_decimal("7.5"));
    assert_eq!(reopened.tax_lots.short_lots("inst.us_option.spy.far")[0].remaining_quantity, combo_decimal("1"));
    reopened.broker.pending_events.push_back(BrokerEvent::ComboExecution(fill));
    reopened.reconnect_and_reconcile("2026-01-02T14:32:00Z").unwrap();
    assert_eq!(reopened.cash, cash);
    assert_eq!(reopened.execution_ids.len(), 3);
    let remainder = combo_execution(&reopened, &id, "durable-remainder", "3");
    reopened.broker.queue_combo_fill(remainder).unwrap(); reopened.synchronize().unwrap();
    assert!(reopened.reconcile("2026-01-02T14:33:00Z").unwrap().is_clean());
    drop(reopened); fs::remove_file(path).unwrap();
}

#[test]
fn combo_fractional_intents_are_refused_and_short_permission_is_recovery_bound() {
    let mut service = service_permitting_shorts();
    let mut intent = combo_intent("fractional-combo", "2026-01-02T14:30:00Z");
    intent.combo_quantity = combo_decimal("0.5");
    assert!(service.submit_combo_intent(intent, combo_market("2026-01-02T14:30:00Z"), "2026-01-02T14:30:02Z").is_err());
    assert!(service.combo_orders.is_empty());
    let state = service.persistent_state();
    service.risk_policy.short_exposure.as_mut().unwrap().max_short_quantity = combo_decimal("999");
    assert!(service.restore(state).is_err());
}
