//! Unit tests for portfolio risk aggregation and pre-trade controls.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use follon_domain::{Decimal, Side};
use follon_fx::{FxOutrightQuote, FxPair, FxPriceTerms, FxPricingSnapshot, FxProduct, FxValueDate};

use super::*;

fn amount(value: &str) -> Decimal {
    Decimal::from_str(value).expect("decimal")
}

fn policy() -> PortfolioRiskPolicy {
    PortfolioRiskPolicy {
        version: "portfolio.risk.v1".to_owned(),
        global_kill_switch: false,
        max_gross_exposure: amount("100000"),
        max_abs_net_exposure: amount("50000"),
        max_leverage_bps: amount("20000"),
        max_concentration_bps: amount("9000"),
        max_daily_loss: amount("5000"),
        max_drawdown_bps: amount("1000"),
        max_margin_utilization_bps: amount("5000"),
        max_abs_delta: amount("1000"),
        max_abs_gamma: amount("100"),
        max_open_orders: 10,
        max_order_rate: 20,
        allowed_instruments: BTreeSet::new(),
        restricted_instruments: BTreeSet::new(),
        sector_limits: BTreeMap::from([("technology".to_owned(), amount("40000"))]),
        asset_class_limits: BTreeMap::new(),
        currency_limits: BTreeMap::from([("USD".to_owned(), amount("100000"))]),
        strategy_limits: BTreeMap::new(),
        max_news_slippage_bps: None,
        max_spread_multiplier_bps: None,
    }
}

#[test]
fn canonical_parts_change_with_every_field_of_the_policy() {
    // Each change touches exactly one field; the exhaustive destructuring
    // makes a new field a compile error, and this makes a dropped one a
    // test failure (delivery state E7.4).
    let base = policy().canonical_parts();
    type Change = Box<dyn Fn(&mut PortfolioRiskPolicy)>;
    let changes: Vec<(&str, Change)> = vec![
        (
            "version",
            Box::new(|p| p.version = "portfolio.risk.v2".to_owned()),
        ),
        (
            "global_kill_switch",
            Box::new(|p| p.global_kill_switch = true),
        ),
        (
            "max_gross_exposure",
            Box::new(|p| p.max_gross_exposure = amount("99999")),
        ),
        (
            "max_abs_net_exposure",
            Box::new(|p| p.max_abs_net_exposure = amount("49999")),
        ),
        (
            "max_leverage_bps",
            Box::new(|p| p.max_leverage_bps = amount("19999")),
        ),
        (
            "max_concentration_bps",
            Box::new(|p| p.max_concentration_bps = amount("8999")),
        ),
        (
            "max_daily_loss",
            Box::new(|p| p.max_daily_loss = amount("4999")),
        ),
        (
            "max_drawdown_bps",
            Box::new(|p| p.max_drawdown_bps = amount("999")),
        ),
        (
            "max_margin_utilization_bps",
            Box::new(|p| p.max_margin_utilization_bps = amount("4999")),
        ),
        (
            "max_abs_delta",
            Box::new(|p| p.max_abs_delta = amount("999")),
        ),
        (
            "max_abs_gamma",
            Box::new(|p| p.max_abs_gamma = amount("99")),
        ),
        ("max_open_orders", Box::new(|p| p.max_open_orders = 9)),
        ("max_order_rate", Box::new(|p| p.max_order_rate = 19)),
        (
            "allowed_instruments",
            Box::new(|p| {
                p.allowed_instruments
                    .insert("inst.us_equity.spy".to_owned());
            }),
        ),
        (
            "restricted_instruments",
            Box::new(|p| {
                p.restricted_instruments
                    .insert("inst.us_equity.spy".to_owned());
            }),
        ),
        (
            "sector_limits",
            Box::new(|p| {
                p.sector_limits
                    .insert("technology".to_owned(), amount("39999"));
            }),
        ),
        (
            "asset_class_limits",
            Box::new(|p| {
                p.asset_class_limits
                    .insert("equity".to_owned(), amount("1"));
            }),
        ),
        (
            "currency_limits",
            Box::new(|p| {
                p.currency_limits.insert("USD".to_owned(), amount("99999"));
            }),
        ),
        (
            "strategy_limits",
            Box::new(|p| {
                p.strategy_limits
                    .insert("strategy.alpha".to_owned(), amount("1"));
            }),
        ),
        (
            "max_news_slippage_bps",
            Box::new(|p| p.max_news_slippage_bps = Some(amount("10"))),
        ),
        (
            "max_spread_multiplier_bps",
            Box::new(|p| p.max_spread_multiplier_bps = Some(amount("10"))),
        ),
    ];
    assert_eq!(changes.len(), 21, "one change per field");
    for (field, change) in &changes {
        let mut changed = policy();
        change(&mut changed);
        assert_ne!(
            changed.canonical_parts(),
            base,
            "{field} is not in the canonical parts"
        );
    }
}

