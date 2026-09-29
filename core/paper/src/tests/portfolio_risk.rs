//! Portfolio-wide risk composition and its durable state.

use super::*;

/// A permissive aggregate policy: every real limit is wide enough that
/// nothing rejects until a test deliberately tightens one field.
/// `max_drawdown_bps` at `10000` (100%) can never trigger -- the ratio is
/// always strictly below `10000` since equity must be positive to reach
/// this kernel at all -- unlike a `0` sentinel, which would be the
/// tightest possible drawdown limit now that drawdown is genuinely
/// computed (Slice 2), not permanently zeroed (Slice 1).
fn permissive_portfolio_risk_policy() -> follon_risk::PortfolioRiskPolicy {
    follon_risk::PortfolioRiskPolicy {
        version: "portfolio-risk-v1".to_owned(),
        global_kill_switch: false,
        max_gross_exposure: decimal("gross", "100000").unwrap(),
        max_abs_net_exposure: decimal("net", "100000").unwrap(),
        max_leverage_bps: decimal("leverage", "10000").unwrap(),
        max_concentration_bps: decimal("concentration", "10000").unwrap(),
        // Never trip on daily loss unless a test deliberately overrides
        // it: `i64::MAX`, the same "no real limit" sentinel the CLI
        // loader uses when an operator omits `max_daily_loss`.
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
    }
}

#[test]
fn every_portfolio_risk_limit_is_part_of_the_configuration_fingerprint() {
    // Version 1 of the portfolio-risk fingerprint part omitted these, so a
    // journal reopened under any change to them (delivery state E7.4).
    let fingerprint = |change: &dyn Fn(&mut PortfolioRiskComposition)| {
        let mut composition = PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        };
        change(&mut composition);
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(composition);
        service_with(risk_policy).configuration_fingerprint()
    };
    let base = fingerprint(&|_| {});
    type Change = Box<dyn Fn(&mut PortfolioRiskComposition)>;
    let changes: Vec<(&str, Change)> = vec![
        (
            "max_daily_loss",
            Box::new(|c| {
                c.policy.max_daily_loss = decimal("loss", "1234").unwrap();
            }),
        ),
        (
            "max_drawdown_bps",
            Box::new(|c| {
                c.policy.max_drawdown_bps = decimal("drawdown", "1234").unwrap();
            }),
        ),
        (
            "max_margin_utilization_bps",
            Box::new(|c| {
                c.policy.max_margin_utilization_bps = decimal("margin", "1234").unwrap();
            }),
        ),
        (
            "strategy_limits",
            Box::new(|c| {
                c.policy.strategy_limits.insert(
                    "strategy.paper.001".to_owned(),
                    decimal("limit", "1000").unwrap(),
                );
            }),
        ),
        (
            "margin_rates",
            Box::new(|c| {
                c.margin_rates = Some(BTreeMap::from([(
                    "equity".to_owned(),
                    follon_accounting::MarginRate {
                        initial_bps: 5_000,
                        maintenance_bps: 2_500,
                    },
                )]));
            }),
        ),
    ];
    for (limit, change) in &changes {
        assert_ne!(
            fingerprint(change.as_ref()),
            base,
            "{limit} is not in the fingerprint"
        );
    }
}

#[test]
fn paper_portfolio_risk_composition_rejects_when_gross_exposure_limit_is_exceeded() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    portfolio_policy.max_gross_exposure = decimal("gross", "50").unwrap();
    let mut risk_policy = policy();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();
    let result = service
        .submit_intent(
            intent("intent-portfolio-risk-gross", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result.order_id.is_none());
    assert!(result
        .decision
        .reason_codes
        .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_gross_exposure=100.00000000"));
}

#[test]
fn paper_portfolio_risk_composition_rejects_when_sector_bucket_limit_is_exceeded() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    portfolio_policy
        .sector_limits
        .insert("index".to_owned(), decimal("sector limit", "50").unwrap());
    let mut instrument_buckets = BTreeMap::new();
    instrument_buckets.insert(
        "inst.us_equity.spy".to_owned(),
        InstrumentBucket {
            asset_class: "equity".to_owned(),
            currency: "USD".to_owned(),
            sector: "index".to_owned(),
        },
    );
    let mut risk_policy = policy();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets,
        margin_rates: None,
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();
    let result = service
        .submit_intent(
            intent("intent-portfolio-risk-sector", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"SECTOR_LIMIT_EXCEEDED:index".to_owned()));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_sector_gross=index:100.00000000"));
}

#[test]
fn paper_portfolio_risk_composition_rejects_a_restricted_instrument() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    portfolio_policy
        .restricted_instruments
        .insert("inst.us_equity.spy".to_owned());
    let mut risk_policy = policy();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();
    let result = service
        .submit_intent(
            intent("intent-portfolio-risk-restricted", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"RESTRICTED_INSTRUMENT".to_owned()));
}

