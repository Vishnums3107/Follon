//! Local, non-live reproducible backtest demonstration.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use follon_accounting::{Currency, FxBook, FxQuote, MarginPolicy, MarginRate};
use follon_backtest::{
    AdvancedBacktestAccount, AdvancedBacktestReport, AdvancedInstrumentTerms,
    AdversarialProbeResult, AdversarialResearchGate, BacktestCapitalCheck,
    BacktestExecutionCharges, BacktestInput, BacktestRunner, BacktestSpec, CompletedBacktest,
    CounterfactualDeltaMetrics, CounterfactualEngine, CounterfactualIntervention,
    CounterfactualInterventionType, CounterfactualScenario, DatasetManifest, ExperimentRecord,
    FileExperimentStore,
};
use follon_cli::{sha256_text, write_immutable};
use follon_commercial::TrustedReleaseKey;
use follon_control_plane::{
    build_strategy_bundle, extract_strategy_bundle, import_historical_bars, read_strategy_capsule,
    sign_strategy_capsule, verify_capsule_signature, write_capsule_signature, BuyOnceStrategy,
    CapsuleContents, DeterministicFillModel, HistoricalBar, MarketPreconditions,
    ProcessStrategyWorker, ReplayEngine, RiskPolicy, StrategyWorkerIdentity, StrategyWorkerSandbox,
    StrategyWorkerServicesConfig,
};
use follon_domain::{
    validate_canonical_id, validate_utc_timestamp, Decimal, Fill, Side, DECIMAL_SCALE,
};
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
    Python(Box<PythonWorkerArguments>),
}

struct PythonWorkerArguments {
    program: String,
    strategy_file: String,
    class_name: String,
    bundle_root: String,
    strategy_id: String,
    strategy_version: String,
    bundle_hash: String,
    /// Present only for a capsule replay; see [`StrategyWorkerSandbox`].
    sandbox: Option<StrategyWorkerSandbox>,
}

impl PythonWorkerArguments {
    /// Interpreter arguments for the worker protocol.
    ///
    /// A sandboxed worker also runs with `-S`, so no `site` directory is on
    /// its path: a capsule whose strategy imports anything it did not vendor
    /// fails its replay instead of silently resolving against whatever
    /// happens to be installed.
    fn protocol_arguments(&self) -> Vec<OsString> {
        let isolation: &[&str] = if self.sandbox.is_some() { &["-S"] } else { &[] };
        isolation
            .iter()
            .copied()
            .chain([
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
            ])
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
    let raw_args: Vec<String> = env::args().skip(1).collect();
    if let Some(subcommand) = raw_args.first() {
        match subcommand.as_str() {
            "adversarial" => return run_adversarial(&raw_args[1..]),
            "counterfactual" => return run_counterfactual(&raw_args[1..]),
            "capsule-package" => return run_capsule_package(&raw_args[1..]),
            "capsule-verify" => return run_capsule_verify(&raw_args[1..]),
            "capsule-sign" => return run_capsule_sign(&raw_args[1..]),
            _ => {}
        }
    }
    let arguments = parse_arguments(raw_args)?;
    if let Some(parent) = arguments.artifact_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let outputs = evaluate_backtest(
        &arguments.input_path,
        &arguments.configuration_path,
        arguments.action_path.as_deref(),
        arguments.strategy_mode,
    )?;
    let event_path = arguments.artifact_path.with_extension("events.ndjson");
    let report_path = arguments.artifact_path.with_extension("report.md");
    let manifest_path = arguments.artifact_path.with_extension("manifest.json");
    write_immutable(&arguments.artifact_path, &outputs.artifact_json)?;
    write_immutable(&event_path, &outputs.event_stream)?;
    write_immutable(&report_path, &outputs.report)?;
    write_immutable(&manifest_path, &outputs.completion_manifest)?;
    if let Some(experiment) = arguments.experiment {
        let record = ExperimentRecord::from_artifact(
            experiment.experiment_id,
            experiment.run_id,
            BTreeMap::new(),
            &outputs.completed.artifact,
        )?;
        let mut store = FileExperimentStore::open(experiment.catalog_path)?;
        store.record(record)?;
    }
    eprintln!("artifact: {}", arguments.artifact_path.display());
    eprintln!("event stream: {}", event_path.display());
    eprintln!("report: {}", report_path.display());
    eprintln!("completion manifest: {}", manifest_path.display());
    eprintln!(
        "artifact fingerprint: {}",
        outputs.completed.artifact.fingerprint()
    );
    eprintln!("configuration hash: {}", outputs.configuration_hash);
    Ok(())
}

/// Every output of one deterministic backtest, held in memory.
///
/// `main` publishes these as immutable files; a capsule replay instead
/// compares `completion_manifest` byte for byte with a sealed receipt.
struct EvaluationOutputs {
    completed: CompletedBacktest,
    configuration_hash: String,
    artifact_json: String,
    event_stream: String,
    report: String,
    completion_manifest: String,
}

fn evaluate_backtest(
    input_path: &Path,
    configuration_path: &Path,
    action_path: Option<&Path>,
    strategy_mode: StrategyMode,
) -> Result<EvaluationOutputs, Box<dyn std::error::Error>> {
    let configuration = load_runtime_configuration(configuration_path)?;
    let document = &configuration.document;

    let bars = import_historical_bars(&fs::read_to_string(input_path)?)?;
    let corporate_actions = match action_path {
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
    let (strategy_bundle_hash, strategy_id, strategy_version) = match &strategy_mode {
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
    let mut completed = match strategy_mode {
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
            let services = StrategyWorkerServicesConfig {
                currency: document.account.currency.clone(),
                initial_cash: configuration.initial_cash,
            };
            let mut strategy = match &worker.sandbox {
                Some(sandbox) => ProcessStrategyWorker::spawn_sandboxed_with_services(
                    &worker.program,
                    worker.protocol_arguments(),
                    identity,
                    services,
                    sandbox,
                )?,
                None => ProcessStrategyWorker::spawn_with_services(
                    &worker.program,
                    worker.protocol_arguments(),
                    identity,
                    services,
                )?,
            };
            runner.run(&mut strategy, &input, &market)?
        }
    };
    let advanced_report = advanced_account_projection(
        &completed.canonical_events,
        &input.corporate_actions,
        &configuration.advanced_account,
    )?;
    // The advanced economics travel inside the artifact itself (schema 3),
    // so its hash and fingerprint bind them; there is no sidecar to read.
    completed.artifact = completed.artifact.with_advanced_account(advanced_report);
    let artifact_json = completed.artifact.canonical_json();
    let event_stream = completed.canonical_events.join("\n") + "\n";
    let report = completed.artifact.markdown_report();
    let completion_manifest = format!(
        "{{\"artifact_fingerprint\":\"{}\",\"artifact_sha256\":\"{}\",\"configuration_hash\":\"{}\",\"event_output_hash\":\"{}\",\"events_sha256\":\"{}\",\"manifest_schema_version\":3,\"report_sha256\":\"{}\",\"specification_fingerprint\":\"{}\"}}",
        completed.artifact.fingerprint(),
        sha256_text(&artifact_json),
        configuration.content_hash,
        completed.artifact.event_output_hash,
        sha256_text(&event_stream),
        sha256_text(&report),
        completed.artifact.specification_fingerprint,
    );
    Ok(EvaluationOutputs {
        completed,
        configuration_hash: configuration.content_hash,
        artifact_json,
        event_stream,
        report,
        completion_manifest,
    })
}

/// `follon-backtest capsule-package`: seals a portable capsule around a real
/// Python-worker evaluation.
///
/// Nothing is taken on trust. The strategy archive is rebuilt from the bundle
/// and SDK trees and must open exactly as the SDK's lock describes. It must
/// hash to the bundle hash the evaluation's own specification recorded, which
/// is the hash the worker announced and the runner verified. The
/// configuration must hash to what the evaluation and its completion manifest
/// recorded, and the completion manifest must hash-bind the artifact. The
/// capsule's own copies are then replayed in a sandbox, and a manifest is
/// sealed only if that replay reproduces the completion manifest byte for
/// byte. The evaluation must have run without corporate actions, because the
/// replay supplies none.
fn run_capsule_package(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (positional, flags) = parse_flag_values(
        arguments,
        &[
            "--bundle-root",
            "--sdk-root",
            "--lock",
            "--config",
            "--evaluation",
            "--bars",
            "--python",
            "--packaged-at",
            "--output",
        ],
        &[],
    )?;
    if !positional.is_empty() {
        return Err("usage: follon-backtest capsule-package --bundle-root <dir> --sdk-root <dir> --lock <file> --config <file> --evaluation <artifact.json> --bars <csv> --python <interpreter> --packaged-at <utc> --output <dir>".into());
    }
    let flag = |name: &str| flags[name].as_str();

    let lock_bytes = fs::read(flag("--lock"))?;
    let lock = follon_control_plane::StrategyBundleLock::parse(&lock_bytes)?;
    let archive = build_strategy_bundle(
        Path::new(flag("--bundle-root")),
        Path::new(flag("--sdk-root")),
        &lock.runtime,
    )?;
    let configuration_path = PathBuf::from(flag("--config"));
    let configuration_bytes = fs::read(&configuration_path)?;
    load_runtime_configuration(&configuration_path)?;
    let evaluation_path = PathBuf::from(flag("--evaluation"));
    let artifact_bytes = fs::read(&evaluation_path)?;
    let receipt_bytes = fs::read(evaluation_path.with_extension("manifest.json"))?;
    let contents = CapsuleContents::new(lock_bytes, archive, configuration_bytes, receipt_bytes)?;

    let receipt: serde_json::Value = serde_json::from_slice(contents.receipt())?;
    if receipt
        .get("artifact_sha256")
        .and_then(serde_json::Value::as_str)
        != Some(format!("{:x}", Sha256::digest(&artifact_bytes)).as_str())
    {
        return Err("the completion manifest does not bind this evaluation artifact".into());
    }
    let artifact: serde_json::Value = serde_json::from_slice(&artifact_bytes)?;
    let specification = artifact
        .get("specification")
        .ok_or("evaluation artifact has no specification")?;
    let recorded = |field: &str| {
        specification
            .get(field)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("evaluation specification has no {field}"))
    };
    if recorded("strategy_bundle_hash")? != contents.lock.strategy_bundle_hash {
        return Err("the evaluation was not run with this strategy bundle".into());
    }
    if recorded("configuration_hash")? != format!("{:x}", Sha256::digest(contents.configuration()))
    {
        return Err("the evaluation was not run with this configuration".into());
    }
    let dataset = |field: &str| {
        specification
            .get("dataset")
            .and_then(|dataset| dataset.get(field))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("evaluation dataset has no {field}"))
    };
    let replay_command = format!(
        "follon-backtest capsule-verify <capsule-dir> --bars <{} {} bars, dataset content hash {}> --python <{} interpreter>",
        dataset("dataset_id")?,
        dataset("dataset_version")?,
        dataset("content_hash")?,
        contents.lock.runtime,
    );

