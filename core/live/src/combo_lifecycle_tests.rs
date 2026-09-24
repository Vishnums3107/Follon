fn submit_lifecycle_combo(
    service: &mut LiveTradingService<TestBroker>,
    intent: ComboIntent,
    approval_id: &str,
) -> String {
    let order_id = OmsComboOrder::order_id_for(&intent.intent_id);
    service
        .submit_canary_combo_intent(
            intent,
            combo_market(),
            approval_id,
            "2026-01-02T14:30:02Z",
            "operator.requester.001",
        )
        .expect("live combination submission");
    order_id
}

fn lifecycle_execution(
    service: &LiveTradingService<TestBroker>,
    order_id: &str,
    execution_id: &str,
    units: &str,
) -> LiveBrokerComboExecution {
    let order = service.combo_order(order_id).expect("live combination");
    let units = amount(units);
    LiveBrokerComboExecution {
        execution_id: execution_id.to_owned(),
        client_order_id: order_id.to_owned(),
        broker_order_id: order
            .broker_order_id
            .clone()
            .expect("acknowledged live broker identity"),
        units,
        legs: order
            .oms
            .intent
            .legs
            .iter()
            .enumerate()
            .map(|(index, leg)| LiveBrokerComboExecutionLeg {
                execution_id: format!("{execution_id}-leg-{index}"),
                instrument_id: leg.instrument_id.clone(),
                side: leg.side,
                quantity: units
                    .checked_mul(Decimal::from_integer(i64::from(leg.ratio)).expect("ratio"))
                    .expect("leg quantity"),
                price: leg.limit_price,
                fee: amount("0.1"),
                executed_at: "2026-01-02T14:31:00Z".to_owned(),
            })
            .collect(),
    }
}

fn set_snapshot_order_state(
    service: &mut LiveTradingService<TestBroker>,
    order_id: &str,
    state: OrderState,
) {
    service
        .broker_mut()
        .snapshot
        .orders
        .iter_mut()
        .find(|order| order.client_order_id == order_id)
        .expect("broker snapshot order")
        .state = state;
}

#[test]
fn live_combo_partial_and_full_fills_account_every_leg_and_remaining_reservation() {
    let path = journal_path("combo-lifecycle-fill");
    let intent = combo_intent("intent.live.combo.lifecycle.fill");
    let mut service = canary_ready(&path, &intent);
    let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");

    let first = lifecycle_execution(&service, &order_id, "live-group-first", "1");
    service
        .broker_mut()
        .queue_combo_fill(first.clone())
        .expect("queue first group");
    assert_eq!(
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
            .expect("first synchronization"),
        1
    );
    let order = service.combo_order(&order_id).expect("combination");
    assert_eq!(order.oms.state, OrderState::PartiallyFilled);
    assert_eq!(order.filled_quantity, amount("1"));
    assert_eq!(service.total_reserved_cash().expect("reserved"), amount("2.5"));
    assert_eq!(service.cash, amount("997.3"));
    assert_eq!(
        service.portfolios["inst.us_option.spy.near"]
            .position_snapshot()
            .quantity,
        amount("1")
    );
    assert_eq!(
        service.portfolios["inst.us_option.spy.far"]
            .position_snapshot()
            .quantity,
        amount("-1")
    );
    assert_eq!(
        service.tax_lots.short_lots("inst.us_option.spy.far")[0].unit_proceeds,
        amount("4.9")
    );
    assert_eq!(
        service.strategy_attribution["inst.us_option.spy.far"]["strategy.live.001"],
        amount("-1")
    );
    assert!(service
        .reconcile("operator.approver.001", "2026-01-02T14:31:02Z")
        .expect("partial reconciliation")
        .is_clean());

    let before = serde_json::to_string(&service.persistent_state()).expect("state");
    let mut reordered = first;
    reordered.legs.reverse();
    service
        .broker_mut()
        .events
        .push(LiveBrokerEvent::ComboExecution(reordered));
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:03Z")
        .expect("exact replay");
    assert_eq!(
        serde_json::to_string(&service.persistent_state()).expect("state"),
        before
    );

    let final_group = lifecycle_execution(&service, &order_id, "live-group-final", "1");
    service
        .broker_mut()
        .queue_combo_fill(final_group)
        .expect("queue final group");
    set_snapshot_order_state(&mut service, &order_id, OrderState::Filled);
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:04Z")
        .expect("final synchronization");
    assert_eq!(
        service.combo_order(&order_id).expect("combination").oms.state,
        OrderState::Filled
    );
    assert_eq!(service.total_reserved_cash().expect("reserved"), Decimal::ZERO);
    assert_eq!(service.cash, amount("994.6"));
    assert!(service
        .reconcile("operator.approver.001", "2026-01-02T14:31:05Z")
        .expect("full reconciliation")
        .is_clean());
    let _ = fs::remove_file(path);
}

