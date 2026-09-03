//! Local, non-live reproducible backtest demonstration.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_accounting::{Currency, FxBook, FxQuote, MarginPolicy, MarginRate};
use follon_backtest::{
    AdvancedBacktestAccount, AdvancedBacktestReport, AdvancedInstrumentTerms, BacktestCapitalCheck,
    BacktestExecutionCharges, BacktestInput, BacktestRunner, BacktestSpec, DatasetManifest,
    ExperimentRecord, FileExperimentStore,
};
use follon_cli::{sha256_text, write_immutable};
use follon_control_plane::{
    import_historical_bars, BuyOnceStrategy, DeterministicFillModel, MarketPreconditions,
    ProcessStrategyWorker, ReplayEngine, RiskPolicy, StrategyWorkerIdentity,
    StrategyWorkerServicesConfig,
};
use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, Fill, Side};
use follon_instrument::{
    AssetClass, Instrument, InstrumentRegistry, InstrumentVersion, StaticTradingCalendar,
    TradingHalt, TradingSession,
};
use follon_market_data::import_corporate_actions;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

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
    #[serde(default)]
    advanced_account: Option<AdvancedAccountDocument>,
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
struct CurrencyBalanceDocument {
    currency: String,
    amount: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FxRateDocument {
    base_currency: String,
    quote_currency: String,
    rate: String,
    observed_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarginRateDocument {
    asset_class: String,
    initial_bps: u32,
    maintenance_bps: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvancedInstrumentTermsDocument {
    instrument_id: String,
    asset_class: String,
    currency: String,
    multiplier: String,
    shortable: bool,
    borrow_available: String,
    borrow_rate_bps: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinancingAccrualDocument {
    accrual_id: String,
    effective_at: String,
    days: u32,
    day_count_basis: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DelistingDocument {
    event_id: String,
    instrument_id: String,
    effective_at: String,
    settlement_price: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvancedAccountDocument {
    base_currency: String,
    initial_cash_by_currency: Vec<CurrencyBalanceDocument>,
    maximum_fx_age_seconds: i64,
    fx_rates: Vec<FxRateDocument>,
    margin_rates: Vec<MarginRateDocument>,
    instrument_terms: Vec<AdvancedInstrumentTermsDocument>,
    #[serde(default)]
    cash_debit_rates_bps: BTreeMap<String, u32>,
    #[serde(default)]
    financing_accruals: Vec<FinancingAccrualDocument>,
    #[serde(default)]
    delistings: Vec<DelistingDocument>,
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
    max_price_deviation_bps: String,
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
    /// Every replay is projected through the advanced account. Configurations
    /// without explicit economics receive a conservative cash-account profile
    /// derived only from their already-versioned reference data.
    advanced_account: AdvancedAccountRuntime,
}

struct AdvancedAccountRuntime {
    cash_by_currency: BTreeMap<Currency, Decimal>,
    terms_by_instrument: BTreeMap<String, AdvancedInstrumentTerms>,
    fx: FxBook,
    margin_policy: MarginPolicy,
    cash_debit_rates_bps: BTreeMap<Currency, u32>,
    financing_accruals: Vec<FinancingAccrualRuntime>,
    delistings: Vec<DelistingRuntime>,
}

struct FinancingAccrualRuntime {
    accrual_id: String,
    effective_at: String,
    days: u32,
    day_count_basis: u32,
}

struct DelistingRuntime {
    event_id: String,
    instrument_id: String,
    effective_at: String,
    settlement_price: Decimal,
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
            let mut strategy = ProcessStrategyWorker::spawn_with_services(
                &worker.program,
                worker.protocol_arguments(),
                identity,
                StrategyWorkerServicesConfig {
                    currency: document.account.currency.clone(),
                    initial_cash: configuration.initial_cash,
                },
            )?;
            runner.run(&mut strategy, &input, &market)?
        }
    };
    let advanced_report = advanced_account_projection(
        &completed.canonical_events,
        &input.corporate_actions,
        &configuration.advanced_account,
    )?;
    let event_path = arguments.artifact_path.with_extension("events.ndjson");
    let report_path = arguments.artifact_path.with_extension("report.md");
    let manifest_path = arguments.artifact_path.with_extension("manifest.json");
    let artifact_json = completed.artifact.canonical_json();
    let event_stream = completed.canonical_events.join("\n") + "\n";
    let report = completed.artifact.markdown_report();
    write_immutable(&arguments.artifact_path, &artifact_json)?;
    write_immutable(&event_path, &event_stream)?;
    write_immutable(&report_path, &report)?;
    let advanced_artifact = advanced_report.canonical_json();
    let advanced_report_text = advanced_report.markdown_report();
    let advanced_artifact_path = arguments
        .artifact_path
        .with_extension("advanced-account.json");
    let advanced_report_path = arguments.artifact_path.with_extension("advanced-report.md");
    write_immutable(&advanced_artifact_path, &advanced_artifact)?;
    write_immutable(&advanced_report_path, &advanced_report_text)?;
    let advanced_manifest = format!(
        "{{\"artifact_sha256\":\"{}\",\"report_sha256\":\"{}\"}}",
        sha256_text(&advanced_artifact),
        sha256_text(&advanced_report_text),
    );
    let completion_manifest = format!(
        "{{\"advanced_account\":{},\"artifact_fingerprint\":\"{}\",\"artifact_sha256\":\"{}\",\"configuration_hash\":\"{}\",\"event_output_hash\":\"{}\",\"events_sha256\":\"{}\",\"manifest_schema_version\":2,\"report_sha256\":\"{}\",\"specification_fingerprint\":\"{}\"}}",
        advanced_manifest,
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
    eprintln!(
        "advanced account artifact: {}",
        advanced_artifact_path.display()
    );
    eprintln!(
        "advanced account report: {}",
        advanced_report_path.display()
    );
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
        max_price_deviation_bps: decimal(&document.risk.max_price_deviation_bps)?,
        max_news_slippage_bps: None,
        max_news_spread_multiplier_bps: None,
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
    let advanced_account = match document.advanced_account.as_ref() {
        Some(advanced) => advanced_account_runtime(advanced, &document.instruments)?,
        None => conservative_advanced_account_runtime(&document.account, &document.instruments)?,
    };
    Ok(RuntimeConfiguration {
        document,
        content_hash,
        initial_cash,
        entry_threshold,
        risk_policy,
        fill_model,
        instruments,
        calendar,
        advanced_account,
    })
}

/// Builds the deterministic default for legacy v1 configurations.
///
/// This is deliberately a fully paid cash account: every open position carries
/// 100% initial and maintenance margin, shorting is disabled, and there is no
/// inferred FX, borrow, financing, or lifecycle data. It replaces the former
/// simple-account projection without inventing economics that the immutable
/// configuration did not supply.
fn conservative_advanced_account_runtime(
    account: &AccountDocument,
    instruments: &[InstrumentDocument],
) -> Result<AdvancedAccountRuntime, Box<dyn std::error::Error>> {
    let base_currency = Currency::new(account.currency.clone())?;
    let mut rates = BTreeMap::new();
    let mut terms_by_instrument = BTreeMap::new();
    for instrument in instruments {
        let asset_class = instrument.asset_class.to_ascii_lowercase();
        validate_canonical_id("default advanced margin asset_class", &asset_class)?;
        rates.entry(asset_class.clone()).or_insert(MarginRate {
            initial_bps: 10_000,
            maintenance_bps: 10_000,
        });
        let terms = AdvancedInstrumentTerms {
            currency: Currency::new(instrument.currency.clone())?,
            asset_class,
            multiplier: decimal(&instrument.multiplier)?,
            shortable: false,
            borrow_available: Decimal::ZERO,
            borrow_rate_bps: 0,
        };
        if terms_by_instrument
            .insert(instrument.instrument_id.clone(), terms)
            .is_some()
        {
            return Err("default advanced account has duplicate instrument terms".into());
        }
    }
    Ok(AdvancedAccountRuntime {
        cash_by_currency: BTreeMap::from([(
            base_currency.clone(),
            decimal(&account.initial_cash)?,
        )]),
        terms_by_instrument,
        fx: FxBook::default(),
        margin_policy: MarginPolicy {
            base_currency,
            maximum_fx_age_seconds: 0,
            rates,
        },
        cash_debit_rates_bps: BTreeMap::new(),
        financing_accruals: Vec::new(),
        delistings: Vec::new(),
    })
}

fn advanced_account_runtime(
    document: &AdvancedAccountDocument,
    instruments: &[InstrumentDocument],
) -> Result<AdvancedAccountRuntime, Box<dyn std::error::Error>> {
    let base_currency = Currency::new(document.base_currency.clone())?;
    if document.maximum_fx_age_seconds < 0 || document.initial_cash_by_currency.is_empty() {
        return Err("advanced account requires non-negative FX age and opening cash".into());
    }
    let configured_instruments: BTreeMap<_, _> = instruments
        .iter()
        .map(|instrument| {
            (
                instrument.instrument_id.as_str(),
                (
                    instrument.currency.as_str(),
                    instrument.multiplier.as_str(),
                    instrument.asset_class.to_ascii_lowercase(),
                ),
            )
        })
        .collect();
    let mut cash_by_currency = BTreeMap::new();
    for balance in &document.initial_cash_by_currency {
        let currency = Currency::new(balance.currency.clone())?;
        if cash_by_currency
            .insert(currency, decimal(&balance.amount)?)
            .is_some()
        {
            return Err("advanced account has duplicate opening cash currency".into());
        }
    }
    let mut fx = FxBook::default();
    for rate in &document.fx_rates {
        fx.upsert(FxQuote {
            base: Currency::new(rate.base_currency.clone())?,
            quote: Currency::new(rate.quote_currency.clone())?,
            quote_rate: decimal(&rate.rate)?,
            observed_at_epoch_seconds: epoch_seconds(&rate.observed_at)?,
        })?;
    }
    let mut rates = BTreeMap::new();
    for rate in &document.margin_rates {
        validate_canonical_id("advanced margin asset_class", &rate.asset_class)?;
        if rate.initial_bps == 0
            || rate.initial_bps > 10_000
            || rate.maintenance_bps == 0
            || rate.maintenance_bps > rate.initial_bps
        {
            return Err("advanced account has invalid initial or maintenance margin rate".into());
        }
        if rates
            .insert(
                rate.asset_class.clone(),
                MarginRate {
                    initial_bps: rate.initial_bps,
                    maintenance_bps: rate.maintenance_bps,
                },
            )
            .is_some()
        {
            return Err("advanced account has duplicate margin asset class".into());
        }
    }
    let margin_policy = MarginPolicy {
        base_currency,
        maximum_fx_age_seconds: document.maximum_fx_age_seconds,
        rates,
    };
    let mut terms_by_instrument = BTreeMap::new();
    for terms in &document.instrument_terms {
        validate_canonical_id("advanced instrument_id", &terms.instrument_id)?;
        let Some((currency, multiplier, asset_class)) =
            configured_instruments.get(terms.instrument_id.as_str())
        else {
            return Err("advanced account terms reference an unconfigured instrument".into());
        };
        if terms.currency != *currency
            || terms.multiplier != *multiplier
            || terms.asset_class != *asset_class
        {
            return Err(
                "advanced instrument terms must match immutable configured reference data".into(),
            );
        }
        let parsed = AdvancedInstrumentTerms {
            currency: Currency::new(terms.currency.clone())?,
            asset_class: terms.asset_class.clone(),
            multiplier: decimal(&terms.multiplier)?,
            shortable: terms.shortable,
            borrow_available: decimal(&terms.borrow_available)?,
            borrow_rate_bps: terms.borrow_rate_bps,
        };
        if terms_by_instrument
            .insert(terms.instrument_id.clone(), parsed)
            .is_some()
        {
            return Err("advanced account has duplicate instrument terms".into());
        }
    }
    if terms_by_instrument.len() != configured_instruments.len()
        || !configured_instruments
            .keys()
            .all(|instrument_id| terms_by_instrument.contains_key(*instrument_id))
    {
        return Err("advanced account must declare terms for every configured instrument".into());
    }
    let mut cash_debit_rates_bps = BTreeMap::new();
    for (currency, rate) in &document.cash_debit_rates_bps {
        if cash_debit_rates_bps
            .insert(Currency::new(currency.clone())?, *rate)
            .is_some()
        {
            return Err("advanced account has duplicate cash-debit currency".into());
        }
    }
    let mut financing_ids = BTreeMap::new();
    let mut financing_accruals = Vec::with_capacity(document.financing_accruals.len());
    for accrual in &document.financing_accruals {
        validate_canonical_id("advanced financing accrual_id", &accrual.accrual_id)?;
        validate_utc_timestamp("advanced financing effective_at", &accrual.effective_at)?;
        if accrual.days == 0 || accrual.day_count_basis == 0 || accrual.day_count_basis > 366 {
            return Err("advanced financing days and day-count basis must be positive".into());
        }
        if financing_ids
            .insert(accrual.accrual_id.as_str(), ())
            .is_some()
        {
            return Err("advanced account has duplicate financing accrual".into());
        }
        financing_accruals.push(FinancingAccrualRuntime {
            accrual_id: accrual.accrual_id.clone(),
            effective_at: accrual.effective_at.clone(),
            days: accrual.days,
            day_count_basis: accrual.day_count_basis,
        });
    }
    financing_accruals.sort_by(|left, right| left.effective_at.cmp(&right.effective_at));
    let mut delisting_ids = BTreeMap::new();
    let mut delistings = Vec::with_capacity(document.delistings.len());
    for delisting in &document.delistings {
        validate_canonical_id("advanced delisting event_id", &delisting.event_id)?;
        validate_canonical_id("advanced delisting instrument_id", &delisting.instrument_id)?;
        validate_utc_timestamp("advanced delisting effective_at", &delisting.effective_at)?;
        if !terms_by_instrument.contains_key(&delisting.instrument_id) {
            return Err("advanced delisting references an unconfigured instrument".into());
        }
        if delisting_ids
            .insert(delisting.event_id.as_str(), ())
            .is_some()
        {
            return Err("advanced account has duplicate delisting event".into());
        }
        delistings.push(DelistingRuntime {
            event_id: delisting.event_id.clone(),
            instrument_id: delisting.instrument_id.clone(),
            effective_at: delisting.effective_at.clone(),
            settlement_price: decimal(&delisting.settlement_price)?,
        });
    }
    delistings.sort_by(|left, right| left.effective_at.cmp(&right.effective_at));
    Ok(AdvancedAccountRuntime {
        cash_by_currency,
        terms_by_instrument,
        fx,
        margin_policy,
        cash_debit_rates_bps,
        financing_accruals,
        delistings,
    })
}

fn epoch_seconds(value: &str) -> Result<i64, Box<dyn std::error::Error>> {
    validate_utc_timestamp("advanced timestamp", value)?;
    Ok(OffsetDateTime::parse(value, &Rfc3339)?.unix_timestamp())
}

fn advanced_account_projection(
    canonical_events: &[String],
    corporate_actions: &[follon_market_data::CorporateAction],
    runtime: &AdvancedAccountRuntime,
) -> Result<AdvancedBacktestReport, Box<dyn std::error::Error>> {
    let mut account = AdvancedBacktestAccount::new(runtime.cash_by_currency.clone())?;
    let mut marks = BTreeMap::new();
    let mut actions: Vec<_> = corporate_actions.iter().collect();
    actions.sort_by(|left, right| {
        left.effective_at()
            .cmp(right.effective_at())
            .then_with(|| left.action_id().cmp(right.action_id()))
    });
    let mut next_action = 0;
    let mut next_financing = 0;
    let mut next_delisting = 0;
    let mut final_time = None;

    for line in canonical_events {
        let event: serde_json::Value = serde_json::from_str(line)?;
        let object = event
            .as_object()
            .ok_or("canonical backtest event must be an object")?;
        let event_type = required_json_string(object, "event_type")?;
        let event_time = required_json_string(object, "event_time")?;
        if event_type != "market.bar.v1" {
            if event_type == "execution.fill.v1" {
                let payload = object
                    .get("payload")
                    .and_then(serde_json::Value::as_object)
                    .ok_or("fill event payload must be an object")?;
                let side = match required_json_string(payload, "side")? {
                    "BUY" => Side::Buy,
                    "SELL" => Side::Sell,
                    _ => return Err("fill event has invalid side".into()),
                };
                let fill = Fill {
                    execution_id: required_json_string(payload, "execution_id")?.to_owned(),
                    order_id: required_json_string(payload, "order_id")?.to_owned(),
                    instrument_id: required_json_string(payload, "instrument_id")?.to_owned(),
                    side,
                    quantity: decimal(required_json_string(payload, "quantity")?)?,
                    price: decimal(required_json_string(payload, "price")?)?,
                    fee: decimal(required_json_string(payload, "fee")?)?,
                    executed_at: required_json_string(payload, "executed_at")?.to_owned(),
                };
                let terms = runtime
                    .terms_by_instrument
                    .get(&fill.instrument_id)
                    .ok_or("advanced account has no terms for simulated fill")?;
                account.apply_fill_with_capital_check(
                    &fill,
                    terms,
                    BacktestExecutionCharges {
                        commission: fill.fee,
                        exchange: Decimal::ZERO,
                        regulatory: Decimal::ZERO,
                    },
                    BacktestCapitalCheck {
                        marks_after_fill: &marks,
                        fx: &runtime.fx,
                        policy: &runtime.margin_policy,
                        as_of_epoch_seconds: epoch_seconds(&fill.executed_at)?,
                    },
                )?;
            }
            continue;
        }

        while actions
            .get(next_action)
            .is_some_and(|action| action.effective_at() <= event_time)
        {
            account.apply_corporate_action(actions[next_action])?;
            next_action += 1;
        }
        while runtime
            .delistings
            .get(next_delisting)
            .is_some_and(|delisting| delisting.effective_at.as_str() <= event_time)
        {
            let delisting = &runtime.delistings[next_delisting];
            account.settle_delisting(
                &delisting.event_id,
                &delisting.instrument_id,
                &delisting.effective_at,
                delisting.settlement_price,
            )?;
            next_delisting += 1;
        }
        while runtime
            .financing_accruals
            .get(next_financing)
            .is_some_and(|accrual| accrual.effective_at.as_str() <= event_time)
        {
            let accrual = &runtime.financing_accruals[next_financing];
            account.accrue_financing(
                &accrual.accrual_id,
                accrual.days,
                accrual.day_count_basis,
                &marks,
                &runtime.cash_debit_rates_bps,
            )?;
            next_financing += 1;
        }
        let payload = object
            .get("payload")
            .and_then(serde_json::Value::as_object)
            .ok_or("market event payload must be an object")?;
        let instrument_id = required_json_string(payload, "instrument_id")?;
        let close = decimal(required_json_string(payload, "close")?)?;
        if close <= Decimal::ZERO {
            return Err("market event close must be positive".into());
        }
        marks.insert(instrument_id.to_owned(), close);
        final_time = Some(event_time.to_owned());
    }

    let final_time = final_time.ok_or("advanced backtest received no market events")?;
    if next_financing != runtime.financing_accruals.len()
        || next_delisting != runtime.delistings.len()
    {
        return Err("advanced lifecycle input falls outside the selected backtest range".into());
    }
    Ok(account.report(
        &marks,
        &runtime.fx,
        &runtime.margin_policy,
        epoch_seconds(&final_time)?,
    )?)
}

fn required_json_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("canonical backtest event has no {field}").into())
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
