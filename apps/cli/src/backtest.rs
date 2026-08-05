//! Local, non-live reproducible backtest demonstration.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_backtest::{
    BacktestInput, BacktestRunner, BacktestSpec, DatasetManifest, ExperimentRecord,
    FileExperimentStore,
};
use follon_control_plane::{
    import_historical_bars, BuyOnceStrategy, DeterministicFillModel, MarketPreconditions,
    ProcessStrategyWorker, ReplayEngine, RiskPolicy, StrategyWorkerIdentity,
};
use follon_domain::Decimal;
use follon_instrument::{
    AssetClass, Instrument, InstrumentRegistry, InstrumentVersion, StaticTradingCalendar,
    TradingSession,
};
use follon_market_data::import_corporate_actions;
use sha2::{Digest, Sha256};

const BUILTIN_STRATEGY_SOURCE: &str = include_str!("../../../core/control-plane/src/lib.rs");

enum StrategyMode {
    Builtin,
    Python(PythonWorkerArguments),
}

struct PythonWorkerArguments {
    program: String,
    strategy_file: String,
    class_name: String,
    bundle_root: String,
    strategy_id: String,
    strategy_version: String,
    bundle_hash: String,
}

impl PythonWorkerArguments {
    fn protocol_arguments(&self) -> Vec<OsString> {
        [
            "-m",
            "follon_strategy_sdk.worker",
            "--strategy-file",
            &self.strategy_file,
            "--class-name",
            &self.class_name,
            "--bundle-root",
            &self.bundle_root,
            "--strategy-id",
            &self.strategy_id,
            "--strategy-version",
            &self.strategy_version,
        ]
        .into_iter()
        .map(OsString::from)
        .collect()
    }
}

struct CommandArguments {
    input_path: PathBuf,
    artifact_path: PathBuf,
    action_path: Option<PathBuf>,
    strategy_mode: StrategyMode,
    experiment: Option<ExperimentArguments>,
}

