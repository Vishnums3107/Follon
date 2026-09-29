//! The backtest crate's two P&L conventions, stated and held (delivery state E8.5).
//!
//! * `BacktestLedger`, the primary account of every replay, puts fees in the
//!   cost basis. A buy fee raises the position's average cost, so realized and
//!   unrealized P&L are both **net of fees**, and a sell fee is deducted from the
//!   realized P&L. `total_fees` is reported beside them, informationally.
//! * `AdvancedBacktestAccount` reports **trading P&L before charges**. The
//!   average price excludes fees, and `execution_charges` carries them separately.
//!
//! Neither is wrong, and the two must not be compared without knowing which
//! is which. What must hold is that they agree on everything that is money:
//! the same fills leave the same cash, and the difference between the two
//! realized figures is exactly the fees, no more and no less.

use std::collections::BTreeMap;

use follon_accounting::{Currency, FxBook, MarginPolicy, MarginRate};
use follon_backtest::{
    AdvancedBacktestAccount, AdvancedBacktestReport, AdvancedInstrumentTerms,
    BacktestExecutionCharges, BacktestLedger, BacktestReport,
};
use follon_domain::{Decimal, Fill, Side};
use std::str::FromStr;

const INSTRUMENT: &str = "inst.us_equity.spy";

fn amount(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

fn fill(id: &str, side: Side, price: &str, fee: &str) -> Fill {
    Fill {
        execution_id: format!("execution.{id}"),
        order_id: format!("order.{id}"),
        instrument_id: INSTRUMENT.to_owned(),
        side,
        quantity: amount("10"),
        price: amount(price),
        fee: amount(fee),
        executed_at: "2026-01-02T14:30:00Z".to_owned(),
    }
}

/// Splits a composite fee into the commission, exchange and regulatory parts
/// the advanced account demands, so that they add up to exactly the fee.
fn charges(commission: &str, exchange: &str, regulatory: &str) -> BacktestExecutionCharges {
    BacktestExecutionCharges {
        commission: amount(commission),
        exchange: amount(exchange),
        regulatory: amount(regulatory),
    }
}

fn terms() -> AdvancedInstrumentTerms {
    AdvancedInstrumentTerms {
        currency: usd(),
        asset_class: "equity".to_owned(),
        multiplier: amount("1"),
        shortable: false,
        borrow_available: Decimal::ZERO,
        borrow_rate_bps: 0,
    }
}

fn usd() -> Currency {
    Currency::new("USD").unwrap()
}

/// What the same fills look like through each account.
struct Runs {
    ledger: BacktestReport,
    /// The ledger's FIFO tax-lot P&L, which includes fees in the lot cost.
    ledger_tax_pnl: Decimal,
    advanced: AdvancedBacktestReport,
    advanced_cash: Decimal,
    /// The advanced account's FIFO tax-lot P&L, which also includes fees.
    advanced_tax_pnl: Decimal,
}

/// Runs the same fills through both accounts and reports each at `mark`.
fn run_both(fills: &[(Fill, BacktestExecutionCharges)], mark: &str) -> Runs {
    let marks = BTreeMap::from([(INSTRUMENT.to_owned(), amount(mark))]);
    let mut ledger = BacktestLedger::new("USD", amount("10000")).unwrap();
    let mut advanced =
        AdvancedBacktestAccount::new(BTreeMap::from([(usd(), amount("10000"))])).unwrap();
    for (fill, parts) in fills {
        ledger.apply_fill(fill).unwrap();
        advanced.apply_fill(fill, &terms(), *parts).unwrap();
    }
    let policy = MarginPolicy {
        base_currency: usd(),
        maximum_fx_age_seconds: 0,
        rates: BTreeMap::from([(
            "equity".to_owned(),
            MarginRate {
                initial_bps: 10_000,
                maintenance_bps: 10_000,
            },
        )]),
    };
    Runs {
        ledger: ledger.report(&marks).unwrap(),
        ledger_tax_pnl: ledger.realized_tax_pnl().unwrap(),
        advanced: advanced
            .report(&marks, &FxBook::default(), &policy, 0)
            .unwrap(),
        advanced_cash: advanced.cash_by_currency()[&usd()],
        advanced_tax_pnl: advanced.tax_realized_pnl(&usd()),
    }
}

/// Buy 10 at 100 with a fee of 1.5 and sell 10 at 110 with a fee of 2: a
/// trading gain of 100 and fees of 3.5.
fn round_trip() -> Vec<(Fill, BacktestExecutionCharges)> {
    vec![
        (
            fill("buy", Side::Buy, "100", "1.5"),
            charges("1", "0.5", "0"),
        ),
        (
            fill("sell", Side::Sell, "110", "2"),
            charges("1.5", "0.25", "0.25"),
        ),
    ]
}

#[test]
fn the_primary_ledger_reports_realized_pnl_net_of_every_fee() {
    let runs = run_both(&round_trip(), "110");
    // (110 - 100.15) * 10 = 98.5, where 100.15 is the buy price plus the buy
    // fee per unit, less the sell fee of 2.
    assert_eq!(runs.ledger.realized_pnl, amount("96.5"));
    assert_eq!(runs.ledger.total_fees, amount("3.5"));
}

#[test]
fn the_advanced_account_reports_trading_pnl_before_charges() {
    let runs = run_both(&round_trip(), "110");
    assert_eq!(runs.advanced.realized_pnl, amount("100"));
    assert_eq!(runs.advanced.execution_charges, amount("3.5"));
}

/// The two figures differ by exactly the fees, and the money agrees.
#[test]
fn the_two_conventions_differ_by_exactly_the_fees_and_leave_the_same_cash() {
    let runs = run_both(&round_trip(), "110");
    assert_eq!(
        runs.ledger.realized_pnl,
        runs.advanced
            .realized_pnl
            .checked_sub(runs.advanced.execution_charges)
            .unwrap()
    );
    assert_eq!(runs.ledger.total_fees, runs.advanced.execution_charges);
    assert_eq!(runs.ledger.cash, runs.advanced_cash);
    assert_eq!(runs.ledger.cash, amount("10096.5"));
}

/// Both accounts also keep a FIFO tax-lot book, and there fees are part of the
/// lot cost in each. So the tax P&L is one figure across the two accounts, and
/// it equals the primary ledger's realized P&L here and the advanced account's
/// trading P&L less its charges.
#[test]
fn both_accounts_agree_on_fifo_tax_pnl_because_each_puts_fees_in_the_lot_cost() {
    let runs = run_both(&round_trip(), "110");
    assert_eq!(runs.ledger_tax_pnl, amount("96.5"));
    assert_eq!(runs.advanced_tax_pnl, runs.ledger_tax_pnl);
    assert_eq!(
        runs.advanced_tax_pnl,
        runs.advanced
            .realized_pnl
            .checked_sub(runs.advanced.execution_charges)
            .unwrap()
    );
}

/// An open position shows the same split: the primary ledger's unrealized P&L
/// carries the buy fee in its cost basis, the advanced account's does not.
#[test]
fn an_open_position_shows_the_same_split_in_unrealized_pnl() {
    let opening = vec![(
        fill("buy", Side::Buy, "100", "1.5"),
        charges("1", "0.5", "0"),
    )];
    let runs = run_both(&opening, "105");
    // (105 - 100.15) * 10 against (105 - 100) * 10.
    assert_eq!(runs.ledger.unrealized_pnl, amount("48.5"));
    assert_eq!(runs.advanced.unrealized_pnl, amount("50"));
    assert_eq!(
        runs.ledger.unrealized_pnl,
        runs.advanced
            .unrealized_pnl
            .checked_sub(runs.advanced.execution_charges)
            .unwrap()
    );
    assert_eq!(runs.ledger.cash, runs.advanced_cash);
    // Equity is money, so the conventions agree on it: cash plus the position
    // at its mark, whichever way the fee is shown.
    assert_eq!(runs.ledger.total_equity, amount("10048.5"));
    assert_eq!(
        runs.advanced.margin.net_liquidation_value,
        runs.ledger.total_equity
    );
}