fn position(instrument: &str, quantity: &str, price: &str) -> RiskPosition {
    RiskPosition {
        account_id: "account.main".to_owned(),
        strategy_id: "strategy.alpha".to_owned(),
        instrument_id: instrument.to_owned(),
        asset_class: "equity".to_owned(),
        sector: "technology".to_owned(),
        currency: "USD".to_owned(),
        quantity: amount(quantity),
        mark_price: amount(price),
        multiplier: amount("1"),
        delta: amount(quantity),
        gamma: Decimal::ZERO,
    }
}

fn fx_spot_snapshot() -> FxPricingSnapshot {
    FxPricingSnapshot {
        snapshot_id: "fx.snapshot.001".to_owned(),
        reference_version: "fx.price.v1".to_owned(),
        instrument_id: "instrument.fx.eur-usd".to_owned(),
        product: FxProduct::Spot,
        pair: FxPair::new("EUR", "USD").unwrap(),
        terms: FxPriceTerms::Outright {
            value_date: FxValueDate::new("2026-01-06").unwrap(),
            quote: FxOutrightQuote {
                bid: amount("1.1000"),
                ask: amount("1.1002"),
            },
        },
        source_id: "source.fixture".to_owned(),
        source_sequence: 1,
        source_time: "2026-01-02T10:00:00Z".to_owned(),
        received_at: "2026-01-02T10:00:01Z".to_owned(),
    }
}

#[test]
fn aggregates_long_short_and_bucket_exposure_exactly() {
    let snapshot = PortfolioRiskSnapshot {
        equity: amount("50000"),
        peak_equity: amount("52000"),
        daily_pnl: amount("-1000"),
        margin_used: amount("10000"),
        positions: vec![
            position("instrument.a", "200", "100"),
            position("instrument.b", "-100", "50"),
        ],
        working_positions: vec![],
        resting_orders: vec![],
        recent_order_count: 0,
    };
    let decision = evaluate_portfolio_risk(&policy(), &snapshot, None).expect("decision");
    assert!(decision.approved);
    assert_eq!(decision.metrics.gross_exposure, amount("25000"));
    assert_eq!(decision.metrics.net_exposure, amount("15000"));
    assert_eq!(decision.metrics.leverage_bps, amount("5000"));
    assert_eq!(decision.metrics.drawdown_bps, amount("384.61538461"));
}

#[test]
fn concentration_combines_rows_of_one_instrument_including_working_exposure() {
    let snapshot = PortfolioRiskSnapshot {
        equity: amount("100000"),
        peak_equity: amount("100000"),
        daily_pnl: Decimal::ZERO,
        margin_used: Decimal::ZERO,
        positions: vec![
            position("instrument.a", "100", "100"),
            position("instrument.a", "100", "100"),
        ],
        working_positions: vec![],
        resting_orders: vec![],
        recent_order_count: 0,
    };
    let decision = evaluate_portfolio_risk(&policy(), &snapshot, None).unwrap();
    assert!(!decision.approved);
    assert_eq!(decision.metrics.gross_exposure, amount("20000"));
    assert_eq!(decision.metrics.concentration_bps, amount("10000"));
    assert!(decision
        .reason_codes
        .contains(&"MAX_CONCENTRATION_EXCEEDED".to_owned()));
}