struct ExperimentArguments {
    catalog_path: PathBuf,
    experiment_id: String,
    run_id: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(env::args().skip(1).collect())?;
    if let Some(parent) = arguments.artifact_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let bars = import_historical_bars(&fs::read_to_string(&arguments.input_path)?)?;
    let corporate_actions = match &arguments.action_path {
        Some(path) => import_corporate_actions(&fs::read_to_string(path)?)?,
        None => Vec::new(),
    };
    let dataset_bars: Vec<_> = bars
        .iter()
        .map(|bar| (bar.event_time.clone(), bar.bar.clone()))
        .collect();
    let dataset = DatasetManifest::from_market_data(
        "dataset.spy",
        "v1",
        "reference-example-1",
        "universe.spy",
        &dataset_bars,
        &corporate_actions,
    )?;
    let (strategy_bundle_hash, strategy_id, strategy_version) = match &arguments.strategy_mode {
        StrategyMode::Builtin => (
            format!("{:x}", Sha256::digest(BUILTIN_STRATEGY_SOURCE.as_bytes())),
            "strategy-example-001".to_owned(),
            "strategy-example-v1".to_owned(),
        ),
        StrategyMode::Python(worker) => (
            worker.bundle_hash.clone(),
            worker.strategy_id.clone(),
            worker.strategy_version.clone(),
        ),
    };
    let spec = BacktestSpec {
        strategy_bundle_hash: strategy_bundle_hash.clone(),
        dataset,
        configuration_version: "cfg-example-1".to_owned(),
        seed: 7,
        engine_version: "core-0.1.0".to_owned(),
        starts_at: dataset_bars
            .first()
            .ok_or("dataset contains no bars")?
            .0
            .clone(),
        ends_at: dataset_bars
            .last()
            .ok_or("dataset contains no bars")?
            .0
            .clone(),
    };
    let (instruments, calendar) = market_dependencies()?;
    let market = MarketPreconditions {
        instruments: &instruments,
        calendar: &calendar,
    };
    let mut runner = BacktestRunner::new(
        spec,
        ReplayEngine::new(
            "2026-01-02T14:30:00Z",
            "core-0.1.0",
            "cfg-example-1",
            RiskPolicy {
                version: "risk-example-1".to_owned(),
                global_kill_switch: false,
                max_quantity: decimal("10")?,
                max_notional: decimal("10000")?,
            },
            DeterministicFillModel {
                slippage_bps: Decimal::ZERO,
                flat_fee: decimal("0.10")?,
            },
        ),
    )?;
    let input = BacktestInput {
        account_id: "acct-paper-001".to_owned(),
        currency: "USD".to_owned(),
        initial_cash: decimal("100000")?,
        bars,
        corporate_actions,
    };
    let completed = match arguments.strategy_mode {
        StrategyMode::Builtin => {
            let mut strategy = BuyOnceStrategy::new(
                "acct-paper-001",
                strategy_id,
                strategy_version,
                "cfg-example-1",
                decimal("100")?,
            );
            runner.run(&mut strategy, &input, &market)?
        }
        StrategyMode::Python(worker) => {
            let identity = StrategyWorkerIdentity {
                account_id: "acct-paper-001".to_owned(),
                strategy_id,
                strategy_version,
                configuration_version: "cfg-example-1".to_owned(),
                strategy_bundle_hash,
                environment: "SIMULATION".to_owned(),
            };
            let mut strategy = ProcessStrategyWorker::spawn(
                &worker.program,
                worker.protocol_arguments(),
                identity,
            )?;
            runner.run(&mut strategy, &input, &market)?
        }
    };
    let event_path = arguments.artifact_path.with_extension("events.ndjson");
    let report_path = arguments.artifact_path.with_extension("report.md");
    write_immutable(
        &arguments.artifact_path,
        &completed.artifact.canonical_json(),
    )?;
    write_immutable(&event_path, &(completed.canonical_events.join("\n") + "\n"))?;
    write_immutable(&report_path, &completed.artifact.markdown_report())?;
    if let Some(experiment) = arguments.experiment {
        let record = ExperimentRecord::from_artifact(
            experiment.experiment_id,
            experiment.run_id,
            BTreeMap::new(),
            &completed.artifact,
        )?;
        let mut store = FileExperimentStore::open(experiment.catalog_path)?;
        store.record(record)?;
    }
    eprintln!("artifact: {}", arguments.artifact_path.display());
    eprintln!("event stream: {}", event_path.display());
    eprintln!("report: {}", report_path.display());
    eprintln!("artifact fingerprint: {}", completed.artifact.fingerprint());
    Ok(())
}

fn parse_arguments(arguments: Vec<String>) -> Result<CommandArguments, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut action_path = None;
    let mut strategy_mode = StrategyMode::Builtin;
    let mut experiment = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--actions" => {
                index += 1;
                if action_path
                    .replace(PathBuf::from(required_argument(
                        &arguments,
                        index,
                        "--actions",
                    )?))
                    .is_some()
                {
                    return Err("--actions may be specified only once".into());
                }
            }
            "--python-worker" => {
                if !matches!(&strategy_mode, StrategyMode::Builtin) {
                    return Err("only one strategy execution mode may be selected".into());
                }
                let worker = PythonWorkerArguments {
                    program: required_argument(&arguments, index + 1, "--python-worker")?
                        .to_owned(),
                    strategy_file: required_argument(&arguments, index + 2, "--python-worker")?
                        .to_owned(),
                    class_name: required_argument(&arguments, index + 3, "--python-worker")?
                        .to_owned(),
                    bundle_root: required_argument(&arguments, index + 4, "--python-worker")?
                        .to_owned(),
                    strategy_id: required_argument(&arguments, index + 5, "--python-worker")?
                        .to_owned(),
                    strategy_version: required_argument(&arguments, index + 6, "--python-worker")?
                        .to_owned(),
                    bundle_hash: required_argument(&arguments, index + 7, "--python-worker")?
                        .to_owned(),
                };
                strategy_mode = StrategyMode::Python(worker);
                index += 7;
            }
            "--experiment" => {
                if experiment.is_some() {
                    return Err("--experiment may be specified only once".into());
                }
                experiment = Some(ExperimentArguments {
                    catalog_path: PathBuf::from(required_argument(
                        &arguments,
                        index + 1,
                        "--experiment",
                    )?),
                    experiment_id: required_argument(&arguments, index + 2, "--experiment")?
                        .to_owned(),
                    run_id: required_argument(&arguments, index + 3, "--experiment")?.to_owned(),
                });
                index += 3;
            }
            value if value.starts_with("--") => {
                return Err(format!("unsupported argument: {value}").into())
            }
            value => positional.push(PathBuf::from(value)),
        }
        index += 1;
    }
    if positional.len() > 2 {
        return Err("usage: follon-backtest [bars.csv] [artifact.json] [options]".into());
    }
    Ok(CommandArguments {
        input_path: positional
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("tests/fixtures/historical-bars/spy-one-minute.csv")),
        artifact_path: positional
            .get(1)
            .cloned()
            .unwrap_or_else(|| PathBuf::from("var/follon-backtest-artifact.json")),
        action_path,
        strategy_mode,
        experiment,
    })
}

