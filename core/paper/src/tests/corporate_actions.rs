//! Operator-attested corporate actions in PAPER (delivery state E8.4b).

use super::*;
use follon_market_data::CorporateAction;

const SPY: &str = "inst.us_equity.spy";
const QQQ: &str = "inst.us_equity.qqq";
const NEAR: &str = "inst.us_option.spy.near";

fn split(action_id: &str, instrument_id: &str, ratio: &str) -> CorporateAction {
    CorporateAction::Split {
        action_id: action_id.to_owned(),
        instrument_id: instrument_id.to_owned(),
        effective_at: "2026-01-05T13:30:00Z".to_owned(),
        ratio: decimal("ratio", ratio).unwrap(),
    }
}

fn dividend(action_id: &str, amount: &str) -> CorporateAction {
    CorporateAction::CashDividend {
        action_id: action_id.to_owned(),
        instrument_id: SPY.to_owned(),
        effective_at: "2026-01-05T13:30:00Z".to_owned(),
        amount: decimal("amount", amount).unwrap(),
    }
}

fn request(action: CorporateAction, held: &str) -> PaperCorporateAction {
    PaperCorporateAction {
        action,
        held_quantity: decimal("held", held).unwrap(),
        applied_by: "operator.ops".to_owned(),
        applied_at: "2026-01-05T13:31:00Z".to_owned(),
    }
}

/// Submits one market order on `instrument_id` at `price` and, unless `fill` is
/// false, completely fills it with a fee of 0.10. Returns the order id.
#[allow(clippy::too_many_arguments)]
fn trade(
    service: &mut PaperTradingService<IbkrPaperAdapter>,
    intent_id: &str,
    instrument_id: &str,
    side: Side,
    quantity: &str,
    price: &str,
    at: &str,
    fill: bool,
) -> String {
    let outcome = service
        .submit_intent(
            OrderIntent {
                instrument_id: instrument_id.to_owned(),
                side,
                quantity: decimal("quantity", quantity).unwrap(),
                ..intent(intent_id, at)
            },
            PaperMarketData {
                instrument_id: instrument_id.to_owned(),
                mark_price: decimal("mark", price).unwrap(),
                observed_at: at.to_owned(),
            },
            at,
        )
        .unwrap();
    assert!(outcome.decision.approved, "{:?}", outcome.decision);
    let order_id = outcome.order_id.unwrap();
    service.synchronize().unwrap();
    if fill {
        service
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", quantity).unwrap(),
                decimal("price", price).unwrap(),
                decimal("fee", "0.10").unwrap(),
                at,
            )
            .unwrap();
        service.synchronize().unwrap();
    }
    order_id
}

fn buy(
    service: &mut PaperTradingService<IbkrPaperAdapter>,
    intent_id: &str,
    quantity: &str,
    price: &str,
    at: &str,
) {
    trade(
        service,
        intent_id,
        SPY,
        Side::Buy,
        quantity,
        price,
        at,
        true,
    );
}

fn position(
    service: &PaperTradingService<IbkrPaperAdapter>,
    instrument_id: &str,
) -> PaperDashboardPosition {
    service
        .dashboard()
        .positions
        .into_iter()
        .find(|row| row.instrument_id == instrument_id)
        .expect("a position row")
}

fn lots(service: &PaperTradingService<IbkrPaperAdapter>) -> Vec<(String, String)> {
    service
        .tax_lots(SPY)
        .iter()
        .map(|lot| {
            (
                lot.remaining_quantity.to_string(),
                lot.unit_cost.to_string(),
            )
        })
        .collect()
}

/// Everything a refused action must leave exactly as it was.
fn books(
    service: &PaperTradingService<IbkrPaperAdapter>,
) -> (PaperDashboard, Vec<(String, String)>, usize) {
    (
        service.dashboard(),
        lots(service),
        service.corporate_actions().len(),
    )
}