#[test]
fn opposing_working_fills_cannot_hide_a_net_limit_breach() {
    let mut limits = policy();
    limits.max_abs_net_exposure = amount("5000");
    limits.max_concentration_bps = amount("10000");
    let snapshot = PortfolioRiskSnapshot {
        equity: amount("100000"),
        peak_equity: amount("100000"),
        daily_pnl: Decimal::ZERO,
        margin_used: Decimal::ZERO,
        positions: vec![],
        working_positions: vec![
            position("instrument.a", "100", "100"),
            position("instrument.b", "-100", "100"),
        ],
        resting_orders: vec![],
        recent_order_count: 0,
    };
    let decision = evaluate_portfolio_risk(&limits, &snapshot, None).unwrap();
    assert!(!decision.approved);
    assert_eq!(decision.metrics.net_exposure, Decimal::ZERO);
    assert_eq!(decision.metrics.possible_abs_net_exposure, amount("10000"));
    assert!(decision
        .reason_codes
        .contains(&"MAX_NET_EXPOSURE_EXCEEDED".to_owned()));
}

#[test]
fn an_unfilled_diversifier_cannot_hide_concentration() {
    let mut limits = policy();
    limits.max_concentration_bps = amount("6000");
    let snapshot = PortfolioRiskSnapshot {
        equity: amount("100000"),
        peak_equity: amount("100000"),
        daily_pnl: Decimal::ZERO,
        margin_used: Decimal::ZERO,
        positions: vec![
            position("instrument.a", "50", "100"),
            position("instrument.b", "50", "100"),
        ],
        working_positions: vec![
            position("instrument.a", "100", "100"),
            position("instrument.b", "100", "100"),
        ],
        resting_orders: vec![],
        recent_order_count: 0,
    };
    let decision = evaluate_portfolio_risk(&limits, &snapshot, None).unwrap();
    assert!(!decision.approved);
    assert_eq!(decision.metrics.concentration_bps, amount("5000"));
    assert_eq!(decision.metrics.possible_concentration_bps, amount("7500"));
    assert!(decision
        .reason_codes
        .contains(&"MAX_CONCENTRATION_EXCEEDED".to_owned()));
}

#[test]
fn candidate_is_checked_for_permissions_self_trade_rate_and_aggregate_limits() {
    let mut policy = policy();
    policy
        .restricted_instruments
        .insert("instrument.blocked".to_owned());
    let snapshot = PortfolioRiskSnapshot {
        equity: amount("10000"),
        peak_equity: amount("12000"),
        daily_pnl: amount("-6000"),
        margin_used: amount("6000"),
        positions: vec![position("instrument.existing", "300", "100")],
        working_positions: vec![],
        resting_orders: vec![RestingOrder {
            order_id: "order.resting".to_owned(),
            account_id: "account.main".to_owned(),
            instrument_id: "instrument.blocked".to_owned(),
            side: Side::Sell,
        }],
        recent_order_count: 20,
    };
    let candidate = CandidateOrder {
        intent_id: "intent.candidate".to_owned(),
        account_id: "account.main".to_owned(),
        strategy_id: "strategy.alpha".to_owned(),
        instrument_id: "instrument.blocked".to_owned(),
        asset_class: "equity".to_owned(),
        sector: "technology".to_owned(),
        currency: "USD".to_owned(),
        side: Side::Buy,
        quantity: amount("200"),
        mark_price: amount("100"),
        multiplier: amount("1"),
        delta: amount("200"),
        gamma: Decimal::ZERO,
    };
    let decision = evaluate_portfolio_risk(&policy, &snapshot, Some(&candidate)).expect("decision");
    assert!(!decision.approved);
    for code in [
        "RESTRICTED_INSTRUMENT",
        "SELF_TRADE_RISK",
        "MAX_ORDER_RATE_EXCEEDED",
        "MAX_DAILY_LOSS_EXCEEDED",
        "MAX_MARGIN_UTILIZATION_EXCEEDED",
        "SECTOR_LIMIT_EXCEEDED:technology",
    ] {
        assert!(
            decision.reason_codes.contains(&code.to_owned()),
            "missing {code}"
        );
    }
}