/// A service with the permissive aggregate policy composed and short exposure
/// permitted, whose per-order bounds are wide enough for a blown-up mark.
fn service_with_aggregate_risk_and_shorts() -> PaperTradingService<IbkrPaperAdapter> {
    let mut risk_policy = PaperRiskPolicy {
        max_order_quantity: decimal("quantity", "1000").unwrap(),
        max_order_notional: decimal("notional", "1000000").unwrap(),
        ..policy_permitting_shorts()
    };
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: permissive_portfolio_risk_policy(),
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    service_with(risk_policy)
}

/// With equity not positive no aggregate ratio can be computed. Skipping the
/// limits then would let an underwater account open exposure past every one of
/// them, so only a trade that moves a position toward flat passes (delivery
/// state E7.4b).
#[test]
fn an_underwater_paper_account_may_only_reduce_a_position() {
    let mut service = service_with_aggregate_risk_and_shorts();
    let short = OrderIntent {
        side: Side::Sell,
        quantity: decimal("quantity", "100").unwrap(),
        ..intent("intent-underwater-short", "2026-01-02T14:31:00Z")
    };
    let opened = service
        .submit_intent(
            short,
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(
        opened.decision.approved,
        "{:?}",
        opened.decision.reason_codes
    );
    service
        .broker_mut()
        .queue_fill(
            &opened.order_id.unwrap(),
            decimal("quantity", "100").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    service.synchronize().unwrap();
    // Cash is now 110,000 against a short of 100. The mark rising to 1,200
    // takes equity to 110,000 - 100 * 1,200 = -10,000.
    let assess = |service: &mut PaperTradingService<IbkrPaperAdapter>,
                  id: &str,
                  side: Side,
                  quantity: &str| {
        service
            .submit_intent(
                OrderIntent {
                    side,
                    quantity: decimal("quantity", quantity).unwrap(),
                    ..intent(id, "2026-01-02T14:32:00Z")
                },
                market_at_price("1200", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap()
    };
    let refused = "PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned();

    // Adding to the short, and reversing it through flat, are refused.
    let adding = assess(&mut service, "intent-underwater-add", Side::Sell, "10");
    assert!(!adding.decision.approved);
    assert!(adding.order_id.is_none());
    assert!(adding.decision.reason_codes.contains(&refused));
    // The kernel that cannot run is not reported as having run.
    assert!(!adding
        .decision
        .evaluated_limits
        .contains("portfolio_gross_exposure"));
    let reversing = assess(&mut service, "intent-underwater-flip", Side::Buy, "150");
    assert!(reversing.decision.reason_codes.contains(&refused));

    // Buying part of the short back moves it toward flat, so it passes.
    let reducing = assess(&mut service, "intent-underwater-reduce", Side::Buy, "50");
    assert!(
        reducing.decision.approved,
        "{:?}",
        reducing.decision.reason_codes
    );
    assert!(reducing.order_id.is_some());
}

/// A combination is one atomic group, so it reduces risk only if every leg
/// moves its own position toward flat.
#[test]
fn an_underwater_paper_account_may_only_close_every_leg_of_a_combination() {
    let mut service = service_with_aggregate_risk_and_shorts();
    // The standard vertical, filled: long 4 near, short 4 far.
    let opening = submit_lifecycle_combo(&mut service, "underwater-open");
    let fill = combo_execution(&service, &opening, "group-underwater", "4");
    service.broker.queue_combo_fill(fill).unwrap();
    service.synchronize().unwrap();
    // The short leg's mark rises to 30,000, taking equity far below zero.
    let market = PaperComboMarketData {
        marks: vec![
            PaperMarketData {
                instrument_id: "inst.us_option.spy.near".to_owned(),
                mark_price: decimal("mark", "7.50").unwrap(),
                observed_at: "2026-01-02T14:32:00Z".to_owned(),
            },
            PaperMarketData {
                instrument_id: "inst.us_option.spy.far".to_owned(),
                mark_price: decimal("mark", "30000").unwrap(),
                observed_at: "2026-01-02T14:32:00Z".to_owned(),
            },
        ],
    };
    let assess = |service: &mut PaperTradingService<IbkrPaperAdapter>,
                  id: &str,
                  near: Side,
                  far: Side,
                  limit: follon_domain::ComboPriceLimit| {
        let mut structure = combo_intent(id, "2026-01-02T14:32:00Z");
        structure.combo_quantity = decimal("units", "1").unwrap();
        structure.legs[0].side = near;
        structure.legs[0].limit_price = decimal("near", "7.50").unwrap();
        structure.legs[1].side = far;
        structure.legs[1].limit_price = decimal("far", "30000").unwrap();
        structure.price_limit = limit;
        service
            .submit_combo_intent(structure, market.clone(), "2026-01-02T14:32:00Z")
            .unwrap()
    };
    let refused = "PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned();

    // Adding to both legs, and closing one leg while adding to the other, are refused.
    let adding = assess(
        &mut service,
        "underwater-add",
        Side::Buy,
        Side::Sell,
        follon_domain::ComboPriceLimit::MinimumCredit(decimal("credit", "29992.50").unwrap()),
    );
    assert!(adding.decision.reason_codes.contains(&refused));
    assert!(adding.order_id.is_none());
    let mixed = assess(
        &mut service,
        "underwater-mixed",
        Side::Sell,
        Side::Sell,
        follon_domain::ComboPriceLimit::MinimumCredit(decimal("credit", "30007.50").unwrap()),
    );
    assert!(mixed.decision.reason_codes.contains(&refused));
    assert!(mixed.order_id.is_none());
    // Order does not matter: the leg that adds may come first or last.
    let mixed_the_other_way = assess(
        &mut service,
        "underwater-mixed-reversed",
        Side::Buy,
        Side::Buy,
        follon_domain::ComboPriceLimit::MaximumDebit(decimal("debit", "30007.50").unwrap()),
    );
    assert!(mixed_the_other_way.decision.reason_codes.contains(&refused));

    // Closing one unit of both legs passes.
    let closing = assess(
        &mut service,
        "underwater-close",
        Side::Sell,
        Side::Buy,
        follon_domain::ComboPriceLimit::MaximumDebit(decimal("debit", "29992.50").unwrap()),
    );
    assert!(
        closing.decision.approved,
        "{:?}",
        closing.decision.reason_codes
    );
    assert!(closing.order_id.is_some());
}

#[test]
fn paper_portfolio_risk_kernel_is_not_run_when_equity_is_not_positive() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    // Tight enough that a positive-equity account would certainly reject
    // on this limit -- its absence from the rejection below is what
    // proves composition was skipped, not merely satisfied.
    portfolio_policy.max_gross_exposure = decimal("gross", "0.01").unwrap();
    let mut risk_policy = policy();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let zero_cash_account = PaperAccount {
        initial_cash: Decimal::ZERO,
        ..account()
    };
    let mut service = PaperTradingService::new(
        zero_cash_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&zero_cash_account).unwrap(),
    )
    .unwrap();
    let result = service
        .submit_intent(
            intent("intent-portfolio-risk-zero-equity", "2026-01-02T14:31:00Z"),
            market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"INSUFFICIENT_INTERNAL_CASH".to_owned()));
    // An order that opens exposure is refused for the equity itself, not
    // waved through because its limits could not be computed (E7.4b).
    assert!(result
        .decision
        .reason_codes
        .contains(&"PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned()));
    // The kernel did not run, so none of its own limits is reported.
    assert!(!result
        .decision
        .reason_codes
        .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
    assert!(!result
        .decision
        .evaluated_limits
        .contains("portfolio_gross_exposure"));
}

#[test]
fn paper_portfolio_risk_composition_rejects_when_drawdown_limit_is_exceeded() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    portfolio_policy.max_drawdown_bps = decimal("drawdown", "2000").unwrap();
    // Never trip on leverage/concentration; this test only wants
    // drawdown to be the exercised check.
    portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();

    // Buy and fill 1,000 shares at 100 -- real equity is ~100,000 cash
    // moved into a position of equal value, establishing a 100,000 peak.
    let filled = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "1000").unwrap(),
                ..intent("intent-drawdown-buy", "2026-01-02T14:31:00Z")
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1000").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);

    // The mark drops to 70: a real, computed 30% drawdown from the
    // 100,000 peak (equity is now 1,000 * 70 = 70,000), which exceeds the
    // configured 20% limit.
    let mut sell = intent("intent-drawdown-sell", "2026-01-02T14:32:00Z");
    sell.side = Side::Sell;
    sell.quantity = decimal("quantity", "1").unwrap();
    let result = service
        .submit_intent(
            sell,
            market_at_price("70", "2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"MAX_DRAWDOWN_EXCEEDED".to_owned()));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_peak_equity=100000.00000000"));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_drawdown_bps=3000.00000000"));
}

#[test]
fn paper_portfolio_risk_composition_rejects_when_daily_loss_limit_is_exceeded() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    portfolio_policy.max_daily_loss = decimal("daily_loss", "2000").unwrap();
    // Never trip on leverage/concentration/drawdown; this test only wants
    // daily loss to be the exercised check.
    portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();

    // The very first risk evaluation of the day establishes the
    // session-start baseline at pure cash equity (100,000; no position
    // exists yet).
    let filled = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "1000").unwrap(),
                ..intent("intent-daily-loss-buy", "2026-01-02T14:31:00Z")
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1000").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);

    // The mark drops to 70 later the same UTC day: real equity falls from
    // the baseline's 100,000 (pure cash, observed before the buy filled)
    // to 70,000 -- a genuine 30,000 daily loss exceeding the configured
    // 2,000 limit.
    let mut sell = intent("intent-daily-loss-sell", "2026-01-02T14:32:00Z");
    sell.side = Side::Sell;
    sell.quantity = decimal("quantity", "1").unwrap();
    let result = service
        .submit_intent(
            sell,
            market_at_price("70", "2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"MAX_DAILY_LOSS_EXCEEDED".to_owned()));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_daily_baseline_equity=100000.00000000"));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_daily_pnl=-30000.00000000"));
}