#[test]
fn a_split_follows_the_position_and_its_lots_and_reconciles_with_the_venue() {
    let mut service = service();
    buy(
        &mut service,
        "intent-ca-buy-1",
        "5",
        "100",
        "2026-01-02T14:31:00Z",
    );
    buy(
        &mut service,
        "intent-ca-buy-2",
        "5",
        "110",
        "2026-01-02T14:32:00Z",
    );
    let cash = service.dashboard().internal_cash;
    let action = split("ca-split-spy-2026", SPY, "2");

    service
        .broker_mut()
        .apply_corporate_action(&action)
        .unwrap();
    let receipt = service
        .apply_corporate_action(request(action.clone(), "10"))
        .unwrap();

    assert_eq!(
        receipt,
        PaperCorporateActionReceipt {
            action,
            applied_by: "operator.ops".to_owned(),
            applied_at: "2026-01-05T13:31:00Z".to_owned(),
            quantity_before: decimal("before", "10").unwrap(),
            quantity_after: decimal("after", "20").unwrap(),
            cash_delta: Decimal::ZERO,
        }
    );
    assert_eq!(service.corporate_actions(), [receipt]);
    // The position's total cost, 1,050.20, is unchanged over twice the shares.
    let row = position(&service, SPY);
    assert_eq!(
        (row.quantity.as_str(), row.average_cost.as_str()),
        ("20.00000000", "52.51000000")
    );
    assert_eq!(
        lots(&service),
        [
            ("10.00000000".to_owned(), "50.01000000".to_owned()),
            ("10.00000000".to_owned(), "55.01000000".to_owned())
        ]
    );
    assert_eq!(service.dashboard().internal_cash, cash);
    assert!(service
        .reconcile("2026-01-05T13:32:00Z")
        .unwrap()
        .is_clean());

    // The whole post-split position can be sold, and the lots realize against
    // the split cost: 1,100 less the 0.10 fee less 1,050.20.
    trade(
        &mut service,
        "intent-ca-sell",
        SPY,
        Side::Sell,
        "20",
        "55",
        "2026-01-05T14:00:00Z",
        true,
    );
    assert_eq!(position(&service, SPY).quantity, "0.00000000");
    assert!(service.tax_lots(SPY).is_empty());
    assert_eq!(
        service.realized_tax_pnl().unwrap().to_string(),
        "49.70000000"
    );
    assert!(service
        .reconcile("2026-01-05T14:01:00Z")
        .unwrap()
        .is_clean());
}

#[test]
fn without_the_split_the_oms_cannot_sell_what_the_venue_holds() {
    // The defect this closes: the venue split the position and the OMS did not,
    // so reconciliation disagrees and the post-split position cannot be sold.
    let mut service = service();
    buy(
        &mut service,
        "intent-ca-buy-1",
        "10",
        "100",
        "2026-01-02T14:31:00Z",
    );
    service
        .broker_mut()
        .apply_corporate_action(&split("ca-split-spy-2026", SPY, "2"))
        .unwrap();
    let report = service.reconcile("2026-01-05T13:32:00Z").unwrap();
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.category == "POSITION_QUANTITY_MISMATCH"));
    let sale = service
        .submit_intent(
            OrderIntent {
                side: Side::Sell,
                quantity: decimal("quantity", "20").unwrap(),
                ..intent("intent-ca-sell", "2026-01-05T14:00:00Z")
            },
            market("2026-01-05T14:00:00Z"),
            "2026-01-05T14:00:00Z",
        )
        .unwrap();
    assert!(!sale.decision.approved);
}