#[test]
fn live_combo_cash_overdraft_records_incident_and_blocks_later_canaries() {
    let path = journal_path("combo-cash-overdraft");
    let intent = combo_intent("intent.live.combo.cash.overdraft");
    let mut service = canary_ready(&path, &intent);
    let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
    service.cash = amount("1");

    let execution = lifecycle_execution(&service, &order_id, "live-group-overdraft", "1");
    service
        .broker_mut()
        .queue_combo_fill(execution)
        .expect("queue overdrawing fill");
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
        .expect("apply complete atomic fill");

    assert!(service.cash < Decimal::ZERO);
    assert!(service.incidents.values().any(|incident| {
        incident.unexplained()
            && incident.issue.category == "LIVE_CASH_OVERDRAFT"
            && incident.issue.subject == service.account.account_id
    }));
    let mut later_market = combo_market();
    for mark in &mut later_market.marks {
        mark.observed_at = "2026-01-02T14:31:02Z".to_owned();
    }
    let decision = service
        .evaluate_combo_risk(
            &combo_intent("intent.live.combo.after.overdraft"),
            &later_market,
            "2026-01-02T14:31:02Z",
            false,
        )
        .expect("post-overdraft assessment");
    assert!(decision
        .reason_codes
        .contains(&"UNRESOLVED_INCIDENTS_REQUIRE_REVIEW".to_owned()));
    let _ = fs::remove_file(path);
}

#[test]
fn live_combo_invalid_atomic_evidence_rolls_back_records_incident_and_blocks_canary() {
    let path = journal_path("combo-invalid-evidence");
    let first_intent = combo_intent("intent.live.combo.invalid.first");
    let mut service = canary_ready(&path, &first_intent);
    let first_id = submit_lifecycle_combo(&mut service, first_intent, "approval.live.001");

    let second_intent = combo_intent("intent.live.combo.invalid.second");
    let mut second_approval = combo_approval_for(&service, &second_intent);
    second_approval.approval_id = "approval.live.002".to_owned();
    service
        .register_approval(
            second_approval,
            "2026-01-02T14:30:03Z",
            "operator.approver.001",
        )
        .expect("second approval");
    let second_id = submit_lifecycle_combo(&mut service, second_intent, "approval.live.002");

    let invalid = lifecycle_execution(&service, &first_id, "live-group-overfill", "3");
    service
        .broker_mut()
        .events
        .push(LiveBrokerEvent::ComboExecution(invalid));
    let valid = lifecycle_execution(&service, &second_id, "live-group-later-valid", "2");
    service
        .broker_mut()
        .queue_combo_fill(valid)
        .expect("queue later valid evidence");
    set_snapshot_order_state(&mut service, &second_id, OrderState::Filled);

    let error = service
        .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
        .expect_err("overfill must be refused");
    assert!(error.0.contains("exceeds approved units"));
    let first = service.combo_order(&first_id).expect("first combination");
    assert_eq!(first.filled_quantity, Decimal::ZERO);
    assert_eq!(first.oms.state, OrderState::Unknown);
    assert_eq!(
        service.combo_order(&second_id).expect("second combination").filled_quantity,
        amount("2")
    );
    // Only the valid second group reached accounting; no prefix of the bad
    // atomic group leaked through before its rejection.
    assert_eq!(service.cash, amount("994.8"));
    assert_eq!(service.execution_ids.len(), 3);
    assert!(service.incidents.values().any(|incident| {
        incident.unexplained()
            && incident.issue.category == "COMBINATION_EXECUTION_ANOMALY"
            && incident.issue.subject == first_id
    }));
    let decision = service
        .evaluate_combo_risk(
            &combo_intent("intent.live.combo.after.incident"),
            &combo_market(),
            "2026-01-02T14:30:59Z",
            false,
        )
        .expect("post-incident assessment");
    assert!(decision
        .reason_codes
        .contains(&"UNRESOLVED_INCIDENTS_REQUIRE_REVIEW".to_owned()));
    let report = service
        .reconcile("operator.approver.001", "2026-01-02T14:31:03Z")
        .expect("incident reconciliation");
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.category == "COMBINATION_EXECUTION_ANOMALY"));
    let _ = fs::remove_file(path);
}

