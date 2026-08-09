//! Read-only controlled-live monitoring snapshot command.
//!
//! This executable deliberately has no credential provider and its adapter refuses every
//! connection, submission, cancellation, and reconciliation request. It can therefore create
//! signed-on-disk monitoring evidence, but can never place a live order.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_cli::write_immutable;
use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use follon_live::{
    LiveAccount, LiveActivation, LiveActivationRequest, LiveBrokerAccountSnapshot,
    LiveBrokerAdapter, LiveBrokerEvent, LiveBrokerOrderRequest, LiveBrokerSubmitResult, LiveError,
    LiveKillSwitchRegistry, LiveRiskPolicy, LiveRunMode, LiveTradingService,
};
use follon_secrets::{SecretMaterial, SecretReference};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveConfigurationDocument {
    schema_version: u32,
    configuration_id: String,
    configuration_version: String,
    account: LiveAccountDocument,
    risk: LiveRiskDocument,
    kill_switch_version: String,
    activation: LiveActivationDocument,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveAccountDocument {
    account_id: String,
    currency: String,
    initial_cash: String,
    max_deployed_capital: String,
    environment: String,
    credential_reference: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveRiskDocument {
    policy_version: String,
    trading_calendar_id: String,
    max_order_quantity: String,
    max_order_notional: String,
    canary_max_order_notional: String,
    canary_max_orders: u32,
    max_open_orders: usize,
    max_position_quantity: String,
    max_realized_loss: String,
    max_market_data_age_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveActivationDocument {
    activation_id: String,
    mode: String,
    requested_by: String,
    approved_by: String,
    activated_at: String,
    expires_at: String,
}

struct CommandArguments {
    journal_path: PathBuf,
    output_path: PathBuf,
    configuration_path: PathBuf,
    opened_at: String,
}

/// A deliberate inert adapter used by the read-only monitoring binary.
struct OfflineLiveAdapter;

impl LiveBrokerAdapter for OfflineLiveAdapter {
    fn connect(&mut self, _: &str, _: &SecretMaterial) -> Result<(), LiveError> {
        Err(LiveError(
            "follon-live-status is read-only and cannot connect to a broker".to_owned(),
        ))
    }

    fn submit(&mut self, _: &LiveBrokerOrderRequest) -> Result<LiveBrokerSubmitResult, LiveError> {
        Err(LiveError(
            "follon-live-status is read-only and cannot submit orders".to_owned(),
        ))
    }

    fn cancel(&mut self, _: &str) -> Result<(), LiveError> {
        Err(LiveError(
            "follon-live-status is read-only and cannot cancel orders".to_owned(),
        ))
    }

    fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
        Err(LiveError(
            "follon-live-status is read-only and cannot poll a broker".to_owned(),
        ))
    }

    fn snapshot(&mut self, _: &str) -> Result<LiveBrokerAccountSnapshot, LiveError> {
        Err(LiveError(
            "follon-live-status is read-only and cannot reconcile a broker".to_owned(),
        ))
    }

    fn reconnect(&mut self, _: &str, _: &SecretMaterial) -> Result<(), LiveError> {
        Err(LiveError(
            "follon-live-status is read-only and cannot reconnect a broker".to_owned(),
        ))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(env::args().skip(1).collect())?;
    let (configuration, configuration_hash) = load_configuration(&arguments.configuration_path)?;
    let account = LiveAccount {
        account_id: configuration.account.account_id,
        currency: configuration.account.currency,
        initial_cash: decimal(&configuration.account.initial_cash)?,
        max_deployed_capital: decimal(&configuration.account.max_deployed_capital)?,
        environment: configuration.account.environment,
        credential_reference: SecretReference::new(configuration.account.credential_reference)?,
    };
    let risk = LiveRiskPolicy {
        version: configuration.risk.policy_version,
        trading_calendar_id: configuration.risk.trading_calendar_id,
        max_order_quantity: decimal(&configuration.risk.max_order_quantity)?,
        max_order_notional: decimal(&configuration.risk.max_order_notional)?,
        canary_max_order_notional: decimal(&configuration.risk.canary_max_order_notional)?,
        canary_max_orders: configuration.risk.canary_max_orders,
        max_open_orders: configuration.risk.max_open_orders,
        max_position_quantity: decimal(&configuration.risk.max_position_quantity)?,
        max_realized_loss: decimal(&configuration.risk.max_realized_loss)?,
        max_market_data_age_seconds: configuration.risk.max_market_data_age_seconds,
    };
    let switches = LiveKillSwitchRegistry::new(configuration.kill_switch_version)?;
    let activation = LiveActivation::for_configuration(
        LiveActivationRequest {
            activation_id: configuration.activation.activation_id,
            mode: parse_mode(&configuration.activation.mode)?,
            requested_by: configuration.activation.requested_by,
            approved_by: configuration.activation.approved_by,
            activated_at: configuration.activation.activated_at,
            expires_at: configuration.activation.expires_at,
        },
        &account,
        &risk,
        &switches,
    )?;
    let service = LiveTradingService::open_durable(
        account,
        risk,
        activation,
        switches,
        OfflineLiveAdapter,
        &arguments.journal_path,
        &arguments.opened_at,
    )?;
    let dashboard = service.canonical_monitoring_json()?;
    if let Some(parent) = arguments.output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_immutable(&arguments.output_path, &dashboard)?;
    eprintln!(
        "controlled-live journal: {}",
        arguments.journal_path.display()
    );
    eprintln!("monitoring snapshot: {}", arguments.output_path.display());
    eprintln!("configuration hash: {configuration_hash}");
    eprintln!(
        "controlled-live gate: {}/{}; broker capability: disabled in this binary",
        service.promotion_status().clean_live_days,
        service.promotion_status().required_live_days,
    );
    Ok(())
}

fn parse_arguments(arguments: Vec<String>) -> Result<CommandArguments, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut configuration_path = PathBuf::from("tests/fixtures/config/live-v1.json");
    let mut configuration_explicit = false;
    let mut opened_at = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--config" => {
                if configuration_explicit {
                    return Err("--config may be specified only once".into());
                }
                index += 1;
                configuration_path = PathBuf::from(required(&arguments, index, "--config")?);
                configuration_explicit = true;
            }
            "--opened-at" => {
                if opened_at.is_some() {
                    return Err("--opened-at may be specified only once".into());
                }
                index += 1;
                opened_at = Some(required(&arguments, index, "--opened-at")?.to_owned());
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported argument: {value}").into())
            }
            value => positional.push(PathBuf::from(value)),
        }
        index += 1;
    }
    if positional.len() > 2 {
        return Err(
            "usage: follon-live-status [journal.ndjson] [dashboard.json] --opened-at <UTC> [--config live.json]"
                .into(),
        );
    }
    let opened_at =
        opened_at.ok_or("--opened-at is required; use an authoritative UTC timestamp")?;
    validate_utc_timestamp("--opened-at", &opened_at)?;
    Ok(CommandArguments {
        journal_path: positional
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("var/follon-live.journal.ndjson")),
        output_path: positional
            .get(1)
            .cloned()
            .unwrap_or_else(|| PathBuf::from("var/follon-live-dashboard.json")),
        configuration_path,
        opened_at,
    })
}