#[test]
fn a_split_rebases_the_cached_mark_and_scales_the_strategy_attribution() {
    let mut aggregate = follon_risk::PortfolioRiskPolicy {
        version: "portfolio-risk-v1".to_owned(),
        global_kill_switch: false,
        max_gross_exposure: decimal("gross", "100000").unwrap(),
        max_abs_net_exposure: decimal("net", "100000").unwrap(),
        max_leverage_bps: decimal("leverage", "10000").unwrap(),
        max_concentration_bps: decimal("concentration", "10000").unwrap(),
        max_daily_loss: Decimal::from_integer(i64::MAX).unwrap(),
        max_drawdown_bps: decimal("drawdown", "10000").unwrap(),
        max_margin_utilization_bps: Decimal::ZERO,
        max_abs_delta: Decimal::ZERO,
        max_abs_gamma: Decimal::ZERO,
        max_open_orders: usize::MAX,
        max_order_rate: u32::MAX,
        allowed_instruments: BTreeSet::new(),
        restricted_instruments: BTreeSet::new(),
        sector_limits: BTreeMap::new(),
        asset_class_limits: BTreeMap::new(),
        currency_limits: BTreeMap::new(),
        strategy_limits: BTreeMap::new(),
        max_news_slippage_bps: None,
        max_spread_multiplier_bps: None,
    };
    aggregate.max_gross_exposure = decimal("gross", "100000").unwrap();
    let mut risk_policy = policy();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: aggregate,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let mut service = service_with(risk_policy);
    buy(
        &mut service,
        "intent-ca-spy",
        "5",
        "100",
        "2026-01-02T14:31:00Z",
    );
    trade(
        &mut service,
        "intent-ca-qqq",
        QQQ,
        Side::Buy,
        "10",
        "100",
        "2026-01-02T14:32:00Z",
        true,
    );
    let action = split("ca-split-qqq-2026", QQQ, "2");
    service
        .broker_mut()
        .apply_corporate_action(&action)
        .unwrap();
    service
        .apply_corporate_action(request(action, "10"))
        .unwrap();

    // A later SPY decision marks QQQ from the cache. Rebased, the split moves no
    // exposure: SPY 5 x 100, QQQ 20 x 50 and the 1 x 100 candidate. Left at the
    // pre-split 100 it would read 2,600, and an unscaled attribution would leave
    // ten QQQ shares to no strategy.
    let outcome = service
        .submit_intent(
            intent("intent-ca-after", "2026-01-05T14:00:00Z"),
            market("2026-01-05T14:00:00Z"),
            "2026-01-05T14:00:00Z",
        )
        .unwrap();
    let limits = &outcome.decision.evaluated_limits;
    assert!(
        limits.contains("portfolio_gross_exposure=1600.00000000"),
        "{limits}"
    );
    assert!(!limits.contains("unattributed"), "{limits}");
}

#[test]
fn a_split_is_refused_while_an_order_works_in_its_instrument() {
    let mut service = service();
    buy(
        &mut service,
        "intent-ca-buy",
        "10",
        "100",
        "2026-01-02T14:31:00Z",
    );
    let working = trade(
        &mut service,
        "intent-ca-rest",
        SPY,
        Side::Sell,
        "4",
        "100",
        "2026-01-02T14:33:00Z",
        false,
    );
    let before = books(&service);

    let refusal = service
        .apply_corporate_action(request(split("ca-split-spy-2026", SPY, "2"), "10"))
        .unwrap_err();
    assert!(
        refusal
            .0
            .contains(&format!("working order {working} cannot rest across")),
        "{refusal}"
    );
    assert_eq!(books(&service), before);

    // A working order in another instrument does not hold up the split, and once the
    // order in this one is cancelled the split applies.
    trade(
        &mut service,
        "intent-ca-qqq",
        QQQ,
        Side::Buy,
        "1",
        "100",
        "2026-01-02T14:34:00Z",
        false,
    );
    service.cancel_order(&working).unwrap();
    service.synchronize().unwrap();
    service
        .apply_corporate_action(request(split("ca-split-spy-2026", SPY, "2"), "10"))
        .unwrap();
    assert_eq!(position(&service, SPY).quantity, "20.00000000");
}

