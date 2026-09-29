//! A position held across a stock split is the same position in the replay engine and in
//! the ledger (delivery state E8.2).
//!
//! `BacktestLedger` applied a split to its position and its FIFO lots (E8.1). The replay
//! engine keeps a portfolio of its own, and it is that portfolio the strategy's execution
//! callbacks and the fingerprinted event stream project. It did not follow the split, so
//! a strategy that held one share across a 2:1 split and then sold the two it now held
//! was refused: the engine still believed it held one, and the whole backtest aborted.

use std::collections::BTreeMap;
use std::str::FromStr;

use follon_backtest::{
    BacktestInput, BacktestRunner, BacktestSpec, CompletedBacktest, DatasetManifest,
};
use follon_control_plane::{
    DeterministicFillModel, EngineError, HistoricalBar, MarketPreconditions, ReplayEngine,
    RiskPolicy, Strategy,
};
use follon_domain::{Bar, Decimal, OrderIntent, OrderType, Side, TimeInForce};
use follon_instrument::{
    AssetClass, Instrument, InstrumentRegistry, InstrumentVersion, StaticTradingCalendar,
    TradingSession,
};
use follon_market_data::CorporateAction;

const ACCOUNT: &str = "acct-paper-001";
const INSTRUMENT: &str = "inst.us_equity.spy";
const SPLIT_ID: &str = "action-split-001";

