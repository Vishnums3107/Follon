//! The operator-facing document for a PAPER portfolio-risk composition.
//!
//! Every application that opens a PAPER service from a configuration file
//! shares this one document and its conversion, so a limit that means one thing
//! to `follon-paper-status` cannot mean another to the order-submitting routes
//! (delivery state E7.5). The document is strict: an unknown field is refused,
//! and every limit is an exact decimal string.

use std::collections::BTreeMap;

use follon_domain::Decimal;
use serde::Deserialize;

use crate::*;

/// The `portfolio_risk` block of a PAPER configuration.
///
/// Absent from a configuration, no aggregate limit applies, exactly as before.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortfolioRiskDocument {
    /// Version stamped on every aggregate decision.
    pub policy_version: String,
    /// Largest gross exposure, as an exact decimal.
    pub max_gross_exposure: String,
    /// Largest absolute net exposure.
    pub max_abs_net_exposure: String,
    /// Largest leverage in basis points of equity.
    pub max_leverage_bps: String,
    /// Largest single-instrument concentration in basis points of gross.
    pub max_concentration_bps: String,
    /// Absent means "no real drawdown limit" (`10000` bps = 100%, which the
    /// aggregate kernel's ratio can never reach or exceed).
    #[serde(default)]
    pub max_drawdown_bps: Option<String>,
    /// Absent means "no real daily-loss limit" (`i64::MAX` currency units,
    /// which no real account's session P&L can ever reach).
    #[serde(default)]
    pub max_daily_loss: Option<String>,
    /// Absent means "no real margin-utilization limit" (`10000` bps = 100%).
    /// Safe only because `core/paper`'s `Portfolio` is fully-paid and
    /// long-only: with a per-position rate at or under 100% and no cash
    /// borrowed against a position, margin utilization cannot reach exactly
    /// 100% unless the operator sets a 100% rate and the account carries zero
    /// spare cash -- an edge case the operator controls directly via
    /// `margin_rates`, not one this sentinel silently hides.
    #[serde(default)]
    pub max_margin_utilization_bps: Option<String>,
    /// When non-empty, the only instruments the account may trade.
    #[serde(default)]
    pub allowed_instruments: Vec<String>,
    /// Instruments the account may never trade.
    #[serde(default)]
    pub restricted_instruments: Vec<String>,
    /// Gross-exposure limit per sector bucket.
    #[serde(default)]
    pub sector_limits: BTreeMap<String, String>,
    /// Gross-exposure limit per asset-class bucket.
    #[serde(default)]
    pub asset_class_limits: BTreeMap<String, String>,
    /// Gross-exposure limit per currency bucket.
    #[serde(default)]
    pub currency_limits: BTreeMap<String, String>,
    /// Real once `core/paper`'s per-strategy attribution ledger is populated
    /// (Slice 2d); absent or empty means no strategy ever trips this check.
    #[serde(default)]
    pub strategy_limits: BTreeMap<String, String>,
    /// Reference data for every instrument this account may hold or trade.
    #[serde(default)]
    pub instrument_buckets: BTreeMap<String, InstrumentBucketDocument>,
    /// Slice-2c margin-utilization composition (see
    /// [`PortfolioRiskComposition::margin_rates`]). Absent or empty means
    /// margin utilization stays fixed at zero, exactly as before.
    #[serde(default)]
    pub margin_rates: BTreeMap<String, MarginRateDocument>,
}

/// One instrument's classification for the aggregate kernel.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentBucketDocument {
    /// Asset-class label.
    pub asset_class: String,
    /// Three-letter currency.
    pub currency: String,
    /// Sector or risk-bucket label.
    pub sector: String,
}

/// One asset class's margin rates.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarginRateDocument {
    /// Initial margin in basis points of position value.
    pub initial_bps: u32,
    /// Maintenance margin in basis points of position value.
    pub maintenance_bps: u32,
}

fn decimal_map(
    name: &str,
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, Decimal>, PaperError> {
    values
        .into_iter()
        .map(|(bucket, limit)| Ok((bucket, decimal(name, &limit)?)))
        .collect()
}

impl PortfolioRiskDocument {
    /// Converts the document into the composition a [`PaperRiskPolicy`] carries.
    pub fn into_composition(self) -> Result<PortfolioRiskComposition, PaperError> {
        let instrument_buckets = self
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
                version: self.policy_version,
                global_kill_switch: false,
                max_gross_exposure: decimal("max_gross_exposure", &self.max_gross_exposure)?,
                max_abs_net_exposure: decimal("max_abs_net_exposure", &self.max_abs_net_exposure)?,
                max_leverage_bps: decimal("max_leverage_bps", &self.max_leverage_bps)?,
                max_concentration_bps: decimal(
                    "max_concentration_bps",
                    &self.max_concentration_bps,
                )?,
                // Real, operator-configurable (Slice 2): `core/paper` durably
                // tracks a running peak equity (`PaperTradingService::peak_equity`),
                // so drawdown is a genuine computed ratio. Absent means "no real
                // limit" (100%, which the ratio can never reach).
                max_drawdown_bps: match self.max_drawdown_bps {
                    Some(value) => decimal("max_drawdown_bps", &value)?,
                    None => Decimal::from_integer(10_000)?,
                },
                // Real, operator-configurable (Slice 2b): `core/paper` durably
                // tracks a session-start equity baseline
                // (`PaperTradingService::daily_baseline_equity`), reset at the
                // first risk evaluation on a new UTC calendar day, so daily P&L is
                // a genuine computed figure. Absent means "no real limit"
                // (`i64::MAX`, which no real account's session P&L can reach).
                max_daily_loss: match self.max_daily_loss {
                    Some(value) => decimal("max_daily_loss", &value)?,
                    None => Decimal::from_integer(i64::MAX)?,
                },
                // Real, operator-configurable (Slice 2c): real once `margin_rates`
                // below is configured. Absent means "no real limit" (100%).
                max_margin_utilization_bps: match self.max_margin_utilization_bps {
                    Some(value) => decimal("max_margin_utilization_bps", &value)?,
                    None => Decimal::from_integer(10_000)?,
                },
                max_abs_delta: Decimal::ZERO,
                max_abs_gamma: Decimal::ZERO,
                // core/paper already independently enforces open-order count and
                // order rate (`MAX_OPEN_ORDERS_EXCEEDED`/`MAX_ORDER_RATE_EXCEEDED`
                // in `PaperTradingService::evaluate_risk`); these permissive
                // sentinels keep this composed kernel from ever producing a
                // second, parallel copy of the same check.
                max_open_orders: usize::MAX,
                max_order_rate: u32::MAX,
                allowed_instruments: self.allowed_instruments.into_iter().collect(),
                restricted_instruments: self.restricted_instruments.into_iter().collect(),
                sector_limits: decimal_map("sector_limits", self.sector_limits)?,
                asset_class_limits: decimal_map("asset_class_limits", self.asset_class_limits)?,
                currency_limits: decimal_map("currency_limits", self.currency_limits)?,
                // Real, operator-configurable (Slice 2d): `core/paper` durably
                // tracks each strategy's own net contribution to every
                // instrument (`PaperTradingService::strategy_attribution`), so a
                // strategy-bucket limit is a genuine cumulative-exposure check.
                strategy_limits: decimal_map("strategy_limits", self.strategy_limits)?,
                max_news_slippage_bps: None,
                max_spread_multiplier_bps: None,
            },
            instrument_buckets,
            margin_rates: if self.margin_rates.is_empty() {
                None
            } else {
                Some(
                    self.margin_rates
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
}
