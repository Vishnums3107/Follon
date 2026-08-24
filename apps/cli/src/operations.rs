//! Deterministic operations-workbench command.
//!
//! The command accepts a versioned local configuration and an explicit UTC
//! `--as-of` instant. It never reads a wall clock, credentials, a broker, or an
//! order-control endpoint. `journal` is the only stateful subcommand and it
//! appends a caller-declared non-secret operational fact to an fsynced hash chain.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_cli::{sha256_text, write_immutable};
use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use follon_operations::{
    apply_schedule_completions, canonical_dashboard_json, derive_schedule_statuses,
    derive_schedule_statuses_with_completions, markdown_report, AttributionCategory,
    AttributionEntry, DailySchedule, JournalEntryInput, JournalInspection, OperationalHealth,
    OperationalJournal, OperationalPosition, OperationsSnapshot, ParameterApproval,
    ParameterControl, ParameterSet, ParameterValue, ReproducibilityStamp, RiskLimits,
    SCHEDULE_COMPLETION_EVENT_TYPE,
};
use serde::Deserialize;

const DEFAULT_CONFIGURATION: &str = "tests/fixtures/config/operations-v1.json";
const DEFAULT_JOURNAL: &str = "var/follon-operations.journal.ndjson";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationsConfigurationDocument {
    schema_version: u32,
    environment: String,
    account: AccountDocument,
    configuration: ConfigurationDocument,
    reproducibility: ReproducibilityDocument,
    parameters: ParameterSetDocument,
    valuation: ValuationDocument,
    positions: Vec<PositionDocument>,
    risk_limits: RiskLimitsDocument,
    operational_health: OperationalHealthDocument,
    attribution_entries: Vec<AttributionEntryDocument>,
    schedules: Vec<ScheduleDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountDocument {
    account_id: String,
    currency: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigurationDocument {
    configuration_id: String,
    configuration_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReproducibilityDocument {
    strategy_id: String,
    strategy_version: String,
    strategy_bundle_hash: String,
    dataset_id: String,
    dataset_version: String,
    dataset_hash: String,
    replay_event_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterSetDocument {
    parameter_set_id: String,
    revision: String,
    previous_revision: NullableValue<String>,
    previous_parameter_set_fingerprint: NullableValue<String>,
    values: Vec<ParameterValueDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterValueDocument {
    parameter_id: String,
    value: String,
    minimum: String,
    maximum: String,
    control: String,
    approval: NullableValue<ParameterApprovalDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterApprovalDocument {
    approval_id: String,
    requested_by: String,
    approved_by: String,
    approved_at: String,
    approval_subject_hash: String,
    authorization_policy_hash: String,
    approval_evidence_hash: String,
}

/// An explicitly present JSON value or `null`; omission remains a configuration error.
#[derive(Deserialize)]
#[serde(untagged)]
enum NullableValue<T> {
    Value(T),
    Null(()),
}

impl<T> NullableValue<T> {
    fn into_option(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Null(()) => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValuationDocument {
    starting_equity: String,
    cash: String,
    peak_equity: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PositionDocument {
    instrument_id: String,
    quantity: String,
    mark_price: String,
    average_cost: String,
    realized_pnl: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RiskLimitsDocument {
    max_gross_exposure: String,
    max_single_instrument_exposure: String,
    max_drawdown_bps: String,
    max_working_orders: usize,
    max_unknown_orders: usize,
    max_unresolved_incidents: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationalHealthDocument {
    audit_healthy: bool,
    reconciliation_healthy: bool,
    broker_connected: bool,
    active_kill_switches: Vec<String>,
    working_orders: usize,
    unknown_orders: usize,
    unresolved_incidents: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttributionEntryDocument {
    entry_id: String,
    occurred_at: String,
    instrument_id: String,
    category: String,
    amount: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleDocument {
    schedule_id: String,
    purpose: String,
    time_utc: String,
    enabled: bool,
    last_completed_at: NullableValue<String>,
}

enum Command {
    Validate { configuration_path: PathBuf },
    ConfigDiff(ConfigDiffArguments),
    Dashboard(ProjectionArguments),
    Report(ProjectionArguments),
    Schedule(ScheduleArguments),
    CompleteSchedule(CompleteScheduleArguments),
    Journal(JournalArguments),
}

struct ProjectionArguments {
    configuration_path: PathBuf,
    output_path: PathBuf,
    journal_path: PathBuf,
    as_of: String,
}

struct ScheduleArguments {
    configuration_path: PathBuf,
    output_path: PathBuf,
    journal_path: PathBuf,
    as_of: String,
}

struct ConfigDiffArguments {
    previous_path: PathBuf,
    target_path: PathBuf,
    output_path: PathBuf,
}

struct CompleteScheduleArguments {
    configuration_path: PathBuf,
    journal_path: PathBuf,
    schedule_id: String,
    entry_id: String,
    actor: String,
    occurred_at: String,
}

struct JournalArguments {
    journal_path: PathBuf,
    entry_id: String,
    event_type: String,
    actor: String,
    occurred_at: String,
    details: BTreeMap<String, String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match parse_command(env::args().skip(1).collect())? {
        Command::Validate { configuration_path } => {
            let snapshot = load_snapshot(&configuration_path, "9999-12-31T23:59:59Z")?;
            println!(
                "{{\"configuration_content_hash\":{},\"configuration_id\":{},\"configuration_version\":{},\"parameter_set_fingerprint\":{},\"valid\":true}}",
                json_string(&snapshot.configuration_content_hash),
                json_string(&snapshot.configuration_id),
                json_string(&snapshot.configuration_version),
                json_string(&snapshot.parameters.fingerprint()?),
            );
        }
        Command::Dashboard(arguments) => {
            let snapshot = load_snapshot(&arguments.configuration_path, &arguments.as_of)?;
            let (journal, records) = journal_evidence(&arguments.journal_path);
            let snapshot = if journal.healthy {
                apply_schedule_completions(&snapshot, &records)?
            } else {
                snapshot
            };
            let dashboard = canonical_dashboard_json(&snapshot, &journal)?;
            publish(&arguments.output_path, &dashboard)?;
            eprintln!("operations dashboard: {}", arguments.output_path.display());
            eprintln!("journal inspection: {}", arguments.journal_path.display());
        }
        Command::Report(arguments) => {
            let snapshot = load_snapshot(&arguments.configuration_path, &arguments.as_of)?;
            let (journal, records) = journal_evidence(&arguments.journal_path);
            let snapshot = if journal.healthy {
                apply_schedule_completions(&snapshot, &records)?
            } else {
                snapshot
            };
            let report = markdown_report(&snapshot, &journal)?;
            publish(&arguments.output_path, &report)?;
            eprintln!("operations report: {}", arguments.output_path.display());
            eprintln!("journal inspection: {}", arguments.journal_path.display());
        }
        Command::Schedule(arguments) => {
            let snapshot = load_snapshot(&arguments.configuration_path, &arguments.as_of)?;
            let (journal, records) = journal_evidence(&arguments.journal_path);
            if !journal.healthy {
                return Err(format!(
                    "cannot derive a schedule plan from an unhealthy journal: {}",
                    journal
                        .failure_reason
                        .unwrap_or_else(|| "unknown verification failure".to_owned())
                )
                .into());
            }
            let schedules = derive_schedule_statuses_with_completions(&snapshot, &records)?;
            let artifact = canonical_schedule_json(&snapshot, &journal, &schedules)?;
            publish(&arguments.output_path, &artifact)?;
            eprintln!("schedule plan: {}", arguments.output_path.display());
        }
        Command::ConfigDiff(arguments) => {
            let previous = load_snapshot(&arguments.previous_path, "9999-12-31T23:59:59Z")?;
            let target = load_snapshot(&arguments.target_path, "9999-12-31T23:59:59Z")?;
            let artifact = canonical_parameter_change_json(&previous, &target)?;
            publish(&arguments.output_path, &artifact)?;
            eprintln!(
                "parameter revision diff: {}",
                arguments.output_path.display()
            );
        }
        Command::CompleteSchedule(arguments) => {
            let snapshot = load_snapshot(&arguments.configuration_path, &arguments.occurred_at)?;
            let (journal_inspection, records) =
                OperationalJournal::read_verified(&arguments.journal_path)?;
            let projected = apply_schedule_completions(&snapshot, &records)?;
            let schedule = derive_schedule_statuses(&projected)?
                .into_iter()
                .find(|schedule| schedule.schedule_id == arguments.schedule_id)
                .ok_or("--schedule-id does not exist in the selected configuration")?;
            if !schedule.enabled || !schedule.due {
                return Err("the selected schedule is not enabled and due at --occurred-at".into());
            }
            let mut details = BTreeMap::new();
            details.insert("schedule_id".to_owned(), arguments.schedule_id);
            details.insert(
                "configuration_hash".to_owned(),
                snapshot.configuration_content_hash.clone(),
            );
            details.insert(
                "parameter_set_fingerprint".to_owned(),
                snapshot.parameters.fingerprint()?,
            );
            details.insert("scheduled_for".to_owned(), schedule.next_due_at);
            let mut journal = OperationalJournal::open(&arguments.journal_path)?;
            if journal.inspection() != &journal_inspection {
                return Err(
                    "operations journal changed while recording schedule completion; retry".into(),
                );
            }
            let record = journal.append(JournalEntryInput {
                entry_id: arguments.entry_id,
                event_type: SCHEDULE_COMPLETION_EVENT_TYPE.to_owned(),
                occurred_at: arguments.occurred_at,
                actor: arguments.actor,
                details,
            })?;
            println!("{}", record.canonical_json());
            eprintln!(
                "schedule completion journaled: {}",
                journal.path().display()
            );
        }
        Command::Journal(arguments) => {
            if matches!(
                arguments.event_type.as_str(),
                "operations.schedule_completed.v1" | SCHEDULE_COMPLETION_EVENT_TYPE
            ) {
                return Err(
                    "schedule completion is typed evidence; use complete-schedule after the due work finishes"
                        .into(),
                );
            }
            let mut journal = OperationalJournal::open(&arguments.journal_path)?;
            let record = journal.append(JournalEntryInput {
                entry_id: arguments.entry_id,
                event_type: arguments.event_type,
                occurred_at: arguments.occurred_at,
                actor: arguments.actor,
                details: arguments.details,
            })?;
            println!("{}", record.canonical_json());
            eprintln!("operations journal: {}", journal.path().display());
        }
    }
    Ok(())
}

fn parse_command(arguments: Vec<String>) -> Result<Command, Box<dyn std::error::Error>> {
    let Some((command, remainder)) = arguments.split_first() else {
        return Err(usage().into());
    };
    match command.as_str() {
        "validate-config" => {
            if remainder.len() > 1
                || remainder
                    .first()
                    .is_some_and(|value| value.starts_with('-'))
            {
                return Err("usage: follon-operations validate-config [operations.json]".into());
            }
            Ok(Command::Validate {
                configuration_path: remainder
                    .first()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIGURATION)),
            })
        }
        "config-diff" => Ok(Command::ConfigDiff(parse_config_diff_arguments(remainder)?)),
        "dashboard" => Ok(Command::Dashboard(parse_projection_arguments(
            remainder,
            "var/follon-operations-dashboard.json",
        )?)),
        "report" => Ok(Command::Report(parse_projection_arguments(
            remainder,
            "var/follon-operations-report.md",
        )?)),
        "schedule" => Ok(Command::Schedule(parse_schedule_arguments(remainder)?)),
        "complete-schedule" => Ok(Command::CompleteSchedule(
            parse_complete_schedule_arguments(remainder)?,
        )),
        "journal" => Ok(Command::Journal(parse_journal_arguments(remainder)?)),
        _ => Err(usage().into()),
    }
}

fn parse_projection_arguments(
    arguments: &[String],
    default_output: &str,
) -> Result<ProjectionArguments, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut as_of = None;
    let mut journal_path = PathBuf::from(DEFAULT_JOURNAL);
    let mut journal_explicit = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--as-of" => {
                if as_of.is_some() {
                    return Err("--as-of may be specified only once".into());
                }
                index += 1;
                as_of = Some(required(arguments, index, "--as-of")?.to_owned());
            }
            "--journal" => {
                if journal_explicit {
                    return Err("--journal may be specified only once".into());
                }
                index += 1;
                journal_path = PathBuf::from(required(arguments, index, "--journal")?);
                journal_explicit = true;
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported argument: {value}").into())
            }
            value => positional.push(PathBuf::from(value)),
        }
        index += 1;
    }
    if positional.len() > 2 {
        return Err("projection accepts [operations.json] [output]".into());
    }
    let as_of = as_of.ok_or("--as-of is required; use a canonical UTC timestamp")?;
    validate_utc_timestamp("--as-of", &as_of)?;
    Ok(ProjectionArguments {
        configuration_path: positional
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIGURATION)),
        output_path: positional
            .get(1)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(default_output)),
        journal_path,
        as_of,
    })
}

fn parse_schedule_arguments(
    arguments: &[String],
) -> Result<ScheduleArguments, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut as_of = None;
    let mut journal_path = PathBuf::from(DEFAULT_JOURNAL);
    let mut journal_explicit = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--as-of" => {
                if as_of.is_some() {
                    return Err("--as-of may be specified only once".into());
                }
                index += 1;
                as_of = Some(required(arguments, index, "--as-of")?.to_owned());
            }
            "--journal" => {
                if journal_explicit {
                    return Err("--journal may be specified only once".into());
                }
                index += 1;
                journal_path = PathBuf::from(required(arguments, index, "--journal")?);
                journal_explicit = true;
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported argument: {value}").into())
            }
            value => positional.push(PathBuf::from(value)),
        }
        index += 1;
    }
    if positional.len() > 2 {
        return Err("schedule accepts [operations.json] [output]".into());
    }
    let as_of = as_of.ok_or("--as-of is required; use a canonical UTC timestamp")?;
    validate_utc_timestamp("--as-of", &as_of)?;
    Ok(ScheduleArguments {
        configuration_path: positional
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIGURATION)),
        output_path: positional
            .get(1)
            .cloned()
            .unwrap_or_else(|| PathBuf::from("var/follon-operations-schedule.json")),
        journal_path,
        as_of,
    })
}