#[test]
fn paper_daily_loss_baseline_resets_at_a_new_utc_calendar_day() {
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1001").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: permissive_portfolio_risk_policy(),
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();

    // Buy and fill 1,000 shares at 100 -- the very first evaluation ever
    // establishes the day-1 baseline at pure cash equity (100,000).
    let filled = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "1000").unwrap(),
                ..intent("intent-daily-reset-buy", "2026-01-02T14:31:00Z")
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1000").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);

    // Re-quote to 150 later the same UTC day: equity rises to 150,000
    // relative to the still-standing day-1 baseline of 100,000, a real
    // +50,000 daily gain. This order is rejected on quantity alone, but
    // the mark update (and hence the daily-P&L read) is unconditional.
    let mut spike = intent("intent-daily-reset-spike", "2026-01-02T20:00:00Z");
    spike.quantity = decimal("quantity", "1002").unwrap();
    let spiked = service
        .submit_intent(
            spike,
            market_at_price("150", "2026-01-02T20:00:00Z"),
            "2026-01-02T20:00:00Z",
        )
        .unwrap();
    assert!(!spiked.decision.approved);
    assert!(spiked
        .decision
        .evaluated_limits
        .contains("portfolio_daily_baseline_equity=100000.00000000"));
    assert!(spiked
        .decision
        .evaluated_limits
        .contains("portfolio_daily_pnl=50000.00000000"));

    // The next UTC calendar day resets the baseline to that day's own
    // first observed equity (150,000, the mark is unchanged), not the
    // prior day's 100,000 -- proving a genuine reset rather than a
    // carried-over accumulation.
    let mut next_day = intent("intent-daily-reset-day2", "2026-01-03T09:00:00Z");
    next_day.quantity = decimal("quantity", "1002").unwrap();
    let after_rollover = service
        .submit_intent(
            next_day,
            market_at_price("150", "2026-01-03T09:00:00Z"),
            "2026-01-03T09:00:00Z",
        )
        .unwrap();
    assert!(!after_rollover.decision.approved);
    assert!(after_rollover
        .decision
        .evaluated_limits
        .contains("portfolio_daily_baseline_equity=150000.00000000"));
    assert!(after_rollover
        .decision
        .evaluated_limits
        .contains("portfolio_daily_pnl=0.00000000"));
}

