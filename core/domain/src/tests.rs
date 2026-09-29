//! Unit tests for the domain contracts.

use super::*;
use std::str::FromStr;

fn decimal(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

/// A long call vertical: buy the 500 strike, sell the 510, for a net debit.
fn vertical_spread() -> ComboIntent {
    ComboIntent {
        intent_id: "combo-000001".to_owned(),
        account_id: "acct-paper-001".to_owned(),
        strategy_id: "strat.vertical".to_owned(),
        correlation_id: "corr-combo-000001".to_owned(),
        legs: vec![
            ComboIntentLeg {
                instrument_id: "inst.us_option.spy.20260320.c500".to_owned(),
                side: Side::Buy,
                ratio: 1,
                limit_price: decimal("7.50"),
            },
            ComboIntentLeg {
                instrument_id: "inst.us_option.spy.20260320.c510".to_owned(),
                side: Side::Sell,
                ratio: 1,
                limit_price: decimal("5.00"),
            },
        ],
        combo_quantity: decimal("4"),
        price_limit: ComboPriceLimit::MaximumDebit(decimal("2.50")),
        time_in_force: TimeInForce::Day,
        rationale: "capped-risk directional expression".to_owned(),
        created_at: "2026-01-02T14:30:00Z".to_owned(),
        strategy_version: "bundle-1".to_owned(),
        configuration_version: "cfg-1".to_owned(),
        environment: "PAPER".to_owned(),
    }
}

#[test]
fn combo_intent_prices_a_debit_spread_exactly() {
    let intent = vertical_spread();
    intent.validate().unwrap();
    // 7.50 paid less 5.00 received, per one-lot combination unit.
    assert_eq!(intent.protected_net_price().unwrap(), decimal("2.50"));
    // Four units at ratio 1 on each leg.
    assert_eq!(intent.leg_quantity(&intent.legs[0]).unwrap(), decimal("4"));
    assert_eq!(
        intent.projected_leg_delta(&intent.legs[0]).unwrap(),
        decimal("4")
    );
    assert_eq!(
        intent.projected_leg_delta(&intent.legs[1]).unwrap(),
        decimal("-4")
    );
}

#[test]
fn combo_gross_notional_counts_both_legs_not_the_net() {
    let intent = vertical_spread();
    // 4 * 7.50 + 4 * 5.00 = 50, not the 10 a net-price view would report.
    // The short leg is a real obligation until the combination is closed,
    // so an aggregate limit must see it.
    assert_eq!(intent.gross_notional().unwrap(), decimal("50"));
    assert!(intent.gross_notional().unwrap() > intent.protected_net_price().unwrap());
}

#[test]
fn combo_intent_prices_a_credit_spread_exactly() {
    let mut intent = vertical_spread();
    intent.legs[0].side = Side::Sell;
    intent.legs[1].side = Side::Buy;
    intent.price_limit = ComboPriceLimit::MinimumCredit(decimal("2.00"));
    intent.validate().unwrap();
    assert_eq!(intent.protected_net_price().unwrap(), decimal("-2.50"));
}

#[test]
fn combo_price_limit_refuses_the_wrong_economics_not_just_the_wrong_magnitude() {
    // A combination that prices to a credit cannot satisfy a *debit*
    // protection, even though a credit is "cheaper" than any debit cap.
    // The operator approved a debit structure; filling a credit one is a
    // different trade.
    let mut credit = vertical_spread();
    credit.legs[0].side = Side::Sell;
    credit.legs[1].side = Side::Buy;
    assert_eq!(credit.protected_net_price().unwrap(), decimal("-2.50"));
    assert!(credit.validate().is_err());

    // And the mirror: a debit structure cannot satisfy a credit protection.
    let mut debit = vertical_spread();
    debit.price_limit = ComboPriceLimit::MinimumCredit(decimal("1.00"));
    assert!(debit.validate().is_err());
}

#[test]
fn combo_price_limit_refuses_a_breach_by_the_smallest_representable_amount() {
    let mut intent = vertical_spread();
    assert_eq!(intent.protected_net_price().unwrap(), decimal("2.50"));
    intent.price_limit =
        ComboPriceLimit::MaximumDebit(Decimal::from_scaled(decimal("2.50").scaled() - 1));
    assert!(intent.validate().is_err());
    // Exactly at the limit is accepted; one unit beyond is not.
    intent.price_limit = ComboPriceLimit::MaximumDebit(decimal("2.50"));
    intent.validate().unwrap();
}

#[test]
fn combo_intent_refuses_a_single_leg_and_an_oversized_group() {
    // One leg is a plain order, and the plain-order path carries strictly
    // more risk coverage, so it must not be reachable through this type.
    let mut single = vertical_spread();
    single.legs.truncate(1);
    assert!(single.validate().is_err());

    let mut oversized = vertical_spread();
    let template = oversized.legs[0].clone();
    while oversized.legs.len() <= *COMBO_LEG_BOUNDS.end() {
        let index = oversized.legs.len();
        oversized.legs.push(ComboIntentLeg {
            instrument_id: format!("inst.us_option.spy.20260320.c{index}"),
            ..template.clone()
        });
    }
    assert!(oversized.legs.len() > *COMBO_LEG_BOUNDS.end());
    assert!(oversized.validate().is_err());
}

#[test]
fn combo_intent_refuses_duplicate_instruments_rather_than_netting_them() {
    let mut intent = vertical_spread();
    intent.legs[1].instrument_id = intent.legs[0].instrument_id.clone();
    assert!(intent.validate().is_err());
}

#[test]
fn combo_intent_refuses_malformed_legs_quantities_and_identities() {
    let mut zero_ratio = vertical_spread();
    zero_ratio.legs[0].ratio = 0;
    assert!(zero_ratio.validate().is_err());

    let mut huge_ratio = vertical_spread();
    huge_ratio.legs[0].ratio = *COMBO_LEG_RATIO_BOUNDS.end() + 1;
    assert!(huge_ratio.validate().is_err());

    let mut free_leg = vertical_spread();
    free_leg.legs[0].limit_price = Decimal::ZERO;
    assert!(free_leg.validate().is_err());

    let mut no_units = vertical_spread();
    no_units.combo_quantity = Decimal::ZERO;
    assert!(no_units.validate().is_err());

    let mut negative_protection = vertical_spread();
    negative_protection.price_limit = ComboPriceLimit::MaximumDebit(Decimal::ZERO);
    assert!(negative_protection.validate().is_err());

    let mut display_symbol = vertical_spread();
    display_symbol.legs[0].instrument_id = "SPY 500C".to_owned();
    assert!(display_symbol.validate().is_err());

    let mut unexplained = vertical_spread();
    unexplained.rationale.clear();
    assert!(unexplained.validate().is_err());

    let mut local_time = vertical_spread();
    local_time.created_at = "2026-01-02T14:30:00+00:00".to_owned();
    assert!(local_time.validate().is_err());
}

#[test]
fn combo_price_limit_reports_a_stable_kind_and_amount() {
    assert_eq!(
        ComboPriceLimit::MaximumDebit(decimal("2.50")).kind(),
        "MAXIMUM_DEBIT"
    );
    assert_eq!(
        ComboPriceLimit::MinimumCredit(decimal("2.50")).kind(),
        "MINIMUM_CREDIT"
    );
    assert_eq!(
        ComboPriceLimit::MinimumCredit(decimal("2.50")).amount(),
        decimal("2.50")
    );
}

#[test]
fn decimal_is_exact_and_stably_rendered() {
    let value = Decimal::from_str("12.5").unwrap();
    assert_eq!(value.to_string(), "12.50000000");
    assert_eq!(
        value
            .checked_mul(Decimal::from_str("2.0").unwrap())
            .unwrap()
            .to_string(),
        "25.00000000"
    );
}

#[test]
fn price_deviation_is_exact_and_rejects_invalid_inputs() {
    let reference = Decimal::from_str("100").unwrap();
    assert_eq!(
        price_deviation_bps(reference, Decimal::from_str("101.25").unwrap()).unwrap(),
        Decimal::from_str("125").unwrap()
    );
    assert_eq!(
        price_deviation_bps(reference, Decimal::from_str("99.25").unwrap()).unwrap(),
        Decimal::from_str("75").unwrap()
    );
    assert!(price_deviation_bps(Decimal::ZERO, reference).is_err());
}

#[test]
fn identifiers_reject_display_symbols_and_whitespace() {
    assert!(validate_canonical_id("instrument_id", "inst.us_equity.spy").is_ok());
    assert!(validate_canonical_id("instrument_id", "SPY").is_err());
    assert!(validate_canonical_id("instrument_id", "inst spy").is_err());
}

#[test]
fn timestamps_and_bar_prices_are_canonical_at_ingress() {
    assert!(validate_utc_timestamp("time", "2026-01-02T14:30:00Z").is_ok());
    assert!(validate_utc_timestamp("time", "2026-01-02T14:30:00.1Z").is_err());
    assert!(validate_utc_timestamp("time", "2026-01-02T14:30:00+00:00").is_err());
    let invalid = Bar {
        instrument_id: "inst.us_equity.spy".to_owned(),
        open: Decimal::ZERO,
        high: Decimal::ZERO,
        low: Decimal::ZERO,
        close: Decimal::ZERO,
        volume: Decimal::ZERO,
        interval_seconds: 60,
        exchange_timezone: "America/New_York".to_owned(),
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn envelope_serialization_is_stable() {
    let envelope = EventEnvelope {
        event_id: "evt-000001".to_owned(),
        event_type: "market.bar.v1".to_owned(),
        schema_version: 1,
        event_time: "2026-01-02T14:30:00Z".to_owned(),
        receive_time: "2026-01-02T14:30:00Z".to_owned(),
        account_id: None,
        strategy_id: None,
        instrument_id: Some("inst.us_equity.spy".to_owned()),
        correlation_id: "corr-market-000001".to_owned(),
        causation_id: None,
        actor: "market_data".to_owned(),
        source: "historical_import".to_owned(),
        payload: EventPayload::MarketBar(Bar {
            instrument_id: "inst.us_equity.spy".to_owned(),
            open: "100".parse().unwrap(),
            high: "101".parse().unwrap(),
            low: "99".parse().unwrap(),
            close: "100.5".parse().unwrap(),
            volume: "10".parse().unwrap(),
            interval_seconds: 60,
            exchange_timezone: "America/New_York".to_owned(),
        }),
        software_version: "dev".to_owned(),
        configuration_version: "cfg-1".to_owned(),
    };
    envelope.validate().unwrap();
    assert_eq!(envelope.canonical_json(), envelope.canonical_json());
    assert!(envelope
        .canonical_json()
        .contains("\"event_type\":\"market.bar.v1\""));
}

/// Only a trade that moves a position strictly toward flat, without passing
/// through it, reduces risk (delivery state E7.4b).
#[test]
fn only_a_trade_toward_flat_reduces_a_position() {
    let cases = [
        // (current, projected, reduces)
        ("10", "4", true),
        ("10", "0", true),
        ("-10", "-4", true),
        ("-10", "0", true),
        ("10", "-2", false),
        ("-10", "2", false),
        ("10", "12", false),
        ("-10", "-12", false),
        ("10", "10", false),
        ("-10", "-10", false),
        ("0", "5", false),
        ("0", "-5", false),
        ("0", "0", false),
    ];
    for (current, projected, expected) in cases {
        assert_eq!(
            reduces_position(decimal(current), decimal(projected)),
            expected,
            "{current} -> {projected}"
        );
    }
}