fn parse_config_diff_arguments(
    arguments: &[String],
) -> Result<ConfigDiffArguments, Box<dyn std::error::Error>> {
    if !(2..=3).contains(&arguments.len()) || arguments.iter().any(|value| value.starts_with('-')) {
        return Err(
            "usage: follon-operations config-diff <previous operations.json> <target operations.json> [changes.json]"
                .into(),
        );
    }
    Ok(ConfigDiffArguments {
        previous_path: PathBuf::from(&arguments[0]),
        target_path: PathBuf::from(&arguments[1]),
        output_path: arguments
            .get(2)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("var/follon-parameter-changes.json")),
    })
}

fn parse_complete_schedule_arguments(
    arguments: &[String],
) -> Result<CompleteScheduleArguments, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut journal_path = PathBuf::from(DEFAULT_JOURNAL);
    let mut journal_explicit = false;
    let mut schedule_id = None;
    let mut entry_id = None;
    let mut actor = None;
    let mut occurred_at = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--journal" => {
                if journal_explicit {
                    return Err("--journal may be specified only once".into());
                }
                index += 1;
                journal_path = PathBuf::from(required(arguments, index, "--journal")?);
                journal_explicit = true;
            }
            "--schedule-id" => assign_once(
                &mut schedule_id,
                required(arguments, index + 1, "--schedule-id")?,
                "--schedule-id",
            )?,
            "--entry-id" => assign_once(
                &mut entry_id,
                required(arguments, index + 1, "--entry-id")?,
                "--entry-id",
            )?,
            "--actor" => assign_once(
                &mut actor,
                required(arguments, index + 1, "--actor")?,
                "--actor",
            )?,
            "--occurred-at" => assign_once(
                &mut occurred_at,
                required(arguments, index + 1, "--occurred-at")?,
                "--occurred-at",
            )?,
            value if value.starts_with('-') => {
                return Err(format!("unsupported argument: {value}").into())
            }
            value => positional.push(PathBuf::from(value)),
        }
        if matches!(
            arguments[index].as_str(),
            "--schedule-id" | "--entry-id" | "--actor" | "--occurred-at"
        ) {
            index += 1;
        }
        index += 1;
    }
    if positional.len() > 1 {
        return Err("complete-schedule accepts at most one operations.json argument".into());
    }
    let schedule_id = schedule_id.ok_or("--schedule-id is required")?;
    let entry_id = entry_id.ok_or("--entry-id is required")?;
    let actor = actor.ok_or("--actor is required")?;
    let occurred_at = occurred_at.ok_or("--occurred-at is required")?;
    for (name, value) in [
        ("--schedule-id", schedule_id.as_str()),
        ("--entry-id", entry_id.as_str()),
        ("--actor", actor.as_str()),
    ] {
        validate_canonical_id(name, value)?;
    }
    validate_utc_timestamp("--occurred-at", &occurred_at)?;
    Ok(CompleteScheduleArguments {
        configuration_path: positional
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIGURATION)),
        journal_path,
        schedule_id,
        entry_id,
        actor,
        occurred_at,
    })
}

