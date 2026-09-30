//! Broker-route capability declarations and refusals.

use super::*;

/// The model behind a route that declares only the trait default, single
/// DAY orders, as the real IBKR bridge adapter does. It forwards every
/// call to the model, which could execute all of them, so only the
/// service's capability check stands between a request and the broker.
struct NarrowRouteBroker {
    inner: IbkrPaperAdapter,
    broker_calls: u32,
}

impl NarrowRouteBroker {
    fn new() -> Self {
        Self {
            inner: IbkrPaperAdapter::new(&account()).unwrap(),
            broker_calls: 0,
        }
    }
}

impl PaperBrokerAdapter for NarrowRouteBroker {
    fn adapter_configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        self.inner.adapter_configuration_fingerprint(account_id)
    }

    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        self.broker_calls += 1;
        self.inner.submit(request)
    }

    fn submit_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        self.broker_calls += 1;
        self.inner.submit_combo(request)
    }

    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        self.inner.cancel(request)
    }

    fn replace(&mut self, request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        self.broker_calls += 1;
        self.inner.replace(request)
    }

    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        self.inner.poll(account_id)
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        self.inner.snapshot(account_id)
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        self.inner.reconnect(account_id)
    }
}

/// A durable service over a narrow route, in a fresh directory, so a test
/// can prove a refusal journaled nothing.
fn narrow_route_service(name: &str) -> (PathBuf, PaperTradingService<NarrowRouteBroker>) {
    let directory = std::env::temp_dir().join(format!(
        "follon-paper-narrow-route-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let service = PaperTradingService::open_durable(
        account(),
        policy_permitting_shorts(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        NarrowRouteBroker::new(),
        directory.join("journal.ndjson"),
    )
    .unwrap();
    (directory, service)
}

#[test]
fn a_route_without_gtc_refuses_a_gtc_intent_before_anything_is_recorded() {
    let (directory, mut service) = narrow_route_service("gtc");
    let at = "2026-01-02T14:31:00Z";
    let sequence = service.dashboard().audit_sequence;
    // On an instrument nothing else in this test touches, so any trace of
    // it in the journal can only have come from the refused request.
    let gtc = OrderIntent {
        instrument_id: "inst.us_equity.qqq".to_owned(),
        time_in_force: TimeInForce::GoodTilCancelled,
        ..intent("intent-narrow-gtc-001", at)
    };
    let qqq = PaperMarketData {
        instrument_id: "inst.us_equity.qqq".to_owned(),
        mark_price: decimal("mark", "250").unwrap(),
        observed_at: at.to_owned(),
    };

    let error = service.submit_intent(gtc, qqq, at).unwrap_err();

    assert!(error.0.contains("carries only DAY orders"), "{}", error.0);
    assert!(service.order("order-intent-narrow-gtc-001").is_none());
    assert!(service
        .risk_evidence("paper-risk-intent-narrow-gtc-001")
        .is_none());
    assert_eq!(service.broker_mut().broker_calls, 0);
    let dashboard = service.dashboard();
    assert_eq!(
        dashboard.audit_sequence, sequence,
        "the refusal was journaled"
    );
    assert!(dashboard.broker_connected);
    assert_eq!(dashboard.unknown_orders, 0);

    // The same route still carries a DAY order, and journaling it must
    // not carry anything the refused request left behind: risk
    // evaluation caches the request's mark, and the next record persists
    // that cache, so a refusal after evaluation would leak it here.
    let day = service
        .submit_intent(intent("intent-narrow-day-001", at), market(at), at)
        .unwrap();
    assert!(day.decision.approved, "{:?}", day.decision.reason_codes);
    assert_eq!(service.broker_mut().broker_calls, 1);
    drop(service);
    let journal = fs::read_to_string(directory.join("journal.ndjson")).unwrap();
    assert!(
        !journal.contains("inst.us_equity.qqq"),
        "the refused request reached the journal"
    );
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_route_without_combinations_refuses_one_before_anything_is_recorded() {
    // Before the capability check, the real bridge adapter answered a
    // combination with an error the OMS records as a transport failure:
    // the combination became UNKNOWN, the session disconnected, and every
    // later order was refused until evidence that could never arrive.
    let (directory, mut service) = narrow_route_service("combo");
    let at = "2026-01-02T14:31:00Z";
    let sequence = service.dashboard().audit_sequence;

    let error = service
        .submit_combo_intent(
            combo_intent("intent-narrow-combo-001", at),
            combo_market(at),
            at,
        )
        .unwrap_err();

    assert!(
        error.0.contains("cannot execute combinations"),
        "{}",
        error.0
    );
    assert!(service
        .combo_order(&OmsComboOrder::order_id_for("intent-narrow-combo-001"))
        .is_none());
    assert!(service
        .combo_risk_evidence("paper-combo-risk-intent-narrow-combo-001")
        .is_none());
    assert_eq!(service.broker_mut().broker_calls, 0);
    let dashboard = service.dashboard();
    assert_eq!(
        dashboard.audit_sequence, sequence,
        "the refusal was journaled"
    );
    assert!(dashboard.broker_connected);
    assert_eq!(dashboard.working_orders, 0);

    // Nothing blocks the next single order, and journaling it carries no
    // leg mark the refused request's risk evaluation would have cached.
    let day = service
        .submit_intent(intent("intent-narrow-day-002", at), market(at), at)
        .unwrap();
    assert!(day.decision.approved, "{:?}", day.decision.reason_codes);
    drop(service);
    let journal = fs::read_to_string(directory.join("journal.ndjson")).unwrap();
    assert!(
        !journal.contains("inst.us_option.spy.near"),
        "the refused request reached the journal"
    );
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_route_without_replacement_refuses_one_and_leaves_the_order_unchanged() {
    let (directory, mut service) = narrow_route_service("replace");
    let at = "2026-01-02T14:31:00Z";
    let resting = service
        .submit_intent(
            OrderIntent {
                order_type: OrderType::Limit,
                limit_price: Some(decimal("limit", "99.50").unwrap()),
                ..intent("intent-narrow-replace-001", at)
            },
            market(at),
            at,
        )
        .unwrap();
    let order_id = resting.order_id.unwrap();
    assert_eq!(resting.state, Some(OrderState::Acknowledged));
    let sequence = service.dashboard().audit_sequence;
    let calls = service.broker_mut().broker_calls;

    let error = service
        .replace_order(&order_id, decimal("limit", "99").unwrap())
        .unwrap_err();

    assert!(error.0.contains("cannot replace orders"), "{}", error.0);
    assert_eq!(
        service.order(&order_id).unwrap().oms.state,
        OrderState::Acknowledged
    );
    assert_eq!(service.broker_mut().broker_calls, calls);
    let dashboard = service.dashboard();
    assert_eq!(
        dashboard.audit_sequence, sequence,
        "the refusal was journaled"
    );
    assert!(dashboard.broker_connected);
    assert_eq!(dashboard.unknown_orders, 0);
    drop(service);
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_route_declares_what_its_adapter_can_carry() {
    let everything = PaperBrokerCapabilities {
        combinations: true,
        good_til_cancelled: true,
        replacement: true,
    };
    assert_eq!(
        PaperBrokerCapabilities::default(),
        PaperBrokerCapabilities {
            combinations: false,
            good_til_cancelled: false,
            replacement: false,
        }
    );
    let model = IbkrPaperAdapter::new(&account()).unwrap();
    assert_eq!(model.capabilities("acct.paper.001").unwrap(), everything);
    assert!(model.capabilities("acct.paper.002").is_err());
    assert_eq!(
        NarrowRouteBroker::new()
            .capabilities("acct.paper.001")
            .unwrap(),
        PaperBrokerCapabilities::default()
    );
    assert_eq!(
        FaultInjectingBroker::new(IbkrPaperAdapter::new(&account()).unwrap())
            .capabilities("acct.paper.001")
            .unwrap(),
        everything
    );
    assert_eq!(
        FaultInjectingBroker::new(NarrowRouteBroker::new())
            .capabilities("acct.paper.001")
            .unwrap(),
        PaperBrokerCapabilities::default()
    );

    // A registry route carries exactly what its own adapter declares:
    // a narrow route and a model route side by side.
    let second_account = PaperAccount {
        account_id: "acct.paper.002".to_owned(),
        ..account()
    };
    let mut registry = PaperBrokerRegistry::new();
    registry
        .register(
            PaperBrokerRoute {
                account_id: "acct.paper.001".to_owned(),
                adapter_id: "adapter.ibkr.paper.narrow".to_owned(),
                venue_id: "venue.ibkr.paper".to_owned(),
                environment: "PAPER".to_owned(),
            },
            Box::new(NarrowRouteBroker::new()),
        )
        .unwrap();
    registry
        .register(
            PaperBrokerRoute {
                account_id: "acct.paper.002".to_owned(),
                adapter_id: "adapter.ibkr.paper.model".to_owned(),
                venue_id: "venue.ibkr.paper".to_owned(),
                environment: "PAPER".to_owned(),
            },
            Box::new(IbkrPaperAdapter::new(&second_account).unwrap()),
        )
        .unwrap();
    assert_eq!(
        registry.capabilities("acct.paper.001").unwrap(),
        PaperBrokerCapabilities::default()
    );
    assert_eq!(registry.capabilities("acct.paper.002").unwrap(), everything);
    assert!(registry.capabilities("acct.paper.unrouted").is_err());
}