#[test]
fn live_combo_changed_duplicate_and_overlapping_receipts_are_incidents() {
    for overlap_leg in [false, true] {
        let path = journal_path(if overlap_leg {
            "combo-overlapping-receipt"
        } else {
            "combo-changed-receipt"
        });
        let intent = combo_intent("intent.live.combo.duplicate");
        let mut service = canary_ready(&path, &intent);
        let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
        let first = lifecycle_execution(&service, &order_id, "live-group-original", "1");
        service
            .broker_mut()
            .queue_combo_fill(first.clone())
            .expect("queue original");
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
            .expect("original synchronization");
        let cash = service.cash;
        let mut changed = first;
        if overlap_leg {
            changed.execution_id = "live-group-overlap".to_owned();
        } else {
            changed.legs[0].fee = amount("0.2");
        }
        service
            .broker_mut()
            .events
            .push(LiveBrokerEvent::ComboExecution(changed));
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:02Z")
            .expect_err("changed or overlapping evidence must fail");
        assert_eq!(service.cash, cash);
        assert_eq!(
            service.combo_order(&order_id).expect("combination").filled_quantity,
            amount("1")
        );
        assert!(service.unresolved_incident_count() > 0);
        let _ = fs::remove_file(path);
    }
}

#[test]
fn live_plain_execution_refuses_a_combination_receipt_identity() {
    let path = journal_path("combo-cross-shape-receipt");
    let combo = combo_intent("intent.live.combo.cross.shape");
    let mut service = canary_ready(&path, &combo);
    let combo_order_id = submit_lifecycle_combo(&mut service, combo, "approval.live.001");
    let combo_execution = lifecycle_execution(
        &service,
        &combo_order_id,
        "live-group-cross-shape",
        "1",
    );
    service
        .broker_mut()
        .queue_combo_fill(combo_execution)
        .expect("queue combination fill");
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
        .expect("apply combination fill");

    let plain = intent("LIVE", "intent.live.combo.cross.shape.plain");
    let mut approval = approval_for(&service, &plain);
    approval.approval_id = "approval.live.plain.001".to_owned();
    service
        .register_approval(
            approval,
            "2026-01-02T14:31:02Z",
            "operator.approver.001",
        )
        .expect("plain approval");
    let mut plain_market = market();
    plain_market.observed_at = "2026-01-02T14:31:02Z".to_owned();
    service
        .submit_canary_intent(
            plain,
            plain_market,
            "approval.live.plain.001",
            "2026-01-02T14:31:03Z",
            "operator.requester.001",
        )
        .expect("plain submission");
    let plain_order_id = "order-intent.live.combo.cross.shape.plain";
    let plain_broker_id = service.orders[plain_order_id]
        .broker_order_id
        .clone()
        .expect("plain broker identity");
    let cash = service.cash;
    service.broker_mut().events.push(LiveBrokerEvent::Execution {
        execution_id: "live-group-cross-shape".to_owned(),
        client_order_id: plain_order_id.to_owned(),
        broker_order_id: plain_broker_id,
        quantity: amount("1"),
        price: amount("10"),
        fee: Decimal::ZERO,
        executed_at: "2026-01-02T14:31:04Z".to_owned(),
    });
    let error = service
        .synchronize("operator.approver.001", "2026-01-02T14:31:05Z")
        .expect_err("combination receipt identity must not become a plain replay");
    assert!(error.0.contains("belongs to combination evidence"));
    assert_eq!(service.orders[plain_order_id].filled_quantity, Decimal::ZERO);
    assert_eq!(service.cash, cash);
    let _ = fs::remove_file(path);
}