    let reproduced = replay_capsule(&contents, Path::new(flag("--bars")), flag("--python"))?;
    let manifest = contents.seal(
        flag("--packaged-at"),
        &replay_command,
        reproduced.as_bytes(),
    )?;
    let output = PathBuf::from(flag("--output"));
    contents.write_sealed(&manifest, &output)?;
    let sealed = read_strategy_capsule(&output)?;
    println!(
        "{} {}",
        sealed.manifest.capsule_id,
        sealed.manifest.export_disposition.as_str()
    );
    eprintln!("capsule: {}", output.display());
    eprintln!("bundle hash: {}", sealed.manifest.bundle_sha256);
    eprintln!(
        "evaluation receipt: {}",
        sealed.manifest.evaluation_receipt_id
    );
    Ok(())
}

/// `follon-backtest capsule-verify`: re-checks a sealed capsule and replays it.
///
/// Every static binding is recomputed from the capsule's files, then the
/// capsule's own strategy and SDK are replayed in a sandbox. Success means the
/// replay reproduced the sealed evaluation receipt byte for byte.
fn run_capsule_verify(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (positional, flags) =
        parse_flag_values(arguments, &["--bars", "--python"], &["--trusted-key"])?;
    let [capsule_directory] = positional.as_slice() else {
        return Err(
            "usage: follon-backtest capsule-verify <capsule-dir> --bars <csv> --python <interpreter> [--trusted-key <key.json>]"
                .into(),
        );
    };
    let sealed = read_strategy_capsule(Path::new(capsule_directory))?;
    let signer = match (flags.get("--trusted-key"), &sealed.signature) {
        (Some(path), _) => {
            let key = TrustedReleaseKey::parse_canonical(&fs::read_to_string(path)?)?;
            verify_capsule_signature(&sealed, &key.key_id, &key.public_key_hex)?;
            format!("signed by trusted key {}", key.key_id)
        }
        (None, Some(signature)) => format!(
            "signature by {} present but not checked: no --trusted-key given",
            signature.key_id
        ),
        (None, None) => "unsigned".to_owned(),
    };
    let reproduced = replay_capsule(
        &sealed.contents,
        Path::new(&flags["--bars"]),
        &flags["--python"],
    )?;
    if reproduced.as_bytes() != sealed.contents.receipt() {
        return Err("capsule replay did not reproduce its evaluation receipt".into());
    }
    println!(
        "{} {}: replay reproduced {}; {signer}",
        sealed.manifest.capsule_id,
        sealed.manifest.export_disposition.as_str(),
        sealed.manifest.evaluation_receipt_id
    );
    Ok(())
}

/// `follon-backtest capsule-sign`: adds a detached Ed25519 signature to a
/// sealed capsule that does not yet carry one.
///
/// The capsule is fully re-read first, so only a capsule whose every binding
/// holds can be signed. The key is the PKCS#8 file `follon-admin
/// release-keygen` writes; its bytes are zeroed after use. A signature says who
/// sealed the capsule; `capsule-verify --trusted-key` checks it.
fn run_capsule_sign(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (positional, flags) = parse_flag_values(
        arguments,
        &["--private-key", "--key-id", "--signed-at"],
        &[],
    )?;
    let [capsule_directory] = positional.as_slice() else {
        return Err("usage: follon-backtest capsule-sign <capsule-dir> --private-key <key.pk8> --key-id <id> --signed-at <utc>".into());
    };
    let directory = Path::new(capsule_directory);
    let sealed = read_strategy_capsule(directory)?;
    let mut private_key = fs::read(&flags["--private-key"])?;
    let signed = sign_strategy_capsule(
        &sealed,
        &private_key,
        &flags["--key-id"],
        &flags["--signed-at"],
    );
    private_key.fill(0);
    let signature = signed?;
    write_capsule_signature(directory, &signature)?;
    println!("{}", signature.to_json());
    Ok(())
}

/// Replays a capsule's own strategy and SDK and returns the completion
/// manifest the replay produced.
///
/// The archive is extracted into a fresh temporary directory, which is
/// removed afterwards. The worker's only import root is the extracted SDK, its
/// working directory is the temporary directory, and it runs with `-S`, so
/// neither the caller's environment nor installed packages can stand in for
/// the capsule's contents.
fn replay_capsule(
    contents: &CapsuleContents,
    bars: &Path,
    python: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if !Path::new(python).is_absolute() {
        return Err(
            "--python must be an absolute interpreter path; the worker receives no PATH".into(),
        );
    }
    let workspace = ReplayWorkspace::create()?;
    let extracted = extract_strategy_bundle(
        &contents.sources,
        &contents.lock.entry_point,
        &workspace.path.join("bundle"),
    )?;
    let configuration_path = workspace.path.join("configuration.json");
    fs::write(&configuration_path, contents.configuration())?;
    let text = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or("capsule replay path is not UTF-8")
    };
    let worker = PythonWorkerArguments {
        program: python.to_owned(),
        strategy_file: text(&extracted.strategy_file)?,
        class_name: contents.lock.entry_point.class_name.clone(),
        bundle_root: text(&extracted.strategy_root)?,
        strategy_id: contents.strategy_id.clone(),
        strategy_version: contents.strategy_version.clone(),
        bundle_hash: contents.lock.strategy_bundle_hash.clone(),
        sandbox: Some(StrategyWorkerSandbox {
            python_path: extracted.sdk_search_path.clone(),
            working_directory: workspace.path.clone(),
        }),
    };
    let outputs = evaluate_backtest(
        bars,
        &configuration_path,
        None,
        StrategyMode::Python(Box::new(worker)),
    )?;
    Ok(outputs.completion_manifest)
}