#[test]
fn paper_daily_loss_baseline_survives_a_durable_journal_reopen() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-daily-loss-baseline.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&journal_path);
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1001").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: permissive_portfolio_risk_policy(),
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut durable = PaperTradingService::open_durable(
        paper_account.clone(),
        risk_policy.clone(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();

    // Buy and fill 1,000 shares at 100 -- the very first evaluation ever
    // establishes the baseline at pure cash equity (100,000).
    let filled = durable
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "1000").unwrap(),
                ..intent("intent-daily-durability-buy", "2026-01-02T14:31:00Z")
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    durable
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1000").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(durable.synchronize().unwrap(), 2);

    // Re-quote to 150: equity rises to 150,000 -- a real +50,000 gain
    // against the day's still-standing 100,000 baseline. This order is
    // rejected on quantity alone, but the mark update (and hence the
    // daily-P&L read) is unconditional.
    let mut spike = intent("intent-daily-durability-spike", "2026-01-02T20:00:00Z");
    spike.quantity = decimal("quantity", "1002").unwrap();
    let spiked = durable
        .submit_intent(
            spike,
            market_at_price("150", "2026-01-02T20:00:00Z"),
            "2026-01-02T20:00:00Z",
        )
        .unwrap();
    assert!(!spiked.decision.approved);
    assert!(spiked
        .decision
        .evaluated_limits
        .contains("portfolio_daily_baseline_equity=100000.00000000"));
    drop(durable);

    // Reopen later the same UTC day: the durable baseline (100,000) must
    // survive, not reset to today's current equity (150,000).
    let mut reopened = PaperTradingService::open_durable(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();
    reopened
        .reconnect_and_reconcile("2026-01-02T21:00:00Z")
        .unwrap();
    let mut probe = intent(
        "intent-daily-durability-after-reopen",
        "2026-01-02T21:00:00Z",
    );
    probe.quantity = decimal("quantity", "1002").unwrap();
    let result = reopened
        .submit_intent(
            probe,
            market_at_price("150", "2026-01-02T21:00:00Z"),
            "2026-01-02T21:00:00Z",
        )
        .unwrap();
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_daily_baseline_equity=100000.00000000"));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_daily_pnl=50000.00000000"));
    fs::remove_file(journal_path).unwrap();
}