#[test]
fn repeated_live_combo_anomalies_reconcile_with_unique_incidents_and_reopen() {
    let path = journal_path("combo-repeated-anomalies");
    let intent = combo_intent("intent.live.combo.repeated.anomaly");
    let mut service = canary_ready(&path, &intent);
    let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
    let original = lifecycle_execution(&service, &order_id, "live-group-repeated", "1");
    service
        .broker_mut()
        .queue_combo_fill(original.clone())
        .expect("queue original");
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
        .expect("original synchronization");

    let mut changed = original.clone();
    changed.legs[0].fee = amount("0.2");
    service
        .broker_mut()
        .events
        .push(LiveBrokerEvent::ComboExecution(changed));
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:02Z")
        .expect_err("changed receipt must fail");

    let mut overlapping = original;
    overlapping.execution_id = "live-group-repeated-overlap".to_owned();
    service
        .broker_mut()
        .events
        .push(LiveBrokerEvent::ComboExecution(overlapping));
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:03Z")
        .expect_err("overlapping receipt must fail");
    assert_eq!(
        service
            .incidents
            .values()
            .filter(|incident| {
                incident.unexplained()
                    && incident.issue.category == "COMBINATION_EXECUTION_ANOMALY"
                    && incident.issue.subject == order_id
            })
            .count(),
        1
    );

    let report = service
        .reconcile("operator.approver.001", "2026-01-02T14:31:04Z")
        .expect("reconciliation");
    let incident_ids: BTreeSet<_> = report
        .issues
        .iter()
        .map(|issue| issue.incident_id.as_str())
        .collect();
    assert_eq!(incident_ids.len(), report.issues.len());
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.category == "COMBINATION_EXECUTION_ANOMALY"));
    drop(service);

    let account = account();
    let policy = policy_permitting_shorts();
    let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("switches");
    let activation = activation(LiveRunMode::Canary, &account, &policy, &switches);
    let reopened = LiveTradingService::open_durable(
        account,
        policy,
        activation,
        switches,
        TestBroker::new(),
        &path,
        "2026-01-02T14:31:05Z",
    )
    .expect("reopen after repeated anomalies");
    assert_eq!(
        reopened
            .combo_order(&order_id)
            .expect("recovered combination")
            .oms
            .state,
        OrderState::Unknown
    );
    assert!(reopened.unresolved_incident_count() >= 1);
    drop(reopened);
    let _ = fs::remove_file(path);
}

