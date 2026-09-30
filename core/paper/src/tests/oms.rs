//! OMS execution, broker registry, broker evidence and reconciliation.

use super::*;

fn broker_registry() -> PaperBrokerRegistry {
    let account = account();
    let second_account = PaperAccount {
        account_id: "acct.paper.002".to_owned(),
        currency: "USD".to_owned(),
        initial_cash: decimal("initial cash", "25000").unwrap(),
        environment: "PAPER".to_owned(),
    };
    let mut registry = PaperBrokerRegistry::new();
    registry
        .register(
            PaperBrokerRoute {
                account_id: second_account.account_id.clone(),
                adapter_id: "adapter.ibkr.paper.002".to_owned(),
                venue_id: "venue.ibkr.paper".to_owned(),
                environment: "PAPER".to_owned(),
            },
            Box::new(IbkrPaperAdapter::new(&second_account).unwrap()),
        )
        .unwrap();
    registry
        .register(
            PaperBrokerRoute {
                account_id: account.account_id.clone(),
                adapter_id: "adapter.ibkr.paper.001".to_owned(),
                venue_id: "venue.ibkr.paper".to_owned(),
                environment: "PAPER".to_owned(),
            },
            Box::new(IbkrPaperAdapter::new(&account).unwrap()),
        )
        .unwrap();
    registry
}

