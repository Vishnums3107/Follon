//! Paper-only operational OMS, risk controls, broker fault injection, and reconciliation.
//!
//! This crate is intentionally incapable of live trading. It owns the state
//! between a validated paper intent and a normalized broker response, preserving
//! the safe `UNKNOWN` lifecycle state whenever submission or cancellation cannot
//! be proven. Broker snapshots are compared against independent internal state;
//! reconciliation never silently overwrites that state.

mod account;
mod broker;
mod combinations;
mod dashboard;
mod error;
mod fault;
mod fingerprint;
mod ibkr;
mod journal;
mod kill_switch;
mod market;
mod order;
mod policy;
pub mod qualification;
mod reconciliation;
mod records;
mod registry;
mod service;
mod validation;

use combinations::PersistentComboExecutionState;
pub use combinations::{BrokerComboExecution, BrokerComboExecutionLeg};

pub use qualification::{
    GatewayQualificationError, GatewayQualificationMatrix, QualificationState, QualifiedCapability,
};

pub use account::*;
pub use broker::*;
pub use dashboard::*;
pub use error::*;
pub use fault::*;
use fingerprint::*;
pub use ibkr::*;
pub use journal::*;
pub use kill_switch::*;
pub use market::*;
pub use order::*;
pub use policy::*;
pub use reconciliation::*;
use records::*;
pub use registry::*;
pub use service::*;
use validation::*;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs::{self};
    use std::path::{Path, PathBuf};
    use std::str::FromStr;

    use follon_control_plane::OmsComboOrder;
    use follon_domain::{
        ComboIntent, Decimal, OrderIntent, OrderState, OrderType, Side, TimeInForce,
    };
    use follon_instrument::TradingSession;

    use super::*;
    include!("combo_lifecycle_tests.rs");
    use follon_instrument::StaticTradingCalendar;

    fn account() -> PaperAccount {
        PaperAccount {
            account_id: "acct.paper.001".to_owned(),
            currency: "USD".to_owned(),
            initial_cash: decimal("initial cash", "100000").unwrap(),
            environment: "PAPER".to_owned(),
        }
    }

    fn policy() -> PaperRiskPolicy {
        PaperRiskPolicy {
            version: "paper-risk-v1".to_owned(),
            trading_calendar_id: "cal.us_equities.nyse.v1".to_owned(),
            max_order_quantity: decimal("quantity", "100").unwrap(),
            max_order_notional: decimal("notional", "50000").unwrap(),
            max_price_deviation_bps: decimal("price collar", "100").unwrap(),
            max_open_orders: 10,
            max_position_quantity: decimal("position", "1000").unwrap(),
            max_realized_loss: decimal("loss", "10000").unwrap(),
            max_market_data_age_seconds: 5,
            max_order_rate: 20,
            order_rate_window_seconds: 60,
            portfolio_risk: None,
            short_exposure: None,
            instrument_tick_sizes: test_tick_sizes(),
            instrument_lot_sizes: test_lot_sizes(),
        }
    }

    fn test_tick_sizes() -> BTreeMap<String, Decimal> {
        [
            "inst.us_equity.spy",
            "inst.us_equity.qqq",
            "inst.us_option.spy.near",
            "inst.us_option.spy.far",
        ]
        .into_iter()
        .map(|instrument| (instrument.to_owned(), decimal("tick", "0.01").unwrap()))
        .collect()
    }

    /// A lot of one for each listed instrument: every whole quantity passes,
    /// so only a test that sets a coarser lot exercises the lot rule.
    fn test_lot_sizes() -> BTreeMap<String, Decimal> {
        test_tick_sizes()
            .into_keys()
            .map(|instrument| (instrument, decimal("lot", "1").unwrap()))
            .collect()
    }

    /// The same policy with net short exposure explicitly permitted, bounded at
    /// 1,000 per instrument. Combination tests that need a short leg use this;
    /// none of them may quietly widen `policy()` instead.
    fn policy_permitting_shorts() -> PaperRiskPolicy {
        PaperRiskPolicy {
            short_exposure: Some(ShortExposurePolicy {
                max_short_quantity: decimal("short bound", "1000").unwrap(),
            }),
            ..policy()
        }
    }

    fn service_permitting_shorts() -> PaperTradingService<IbkrPaperAdapter> {
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        PaperTradingService::new(
            account,
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            adapter,
        )
        .unwrap()
    }

    fn service() -> PaperTradingService<IbkrPaperAdapter> {
        service_with(policy())
    }

    fn service_with(policy: PaperRiskPolicy) -> PaperTradingService<IbkrPaperAdapter> {
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        PaperTradingService::new(
            account,
            policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            adapter,
        )
        .unwrap()
    }

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

    fn modern_registry(account: &PaperAccount, venue_id: &str) -> PaperBrokerRegistry {
        let mut registry = PaperBrokerRegistry::new();
        registry
            .register(
                PaperBrokerRoute {
                    account_id: account.account_id.clone(),
                    adapter_id: format!("adapter.ibkr.paper.{}", account.account_id),
                    venue_id: venue_id.to_owned(),
                    environment: "PAPER".to_owned(),
                },
                Box::new(IbkrPaperAdapter::new(account).unwrap()),
            )
            .unwrap();
        registry
    }

    fn intent(intent_id: &str, created_at: &str) -> OrderIntent {
        OrderIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.paper.001".to_owned(),
            strategy_id: "strategy.paper.001".to_owned(),
            instrument_id: "inst.us_equity.spy".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            side: Side::Buy,
            quantity: decimal("quantity", "1").unwrap(),
            order_type: OrderType::Market,
            limit_price: None,
            time_in_force: TimeInForce::Day,
            rationale: "paper acceptance test".to_owned(),
            created_at: created_at.to_owned(),
            strategy_version: "strategy-paper-v1".to_owned(),
            configuration_version: "config-paper-v1".to_owned(),
            environment: "PAPER".to_owned(),
        }
    }

    fn market(observed_at: &str) -> PaperMarketData {
        PaperMarketData {
            instrument_id: "inst.us_equity.spy".to_owned(),
            mark_price: decimal("mark", "100").unwrap(),
            observed_at: observed_at.to_owned(),
        }
    }

    fn market_at_price(price: &str, observed_at: &str) -> PaperMarketData {
        PaperMarketData {
            instrument_id: "inst.us_equity.spy".to_owned(),
            mark_price: decimal("mark", price).unwrap(),
            observed_at: observed_at.to_owned(),
        }
    }

    /// A long call vertical on two distinct option instruments: buy the near
    /// strike at 7.50, sell the far strike at 5.00, for a 2.50 net debit per
    /// combination unit.
    fn combo_intent(intent_id: &str, created_at: &str) -> ComboIntent {
        ComboIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.paper.001".to_owned(),
            strategy_id: "strategy.paper.001".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            legs: vec![
                follon_domain::ComboIntentLeg {
                    instrument_id: "inst.us_option.spy.near".to_owned(),
                    side: Side::Buy,
                    ratio: 1,
                    limit_price: decimal("near", "7.50").unwrap(),
                },
                follon_domain::ComboIntentLeg {
                    instrument_id: "inst.us_option.spy.far".to_owned(),
                    side: Side::Sell,
                    ratio: 1,
                    limit_price: decimal("far", "5").unwrap(),
                },
            ],
            combo_quantity: decimal("units", "4").unwrap(),
            price_limit: follon_domain::ComboPriceLimit::MaximumDebit(
                decimal("cap", "2.50").unwrap(),
            ),
            time_in_force: TimeInForce::Day,
            rationale: "paper combo acceptance test".to_owned(),
            created_at: created_at.to_owned(),
            strategy_version: "strategy-paper-v1".to_owned(),
            configuration_version: "config-paper-v1".to_owned(),
            environment: "PAPER".to_owned(),
        }
    }

    /// Marks that sit exactly on each leg's own limit price, so the per-leg
    /// price collar reads zero deviation unless a test moves one.
    fn combo_market(observed_at: &str) -> PaperComboMarketData {
        PaperComboMarketData {
            marks: vec![
                PaperMarketData {
                    instrument_id: "inst.us_option.spy.near".to_owned(),
                    mark_price: decimal("mark", "7.50").unwrap(),
                    observed_at: observed_at.to_owned(),
                },
                PaperMarketData {
                    instrument_id: "inst.us_option.spy.far".to_owned(),
                    mark_price: decimal("mark", "5").unwrap(),
                    observed_at: observed_at.to_owned(),
                },
            ],
        }
    }

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
    fn a_refused_configuration_leaves_no_paper_journal() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "refused-configuration"
        ));
        let _ = fs::remove_file(&journal_path);
        let open = |policy: PaperRiskPolicy| {
            PaperTradingService::open_durable(
                account(),
                policy,
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                IbkrPaperAdapter::new(&account()).unwrap(),
                &journal_path,
            )
        };
        let mut unpaired = policy();
        unpaired.instrument_lot_sizes.insert(
            "inst.us_equity.iwm".to_owned(),
            decimal("lot", "1").unwrap(),
        );
        assert!(open(unpaired).is_err());
        assert!(
            !journal_path.exists(),
            "a refused configuration created a journal"
        );

        // The same path opens, and journals, once the configuration is valid.
        drop(open(policy()).unwrap());
        assert!(journal_path.exists());
        fs::remove_file(&journal_path).unwrap();
    }

    #[test]
    fn a_legacy_route_refusal_leaves_no_paper_journal() {
        let directory = std::env::temp_dir().join(format!(
            "follon-paper-legacy-refusal-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        let journal_path = directory.join("journal.ndjson");
        let open = || {
            let mut registry = PaperBrokerRegistry::new();
            registry
                .register_legacy_ibkr_paper_route(&account())
                .unwrap();
            PaperTradingService::open_durable(
                account(),
                policy(),
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                registry,
                &journal_path,
            )
        };
        let refusal = "legacy PAPER adapter routing may only reopen an existing journal";

        // Legacy routing may only reopen, so it creates neither the journal
        // nor its directory.
        assert_eq!(open().err().unwrap().0, refusal);
        assert!(
            !directory.exists(),
            "a legacy-route refusal created the journal's directory"
        );

        // Nor does it create the journal in a directory that exists.
        fs::create_dir_all(&directory).unwrap();
        assert_eq!(open().err().unwrap().0, refusal);
        assert!(
            !journal_path.exists(),
            "a legacy-route refusal created a journal"
        );

        // An empty journal that already existed is refused and left as it was.
        fs::write(&journal_path, b"").unwrap();
        assert_eq!(open().err().unwrap().0, refusal);
        assert_eq!(fs::read(&journal_path).unwrap(), b"");

        // A journal the single-adapter composition initialised still reopens.
        fs::remove_file(&journal_path).unwrap();
        drop(
            PaperTradingService::open_durable(
                account(),
                policy(),
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                IbkrPaperAdapter::new(&account()).unwrap(),
                &journal_path,
            )
            .unwrap(),
        );
        drop(open().unwrap());
        fs::remove_dir_all(&directory).unwrap();
    }

    /// Links `link` to `target`, which need not exist. Returns false where
    /// this account cannot create a symbolic link, such as Windows without
    /// Developer Mode, after saying so.
    fn symlink_to(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(target, link);
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(target, link);
        if let Err(error) = &linked {
            eprintln!("cannot create a symbolic link ({error}); the refusal was not exercised");
        }
        linked.is_ok()
    }

    #[test]
    fn a_paper_journal_refuses_a_symbolic_link_even_a_dangling_one() {
        let directory =
            std::env::temp_dir().join(format!("follon-paper-journal-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("elsewhere.ndjson");
        let link = directory.join("journal.ndjson");
        if !symlink_to(&target, &link) {
            fs::remove_dir_all(&directory).unwrap();
            return;
        }
        let refusal = "paper journal path must not be a symbolic link";

        // Nothing exists at the target, so following the link finds nothing.
        // Opening must not create the journal there.
        assert_eq!(FilePaperJournal::open(&link).err().unwrap().0, refusal);
        assert!(!target.exists(), "the journal was created through the link");
        assert_eq!(
            PaperTradingService::open_durable(
                account(),
                policy(),
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                IbkrPaperAdapter::new(&account()).unwrap(),
                &link,
            )
            .err()
            .unwrap()
            .0,
            refusal
        );
        assert!(!target.exists(), "the journal was created through the link");

        // A link to a real journal is refused too.
        drop(FilePaperJournal::open(&target).unwrap());
        assert_eq!(FilePaperJournal::open(&link).err().unwrap().0, refusal);
        fs::remove_dir_all(&directory).unwrap();
    }

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
        fn adapter_configuration_fingerprint(
            &self,
            account_id: &str,
        ) -> Result<String, PaperError> {
            self.inner.adapter_configuration_fingerprint(account_id)
        }

        fn submit(
            &mut self,
            request: &BrokerOrderRequest,
        ) -> Result<BrokerSubmitResult, PaperError> {
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
    fn combo_submission_creates_one_acknowledged_order_for_the_whole_group() {
        let mut service = service_permitting_shorts();
        let outcome = service
            .submit_combo_intent(
                combo_intent("combo-000020", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(
            outcome.decision.approved,
            "{:?}",
            outcome.decision.reason_codes
        );
        assert_eq!(
            outcome.order_id.as_deref(),
            Some("combo-order-combo-000020")
        );
        assert_eq!(outcome.state, Some(OrderState::Acknowledged));

        let order = service.combo_order("combo-order-combo-000020").unwrap();
        assert_eq!(order.oms.intent.legs.len(), 2);
        assert!(order.broker_order_id.is_some());
        // One broker order for the whole group, not one per leg.
        assert_eq!(order.broker_order_versions.len(), 1);
        // And one open order, not two.
        assert_eq!(service.dashboard().working_orders, 1);
        assert!(service.orders.is_empty());
    }

    #[test]
    fn combo_submission_is_idempotent_and_refuses_a_changed_retry() {
        let mut service = service_permitting_shorts();
        let intent = combo_intent("combo-000021", "2026-01-02T14:30:00Z");
        let market = combo_market("2026-01-02T14:30:00Z");
        let first = service
            .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
            .unwrap();
        let replay = service
            .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
            .unwrap();
        assert_eq!(first.order_id, replay.order_id);
        assert_eq!(first.decision, replay.decision);
        assert_eq!(service.combo_orders.len(), 1);

        // The same identity with different economics is refused outright, not
        // silently treated as the original.
        let mut tampered = intent.clone();
        tampered.combo_quantity = decimal("units", "5").unwrap();
        assert!(service
            .submit_combo_intent(tampered, market.clone(), "2026-01-02T14:30:02Z")
            .is_err());

        // And a retry that re-prices the original is refused too: a retry is a
        // retry, not a new decision wearing an old identity.
        let mut moved = market;
        moved.marks[0].mark_price = decimal("mark", "7.51").unwrap();
        assert!(service
            .submit_combo_intent(intent, moved, "2026-01-02T14:30:02Z")
            .is_err());
    }

    #[test]
    fn combo_refusal_records_evidence_and_creates_no_order() {
        let mut service = service_permitting_shorts();
        service
            .activate_kill_switch(KillSwitchScope::Global)
            .unwrap();
        let outcome = service
            .submit_combo_intent(
                combo_intent("combo-000022", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(!outcome.decision.approved);
        assert!(outcome.order_id.is_none());
        assert!(service.combo_orders.is_empty());
        // The refusal itself is durable evidence: a rejected combination is
        // still a decision that was made and has to be auditable.
        assert!(service
            .combo_risk_evidence("paper-combo-risk-combo-000022")
            .is_some());
    }

    #[test]
    fn combo_transport_failure_leaves_the_group_unknown_and_disconnects() {
        // Until E5.1 the fault wrapper did not forward combinations, so this
        // passed on the trait's refusal and the scheduled fault was never
        // consumed. It now exercises the fault itself; the test below shows
        // an unfaulted combination reaches the model.
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        let mut faulted = FaultInjectingBroker::new(adapter);
        faulted.inject(BrokerOperation::Submit, BrokerFault::Disconnect);
        let mut service = PaperTradingService::new(
            account,
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            faulted,
        )
        .unwrap();
        let result = service.submit_combo_intent(
            combo_intent("combo-000023", "2026-01-02T14:30:00Z"),
            combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        );
        // The outcome is genuinely unknown -- the request may or may not have
        // reached the venue -- so it is never reported as a clean rejection.
        assert!(result.is_err());
        let order = service.combo_order("combo-order-combo-000023").unwrap();
        assert_eq!(order.oms.state, OrderState::Unknown);
        // And an UNKNOWN combination blocks the next decision of either kind,
        // exactly as an UNKNOWN plain order does.
        assert!(service.has_unknown_order());
    }

    #[test]
    fn the_fault_wrapper_forwards_a_combination_when_no_fault_is_scheduled() {
        let account = account();
        let faulted = FaultInjectingBroker::new(IbkrPaperAdapter::new(&account).unwrap());
        let mut service = PaperTradingService::new(
            account,
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            faulted,
        )
        .unwrap();
        let outcome = service
            .submit_combo_intent(
                combo_intent("combo-000025", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert_eq!(outcome.state, Some(OrderState::Acknowledged));
        assert!(!service.has_unknown_order());
    }

    #[test]
    fn a_working_combination_is_visible_to_every_single_order_risk_counter() {
        let mut service = service_permitting_shorts();
        service
            .submit_combo_intent(
                combo_intent("combo-000024", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();

        // Open-order count, rate window, and reserved cash all see it.
        assert_eq!(service.working_order_count(), 1);
        assert_eq!(
            service.recent_order_count("2026-01-02T14:30:30Z").unwrap(),
            1
        );
        // 4 units * 2.50 net debit.
        assert_eq!(
            service.total_reserved_cash().unwrap(),
            decimal("reserved", "10").unwrap()
        );

        // And self-trade: the combination's short far-strike leg is a real
        // resting sell, so a plain buy on that instrument is a self-trade.
        assert!(service.conflicts_with_working_order("inst.us_option.spy.far", Side::Buy));
        assert!(!service.conflicts_with_working_order("inst.us_option.spy.far", Side::Sell));
        // The long near-strike leg is the mirror image.
        assert!(service.conflicts_with_working_order("inst.us_option.spy.near", Side::Sell));
    }

    #[test]
    fn a_working_combination_reconciles_against_its_independent_broker_order() {
        let mut service = service_permitting_shorts();
        service
            .submit_combo_intent(
                combo_intent("combo-000025", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
        assert!(report.is_clean(), "{:?}", report.issues);
        service.broker.combos.clear();
        let missing = service.reconcile("2026-01-02T21:00:01Z").unwrap();
        assert!(missing
            .issues
            .iter()
            .any(|issue| issue.category == "MISSING_BROKER_ORDER"));
    }

    #[test]
    fn an_authenticated_submitter_is_journaled_and_owns_the_retry() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "combo-submitted-by"
        ));
        let _ = fs::remove_file(&journal_path);
        let account = account();
        let open = || {
            PaperTradingService::open_durable(
                account.clone(),
                policy_permitting_shorts(),
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                IbkrPaperAdapter::new(&account).unwrap(),
                &journal_path,
            )
            .unwrap()
        };
        let mut service = open();
        let intent = combo_intent("combo-000027", "2026-01-02T14:30:00Z");
        let market = combo_market("2026-01-02T14:30:00Z");
        let decided_at = "2026-01-02T14:30:02Z";
        let outcome = service
            .submit_combo_intent_as(
                intent.clone(),
                market.clone(),
                decided_at,
                Some("user.trader"),
            )
            .unwrap();
        assert!(outcome.decision.approved);
        let decision_id = outcome.decision.decision_id.clone();
        assert_eq!(
            service
                .combo_risk_evidence(&decision_id)
                .unwrap()
                .submitted_by
                .as_deref(),
            Some("user.trader")
        );
        // Neither another operator nor an unattributed caller may claim it.
        for other in [Some("user.other"), None] {
            assert!(service
                .submit_combo_intent_as(intent.clone(), market.clone(), decided_at, other)
                .is_err());
        }
        assert!(service
            .submit_combo_intent_as(
                combo_intent("combo-000028", "2026-01-02T14:30:00Z"),
                market.clone(),
                decided_at,
                Some("User Trader"),
            )
            .is_err());
        // The original submitter's retry is the idempotent original.
        let retry = service
            .submit_combo_intent_as(intent, market.clone(), decided_at, Some("user.trader"))
            .unwrap();
        assert_eq!(retry.order_id, outcome.order_id);
        // A direct, unattributed submission journals no `submitted_by` key at
        // all, keeping the earlier serialization byte for byte.
        service
            .submit_combo_intent(
                combo_intent("combo-000029", "2026-01-02T14:30:00Z"),
                market,
                decided_at,
            )
            .unwrap();
        drop(service);

        // The attribution survives a restart.
        let reopened = open();
        assert_eq!(
            reopened
                .combo_risk_evidence(&decision_id)
                .unwrap()
                .submitted_by
                .as_deref(),
            Some("user.trader")
        );
        drop(reopened);
        let journal = fs::read_to_string(&journal_path).unwrap();
        let last: serde_json::Value =
            serde_json::from_str(journal.lines().last().unwrap()).unwrap();
        fn find<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
            match value {
                serde_json::Value::Object(map) => map
                    .get(key)
                    .or_else(|| map.values().find_map(|child| find(child, key))),
                serde_json::Value::Array(items) => items.iter().find_map(|child| find(child, key)),
                _ => None,
            }
        }
        let attributed = find(&last, "paper-combo-risk-combo-000027").unwrap();
        assert_eq!(attributed["submitted_by"], "user.trader");
        let unattributed = find(&last, "paper-combo-risk-combo-000029").unwrap();
        assert!(
            unattributed.get("submitted_by").is_none(),
            "an unattributed decision must not carry submitted_by"
        );
        let _ = fs::remove_file(&journal_path);
    }

    #[test]
    fn an_operator_kill_switch_change_is_journaled_and_survives_reopen() {
        let journal = |label: &str| {
            let path = std::env::temp_dir().join(format!(
                "follon-paper-journal-{}-{label}.ndjson",
                std::process::id()
            ));
            let _ = fs::remove_file(&path);
            path
        };
        let account = account();
        let open = |path: &Path| {
            PaperTradingService::open_durable(
                account.clone(),
                policy(),
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                IbkrPaperAdapter::new(&account).unwrap(),
                path,
            )
            .unwrap()
        };
        // The journal is exclusively locked while its service is open, so it
        // is read only after the service is dropped.
        let last_state = |path: &Path| {
            let journal = fs::read_to_string(path).unwrap();
            let record: serde_json::Value =
                serde_json::from_str(journal.lines().last().unwrap()).unwrap();
            record["state"].clone()
        };

        // A local, unattributed change journals no operator record at all,
        // keeping the earlier serialization byte for byte.
        let local_path = journal("kill-switch-local");
        let mut local = open(&local_path);
        assert!(local.activate_kill_switch(KillSwitchScope::Global).unwrap());
        drop(local);
        assert!(last_state(&local_path)
            .get("kill_switch_operations")
            .is_none());
        let _ = fs::remove_file(&local_path);

        let journal_path = journal("kill-switch-operator");
        let mut service = open(&journal_path);
        let spy = KillSwitchScope::Instrument("inst.us_equity.spy".to_owned());
        assert!(service
            .activate_kill_switch_as(spy.clone(), "user.risk", "2026-01-02T14:29:00Z")
            .unwrap());
        // A repeat changes nothing and journals nothing.
        assert!(!service
            .activate_kill_switch_as(spy.clone(), "user.risk", "2026-01-02T14:29:30Z")
            .unwrap());
        // The switch binds: an order on that instrument is refused.
        let refused = service
            .submit_intent(
                intent("intent-kill-operator-001", "2026-01-02T14:30:00Z"),
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:01Z",
            )
            .unwrap();
        assert_eq!(
            refused.decision.reason_codes,
            vec!["KILL_SWITCH_INSTRUMENT_INST.US_EQUITY.SPY".to_owned()]
        );
        // A malformed operator or time changes nothing.
        assert!(service
            .release_kill_switch_as(spy.clone(), "User Risk", "2026-01-02T14:31:00Z")
            .is_err());
        assert!(service
            .release_kill_switch_as(spy.clone(), "user.risk", "yesterday")
            .is_err());
        assert_eq!(
            service.kill_switches().active_keys(),
            vec!["instrument:inst.us_equity.spy".to_owned()]
        );
        assert!(service
            .release_kill_switch_as(spy, "user.risk.second", "2026-01-02T14:31:00Z")
            .unwrap());
        let expected = vec![
            KillSwitchOperation {
                scope: "instrument:inst.us_equity.spy".to_owned(),
                action: KillSwitchAction::Activate,
                operator: "user.risk".to_owned(),
                operated_at: "2026-01-02T14:29:00Z".to_owned(),
            },
            KillSwitchOperation {
                scope: "instrument:inst.us_equity.spy".to_owned(),
                action: KillSwitchAction::Release,
                operator: "user.risk.second".to_owned(),
                operated_at: "2026-01-02T14:31:00Z".to_owned(),
            },
        ];
        assert_eq!(service.kill_switch_operations(), expected.as_slice());
        drop(service);

        // The journal names each operator, and a reopen restores the record.
        let state = last_state(&journal_path);
        assert_eq!(state["kill_switch_operations"][0]["operator"], "user.risk");
        assert_eq!(state["kill_switch_operations"][1]["action"], "RELEASE");
        let reopened = open(&journal_path);
        assert_eq!(reopened.kill_switch_operations(), expected.as_slice());
        assert!(reopened.kill_switches().active_keys().is_empty());
        drop(reopened);
        let _ = fs::remove_file(&journal_path);
    }

    #[test]
    fn a_persisted_kill_switch_operation_must_be_well_formed() {
        let mut attributed = service();
        attributed
            .activate_kill_switch_as(KillSwitchScope::Global, "user.risk", "2026-01-02T14:29:00Z")
            .unwrap();
        let valid = attributed.persistent_state();
        let mut restored = service();
        restored.restore(valid.clone()).unwrap();
        assert_eq!(restored.kill_switch_operations().len(), 1);
        let corruptions: [fn(&mut PersistentKillSwitchOperation); 4] = [
            |operation| operation.scope = "everything".to_owned(),
            |operation| operation.action = "TOGGLE".to_owned(),
            |operation| operation.operator = "User Risk".to_owned(),
            |operation| operation.operated_at = "yesterday".to_owned(),
        ];
        for corrupt in corruptions {
            let mut state = valid.clone();
            corrupt(&mut state.kill_switch_operations[0]);
            assert!(service().restore(state).is_err());
        }
    }

    fn scratch_journal(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{label}.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        path
    }

    /// The journal's last recorded state, read once its service is dropped:
    /// an open service holds the journal exclusively.
    fn last_journal_state(path: &Path) -> serde_json::Value {
        let journal = fs::read_to_string(path).unwrap();
        let record: serde_json::Value =
            serde_json::from_str(journal.lines().last().unwrap()).unwrap();
        record["state"].clone()
    }

    #[test]
    fn an_authenticated_order_submitter_is_journaled_and_owns_the_retry() {
        let journal_path = scratch_journal("order-submitted-by");
        let account = account();
        let open = || {
            PaperTradingService::open_durable(
                account.clone(),
                policy(),
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                IbkrPaperAdapter::new(&account).unwrap(),
                &journal_path,
            )
            .unwrap()
        };
        let mut service = open();
        let order = intent("paper-000041", "2026-01-02T14:30:00Z");
        let observation = market("2026-01-02T14:30:00Z");
        let decided_at = "2026-01-02T14:30:02Z";
        let outcome = service
            .submit_intent_as(
                order.clone(),
                observation.clone(),
                decided_at,
                Some("user.trader"),
            )
            .unwrap();
        assert!(outcome.decision.approved);
        let decision_id = outcome.decision.decision_id.clone();
        assert_eq!(
            service
                .risk_evidence(&decision_id)
                .unwrap()
                .submitted_by
                .as_deref(),
            Some("user.trader")
        );
        // Neither another operator nor an unattributed caller may claim it.
        for other in [Some("user.other"), None] {
            assert!(service
                .submit_intent_as(order.clone(), observation.clone(), decided_at, other)
                .is_err());
        }
        // A malformed submitter is refused before risk sees the intent.
        assert!(service
            .submit_intent_as(
                intent("paper-000042", "2026-01-02T14:30:00Z"),
                observation.clone(),
                decided_at,
                Some("User Trader"),
            )
            .is_err());
        assert!(service.risk_evidence("paper-risk-paper-000042").is_none());
        // The original submitter's retry is the idempotent original.
        let retry = service
            .submit_intent_as(order, observation.clone(), decided_at, Some("user.trader"))
            .unwrap();
        assert_eq!(retry.order_id, outcome.order_id);
        // A direct submission journals no `submitted_by` key at all, keeping
        // the earlier serialization byte for byte.
        service
            .submit_intent(
                intent("paper-000043", "2026-01-02T14:30:00Z"),
                observation,
                decided_at,
            )
            .unwrap();
        drop(service);

        let state = last_journal_state(&journal_path);
        assert_eq!(
            state["risk_evidence"]["paper-risk-paper-000041"]["submitted_by"],
            "user.trader"
        );
        assert!(
            state["risk_evidence"]["paper-risk-paper-000043"]
                .get("submitted_by")
                .is_none(),
            "an unattributed decision must not carry submitted_by"
        );
        // The attribution survives a restart.
        let reopened = open();
        assert_eq!(
            reopened
                .risk_evidence(&decision_id)
                .unwrap()
                .submitted_by
                .as_deref(),
            Some("user.trader")
        );
        drop(reopened);
        let _ = fs::remove_file(&journal_path);
    }

    /// The model, except that asking it to cancel stops the process, as a
    /// crash just after the broker received the request would.
    struct CrashOnCancelBroker(IbkrPaperAdapter);

    impl PaperBrokerAdapter for CrashOnCancelBroker {
        fn adapter_configuration_fingerprint(
            &self,
            account_id: &str,
        ) -> Result<String, PaperError> {
            self.0.adapter_configuration_fingerprint(account_id)
        }

        fn configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
            self.0.configuration_fingerprint(account_id)
        }

        fn permits_empty_journal(&self, account_id: &str) -> bool {
            self.0.permits_empty_journal(account_id)
        }

        fn submit(
            &mut self,
            request: &BrokerOrderRequest,
        ) -> Result<BrokerSubmitResult, PaperError> {
            self.0.submit(request)
        }

        fn cancel(&mut self, _request: &BrokerCancelRequest) -> Result<(), PaperError> {
            panic!("the process stopped while the broker held the cancellation")
        }

        fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
            self.0.poll(account_id)
        }

        fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
            self.0.snapshot(account_id)
        }

        fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
            self.0.reconnect(account_id)
        }
    }

    #[test]
    fn an_operator_cancellation_is_durable_before_the_broker_is_asked() {
        let journal_path = scratch_journal("order-cancel-crash");
        let account = account();
        let mut service = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            CrashOnCancelBroker(IbkrPaperAdapter::new(&account).unwrap()),
            &journal_path,
        )
        .unwrap();
        let order_id = service
            .submit_intent_as(
                intent("paper-000051", "2026-01-02T14:30:00Z"),
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
                Some("user.trader"),
            )
            .unwrap()
            .order_id
            .unwrap();
        assert_eq!(
            service.order(&order_id).unwrap().oms.state,
            OrderState::Acknowledged
        );
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.cancel_order_as(&order_id, "user.risk", "2026-01-02T14:31:00Z")
        }));
        assert!(crashed.is_err(), "the broker was never asked to cancel");
        drop(service);

        // The restarted service knows the cancellation was requested, and by
        // whom, although the process stopped before it heard back.
        let reopened = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        assert_eq!(
            reopened.order(&order_id).unwrap().oms.state,
            OrderState::PendingCancel
        );
        assert_eq!(
            reopened.order_operations(),
            [OrderOperation {
                order_id: order_id.clone(),
                action: OrderOperationAction::CancelRequested,
                operator: "user.risk".to_owned(),
                operated_at: "2026-01-02T14:31:00Z".to_owned(),
            }]
            .as_slice()
        );
        drop(reopened);
        let _ = fs::remove_file(&journal_path);
    }

    #[test]
    fn an_operator_cancellation_is_journaled_once_for_a_single_order_or_a_combination() {
        let account = account();
        let open = |path: &Path| {
            PaperTradingService::open_durable(
                account.clone(),
                policy_permitting_shorts(),
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                IbkrPaperAdapter::new(&account).unwrap(),
                path,
            )
            .unwrap()
        };

        // A direct, unattributed cancellation journals no operation record at
        // all, keeping the earlier serialization byte for byte.
        let local_path = scratch_journal("order-operations-local");
        let mut local = open(&local_path);
        let local_id = local
            .submit_intent(
                intent("paper-000060", "2026-01-02T14:30:00Z"),
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap()
            .order_id
            .unwrap();
        local.cancel_order(&local_id).unwrap();
        drop(local);
        let local_state = last_journal_state(&local_path);
        assert_eq!(
            local_state["orders"][local_id.as_str()]["state"],
            "PENDING_CANCEL"
        );
        assert!(local_state.get("order_operations").is_none());
        let _ = fs::remove_file(&local_path);

        let journal_path = scratch_journal("order-operations");
        let mut service = open(&journal_path);
        let submit = |service: &mut PaperTradingService<IbkrPaperAdapter>, intent_id: &str| {
            service
                .submit_intent(
                    intent(intent_id, "2026-01-02T14:30:00Z"),
                    market("2026-01-02T14:30:00Z"),
                    "2026-01-02T14:30:02Z",
                )
                .unwrap()
                .order_id
                .unwrap()
        };
        let order_id = submit(&mut service, "paper-000061");
        let direct_id = submit(&mut service, "paper-000062");
        let combo_id = submit_lifecycle_combo(&mut service, "operator-cancel");

        // A malformed operator or time changes nothing.
        assert!(service
            .cancel_order_as(&order_id, "User Risk", "2026-01-02T14:31:00Z")
            .is_err());
        assert!(service
            .cancel_order_as(&order_id, "user.risk", "yesterday")
            .is_err());
        assert_eq!(
            service.order(&order_id).unwrap().oms.state,
            OrderState::Acknowledged
        );
        assert!(service.order_operations().is_empty());

        service
            .cancel_order_as(&order_id, "user.risk", "2026-01-02T14:31:00Z")
            .unwrap();
        // A retry, even by another operator, changes nothing and journals nothing.
        service
            .cancel_order_as(&order_id, "user.risk.second", "2026-01-02T14:31:05Z")
            .unwrap();
        service
            .cancel_order_as(&combo_id, "user.risk.second", "2026-01-02T14:31:10Z")
            .unwrap();
        // A direct cancellation is not attributed.
        service.cancel_order(&direct_id).unwrap();
        let operation = |order_id: &str, operator: &str, operated_at: &str| OrderOperation {
            order_id: order_id.to_owned(),
            action: OrderOperationAction::CancelRequested,
            operator: operator.to_owned(),
            operated_at: operated_at.to_owned(),
        };
        let expected = vec![
            operation(&order_id, "user.risk", "2026-01-02T14:31:00Z"),
            operation(&combo_id, "user.risk.second", "2026-01-02T14:31:10Z"),
        ];
        assert_eq!(service.order_operations(), expected.as_slice());
        for id in [&order_id, &direct_id] {
            assert_eq!(
                service.order(id).unwrap().oms.state,
                OrderState::PendingCancel
            );
        }
        assert_eq!(
            service.combo_order(&combo_id).unwrap().oms.state,
            OrderState::PendingCancel
        );
        drop(service);

        let state = last_journal_state(&journal_path);
        assert_eq!(state["order_operations"].as_array().unwrap().len(), 2);
        assert_eq!(state["order_operations"][0]["action"], "CANCEL_REQUESTED");
        assert_eq!(state["order_operations"][1]["operator"], "user.risk.second");
        let reopened = open(&journal_path);
        assert_eq!(reopened.order_operations(), expected.as_slice());
        drop(reopened);
        let _ = fs::remove_file(&journal_path);
    }

    #[test]
    fn a_persisted_order_attribution_must_be_well_formed() {
        let mut attributed = service();
        let order_id = attributed
            .submit_intent_as(
                intent("paper-000071", "2026-01-02T14:30:00Z"),
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
                Some("user.trader"),
            )
            .unwrap()
            .order_id
            .unwrap();
        attributed
            .cancel_order_as(&order_id, "user.risk", "2026-01-02T14:31:00Z")
            .unwrap();
        let valid = attributed.persistent_state();
        let mut restored = service();
        restored.restore(valid.clone()).unwrap();
        assert_eq!(restored.order_operations().len(), 1);
        assert_eq!(
            restored
                .risk_evidence("paper-risk-paper-000071")
                .unwrap()
                .submitted_by
                .as_deref(),
            Some("user.trader")
        );
        let corruptions: [fn(&mut PersistentPaperState); 5] = [
            |state| state.order_operations[0].order_id = "order-paper-000099".to_owned(),
            |state| state.order_operations[0].action = "CANCEL".to_owned(),
            |state| state.order_operations[0].operator = "User Risk".to_owned(),
            |state| state.order_operations[0].operated_at = "yesterday".to_owned(),
            |state| {
                state
                    .risk_evidence
                    .get_mut("paper-risk-paper-000071")
                    .unwrap()
                    .submitted_by = Some("User Trader".to_owned());
            },
        ];
        for corrupt in corruptions {
            let mut state = valid.clone();
            corrupt(&mut state);
            assert!(service().restore(state).is_err());
        }
    }

    #[test]
    fn a_kill_switch_scope_parses_from_its_stable_key() {
        for scope in [
            KillSwitchScope::Global,
            KillSwitchScope::Account("acct.paper.001".to_owned()),
            KillSwitchScope::Strategy("strategy.paper.001".to_owned()),
            KillSwitchScope::Instrument("inst.us_equity.spy".to_owned()),
        ] {
            assert_eq!(KillSwitchScope::from_key(&scope.as_key()).unwrap(), scope);
        }
        for malformed in [
            "",
            "GLOBAL",
            "everything",
            "instrument:",
            "account:Not Canonical",
        ] {
            assert!(
                KillSwitchScope::from_key(malformed).is_err(),
                "accepted {malformed:?}"
            );
        }
    }

    #[test]
    fn a_combination_survives_a_durable_journal_reopen() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "combo-recovery"
        ));
        let _ = fs::remove_file(&journal_path);
        let account = account();
        let mut service = PaperTradingService::open_durable(
            account.clone(),
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        let intent = combo_intent("combo-000026", "2026-01-02T14:30:00Z");
        let market = combo_market("2026-01-02T14:30:00Z");
        service
            .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
            .unwrap();
        let before = service
            .combo_order("combo-order-combo-000026")
            .unwrap()
            .clone();
        drop(service);

        let reopened = PaperTradingService::open_durable(
            account.clone(),
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        let after = reopened.combo_order("combo-order-combo-000026").unwrap();
        // The whole structure comes back exactly: every leg, its ratio, its
        // protected price, the price-limit kind, and the per-leg observation
        // that priced it.
        assert_eq!(after.oms.intent, intent);
        assert_eq!(after.market, market);
        assert_eq!(after.oms.state, before.oms.state);
        assert_eq!(after.broker_order_id, before.broker_order_id);
        assert_eq!(
            after.oms.intent.price_limit.kind(),
            follon_domain::ComboPriceLimit::MaximumDebit(Decimal::ZERO).kind()
        );
        // And the reservation it implies survives with it, so a restart does
        // not free cash the combination still has committed.
        assert_eq!(
            reopened.total_reserved_cash().unwrap(),
            decimal("reserved", "10").unwrap()
        );
        let _ = fs::remove_file(&journal_path);
    }

    /// An absent combination field deserializes as "no combinations".
    ///
    /// This checks the persisted *type*, not a reopened journal file, and the
    /// distinction is real. `FilePaperJournal::open` additionally requires each
    /// line to re-serialize byte-for-byte, so a file missing any field the
    /// current serializer writes is rejected before `#[serde(default)]` can
    /// apply. That is pre-existing behaviour, not a consequence of the
    /// combination fields: deleting `tax_lots` -- which predates them -- from a
    /// journal line fails exactly the same way, verified directly. So the
    /// defaults make the type tolerant, which is what a future format change
    /// needs, while whole-file compatibility across a schema change remains an
    /// open question recorded in `docs/06-delivery/16-delivery-state.md`.
    #[test]
    fn an_absent_combination_field_deserializes_as_no_combinations() {
        let without_combinations = serde_json::json!({
            "configuration_fingerprint": "",
            "account_id": "acct.paper.001",
            "currency": "USD",
            "cash": "100000.00000000",
            "orders": {},
            "risk_evidence": {},
            "positions": {},
            "execution_ids": [],
            "active_kill_switches": [],
            "incidents": {},
            "last_reconciled_at": serde_json::Value::Null,
            "last_reconciliation_clean": serde_json::Value::Null,
            "paper_days": {},
            "next_reconciliation": 0,
        });
        let state: PersistentPaperState = serde_json::from_value(without_combinations).unwrap();
        assert!(state.combo_orders.is_empty());
        assert!(state.combo_risk_evidence.is_empty());
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
        intent.price_limit =
            follon_domain::ComboPriceLimit::MaximumDebit(decimal("cap", "7").unwrap());
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

    fn paper_session(exchange_date: &str) -> PaperTradingSession {
        let (opens_at, closes_at) = if exchange_date >= "2026-03-09" {
            ("13:30:00Z", "20:00:00Z")
        } else {
            ("14:30:00Z", "21:00:00Z")
        };
        PaperTradingSession {
            calendar_id: "cal.us_equities.nyse.v1".to_owned(),
            session: TradingSession {
                exchange_date: exchange_date.to_owned(),
                opens_at: format!("{exchange_date}T{opens_at}"),
                closes_at: format!("{exchange_date}T{closes_at}"),
            },
        }
    }

    fn paper_calendar(sessions: &[PaperTradingSession]) -> StaticTradingCalendar {
        StaticTradingCalendar::new(
            "cal.us_equities.nyse.v1",
            sessions.iter().map(|value| value.session.clone()).collect(),
        )
        .expect("test paper calendar")
    }

    struct CashAndPositionMismatchBroker;

    impl PaperBrokerAdapter for CashAndPositionMismatchBroker {
        fn submit(
            &mut self,
            _request: &BrokerOrderRequest,
        ) -> Result<BrokerSubmitResult, PaperError> {
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
    fn paper_fifo_tax_lots_track_disposal_cost_basis_independent_of_average_cost() {
        let mut service = service();

        // First lot: 2 units @ $100.
        let first = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "2").unwrap(),
                    ..intent("intent-tax-lot-001", "2026-01-02T14:31:00Z")
                },
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        let first_order_id = first.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &first_order_id,
                decimal("quantity", "2").unwrap(),
                decimal("price", "100").unwrap(),
                decimal("fee", "0.20").unwrap(),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // Second lot: 2 units @ $120, a distinctly higher price than the
        // first, so a FIFO disposal's cost basis provably differs from what
        // a blended average cost would report.
        let second = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "2").unwrap(),
                    ..intent("intent-tax-lot-002", "2026-01-02T14:32:00Z")
                },
                market("2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        let second_order_id = second.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &second_order_id,
                decimal("quantity", "2").unwrap(),
                decimal("price", "120").unwrap(),
                decimal("fee", "0.20").unwrap(),
                "2026-01-02T14:32:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);
        assert_eq!(service.tax_lots("inst.us_equity.spy").len(), 2);

        // Dispose 2 units at $130. FIFO must consume the $100 lot first.
        let sell = service
            .submit_intent(
                OrderIntent {
                    side: Side::Sell,
                    quantity: decimal("quantity", "2").unwrap(),
                    ..intent("intent-tax-lot-003", "2026-01-02T14:33:00Z")
                },
                market("2026-01-02T14:33:00Z"),
                "2026-01-02T14:33:00Z",
            )
            .unwrap();
        let sell_order_id = sell.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &sell_order_id,
                decimal("quantity", "2").unwrap(),
                decimal("price", "130").unwrap(),
                decimal("fee", "0.20").unwrap(),
                "2026-01-02T14:33:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // The FIFO book fully consumed the $100 lot (all-in unit cost
        // 100.10) and left the $120 lot (all-in unit cost 120.10) untouched.
        let remaining = service.tax_lots("inst.us_equity.spy");
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].unit_cost,
            decimal("unit cost", "120.10").unwrap()
        );
        // realized = proceeds(260) - fifo cost basis(200.20) - fee(0.20) = 59.60
        assert_eq!(
            service.realized_tax_pnl().unwrap(),
            decimal("realized", "59.60").unwrap()
        );

        // Portfolio's own blended average-cost realized P&L is a distinct
        // figure: average cost across both lots is 110.10/unit, so
        // realized = (130 - 110.10) * 2 - 0.20 = 39.60 -- proving the
        // tax-lot book is an independent ledger, not a relabeling of the
        // existing average-cost figure.
        let dashboard = service.dashboard();
        assert_eq!(dashboard.positions[0].realized_pnl, "39.60000000");
    }

    #[test]
    fn paper_tax_lots_survive_a_durable_journal_reopen() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-tax-lots.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let account = account();
        let mut durable = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        let submitted = durable
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "2").unwrap(),
                    ..intent("intent-tax-lot-durable-001", "2026-01-02T14:31:00Z")
                },
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        let order_id = submitted.order_id.unwrap();
        durable
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "2").unwrap(),
                decimal("price", "100").unwrap(),
                decimal("fee", "0.20").unwrap(),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(durable.synchronize().unwrap(), 2);
        assert_eq!(durable.tax_lots("inst.us_equity.spy").len(), 1);
        drop(durable);

        let recovered = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        let remaining = recovered.tax_lots("inst.us_equity.spy");
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].unit_cost,
            decimal("unit cost", "100.10").unwrap()
        );
        assert_eq!(
            remaining[0].remaining_quantity,
            decimal("qty", "2").unwrap()
        );
        assert_eq!(recovered.realized_tax_pnl().unwrap(), Decimal::ZERO);
        drop(recovered);
        let _ = fs::remove_file(&journal_path);
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
    fn legacy_single_account_journal_reopens_through_registry_without_rewriting_evidence() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-registry-migration-{}.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let paper_account = account();
        let mut legacy_service = PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();
        legacy_service
            .activate_kill_switch(KillSwitchScope::Global)
            .unwrap();
        drop(legacy_service);

        let mut registry = PaperBrokerRegistry::new();
        registry
            .register_legacy_ibkr_paper_route(&paper_account)
            .unwrap();
        let recovered = PaperTradingService::open_durable(
            paper_account,
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            registry,
            &journal_path,
        )
        .unwrap();
        assert_eq!(
            recovered.kill_switches().active_keys(),
            vec!["global".to_owned()]
        );
        drop(recovered);
        let _ = fs::remove_file(&journal_path);
    }

    #[test]
    fn legacy_registry_refuses_empty_journals_and_modern_route_changes_fail_recovery() {
        let legacy_path = std::env::temp_dir().join(format!(
            "follon-paper-legacy-empty-{}.ndjson",
            std::process::id()
        ));
        let modern_path = std::env::temp_dir().join(format!(
            "follon-paper-route-mismatch-{}.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&legacy_path);
        let _ = fs::remove_file(&modern_path);
        let paper_account = account();

        let mut legacy_registry = PaperBrokerRegistry::new();
        legacy_registry
            .register_legacy_ibkr_paper_route(&paper_account)
            .unwrap();
        assert!(PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            legacy_registry,
            &legacy_path,
        )
        .is_err());

        let mut modern_service = PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            modern_registry(&paper_account, "venue.ibkr.paper"),
            &modern_path,
        )
        .unwrap();
        modern_service
            .activate_kill_switch(KillSwitchScope::Global)
            .unwrap();
        drop(modern_service);
        assert!(PaperTradingService::open_durable(
            paper_account,
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            modern_registry(&account(), "venue.ibkr.paper.changed"),
            &modern_path,
        )
        .is_err());
        let _ = fs::remove_file(&legacy_path);
        let _ = fs::remove_file(&modern_path);
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

    #[test]
    fn paper_portfolio_risk_composition_is_skipped_when_equity_is_not_positive() {
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
    fn paper_journal_allows_exactly_one_open_operator_process() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "exclusive-lock"
        ));
        let _ = fs::remove_file(&journal_path);
        let first = FilePaperJournal::open(&journal_path).unwrap();
        assert!(FilePaperJournal::open(&journal_path).is_err());
        drop(first);
        let reopened = FilePaperJournal::open(&journal_path).unwrap();
        drop(reopened);
        fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn paper_journal_refuses_a_tampered_hash_chain() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-tampered.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let paper_account = account();
        let service = PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .expect("durable service");
        drop(service);
        let original = fs::read_to_string(&journal_path).expect("read journal");
        let tampered = original.replacen("100000.00000000", "100001.00000000", 1);
        assert_ne!(tampered, original);
        fs::write(&journal_path, tampered).expect("tamper journal");

        assert!(PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .is_err());
        fs::remove_file(journal_path).unwrap();
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

    #[test]
    fn durable_journal_recovers_unknown_orders_and_30_clean_day_gate_is_measured() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "recovery"
        ));
        let _ = fs::remove_file(&journal_path);
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        let mut faulted = FaultInjectingBroker::new(adapter);
        faulted.inject(BrokerOperation::Submit, BrokerFault::Disconnect);
        let mut durable_service = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            faulted,
            &journal_path,
        )
        .unwrap();
        assert!(durable_service
            .submit_intent(
                intent("intent-paper-004", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .is_err());
        drop(durable_service);

        let recovered = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        assert_eq!(
            recovered.order("order-intent-paper-004").unwrap().oms.state,
            OrderState::Unknown
        );
        drop(recovered);
        let mut incompatible_policy = policy();
        incompatible_policy.max_order_notional = decimal("notional", "40000").unwrap();
        assert!(PaperTradingService::open_durable(
            account.clone(),
            incompatible_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .is_err());
        let gate_journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "thirty-day-gate"
        ));
        let _ = fs::remove_file(&gate_journal_path);
        let gate_account = account.clone();
        let mut gate_service = PaperTradingService::open_durable(
            gate_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&gate_account).unwrap(),
            &gate_journal_path,
        )
        .unwrap();
        let dates = [
            "2026-03-02",
            "2026-03-03",
            "2026-03-04",
            "2026-03-05",
            "2026-03-06",
            "2026-03-09",
            "2026-03-10",
            "2026-03-11",
            "2026-03-12",
            "2026-03-13",
            "2026-03-16",
            "2026-03-17",
            "2026-03-18",
            "2026-03-19",
            "2026-03-20",
            "2026-03-23",
            "2026-03-24",
            "2026-03-25",
            "2026-03-26",
            "2026-03-27",
            "2026-03-30",
            "2026-03-31",
            "2026-04-01",
            "2026-04-02",
            "2026-04-06",
            "2026-04-07",
            "2026-04-08",
            "2026-04-09",
            "2026-04-10",
            "2026-04-13",
        ];
        let gate_sessions: Vec<_> = dates.iter().map(|date| paper_session(date)).collect();
        let gate_calendar = paper_calendar(&gate_sessions);
        for (date, session) in dates.into_iter().zip(&gate_sessions) {
            let report = gate_service
                .reconcile(&format!("{date}T21:00:00Z"))
                .unwrap();
            assert!(report.is_clean());
            gate_service
                .record_paper_session(session, &report, &gate_calendar)
                .unwrap();
        }
        assert!(gate_service.promotion_status().eligible_for_next_gate);
        drop(gate_service);
        fs::remove_file(gate_journal_path).unwrap();
        fs::remove_file(journal_path).unwrap();
    }
}