#[test]
fn live_combo_cancellation_is_idempotent_and_preserves_fill_races() {
    let path = journal_path("combo-cancel-race");
    let intent = combo_intent("intent.live.combo.cancel.race");
    let mut service = canary_ready(&path, &intent);
    let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
    service
        .cancel_order(
            &order_id,
            "operator.approver.001",
            "2026-01-02T14:31:00Z",
        )
        .expect("cancel");
    service
        .cancel_order(
            &order_id,
            "operator.approver.001",
            "2026-01-02T14:31:00Z",
        )
        .expect("idempotent cancel");
    assert_eq!(service.broker_mut().cancelled, 1);

    let partial = lifecycle_execution(&service, &order_id, "live-group-cancel-partial", "1");
    service
        .broker_mut()
        .queue_combo_fill(partial)
        .expect("queue partial fill");
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
        .expect("partial during cancel");
    assert_eq!(
        service.combo_order(&order_id).expect("combination").oms.state,
        OrderState::PendingCancel
    );

    service.broker_mut().events.push(LiveBrokerEvent::Cancelled {
        client_order_id: order_id.clone(),
        reason: "broker-cancelled".to_owned(),
    });
    set_snapshot_order_state(&mut service, &order_id, OrderState::Cancelled);
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:02Z")
        .expect("cancel confirmation");
    let order = service.combo_order(&order_id).expect("combination");
    assert_eq!(order.oms.state, OrderState::Cancelled);
    assert_eq!(order.filled_quantity, amount("1"));
    assert_eq!(service.total_reserved_cash().expect("reserved"), Decimal::ZERO);
    assert!(service
        .reconcile("operator.approver.001", "2026-01-02T14:31:03Z")
        .expect("cancel reconciliation")
        .is_clean());
    let _ = fs::remove_file(path);
}

#[test]
fn live_combo_cancel_rejection_and_full_fill_races_restore_the_evidenced_state() {
    let rejection_path = journal_path("combo-cancel-rejected");
    let intent = combo_intent("intent.live.combo.cancel.rejected");
    let mut service = canary_ready(&rejection_path, &intent);
    let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
    let partial = lifecycle_execution(&service, &order_id, "live-group-before-reject", "1");
    service
        .broker_mut()
        .queue_combo_fill(partial)
        .expect("queue partial");
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
        .expect("partial fill");
    service
        .cancel_order(
            &order_id,
            "operator.approver.001",
            "2026-01-02T14:31:02Z",
        )
        .expect("cancel");
    service
        .broker_mut()
        .events
        .push(LiveBrokerEvent::CancelRejected {
            client_order_id: order_id.clone(),
            reason: "too-late".to_owned(),
        });
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:03Z")
        .expect("cancel rejection");
    assert_eq!(
        service.combo_order(&order_id).expect("combination").oms.state,
        OrderState::PartiallyFilled
    );
    service.broker_mut().events.push(LiveBrokerEvent::Expired {
        client_order_id: order_id.clone(),
        reason: "expired".to_owned(),
    });
    set_snapshot_order_state(&mut service, &order_id, OrderState::Expired);
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:04Z")
        .expect("expiry");
    assert_eq!(
        service.combo_order(&order_id).expect("combination").filled_quantity,
        amount("1")
    );
    let _ = fs::remove_file(rejection_path);

    for terminal_first in [false, true] {
        let path = journal_path(if terminal_first {
            "combo-terminal-before-fill"
        } else {
            "combo-fill-before-terminal"
        });
        let intent = combo_intent("intent.live.combo.full.cancel.race");
        let mut service = canary_ready(&path, &intent);
        let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
        service
            .cancel_order(
                &order_id,
                "operator.approver.001",
                "2026-01-02T14:31:00Z",
            )
            .expect("cancel");
        let cancelled = LiveBrokerEvent::Cancelled {
            client_order_id: order_id.clone(),
            reason: "broker-cancelled".to_owned(),
        };
        if terminal_first {
            service.broker_mut().events.push(cancelled.clone());
            set_snapshot_order_state(&mut service, &order_id, OrderState::Cancelled);
            service
                .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
                .expect("terminal first");
        }
        let full = lifecycle_execution(&service, &order_id, "live-group-full-race", "2");
        service
            .broker_mut()
            .queue_combo_fill(full)
            .expect("queue full fill");
        if !terminal_first {
            service.broker_mut().events.push(cancelled);
        }
        set_snapshot_order_state(&mut service, &order_id, OrderState::Filled);
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:02Z")
            .expect("resolve race");
        assert_eq!(
            service.combo_order(&order_id).expect("combination").oms.state,
            OrderState::Filled
        );
        assert!(service
            .reconcile("operator.approver.001", "2026-01-02T14:31:03Z")
            .expect("race reconciliation")
            .is_clean());
        let _ = fs::remove_file(path);
    }
}

