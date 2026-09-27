//! The version-1 controlled-LIVE configuration document
//! (`contracts/json-schema/v1/live-configuration.schema.json`).
//!
//! `follon-live-status` and the trading API's controlled-LIVE kill-switch
//! route both read it. One parser serves both, because a journal opens only
//! under the exact configuration fingerprint it was written with: two parsers
//! that drifted apart would disagree about which journal a file describes
//! (E3.3c).

use std::collections::BTreeMap;
use std::str::FromStr;

use follon_domain::{validate_canonical_id, Decimal};
use follon_secrets::SecretReference;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    InstrumentBucket, LiveAccount, LiveActivation, LiveActivationRequest, LiveError,
    LiveKillSwitchRegistry, LiveRiskPolicy, LiveRunMode, PortfolioRiskComposition,
};

/// Largest configuration document accepted, in bytes.
const MAX_CONFIGURATION_BYTES: usize = 1024 * 1024;

/// A controlled-LIVE configuration, validated and ready to open a service.
#[derive(Clone, Debug)]
pub struct LiveConfiguration {
    /// Stable configuration identity.
    pub configuration_id: String,
    /// Operator-chosen configuration revision.
    pub configuration_version: String,
    /// The controlled-LIVE account.
    pub account: LiveAccount,
    /// The controlled-LIVE risk policy.
    pub risk: LiveRiskPolicy,
    /// An empty kill-switch registry at the configured revision.
    pub kill_switches: LiveKillSwitchRegistry,
    /// The time-bounded activation, bound to this configuration.
    pub activation: LiveActivation,
    /// SHA-256 of the exact document bytes.
    pub content_hash: String,
}

impl LiveConfiguration {
    /// Parses and validates a version-1 document from its exact bytes.
    pub fn from_json(bytes: &[u8]) -> Result<Self, LiveError> {
        if bytes.is_empty() || bytes.len() > MAX_CONFIGURATION_BYTES {
            return Err(LiveError(
                "live configuration must be between 1 byte and 1 MiB".to_owned(),
            ));
        }
        let document: LiveConfigurationDocument = serde_json::from_slice(bytes)
            .map_err(|error| LiveError(format!("invalid live configuration: {error}")))?;
        if document.schema_version != 1 {
            return Err(LiveError(
                "unsupported live configuration schema version".to_owned(),
            ));
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
            return Err(LiveError(
                "live configuration_version is required".to_owned(),
            ));
        }
        let account = LiveAccount {
            account_id: document.account.account_id,
            currency: document.account.currency,
            initial_cash: decimal(&document.account.initial_cash)?,
            max_deployed_capital: decimal(&document.account.max_deployed_capital)?,
            environment: document.account.environment,
            credential_reference: SecretReference::new(document.account.credential_reference)?,
        };
        let risk_document = document.risk;
        let risk = LiveRiskPolicy {
            version: risk_document.policy_version,
            trading_calendar_id: risk_document.trading_calendar_id,
            max_order_quantity: decimal(&risk_document.max_order_quantity)?,
            max_order_notional: decimal(&risk_document.max_order_notional)?,
            max_price_deviation_bps: decimal(&risk_document.max_price_deviation_bps)?,
            canary_max_order_notional: decimal(&risk_document.canary_max_order_notional)?,
            canary_max_orders: risk_document.canary_max_orders,
            max_open_orders: risk_document.max_open_orders,
            max_position_quantity: decimal(&risk_document.max_position_quantity)?,
            max_realized_loss: decimal(&risk_document.max_realized_loss)?,
            max_market_data_age_seconds: risk_document.max_market_data_age_seconds,
            max_order_rate: risk_document.max_order_rate,
            order_rate_window_seconds: risk_document.order_rate_window_seconds,
            portfolio_risk: risk_document
                .portfolio_risk
                .map(portfolio_risk_composition)
                .transpose()?,
            // Not exposed through the controlled-live configuration contract:
            // permitting net short exposure with real capital is a deliberate,
            // separately-reviewed operator decision, not something a
            // configuration file should be able to turn on implicitly.
            short_exposure: None,
            instrument_tick_sizes: decimal_map(risk_document.instrument_tick_sizes)?,
            instrument_lot_sizes: decimal_map(risk_document.instrument_lot_sizes)?,
        };
        let kill_switches = LiveKillSwitchRegistry::new(document.kill_switch_version)?;
        let activation = LiveActivation::for_configuration(
            LiveActivationRequest {
                activation_id: document.activation.activation_id,
                mode: parse_mode(&document.activation.mode)?,
                requested_by: document.activation.requested_by,
                approved_by: document.activation.approved_by,
                activated_at: document.activation.activated_at,
                expires_at: document.activation.expires_at,
            },
            &account,
            &risk,
            &kill_switches,
        )?;
        Ok(Self {
            configuration_id: document.configuration_id,
            configuration_version: document.configuration_version,
            account,
            risk,
            kill_switches,
            activation,
            content_hash: format!("{:x}", Sha256::digest(bytes)),
        })
    }
}

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
    /// [`PortfolioRiskComposition`]). Absent by default so every existing
    /// configuration keeps today's behavior exactly.
    #[serde(default)]
    portfolio_risk: Option<PortfolioRiskDocument>,
    /// Required venue tick size per tradable instrument, as exact decimal
    /// strings. An order for an unlisted instrument, or a limit off its grid,
    /// is refused before it can reach the broker.
    instrument_tick_sizes: BTreeMap<String, String>,
    /// Required venue lot size per tradable instrument, as exact decimal
    /// strings. An order for an unlisted instrument, or a quantity that is
    /// not a whole number of lots, is refused before it can reach the broker.
    instrument_lot_sizes: BTreeMap<String, String>,
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
    /// [`PortfolioRiskComposition::margin_rates`]). Absent or empty means
    /// margin utilization stays fixed at zero, exactly as before.
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

fn decimal(value: &str) -> Result<Decimal, LiveError> {
    Ok(Decimal::from_str(value)?)
}

fn decimal_map(values: BTreeMap<String, String>) -> Result<BTreeMap<String, Decimal>, LiveError> {
    values
        .into_iter()
        .map(|(key, value)| Ok((key, decimal(&value)?)))
        .collect()
}

fn parse_mode(value: &str) -> Result<LiveRunMode, LiveError> {
    match value {
        "SHADOW" => Ok(LiveRunMode::Shadow),
        "CANARY" => Ok(LiveRunMode::Canary),
        _ => Err(LiveError(
            "live activation mode must be SHADOW or CANARY".to_owned(),
        )),
    }
}

fn portfolio_risk_composition(
    document: PortfolioRiskDocument,
) -> Result<PortfolioRiskComposition, LiveError> {
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