/// A temporary replay directory, removed when dropped.
struct ReplayWorkspace {
    path: PathBuf,
}

impl ReplayWorkspace {
    fn create() -> Result<Self, Box<dyn std::error::Error>> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir().join(format!(
            "follon-capsule-replay-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }
}

impl Drop for ReplayWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Positional arguments, then each flag's value keyed by the flag.
type FlagValues = (Vec<String>, BTreeMap<String, String>);

/// Splits `--flag value` pairs from positional arguments. Each `required`
/// flag must appear exactly once, each `optional` flag at most once, and no
/// other flag is accepted.
fn parse_flag_values(
    arguments: &[String],
    required: &[&str],
    optional: &[&str],
) -> Result<FlagValues, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut values = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument.starts_with("--") {
            if !required.contains(&argument.as_str()) && !optional.contains(&argument.as_str()) {
                return Err(format!("unsupported argument: {argument}").into());
            }
            let value = required_argument(arguments, index + 1, argument)?.to_owned();
            if values.insert(argument.clone(), value).is_some() {
                return Err(format!("{argument} may be specified only once").into());
            }
            index += 2;
        } else {
            positional.push(argument.clone());
            index += 1;
        }
    }
    if let Some(missing) = required.iter().find(|flag| !values.contains_key(**flag)) {
        return Err(format!("{missing} is required").into());
    }
    Ok((positional, values))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdversarialConfigDocument {
    strategy_version: String,
    evaluated_at: String,
    #[serde(default)]
    probes: Vec<AdversarialProbeDocument>,
    #[serde(default)]
    execute: Option<AdversarialExecuteDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdversarialProbeDocument {
    probe_name: String,
    probe_description: String,
    passed: bool,
    degradation_bps: i64,
    threshold_bps: i64,
}

/// Configuration for actually executing the 5 standardized stress probes
/// against a real deterministic backtest, instead of certifying
/// operator-attested figures. See [`execute_adversarial_probes`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdversarialExecuteDocument {
    /// Path to a `backtest-*.json`-shaped configuration, resolved relative to
    /// the directory containing this adversarial config file.
    backtest_configuration: String,
    /// Path to the historical-bar CSV, resolved the same way.
    historical_bars: String,
    /// Seed for the deterministic perturbations used by the noise and regime
    /// probes (independent of the backtest configuration's own `seed`).
    #[serde(default)]
    seed: u64,
    probes: Vec<AdversarialExecuteProbeDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdversarialExecuteProbeDocument {
    probe_name: String,
    probe_description: String,
    threshold_bps: i64,
}

/// Certifies adversarial probe results into a composite score and pass/fail
/// gate via `AdversarialResearchGate::evaluate_probes`.
///
/// The config file selects one of two modes. `probes` (a `passed` /
/// `degradation_bps` value per probe) is operator-attested: this command does
/// not run any stress probe itself, and those figures must already reflect a
/// real stress test performed by a separate tool or human operator. `execute`
/// instead names a real backtest configuration and historical-bar corpus;
/// this command then actually drives the built-in deterministic strategy
/// through 5 genuine perturbed replays (see [`execute_adversarial_probes`])
/// and computes `passed`/`degradation_bps` from their real output. Either way
/// the final certification step — aggregation, composite score, and gate
/// decision — is the same unchanged `AdversarialResearchGate`.
fn run_adversarial(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if arguments.is_empty() || arguments.len() > 2 {
        return Err("usage: follon-backtest adversarial <config.json> [output.json]".into());
    }
    let input_path = PathBuf::from(&arguments[0]);
    let output_path = arguments
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("var/adversarial-eval.json"));

    let content = fs::read_to_string(&input_path)?;
    let doc: AdversarialConfigDocument = serde_json::from_str(&content)?;
    validate_utc_timestamp("evaluated_at", &doc.evaluated_at)?;

    let probes: Vec<AdversarialProbeResult> = match (doc.execute, doc.probes) {
        (Some(_), probes) if !probes.is_empty() => {
            return Err(
                "adversarial config cannot combine 'execute' with attested 'probes' results".into(),
            );
        }
        (Some(execute), _) => {
            let base_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
            execute_adversarial_probes(base_dir, &execute)?
        }
        (None, probes) if probes.is_empty() => {
            return Err("adversarial config must specify 'probes' or 'execute'".into());
        }
        (None, probes) => probes
            .into_iter()
            .map(|p| AdversarialProbeResult {
                probe_name: p.probe_name,
                probe_description: p.probe_description,
                passed: p.passed,
                degradation_bps: p.degradation_bps,
                threshold_bps: p.threshold_bps,
            })
            .collect(),
    };

    let eval =
        AdversarialResearchGate::evaluate_probes(&doc.strategy_version, probes, &doc.evaluated_at)?;
    let json = eval.to_json();

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_immutable(&output_path, &json)?;
    eprintln!("adversarial evaluation: {}", output_path.display());
    eprintln!("evaluation id: {}", eval.evaluation_id);
    eprintln!("gate passed: {}", eval.gate_passed);
    eprintln!(
        "composite score bps: {}",
        eval.composite_robustness_score_bps
    );
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CounterfactualConfigDocument {
    #[serde(default)]
    scenario_id: Option<String>,
    baseline_run_id: String,
    seed: u64,
    divergence_event_id: String,
    created_at: String,
    interventions: Vec<CounterfactualInterventionDocument>,
    #[serde(default)]
    delta_metrics: Option<CounterfactualDeltaMetricsDocument>,
    #[serde(default)]
    metrics: Option<CounterfactualMetricsDocument>,
    #[serde(default)]
    execute: Option<CounterfactualExecuteDocument>,
}

/// Configuration for actually executing the declared `interventions` against
/// a real deterministic backtest, instead of certifying operator-attested
/// figures. See [`execute_counterfactual_scenario`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CounterfactualExecuteDocument {
    /// Path to a `backtest-*.json`-shaped configuration, resolved relative to
    /// the directory containing this counterfactual config file.
    backtest_configuration: String,
    /// Path to the historical-bar CSV, resolved the same way.
    historical_bars: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CounterfactualInterventionDocument {
    intervention_type: String,
    parameter_name: String,
    baseline_value: String,
    counterfactual_value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CounterfactualDeltaMetricsDocument {
    fill_count_delta: i64,
    pnl_delta_usd: String,
    max_drawdown_delta_bps: i64,
    risk_rejection_count_delta: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CounterfactualMetricsDocument {
    baseline_fills: i64,
    counterfactual_fills: i64,
    baseline_pnl_cents: i64,
    counterfactual_pnl_cents: i64,
    baseline_max_drawdown_bps: i64,
    counterfactual_max_drawdown_bps: i64,
    baseline_rejections: i64,
    counterfactual_rejections: i64,
}

fn parse_intervention_type(
    s: &str,
) -> Result<CounterfactualInterventionType, Box<dyn std::error::Error>> {
    match s {
        "RISK_COLLAR_ADJUSTMENT" => Ok(CounterfactualInterventionType::RiskCollarAdjustment),
        "NETWORK_LATENCY_INJECTION" => Ok(CounterfactualInterventionType::NetworkLatencyInjection),
        "DATA_BAR_CORRUPTION" => Ok(CounterfactualInterventionType::DataBarCorruption),
        "VOLATILITY_SHOCK" => Ok(CounterfactualInterventionType::VolatilityShock),
        other => Err(format!("unknown intervention_type: {other}").into()),
    }
}

/// Reads an operator-supplied counterfactual scenario (baseline run identity
/// and interventions) from a JSON config file and certifies it via
/// `CounterfactualEngine::evaluate_scenario`.
///
/// The config file selects one of three ways to obtain the baseline/
/// counterfactual figures. `metrics` and `delta_metrics` are
/// operator-attested: this command does not simulate any intervention
/// itself, and those figures must already come from a real counterfactual
/// run performed by a separate tool or human operator. `execute` instead
/// names a real backtest configuration and historical-bar corpus; this
/// command then actually drives the built-in deterministic strategy through
/// two genuine replays — one unperturbed, one with every declared
/// intervention applied (see [`execute_counterfactual_scenario`]) — and
/// computes the metrics from their real output. Either way the final
/// certification step is the same unchanged `CounterfactualEngine`.
fn run_counterfactual(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if arguments.is_empty() || arguments.len() > 2 {
        return Err("usage: follon-backtest counterfactual <config.json> [output.json]".into());
    }
    let input_path = PathBuf::from(&arguments[0]);
    let output_path = arguments
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("var/counterfactual.json"));

    let content = fs::read_to_string(&input_path)?;
    let doc: CounterfactualConfigDocument = serde_json::from_str(&content)?;
    validate_utc_timestamp("created_at", &doc.created_at)?;

    let mut interventions = Vec::with_capacity(doc.interventions.len());
    for item in doc.interventions {
        let itype = parse_intervention_type(&item.intervention_type)?;
        interventions.push(CounterfactualIntervention {
            intervention_type: itype,
            parameter_name: item.parameter_name,
            baseline_value: item.baseline_value,
            counterfactual_value: item.counterfactual_value,
        });
    }

    let scenario = if let Some(execute) = doc.execute {
        if doc.metrics.is_some() || doc.delta_metrics.is_some() {
            return Err(
                "counterfactual config cannot combine 'execute' with 'metrics' or 'delta_metrics'"
                    .into(),
            );
        }
        if interventions.is_empty() {
            return Err("counterfactual scenario requires at least one intervention".into());
        }
        let base_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
        let m = execute_counterfactual_scenario(base_dir, &execute, &interventions, doc.seed)?;
        CounterfactualEngine::evaluate_scenario(
            &doc.baseline_run_id,
            doc.seed,
            interventions,
            m.baseline_fills,
            m.counterfactual_fills,
            m.baseline_pnl_cents,
            m.counterfactual_pnl_cents,
            m.baseline_max_drawdown_bps,
            m.counterfactual_max_drawdown_bps,
            m.baseline_rejections,
            m.counterfactual_rejections,
            &doc.divergence_event_id,
            &doc.created_at,
        )?
    } else if let Some(m) = doc.metrics {
        CounterfactualEngine::evaluate_scenario(
            &doc.baseline_run_id,
            doc.seed,
            interventions,
            m.baseline_fills,
            m.counterfactual_fills,
            m.baseline_pnl_cents,
            m.counterfactual_pnl_cents,
            m.baseline_max_drawdown_bps,
            m.counterfactual_max_drawdown_bps,
            m.baseline_rejections,
            m.counterfactual_rejections,
            &doc.divergence_event_id,
            &doc.created_at,
        )?
    } else if let Some(d) = doc.delta_metrics {
        if interventions.is_empty() {
            return Err("counterfactual scenario requires at least one intervention".into());
        }
        let digest = format!(
            "{:x}",
            Sha256::digest(
                format!("{}:{}:{}", doc.baseline_run_id, doc.seed, doc.created_at).as_bytes()
            )
        );
        let scenario_id = doc
            .scenario_id
            .unwrap_or_else(|| format!("cf.{}", &digest[..16]));
        CounterfactualScenario {
            scenario_schema_version: 1,
            scenario_id,
            baseline_run_id: doc.baseline_run_id,
            seed: doc.seed,
            interventions,
            delta_metrics: CounterfactualDeltaMetrics {
                fill_count_delta: d.fill_count_delta,
                pnl_delta_usd: d.pnl_delta_usd,
                max_drawdown_delta_bps: d.max_drawdown_delta_bps,
                risk_rejection_count_delta: d.risk_rejection_count_delta,
            },
            divergence_event_id: doc.divergence_event_id,
            created_at: doc.created_at,
        }
    } else {
        return Err(
            "counterfactual config must specify 'execute', 'metrics', or 'delta_metrics'".into(),
        );
    };

    let json = scenario.to_json();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_immutable(&output_path, &json)?;
    eprintln!("counterfactual scenario: {}", output_path.display());
    eprintln!("scenario id: {}", scenario.scenario_id);
    eprintln!("baseline run id: {}", scenario.baseline_run_id);
    Ok(())
}

/// Immutable inputs shared by every genuine replay a counterfactual
/// intervention or adversarial probe drives, factored out of a loaded
/// `RuntimeConfiguration` so each perturbed rerun only has to name what it
/// actually changes: bars, risk policy, fill model, or entry threshold.
struct ReplayContext<'a> {
    account_id: &'a str,
    currency: &'a str,
    initial_cash: Decimal,
    strategy_id: &'a str,
    strategy_version: &'a str,
    configuration_id: &'a str,
    configuration_version: &'a str,
    configuration_hash: &'a str,
    engine_version: &'a str,
    seed: u64,
    instruments: &'a InstrumentRegistry,
    calendar: &'a StaticTradingCalendar,
    dataset_id: &'a str,
    dataset_version: &'a str,
    reference_data_version: &'a str,
    universe_id: &'a str,
}

impl<'a> ReplayContext<'a> {
    fn from_configuration(
        document: &'a BacktestConfigurationDocument,
        configuration: &'a RuntimeConfiguration,
    ) -> Self {
        Self {
            account_id: &document.account.account_id,
            currency: &document.account.currency,
            initial_cash: configuration.initial_cash,
            strategy_id: &document.strategy.strategy_id,
            strategy_version: &document.strategy.strategy_version,
            configuration_id: &document.configuration_id,
            configuration_version: &document.configuration_version,
            configuration_hash: &configuration.content_hash,
            engine_version: &document.engine_version,
            seed: document.seed,
            instruments: &configuration.instruments,
            calendar: &configuration.calendar,
            dataset_id: &document.dataset.dataset_id,
            dataset_version: &document.dataset.dataset_version,
            reference_data_version: &document.dataset.reference_data_version,
            universe_id: &document.dataset.universe_id,
        }
    }

    /// Executes one complete, independent, single-use replay of the built-in
    /// `BuyOnceStrategy` against the given bars and economics. No corporate
    /// actions are supported at this boundary: a counterfactual or
    /// adversarial rerun perturbs price, cost, timing, or risk-limit inputs,
    /// not corporate-action evidence.
    fn run(
        &self,
        bars: Vec<HistoricalBar>,
        risk_policy: RiskPolicy,
        fill_model: DeterministicFillModel,
        entry_threshold: Decimal,
    ) -> Result<CompletedBacktest, Box<dyn std::error::Error>> {
        let dataset_bars: Vec<_> = bars
            .iter()
            .map(|bar| (bar.event_time.clone(), bar.bar.clone()))
            .collect();
        let dataset = DatasetManifest::from_market_data(
            self.dataset_id,
            self.dataset_version,
            self.reference_data_version,
            self.universe_id,
            &dataset_bars,
            &[],
        )?;
        let starts_at = dataset.starts_at.clone();
        let ends_at = dataset.ends_at.clone();
        let strategy_bundle_hash =
            format!("{:x}", Sha256::digest(BUILTIN_STRATEGY_SOURCE.as_bytes()));
        let spec = BacktestSpec {
            strategy_bundle_hash,
            dataset,
            configuration_id: self.configuration_id.to_owned(),
            configuration_version: self.configuration_version.to_owned(),
            configuration_hash: self.configuration_hash.to_owned(),
            seed: self.seed,
            engine_version: self.engine_version.to_owned(),
            starts_at: starts_at.clone(),
            ends_at,
        };
        let market = MarketPreconditions {
            instruments: self.instruments,
            calendar: self.calendar,
        };
        let mut runner = BacktestRunner::new(
            spec,
            ReplayEngine::new(
                starts_at,
                self.engine_version,
                self.configuration_version,
                risk_policy,
                fill_model,
            )?,
        )?;
        let input = BacktestInput {
            account_id: self.account_id.to_owned(),
            currency: self.currency.to_owned(),
            initial_cash: self.initial_cash,
            bars,
            corporate_actions: Vec::new(),
        };
        let mut strategy = BuyOnceStrategy::new(
            self.account_id,
            self.strategy_id,
            self.strategy_version,
            self.configuration_version,
            entry_threshold,
        );
        Ok(runner.run(&mut strategy, &input, &market)?)
    }
}

/// Converts a [`Decimal`] to an exact integer count of `unit_scale`-sized
/// units (for example `DECIMAL_SCALE / 100` for cents, or `DECIMAL_SCALE` for
/// integer basis points already expressed as a whole-number `Decimal`),
/// truncating any remaining fraction.
fn decimal_to_scaled_i64(
    value: Decimal,
    unit_scale: i128,
) -> Result<i64, Box<dyn std::error::Error>> {
    i64::try_from(value.scaled() / unit_scale).map_err(|_| "decimal value out of i64 range".into())
}

/// Counts `risk.decision.v1` canonical events carrying `"approved":false`,
/// the exact rejection marker every real pre-trade risk decision emits.
fn count_risk_rejections(canonical_events: &[String]) -> i64 {
    canonical_events
        .iter()
        .filter(|event| {
            event.contains("\"event_type\":\"risk.decision.v1\"")
                && event.contains("\"approved\":false")
        })
        .count() as i64
}

/// Real fill count, total (realized plus unrealized) P&L in cents, maximum
/// drawdown in basis points, and risk-rejection count derived from one
/// genuinely completed replay.
fn summarize_run(
    completed: &CompletedBacktest,
) -> Result<(i64, i64, i64, i64), Box<dyn std::error::Error>> {
    let fills = i64::try_from(completed.artifact.performance.trade_count)
        .map_err(|_| "trade count out of i64 range")?;
    let total_pnl = completed
        .artifact
        .report
        .realized_pnl
        .checked_add(completed.artifact.report.unrealized_pnl)?;
    let pnl_cents = decimal_to_scaled_i64(total_pnl, DECIMAL_SCALE / 100)?;
    let drawdown_bps = decimal_to_scaled_i64(
        completed.artifact.performance.max_drawdown_bps,
        DECIMAL_SCALE,
    )?;
    let rejections = count_risk_rejections(&completed.canonical_events);
    Ok((fills, pnl_cents, drawdown_bps, rejections))
}

/// Applies a basis-point shift to a [`Decimal`]: `10_000 + bps` parts per
/// ten-thousand of the original value. A negative `bps` shrinks it.
fn shift_by_bps(value: Decimal, bps: i64) -> Result<Decimal, Box<dyn std::error::Error>> {
    let multiplier =
        Decimal::from_integer(10_000 + bps)?.checked_div(Decimal::from_integer(10_000)?)?;
    Ok(value.checked_mul(multiplier)?)
}

/// Scales every OHLC field of every bar from `start_index` onward by the same
/// `shock_bps` basis-point factor, preserving each bar's internal ordering
/// (open/close within [low, high]) exactly because all four fields move by
/// the identical positive multiplier.
fn shock_bars_from(
    bars: &[HistoricalBar],
    start_index: usize,
    shock_bps: i64,
) -> Result<Vec<HistoricalBar>, Box<dyn std::error::Error>> {
    let mut result = bars.to_vec();
    for historical in result.iter_mut().skip(start_index) {
        historical.bar.open = shift_by_bps(historical.bar.open, shock_bps)?;
        historical.bar.high = shift_by_bps(historical.bar.high, shock_bps)?;
        historical.bar.low = shift_by_bps(historical.bar.low, shock_bps)?;
        historical.bar.close = shift_by_bps(historical.bar.close, shock_bps)?;
        historical.bar.validate()?;
    }
    Ok(result)
}

/// Removes `count` consecutive bars starting at `start_index`, simulating a
/// missing-data gap. At least one bar must remain.
fn drop_bars(
    bars: &[HistoricalBar],
    start_index: usize,
    count: usize,
) -> Result<Vec<HistoricalBar>, Box<dyn std::error::Error>> {
    let mut result = bars.to_vec();
    let start = start_index.min(result.len());
    let end = start.saturating_add(count).min(result.len());
    result.drain(start..end);
    if result.is_empty() {
        return Err("DATA_BAR_CORRUPTION would remove every bar".into());
    }
    Ok(result)
}

/// Deterministic pseudo-random basis-point offset in `[-magnitude, magnitude]`
/// for one bar, derived from a scenario seed and the bar's index so the same
/// seed always reproduces byte-identical perturbed bars.
fn deterministic_bps_offset(seed: u64, index: usize, magnitude_bps: i64) -> i64 {
    if magnitude_bps <= 0 {
        return 0;
    }
    let digest = Sha256::digest(format!("{seed}:{index}").as_bytes());
    let raw = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    let span = u32::try_from(2 * magnitude_bps + 1).unwrap_or(u32::MAX);
    i64::from(raw % span) - magnitude_bps
}

/// Applies an independent deterministic +/- `magnitude_bps` noise offset to
/// every bar's OHLC fields, simulating microstructure jitter.
fn jitter_bars(
    bars: &[HistoricalBar],
    seed: u64,
    magnitude_bps: i64,
) -> Result<Vec<HistoricalBar>, Box<dyn std::error::Error>> {
    let mut result = bars.to_vec();
    for (index, historical) in result.iter_mut().enumerate() {
        let offset = deterministic_bps_offset(seed, index, magnitude_bps);
        historical.bar.open = shift_by_bps(historical.bar.open, offset)?;
        historical.bar.high = shift_by_bps(historical.bar.high, offset)?;
        historical.bar.low = shift_by_bps(historical.bar.low, offset)?;
        historical.bar.close = shift_by_bps(historical.bar.close, offset)?;
        historical.bar.validate()?;
    }
    Ok(result)
}

/// Applies a `RISK_COLLAR_ADJUSTMENT` intervention to a cloned risk policy.
/// `parameter_name` selects which pre-trade limit changes; every other field
/// keeps its baseline value.
fn apply_risk_override(
    base: &RiskPolicy,
    parameter_name: &str,
    counterfactual_value: &str,
) -> Result<RiskPolicy, Box<dyn std::error::Error>> {
    let mut policy = base.clone();
    match parameter_name {
        "max_quantity" => policy.max_quantity = decimal(counterfactual_value)?,
        "max_notional" => policy.max_notional = decimal(counterfactual_value)?,
        "max_price_deviation_bps" => policy.max_price_deviation_bps = decimal(counterfactual_value)?,
        "global_kill_switch" => {
            policy.global_kill_switch = counterfactual_value.parse::<bool>().map_err(|_| {
                "RISK_COLLAR_ADJUSTMENT global_kill_switch counterfactual_value must be true or false"
            })?
        }
        other => return Err(format!("unknown RISK_COLLAR_ADJUSTMENT parameter_name: {other}").into()),
    }
    policy.validate()?;
    Ok(policy)
}

/// Applies a `NETWORK_LATENCY_INJECTION` intervention to a cloned fill model.
fn apply_latency_override(
    base: &DeterministicFillModel,
    counterfactual_value: &str,
) -> Result<DeterministicFillModel, Box<dyn std::error::Error>> {
    let mut model = base.clone();
    model.latency_bars = counterfactual_value.parse::<u32>().map_err(|_| {
        "NETWORK_LATENCY_INJECTION counterfactual_value must be a non-negative integer bar count"
    })?;
    model.validate()?;
    Ok(model)
}

/// Doubles slippage and the flat fee, used by `TRANSACTION_COST_SHOCK`.
fn double_costs(
    base: &DeterministicFillModel,
) -> Result<DeterministicFillModel, Box<dyn std::error::Error>> {
    let mut model = base.clone();
    let two = Decimal::from_integer(2)?;
    model.slippage_bps = model.slippage_bps.checked_mul(two)?;
    model.flat_fee = model.flat_fee.checked_mul(two)?;
    model.validate()?;
    Ok(model)
}

/// Resolves a path declared inside a counterfactual/adversarial config file
/// relative to that config file's own directory, so a checked-in fixture
/// works regardless of the caller's working directory.
fn resolve_relative(base_dir: &Path, declared: &str) -> PathBuf {
    base_dir.join(declared)
}

/// Actually executes a counterfactual scenario: one unperturbed baseline
/// replay and one replay with every declared intervention applied, both
/// through the exact same deterministic `BuyOnceStrategy` kernel used by
/// `follon-backtest run`. Returns the real fill/P&L/drawdown/rejection
/// figures `CounterfactualEngine::evaluate_scenario` certifies.
///
/// `RISK_COLLAR_ADJUSTMENT`, `NETWORK_LATENCY_INJECTION`, `DATA_BAR_CORRUPTION`,
/// and `VOLATILITY_SHOCK` interventions are applied in the order declared;
/// `DATA_BAR_CORRUPTION`/`VOLATILITY_SHOCK` locate their starting bar
/// deterministically from the scenario's `seed`. Only the built-in strategy
/// is supported — a Python-worker-driven backtest still requires
/// operator-attested `metrics`/`delta_metrics`.
fn execute_counterfactual_scenario(
    base_dir: &Path,
    execute: &CounterfactualExecuteDocument,
    interventions: &[CounterfactualIntervention],
    seed: u64,
) -> Result<CounterfactualMetricsDocument, Box<dyn std::error::Error>> {
    let configuration =
        load_runtime_configuration(&resolve_relative(base_dir, &execute.backtest_configuration))?;
    let document = &configuration.document;
    let bars = import_historical_bars(&fs::read_to_string(resolve_relative(
        base_dir,
        &execute.historical_bars,
    ))?)?;
    let context = ReplayContext::from_configuration(document, &configuration);

    let baseline = context.run(
        bars.clone(),
        configuration.risk_policy.clone(),
        configuration.fill_model.clone(),
        configuration.entry_threshold,
    )?;

    let mut risk_policy = configuration.risk_policy.clone();
    let mut fill_model = configuration.fill_model.clone();
    let mut counterfactual_bars = bars.clone();
    for intervention in interventions {
        match intervention.intervention_type {
            CounterfactualInterventionType::RiskCollarAdjustment => {
                risk_policy = apply_risk_override(
                    &risk_policy,
                    &intervention.parameter_name,
                    &intervention.counterfactual_value,
                )?;
            }
            CounterfactualInterventionType::NetworkLatencyInjection => {
                fill_model =
                    apply_latency_override(&fill_model, &intervention.counterfactual_value)?;
            }
            CounterfactualInterventionType::DataBarCorruption => {
                let count: usize = intervention.counterfactual_value.parse().map_err(|_| {
                    "DATA_BAR_CORRUPTION counterfactual_value must be a non-negative integer bar count"
                })?;
                let start_index = (seed as usize) % counterfactual_bars.len();
                counterfactual_bars = drop_bars(&counterfactual_bars, start_index, count)?;
            }
            CounterfactualInterventionType::VolatilityShock => {
                let shock_bps: i64 = intervention.counterfactual_value.parse().map_err(|_| {
                    "VOLATILITY_SHOCK counterfactual_value must be a signed integer basis-point shock"
                })?;
                let start_index = (seed as usize) % counterfactual_bars.len();
                counterfactual_bars =
                    shock_bars_from(&counterfactual_bars, start_index, shock_bps)?;
            }
        }
    }
    let counterfactual = context.run(
        counterfactual_bars,
        risk_policy,
        fill_model,
        configuration.entry_threshold,
    )?;

    let (baseline_fills, baseline_pnl_cents, baseline_max_drawdown_bps, baseline_rejections) =
        summarize_run(&baseline)?;
    let (
        counterfactual_fills,
        counterfactual_pnl_cents,
        counterfactual_max_drawdown_bps,
        counterfactual_rejections,
    ) = summarize_run(&counterfactual)?;
    Ok(CounterfactualMetricsDocument {
        baseline_fills,
        counterfactual_fills,
        baseline_pnl_cents,
        counterfactual_pnl_cents,
        baseline_max_drawdown_bps,
        counterfactual_max_drawdown_bps,
        baseline_rejections,
        counterfactual_rejections,
    })
}

/// Actually executes the 5 standardized adversarial stress probes against a
/// real deterministic backtest, returning genuine `passed`/`degradation_bps`
/// results for `AdversarialResearchGate::evaluate_probes` to certify. Only
/// the built-in strategy is supported — a Python-worker-driven backtest
/// still requires operator-attested `probes` results.
fn execute_adversarial_probes(
    base_dir: &Path,
    execute: &AdversarialExecuteDocument,
) -> Result<Vec<AdversarialProbeResult>, Box<dyn std::error::Error>> {
    if execute.probes.len() < 5 {
        return Err("adversarial execute requires all 5 standardized stress probes".into());
    }
    let configuration =
        load_runtime_configuration(&resolve_relative(base_dir, &execute.backtest_configuration))?;
    let document = &configuration.document;
    let bars = import_historical_bars(&fs::read_to_string(resolve_relative(
        base_dir,
        &execute.historical_bars,
    ))?)?;
    let context = ReplayContext::from_configuration(document, &configuration);

    let baseline = context.run(
        bars.clone(),
        configuration.risk_policy.clone(),
        configuration.fill_model.clone(),
        configuration.entry_threshold,
    )?;
    let baseline_return_bps =
        decimal_to_scaled_i64(baseline.artifact.performance.return_bps, DECIMAL_SCALE)?;

    let mut results = Vec::with_capacity(execute.probes.len());
    for probe in &execute.probes {
        let degradation_bps = match probe.probe_name.as_str() {
            "LOOKAHEAD_LEAKAGE_PROBE" => {
                run_lookahead_probe(&context, &configuration, &bars, &baseline)?
            }
            "PRICE_JITTER_PROBE" => run_jitter_probe(
                &context,
                &configuration,
                &bars,
                execute.seed,
                baseline_return_bps,
            )?,
            "TRANSACTION_COST_SHOCK" => {
                run_cost_shock_probe(&context, &configuration, &bars, baseline_return_bps)?
            }
            "PARAMETER_CLIFF_PROBE" => run_parameter_cliff_probe(&context, &configuration, &bars)?,
            "REGIME_STRESS_PROBE" => run_regime_stress_probe(
                &context,
                &configuration,
                &bars,
                execute.seed,
                baseline_return_bps,
            )?,
            other => {
                return Err(
                    format!("unknown standardized probe_name for execute mode: {other}").into(),
                )
            }
        };
        results.push(AdversarialProbeResult {
            probe_name: probe.probe_name.clone(),
            probe_description: probe.probe_description.clone(),
            passed: degradation_bps <= probe.threshold_bps,
            degradation_bps,
            threshold_bps: probe.threshold_bps,
        });
    }
    Ok(results)
}

/// Reruns the replay against the first three-quarters of the bars only and
/// compares its equity curve against the full baseline's over that shared
/// prefix. A real look-ahead leak would move an earlier decision when later
/// bars are removed, showing up as a nonzero divergence; the engine's
/// bar-by-bar construction makes that structurally impossible, so this
/// proves the invariant on this corpus rather than assuming it.
fn run_lookahead_probe(
    context: &ReplayContext<'_>,
    configuration: &RuntimeConfiguration,
    bars: &[HistoricalBar],
    baseline: &CompletedBacktest,
) -> Result<i64, Box<dyn std::error::Error>> {
    let keep = (bars.len() * 3 / 4).max(1);
    if keep >= bars.len() {
        return Err(
            "LOOKAHEAD_LEAKAGE_PROBE requires enough bars to truncate a quarter of them".into(),
        );
    }
    let truncated_bars = bars[..keep].to_vec();
    let truncated = context.run(
        truncated_bars,
        configuration.risk_policy.clone(),
        configuration.fill_model.clone(),
        configuration.entry_threshold,
    )?;
    let mut max_bps: i64 = 0;
    for point in &truncated.artifact.performance.equity_curve {
        let Some(full_point) = baseline
            .artifact
            .performance
            .equity_curve
            .iter()
            .find(|candidate| candidate.event_time == point.event_time)
        else {
            continue;
        };
        let diff = if full_point.total_equity >= point.total_equity {
            full_point.total_equity.checked_sub(point.total_equity)?
        } else {
            point.total_equity.checked_sub(full_point.total_equity)?
        };
        let denominator = if point.total_equity > Decimal::ZERO {
            point.total_equity
        } else {
            Decimal::from_integer(1)?
        };
        let bps = diff
            .checked_mul(Decimal::from_integer(10_000)?)?
            .checked_div(denominator)?;
        max_bps = max_bps.max(decimal_to_scaled_i64(bps, DECIMAL_SCALE)?);
    }
    Ok(max_bps)
}

/// Reruns the replay with deterministic +/-20bps per-bar microstructure
/// noise and measures the return degradation against the baseline.
fn run_jitter_probe(
    context: &ReplayContext<'_>,
    configuration: &RuntimeConfiguration,
    bars: &[HistoricalBar],
    seed: u64,
    baseline_return_bps: i64,
) -> Result<i64, Box<dyn std::error::Error>> {
    const JITTER_MAGNITUDE_BPS: i64 = 20;
    let jittered_bars = jitter_bars(bars, seed, JITTER_MAGNITUDE_BPS)?;
    let jittered = context.run(
        jittered_bars,
        configuration.risk_policy.clone(),
        configuration.fill_model.clone(),
        configuration.entry_threshold,
    )?;
    let jittered_return_bps =
        decimal_to_scaled_i64(jittered.artifact.performance.return_bps, DECIMAL_SCALE)?;
    Ok((baseline_return_bps - jittered_return_bps).max(0))
}

/// Reruns the replay with slippage and the flat fee doubled and measures the
/// return degradation against the baseline.
fn run_cost_shock_probe(
    context: &ReplayContext<'_>,
    configuration: &RuntimeConfiguration,
    bars: &[HistoricalBar],
    baseline_return_bps: i64,
) -> Result<i64, Box<dyn std::error::Error>> {
    let costed_fill_model = double_costs(&configuration.fill_model)?;
    let costed = context.run(
        bars.to_vec(),
        configuration.risk_policy.clone(),
        costed_fill_model,
        configuration.entry_threshold,
    )?;
    let costed_return_bps =
        decimal_to_scaled_i64(costed.artifact.performance.return_bps, DECIMAL_SCALE)?;
    Ok((baseline_return_bps - costed_return_bps).max(0))
}

/// Reruns the replay with the entry threshold shifted +/-10bps and measures
/// the largest resulting return swing, revealing whether the configured
/// threshold sits on a fragile decision boundary.
fn run_parameter_cliff_probe(
    context: &ReplayContext<'_>,
    configuration: &RuntimeConfiguration,
    bars: &[HistoricalBar],
) -> Result<i64, Box<dyn std::error::Error>> {
    const NEIGHBOR_SHIFT_BPS: i64 = 10;
    let baseline = context.run(
        bars.to_vec(),
        configuration.risk_policy.clone(),
        configuration.fill_model.clone(),
        configuration.entry_threshold,
    )?;
    let baseline_return_bps =
        decimal_to_scaled_i64(baseline.artifact.performance.return_bps, DECIMAL_SCALE)?;
    let mut max_swing: i64 = 0;
    for shift in [NEIGHBOR_SHIFT_BPS, -NEIGHBOR_SHIFT_BPS] {
        let neighbor_threshold = shift_by_bps(configuration.entry_threshold, shift)?;
        let neighbor = context.run(
            bars.to_vec(),
            configuration.risk_policy.clone(),
            configuration.fill_model.clone(),
            neighbor_threshold,
        )?;
        let neighbor_return_bps =
            decimal_to_scaled_i64(neighbor.artifact.performance.return_bps, DECIMAL_SCALE)?;
        max_swing = max_swing.max((baseline_return_bps - neighbor_return_bps).abs());
    }
    Ok(max_swing)
}

/// Reruns the replay with a -15% price shock applied to every bar from a
/// seed-selected point onward, simulating a sudden regime change, and
/// measures the return degradation against the baseline.
fn run_regime_stress_probe(
    context: &ReplayContext<'_>,
    configuration: &RuntimeConfiguration,
    bars: &[HistoricalBar],
    seed: u64,
    baseline_return_bps: i64,
) -> Result<i64, Box<dyn std::error::Error>> {
    const REGIME_SHOCK_BPS: i64 = -1_500;
    let start_index = (bars.len() / 2) + ((seed as usize) % bars.len().max(1).div_ceil(2).max(1));
    let start_index = start_index.min(bars.len().saturating_sub(1));
    let shocked_bars = shock_bars_from(bars, start_index, REGIME_SHOCK_BPS)?;
    let shocked = context.run(
        shocked_bars,
        configuration.risk_policy.clone(),
        configuration.fill_model.clone(),
        configuration.entry_threshold,
    )?;
    let shocked_return_bps =
        decimal_to_scaled_i64(shocked.artifact.performance.return_bps, DECIMAL_SCALE)?;
    Ok((baseline_return_bps - shocked_return_bps).max(0))
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
                    sandbox: None,
                };
                strategy_mode = StrategyMode::Python(Box::new(worker));
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

    #[test]
    fn adversarial_subcommand_processes_fixture_and_emits_valid_json() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/adversarial-v1.json");
        let output_path = std::env::temp_dir().join(format!(
            "follon-adversarial-test-{}.json",
            std::process::id()
        ));
        let args = vec![
            fixture.to_str().unwrap().to_owned(),
            output_path.to_str().unwrap().to_owned(),
        ];
        run_adversarial(&args).unwrap();
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("\"adversarial_schema_version\":1"));
        assert!(content.contains("\"LOOKAHEAD_LEAKAGE_PROBE\""));
        assert!(content.contains("\"gate_passed\":true"));
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn counterfactual_subcommand_processes_fixture_and_emits_valid_json() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/counterfactual-v1.json");
        let output_path = std::env::temp_dir().join(format!(
            "follon-counterfactual-test-{}.json",
            std::process::id()
        ));
        let args = vec![
            fixture.to_str().unwrap().to_owned(),
            output_path.to_str().unwrap().to_owned(),
        ];
        run_counterfactual(&args).unwrap();
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("\"scenario_schema_version\":1"));
        assert!(content.contains("\"cf.latency-shock.001\""));
        assert!(content.contains("\"NETWORK_LATENCY_INJECTION\""));
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn counterfactual_execute_mode_runs_a_real_intervention_and_computes_genuine_deltas() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/counterfactual-execute-v1.json");
        let output_path = std::env::temp_dir().join(format!(
            "follon-counterfactual-execute-test-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&output_path);
        let args = vec![
            fixture.to_str().unwrap().to_owned(),
            output_path.to_str().unwrap().to_owned(),
        ];
        run_counterfactual(&args).unwrap();
        let content = std::fs::read_to_string(&output_path).unwrap();
        // A real ~$100 buy is rejected once max_notional drops to $50: the
        // baseline's one fill and small unrealized gain disappear, and a
        // genuine MAX_NOTIONAL_EXCEEDED risk rejection is recorded — not a
        // caller-supplied number.
        assert!(content.contains("\"fill_count_delta\":-1"));
        assert!(content.contains("\"risk_rejection_count_delta\":1"));
        assert!(content.contains("\"max_drawdown_delta_bps\":-53"));
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn counterfactual_execute_mode_rejects_an_unknown_risk_parameter() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/counterfactual-execute-v1.json");
        let mut document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&fixture).unwrap()).unwrap();
        document["interventions"][0]["parameter_name"] =
            serde_json::Value::String("not_a_real_risk_parameter".to_owned());
        let fixtures_dir = fixture.parent().unwrap();
        document["execute"]["backtest_configuration"] = serde_json::Value::String(
            fixtures_dir
                .join("backtest-probe-v1.json")
                .to_str()
                .unwrap()
                .to_owned(),
        );
        document["execute"]["historical_bars"] = serde_json::Value::String(
            fixtures_dir
                .join("../historical-bars/probe-corpus-one-minute.csv")
                .to_str()
                .unwrap()
                .to_owned(),
        );
        let config_path = std::env::temp_dir().join(format!(
            "follon-counterfactual-bad-parameter-{}.json",
            std::process::id()
        ));
        std::fs::write(&config_path, serde_json::to_vec(&document).unwrap()).unwrap();
        let output_path = std::env::temp_dir().join(format!(
            "follon-counterfactual-bad-parameter-out-{}.json",
            std::process::id()
        ));
        let args = vec![
            config_path.to_str().unwrap().to_owned(),
            output_path.to_str().unwrap().to_owned(),
        ];
        let error = run_counterfactual(&args).unwrap_err();
        assert!(error
            .to_string()
            .contains("unknown RISK_COLLAR_ADJUSTMENT parameter_name"));
        let _ = std::fs::remove_file(&config_path);
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn adversarial_execute_mode_runs_real_probes_and_computes_genuine_degradation() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/adversarial-execute-v1.json");
        let output_path = std::env::temp_dir().join(format!(
            "follon-adversarial-execute-test-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&output_path);
        let args = vec![
            fixture.to_str().unwrap().to_owned(),
            output_path.to_str().unwrap().to_owned(),
        ];
        run_adversarial(&args).unwrap();
        let content = std::fs::read_to_string(&output_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let probes = parsed["probes"].as_array().unwrap();
        let degradation = |name: &str| -> i64 {
            probes
                .iter()
                .find(|probe| probe["probe_name"] == name)
                .unwrap()["degradation_bps"]
                .as_i64()
                .unwrap()
        };
        // A real look-ahead leak would move the truncated run's equity curve
        // away from the full run's over their shared prefix; the engine's
        // bar-by-bar construction makes that impossible, so this is a
        // genuine, computed zero rather than a hardcoded pass.
        assert_eq!(degradation("LOOKAHEAD_LEAKAGE_PROBE"), 0);
        // Fills round up onto the cent grid (E3.6e): 100.28016 -> 100.29 at
        // base costs and 100.48032 -> 100.49 doubled. Each run's return
        // truncates to whole bps, so this is 19 - 2; unrounded it was 19 - 3.
        assert_eq!(degradation("TRANSACTION_COST_SHOCK"), 17);
        assert_eq!(degradation("PARAMETER_CLIFF_PROBE"), 19);
        assert_eq!(degradation("REGIME_STRESS_PROBE"), 1005);
        assert_eq!(parsed["gate_passed"], true);
        assert_eq!(parsed["composite_robustness_score_bps"], 10_000);
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn adversarial_execute_mode_fails_the_gate_when_real_degradation_exceeds_the_operators_threshold(
    ) {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/adversarial-execute-v1.json");
        let mut document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&fixture).unwrap()).unwrap();
        // The real REGIME_STRESS_PROBE measurement is 1005bps (see the test
        // above); tightening its allowed threshold below that must fail the
        // gate on a genuine measured number, not a caller-supplied one.
        document["execute"]["probes"][4]["threshold_bps"] = serde_json::Value::from(100);
        let fixtures_dir = fixture.parent().unwrap();
        document["execute"]["backtest_configuration"] = serde_json::Value::String(
            fixtures_dir
                .join("backtest-probe-v1.json")
                .to_str()
                .unwrap()
                .to_owned(),
        );
        document["execute"]["historical_bars"] = serde_json::Value::String(
            fixtures_dir
                .join("../historical-bars/probe-corpus-one-minute.csv")
                .to_str()
                .unwrap()
                .to_owned(),
        );
        let config_path = std::env::temp_dir().join(format!(
            "follon-adversarial-strict-threshold-{}.json",
            std::process::id()
        ));
        std::fs::write(&config_path, serde_json::to_vec(&document).unwrap()).unwrap();
        let output_path = std::env::temp_dir().join(format!(
            "follon-adversarial-strict-threshold-out-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&output_path);
        let args = vec![
            config_path.to_str().unwrap().to_owned(),
            output_path.to_str().unwrap().to_owned(),
        ];
        run_adversarial(&args).unwrap();
        let content = std::fs::read_to_string(&output_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["gate_passed"], false);
        assert!(parsed["blocking_failure_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("REGIME_STRESS_PROBE")));
        let _ = std::fs::remove_file(&config_path);
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn jitter_bars_perturbs_every_bar_but_keeps_ohlc_valid() {
        let fixture_csv = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/historical-bars/probe-corpus-one-minute.csv"),
        )
        .unwrap();
        let bars = import_historical_bars(&fixture_csv).unwrap();
        let jittered = jitter_bars(&bars, 11, 20).unwrap();
        assert_eq!(bars.len(), jittered.len());
        assert!(bars
            .iter()
            .zip(&jittered)
            .any(|(original, perturbed)| original.bar.close != perturbed.bar.close));
        for historical in &jittered {
            historical.bar.validate().unwrap();
        }
    }

    #[test]
    fn shock_bars_from_preserves_ohlc_ordering_under_a_large_negative_shock() {
        let fixture_csv = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/historical-bars/probe-corpus-one-minute.csv"),
        )
        .unwrap();
        let bars = import_historical_bars(&fixture_csv).unwrap();
        let shocked = shock_bars_from(&bars, bars.len() / 2, -1_500).unwrap();
        for historical in shocked.iter().skip(bars.len() / 2) {
            historical.bar.validate().unwrap();
        }
        assert_eq!(
            shocked[0].bar.close, bars[0].bar.close,
            "bars before the shock start index are untouched"
        );
        assert!(shocked[bars.len() / 2].bar.close < bars[bars.len() / 2].bar.close);
    }

    #[test]
    fn drop_bars_removes_exactly_the_requested_range() {
        let fixture_csv = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/historical-bars/probe-corpus-one-minute.csv"),
        )
        .unwrap();
        let bars = import_historical_bars(&fixture_csv).unwrap();
        let dropped = drop_bars(&bars, 10, 5).unwrap();
        assert_eq!(dropped.len(), bars.len() - 5);
        assert_eq!(dropped[9].event_time, bars[9].event_time);
        assert_eq!(dropped[10].event_time, bars[15].event_time);
    }

    #[test]
    fn drop_bars_refuses_to_remove_every_bar() {
        let fixture_csv = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/historical-bars/probe-corpus-one-minute.csv"),
        )
        .unwrap();
        let bars = import_historical_bars(&fixture_csv).unwrap();
        let count = bars.len();
        assert!(drop_bars(&bars, 0, count).is_err());
    }
}
