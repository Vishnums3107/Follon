//! Per-instrument tick and lot tables.

use super::*;

#[test]
fn an_order_off_its_instruments_tick_grid_is_refused_before_the_broker() {
    let mut service = service();
    let mut off_grid = intent("intent-tick-001", "2026-01-02T14:30:00Z");
    off_grid.order_type = OrderType::Limit;
    off_grid.limit_price = Some(decimal("limit", "100.005").unwrap());
    let outcome = service
        .submit_intent(
            off_grid,
            market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:01Z",
        )
        .unwrap();
    assert!(!outcome.decision.approved);
    assert_eq!(
        outcome.decision.reason_codes,
        vec!["LIMIT_PRICE_OFF_TICK_GRID".to_owned()]
    );
    assert!(
        outcome.order_id.is_none(),
        "an off-grid order reached the OMS"
    );
    assert!(outcome
        .decision
        .evaluated_limits
        .contains("instrument_tick_size=0.01"));

    let mut on_grid = intent("intent-tick-002", "2026-01-02T14:30:00Z");
    on_grid.order_type = OrderType::Limit;
    on_grid.limit_price = Some(decimal("limit", "100.01").unwrap());
    assert!(
        service
            .submit_intent(
                on_grid,
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z"
            )
            .unwrap()
            .decision
            .approved,
        "an on-grid limit was refused"
    );
}

#[test]
fn an_order_for_an_instrument_in_neither_table_is_refused_on_both_counts() {
    // A validated policy lists an instrument in both tables or in neither
    // (E3.6f), and iwm is in neither.
    let mut service = service();
    let mut unlisted = intent("intent-tick-003", "2026-01-02T14:30:00Z");
    unlisted.instrument_id = "inst.us_equity.iwm".to_owned();
    let mut iwm = market("2026-01-02T14:30:00Z");
    iwm.instrument_id = "inst.us_equity.iwm".to_owned();
    // A market order carries no limit, but its instrument is still unknown
    // reference data, so it fails closed.
    let outcome = service
        .submit_intent(unlisted, iwm, "2026-01-02T14:30:01Z")
        .unwrap();
    assert!(!outcome.decision.approved);
    assert_eq!(
        outcome.decision.reason_codes,
        vec![
            "INSTRUMENT_TICK_SIZE_UNCONFIGURED".to_owned(),
            "INSTRUMENT_LOT_SIZE_UNCONFIGURED".to_owned()
        ]
    );
    assert!(outcome.order_id.is_none());
    for evidence in [
        "instrument_tick_size=UNCONFIGURED",
        "instrument_lot_size=UNCONFIGURED",
    ] {
        assert!(outcome.decision.evaluated_limits.contains(evidence));
    }

    // A listed instrument's market order is unaffected.
    assert!(
        service
            .submit_intent(
                intent("intent-tick-004", "2026-01-02T14:30:00Z"),
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z"
            )
            .unwrap()
            .decision
            .approved
    );
}

#[test]
fn a_tick_size_table_must_be_nonempty_positive_and_canonical() {
    assert!(policy().validate().is_ok());
    for broken in [
        BTreeMap::new(),
        BTreeMap::from([("inst.us_equity.spy".to_owned(), Decimal::ZERO)]),
        BTreeMap::from([("INST.SPY".to_owned(), decimal("tick", "0.01").unwrap())]),
    ] {
        let policy = PaperRiskPolicy {
            instrument_tick_sizes: broken.clone(),
            ..policy()
        };
        assert!(policy.validate().is_err(), "accepted tick table {broken:?}");
    }
    let mut coarser = policy();
    coarser.instrument_tick_sizes.insert(
        "inst.us_equity.spy".to_owned(),
        decimal("tick", "0.05").unwrap(),
    );
    let coarser_service = PaperTradingService::new(
        account(),
        coarser,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        IbkrPaperAdapter::new(&account()).unwrap(),
    )
    .unwrap();
    assert_ne!(
        service().configuration_fingerprint(),
        coarser_service.configuration_fingerprint(),
        "a journal must not reopen under a changed tick table"
    );
}

#[test]
fn an_order_off_its_instruments_lot_size_is_refused_before_the_broker() {
    let mut policy = policy();
    policy.instrument_lot_sizes.insert(
        "inst.us_equity.spy".to_owned(),
        decimal("lot", "5").unwrap(),
    );
    let mut service = service_with(policy);
    // Three shares is a whole number but not a whole number of five-share
    // lots, and every other limit passes.
    let mut off_lot = intent("intent-lot-001", "2026-01-02T14:30:00Z");
    off_lot.quantity = decimal("quantity", "3").unwrap();
    let outcome = service
        .submit_intent(
            off_lot,
            market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:01Z",
        )
        .unwrap();
    assert!(!outcome.decision.approved);
    assert_eq!(
        outcome.decision.reason_codes,
        vec!["ORDER_QUANTITY_OFF_LOT_SIZE".to_owned()]
    );
    assert!(
        outcome.order_id.is_none(),
        "an off-lot order reached the OMS"
    );
    assert!(outcome
        .decision
        .evaluated_limits
        .contains("instrument_lot_size=5.00000000"));

    let mut whole_lots = intent("intent-lot-002", "2026-01-02T14:30:00Z");
    whole_lots.quantity = decimal("quantity", "10").unwrap();
    assert!(
        service
            .submit_intent(
                whole_lots,
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z"
            )
            .unwrap()
            .decision
            .approved,
        "a whole number of lots was refused"
    );
}

#[test]
fn the_tick_and_lot_tables_must_list_the_same_instruments() {
    assert!(policy().validate().is_ok());
    let iwm = "inst.us_equity.iwm";
    let mut lot_only = policy();
    lot_only
        .instrument_lot_sizes
        .insert(iwm.to_owned(), decimal("lot", "1").unwrap());
    let mut tick_only = policy();
    tick_only
        .instrument_tick_sizes
        .insert(iwm.to_owned(), decimal("tick", "0.01").unwrap());
    for unpaired in [lot_only, tick_only] {
        assert_eq!(
            unpaired.validate().unwrap_err().0,
            "paper risk policy lists inst.us_equity.iwm in only one of its tick and lot tables"
        );
        // The service refuses to start, before it can decide anything.
        assert!(PaperTradingService::new(
            account(),
            unpaired,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account()).unwrap(),
        )
        .is_err());
    }
}

#[test]
fn a_lot_size_table_must_be_nonempty_positive_and_canonical() {
    for broken in [
        BTreeMap::new(),
        BTreeMap::from([("inst.us_equity.spy".to_owned(), Decimal::ZERO)]),
        BTreeMap::from([("INST.SPY".to_owned(), decimal("lot", "1").unwrap())]),
    ] {
        let policy = PaperRiskPolicy {
            instrument_lot_sizes: broken.clone(),
            ..policy()
        };
        assert!(policy.validate().is_err(), "accepted lot table {broken:?}");
    }
    let mut round_lots = policy();
    round_lots.instrument_lot_sizes.insert(
        "inst.us_equity.spy".to_owned(),
        decimal("lot", "100").unwrap(),
    );
    assert_ne!(
        service().configuration_fingerprint(),
        service_with(round_lots).configuration_fingerprint(),
        "a journal must not reopen under a changed lot table"
    );
}