fn parse_journal_arguments(
    arguments: &[String],
) -> Result<JournalArguments, Box<dyn std::error::Error>> {
    let mut journal_path = PathBuf::from(DEFAULT_JOURNAL);
    let mut entry_id = None;
    let mut event_type = None;
    let mut actor = None;
    let mut occurred_at = None;
    let mut details = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--journal" => {
                index += 1;
                journal_path = PathBuf::from(required(arguments, index, "--journal")?);
            }
            "--entry-id" => assign_once(
                &mut entry_id,
                required(arguments, index + 1, "--entry-id")?,
                "--entry-id",
            )?,
            "--event-type" => assign_once(
                &mut event_type,
                required(arguments, index + 1, "--event-type")?,
                "--event-type",
            )?,
            "--actor" => assign_once(
                &mut actor,
                required(arguments, index + 1, "--actor")?,
                "--actor",
            )?,
            "--occurred-at" => assign_once(
                &mut occurred_at,
                required(arguments, index + 1, "--occurred-at")?,
                "--occurred-at",
            )?,
            "--detail" => {
                let detail = required(arguments, index + 1, "--detail")?;
                let Some((key, value)) = detail.split_once('=') else {
                    return Err("--detail must use canonical_key=value".into());
                };
                if key.is_empty()
                    || value.is_empty()
                    || details.insert(key.to_owned(), value.to_owned()).is_some()
                {
                    return Err("--detail keys must be unique and have non-empty values".into());
                }
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported argument: {value}").into())
            }
            _ => return Err("journal accepts only named arguments".into()),
        }
        if matches!(
            arguments[index].as_str(),
            "--entry-id" | "--event-type" | "--actor" | "--occurred-at" | "--detail"
        ) {
            index += 1;
        }
        index += 1;
    }
    let entry_id = entry_id.ok_or("--entry-id is required")?;
    let actor = actor.ok_or("--actor is required")?;
    validate_canonical_id("--entry-id", &entry_id)?;
    validate_canonical_id("--actor", &actor)?;
    let occurred_at = occurred_at.ok_or("--occurred-at is required")?;
    validate_utc_timestamp("--occurred-at", &occurred_at)?;
    Ok(JournalArguments {
        journal_path,
        entry_id,
        event_type: event_type.ok_or("--event-type is required")?,
        actor,
        occurred_at,
        details,
    })
}