#[test]
fn paper_portfolio_risk_composition_rejects_when_margin_utilization_limit_is_exceeded() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    portfolio_policy.max_margin_utilization_bps = decimal("margin", "4000").unwrap();
    // Never trip on leverage/concentration; this test only wants margin
    // utilization to be the exercised check.
    portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
    let mut instrument_buckets = BTreeMap::new();
    instrument_buckets.insert(
        "inst.us_equity.spy".to_owned(),
        InstrumentBucket {
            asset_class: "equity".to_owned(),
            currency: "USD".to_owned(),
            sector: "index".to_owned(),
        },
    );
    let mut margin_rates = BTreeMap::new();
    margin_rates.insert(
        "equity".to_owned(),
        follon_accounting::MarginRate {
            initial_bps: 5000,
            maintenance_bps: 2500,
        },
    );
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets,
        margin_rates: Some(margin_rates),
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();

    // Buy and fill 1,000 shares at 100 -- cash is fully spent, so equity
    // (100,000) equals the position's mark value exactly.
    let filled = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "1000").unwrap(),
                ..intent("intent-margin-buy", "2026-01-02T14:31:00Z")
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1000").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);

    // A second order against the same mark: real margin_used is now
    // 100,000 * 50% = 50,000 against equity of 100,000, a genuine 50%
    // utilization exceeding the configured 40% limit.
    let mut sell = intent("intent-margin-sell", "2026-01-02T14:32:00Z");
    sell.side = Side::Sell;
    sell.quantity = decimal("quantity", "1").unwrap();
    let result = service
        .submit_intent(
            sell,
            market_at_price("100", "2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"MAX_MARGIN_UTILIZATION_EXCEEDED".to_owned()));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_margin_used=50000.00000000"));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_margin_utilization_bps=5000.00000000"));
}

