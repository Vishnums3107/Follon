//! Read-only paper-operations dashboard snapshot command.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_cli::write_immutable;
use follon_domain::{validate_canonical_id, Decimal};
use follon_paper::{
    IbkrPaperAdapter, KillSwitchRegistry, KillSwitchScope, PaperAccount, PaperRiskPolicy,
    PaperTradingService,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaperConfigurationDocument {
    schema_version: u32,
    configuration_id: String,
    configuration_version: String,
    account: PaperAccountDocument,
    risk: PaperRiskDocument,
    kill_switch_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaperAccountDocument {
    account_id: String,
    currency: String,
    initial_cash: String,
    environment: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaperRiskDocument {
    policy_version: String,
    trading_calendar_id: String,
    max_order_quantity: String,
    max_order_notional: String,
    max_price_deviation_bps: String,
    max_open_orders: usize,
    max_position_quantity: String,
    max_realized_loss: String,
    max_market_data_age_seconds: u64,
}

struct CommandArguments {
    journal_path: PathBuf,
    output_path: PathBuf,
    configuration_path: PathBuf,
    kill_switch_action: Option<KillSwitchAction>,
}

enum KillSwitchAction {
    Activate(KillSwitchScope),
    Deactivate(KillSwitchScope),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(env::args().skip(1).collect())?;
    if arguments.kill_switch_action.is_some() && arguments.output_path.exists() {
        return Err(
            "a kill-switch action requires a new immutable dashboard output path; refusing to change state before evidence can be published"
                .into(),
        );
    }
    if arguments.kill_switch_action.is_some() {
        if let Some(parent) = arguments.output_path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let (configuration, configuration_hash) = load_configuration(&arguments.configuration_path)?;
    let account = PaperAccount {
        account_id: configuration.account.account_id,
        currency: configuration.account.currency,
        initial_cash: decimal(&configuration.account.initial_cash)?,
        environment: configuration.account.environment,
    };
    let risk = PaperRiskPolicy {
        version: configuration.risk.policy_version,
        trading_calendar_id: configuration.risk.trading_calendar_id,
        max_order_quantity: decimal(&configuration.risk.max_order_quantity)?,
        max_order_notional: decimal(&configuration.risk.max_order_notional)?,
        max_price_deviation_bps: decimal(&configuration.risk.max_price_deviation_bps)?,
        max_open_orders: configuration.risk.max_open_orders,
        max_position_quantity: decimal(&configuration.risk.max_position_quantity)?,
        max_realized_loss: decimal(&configuration.risk.max_realized_loss)?,
        max_market_data_age_seconds: configuration.risk.max_market_data_age_seconds,
    };
    let adapter = IbkrPaperAdapter::new(&account)?;
    let mut service = PaperTradingService::open_durable(
        account,
        risk,
        KillSwitchRegistry::new(configuration.kill_switch_version)?,
        adapter,
        &arguments.journal_path,
    )?;
    if let Some(action) = arguments.kill_switch_action {
        match action {
            KillSwitchAction::Activate(scope) => {
                service.activate_kill_switch(scope)?;
            }
            KillSwitchAction::Deactivate(scope) => {
                service.deactivate_kill_switch(&scope)?;
            }
        }
    }
    let dashboard = service.canonical_dashboard_json()?;
    if let Some(parent) = arguments.output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_immutable(&arguments.output_path, &dashboard)?;
    eprintln!("paper journal: {}", arguments.journal_path.display());
    eprintln!("dashboard snapshot: {}", arguments.output_path.display());
    eprintln!("configuration hash: {configuration_hash}");
    eprintln!(
        "paper gate: {}/30",
        service.promotion_status().clean_paper_days
    );
    Ok(())
}

fn parse_arguments(arguments: Vec<String>) -> Result<CommandArguments, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut configuration_path = PathBuf::from("tests/fixtures/config/paper-v1.json");
    let mut configuration_explicit = false;
    let mut kill_switch_action = None;
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
            "--activate" | "--deactivate" => {
                if kill_switch_action.is_some() {
                    return Err(
                        "only one kill-switch action may be requested per invocation".into(),
                    );
                }
                index += 1;
                let scope =
                    parse_kill_switch_scope(required(&arguments, index, "kill-switch action")?)?;
                kill_switch_action = Some(if arguments[index - 1] == "--activate" {
                    KillSwitchAction::Activate(scope)
                } else {
                    KillSwitchAction::Deactivate(scope)
                });
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
            "usage: follon-paper-status [journal.ndjson] [dashboard.json] [--config paper.json] [--activate scope|--deactivate scope]"
                .into(),
        );
    }
    Ok(CommandArguments {
        journal_path: positional
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("var/follon-paper.journal.ndjson")),
        output_path: positional
            .get(1)
            .cloned()
            .unwrap_or_else(|| PathBuf::from("var/follon-paper-dashboard.json")),
        configuration_path,
        kill_switch_action,
    })
}

fn parse_kill_switch_scope(value: &str) -> Result<KillSwitchScope, Box<dyn std::error::Error>> {
    if value == "global" {
        return Ok(KillSwitchScope::Global);
    }
    for (prefix, constructor) in [
        (
            "account:",
            KillSwitchScope::Account as fn(String) -> KillSwitchScope,
        ),
        (
            "strategy:",
            KillSwitchScope::Strategy as fn(String) -> KillSwitchScope,
        ),
        (
            "instrument:",
            KillSwitchScope::Instrument as fn(String) -> KillSwitchScope,
        ),
    ] {
        if let Some(identifier) = value.strip_prefix(prefix) {
            validate_canonical_id("kill-switch scope", identifier)?;
            return Ok(constructor(identifier.to_owned()));
        }
    }
    Err("kill-switch scope must be global, account:<id>, strategy:<id>, or instrument:<id>".into())
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
) -> Result<(PaperConfigurationDocument, String), Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err("paper configuration must be between 1 byte and 1 MiB".into());
    }
    let document: PaperConfigurationDocument = serde_json::from_slice(&bytes)?;
    if document.schema_version != 1 {
        return Err("unsupported paper configuration schema version".into());
    }
    for (name, value) in [
        ("paper configuration_id", document.configuration_id.as_str()),
        ("paper account_id", document.account.account_id.as_str()),
    ] {
        validate_canonical_id(name, value)?;
    }
    if document.configuration_version.is_empty() {
        return Err("paper configuration_version is required".into());
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
    fn parser_and_configuration_are_strict() {
        let defaults = parse_arguments(Vec::new()).unwrap();
        assert_eq!(
            defaults.configuration_path,
            PathBuf::from("tests/fixtures/config/paper-v1.json")
        );
        assert!(parse_arguments(vec!["--unknown".to_owned()]).is_err());
        assert!(parse_arguments(vec!["--config".to_owned()]).is_err());
        assert!(matches!(
            parse_arguments(vec!["--activate".to_owned(), "global".to_owned()])
                .unwrap()
                .kill_switch_action,
            Some(KillSwitchAction::Activate(KillSwitchScope::Global))
        ));
        assert!(parse_arguments(vec![
            "--activate".to_owned(),
            "global".to_owned(),
            "--deactivate".to_owned(),
            "global".to_owned(),
        ])
        .is_err());
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        assert!(load_configuration(&root.join("tests/fixtures/config/paper-v1.json")).is_ok());
    }
}
