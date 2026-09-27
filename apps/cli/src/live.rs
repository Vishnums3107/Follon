//! Read-only controlled-live monitoring snapshot command.
//!
//! This executable deliberately has no credential provider and its adapter refuses every
//! connection, submission, cancellation, and reconciliation request. It can therefore create
//! signed-on-disk monitoring evidence, but can never place a live order.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use follon_cli::write_immutable;
use follon_domain::validate_utc_timestamp;
use follon_live::{
    LiveBrokerAccountSnapshot, LiveBrokerAdapter, LiveBrokerEvent, LiveBrokerOrderRequest,
    LiveBrokerSubmitResult, LiveConfiguration, LiveError, LiveTradingService,
};
use follon_secrets::SecretMaterial;

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
    let configuration = load_configuration(&arguments.configuration_path)?;
    let configuration_hash = configuration.content_hash.clone();
    let service = LiveTradingService::open_durable(
        configuration.account,
        configuration.risk,
        configuration.activation,
        configuration.kill_switches,
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

/// Reads the version-1 controlled-LIVE configuration through the parser the
/// trading API shares (`follon_live::LiveConfiguration`).
fn load_configuration(path: &Path) -> Result<LiveConfiguration, Box<dyn std::error::Error>> {
    Ok(LiveConfiguration::from_json(&fs::read(path)?)?)
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
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        assert!(load_configuration(&root.join("tests/fixtures/config/live-v1.json")).is_ok());
    }
}
