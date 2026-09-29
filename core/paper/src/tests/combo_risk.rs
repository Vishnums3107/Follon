//! Combination pre-trade risk evaluation.

use super::*;

#[test]
fn combo_risk_approves_a_priced_vertical_and_records_exact_evidence() {
    let mut service = service_permitting_shorts();
    let decision = service
        .evaluate_combo_risk(
            &combo_intent("combo-000001", "2026-01-02T14:30:00Z"),
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(decision.approved, "{:?}", decision.reason_codes);
    assert_eq!(decision.reason_codes, vec!["APPROVED".to_owned()]);
    // A combination decision must never collide with a plain order's.
    assert_eq!(decision.decision_id, "paper-combo-risk-combo-000001");
    assert!(decision.evaluated_limits.contains("combo_legs=2"));
    assert!(decision
        .evaluated_limits
        .contains("combo_price_limit_kind=MAXIMUM_DEBIT"));
    // 4 units * (1 * 7.50 + 1 * 5.00) = 50 gross; net debit 4 * 2.50 = 10.
    assert!(decision
        .evaluated_limits
        .contains("combo_gross_notional=50.00000000"));
    assert!(decision
        .evaluated_limits
        .contains("combo_net_debit=10.00000000"));
    assert!(decision
        .evaluated_limits
        .contains("combo_protected_net_price=2.50000000"));
}

#[test]
fn combo_legs_meet_the_plain_order_tick_rule_and_the_net_the_finest_grid() {
    let decide = |ticks: &[(&str, &str)], near: &str, far: &str, cap: &str| {
        let mut service = service_permitting_shorts();
        for leg in ["inst.us_option.spy.near", "inst.us_option.spy.far"] {
            service.risk_policy.instrument_tick_sizes.remove(leg);
            // A leg with no tick is listed in neither table, as a
            // validated policy requires (E3.6f).
            if !ticks.iter().any(|(instrument, _)| *instrument == leg) {
                service.risk_policy.instrument_lot_sizes.remove(leg);
            }
        }
        for (instrument, tick) in ticks {
            service
                .risk_policy
                .instrument_tick_sizes
                .insert((*instrument).to_owned(), decimal("tick", tick).unwrap());
        }
        let mut intent = combo_intent("combo-000090", "2026-01-02T14:30:00Z");
        intent.legs[0].limit_price = decimal("near", near).unwrap();
        intent.legs[1].limit_price = decimal("far", far).unwrap();
        intent.price_limit =
            follon_domain::ComboPriceLimit::MaximumDebit(decimal("cap", cap).unwrap());
        let mut market = combo_market("2026-01-02T14:30:00Z");
        market.marks[0].mark_price = decimal("mark", near).unwrap();
        market.marks[1].mark_price = decimal("mark", far).unwrap();
        service
            .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z")
            .unwrap()
    };
    let near = "inst.us_option.spy.near";
    let far = "inst.us_option.spy.far";

    // On every grid: approved, with each leg's tick in the evidence.
    let on_grid = decide(&[(near, "0.05"), (far, "0.01")], "7.50", "5", "2.51");
    assert!(on_grid.approved, "{:?}", on_grid.reason_codes);
    assert!(on_grid.evaluated_limits.contains(
        "combo_tick_sizes=[inst.us_option.spy.near:0.05000000|inst.us_option.spy.far:0.01000000]"
    ));
    // An unlisted leg is refused exactly as a plain order on it would be.
    let unlisted = decide(&[(near, "0.01")], "7.50", "5", "2.50");
    assert!(!unlisted.approved);
    assert!(unlisted
        .reason_codes
        .contains(&"INSTRUMENT_TICK_SIZE_UNCONFIGURED".to_owned()));
    assert!(unlisted
        .evaluated_limits
        .contains("inst.us_option.spy.far:UNCONFIGURED"));
    // A leg price off its own grid.
    let off_leg = decide(&[(near, "0.05"), (far, "0.01")], "7.52", "5.02", "2.50");
    assert_eq!(
        off_leg.reason_codes,
        vec!["LIMIT_PRICE_OFF_TICK_GRID".to_owned()]
    );
    // A net limit off the finest leg grid, with every leg on its own.
    let off_net = decide(&[(near, "0.01"), (far, "0.01")], "7.50", "5", "2.505");
    assert_eq!(
        off_net.reason_codes,
        vec!["COMBO_NET_PRICE_OFF_TICK_GRID".to_owned()]
    );
    // The finest grid binds: a cent-stepped net against two nickel legs is refused.
    let coarse = decide(&[(near, "0.05"), (far, "0.05")], "7.50", "5", "2.51");
    assert_eq!(
        coarse.reason_codes,
        vec!["COMBO_NET_PRICE_OFF_TICK_GRID".to_owned()]
    );
}

#[test]
fn combo_leg_quantities_meet_the_plain_order_lot_rule() {
    let near = "inst.us_option.spy.near";
    let far = "inst.us_option.spy.far";
    // The near leg carries a ratio of 2 at 6.00 and the far leg a ratio of
    // 1 at 5.00, a 7.00 net debit: each unit sends two near contracts and
    // one far contract to the broker.
    let decide = |lots: &[(&str, &str)], units: &str| {
        let mut service = service_permitting_shorts();
        for leg in [near, far] {
            service.risk_policy.instrument_lot_sizes.remove(leg);
            // A leg with no lot is listed in neither table, as a
            // validated policy requires (E3.6f).
            if !lots.iter().any(|(instrument, _)| *instrument == leg) {
                service.risk_policy.instrument_tick_sizes.remove(leg);
            }
        }
        for (instrument, lot) in lots {
            service
                .risk_policy
                .instrument_lot_sizes
                .insert((*instrument).to_owned(), decimal("lot", lot).unwrap());
        }
        let mut intent = combo_intent("combo-000091", "2026-01-02T14:30:00Z");
        intent.combo_quantity = decimal("units", units).unwrap();
        intent.legs[0].ratio = 2;
        intent.legs[0].limit_price = decimal("near", "6").unwrap();
        intent.price_limit =
            follon_domain::ComboPriceLimit::MaximumDebit(decimal("cap", "7").unwrap());
        let mut market = combo_market("2026-01-02T14:30:00Z");
        market.marks[0].mark_price = decimal("mark", "6").unwrap();
        service
            .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z")
            .unwrap()
    };

    // Two units send four near and two far contracts: whole lots of 4 and
    // 2. Two units are not a whole near lot, so only each leg's own
    // quantity can approve this.
    let on_lot = decide(&[(near, "4"), (far, "2")], "2");
    assert!(on_lot.approved, "{:?}", on_lot.reason_codes);
    assert!(on_lot.evaluated_limits.contains(
        "combo_lot_sizes=[inst.us_option.spy.near:4.00000000|inst.us_option.spy.far:2.00000000]"
    ));
    // One unit sends a single far contract against a lot of 2.
    let off_lot = decide(&[(near, "2"), (far, "2")], "1");
    assert_eq!(
        off_lot.reason_codes,
        vec!["ORDER_QUANTITY_OFF_LOT_SIZE".to_owned()]
    );
    // A leg on an unlisted instrument is refused exactly as a plain order
    // on it would be, on both counts.
    let unlisted = decide(&[(near, "1")], "2");
    // A combination's codes are sorted.
    assert_eq!(
        unlisted.reason_codes,
        vec![
            "INSTRUMENT_LOT_SIZE_UNCONFIGURED".to_owned(),
            "INSTRUMENT_TICK_SIZE_UNCONFIGURED".to_owned()
        ]
    );
    assert!(unlisted.evaluated_limits.contains(
        "combo_lot_sizes=[inst.us_option.spy.near:1.00000000|inst.us_option.spy.far:UNCONFIGURED]"
    ));
}

#[test]
fn combo_risk_charges_the_order_notional_limit_the_gross_not_the_net() {
    let mut service = service_permitting_shorts();
    // 400 units: gross 400 * 12.50 = 5,000... raise it until gross crosses
    // the 50,000 policy limit while the *net* stays far below it. Net here
    // is 4,001 * 2.50 = 10,002.50, which a net-based check would approve.
    let mut intent = combo_intent("combo-000002", "2026-01-02T14:30:00Z");
    intent.combo_quantity = decimal("units", "4001").unwrap();
    let decision = service
        .evaluate_combo_risk(
            &intent,
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(!decision.approved);
    assert!(decision
        .reason_codes
        .contains(&"MAX_ORDER_NOTIONAL_EXCEEDED".to_owned()));
    assert!(decision
        .evaluated_limits
        .contains("combo_gross_notional=50012.50000000"));
    assert!(decision
        .evaluated_limits
        .contains("combo_net_debit=10002.50000000"));
}

#[test]
fn combo_risk_collars_each_leg_against_its_own_mark() {
    let mut service = service_permitting_shorts();
    let intent = combo_intent("combo-000003", "2026-01-02T14:30:00Z");
    let mut market = combo_market("2026-01-02T14:30:00Z");
    // Move only the second leg's mark. The first leg is still exactly on
    // its own mark, so a single blended or first-leg-only collar would
    // miss this entirely.
    market.marks[1].mark_price = decimal("mark", "4").unwrap();
    let decision = service
        .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z")
        .unwrap();
    assert!(!decision.approved);
    assert!(decision
        .reason_codes
        .contains(&"PRICE_COLLAR_EXCEEDED".to_owned()));
    // 5.00 requested against a 4.00 mark is 2,500 bps, far past the 100 bps
    // policy limit, and it is the *widest* leg that is recorded.
    assert!(decision
        .evaluated_limits
        .contains("widest_leg_deviation_bps=2500.00000000"));
}

/// A combination's short leg is refused unless an operator permitted it.
///
/// This is the default. `core/paper` holds no option reference data, so it
/// cannot prove that the short far-strike leg is covered by the long near
/// one, and it does not assume it. Almost every real spread has a short
/// leg, so the practical effect is that spreads require an explicit,
/// bounded operator permission — which is the intended behaviour, not an
/// oversight.
#[test]
fn combo_risk_refuses_a_short_leg_until_an_operator_permits_it() {
    let mut default_service = service();
    let decision = default_service
        .evaluate_combo_risk(
            &combo_intent("combo-000004", "2026-01-02T14:30:00Z"),
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(!decision.approved);
    assert!(decision
        .reason_codes
        .contains(&"POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned()));

    // The same structure, with the permission present, is approved.
    let mut permitting = service_permitting_shorts();
    let decision = permitting
        .evaluate_combo_risk(
            &combo_intent("combo-000004", "2026-01-02T14:30:00Z"),
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(decision.approved, "{:?}", decision.reason_codes);
}

#[test]
fn combo_risk_projects_each_leg_against_its_own_position_bound() {
    let mut service = service_permitting_shorts();
    // 2,000 units: the long leg projects +2,000 against a 1,000 long limit
    // and the short leg projects -2,000 against a 1,000 short bound. Both
    // legs breach, and the per-instrument projection is what sees it.
    let mut oversized = combo_intent("combo-000005", "2026-01-02T14:30:00Z");
    oversized.combo_quantity = decimal("units", "2000").unwrap();
    let decision = service
        .evaluate_combo_risk(
            &oversized,
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(decision
        .reason_codes
        .contains(&"POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned()));

    // Exactly at both bounds is permitted; the guard is a ceiling, not an
    // off-by-one refusal.
    let mut exact = combo_intent("combo-000012", "2026-01-02T14:30:00Z");
    exact.combo_quantity = decimal("units", "1000").unwrap();
    let decision = service
        .evaluate_combo_risk(
            &exact,
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(!decision
        .reason_codes
        .contains(&"POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned()));
}

#[test]
fn short_permission_must_state_a_positive_bound() {
    let invalid = PaperRiskPolicy {
        short_exposure: Some(ShortExposurePolicy {
            max_short_quantity: Decimal::ZERO,
        }),
        ..policy()
    };
    // `None` already means "no shorting", so a zero bound can only be a
    // configuration mistake and fails closed rather than silently agreeing.
    assert!(invalid.validate().is_err());
}

#[test]
fn combo_risk_binds_the_quantity_limit_to_the_largest_leg_not_the_unit_count() {
    let mut service = service_permitting_shorts();
    // 60 combination units, but the near leg carries a ratio of 2, so the
    // broker sees a 120-contract order against a 100 limit. Counting units
    // alone would approve it.
    let mut intent = combo_intent("combo-000006", "2026-01-02T14:30:00Z");
    intent.combo_quantity = decimal("units", "60").unwrap();
    intent.legs[0].ratio = 2;
    intent.legs[0].limit_price = decimal("near", "6").unwrap();
    intent.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(decimal("cap", "7").unwrap());
    let mut market = combo_market("2026-01-02T14:30:00Z");
    market.marks[0].mark_price = decimal("mark", "6").unwrap();
    let decision = service
        .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z")
        .unwrap();
    assert!(decision
        .reason_codes
        .contains(&"MAX_ORDER_QUANTITY_EXCEEDED".to_owned()));
    assert!(decision
        .evaluated_limits
        .contains("largest_leg_quantity=120.00000000"));
}

#[test]
fn combo_risk_charges_only_the_net_debit_against_available_cash() {
    let mut service = service_permitting_shorts();
    // A credit structure takes no cash out, so it must not be refused for
    // insufficient cash however large its gross notional is.
    let mut credit = combo_intent("combo-000007", "2026-01-02T14:30:00Z");
    credit.legs[0].side = Side::Sell;
    credit.legs[1].side = Side::Buy;
    credit.price_limit =
        follon_domain::ComboPriceLimit::MinimumCredit(decimal("floor", "2").unwrap());
    let decision = service
        .evaluate_combo_risk(
            &credit,
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(!decision
        .reason_codes
        .contains(&"INSUFFICIENT_INTERNAL_CASH".to_owned()));
    assert!(decision
        .evaluated_limits
        .contains("combo_net_debit=0.00000000"));

    // A debit structure larger than the 100,000 account does not.
    let mut expensive = combo_intent("combo-000008", "2026-01-02T14:30:00Z");
    expensive.combo_quantity = decimal("units", "50000").unwrap();
    let decision = service
        .evaluate_combo_risk(
            &expensive,
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(decision
        .reason_codes
        .contains(&"INSUFFICIENT_INTERNAL_CASH".to_owned()));
}

#[test]
fn combo_risk_is_halted_by_a_kill_switch_on_any_single_leg() {
    let mut service = service_permitting_shorts();
    service
        .activate_kill_switch(KillSwitchScope::Instrument(
            "inst.us_option.spy.far".to_owned(),
        ))
        .unwrap();
    let decision = service
        .evaluate_combo_risk(
            &combo_intent("combo-000009", "2026-01-02T14:30:00Z"),
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(!decision.approved);
    assert!(decision
        .reason_codes
        .contains(&"KILL_SWITCH_INSTRUMENT_INST.US_OPTION.SPY.FAR".to_owned()));
}

#[test]
fn combo_risk_refuses_an_incomplete_or_stale_observation() {
    let mut service = service_permitting_shorts();
    let intent = combo_intent("combo-000010", "2026-01-02T14:30:00Z");

    // One leg unquoted: the gate cannot price the structure and must not
    // guess.
    let mut partial = combo_market("2026-01-02T14:30:00Z");
    partial.marks.truncate(1);
    assert!(service
        .evaluate_combo_risk(&intent, &partial, "2026-01-02T14:30:02Z")
        .is_err());

    // One leg quoted twice, the other not at all: the count matches but
    // the coverage does not.
    let mut duplicated = combo_market("2026-01-02T14:30:00Z");
    duplicated.marks[1] = duplicated.marks[0].clone();
    assert!(service
        .evaluate_combo_risk(&intent, &duplicated, "2026-01-02T14:30:02Z")
        .is_err());

    // Freshness is the *stalest* leg's. One fresh quote beside an old one
    // must not launder it.
    let mut half_stale = combo_market("2026-01-02T14:30:00Z");
    half_stale.marks[0].observed_at = "2026-01-02T14:20:00Z".to_owned();
    assert!(service
        .evaluate_combo_risk(&intent, &half_stale, "2026-01-02T14:30:02Z")
        .is_err());

    // And an observation from after the decision is refused outright.
    assert!(service
        .evaluate_combo_risk(
            &intent,
            &combo_market("2026-01-02T14:31:00Z"),
            "2026-01-02T14:30:02Z"
        )
        .is_err());
}

#[test]
fn combo_risk_creates_no_order_and_contacts_no_broker() {
    let mut service = service_permitting_shorts();
    let decision = service
        .evaluate_combo_risk(
            &combo_intent("combo-000011", "2026-01-02T14:30:00Z"),
            &combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        )
        .unwrap();
    assert!(decision.approved, "{:?}", decision.reason_codes);
    // Assessment only, until E1.3 lands a submission path.
    assert!(service.order("order-combo-000011").is_none());
    assert_eq!(service.dashboard().working_orders, 0);
    assert_eq!(service.dashboard().positions.len(), 0);
}
