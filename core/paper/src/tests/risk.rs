//! Single-order pre-trade risk controls.

use super::*;

#[test]
fn kill_switch_blocks_new_paper_orders_without_strategy_or_broker_health() {
    let mut service = service();
    service
        .activate_kill_switch(KillSwitchScope::Global)
        .unwrap();
    let result = service
        .submit_intent(
            intent("intent-paper-002", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .iter()
        .any(|reason| reason == "KILL_SWITCH_GLOBAL"));
    assert!(result.order_id.is_none());
}

#[test]
fn paper_risk_requires_fresh_data_reserves_cash_and_persists_rejections() {
    let mut limited_account = account();
    limited_account.initial_cash = decimal("cash", "100").unwrap();
    let adapter = IbkrPaperAdapter::new(&limited_account).unwrap();
    let mut service = PaperTradingService::new(
        limited_account,
        policy(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        adapter,
    )
    .unwrap();
    assert!(
        service
            .submit_intent(
                intent("intent-paper-005", "2026-01-02T14:31:00Z"),
                market_at_price("60", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap()
            .decision
            .approved
    );
    let rejected = service
        .submit_intent(
            intent("intent-paper-006", "2026-01-02T14:31:01Z"),
            market_at_price("60", "2026-01-02T14:31:01Z"),
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert!(!rejected.decision.approved);
    assert!(rejected
        .decision
        .reason_codes
        .iter()
        .any(|reason| reason == "INSUFFICIENT_INTERNAL_CASH"));
    assert!(service
        .risk_evidence("paper-risk-intent-paper-006")
        .is_some());
    assert!(service
        .submit_intent(
            intent("intent-paper-007", "2026-01-02T14:31:10Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:10Z",
        )
        .is_err());
}

#[test]
fn paper_price_collar_rejection_is_explainable_and_creates_no_order() {
    let mut service = service();
    let mut far_limit = intent("intent-paper-price-collar", "2026-01-02T14:31:00Z");
    far_limit.order_type = OrderType::Limit;
    far_limit.limit_price = Some(decimal("limit", "105").unwrap());
    let result = service
        .submit_intent(
            far_limit,
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result.order_id.is_none());
    assert!(result
        .decision
        .reason_codes
        .contains(&"PRICE_COLLAR_EXCEEDED".to_owned()));
    assert!(result
        .decision
        .evaluated_limits
        .contains("requested_price_deviation_bps=500.00000000"));
}

#[test]
fn paper_self_trade_risk_rejects_a_new_order_opposite_an_existing_working_order() {
    let mut service = service();
    let filled = service
        .submit_intent(
            intent("intent-paper-self-trade-position", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let position_order_id = filled.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &position_order_id,
            decimal("quantity", "1").unwrap(),
            decimal("price", "100").unwrap(),
            decimal("fee", "0.10").unwrap(),
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);
    assert_eq!(
        service.order(&position_order_id).unwrap().oms.state,
        OrderState::Filled
    );

    let resting_buy = service
        .submit_intent(
            intent("intent-paper-self-trade-buy", "2026-01-02T14:32:00Z"),
            market("2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    assert!(resting_buy.decision.approved);
    assert_eq!(
        service
            .order(&resting_buy.order_id.unwrap())
            .unwrap()
            .oms
            .state,
        OrderState::Acknowledged
    );

    let mut opposite_sell = intent("intent-paper-self-trade-sell", "2026-01-02T14:32:01Z");
    opposite_sell.side = Side::Sell;
    let sell_result = service
        .submit_intent(
            opposite_sell,
            market("2026-01-02T14:32:01Z"),
            "2026-01-02T14:32:01Z",
        )
        .unwrap();
    assert!(!sell_result.decision.approved);
    assert!(sell_result.order_id.is_none());
    assert!(sell_result
        .decision
        .reason_codes
        .contains(&"SELF_TRADE_RISK".to_owned()));
}

#[test]
fn paper_order_rate_limit_rejects_submissions_beyond_the_configured_window() {
    let mut rate_limited_policy = policy();
    rate_limited_policy.max_order_rate = 2;
    let mut service = PaperTradingService::new(
        account(),
        rate_limited_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account()).unwrap(),
    )
    .unwrap();
    let first = service
        .submit_intent(
            intent("intent-paper-rate-001", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(first.decision.approved);
    let second = service
        .submit_intent(
            intent("intent-paper-rate-002", "2026-01-02T14:31:01Z"),
            market("2026-01-02T14:31:01Z"),
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert!(second.decision.approved);
    let third = service
        .submit_intent(
            intent("intent-paper-rate-003", "2026-01-02T14:31:02Z"),
            market("2026-01-02T14:31:02Z"),
            "2026-01-02T14:31:02Z",
        )
        .unwrap();
    assert!(!third.decision.approved);
    assert!(third.order_id.is_none());
    assert!(third
        .decision
        .reason_codes
        .contains(&"MAX_ORDER_RATE_EXCEEDED".to_owned()));
    assert!(third
        .decision
        .evaluated_limits
        .contains("recent_order_count=2"));
}

#[test]
fn paper_order_rate_limit_counts_decision_time_not_caller_supplied_created_at() {
    let mut rate_limited_policy = policy();
    rate_limited_policy.max_order_rate = 2;
    let mut service = PaperTradingService::new(
        account(),
        rate_limited_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account()).unwrap(),
    )
    .unwrap();
    // Every intent claims a `created_at` far outside the rate window, but each
    // is actually decided within it: a caller must not be able to understate
    // its own submission rate and bypass `MAX_ORDER_RATE_EXCEEDED` by
    // backdating the intent's self-reported `created_at`.
    let first = service
        .submit_intent(
            intent("intent-paper-rate-backdate-001", "2020-01-01T00:00:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(first.decision.approved);
    let second = service
        .submit_intent(
            intent("intent-paper-rate-backdate-002", "2020-01-01T00:00:00Z"),
            market("2026-01-02T14:31:01Z"),
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert!(second.decision.approved);
    let third = service
        .submit_intent(
            intent("intent-paper-rate-backdate-003", "2020-01-01T00:00:00Z"),
            market("2026-01-02T14:31:02Z"),
            "2026-01-02T14:31:02Z",
        )
        .unwrap();
    assert!(!third.decision.approved);
    assert!(third.order_id.is_none());
    assert!(third
        .decision
        .reason_codes
        .contains(&"MAX_ORDER_RATE_EXCEEDED".to_owned()));
}