#[test]
fn paper_portfolio_risk_composition_fails_closed_when_a_held_position_has_no_margin_rate() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    let mut instrument_buckets = BTreeMap::new();
    instrument_buckets.insert(
        "inst.us_equity.spy".to_owned(),
        InstrumentBucket {
            asset_class: "equity".to_owned(),
            currency: "USD".to_owned(),
            sector: "index".to_owned(),
        },
    );
    // A margin_rates map that covers a *different* asset class than the
    // one actually held: `value_margin_account` requires a rate for
    // every asset class among currently held positions, so this must
    // fail closed with a technical error rather than silently treating
    // the uncovered position as zero margin.
    let mut margin_rates = BTreeMap::new();
    margin_rates.insert(
        "option".to_owned(),
        follon_accounting::MarginRate {
            initial_bps: 5000,
            maintenance_bps: 2500,
        },
    );
    portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets,
        margin_rates: Some(margin_rates),
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();
    let filled = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "1000").unwrap(),
                ..intent("intent-margin-gap-buy", "2026-01-02T14:31:00Z")
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1000").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);

    let mut sell = intent("intent-margin-gap-sell", "2026-01-02T14:32:00Z");
    sell.side = Side::Sell;
    sell.quantity = decimal("quantity", "1").unwrap();
    let error = service
        .submit_intent(
            sell,
            market_at_price("100", "2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap_err();
    assert!(error.0.contains("missing margin policy"));
}

#[test]
fn paper_portfolio_risk_composition_rejects_when_strategy_limit_is_exceeded() {
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    // Never trip on leverage/concentration/gross; this test only wants
    // the strategy-bucket check exercised.
    portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
    portfolio_policy.max_gross_exposure = decimal("gross", "1000000").unwrap();
    let mut strategy_limits = BTreeMap::new();
    strategy_limits.insert(
        "strategy.beta".to_owned(),
        decimal("limit", "25000").unwrap(),
    );
    portfolio_policy.strategy_limits = strategy_limits;
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut service = PaperTradingService::new(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
    )
    .unwrap();

    // Strategy "strategy.paper.001" (the default test strategy) buys and
    // fills 100 shares at 100 -- a real, durably attributed 10,000
    // position for that strategy alone.
    let filled = service
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "100").unwrap(),
                ..intent("intent-strategy-alpha-buy", "2026-01-02T14:31:00Z")
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    service
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "100").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(service.synchronize().unwrap(), 2);

    // A second, distinct strategy ("strategy.beta") submits a 300-share
    // buy of the *same* instrument at the same mark. Its own candidate
    // notional alone (30,000) already exceeds the configured 25,000
    // strategy.beta limit, independent of strategy.paper.001's already-
    // filled 10,000 position -- proving the two strategies are tracked
    // and limited separately, not pooled into one aggregate bucket.
    let mut beta_buy = intent("intent-strategy-beta-buy", "2026-01-02T14:32:00Z");
    beta_buy.strategy_id = "strategy.beta".to_owned();
    beta_buy.quantity = decimal("quantity", "300").unwrap();
    let result = service
        .submit_intent(
            beta_buy,
            market_at_price("100", "2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"STRATEGY_LIMIT_EXCEEDED:strategy.beta".to_owned()));
    // Total gross exposure reflects both strategies exactly (10,000 from
    // the filled strategy.paper.001 position plus 30,000 from the
    // rejected strategy.beta candidate), proving the per-strategy split
    // never mis-states the true aggregate.
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_gross_exposure=40000.00000000"));
    assert!(result
        .decision
        .evaluated_limits
        .contains("strategy.beta:30000.00000000"));
}

#[test]
fn paper_strategy_attribution_survives_a_durable_journal_reopen() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-strategy-attribution.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&journal_path);
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
    portfolio_policy.max_gross_exposure = decimal("gross", "1000000").unwrap();
    let mut strategy_limits = BTreeMap::new();
    strategy_limits.insert(
        "strategy.beta".to_owned(),
        decimal("limit", "25000").unwrap(),
    );
    portfolio_policy.strategy_limits = strategy_limits;
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut durable = PaperTradingService::open_durable(
        paper_account.clone(),
        risk_policy.clone(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();

    // Strategy "strategy.paper.001" buys and fills 100 shares at 100.
    let filled = durable
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "100").unwrap(),
                ..intent(
                    "intent-attribution-durability-alpha",
                    "2026-01-02T14:31:00Z",
                )
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    durable
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "100").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(durable.synchronize().unwrap(), 2);
    drop(durable);

    // Reopen: the durable per-strategy attribution (100 shares owned by
    // strategy.paper.001) must survive, so a fresh strategy.beta candidate
    // still sees the correct pre-existing gross exposure and its own
    // limit is still evaluated against exactly its own contribution.
    let mut reopened = PaperTradingService::open_durable(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();
    reopened
        .reconnect_and_reconcile("2026-01-02T14:32:00Z")
        .unwrap();
    let mut beta_buy = intent("intent-attribution-durability-beta", "2026-01-02T14:32:00Z");
    beta_buy.strategy_id = "strategy.beta".to_owned();
    beta_buy.quantity = decimal("quantity", "300").unwrap();
    let result = reopened
        .submit_intent(
            beta_buy,
            market_at_price("100", "2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"STRATEGY_LIMIT_EXCEEDED:strategy.beta".to_owned()));
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_gross_exposure=40000.00000000"));
    fs::remove_file(journal_path).unwrap();
}

#[test]
fn paper_portfolio_risk_composition_uses_a_durable_mark_cache_after_journal_reopen() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-portfolio-risk-marks.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&journal_path);
    let mut portfolio_policy = permissive_portfolio_risk_policy();
    portfolio_policy.max_gross_exposure = decimal("gross", "200").unwrap();
    let mut risk_policy = policy();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: portfolio_policy,
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut durable = PaperTradingService::open_durable(
        paper_account.clone(),
        risk_policy.clone(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();

    // Buy and fill instrument A (spy) at 90, so its average cost is 90.
    let filled = durable
        .submit_intent(
            intent("intent-portfolio-risk-marks-a", "2026-01-02T14:31:00Z"),
            market_at_price("90", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    durable
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1").unwrap(),
            decimal("price", "90").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(durable.synchronize().unwrap(), 2);

    // Re-quote A at 150. This order is rejected on quantity alone, but the
    // mark observation is cached unconditionally before any check runs.
    let mut requote = intent(
        "intent-portfolio-risk-marks-requote",
        "2026-01-02T14:32:00Z",
    );
    requote.quantity = decimal("quantity", "1000").unwrap();
    let requoted = durable
        .submit_intent(
            requote,
            market_at_price("150", "2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    assert!(!requoted.decision.approved);
    assert!(requoted
        .decision
        .reason_codes
        .contains(&"MAX_ORDER_QUANTITY_EXCEEDED".to_owned()));
    drop(durable);

    // Reopen: the cached mark for A (150) must survive and be used, not
    // fall back to its average cost (90).
    let mut reopened = PaperTradingService::open_durable(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();
    reopened
        .reconnect_and_reconcile("2026-01-02T14:33:00Z")
        .unwrap();
    let mut second_intent = intent("intent-portfolio-risk-marks-b", "2026-01-02T14:33:00Z");
    second_intent.instrument_id = "inst.us_equity.qqq".to_owned();
    let second_market = PaperMarketData {
        instrument_id: "inst.us_equity.qqq".to_owned(),
        mark_price: decimal("mark", "60").unwrap(),
        observed_at: "2026-01-02T14:33:00Z".to_owned(),
    };
    let result = reopened
        .submit_intent(second_intent, second_market, "2026-01-02T14:33:00Z")
        .unwrap();
    assert!(!result.decision.approved);
    assert!(result
        .decision
        .reason_codes
        .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
    // 1 (A) * 150 (cached mark, not the 90 average cost) + 1 (B) * 60.
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_gross_exposure=210.00000000"));
    fs::remove_file(journal_path).unwrap();
}

#[test]
fn paper_peak_equity_survives_a_durable_journal_reopen() {
    let journal_path = std::env::temp_dir().join(format!(
        "follon-paper-journal-{}-peak-equity.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&journal_path);
    let mut risk_policy = policy();
    risk_policy.max_order_quantity = decimal("quantity", "1001").unwrap();
    risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
    risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
    risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
        policy: permissive_portfolio_risk_policy(),
        instrument_buckets: BTreeMap::new(),
        margin_rates: None,
    });
    let paper_account = account();
    let mut durable = PaperTradingService::open_durable(
        paper_account.clone(),
        risk_policy.clone(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();

    // Buy and fill 1,000 shares at 100 -- equity starts at 100,000.
    let filled = durable
        .submit_intent(
            OrderIntent {
                quantity: decimal("quantity", "1000").unwrap(),
                ..intent("intent-peak-equity-buy", "2026-01-02T14:31:00Z")
            },
            market_at_price("100", "2026-01-02T14:31:00Z"),
            "2026-01-02T14:31:00Z",
        )
        .unwrap();
    assert!(filled.decision.approved);
    let order_id = filled.order_id.unwrap();
    durable
        .broker_mut()
        .queue_fill(
            &order_id,
            decimal("quantity", "1000").unwrap(),
            decimal("price", "100").unwrap(),
            Decimal::ZERO,
            "2026-01-02T14:31:01Z",
        )
        .unwrap();
    assert_eq!(durable.synchronize().unwrap(), 2);

    // Re-quote to 150: a real mark-to-market gain pushes equity to
    // 150,000, raising the peak. This order is rejected on quantity
    // alone, but the peak-equity update is unconditional (see
    // `evaluate_risk`).
    let mut spike = intent("intent-peak-equity-spike", "2026-01-02T14:32:00Z");
    spike.quantity = decimal("quantity", "1002").unwrap();
    let spiked = durable
        .submit_intent(
            spike,
            market_at_price("150", "2026-01-02T14:32:00Z"),
            "2026-01-02T14:32:00Z",
        )
        .unwrap();
    assert!(!spiked.decision.approved);
    assert!(spiked
        .decision
        .evaluated_limits
        .contains("portfolio_peak_equity=150000.00000000"));

    // Re-quote back down to 100: equity falls back to 100,000, but the
    // peak must not fall with it.
    let mut retreat = intent("intent-peak-equity-retreat", "2026-01-02T14:33:00Z");
    retreat.quantity = decimal("quantity", "1002").unwrap();
    let retreated = durable
        .submit_intent(
            retreat,
            market_at_price("100", "2026-01-02T14:33:00Z"),
            "2026-01-02T14:33:00Z",
        )
        .unwrap();
    assert!(!retreated.decision.approved);
    assert!(retreated
        .decision
        .evaluated_limits
        .contains("portfolio_peak_equity=150000.00000000"));
    drop(durable);

    // Reopen: the durable peak (150,000) must survive, not reset to
    // today's current equity (100,000).
    let mut reopened = PaperTradingService::open_durable(
        paper_account.clone(),
        risk_policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&paper_account).unwrap(),
        &journal_path,
    )
    .unwrap();
    reopened
        .reconnect_and_reconcile("2026-01-02T14:34:00Z")
        .unwrap();
    let mut probe = intent("intent-peak-equity-after-reopen", "2026-01-02T14:34:00Z");
    probe.quantity = decimal("quantity", "1002").unwrap();
    let result = reopened
        .submit_intent(
            probe,
            market_at_price("100", "2026-01-02T14:34:00Z"),
            "2026-01-02T14:34:00Z",
        )
        .unwrap();
    assert!(result
        .decision
        .evaluated_limits
        .contains("portfolio_peak_equity=150000.00000000"));
    fs::remove_file(journal_path).unwrap();
}

/// The `portfolio_risk` block every PAPER application reads (delivery state
/// E7.5): strict, exact, and identical whichever application opens it.
mod document {
    use super::*;

    fn parse(json: &str) -> Result<PortfolioRiskDocument, serde_json::Error> {
        serde_json::from_str(json)
    }

    const MINIMAL: &str = r#"{
        "policy_version": "portfolio-risk-v1",
        "max_gross_exposure": "500000",
        "max_abs_net_exposure": "400000",
        "max_leverage_bps": "20000",
        "max_concentration_bps": "6000"
    }"#;

    #[test]
    fn an_absent_optional_limit_is_a_limit_that_can_never_trip() {
        let composition = parse(MINIMAL).unwrap().into_composition().unwrap();
        let policy = &composition.policy;
        assert_eq!(
            policy.max_gross_exposure,
            decimal("gross", "500000").unwrap()
        );
        assert_eq!(
            policy.max_concentration_bps,
            decimal("share", "6000").unwrap()
        );
        // 100% drawdown and margin utilisation cannot be reached, and no
        // account's session P&L reaches `i64::MAX`.
        assert_eq!(
            policy.max_drawdown_bps,
            decimal("drawdown", "10000").unwrap()
        );
        assert_eq!(
            policy.max_margin_utilization_bps,
            decimal("margin", "10000").unwrap()
        );
        assert_eq!(
            policy.max_daily_loss,
            Decimal::from_integer(i64::MAX).unwrap()
        );
        // core/paper enforces open-order count and rate itself, so the composed
        // kernel's copies stay permanently permissive.
        assert_eq!(policy.max_open_orders, usize::MAX);
        assert_eq!(policy.max_order_rate, u32::MAX);
        assert!(!policy.global_kill_switch);
        assert!(composition.margin_rates.is_none());
        assert!(composition.instrument_buckets.is_empty());
    }

    #[test]
    fn every_configured_limit_reaches_the_composition() {
        let composition = parse(
            r#"{
                "policy_version": "portfolio-risk-v2",
                "max_gross_exposure": "500000",
                "max_abs_net_exposure": "400000",
                "max_leverage_bps": "20000",
                "max_concentration_bps": "6000",
                "max_drawdown_bps": "3000",
                "max_daily_loss": "5000",
                "max_margin_utilization_bps": "4000",
                "allowed_instruments": ["inst.us_equity.spy"],
                "restricted_instruments": ["inst.us_equity.qqq"],
                "sector_limits": { "index": "250000" },
                "asset_class_limits": { "equity": "300000" },
                "currency_limits": { "USD": "450000" },
                "strategy_limits": { "strategy.paper.001": "300000" },
                "instrument_buckets": {
                    "inst.us_equity.spy": { "asset_class": "equity", "currency": "USD", "sector": "index" }
                },
                "margin_rates": { "equity": { "initial_bps": 5000, "maintenance_bps": 2500 } }
            }"#,
        )
        .unwrap()
        .into_composition()
        .unwrap();
        let policy = &composition.policy;
        assert_eq!(policy.version, "portfolio-risk-v2");
        assert_eq!(
            policy.max_drawdown_bps,
            decimal("drawdown", "3000").unwrap()
        );
        assert_eq!(policy.max_daily_loss, decimal("loss", "5000").unwrap());
        assert_eq!(
            policy.max_margin_utilization_bps,
            decimal("margin", "4000").unwrap()
        );
        assert!(policy.allowed_instruments.contains("inst.us_equity.spy"));
        assert!(policy.restricted_instruments.contains("inst.us_equity.qqq"));
        assert_eq!(
            policy.sector_limits["index"],
            decimal("limit", "250000").unwrap()
        );
        assert_eq!(
            policy.asset_class_limits["equity"],
            decimal("limit", "300000").unwrap()
        );
        assert_eq!(
            policy.currency_limits["USD"],
            decimal("limit", "450000").unwrap()
        );
        assert_eq!(
            policy.strategy_limits["strategy.paper.001"],
            decimal("limit", "300000").unwrap()
        );
        assert_eq!(
            composition.instrument_buckets["inst.us_equity.spy"],
            InstrumentBucket {
                asset_class: "equity".to_owned(),
                currency: "USD".to_owned(),
                sector: "index".to_owned(),
            }
        );
        let rate = &composition.margin_rates.as_ref().unwrap()["equity"];
        assert_eq!((rate.initial_bps, rate.maintenance_bps), (5000, 2500));
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        assert!(parse(&MINIMAL.replace("}", r#", "max_gross_expsure": "1" }"#)).is_err());
        assert!(parse(
            r#"{ "policy_version": "v", "max_gross_exposure": "1", "max_abs_net_exposure": "1",
                 "max_leverage_bps": "1", "max_concentration_bps": "1",
                 "margin_rates": { "equity": { "initial_bps": 1, "maintenance_bps": 1, "extra": 1 } } }"#
        )
        .is_err());
    }

    #[test]
    fn a_malformed_limit_names_the_field_it_came_from() {
        let error = parse(&MINIMAL.replace("500000", "lots"))
            .unwrap()
            .into_composition()
            .unwrap_err();
        assert!(error.0.contains("max_gross_exposure"), "{}", error.0);
        let error = parse(&MINIMAL.replace(
            r#""6000""#,
            r#""6000", "sector_limits": { "index": "many" }"#,
        ))
        .unwrap()
        .into_composition()
        .unwrap_err();
        assert!(error.0.contains("sector_limits"), "{}", error.0);
    }
}