#[test]
fn a_split_is_refused_while_a_combination_leg_works_in_its_instrument() {
    let mut service = service_permitting_shorts();
    let outcome = service
        .submit_combo_intent(
            combo_intent("intent-ca-combo", "2026-01-02T14:31:00Z"),
            combo_market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(outcome.decision.approved, "{:?}", outcome.decision);
    let order_id = outcome.order_id.unwrap();

    let refusal = service
        .apply_corporate_action(request(split("ca-split-near-2026", NEAR, "2"), "0"))
        .unwrap_err();
    assert!(
        refusal
            .0
            .contains(&format!("working order {order_id} cannot rest across")),
        "{refusal}"
    );
    assert!(service.corporate_actions().is_empty());
}

#[test]
fn a_split_that_leaves_a_fraction_of_a_lot_is_refused() {
    let mut service = service();
    buy(
        &mut service,
        "intent-ca-buy",
        "3",
        "100",
        "2026-01-02T14:31:00Z",
    );
    let before = books(&service);
    let refusal = service
        .apply_corporate_action(request(split("ca-split-spy-3-2", SPY, "1.5"), "3"))
        .unwrap_err();
    assert!(
        refusal.0.contains("not a whole number of its lots"),
        "{refusal}"
    );
    assert_eq!(books(&service), before);

    // The same ratio on a position it divides into whole lots applies.
    buy(
        &mut service,
        "intent-ca-buy-2",
        "1",
        "100",
        "2026-01-02T14:32:00Z",
    );
    service
        .apply_corporate_action(request(split("ca-split-spy-3-2", SPY, "1.5"), "4"))
        .unwrap();
    assert_eq!(position(&service, SPY).quantity, "6.00000000");
}

#[test]
fn an_action_applies_only_to_the_position_it_was_stated_against() {
    let mut service = service();
    buy(
        &mut service,
        "intent-ca-buy",
        "10",
        "100",
        "2026-01-02T14:31:00Z",
    );
    let before = books(&service);
    for (action, held) in [
        (split("ca-split-spy-2026", SPY, "2"), "8"),
        (split("ca-split-spy-2026", SPY, "2"), "0"),
        (dividend("ca-dividend-spy-2026", "0.5"), "11"),
    ] {
        let refusal = service
            .apply_corporate_action(request(action, held))
            .unwrap_err();
        assert!(
            refusal.0.contains("the account holds 10.00000000 of"),
            "{refusal}"
        );
        assert!(
            refusal.0.contains("reconcile before applying it"),
            "{refusal}"
        );
    }
    assert_eq!(books(&service), before);
}

#[test]
fn an_action_cannot_be_applied_before_it_takes_effect() {
    let mut service = service();
    buy(
        &mut service,
        "intent-ca-buy",
        "10",
        "100",
        "2026-01-02T14:31:00Z",
    );
    let before = books(&service);
    let early = PaperCorporateAction {
        applied_at: "2026-01-05T13:29:59Z".to_owned(),
        ..request(split("ca-split-spy-2026", SPY, "2"), "10")
    };
    let refusal = service.apply_corporate_action(early).unwrap_err();
    assert!(
        refusal
            .0
            .contains("cannot be applied at 2026-01-05T13:29:59Z"),
        "{refusal}"
    );
    assert_eq!(books(&service), before);
    // At the instant it takes effect it applies.
    let on_time = PaperCorporateAction {
        applied_at: "2026-01-05T13:30:00Z".to_owned(),
        ..request(split("ca-split-spy-2026", SPY, "2"), "10")
    };
    service.apply_corporate_action(on_time).unwrap();
}

#[test]
fn a_malformed_request_is_refused() {
    let mut service = service();
    let cases = [
        PaperCorporateAction {
            applied_by: "Operator Ops".to_owned(),
            ..request(dividend("ca-dividend-spy-2026", "0.5"), "0")
        },
        PaperCorporateAction {
            applied_at: "2026-01-05 13:31:00Z".to_owned(),
            ..request(dividend("ca-dividend-spy-2026", "0.5"), "0")
        },
        request(split("ca-split-spy-2026", SPY, "0"), "0"),
        request(dividend("ca dividend", "0.5"), "0"),
    ];
    for case in cases {
        assert!(service.apply_corporate_action(case).is_err());
    }
    assert!(service.corporate_actions().is_empty());
}

#[test]
fn a_cash_dividend_credits_a_long_and_debits_a_short_and_reconciles() {
    let mut long = service();
    buy(
        &mut long,
        "intent-ca-buy",
        "10",
        "100",
        "2026-01-02T14:31:00Z",
    );
    let cash = decimal("cash", &long.dashboard().internal_cash).unwrap();
    let action = dividend("ca-dividend-spy-2026", "0.37");
    long.broker_mut().apply_corporate_action(&action).unwrap();
    let receipt = long
        .apply_corporate_action(request(action.clone(), "10"))
        .unwrap();
    assert_eq!(receipt.cash_delta, decimal("credit", "3.70").unwrap());
    assert_eq!(receipt.quantity_after, decimal("held", "10").unwrap());
    assert_eq!(
        decimal("cash", &long.dashboard().internal_cash).unwrap(),
        cash.checked_add(receipt.cash_delta).unwrap()
    );
    assert_eq!(position(&long, SPY).quantity, "10.00000000");
    assert!(long.reconcile("2026-01-05T13:32:00Z").unwrap().is_clean());

    let mut short = service_permitting_shorts();
    trade(
        &mut short,
        "intent-ca-short",
        SPY,
        Side::Sell,
        "10",
        "100",
        "2026-01-02T14:31:00Z",
        true,
    );
    short.broker_mut().apply_corporate_action(&action).unwrap();
    let receipt = short
        .apply_corporate_action(request(action, "-10"))
        .unwrap();
    assert_eq!(receipt.cash_delta, decimal("debit", "-3.70").unwrap());
    assert!(short.reconcile("2026-01-05T13:32:00Z").unwrap().is_clean());
}

#[test]
fn a_dividend_applies_while_an_order_works_in_its_instrument() {
    // A dividend changes no quantity, so an order resting across it is unaffected.
    let mut service = service();
    buy(
        &mut service,
        "intent-ca-buy",
        "10",
        "100",
        "2026-01-02T14:31:00Z",
    );
    trade(
        &mut service,
        "intent-ca-rest",
        SPY,
        Side::Sell,
        "4",
        "100",
        "2026-01-02T14:33:00Z",
        false,
    );
    let receipt = service
        .apply_corporate_action(request(dividend("ca-dividend-spy-2026", "0.5"), "10"))
        .unwrap();
    assert_eq!(receipt.cash_delta, decimal("credit", "5").unwrap());
}

#[test]
fn an_action_on_a_flat_instrument_is_recorded_and_moves_nothing() {
    let mut service = service();
    let before = service.dashboard();
    let receipt = service
        .apply_corporate_action(request(split("ca-split-spy-2026", SPY, "2"), "0"))
        .unwrap();
    assert_eq!(
        (receipt.quantity_before, receipt.quantity_after),
        (Decimal::ZERO, Decimal::ZERO)
    );
    let receipt = service
        .apply_corporate_action(request(dividend("ca-dividend-spy-2026", "0.5"), "0"))
        .unwrap();
    assert_eq!(receipt.cash_delta, Decimal::ZERO);
    assert_eq!(service.dashboard().internal_cash, before.internal_cash);
    assert!(service.dashboard().positions.is_empty());
    assert_eq!(service.corporate_actions().len(), 2);
}

#[test]
fn an_action_applies_once_and_a_retry_changes_nothing() {
    let mut service = service();
    buy(
        &mut service,
        "intent-ca-buy",
        "10",
        "100",
        "2026-01-02T14:31:00Z",
    );
    let first = service
        .apply_corporate_action(request(dividend("ca-dividend-spy-2026", "0.5"), "10"))
        .unwrap();
    let after = books(&service);
    // The retry an operator sends when the first reply was lost: same terms, later.
    let retry = PaperCorporateAction {
        applied_at: "2026-01-05T13:45:00Z".to_owned(),
        ..request(dividend("ca-dividend-spy-2026", "0.5"), "10")
    };
    assert_eq!(service.apply_corporate_action(retry).unwrap(), first);
    assert_eq!(books(&service), after);
    // The same identity with other terms is refused.
    for (action, held) in [
        (dividend("ca-dividend-spy-2026", "0.6"), "10"),
        (split("ca-dividend-spy-2026", SPY, "2"), "10"),
    ] {
        let refusal = service
            .apply_corporate_action(request(action, held))
            .unwrap_err();
        assert!(
            refusal.0.contains("already applied with other terms"),
            "{refusal}"
        );
    }
    assert_eq!(books(&service), after);
}

#[test]
fn applied_actions_survive_a_restart_and_an_unused_journal_is_unchanged() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-corporate-actions-{}.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&journal_path);
    let open = || {
        PaperTradingService::open_durable(
            account(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account()).unwrap(),
            &journal_path,
        )
        .unwrap()
    };
    let mut service = open();
    buy(
        &mut service,
        "intent-ca-buy",
        "10",
        "100",
        "2026-01-02T14:31:00Z",
    );
    // A journal that has never applied an action does not mention one, so every
    // journal written before this field existed re-serializes byte for byte.
    drop(service);
    assert!(!fs::read_to_string(&journal_path)
        .unwrap()
        .contains("corporate_actions"));
    let mut service = open();
    let split_receipt = service
        .apply_corporate_action(request(split("ca-split-spy-2026", SPY, "2"), "10"))
        .unwrap();
    let dividend_receipt = service
        .apply_corporate_action(request(dividend("ca-dividend-spy-2026", "0.25"), "20"))
        .unwrap();
    let before = books(&service);
    drop(service);

    let reopened = open();
    assert_eq!(
        reopened.corporate_actions(),
        [split_receipt, dividend_receipt]
    );
    let (dashboard, lots, count) = books(&reopened);
    assert_eq!((lots, count), (before.1, before.2));
    assert_eq!(dashboard.positions, before.0.positions);
    assert_eq!(dashboard.internal_cash, before.0.internal_cash);
    // The retry after a restart is recognised too.
    let mut reopened = reopened;
    let retry = reopened
        .apply_corporate_action(request(split("ca-split-spy-2026", SPY, "2"), "10"))
        .unwrap();
    assert_eq!(retry, reopened.corporate_actions()[0]);
    drop(reopened);
    fs::remove_file(&journal_path).unwrap();
}