fn assign_once(
    target: &mut Option<String>,
    value: &str,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if target.replace(value.to_owned()).is_some() {
        return Err(format!("{name} may be specified only once").into());
    }
    Ok(())
}

fn required<'a>(
    arguments: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn load_snapshot(
    path: &Path,
    as_of: &str,
) -> Result<OperationsSnapshot, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err("operations configuration must be between 1 byte and 1 MiB".into());
    }
    let document: OperationsConfigurationDocument = serde_json::from_slice(&bytes)?;
    if document.schema_version != 1 {
        return Err("unsupported operations configuration schema version".into());
    }
    let snapshot = OperationsSnapshot {
        as_of: as_of.to_owned(),
        environment: document.environment,
        account_id: document.account.account_id,
        currency: document.account.currency,
        configuration_id: document.configuration.configuration_id,
        configuration_version: document.configuration.configuration_version,
        configuration_content_hash: sha256_text(&String::from_utf8(bytes)?),
        reproducibility: ReproducibilityStamp {
            strategy_id: document.reproducibility.strategy_id,
            strategy_version: document.reproducibility.strategy_version,
            strategy_bundle_hash: document.reproducibility.strategy_bundle_hash,
            dataset_id: document.reproducibility.dataset_id,
            dataset_version: document.reproducibility.dataset_version,
            dataset_hash: document.reproducibility.dataset_hash,
            replay_event_hash: document.reproducibility.replay_event_hash,
        },
        parameters: ParameterSet {
            parameter_set_id: document.parameters.parameter_set_id,
            revision: document.parameters.revision,
            previous_revision: document.parameters.previous_revision.into_option(),
            previous_parameter_set_fingerprint: document
                .parameters
                .previous_parameter_set_fingerprint
                .into_option(),
            values: document
                .parameters
                .values
                .into_iter()
                .map(|value| {
                    Ok(ParameterValue {
                        parameter_id: value.parameter_id,
                        value: decimal(&value.value)?,
                        minimum: decimal(&value.minimum)?,
                        maximum: decimal(&value.maximum)?,
                        control: parse_parameter_control(&value.control)?,
                        approval: value
                            .approval
                            .into_option()
                            .map(|approval| ParameterApproval {
                                approval_id: approval.approval_id,
                                requested_by: approval.requested_by,
                                approved_by: approval.approved_by,
                                approved_at: approval.approved_at,
                                approval_subject_hash: approval.approval_subject_hash,
                                authorization_policy_hash: approval.authorization_policy_hash,
                                approval_evidence_hash: approval.approval_evidence_hash,
                            }),
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
        },
        starting_equity: decimal(&document.valuation.starting_equity)?,
        cash: decimal(&document.valuation.cash)?,
        peak_equity: decimal(&document.valuation.peak_equity)?,
        positions: document
            .positions
            .into_iter()
            .map(|position| {
                Ok(OperationalPosition {
                    instrument_id: position.instrument_id,
                    quantity: decimal(&position.quantity)?,
                    mark_price: decimal(&position.mark_price)?,
                    average_cost: decimal(&position.average_cost)?,
                    realized_pnl: decimal(&position.realized_pnl)?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
        risk_limits: RiskLimits {
            max_gross_exposure: decimal(&document.risk_limits.max_gross_exposure)?,
            max_single_instrument_exposure: decimal(
                &document.risk_limits.max_single_instrument_exposure,
            )?,
            max_drawdown_bps: decimal(&document.risk_limits.max_drawdown_bps)?,
            max_working_orders: document.risk_limits.max_working_orders,
            max_unknown_orders: document.risk_limits.max_unknown_orders,
            max_unresolved_incidents: document.risk_limits.max_unresolved_incidents,
        },
        health: OperationalHealth {
            audit_healthy: document.operational_health.audit_healthy,
            reconciliation_healthy: document.operational_health.reconciliation_healthy,
            broker_connected: document.operational_health.broker_connected,
            active_kill_switches: document.operational_health.active_kill_switches,
            working_orders: document.operational_health.working_orders,
            unknown_orders: document.operational_health.unknown_orders,
            unresolved_incidents: document.operational_health.unresolved_incidents,
        },
        attribution_entries: document
            .attribution_entries
            .into_iter()
            .map(|entry| {
                Ok(AttributionEntry {
                    entry_id: entry.entry_id,
                    occurred_at: entry.occurred_at,
                    instrument_id: entry.instrument_id,
                    category: AttributionCategory::parse(&entry.category)?,
                    amount: decimal(&entry.amount)?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
        schedules: document
            .schedules
            .into_iter()
            .map(|schedule| DailySchedule {
                schedule_id: schedule.schedule_id,
                purpose: schedule.purpose,
                time_utc: schedule.time_utc,
                enabled: schedule.enabled,
                last_completed_at: schedule.last_completed_at.into_option(),
            })
            .collect(),
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn parse_parameter_control(value: &str) -> Result<ParameterControl, Box<dyn std::error::Error>> {
    match value {
        "STANDARD" => Ok(ParameterControl::Standard),
        "TWO_PERSON" => Ok(ParameterControl::TwoPerson),
        _ => Err("parameter control must be STANDARD or TWO_PERSON".into()),
    }
}

fn decimal(value: &str) -> Result<Decimal, follon_domain::DecimalError> {
    Decimal::from_str(value)
}

fn journal_evidence(path: &Path) -> (JournalInspection, Vec<follon_operations::JournalRecord>) {
    OperationalJournal::read_verified(path)
        .unwrap_or_else(|error| (JournalInspection::unhealthy(error.to_string()), Vec::new()))
}

fn publish(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_immutable(path, contents)
}

fn canonical_schedule_json(
    snapshot: &OperationsSnapshot,
    journal: &JournalInspection,
    schedules: &[follon_operations::ScheduleStatus],
) -> Result<String, Box<dyn std::error::Error>> {
    let schedules = schedules
        .iter()
        .map(|schedule| {
            format!(
                "{{\"due\":{},\"enabled\":{},\"last_completed_at\":{},\"next_due_at\":{},\"purpose\":{},\"schedule_id\":{},\"time_utc\":{}}}",
                schedule.due,
                schedule.enabled,
                optional_json_string(schedule.last_completed_at.as_deref()),
                json_string(&schedule.next_due_at),
                json_string(&schedule.purpose),
                json_string(&schedule.schedule_id),
                json_string(&schedule.time_utc),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"as_of\":{},\"configuration_content_hash\":{},\"journal\":{{\"failure_reason\":{},\"head_hash\":{},\"healthy\":{},\"sequence\":{}}},\"parameter_set_fingerprint\":{},\"projection_fingerprint\":{},\"reproducibility\":{{\"dataset_hash\":{},\"replay_event_hash\":{},\"strategy_bundle_hash\":{}}},\"schedule_schema_version\":2,\"source_fingerprint\":{},\"schedules\":[{}]}}",
        json_string(&snapshot.as_of),
        json_string(&snapshot.configuration_content_hash),
        optional_json_string(journal.failure_reason.as_deref()),
        json_string(&journal.head_hash),
        journal.healthy,
        journal.sequence,
        json_string(&snapshot.parameters.fingerprint()?),
        json_string(&follon_operations::projection_fingerprint(snapshot, journal)?),
        json_string(&snapshot.reproducibility.dataset_hash),
        json_string(&snapshot.reproducibility.replay_event_hash),
        json_string(&snapshot.reproducibility.strategy_bundle_hash),
        json_string(&snapshot.fingerprint()?),
        schedules,
    ))
}

fn canonical_parameter_change_json(
    previous: &OperationsSnapshot,
    target: &OperationsSnapshot,
) -> Result<String, Box<dyn std::error::Error>> {
    let changes = target.parameters.diff_from(&previous.parameters)?;
    let changes = changes
        .iter()
        .map(|change| {
            format!(
                "{{\"after\":{},\"before\":{},\"change_kind\":{},\"parameter_id\":{}}}",
                optional_parameter_json(change.after.as_ref()),
                optional_parameter_json(change.before.as_ref()),
                json_string(change.kind.as_str()),
                json_string(&change.parameter_id),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"change_schema_version\":1,\"changes\":[{}],\"previous\":{},\"target\":{}}}",
        changes,
        parameter_revision_json(previous)?,
        parameter_revision_json(target)?,
    ))
}

fn parameter_revision_json(
    snapshot: &OperationsSnapshot,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(format!(
        "{{\"configuration_content_hash\":{},\"configuration_id\":{},\"configuration_version\":{},\"parameter_set_fingerprint\":{},\"parameter_set_id\":{},\"previous_parameter_set_fingerprint\":{},\"previous_revision\":{},\"revision\":{}}}",
        json_string(&snapshot.configuration_content_hash),
        json_string(&snapshot.configuration_id),
        json_string(&snapshot.configuration_version),
        json_string(&snapshot.parameters.fingerprint()?),
        json_string(&snapshot.parameters.parameter_set_id),
        optional_json_string(
            snapshot
                .parameters
                .previous_parameter_set_fingerprint
                .as_deref(),
        ),
        optional_json_string(snapshot.parameters.previous_revision.as_deref()),
        json_string(&snapshot.parameters.revision),
    ))
}

fn optional_parameter_json(value: Option<&ParameterValue>) -> String {
    value
        .map(parameter_json)
        .unwrap_or_else(|| "null".to_owned())
}

fn parameter_json(value: &ParameterValue) -> String {
    let approval = value.approval.as_ref().map_or_else(
        || "null".to_owned(),
        |approval| {
            format!(
                "{{\"approval_evidence_hash\":{},\"approval_id\":{},\"approval_subject_hash\":{},\"approved_at\":{},\"approved_by\":{},\"authorization_policy_hash\":{},\"requested_by\":{}}}",
                json_string(&approval.approval_evidence_hash),
                json_string(&approval.approval_id),
                json_string(&approval.approval_subject_hash),
                json_string(&approval.approved_at),
                json_string(&approval.approved_by),
                json_string(&approval.authorization_policy_hash),
                json_string(&approval.requested_by),
            )
        },
    );
    format!(
        "{{\"approval\":{},\"control\":{},\"maximum\":\"{}\",\"minimum\":\"{}\",\"parameter_id\":{},\"value\":\"{}\"}}",
        approval,
        json_string(value.control.as_str()),
        value.maximum,
        value.minimum,
        json_string(&value.parameter_id),
        value.value,
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

fn optional_json_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_owned())
}

fn usage() -> &'static str {
    "usage:\n  follon-operations validate-config [operations.json]\n  follon-operations config-diff <previous operations.json> <target operations.json> [changes.json]\n  follon-operations dashboard [operations.json] [dashboard.json] --as-of <UTC> [--journal journal.ndjson]\n  follon-operations report [operations.json] [report.md] --as-of <UTC> [--journal journal.ndjson]\n  follon-operations schedule [operations.json] [schedule.json] --as-of <UTC> [--journal journal.ndjson]\n  follon-operations complete-schedule [operations.json] --schedule-id <id> --entry-id <id> --actor <id> --occurred-at <UTC> [--journal journal.ndjson]\n  follon-operations journal --entry-id <id> --event-type <type> --actor <id> --occurred-at <UTC> [--journal journal.ndjson] [--detail key=value]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_parser_requires_an_explicit_projection_time() {
        assert!(parse_command(vec!["dashboard".to_owned()]).is_err());
        assert!(parse_command(vec![
            "dashboard".to_owned(),
            "--as-of".to_owned(),
            "2026-08-10T16:30:00Z".to_owned(),
        ])
        .is_ok());
        assert!(parse_command(vec![
            "journal".to_owned(),
            "--entry-id".to_owned(),
            "entry.1".to_owned(),
        ])
        .is_err());
        assert!(parse_command(vec![
            "complete-schedule".to_owned(),
            "--schedule-id".to_owned(),
            "schedule.reconcile".to_owned(),
            "--entry-id".to_owned(),
            "journal.schedule.1".to_owned(),
            "--actor".to_owned(),
            "operator.alice".to_owned(),
            "--occurred-at".to_owned(),
            "2026-08-10T21:20:00Z".to_owned(),
        ])
        .is_ok());
        assert!(
            parse_command(vec!["config-diff".to_owned(), "previous.json".to_owned(),]).is_err()
        );
    }

    #[test]
    fn fixture_produces_repeatable_dashboard_and_schedule_evidence() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/operations-v1.json");
        let snapshot = load_snapshot(&fixture, "2026-08-10T16:30:00Z").unwrap();
        let journal = JournalInspection::empty();
        let first = canonical_dashboard_json(&snapshot, &journal).unwrap();
        let second = canonical_dashboard_json(&snapshot, &journal).unwrap();
        assert_eq!(first, second);
        assert!(first.contains("\"state\":\"NORMAL\""));
        let schedule = canonical_schedule_json(
            &snapshot,
            &journal,
            &derive_schedule_statuses(&snapshot).unwrap(),
        )
        .unwrap();
        assert!(schedule.contains("\"due\":false"));
    }

    #[test]
    fn parameter_change_artifact_is_repeatable_and_complete() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/operations-v1.json");
        let previous = load_snapshot(&fixture, "9999-12-31T23:59:59Z").unwrap();
        let mut target_parameters = previous.parameters.clone();
        target_parameters.revision = "8".to_owned();
        target_parameters.previous_revision = Some("7".to_owned());
        target_parameters.previous_parameter_set_fingerprint =
            Some(previous.parameters.fingerprint().unwrap());
        target_parameters.values[0].value = decimal("2.25").unwrap();
        let target_subject_hash = target_parameters.approval_subject_fingerprint().unwrap();
        let previous_subject_hash = previous.parameters.values[1]
            .approval
            .as_ref()
            .unwrap()
            .approval_subject_hash
            .clone();
        for value in &mut target_parameters.values {
            if let Some(approval) = &mut value.approval {
                approval.approval_subject_hash = target_subject_hash.clone();
            }
        }
        let previous_source = fs::read_to_string(&fixture).unwrap();
        let target_source = previous_source
            .replace(
                "\"configuration_version\": \"2026.08.10.1\"",
                "\"configuration_version\": \"2026.08.10.2\"",
            )
            .replace(
                "\"revision\": \"7\",\n    \"previous_revision\": null,\n    \"previous_parameter_set_fingerprint\": null",
                &format!(
                    "\"revision\": \"8\",\n    \"previous_revision\": \"7\",\n    \"previous_parameter_set_fingerprint\": \"{}\"",
                    previous.parameters.fingerprint().unwrap()
                ),
            )
            .replace("\"value\": \"2.0\"", "\"value\": \"2.25\"")
            .replace(&previous_subject_hash, &target_subject_hash);
        let path = std::env::temp_dir().join(format!(
            "follon-operations-parameter-change-{}-{}.json",
            std::process::id(),
            "target"
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, target_source).unwrap();
        let target = load_snapshot(&path, "9999-12-31T23:59:59Z").unwrap();
        let first = canonical_parameter_change_json(&previous, &target).unwrap();
        let second = canonical_parameter_change_json(&previous, &target).unwrap();
        assert_eq!(first, second);
        let artifact: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(artifact["changes"].as_array().unwrap().len(), 7);
        assert_eq!(artifact["changes"][0]["change_kind"], "MODIFIED");
        assert_eq!(artifact["changes"][0]["before"]["value"], "2.00000000");
        assert_eq!(artifact["changes"][0]["after"]["value"], "2.25000000");
        assert_eq!(
            artifact["changes"][1]["parameter_id"],
            "risk.max_drawdown_bps"
        );
        fs::remove_file(path).unwrap();
    }
}
