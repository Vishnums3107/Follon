//! A stock split changes how many shares an account holds and what one is worth, and leaves
//! what the account is worth alone (delivery state E8.6, found in review).
//!
//! The runner marks each position at the last bar of its instrument. A split multiplies the
//! position at once and the mark not at all, so until the instrument's next bar an equity
//! point counted the shares the split added at the price before it. That is a jump at the
//! split which nothing in the market caused, and when no bar follows, a wrong ending equity.
//! It shows whenever another instrument's bar is the one a split waits on, because the split
//! is applied at the first bar at or after its time, whichever instrument that belongs to.
//!
//! The ledger's lots had a related fault: scaled one by one, they could total less than the
//! position, which is scaled once, and a sale of the whole position was refused.

mod common;

use std::collections::BTreeMap;

use common::{amount, bar_of, run_input, split, BuyOnce, ACCOUNT, INSTRUMENT};
use follon_backtest::{BacktestInput, BacktestLedger, CompletedBacktest};
use follon_control_plane::HistoricalBar;
use follon_domain::{Decimal, Fill, Side};

const OTHER: &str = "inst.us_equity.aaa";
const INSTRUMENTS: [&str; 2] = [OTHER, INSTRUMENT];

/// An `OTHER` bar every minute from 14:30 to 14:34, at 10, and one of `INSTRUMENT` for each
/// price in `prices`, from 14:30. `OTHER` comes first within a minute, so a split waiting on
/// that minute is applied while `OTHER`'s bar is processed and before `INSTRUMENT`'s.
fn two_instrument_input(
    prices: &[&str],
    actions: Vec<follon_market_data::CorporateAction>,
) -> BacktestInput {
    let mut bars = Vec::new();
    for minute in 0..5 {
        let event_time = format!("2026-01-02T14:{:02}:00Z", 30 + minute);
        bars.push(HistoricalBar {
            event_time: event_time.clone(),
            bar: bar_of(OTHER, "10"),
        });
        if let Some(price) = prices.get(minute) {
            bars.push(HistoricalBar {
                event_time,
                bar: bar_of(INSTRUMENT, price),
            });
        }
    }
    BacktestInput {
        account_id: ACCOUNT.to_owned(),
        currency: "USD".to_owned(),
        initial_cash: amount("1000"),
        bars,
        corporate_actions: actions,
    }
}

fn run(prices: &[&str], ratio: &str, effective_at: &str) -> CompletedBacktest {
    let input = two_instrument_input(prices, vec![split("action-split-001", effective_at, ratio)]);
    run_input(&mut BuyOnce::new(INSTRUMENT), &input, &INSTRUMENTS).expect("the replay completes")
}

fn equity_at(completed: &CompletedBacktest, index: usize) -> Decimal {
    completed.artifact.performance.equity_curve[index].total_equity
}

/// Bars, in the order they are processed: OTHER and INSTRUMENT at 14:30, at 14:31 and so on.
/// The share is bought on the 14:30 bar and fills on the next one of its own instrument,
/// index 3, leaving 899.90 in cash and one share marked at 100. The 2:1 split waits for
/// 14:32 and is applied while the OTHER bar at index 4 is processed.
#[test]
fn a_split_does_not_move_equity_when_another_instruments_bar_is_the_one_it_waits_on() {
    let completed = run(
        &["100", "100", "50", "50", "50"],
        "2",
        "2026-01-02T14:32:00Z",
    );
    assert_eq!(equity_at(&completed, 3), amount("999.90"));
    // Before E8.6 this was 1,099.90: two shares at the mark of one.
    assert_eq!(equity_at(&completed, 4), amount("999.90"));
    // The instrument's own bar at the new price then agrees with it.
    assert_eq!(equity_at(&completed, 5), amount("999.90"));
    // Only the 0.10 fee ever moved equity, so the largest fall from a peak is one basis
    // point of 1,000. With the jump, the fall back from 1,099.90 was over nine per cent.
    assert_eq!(completed.artifact.performance.max_drawdown_bps, amount("1"));
}

/// The same, the other way: a reverse split halves the shares and doubles the mark.
#[test]
fn a_reverse_split_does_not_move_equity_either() {
    let completed = run(
        &["100", "100", "200", "200", "200"],
        "0.5",
        "2026-01-02T14:32:00Z",
    );
    assert_eq!(equity_at(&completed, 3), amount("999.90"));
    // Before E8.6 this was 949.90: half a share at the mark of one.
    assert_eq!(equity_at(&completed, 4), amount("999.90"));
}