fn amount(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

/// Buys one share on its first bar and sells `sell_quantity` on its fourth. Each order
/// fills on the bar after the one that produced it, so the entry fills on the second bar
/// and the exit on the fifth.
struct RoundTrip {
    bars_seen: u32,
    sell_quantity: i64,
}

impl Strategy for RoundTrip {
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError> {
        self.bars_seen += 1;
        let (side, quantity, tag) = match self.bars_seen {
            1 => (Side::Buy, 1, "entry"),
            4 => (Side::Sell, self.sell_quantity, "exit"),
            _ => return Ok(None),
        };
        Ok(Some(OrderIntent {
            intent_id: format!("intent-split-{tag}"),
            account_id: ACCOUNT.to_owned(),
            strategy_id: "strategy-split-001".to_owned(),
            instrument_id: bar.instrument_id.clone(),
            correlation_id: format!("corr-split-{tag}"),
            side,
            quantity: Decimal::from_integer(quantity)?,
            order_type: OrderType::Market,
            limit_price: None,
            time_in_force: TimeInForce::Day,
            rationale: "split parity regression".to_owned(),
            created_at: replay_time.to_owned(),
            strategy_version: "strategy-split-v1".to_owned(),
            configuration_version: "cfg-v1".to_owned(),
            environment: "SIMULATION".to_owned(),
        }))
    }
}

fn bar_at(price: &str) -> Bar {
    let close = amount(price);
    Bar {
        instrument_id: INSTRUMENT.to_owned(),
        open: close,
        high: close.checked_add(amount("1")).unwrap(),
        low: close.checked_sub(amount("1")).unwrap(),
        close,
        volume: amount("1000"),
        interval_seconds: 60,
        exchange_timezone: "America/New_York".to_owned(),
    }
}

/// Five bars. The first two trade near 100 and the last three near 50, because a 2:1
/// split lands between the second and the third.
fn input(actions: Vec<CorporateAction>) -> BacktestInput {
    let prices = ["100", "100", "50", "50", "50"];
    BacktestInput {
        account_id: ACCOUNT.to_owned(),
        currency: "USD".to_owned(),
        initial_cash: amount("1000"),
        bars: prices
            .iter()
            .enumerate()
            .map(|(index, price)| HistoricalBar {
                event_time: format!("2026-01-02T14:{:02}:00Z", 30 + index),
                bar: bar_at(price),
            })
            .collect(),
        corporate_actions: actions,
    }
}

fn split_at(effective_at: &str, ratio: &str) -> CorporateAction {
    CorporateAction::Split {
        action_id: SPLIT_ID.to_owned(),
        instrument_id: INSTRUMENT.to_owned(),
        effective_at: effective_at.to_owned(),
        ratio: amount(ratio),
    }
}

fn two_for_one() -> Vec<CorporateAction> {
    vec![split_at("2026-01-02T14:31:30Z", "2")]
}

fn spec(input: &BacktestInput) -> BacktestSpec {
    let dataset_bars: Vec<_> = input
        .bars
        .iter()
        .map(|bar| (bar.event_time.clone(), bar.bar.clone()))
        .collect();
    BacktestSpec {
        strategy_bundle_hash: "a".repeat(64),
        dataset: DatasetManifest::from_market_data(
            "dataset.spy",
            "v1",
            "reference-example-1",
            "universe.spy",
            &dataset_bars,
            &input.corporate_actions,
        )
        .unwrap(),
        configuration_id: "config.test".to_owned(),
        configuration_version: "cfg-v1".to_owned(),
        configuration_hash: "b".repeat(64),
        seed: 7,
        engine_version: "engine-v1".to_owned(),
        starts_at: "2026-01-02T14:30:00Z".to_owned(),
        ends_at: "2026-01-02T14:34:00Z".to_owned(),
    }
}

fn market() -> (InstrumentRegistry, StaticTradingCalendar) {
    let calendar = StaticTradingCalendar::new(
        "cal.us_equities.nyse",
        vec![TradingSession {
            exchange_date: "2026-01-02".to_owned(),
            opens_at: "2026-01-02T14:30:00Z".to_owned(),
            closes_at: "2026-01-02T21:00:00Z".to_owned(),
        }],
    )
    .unwrap();
    let mut instruments = InstrumentRegistry::default();
    instruments
        .register(InstrumentVersion {
            instrument: Instrument {
                instrument_id: INSTRUMENT.to_owned(),
                symbol: "SPY".to_owned(),
                exchange_symbol: "SPY".to_owned(),
                asset_class: AssetClass::Etf,
                venue: "venue.nyse_arca".to_owned(),
                currency: "USD".to_owned(),
                broker_ids: BTreeMap::new(),
                tick_size: amount("0.01"),
                lot_size: amount("1"),
                multiplier: amount("1"),
                trading_calendar_id: "cal.us_equities.nyse".to_owned(),
            },
            effective_from: "2026-01-01T00:00:00Z".to_owned(),
            effective_to: None,
            reference_version: "reference-example-1".to_owned(),
        })
        .unwrap();
    (instruments, calendar)
}

fn engine() -> ReplayEngine {
    ReplayEngine::new(
        "2026-01-02T14:30:00Z",
        "engine-v1",
        "cfg-v1",
        RiskPolicy {
            version: "risk-v1".to_owned(),
            global_kill_switch: false,
            max_quantity: amount("10"),
            max_notional: amount("10000"),
            max_price_deviation_bps: amount("500"),
            max_news_slippage_bps: None,
            max_news_spread_multiplier_bps: None,
        },
        DeterministicFillModel {
            spread_bps: Decimal::ZERO,
            slippage_bps: Decimal::ZERO,
            flat_fee: amount("0.10"),
            latency_bars: 0,
            max_fill_quantity: None,
        },
    )
    .unwrap()
}

fn run(
    actions: Vec<CorporateAction>,
    sell_quantity: i64,
) -> Result<CompletedBacktest, follon_backtest::BacktestError> {
    let input = input(actions);
    let spec = spec(&input);
    let (instruments, calendar) = market();
    let market = MarketPreconditions {
        instruments: &instruments,
        calendar: &calendar,
    };
    BacktestRunner::new(spec, engine()).unwrap().run(
        &mut RoundTrip {
            bars_seen: 0,
            sell_quantity,
        },
        &input,
        &market,
    )
}

fn events(completed: &CompletedBacktest) -> Vec<serde_json::Value> {
    completed
        .canonical_events
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn positions(events: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    events
        .iter()
        .filter(|event| event["event_type"] == "portfolio.position_updated.v1")
        .collect()
}

fn decimal_of(value: &serde_json::Value) -> Decimal {
    amount(value.as_str().unwrap())
}

#[test]
fn a_strategy_that_holds_across_a_split_can_sell_what_it_now_holds() {
    // Before E8.2 the engine still held one share when the strategy sold two, and the
    // backtest aborted with "first slice does not permit short positions".
    let completed = run(two_for_one(), 2).expect("the post-split quantity is what is held");

    assert_eq!(
        completed.applied_corporate_action_ids,
        vec![SPLIT_ID.to_owned()]
    );
    let report = &completed.artifact.report;
    // Bought 100 + 0.10 fee. Sold two at 50 less 0.10.
    assert_eq!(report.cash, amount("999.80"));
    assert!(report
        .positions
        .iter()
        .all(|position| position.quantity == Decimal::ZERO));
}

#[test]
fn the_engines_position_follows_the_split_in_the_event_stream() {
    let completed = run(two_for_one(), 2).unwrap();
    let events = events(&completed);
    let positions = positions(&events);
    // The entry fill, the split, the exit fill.
    assert_eq!(positions.len(), 3, "{positions:#?}");

    let entry = &positions[0]["payload"];
    assert_eq!(decimal_of(&entry["quantity"]), amount("1"));
    assert_eq!(decimal_of(&entry["average_cost"]), amount("100.10"));

    // The split is a Position event of its own, from the portfolio engine, caused by the
    // corporate action and not by a fill, and it carries the scaled position exactly.
    let split = positions[1];
    assert_eq!(decimal_of(&split["payload"]["quantity"]), amount("2"));
    assert_eq!(
        decimal_of(&split["payload"]["average_cost"]),
        amount("50.05")
    );
    assert_eq!(decimal_of(&split["payload"]["realized_pnl"]), Decimal::ZERO);
    assert_eq!(split["actor"], "portfolio_engine");
    assert_eq!(split["source"], "corporate_action");
    assert_eq!(
        split["correlation_id"],
        format!("corr-corporate-action-{SPLIT_ID}")
    );
    assert!(split["causation_id"].is_null());
    assert_eq!(split["account_id"], ACCOUNT);
    assert_eq!(split["instrument_id"], INSTRUMENT);

    // It is recorded when the replay applied it, on the first bar at or after the split
    // took effect, and before that bar's own market event.
    let split_index = events.iter().position(|event| event == split).unwrap();
    assert_eq!(split["event_time"], "2026-01-02T14:32:00Z");
    assert_eq!(events[split_index + 1]["event_type"], "market.bar.v1");
    assert_eq!(
        events[split_index + 1]["event_time"],
        "2026-01-02T14:32:00Z"
    );

    // The exit leaves nothing, and its realized P&L is the one the ledger reports.
    let exit = &positions[2]["payload"];
    assert_eq!(decimal_of(&exit["quantity"]), Decimal::ZERO);
    assert_eq!(decimal_of(&exit["realized_pnl"]), amount("-0.20"));
}

#[test]
fn the_engine_and_the_ledger_agree_on_the_position_after_the_split() {
    // Hold the position through the end so both books still carry it: the strategy sells
    // one, not two, so one post-split share remains.
    let completed = run(two_for_one(), 1).unwrap();
    let events = events(&completed);
    let engine_position = &positions(&events).last().unwrap()["payload"];
    let ledger_position = &completed.artifact.report.positions[0];

    assert_eq!(
        decimal_of(&engine_position["quantity"]),
        ledger_position.quantity
    );
    assert_eq!(decimal_of(&engine_position["quantity"]), amount("1"));
    assert_eq!(
        decimal_of(&engine_position["average_cost"]),
        ledger_position.average_cost
    );
    assert_eq!(
        decimal_of(&engine_position["average_cost"]),
        amount("50.05")
    );
    assert_eq!(
        decimal_of(&engine_position["realized_pnl"]),
        ledger_position.realized_pnl
    );
}

#[test]
fn a_run_without_a_split_is_unchanged_by_the_engine_carrying_no_action() {
    let completed = run(Vec::new(), 1).unwrap();
    let events = events(&completed);
    // The entry and the exit, and nothing at any split, because there was none.
    assert_eq!(positions(&events).len(), 2);
    assert!(events
        .iter()
        .all(|event| event["source"] != "corporate_action"));
    assert!(completed.applied_corporate_action_ids.is_empty());
}

#[test]
fn a_split_of_an_instrument_the_account_does_not_hold_records_no_position() {
    // The split takes effect before the entry fills, so there is nothing to scale.
    let completed = run(vec![split_at("2026-01-02T14:30:00Z", "2")], 1).unwrap();
    let events = events(&completed);
    assert_eq!(
        completed.applied_corporate_action_ids,
        vec![SPLIT_ID.to_owned()]
    );
    assert!(events
        .iter()
        .all(|event| event["source"] != "corporate_action"));
    assert_eq!(positions(&events).len(), 2);
}
