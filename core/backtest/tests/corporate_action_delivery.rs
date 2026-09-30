//! A strategy is told what a corporate action did to its account (delivery state E8.3).
//!
//! The portfolio snapshot a strategy worker is handed is built by the host for every
//! callback, and it followed fills only. A split or a dividend left it stale until the next
//! fill, so a worker that sized its exit from the snapshot sold a quantity the account no
//! longer held, and reported cash the ledger did not have. The runner now delivers each
//! effect through `Strategy::on_corporate_action`, and the worker's host keeps its
//! snapshot true to the ledger. No worker protocol changes: the snapshot was always the
//! host's to build.

mod common;

use std::ffi::OsString;
use std::path::PathBuf;

use common::{amount, dividend, run_with, split, RoundTrip, ACCOUNT, INSTRUMENT};
use follon_backtest::BacktestError;
use follon_control_plane::{
    CorporateActionEffect, EngineError, ProcessStrategyWorker, Strategy, StrategyWorkerIdentity,
    StrategyWorkerServicesConfig,
};
use follon_domain::{Bar, Decimal, OrderIntent, PositionSnapshot};

const SPLIT_ID: &str = "action-split-001";
const DIVIDEND_ID: &str = "action-dividend-001";
/// Between the second bar, where the entry fills, and the third.
const AFTER_ENTRY: &str = "2026-01-02T14:31:30Z";

fn position(quantity: &str, average_cost: &str) -> PositionSnapshot {
    PositionSnapshot {
        account_id: ACCOUNT.to_owned(),
        instrument_id: INSTRUMENT.to_owned(),
        quantity: amount(quantity),
        average_cost: amount(average_cost),
        realized_pnl: Decimal::ZERO,
    }
}

fn split_effect(ratio: &str, position: PositionSnapshot) -> CorporateActionEffect {
    CorporateActionEffect::Split {
        action_id: SPLIT_ID.to_owned(),
        ratio: amount(ratio),
        position,
    }
}

fn dividend_effect(cash_credited: &str) -> CorporateActionEffect {
    CorporateActionEffect::CashDividend {
        action_id: DIVIDEND_ID.to_owned(),
        account_id: ACCOUNT.to_owned(),
        instrument_id: INSTRUMENT.to_owned(),
        cash_credited: amount(cash_credited),
    }
}

#[test]
fn a_strategy_is_told_what_a_split_did_to_its_position() {
    let mut strategy = RoundTrip::new(1);
    run_with(&mut strategy, vec![split(SPLIT_ID, AFTER_ENTRY, "2")]).unwrap();

    // One share bought at 100.10 with its fee became two at 50.05.
    assert_eq!(
        strategy.effects,
        vec![split_effect("2", position("2", "50.05"))]
    );
}

#[test]
fn a_strategy_is_told_the_cash_a_dividend_credited_on_the_shares_the_ledger_held() {
    let mut strategy = RoundTrip::new(1);
    // The split lands first, so the dividend is paid on the two shares now held, not the one
    // that was bought. What the strategy is told is what the ledger booked.
    run_with(
        &mut strategy,
        vec![
            split(SPLIT_ID, AFTER_ENTRY, "2"),
            dividend(DIVIDEND_ID, "2026-01-02T14:31:45Z", "0.25"),
        ],
    )
    .unwrap();

    assert_eq!(
        strategy.effects,
        vec![
            split_effect("2", position("2", "50.05")),
            dividend_effect("0.50"),
        ]
    );
}

#[test]
fn a_strategy_is_not_told_of_an_action_that_changed_nothing() {
    let mut strategy = RoundTrip::new(1);
    // Both take effect on the first bar, before the entry fills, so the account holds
    // nothing for either to change.
    let completed = run_with(
        &mut strategy,
        vec![
            split(SPLIT_ID, "2026-01-02T14:30:00Z", "2"),
            dividend(DIVIDEND_ID, "2026-01-02T14:30:00Z", "0.25"),
        ],
    )
    .unwrap();

    assert_eq!(completed.applied_corporate_action_ids.len(), 2);
    assert!(strategy.effects.is_empty());
}

/// Buys one share on its first bar and never sells, so a fractional position can survive.
struct BuysOnce {
    bought: bool,
    effects: Vec<CorporateActionEffect>,
}

impl Strategy for BuysOnce {
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError> {
        if self.bought {
            return Ok(None);
        }
        self.bought = true;
        RoundTrip::new(1).on_bar(bar, replay_time)
    }

    fn on_corporate_action(&mut self, effect: &CorporateActionEffect) -> Result<(), EngineError> {
        self.effects.push(effect.clone());
        Ok(())
    }
}

#[test]
fn a_dividend_that_rounds_to_no_cash_is_not_an_effect() {
    let mut strategy = BuysOnce {
        bought: false,
        effects: Vec::new(),
    };
    // A 1:2 reverse split leaves half a share, and a dividend of one unit of the smallest
    // amount on half a share is half a unit of cash, which the ledger books as nothing.
    // There is no credit to tell the strategy of, and the run does not stop over it.
    let completed = run_with(
        &mut strategy,
        vec![
            split(SPLIT_ID, AFTER_ENTRY, "0.5"),
            dividend(DIVIDEND_ID, "2026-01-02T14:31:45Z", "0.00000001"),
        ],
    )
    .unwrap();

    assert_eq!(completed.applied_corporate_action_ids.len(), 2);
    assert_eq!(
        strategy.effects,
        vec![split_effect("0.5", position("0.5", "200.20"))]
    );
}

