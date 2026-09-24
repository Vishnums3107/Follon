//! Read-only controlled-live monitoring snapshot command.
//!
//! This executable deliberately has no credential provider and its adapter refuses every
//! connection, submission, cancellation, and reconciliation request. It can therefore create
//! signed-on-disk monitoring evidence, but can never place a live order.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_cli::write_immutable;
use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use follon_live::{
    InstrumentBucket, LiveAccount, LiveActivation, LiveActivationRequest,
    LiveBrokerAccountSnapshot, LiveBrokerAdapter, LiveBrokerEvent, LiveBrokerOrderRequest,
    LiveBrokerSubmitResult, LiveError, LiveKillSwitchRegistry, LiveRiskPolicy, LiveRunMode,
    LiveTradingService, PortfolioRiskComposition,
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
    max_price_deviation_bps: String,
    canary_max_order_notional: String,
    canary_max_orders: u32,
    max_open_orders: usize,
    max_position_quantity: String,
    max_realized_loss: String,
    max_market_data_age_seconds: u64,
    max_order_rate: u32,
    order_rate_window_seconds: u64,
    /// Slice-1 aggregate portfolio-risk composition (see
    /// `follon_live::PortfolioRiskComposition`). Absent by default so every
    /// existing configuration keeps today's behavior exactly.
    #[serde(default)]
    portfolio_risk: Option<PortfolioRiskDocument>,
    /// Required venue tick size per tradable instrument, as exact decimal
    /// strings. An order for an unlisted instrument, or a limit off its grid,
    /// is refused before it can reach the broker.
    instrument_tick_sizes: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PortfolioRiskDocument {
    policy_version: String,
    max_gross_exposure: String,
    max_abs_net_exposure: String,
    max_leverage_bps: String,
    max_concentration_bps: String,
    /// Absent means "no real drawdown limit" (`10000` bps = 100%, which the
    /// aggregate kernel's ratio can never reach or exceed).
    #[serde(default)]
    max_drawdown_bps: Option<String>,
    /// Absent means "no real daily-loss limit" (`i64::MAX` currency units,
    /// which no real account's session P&L can ever reach).
    #[serde(default)]
    max_daily_loss: Option<String>,
    /// Absent means "no real margin-utilization limit" (`10000` bps = 100%).
    /// Safe only because `core/live`'s `Portfolio` is fully-paid and
    /// long-only: with a per-position rate at or under 100% and no cash
    /// borrowed against a position, margin utilization cannot reach exactly
    /// 100% unless the operator sets a 100% rate and the account carries zero
    /// spare cash -- an edge case the operator controls directly via
    /// `margin_rates`, not one this sentinel silently hides.
    #[serde(default)]
    max_margin_utilization_bps: Option<String>,
    #[serde(default)]
    allowed_instruments: Vec<String>,
    #[serde(default)]
    restricted_instruments: Vec<String>,
    #[serde(default)]
    sector_limits: BTreeMap<String, String>,
    #[serde(default)]
    asset_class_limits: BTreeMap<String, String>,
    #[serde(default)]
    currency_limits: BTreeMap<String, String>,
    /// Real once `core/live`'s per-strategy attribution ledger is populated
    /// (Slice 2d); absent or empty means no strategy ever trips this check.
    #[serde(default)]
    strategy_limits: BTreeMap<String, String>,
    #[serde(default)]
    instrument_buckets: BTreeMap<String, InstrumentBucketDocument>,
    /// Slice-2c margin-utilization composition (see
    /// `follon_live::PortfolioRiskComposition::margin_rates`). Absent or
    /// empty means margin utilization stays fixed at zero, exactly as before.
    #[serde(default)]
    margin_rates: BTreeMap<String, MarginRateDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstrumentBucketDocument {
    asset_class: String,
    currency: String,
    sector: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarginRateDocument {
    initial_bps: u32,
    maintenance_bps: u32,
}

fn decimal_map(
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, Decimal>, Box<dyn std::error::Error>> {
    values
        .into_iter()
        .map(|(bucket, limit)| Ok((bucket, decimal(&limit)?)))
        .collect()
}

fn portfolio_risk_composition(
    document: PortfolioRiskDocument,
) -> Result<PortfolioRiskComposition, Box<dyn std::error::Error>> {
    let instrument_buckets = document
        .instrument_buckets
        .into_iter()
        .map(|(instrument_id, bucket)| {
            (
                instrument_id,
                InstrumentBucket {
                    asset_class: bucket.asset_class,
                    currency: bucket.currency,
                    sector: bucket.sector,
                },
            )
        })
        .collect();
    Ok(PortfolioRiskComposition {
        policy: follon_risk::PortfolioRiskPolicy {
            version: document.policy_version,
            global_kill_switch: false,
            max_gross_exposure: decimal(&document.max_gross_exposure)?,
            max_abs_net_exposure: decimal(&document.max_abs_net_exposure)?,
            max_leverage_bps: decimal(&document.max_leverage_bps)?,
            max_concentration_bps: decimal(&document.max_concentration_bps)?,
            // Real, operator-configurable (Slice 2): `core/live` durably
            // tracks a running peak equity (`LiveTradingService::peak_equity`),
            // so drawdown is a genuine computed ratio. Absent means "no real
            // limit" (100%, which the ratio can never reach).
            max_drawdown_bps: match document.max_drawdown_bps {
                Some(value) => decimal(&value)?,
                None => Decimal::from_integer(10_000)?,
            },
            // Real, operator-configurable (Slice 2b): `core/live` durably
            // tracks a session-start equity baseline
            // (`LiveTradingService::daily_baseline_equity`), reset at the
            // first risk evaluation on a new UTC calendar day, so daily P&L is
            // a genuine computed figure. Absent means "no real limit"
            // (`i64::MAX`, which no real account's session P&L can reach).
            max_daily_loss: match document.max_daily_loss {
                Some(value) => decimal(&value)?,
                None => Decimal::from_integer(i64::MAX)?,
            },
            // Real, operator-configurable (Slice 2c): real once `margin_rates`
            // below is configured. Absent means "no real limit" (100%).
            max_margin_utilization_bps: match document.max_margin_utilization_bps {
                Some(value) => decimal(&value)?,
                None => Decimal::from_integer(10_000)?,
            },
            max_abs_delta: Decimal::ZERO,
            max_abs_gamma: Decimal::ZERO,
            // core/live already independently enforces open-order count and
            // order rate (`MAX_OPEN_ORDERS_EXCEEDED`/`MAX_ORDER_RATE_EXCEEDED`
            // in `LiveTradingService::evaluate_risk`); these permissive
            // sentinels keep this composed kernel from ever producing a
            // second, parallel copy of the same check.
            max_open_orders: usize::MAX,
            max_order_rate: u32::MAX,
            allowed_instruments: document.allowed_instruments.into_iter().collect(),
            restricted_instruments: document.restricted_instruments.into_iter().collect(),
            sector_limits: decimal_map(document.sector_limits)?,
            asset_class_limits: decimal_map(document.asset_class_limits)?,
            currency_limits: decimal_map(document.currency_limits)?,
            // Real, operator-configurable (Slice 2d): `core/live` durably
            // tracks each strategy's own net contribution to every
            // instrument (`LiveTradingService::strategy_attribution`), so a
            // strategy-bucket limit is a genuine cumulative-exposure check.
            strategy_limits: decimal_map(document.strategy_limits)?,
            max_news_slippage_bps: None,
            max_spread_multiplier_bps: None,
        },
        instrument_buckets,
        margin_rates: if document.margin_rates.is_empty() {
            None
        } else {
            Some(
                document
                    .margin_rates
                    .into_iter()
                    .map(|(asset_class, rate)| {
                        (
                            asset_class,
                            follon_accounting::MarginRate {
                                initial_bps: rate.initial_bps,
                                maintenance_bps: rate.maintenance_bps,
                            },
                        )
                    })
                    .collect(),
            )
        },
    })
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
    let portfolio_risk = configuration
        .risk
        .portfolio_risk
        .map(portfolio_risk_composition)
        .transpose()?;
    let risk = LiveRiskPolicy {
        version: configuration.risk.policy_version,
        trading_calendar_id: configuration.risk.trading_calendar_id,
        max_order_quantity: decimal(&configuration.risk.max_order_quantity)?,
        max_order_notional: decimal(&configuration.risk.max_order_notional)?,
        max_price_deviation_bps: decimal(&configuration.risk.max_price_deviation_bps)?,
        canary_max_order_notional: decimal(&configuration.risk.canary_max_order_notional)?,
        canary_max_orders: configuration.risk.canary_max_orders,
        max_open_orders: configuration.risk.max_open_orders,
        max_position_quantity: decimal(&configuration.risk.max_position_quantity)?,
        max_realized_loss: decimal(&configuration.risk.max_realized_loss)?,
        max_market_data_age_seconds: configuration.risk.max_market_data_age_seconds,
        max_order_rate: configuration.risk.max_order_rate,
        order_rate_window_seconds: configuration.risk.order_rate_window_seconds,
        portfolio_risk,
        // Not exposed through the controlled-live configuration contract:
        // permitting net short exposure with real capital is a deliberate,
        // separately-reviewed operator decision, not something a configuration
        // file should be able to turn on implicitly.
        short_exposure: None,
        instrument_tick_sizes: decimal_map(configuration.risk.instrument_tick_sizes)?,
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

    #[test]
    fn portfolio_risk_configuration_loads_and_composes_real_limits() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let (configuration, _hash) =
            load_configuration(&root.join("tests/fixtures/config/live-v1-portfolio-risk.json"))
                .unwrap();
        let document = configuration
            .risk
            .portfolio_risk
            .expect("fixture declares a portfolio_risk block");
        let composition = portfolio_risk_composition(document).unwrap();
        assert_eq!(
            composition.policy.max_gross_exposure,
            decimal("5000").unwrap()
        );
        assert_eq!(
            composition.policy.max_concentration_bps,
            decimal("6000").unwrap()
        );
        // Real and operator-configurable (Slice 2: durable peak-equity
        // tracking makes drawdown a genuine computed ratio).
        assert_eq!(
            composition.policy.max_drawdown_bps,
            decimal("3000").unwrap()
        );
        // Real and operator-configurable (Slice 2b: durable session-start
        // equity baseline makes daily P&L a genuine computed figure).
        assert_eq!(composition.policy.max_daily_loss, decimal("500").unwrap());
        // Real and operator-configurable (Slice 2c: margin_rates below makes
        // margin utilization a genuine computed ratio).
        assert_eq!(
            composition.policy.max_margin_utilization_bps,
            decimal("4000").unwrap()
        );
        let margin_rates = composition
            .margin_rates
            .as_ref()
            .expect("fixture declares margin_rates");
        let equity_rate = margin_rates.get("equity").expect("equity margin rate");
        assert_eq!(equity_rate.initial_bps, 5000);
        assert_eq!(equity_rate.maintenance_bps, 2500);
        assert_eq!(composition.policy.max_abs_delta, Decimal::ZERO);
        assert_eq!(composition.policy.max_abs_gamma, Decimal::ZERO);
        // Live's own dedicated checks already cover these; the composed
        // kernel's copies must stay permanently permissive.
        assert_eq!(composition.policy.max_open_orders, usize::MAX);
        assert_eq!(composition.policy.max_order_rate, u32::MAX);
        // Real and operator-configurable (Slice 2d: durable per-strategy
        // attribution makes strategy exposure a genuine computed figure).
        assert_eq!(
            composition.policy.strategy_limits.get("strategy.live.001"),
            Some(&decimal("3000").unwrap())
        );
        assert_eq!(
            composition.policy.sector_limits.get("index"),
            Some(&decimal("2500").unwrap())
        );
        let bucket = composition
            .instrument_buckets
            .get("inst.us_equity.spy")
            .expect("fixture declares a bucket for the SPY instrument");
        assert_eq!(bucket.asset_class, "equity");
        assert_eq!(bucket.currency, "USD");
        assert_eq!(bucket.sector, "index");
        composition.policy.validate().unwrap();
    }
}