#[test]
fn live_combo_ratio_two_fills_track_units_not_leg_contracts() {
    let path = journal_path("combo-ratio-two-fill");
    let mut intent = combo_intent("intent.live.combo.ratio.two");
    intent.legs[0].ratio = 2;
    intent.legs[0].limit_price = amount("6");
    intent.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(amount("7"));
    let mut service = test_service_with_policy(
        LiveRunMode::Canary,
        &path,
        policy_permitting_shorts(),
    );
    let approval = combo_approval_for(&service, &intent);
    service
        .register_approval(
            approval,
            "2026-01-02T14:30:00Z",
            "operator.approver.001",
        )
        .expect("approval");
    service
        .connect(
            &TestSecrets,
            "operator.approver.001",
            "2026-01-02T14:30:00Z",
        )
        .expect("connection");
    let mut market = combo_market();
    market.marks[0].mark_price = amount("6");
    let order_id = OmsComboOrder::order_id_for(&intent.intent_id);
    service
        .submit_canary_combo_intent(
            intent,
            market,
            "approval.live.001",
            "2026-01-02T14:30:02Z",
            "operator.requester.001",
        )
        .expect("submission");
    let fill = lifecycle_execution(&service, &order_id, "live-group-ratio-two", "1");
    service
        .broker_mut()
        .queue_combo_fill(fill)
        .expect("queue ratio fill");
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
        .expect("synchronize");
    assert_eq!(
        service.combo_order(&order_id).expect("combination").filled_quantity,
        amount("1")
    );
    assert_eq!(
        service.portfolios["inst.us_option.spy.near"]
            .position_snapshot()
            .quantity,
        amount("2")
    );
    assert_eq!(service.total_reserved_cash().expect("reserved"), amount("7"));
    assert!(service
        .reconcile("operator.approver.001", "2026-01-02T14:31:01Z")
        .expect("reconcile")
        .is_clean());
    let _ = fs::remove_file(path);
}

#[test]
fn live_combo_reconciliation_checks_identity_units_state_cash_and_each_leg_position() {
    let expected = [
        "MISSING_BROKER_ORDER",
        "BROKER_ORDER_ID_MISMATCH",
        "FILLED_QUANTITY_MISMATCH",
        "ORDER_STATE_MISMATCH",
        "CASH_MISMATCH",
        "POSITION_QUANTITY_MISMATCH",
    ];
    for (defect, expected_category) in expected.into_iter().enumerate() {
        let path = journal_path(&format!("combo-reconciliation-{defect}"));
        let intent = combo_intent("intent.live.combo.reconciliation");
        let mut service = canary_ready(&path, &intent);
        let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
        let partial = lifecycle_execution(&service, &order_id, "live-group-reconcile", "1");
        service
            .broker_mut()
            .queue_combo_fill(partial)
            .expect("queue fill");
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
            .expect("synchronize");
        match defect {
            0 => service.broker_mut().snapshot.orders.clear(),
            1 => {
                service.broker_mut().snapshot.orders[0].broker_order_id =
                    "broker-unrecognized-combo".to_owned()
            }
            2 => service.broker_mut().snapshot.orders[0].filled_quantity = amount("2"),
            3 => service.broker_mut().snapshot.orders[0].state = OrderState::Filled,
            4 => service.broker_mut().snapshot.cash = Decimal::ZERO,
            5 => service.broker_mut().snapshot.positions[1].quantity = Decimal::ZERO,
            _ => unreachable!(),
        }
        let report = service
            .reconcile("operator.approver.001", "2026-01-02T14:31:02Z")
            .expect("reconciliation");
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.category == expected_category),
            "defect {defect}: {:?}",
            report.issues
        );
        assert!(!report
            .issues
            .iter()
            .any(|issue| issue.category == "UNEXPECTED_BROKER_ORDER"));
        let _ = fs::remove_file(path);
    }
}