/// Two legs that each pass on their own but breach jointly.
///
/// This is the whole reason an atomic group has to be evaluated together.
/// Evaluating a combination one leg at a time would approve exactly the
/// exposure the sector limit exists to refuse, because neither leg reaches
/// it alone and the group executes atomically -- there is no moment at
/// which only one of them is filled.
#[test]
fn simultaneous_candidate_legs_breach_a_bucket_limit_neither_leg_reaches_alone() {
    let mut policy = policy();
    // Concentration is largest-position/gross, so a single-position book is
    // always 100% concentrated. Relaxing it here isolates the sector limit
    // as the only binding constraint, which is what this test is about.
    policy.max_concentration_bps = amount("10000");
    let snapshot = PortfolioRiskSnapshot {
        equity: amount("50000"),
        peak_equity: amount("50000"),
        daily_pnl: Decimal::ZERO,
        margin_used: Decimal::ZERO,
        positions: vec![],
        working_positions: vec![],
        resting_orders: vec![],
        recent_order_count: 0,
    };
    let leg = |instrument: &str, side: Side| CandidateOrder {
        intent_id: "intent.combo".to_owned(),
        account_id: "account.main".to_owned(),
        strategy_id: "strategy.alpha".to_owned(),
        instrument_id: instrument.to_owned(),
        asset_class: "equity".to_owned(),
        sector: "technology".to_owned(),
        currency: "USD".to_owned(),
        side,
        quantity: amount("250"),
        mark_price: amount("100"),
        multiplier: amount("1"),
        delta: Decimal::ZERO,
        gamma: Decimal::ZERO,
    };
    let legs = vec![
        leg("instrument.near", Side::Buy),
        leg("instrument.far", Side::Sell),
    ];

    // 25,000 of technology gross each -- under the 40,000 sector limit.
    for single in &legs {
        let decision =
            evaluate_portfolio_risk(&policy, &snapshot, Some(single)).expect("single-leg decision");
        assert!(
            decision.approved,
            "leg {} should pass alone: {:?}",
            single.instrument_id, decision.reason_codes
        );
    }

    // 50,000 together, which the same limit refuses. Note that a *net*
    // view would see zero here: the legs are opposite sides. Gross is what
    // the limit is written against, and gross is what an atomic group
    // actually puts on.
    let decision =
        evaluate_portfolio_risk_with_candidates(&policy, &snapshot, &legs).expect("group decision");
    assert!(!decision.approved);
    assert!(decision
        .reason_codes
        .contains(&"SECTOR_LIMIT_EXCEEDED:technology".to_owned()));
    assert_eq!(decision.metrics.gross_exposure, amount("50000"));
    assert_eq!(decision.metrics.net_exposure, Decimal::ZERO);
}

/// An atomic group is one order against the open-order and rate limits.
///
/// A four-leg structure is one broker submission and one OMS order, so
/// counting its legs against a rate limit would make an ordinary condor
/// look like a burst of orders.
#[test]
fn an_atomic_group_counts_as_one_order_not_one_per_leg() {
    let mut policy = policy();
    policy.max_open_orders = 1;
    policy.max_order_rate = 1;
    let snapshot = PortfolioRiskSnapshot {
        equity: amount("1000000"),
        peak_equity: amount("1000000"),
        daily_pnl: Decimal::ZERO,
        margin_used: Decimal::ZERO,
        positions: vec![],
        working_positions: vec![],
        resting_orders: vec![],
        recent_order_count: 0,
    };
    let legs: Vec<CandidateOrder> = (0..4)
        .map(|index| CandidateOrder {
            intent_id: "intent.condor".to_owned(),
            account_id: "account.main".to_owned(),
            strategy_id: "strategy.alpha".to_owned(),
            instrument_id: format!("instrument.leg{index}"),
            asset_class: "equity".to_owned(),
            sector: "technology".to_owned(),
            currency: "USD".to_owned(),
            side: if index % 2 == 0 {
                Side::Buy
            } else {
                Side::Sell
            },
            quantity: amount("10"),
            mark_price: amount("100"),
            multiplier: amount("1"),
            delta: Decimal::ZERO,
            gamma: Decimal::ZERO,
        })
        .collect();
    let decision =
        evaluate_portfolio_risk_with_candidates(&policy, &snapshot, &legs).expect("group decision");
    assert!(
        decision.approved,
        "four legs are one order: {:?}",
        decision.reason_codes
    );
}