/// Trades like `RoundTrip`, and refuses every corporate action it is told of.
struct Refusing(RoundTrip);

impl Strategy for Refusing {
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError> {
        self.0.on_bar(bar, replay_time)
    }

    fn on_corporate_action(&mut self, _effect: &CorporateActionEffect) -> Result<(), EngineError> {
        Err(EngineError("the strategy refuses the action".to_owned()))
    }
}

#[test]
fn a_strategy_that_refuses_an_effect_stops_the_run() {
    let error: BacktestError = run_with(
        &mut Refusing(RoundTrip::new(1)),
        vec![split(SPLIT_ID, AFTER_ENTRY, "2")],
    )
    .expect_err("the strategy's error is the run's");
    assert_eq!(error.0, "the strategy refuses the action");
}

// A real worker process.

/// An absolute interpreter path, because the worker receives no PATH.
fn python_executable() -> Option<PathBuf> {
    ["python", "python3"].into_iter().find_map(|candidate| {
        std::process::Command::new(candidate)
            .args([
                "-c",
                "import pathlib,sys;print(pathlib.Path(sys.executable).resolve())",
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|output| PathBuf::from(output.trim()))
            .filter(|path| path.is_absolute() && path.is_file())
    })
}

/// The fixture worker that sizes its exit from the portfolio snapshot it is handed, or
/// `None` when Python is unavailable so the caller can skip.
fn following_worker() -> Option<ProcessStrategyWorker> {
    let Some(python) = python_executable() else {
        eprintln!("Python is unavailable; the worker fixture was skipped");
        return None;
    };
    let identity = StrategyWorkerIdentity {
        account_id: ACCOUNT.to_owned(),
        strategy_id: "strategy-following-001".to_owned(),
        strategy_version: "v1".to_owned(),
        configuration_version: "cfg-v1".to_owned(),
        strategy_bundle_hash: "a".repeat(64),
        environment: "SIMULATION".to_owned(),
    };
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/worker/portfolio-following-worker.py")
        .canonicalize()
        .expect("fixture path");
    let arguments: Vec<OsString> = vec![
        fixture.into_os_string(),
        identity.strategy_bundle_hash.clone().into(),
        identity.strategy_id.clone().into(),
        identity.strategy_version.clone().into(),
    ];
    Some(
        ProcessStrategyWorker::spawn_with_services(
            python.as_os_str(),
            arguments,
            identity,
            StrategyWorkerServicesConfig {
                currency: "USD".to_owned(),
                initial_cash: amount("1000"),
            },
        )
        .expect("worker starts"),
    )
}

/// What the worker last reported it was shown, by metric name.
fn shown(worker: &ProcessStrategyWorker, name: &str) -> Decimal {
    worker
        .latest_metrics()
        .expect("services are enabled")
        .iter()
        .find(|metric| metric.name == name)
        .unwrap_or_else(|| panic!("the worker reported no {name}"))
        .value
}

#[test]
fn a_worker_that_sizes_its_exit_from_its_portfolio_sells_the_post_split_quantity() {
    let Some(mut worker) = following_worker() else {
        return;
    };
    // One share is bought before the 2:1 split. A worker still shown one share would sell
    // one and leave one; shown two, it sells both.
    let completed = run_with(&mut worker, vec![split(SPLIT_ID, AFTER_ENTRY, "2")]).unwrap();

    let report = &completed.artifact.report;
    assert!(report
        .positions
        .iter()
        .all(|position| position.quantity == Decimal::ZERO));
    assert_eq!(report.cash, amount("999.80"));
    // What it saw on its last bar is what the ledger holds: nothing, and the same cash.
    assert_eq!(shown(&worker, "portfolio.quantity"), Decimal::ZERO);
    assert_eq!(shown(&worker, "portfolio.cash"), report.cash);
}

#[test]
fn a_worker_is_shown_the_cash_a_dividend_credited() {
    let Some(mut worker) = following_worker() else {
        return;
    };
    let completed = run_with(
        &mut worker,
        vec![dividend(DIVIDEND_ID, AFTER_ENTRY, "0.50")],
    )
    .unwrap();

    // 1000 - 100.10 for the share, + 0.50 dividend, + 49.90 for its sale at 50.
    let report = &completed.artifact.report;
    assert_eq!(report.cash, amount("950.30"));
    assert_eq!(shown(&worker, "portfolio.cash"), report.cash);
    assert_eq!(shown(&worker, "portfolio.quantity"), Decimal::ZERO);
}

#[test]
fn a_worker_refuses_an_effect_it_cannot_apply() {
    let Some(mut worker) = following_worker() else {
        return;
    };
    let mut other_account = dividend_effect("0.50");
    if let CorporateActionEffect::CashDividend { account_id, .. } = &mut other_account {
        *account_id = "acct-other-001".to_owned();
    }
    for (name, effect, message) in [
        (
            "an effect for another account",
            other_account,
            "another account",
        ),
        (
            "a dividend that credits nothing",
            dividend_effect("0"),
            "must credit a positive amount",
        ),
        (
            "a split of a position the worker does not hold",
            split_effect("2", position("2", "50.05")),
            "holds no position to split",
        ),
        (
            "a split with no ratio",
            split_effect("0", position("2", "50.05")),
            "needs a positive ratio",
        ),
    ] {
        let error = worker
            .on_corporate_action(&effect)
            .expect_err(&format!("{name} was accepted"));
        assert!(error.0.contains(message), "{name}: {}", error.0);
    }
}