#[test]
fn a_journaled_action_whose_effect_does_not_follow_from_it_is_refused() {
    let receipt = PaperCorporateActionReceipt {
        action: split("ca-split-spy-2026", SPY, "2"),
        applied_by: "operator.ops".to_owned(),
        applied_at: "2026-01-05T13:31:00Z".to_owned(),
        quantity_before: decimal("before", "10").unwrap(),
        quantity_after: decimal("after", "20").unwrap(),
        cash_delta: Decimal::ZERO,
    };
    let persisted = PersistentCorporateAction::from(&receipt);
    assert_eq!(
        PaperCorporateActionReceipt::try_from(persisted.clone()).unwrap(),
        receipt
    );
    let dividend_receipt = PaperCorporateActionReceipt {
        action: dividend("ca-dividend-spy-2026", "0.5"),
        quantity_after: decimal("after", "10").unwrap(),
        cash_delta: decimal("cash", "5").unwrap(),
        ..receipt.clone()
    };
    let persisted_dividend = PersistentCorporateAction::from(&dividend_receipt);
    assert_eq!(
        PaperCorporateActionReceipt::try_from(persisted_dividend.clone()).unwrap(),
        dividend_receipt
    );
    for (name, refused) in [
        (
            "a split that moved cash",
            PersistentCorporateAction {
                cash_delta: "1".to_owned(),
                ..persisted.clone()
            },
        ),
        (
            "a split that did not scale",
            PersistentCorporateAction {
                quantity_after: "10".to_owned(),
                ..persisted.clone()
            },
        ),
        (
            "a dividend that moved the position",
            PersistentCorporateAction {
                quantity_after: "11".to_owned(),
                ..persisted_dividend.clone()
            },
        ),
        (
            "a dividend that credited too much",
            PersistentCorporateAction {
                cash_delta: "6".to_owned(),
                ..persisted_dividend.clone()
            },
        ),
        (
            "an unknown type",
            PersistentCorporateAction {
                action_type: "MERGER".to_owned(),
                ..persisted.clone()
            },
        ),
        (
            "an early application",
            PersistentCorporateAction {
                applied_at: "2026-01-05T13:29:00Z".to_owned(),
                ..persisted.clone()
            },
        ),
        (
            "an operator that is not canonical",
            PersistentCorporateAction {
                applied_by: "Ops".to_owned(),
                ..persisted.clone()
            },
        ),
    ] {
        assert!(
            PaperCorporateActionReceipt::try_from(refused).is_err(),
            "{name}"
        );
    }
}