/// When no bar of the instrument follows the split, the ending equity is the last point,
/// so it was wrong for good and not only for a minute.
#[test]
fn the_ending_equity_is_right_when_the_split_instrument_prints_no_later_bar() {
    let completed = run(&["100", "100"], "2", "2026-01-02T14:33:00Z");
    let report = &completed.artifact.report;
    assert_eq!(report.total_equity, amount("999.90"));
    assert_eq!(
        completed.artifact.performance.ending_equity,
        amount("999.90")
    );
    // Two shares at a mark of 50, against the 100.10 they cost including the fee.
    assert_eq!(report.market_value, amount("100"));
    assert_eq!(report.unrealized_pnl, amount("-0.10"));
}

/// A split revalues the instrument it splits and no other. The account holds the share of
/// `INSTRUMENT` and `OTHER` splits, which sorts first, so the split is applied while `OTHER`'s
/// bar is processed, before `INSTRUMENT`'s own bar of that minute could refresh its mark.
#[test]
fn a_split_revalues_only_the_instrument_it_splits() {
    let input = two_instrument_input(
        &["100", "100", "100", "100", "100"],
        vec![follon_market_data::CorporateAction::Split {
            action_id: "action-split-001".to_owned(),
            instrument_id: OTHER.to_owned(),
            effective_at: "2026-01-02T14:32:00Z".to_owned(),
            ratio: amount("2"),
        }],
    );
    let completed = run_input(&mut BuyOnce::new(INSTRUMENT), &input, &INSTRUMENTS).unwrap();
    // One share held at 100, with nothing of `OTHER` to revalue: every point from the fill on is
    // the same 999.90, the one at the split included.
    let curve = &completed.artifact.performance.equity_curve;
    assert_eq!(curve[4].event_time, "2026-01-02T14:32:00Z");
    assert!(curve
        .iter()
        .skip(3)
        .all(|point| point.total_equity == amount("999.90")));
}

fn ledger_fill(number: usize, side: Side, quantity: &str) -> Fill {
    Fill {
        execution_id: format!("exec.{number:06}"),
        order_id: format!("order.{number:06}"),
        instrument_id: INSTRUMENT.to_owned(),
        side,
        quantity: amount(quantity),
        price: amount("100"),
        fee: Decimal::ZERO,
        executed_at: "2026-01-02T14:30:00Z".to_owned(),
    }
}

fn held(ledger: &BacktestLedger) -> Decimal {
    let marks = BTreeMap::from([(INSTRUMENT.to_owned(), amount("100"))]);
    ledger.report(&marks).unwrap().positions[0].quantity
}

/// Two lots of one, split by a third and then by one and a half, scaled to 0.49999999 each,
/// so the lots totalled 0.99999998 where the position, scaled once, was 0.99999999. A sale
/// of the whole position then exceeded the lots and was refused.
#[test]
fn the_whole_position_can_be_sold_after_splits_that_do_not_scale_exactly() {
    let mut ledger = BacktestLedger::new("USD", amount("1000000")).unwrap();
    ledger.apply_fill(&ledger_fill(1, Side::Buy, "1")).unwrap();
    ledger.apply_fill(&ledger_fill(2, Side::Buy, "1")).unwrap();
    for (number, ratio) in [(1, "0.33333333"), (2, "1.5")] {
        ledger
            .apply_corporate_action(&split(
                &format!("action-split-{number}"),
                "2026-01-02T14:31:00Z",
                ratio,
            ))
            .unwrap();
    }
    let position = held(&ledger);
    assert_eq!(position, amount("0.99999999"));
    let lots = ledger
        .tax_lots(INSTRUMENT)
        .iter()
        .fold(Decimal::ZERO, |total, lot| {
            total.checked_add(lot.remaining_quantity).unwrap()
        });
    assert_eq!(lots, position);

    ledger
        .apply_fill(&ledger_fill(3, Side::Sell, "0.99999999"))
        .expect("the lots hold what the position does");
    assert_eq!(held(&ledger), Decimal::ZERO);
    assert!(ledger.tax_lots(INSTRUMENT).is_empty());
}