fn required_argument<'a>(
    arguments: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{flag} requires additional values").into())
}

fn write_immutable(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(contents.as_bytes())?;
            file.sync_data()?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if fs::read_to_string(path)? == contents {
                Ok(())
            } else {
                Err(format!(
                    "refusing to overwrite immutable artifact: {}",
                    path.display()
                )
                .into())
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn decimal(value: &str) -> Result<Decimal, follon_domain::DecimalError> {
    Decimal::from_str(value)
}

fn market_dependencies(
) -> Result<(InstrumentRegistry, StaticTradingCalendar), Box<dyn std::error::Error>> {
    let calendar = StaticTradingCalendar::new(
        "cal.us_equities.nyse",
        vec![TradingSession {
            exchange_date: "2026-01-02".to_owned(),
            opens_at: "2026-01-02T14:30:00Z".to_owned(),
            closes_at: "2026-01-02T21:00:00Z".to_owned(),
        }],
    )?;
    let mut instruments = InstrumentRegistry::default();
    instruments.register(InstrumentVersion {
        instrument: Instrument {
            instrument_id: "inst.us_equity.spy".to_owned(),
            symbol: "SPY".to_owned(),
            exchange_symbol: "SPY".to_owned(),
            asset_class: AssetClass::Etf,
            venue: "venue.nyse_arca".to_owned(),
            currency: "USD".to_owned(),
            broker_ids: BTreeMap::new(),
            tick_size: decimal("0.01")?,
            lot_size: decimal("1")?,
            multiplier: decimal("1")?,
            trading_calendar_id: "cal.us_equities.nyse".to_owned(),
        },
        effective_from: "2026-01-01T00:00:00Z".to_owned(),
        effective_to: None,
        reference_version: "reference-example-1".to_owned(),
    })?;
    Ok((instruments, calendar))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_a_versioned_python_worker_and_experiment_target() {
        let parsed = parse_arguments(vec![
            "bars.csv".to_owned(),
            "artifact.json".to_owned(),
            "--python-worker".to_owned(),
            "python".to_owned(),
            "strategy.py".to_owned(),
            "ExampleStrategy".to_owned(),
            "bundle".to_owned(),
            "strategy-example-001".to_owned(),
            "v1".to_owned(),
            "a".repeat(64),
            "--experiment".to_owned(),
            "experiments.ndjson".to_owned(),
            "experiment-001".to_owned(),
            "run-001".to_owned(),
        ])
        .unwrap();
        assert_eq!(parsed.input_path, PathBuf::from("bars.csv"));
        assert!(parsed.experiment.is_some());
        assert!(matches!(parsed.strategy_mode, StrategyMode::Python(_)));
    }

    #[test]
    fn immutable_writer_is_idempotent_and_rejects_conflicts() {
        let path = std::env::temp_dir().join(format!(
            "follon-immutable-artifact-{}-{}.json",
            std::process::id(),
            "writer"
        ));
        let _ = std::fs::remove_file(&path);
        write_immutable(&path, "first").unwrap();
        write_immutable(&path, "first").unwrap();
        assert!(write_immutable(&path, "different").is_err());
        std::fs::remove_file(path).unwrap();
    }
}