#[test]
fn a_journal_that_applies_one_action_twice_is_refused() {
    let first = PersistentCorporateAction::from(&PaperCorporateActionReceipt {
        action: dividend("ca-dividend-spy-2026", "0.5"),
        applied_by: "operator.ops".to_owned(),
        applied_at: "2026-01-05T13:31:00Z".to_owned(),
        quantity_before: decimal("before", "10").unwrap(),
        quantity_after: decimal("after", "10").unwrap(),
        cash_delta: decimal("cash", "5").unwrap(),
    });
    let other = PersistentCorporateAction {
        action_id: "ca-dividend-spy-2027".to_owned(),
        ..first.clone()
    };
    assert_eq!(
        restore_corporate_actions(vec![first.clone(), other])
            .unwrap()
            .len(),
        2
    );
    let refusal = restore_corporate_actions(vec![first.clone(), first]).unwrap_err();
    assert_eq!(
        refusal.0,
        "paper journal applies one corporate action twice"
    );
}

#[test]
fn a_split_that_would_round_the_cached_mark_to_nothing_is_refused() {
    // A refused order still caches its observation as the instrument's mark.
    let mut service = service();
    let at = "2026-01-02T14:31:00Z";
    let refused = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "101").unwrap(),
                ..intent("intent-ca-mark", at)
            },
            market_at_price("0.00000001", at),
            at,
        )
        .unwrap();
    assert!(!refused.decision.approved);
    let refusal = service
        .apply_corporate_action(request(split("ca-split-spy-2026", SPY, "2"), "0"))
        .unwrap_err();
    assert!(
        refusal.0.contains("mark of 0.00000001 down to nothing"),
        "{refusal}"
    );
    assert!(service.corporate_actions().is_empty());
}
