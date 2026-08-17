//! Local, non-live reproducible backtest demonstration.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_backtest::{
    BacktestInput, BacktestRunner, BacktestSpec, DatasetManifest, ExperimentRecord,
    FileExperimentStore,
};
use follon_cli::{sha256_text, write_immutable};
use follon_control_plane::{
    import_historical_bars, BuyOnceStrategy, DeterministicFillModel, MarketPreconditions,
    ProcessStrategyWorker, ReplayEngine, RiskPolicy, StrategyWorkerIdentity,
};
use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use follon_instrument::{
    AssetClass, Instrument, InstrumentRegistry, InstrumentVersion, StaticTradingCalendar,
    TradingHalt, TradingSession,
};
use follon_market_data::import_corporate_actions;
use serde::Deserialize;
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
    configuration_path: PathBuf,
    action_path: Option<PathBuf>,
    strategy_mode: StrategyMode,
    experiment: Option<ExperimentArguments>,
}

struct ExperimentArguments {
    catalog_path: PathBuf,
    experiment_id: String,
    run_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BacktestConfigurationDocument {
    schema_version: u32,
    configuration_id: String,
    configuration_version: String,
    engine_version: String,
    seed: u64,
    starts_at: String,
    ends_at: String,
    account: AccountDocument,
    dataset: DatasetDocument,
    strategy: StrategyDocument,
    risk: RiskDocument,
    execution: ExecutionDocument,
    calendar: CalendarDocument,
    instruments: Vec<InstrumentDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountDocument {
    account_id: String,
    currency: String,
    initial_cash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DatasetDocument {
    dataset_id: String,
    dataset_version: String,
    reference_data_version: String,
    universe_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyDocument {
    strategy_id: String,
    strategy_version: String,
    builtin_entry_threshold: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RiskDocument {
    policy_version: String,
    global_kill_switch: bool,
    max_quantity: String,
    max_notional: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionDocument {
    #[serde(default = "zero_decimal_string")]
    spread_bps: String,
    slippage_bps: String,
    flat_fee: String,
    #[serde(default)]
    latency_bars: u32,
    #[serde(default)]
    max_fill_quantity: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CalendarDocument {
    calendar_id: String,
    sessions: Vec<SessionDocument>,
    #[serde(default)]
    halts: Vec<HaltDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionDocument {
    exchange_date: String,
    opens_at: String,
    closes_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HaltDocument {
    halt_id: String,
    instrument_id: Option<String>,
    starts_at: String,
    ends_at: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstrumentDocument {
    instrument_id: String,
    symbol: String,
    exchange_symbol: String,
    asset_class: String,
    venue: String,
    currency: String,
    #[serde(default)]
    broker_ids: BTreeMap<String, String>,
    tick_size: String,
    lot_size: String,
    multiplier: String,
    trading_calendar_id: String,
    effective_from: String,
    effective_to: Option<String>,
    reference_version: String,
}

struct RuntimeConfiguration {
    document: BacktestConfigurationDocument,
    content_hash: String,
    initial_cash: Decimal,
    entry_threshold: Decimal,
    risk_policy: RiskPolicy,
    fill_model: DeterministicFillModel,
    instruments: InstrumentRegistry,
    calendar: StaticTradingCalendar,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(env::args().skip(1).collect())?;
    let configuration = load_runtime_configuration(&arguments.configuration_path)?;
    let document = &configuration.document;
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
        &document.dataset.dataset_id,
        &document.dataset.dataset_version,
        &document.dataset.reference_data_version,
        &document.dataset.universe_id,
        &dataset_bars,
        &corporate_actions,
    )?;
    let (strategy_bundle_hash, strategy_id, strategy_version) = match &arguments.strategy_mode {
        StrategyMode::Builtin => (
            format!("{:x}", Sha256::digest(BUILTIN_STRATEGY_SOURCE.as_bytes())),
            document.strategy.strategy_id.clone(),
            document.strategy.strategy_version.clone(),
        ),
        StrategyMode::Python(worker) => {
            if worker.strategy_id != document.strategy.strategy_id
                || worker.strategy_version != document.strategy.strategy_version
            {
                return Err(
                    "Python worker identity does not match the immutable configuration".into(),
                );
            }
            (
                worker.bundle_hash.clone(),
                worker.strategy_id.clone(),
                worker.strategy_version.clone(),
            )
        }
    };
    let spec = BacktestSpec {
        strategy_bundle_hash: strategy_bundle_hash.clone(),
        dataset,
        configuration_id: document.configuration_id.clone(),
        configuration_version: document.configuration_version.clone(),
        configuration_hash: configuration.content_hash.clone(),
        seed: document.seed,
        engine_version: document.engine_version.clone(),
        starts_at: document.starts_at.clone(),
        ends_at: document.ends_at.clone(),
    };
    let market = MarketPreconditions {
        instruments: &configuration.instruments,
        calendar: &configuration.calendar,
    };
    let mut runner = BacktestRunner::new(
        spec,
        ReplayEngine::new(
            &document.starts_at,
            &document.engine_version,
            &document.configuration_version,
            configuration.risk_policy.clone(),
            configuration.fill_model.clone(),
        )?,
    )?;
    let input = BacktestInput {
        account_id: document.account.account_id.clone(),
        currency: document.account.currency.clone(),
        initial_cash: configuration.initial_cash,
        bars,
        corporate_actions,
    };
    let completed = match arguments.strategy_mode {
        StrategyMode::Builtin => {
            let mut strategy = BuyOnceStrategy::new(
                &document.account.account_id,
                strategy_id,
                strategy_version,
                &document.configuration_version,
                configuration.entry_threshold,
            );
            runner.run(&mut strategy, &input, &market)?
        }
        StrategyMode::Python(worker) => {
            let identity = StrategyWorkerIdentity {
                account_id: document.account.account_id.clone(),
                strategy_id,
                strategy_version,
                configuration_version: document.configuration_version.clone(),
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
    let manifest_path = arguments.artifact_path.with_extension("manifest.json");
    let artifact_json = completed.artifact.canonical_json();
    let event_stream = completed.canonical_events.join("\n") + "\n";
    let report = completed.artifact.markdown_report();
    write_immutable(&arguments.artifact_path, &artifact_json)?;
    write_immutable(&event_path, &event_stream)?;
    write_immutable(&report_path, &report)?;
    let completion_manifest = format!(
        "{{\"artifact_fingerprint\":\"{}\",\"artifact_sha256\":\"{}\",\"configuration_hash\":\"{}\",\"event_output_hash\":\"{}\",\"events_sha256\":\"{}\",\"manifest_schema_version\":1,\"report_sha256\":\"{}\",\"specification_fingerprint\":\"{}\"}}",
        completed.artifact.fingerprint(),
        sha256_text(&artifact_json),
        configuration.content_hash,
        completed.artifact.event_output_hash,
        sha256_text(&event_stream),
        sha256_text(&report),
        completed.artifact.specification_fingerprint,
    );
    write_immutable(&manifest_path, &completion_manifest)?;
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
    eprintln!("completion manifest: {}", manifest_path.display());
    eprintln!("artifact fingerprint: {}", completed.artifact.fingerprint());
    eprintln!("configuration hash: {}", configuration.content_hash);
    Ok(())
}

fn parse_arguments(arguments: Vec<String>) -> Result<CommandArguments, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut action_path = None;
    let mut configuration_path = PathBuf::from("tests/fixtures/config/backtest-v1.json");
    let mut configuration_path_explicit = false;
    let mut strategy_mode = StrategyMode::Builtin;
    let mut experiment = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--config" => {
                index += 1;
                if configuration_path_explicit {
                    return Err("--config may be specified only once".into());
                }
                configuration_path =
                    PathBuf::from(required_argument(&arguments, index, "--config")?);
                configuration_path_explicit = true;
            }
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
        configuration_path,
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

fn decimal(value: &str) -> Result<Decimal, follon_domain::DecimalError> {
    Decimal::from_str(value)
}

fn zero_decimal_string() -> String {
    "0".to_owned()
}

fn load_runtime_configuration(
    path: &Path,
) -> Result<RuntimeConfiguration, Box<dyn std::error::Error>> {
    const MAX_CONFIGURATION_BYTES: usize = 1024 * 1024;
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.len() > MAX_CONFIGURATION_BYTES {
        return Err("configuration must be between 1 byte and 1 MiB".into());
    }
    let content_hash = format!("{:x}", Sha256::digest(&bytes));
    let document: BacktestConfigurationDocument = serde_json::from_slice(&bytes)?;
    if document.schema_version != 1 {
        return Err("unsupported backtest configuration schema version".into());
    }
    validate_canonical_id("configuration_id", &document.configuration_id)?;
    validate_canonical_id("account_id", &document.account.account_id)?;
    validate_canonical_id("strategy_id", &document.strategy.strategy_id)?;
    validate_canonical_id("dataset_id", &document.dataset.dataset_id)?;
    validate_canonical_id("universe_id", &document.dataset.universe_id)?;
    validate_utc_timestamp("backtest starts_at", &document.starts_at)?;
    validate_utc_timestamp("backtest ends_at", &document.ends_at)?;
    if document.configuration_version.is_empty()
        || document.engine_version.is_empty()
        || document.dataset.dataset_version.is_empty()
        || document.dataset.reference_data_version.is_empty()
        || document.strategy.strategy_version.is_empty()
        || document.starts_at > document.ends_at
    {
        return Err("configuration contains an invalid version or time range".into());
    }
    if document.account.currency.len() != 3
        || !document
            .account
            .currency
            .bytes()
            .all(|byte| byte.is_ascii_uppercase())
    {
        return Err("account currency must be a three-letter uppercase code".into());
    }
    let initial_cash = decimal(&document.account.initial_cash)?;
    let entry_threshold = decimal(&document.strategy.builtin_entry_threshold)?;
    if initial_cash < Decimal::ZERO || entry_threshold <= Decimal::ZERO {
        return Err("opening cash cannot be negative and entry threshold must be positive".into());
    }
    let risk_policy = RiskPolicy {
        version: document.risk.policy_version.clone(),
        global_kill_switch: document.risk.global_kill_switch,
        max_quantity: decimal(&document.risk.max_quantity)?,
        max_notional: decimal(&document.risk.max_notional)?,
    };
    risk_policy.validate()?;
    let fill_model = DeterministicFillModel {
        spread_bps: decimal(&document.execution.spread_bps)?,
        slippage_bps: decimal(&document.execution.slippage_bps)?,
        flat_fee: decimal(&document.execution.flat_fee)?,
        latency_bars: document.execution.latency_bars,
        max_fill_quantity: document
            .execution
            .max_fill_quantity
            .as_deref()
            .map(decimal)
            .transpose()?,
    };
    fill_model.validate()?;
    let sessions = document
        .calendar
        .sessions
        .iter()
        .map(|session| TradingSession {
            exchange_date: session.exchange_date.clone(),
            opens_at: session.opens_at.clone(),
            closes_at: session.closes_at.clone(),
        })
        .collect();
    let halts = document
        .calendar
        .halts
        .iter()
        .map(|halt| TradingHalt {
            halt_id: halt.halt_id.clone(),
            instrument_id: halt.instrument_id.clone(),
            starts_at: halt.starts_at.clone(),
            ends_at: halt.ends_at.clone(),
            reason: halt.reason.clone(),
        })
        .collect();
    let calendar = StaticTradingCalendar::new_with_halts(
        document.calendar.calendar_id.clone(),
        sessions,
        halts,
    )?;
    if document.instruments.is_empty() {
        return Err("configuration must define at least one instrument version".into());
    }
    let mut instruments = InstrumentRegistry::default();
    for instrument in &document.instruments {
        let asset_class = match instrument.asset_class.as_str() {
            "EQUITY" => AssetClass::Equity,
            "ETF" => AssetClass::Etf,
            _ => {
                return Err(
                    "only EQUITY and ETF instruments are enabled for this deployment boundary"
                        .into(),
                )
            }
        };
        if instrument.currency != document.account.currency
            || instrument.trading_calendar_id != document.calendar.calendar_id
            || instrument.reference_version != document.dataset.reference_data_version
        {
            return Err(
                "instrument currency, calendar, or reference version conflicts with configuration"
                    .into(),
            );
        }
        instruments.register(InstrumentVersion {
            instrument: Instrument {
                instrument_id: instrument.instrument_id.clone(),
                symbol: instrument.symbol.clone(),
                exchange_symbol: instrument.exchange_symbol.clone(),
                asset_class,
                venue: instrument.venue.clone(),
                currency: instrument.currency.clone(),
                broker_ids: instrument.broker_ids.clone(),
                tick_size: decimal(&instrument.tick_size)?,
                lot_size: decimal(&instrument.lot_size)?,
                multiplier: decimal(&instrument.multiplier)?,
                trading_calendar_id: instrument.trading_calendar_id.clone(),
            },
            effective_from: instrument.effective_from.clone(),
            effective_to: instrument.effective_to.clone(),
            reference_version: instrument.reference_version.clone(),
        })?;
    }
    Ok(RuntimeConfiguration {
        document,
        content_hash,
        initial_cash,
        entry_threshold,
        risk_policy,
        fill_model,
        instruments,
        calendar,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_a_versioned_python_worker_and_experiment_target() {
        let parsed = parse_arguments(vec![
            "bars.csv".to_owned(),
            "artifact.json".to_owned(),
            "--config".to_owned(),
            "backtest.json".to_owned(),
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
        assert_eq!(parsed.configuration_path, PathBuf::from("backtest.json"));
        assert!(parsed.experiment.is_some());
        assert!(matches!(parsed.strategy_mode, StrategyMode::Python(_)));
    }

    #[test]
    fn runtime_configuration_is_content_addressed_and_rejects_unknown_fields() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/backtest-v1.json");
        let loaded = load_runtime_configuration(&fixture).unwrap();
        assert_eq!(loaded.document.configuration_id, "config.spy-baseline");
        assert_eq!(loaded.content_hash.len(), 64);

        let invalid_path = std::env::temp_dir().join(format!(
            "follon-invalid-config-{}-{}.json",
            std::process::id(),
            "unknown-field"
        ));
        let invalid = std::fs::read_to_string(&fixture).unwrap().replacen(
            "\"schema_version\": 1,",
            "\"schema_version\": 1,\n  \"unexpected\": true,",
            1,
        );
        std::fs::write(&invalid_path, invalid).unwrap();
        assert!(load_runtime_configuration(&invalid_path).is_err());
        std::fs::remove_file(invalid_path).unwrap();
    }

    #[test]
    fn runtime_configuration_preserves_v1_execution_defaults() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/backtest-v1.json");
        let mut legacy: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&fixture).unwrap()).unwrap();
        let execution = legacy
            .get_mut("execution")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap();
        execution.remove("spread_bps");
        execution.remove("latency_bars");
        execution.remove("max_fill_quantity");
        let calendar = legacy
            .get_mut("calendar")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap();
        calendar.remove("halts");
        let legacy_path = std::env::temp_dir().join(format!(
            "follon-legacy-config-{}-defaults.json",
            std::process::id()
        ));
        std::fs::write(&legacy_path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let loaded = load_runtime_configuration(&legacy_path).unwrap();
        assert_eq!(loaded.fill_model.spread_bps, Decimal::ZERO);
        assert_eq!(loaded.fill_model.latency_bars, 0);
        assert_eq!(loaded.fill_model.max_fill_quantity, None);
        std::fs::remove_file(legacy_path).unwrap();
    }
}
