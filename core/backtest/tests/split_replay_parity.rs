//! A position held across a stock split is the same position in the replay engine and in
//! the ledger (delivery state E8.2).
//!
//! `BacktestLedger` applied a split to its position and its FIFO lots (E8.1). The replay
//! engine keeps a portfolio of its own, and it is that portfolio the strategy's execution
//! callbacks and the fingerprinted event stream project. It did not follow the split, so
//! a strategy that held one share across a 2:1 split and then sold the two it now held
//! was refused: the engine still believed it held one, and the whole backtest aborted.

mod common;

use common::{
    amount, decimal_of, events, positions, run_with, split, RoundTrip, ACCOUNT, INSTRUMENT,
};
use follon_backtest::{BacktestError, CompletedBacktest};
use follon_domain::Decimal;
use follon_market_data::CorporateAction;

const SPLIT_ID: &str = "action-split-001";

fn split_at(effective_at: &str, ratio: &str) -> CorporateAction {
    split(SPLIT_ID, effective_at, ratio)
}

/// A 2:1 split between the second bar and the third.
fn two_for_one() -> Vec<CorporateAction> {
    vec![split_at("2026-01-02T14:31:30Z", "2")]
}

fn run(
    actions: Vec<CorporateAction>,
    sell_quantity: i64,
) -> Result<CompletedBacktest, BacktestError> {
    run_with(&mut RoundTrip::new(sell_quantity), actions)
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