#[test]
fn test_news_shock_collar_protection() {
    let mut policy = policy();
    policy.max_news_slippage_bps = Some(amount("50")); // 50 BPS max price drift
    policy.max_spread_multiplier_bps = Some(amount("30000")); // 3.0x max spread expansion

    // 1. Normal price (100 -> 100.40 = 40 BPS deviation): Passed
    let clean_reasons = evaluate_news_shock_collar(
        &policy,
        amount("100"),
        amount("100.40"),
        Some(amount("0.05")),
        Some(amount("0.02")),
    )
    .expect("reasons");
    assert!(clean_reasons.is_empty());

    // 2. High slippage (100 -> 101.00 = 100 BPS deviation > 50 BPS max): Failed
    // 3. Liquidity hole (current spread 0.10 > 3.0 * baseline 0.02 = 0.06): Failed
    let shock_reasons = evaluate_news_shock_collar(
        &policy,
        amount("100"),
        amount("101.00"),
        Some(amount("0.10")),
        Some(amount("0.02")),
    )
    .expect("reasons");
    assert!(shock_reasons.contains(&"NEWS_SLIPPAGE_EXCEEDED".to_owned()));
    assert!(shock_reasons.contains(&"LIQUIDITY_HOLE_DETECTED".to_owned()));
}

#[test]
fn fx_candidate_uses_frozen_price_evidence_and_normal_risk_policy() {
    let snapshot = fx_spot_snapshot();
    let candidate = FxRiskCandidate::from_pricing_snapshot(
        FxRiskOrderIdentity {
            intent_id: "intent.fx.001".to_owned(),
            account_id: "account.paper.001".to_owned(),
            strategy_id: "strategy.fx.alpha".to_owned(),
            instrument_id: "instrument.fx.eur-usd".to_owned(),
            sector: "fx".to_owned(),
        },
        &snapshot,
        Side::Buy,
        amount("10"),
        amount("1"),
        FxRiskPricingContext {
            value_date: FxValueDate::new("2026-01-06").unwrap(),
            as_of: "2026-01-02T10:00:03Z".to_owned(),
            maximum_quote_age_seconds: 5,
        },
    )
    .unwrap();
    assert_eq!(candidate.candidate.asset_class, "fx_spot");
    assert_eq!(candidate.candidate.currency, "USD");
    assert_eq!(candidate.candidate.mark_price, amount("1.1001"));
    assert_eq!(candidate.candidate.delta, amount("10"));

    let mut fx_limited_policy = policy();
    fx_limited_policy.asset_class_limits = BTreeMap::from([("fx_spot".to_owned(), amount("10"))]);
    let decision = evaluate_portfolio_risk(
        &fx_limited_policy,
        &PortfolioRiskSnapshot {
            equity: amount("50000"),
            peak_equity: amount("50000"),
            daily_pnl: Decimal::ZERO,
            margin_used: Decimal::ZERO,
            positions: vec![],
            working_positions: vec![],
            resting_orders: vec![],
            recent_order_count: 0,
        },
        Some(&candidate.candidate),
    )
    .unwrap();
    assert!(!decision.approved);
    assert!(decision
        .reason_codes
        .contains(&"ASSET_CLASS_LIMIT_EXCEEDED:fx_spot".to_owned()));

    assert!(FxRiskCandidate::from_pricing_snapshot(
        FxRiskOrderIdentity {
            instrument_id: "instrument.fx.other".to_owned(),
            ..FxRiskOrderIdentity {
                intent_id: "intent.fx.002".to_owned(),
                account_id: "account.paper.001".to_owned(),
                strategy_id: "strategy.fx.alpha".to_owned(),
                instrument_id: "instrument.fx.eur-usd".to_owned(),
                sector: "fx".to_owned(),
            }
        },
        &snapshot,
        Side::Buy,
        amount("1"),
        amount("1"),
        FxRiskPricingContext {
            value_date: FxValueDate::new("2026-01-06").unwrap(),
            as_of: "2026-01-02T10:00:03Z".to_owned(),
            maximum_quote_age_seconds: 5,
        },
    )
    .is_err());
}