fn registry_service() -> PaperTradingService<PaperBrokerRegistry> {
    PaperTradingService::new(
        account(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        broker_registry(),
    )
    .unwrap()
}

struct CashAndPositionMismatchBroker;

impl PaperBrokerAdapter for CashAndPositionMismatchBroker {
    fn submit(&mut self, _request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        Err(PaperError("not used by reconciliation test".to_owned()))
    }

    fn cancel(&mut self, _request: &BrokerCancelRequest) -> Result<(), PaperError> {
        Err(PaperError("not used by reconciliation test".to_owned()))
    }

    fn poll(&mut self, _account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        Ok(Vec::new())
    }

    fn snapshot(&mut self, _account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        Ok(BrokerAccountSnapshot {
            orders: Vec::new(),
            positions: vec![BrokerPositionSnapshot {
                instrument_id: "inst.us_equity.spy".to_owned(),
                quantity: decimal("quantity", "1").unwrap(),
            }],
            cash: Decimal::ZERO,
        })
    }

    fn reconnect(&mut self, _account_id: &str) -> Result<(), PaperError> {
        Ok(())
    }
}

#[test]
fn paper_oms_applies_one_execution_and_reconciles_independent_state() {
    let mut service = service();
    let submitted = service
        .submit_intent(
            intent("intent-paper-001", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert_eq!(submitted.state, Some(OrderState::Acknowledged));
    let order_id = submitted.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1").unwrap(),
            decimal("price", "100").unwrap(),
            decimal("fee", "0.10").unwrap(),
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);
    assert_eq!(
        service.order(&order_id).unwrap().oms.state,
        OrderState::Filled
    );
    let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
    assert!(report.is_clean());
    let mut wrong_calendar = paper_session("2026-01-02");
    wrong_calendar.calendar_id = "cal.other.v1".to_owned();
    let configured_session = paper_session("2026-01-02");
    let calendar = paper_calendar(std::slice::from_ref(&configured_session));
    assert!(service
        .record_paper_session(&wrong_calendar, &report, &calendar)
        .is_err());
    service
        .record_paper_session(&configured_session, &report, &calendar)
        .unwrap();
    let dashboard = service.dashboard();
    assert_eq!(dashboard.working_orders, 0);
    assert_eq!(dashboard.positions[0].quantity, "1.00000000");
    assert_eq!(dashboard.clean_paper_days, 1);
}

#[test]
fn paper_broker_registry_keeps_submission_behind_oms_risk() {
    let mut service = registry_service();
    assert_eq!(
        service
            .broker_mut()
            .routes()
            .into_iter()
            .map(|route| route.account_id)
            .collect::<Vec<_>>(),
        vec!["acct.paper.001".to_owned(), "acct.paper.002".to_owned()]
    );
    let first_route_fingerprint = service
        .broker_mut()
        .configuration_fingerprint("acct.paper.001")
        .unwrap();
    let second_route_fingerprint = service
        .broker_mut()
        .configuration_fingerprint("acct.paper.002")
        .unwrap();
    assert_ne!(first_route_fingerprint, second_route_fingerprint);
    let submitted = service
        .submit_intent(
            intent("intent-paper-registry", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(submitted.decision.approved);
    assert_eq!(submitted.state, Some(OrderState::Acknowledged));
    assert_eq!(service.synchronize().unwrap(), 1);
}

#[test]
fn paper_broker_registry_is_account_isolated_and_refuses_invalid_routes() {
    let mut registry = broker_registry();
    let request = BrokerOrderRequest {
        client_order_id: "shared.client.order".to_owned(),
        account_id: "acct.paper.001".to_owned(),
        instrument_id: "inst.us_equity.spy".to_owned(),
        side: Side::Buy,
        quantity: decimal("quantity", "1").unwrap(),
        limit_price: None,
    };
    assert!(matches!(
        registry.submit(&request).unwrap(),
        BrokerSubmitResult::Acknowledged { .. }
    ));
    let second_submit = registry
        .submit(&BrokerOrderRequest {
            account_id: "acct.paper.002".to_owned(),
            ..request.clone()
        })
        .unwrap();
    assert!(matches!(
        second_submit,
        BrokerSubmitResult::Acknowledged { .. }
    ));
    registry
        .cancel(&BrokerCancelRequest {
            account_id: "acct.paper.001".to_owned(),
            client_order_id: request.client_order_id.clone(),
        })
        .unwrap();
    assert_eq!(registry.poll("acct.paper.001").unwrap().len(), 2);
    assert_eq!(registry.poll("acct.paper.002").unwrap().len(), 1);
    assert!(registry
        .submit(&BrokerOrderRequest {
            account_id: "acct.paper.404".to_owned(),
            ..request
        })
        .is_err());
    let duplicate_account = account();
    assert!(registry
        .register(
            PaperBrokerRoute {
                account_id: duplicate_account.account_id.clone(),
                adapter_id: "adapter.ibkr.paper.duplicate".to_owned(),
                venue_id: "venue.ibkr.paper".to_owned(),
                environment: "PAPER".to_owned(),
            },
            Box::new(IbkrPaperAdapter::new(&duplicate_account).unwrap()),
        )
        .is_err());
    assert!(registry
        .register(
            PaperBrokerRoute {
                account_id: "acct.paper.003".to_owned(),
                adapter_id: "adapter.unfingerprinted.paper.003".to_owned(),
                venue_id: "venue.unfingerprinted.paper".to_owned(),
                environment: "PAPER".to_owned(),
            },
            Box::new(CashAndPositionMismatchBroker),
        )
        .is_err());
}

#[test]
fn broker_evidence_handles_out_of_order_fills_pending_cancel_and_late_terminal_messages() {
    let mut service = service();
    let submitted = service
        .submit_intent(
            intent("intent-paper-edge-fill", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    let order_id = submitted.order_id.unwrap();
    let broker_order_id = service
        .order(&order_id)
        .unwrap()
        .broker_order_id
        .clone()
        .unwrap();

    // An execution is sufficient evidence to establish acknowledgement even
    // when its acknowledgement message has not yet been applied.
    service
        .order_mut(&order_id)
        .unwrap()
        .oms
        .transition(OrderState::Unknown, "TEST_OUT_OF_ORDER")
        .unwrap();
    service
        .apply_broker_event(BrokerEvent::Execution {
            execution_id: "exec-paper-before-ack".to_owned(),
            client_order_id: order_id.clone(),
            broker_order_id: broker_order_id.clone(),
            quantity: decimal("quantity", "0.5").unwrap(),
            price: decimal("price", "100").unwrap(),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:01Z".to_owned(),
        })
        .unwrap();
    assert_eq!(
        service.order(&order_id).unwrap().oms.state,
        OrderState::PartiallyFilled
    );

    service.cancel_order(&order_id).unwrap();
    assert_eq!(
        service.order(&order_id).unwrap().oms.state,
        OrderState::PendingCancel
    );
    service
        .apply_broker_event(BrokerEvent::Execution {
            execution_id: "exec-paper-during-cancel".to_owned(),
            client_order_id: order_id.clone(),
            broker_order_id,
            quantity: decimal("quantity", "0.5").unwrap(),
            price: decimal("price", "100").unwrap(),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:02Z".to_owned(),
        })
        .unwrap();
    assert_eq!(
        service.order(&order_id).unwrap().oms.state,
        OrderState::Filled
    );
    service
        .apply_broker_event(BrokerEvent::Cancelled {
            client_order_id: order_id.clone(),
            reason: "LATE_CANCEL".to_owned(),
        })
        .unwrap();
    assert_eq!(
        service.order(&order_id).unwrap().oms.state,
        OrderState::Filled
    );
    assert_eq!(
        service.order(&order_id).unwrap().filled_quantity,
        decimal("quantity", "1").unwrap()
    );
}

#[test]
fn broker_terminal_and_replacement_paths_preserve_partial_fills_and_versions() {
    let mut service = service();
    let submit = |service: &mut PaperTradingService<IbkrPaperAdapter>, suffix: &str| {
        let outcome = service
            .submit_intent(
                intent(
                    &format!("intent-paper-edge-{suffix}"),
                    "2026-01-02T14:31:00Z",
                ),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        outcome.order_id.unwrap()
    };
    let partial = |service: &mut PaperTradingService<IbkrPaperAdapter>,
                   order_id: &str,
                   execution_id: &str| {
        let broker_order_id = service
            .order(order_id)
            .unwrap()
            .broker_order_id
            .clone()
            .unwrap();
        service
            .apply_broker_event(BrokerEvent::Execution {
                execution_id: execution_id.to_owned(),
                client_order_id: order_id.to_owned(),
                broker_order_id,
                quantity: decimal("quantity", "0.5").unwrap(),
                price: decimal("price", "100").unwrap(),
                fee: Decimal::ZERO,
                executed_at: "2026-01-02T14:31:01Z".to_owned(),
            })
            .unwrap();
    };

    let cancel_rejected = submit(&mut service, "cancel-rejected");
    partial(&mut service, &cancel_rejected, "exec-paper-cancel-rejected");
    service.cancel_order(&cancel_rejected).unwrap();
    service
        .apply_broker_event(BrokerEvent::CancelRejected {
            client_order_id: cancel_rejected.clone(),
            reason: "CANCEL_TOO_LATE".to_owned(),
        })
        .unwrap();
    assert_eq!(
        service.order(&cancel_rejected).unwrap().oms.state,
        OrderState::PartiallyFilled
    );

    let rejected = submit(&mut service, "rejected");
    partial(&mut service, &rejected, "exec-paper-rejected");
    service
        .apply_broker_event(BrokerEvent::Rejected {
            client_order_id: rejected.clone(),
            reason: "BROKER_REJECTED_REMAINDER".to_owned(),
        })
        .unwrap();
    assert_eq!(
        service.order(&rejected).unwrap().oms.state,
        OrderState::Rejected
    );
    assert_eq!(
        service.order(&rejected).unwrap().filled_quantity,
        decimal("quantity", "0.5").unwrap()
    );

    let expired = submit(&mut service, "expired");
    partial(&mut service, &expired, "exec-paper-expired");
    service
        .apply_broker_event(BrokerEvent::Expired {
            client_order_id: expired.clone(),
            reason: "DAY_EXPIRED".to_owned(),
        })
        .unwrap();
    assert_eq!(
        service.order(&expired).unwrap().oms.state,
        OrderState::Expired
    );

    let mut limit = intent("intent-paper-edge-replace", "2026-01-02T14:31:00Z");
    limit.order_type = OrderType::Limit;
    limit.limit_price = Some(decimal("limit", "100").unwrap());
    let replacing = service
        .submit_intent(
            limit,
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap()
        .order_id
        .unwrap();
    service
        .replace_order(&replacing, decimal("limit", "90").unwrap())
        .unwrap();
    assert_eq!(
        service.order(&replacing).unwrap().oms.state,
        OrderState::PendingReplace
    );
    service.synchronize().unwrap();
    let replaced = service.order(&replacing).unwrap();
    assert_eq!(replaced.oms.state, OrderState::Acknowledged);
    assert_eq!(replaced.broker_order_versions.len(), 2);
    service
        .apply_broker_event(BrokerEvent::ReplaceRequested {
            client_order_id: replacing.clone(),
            previous_broker_order_id: replaced.broker_order_id.clone().unwrap(),
        })
        .unwrap();
    service
        .apply_broker_event(BrokerEvent::ReplaceRejected {
            client_order_id: replacing.clone(),
            reason: "REPLACE_REJECTED".to_owned(),
        })
        .unwrap();
    assert_eq!(
        service.order(&replacing).unwrap().oms.state,
        OrderState::Acknowledged
    );
}

#[test]
fn reconciliation_creates_durable_incidents_instead_of_overwriting_truth() {
    let mut service = PaperTradingService::new(
        account(),
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        CashAndPositionMismatchBroker,
    )
    .unwrap();
    let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
    assert!(!report.is_clean());
    assert_eq!(report.issues.len(), 2);
    assert_eq!(service.dashboard().unexplained_incidents, 2);
    for issue in report.issues {
        service
            .explain_incident(&issue.incident_id, "operator reviewed independent snapshot")
            .unwrap();
    }
    assert_eq!(service.dashboard().unexplained_incidents, 0);
    assert_eq!(service.dashboard().internal_cash, "100000.00000000");
}

#[test]
fn broker_snapshot_ingress_allows_versions_but_rejects_duplicate_or_malformed_identity() {
    let duplicate = BrokerAccountSnapshot {
        orders: vec![
            BrokerOrderSnapshot {
                client_order_id: "order-intent-paper-001".to_owned(),
                broker_order_id: "ibkr-paper-order-00000001".to_owned(),
                state: OrderState::Acknowledged,
                filled_quantity: Decimal::ZERO,
            },
            BrokerOrderSnapshot {
                client_order_id: "order-intent-paper-001".to_owned(),
                broker_order_id: "ibkr-paper-order-00000002".to_owned(),
                state: OrderState::Acknowledged,
                filled_quantity: Decimal::ZERO,
            },
        ],
        positions: Vec::new(),
        cash: Decimal::ZERO,
    };
    assert!(validate_broker_snapshot(&duplicate).is_ok());
    let duplicated_version = BrokerAccountSnapshot {
        orders: vec![duplicate.orders[0].clone(), duplicate.orders[0].clone()],
        positions: Vec::new(),
        cash: Decimal::ZERO,
    };
    assert!(validate_broker_snapshot(&duplicated_version).is_err());
    assert!(validate_broker_reason("reason", "").is_err());
}

#[test]
fn duplicate_broker_execution_is_idempotent_and_ambiguous_submit_reconciles() {
    let account = account();
    let adapter = IbkrPaperAdapter::new(&account).unwrap();
    let mut faulted = FaultInjectingBroker::new(adapter);
    faulted.inject(BrokerOperation::Submit, BrokerFault::AmbiguousAfterSubmit);
    let mut service = PaperTradingService::new(
        account,
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        faulted,
    )
    .unwrap();
    assert!(service
        .submit_intent(
            intent("intent-paper-003", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .is_err());
    let order_id = "order-intent-paper-003";
    assert_eq!(
        service.order(order_id).unwrap().oms.state,
        OrderState::Unknown
    );
    let clean = service
        .reconnect_and_reconcile("2026-01-02T14:32:00Z")
        .unwrap();
    assert!(clean.is_clean());
    assert_eq!(
        service.order(order_id).unwrap().oms.state,
        OrderState::Acknowledged
    );

    service
        .broker_mut()
        .inner
        .queue_fill(
            order_id,
            decimal("quantity", "1").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:32:01Z",
        )
        .unwrap();
    service
        .broker_mut()
        .inject(BrokerOperation::Poll, BrokerFault::DuplicateFirstEvent);
    service.synchronize().unwrap();
    assert_eq!(
        service.order(order_id).unwrap().filled_quantity,
        decimal("filled", "1").unwrap()
    );
    assert!(service
        .reconcile("2026-01-02T14:33:00Z")
        .unwrap()
        .is_clean());
}