fn parse_mode(value: &str) -> Result<LiveRunMode, Box<dyn std::error::Error>> {
    match value {
        "SHADOW" => Ok(LiveRunMode::Shadow),
        "CANARY" => Ok(LiveRunMode::Canary),
        _ => Err("live activation mode must be SHADOW or CANARY".into()),
    }
}

fn required<'a>(
    values: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    values
        .get(index)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn load_configuration(
    path: &Path,
) -> Result<(LiveConfigurationDocument, String), Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err("live configuration must be between 1 byte and 1 MiB".into());
    }
    let document: LiveConfigurationDocument = serde_json::from_slice(&bytes)?;
    if document.schema_version != 1 {
        return Err("unsupported live configuration schema version".into());
    }
    for (name, value) in [
        ("live configuration_id", document.configuration_id.as_str()),
        ("live account_id", document.account.account_id.as_str()),
        (
            "live activation_id",
            document.activation.activation_id.as_str(),
        ),
        (
            "live activation requester",
            document.activation.requested_by.as_str(),
        ),
        (
            "live activation approver",
            document.activation.approved_by.as_str(),
        ),
    ] {
        validate_canonical_id(name, value)?;
    }
    if document.configuration_version.is_empty() {
        return Err("live configuration_version is required".into());
    }
    Ok((document, format!("{:x}", Sha256::digest(bytes))))
}

fn decimal(value: &str) -> Result<Decimal, follon_domain::DecimalError> {
    Decimal::from_str(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_and_configuration_are_strict_and_read_only() {
        assert!(parse_arguments(Vec::new()).is_err());
        assert!(parse_arguments(vec!["--opened-at".to_owned(), "not-a-time".to_owned()]).is_err());
        let arguments = parse_arguments(vec![
            "--opened-at".to_owned(),
            "2026-01-02T14:00:00Z".to_owned(),
        ])
        .expect("strict valid arguments");
        assert_eq!(
            arguments.configuration_path,
            PathBuf::from("tests/fixtures/config/live-v1.json")
        );
        assert!(parse_mode("PAPER").is_err());
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        assert!(load_configuration(&root.join("tests/fixtures/config/live-v1.json")).is_ok());
    }
}