#[test]
fn live_combo_receipts_and_short_lots_survive_reopen_without_double_application() {
    let path = journal_path("combo-fill-recovery");
    let intent = combo_intent("intent.live.combo.fill.recovery");
    let mut service = canary_ready(&path, &intent);
    let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
    let first = lifecycle_execution(&service, &order_id, "live-group-durable", "1");
    service
        .broker_mut()
        .queue_combo_fill(first.clone())
        .expect("queue fill");
    service
        .synchronize("operator.approver.001", "2026-01-02T14:31:01Z")
        .expect("synchronize");
    let cash = service.cash;
    let broker = std::mem::replace(service.broker_mut(), TestBroker::new());
    drop(service);

    let account = account();
    let policy = policy_permitting_shorts();
    let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("switches");
    let activation = activation(LiveRunMode::Canary, &account, &policy, &switches);
    let mut reopened = LiveTradingService::open_durable(
        account,
        policy,
        activation,
        switches,
        broker,
        &path,
        "2026-01-02T14:31:02Z",
    )
    .expect("reopen");
    assert_eq!(
        reopened.combo_order(&order_id).expect("combination").filled_quantity,
        amount("1")
    );
    assert_eq!(reopened.total_reserved_cash().expect("reserved"), amount("2.5"));
    assert_eq!(
        reopened.tax_lots.short_lots("inst.us_option.spy.far")[0].remaining_quantity,
        amount("1")
    );
    reopened
        .broker_mut()
        .events
        .push(LiveBrokerEvent::ComboExecution(first));
    reopened
        .reconnect_and_reconcile(
            &TestSecrets,
            "operator.approver.001",
            "2026-01-02T14:31:03Z",
        )
        .expect("reconnect and replay");
    assert_eq!(reopened.cash, cash);
    assert_eq!(reopened.execution_ids.len(), 3);

    let remainder = lifecycle_execution(&reopened, &order_id, "live-group-remainder", "1");
    reopened
        .broker_mut()
        .queue_combo_fill(remainder)
        .expect("queue remainder");
    set_snapshot_order_state(&mut reopened, &order_id, OrderState::Filled);
    reopened
        .synchronize("operator.approver.001", "2026-01-02T14:31:04Z")
        .expect("finish fill");
    assert!(reopened
        .reconcile("operator.approver.001", "2026-01-02T14:31:05Z")
        .expect("final reconciliation")
        .is_clean());
    drop(reopened);
    let _ = fs::remove_file(path);
}

#[test]
fn live_combo_cancel_transport_failure_is_unknown_disconnected_and_durable() {
    let path = journal_path("combo-cancel-transport");
    let intent = combo_intent("intent.live.combo.cancel.transport");
    let mut service = canary_ready(&path, &intent);
    let order_id = submit_lifecycle_combo(&mut service, intent, "approval.live.001");
    let dashboard = service.monitoring_dashboard();
    assert_eq!(dashboard.working_orders, 1);
    assert_eq!(dashboard.unknown_orders, 0);
    service.broker_mut().fail_cancel = true;
    assert!(service
        .cancel_order(
            &order_id,
            "operator.approver.001",
            "2026-01-02T14:31:00Z",
        )
        .is_err());
    assert_eq!(
        service.combo_order(&order_id).expect("combination").oms.state,
        OrderState::Unknown
    );
    assert!(!service.broker_connected);
    assert!(service.has_unknown_order());
    let dashboard = service.monitoring_dashboard();
    assert_eq!(dashboard.working_orders, 1);
    assert_eq!(dashboard.unknown_orders, 1);
    let _ = fs::remove_file(path);
}
