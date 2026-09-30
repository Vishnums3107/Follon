//! Portfolio snapshot, versioned limits and the news-shock collar.

use std::collections::{BTreeMap, BTreeSet};

use follon_domain::{validate_canonical_id, Decimal};

use crate::*;

/// Portfolio facts selected at a single explicit evaluation instant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortfolioRiskSnapshot {
    /// Current total equity in reporting currency.
    pub equity: Decimal,
    /// Highest equity used for drawdown.
    pub peak_equity: Decimal,
    /// Current session P&L; losses are negative.
    pub daily_pnl: Decimal,
    /// Current margin requirement in reporting currency.
    pub margin_used: Decimal,
    /// Fully attributed, filled marked positions.
    pub positions: Vec<RiskPosition>,
    /// Unfilled marked exposure rows from working orders. Each can fill or not
    /// fill independently for the conservative pre-trade bounds.
    pub working_positions: Vec<RiskPosition>,
    /// Current working orders.
    pub resting_orders: Vec<RestingOrder>,
    /// Orders observed inside the configured rate window.
    pub recent_order_count: u32,
}

/// Versioned portfolio-wide limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortfolioRiskPolicy {
    /// Immutable policy identity.
    pub version: String,
    /// Independent all-trading switch.
    pub global_kill_switch: bool,
    /// Maximum gross exposure.
    pub max_gross_exposure: Decimal,
    /// Maximum absolute net exposure.
    pub max_abs_net_exposure: Decimal,
    /// Maximum gross/equity ratio in basis points.
    pub max_leverage_bps: Decimal,
    /// Maximum largest-position/gross ratio in basis points.
    pub max_concentration_bps: Decimal,
    /// Maximum daily loss as a positive amount.
    pub max_daily_loss: Decimal,
    /// Maximum peak-to-current drawdown in basis points.
    pub max_drawdown_bps: Decimal,
    /// Maximum margin/equity ratio in basis points.
    pub max_margin_utilization_bps: Decimal,
    /// Maximum total absolute delta.
    pub max_abs_delta: Decimal,
    /// Maximum total absolute gamma.
    pub max_abs_gamma: Decimal,
    /// Maximum simultaneous working orders.
    pub max_open_orders: usize,
    /// Maximum order attempts in the selected rate window.
    pub max_order_rate: u32,
    /// Instruments explicitly allowed. Empty means all except restricted.
    pub allowed_instruments: BTreeSet<String>,
    /// Instruments explicitly blocked.
    pub restricted_instruments: BTreeSet<String>,
    /// Per-sector gross limits.
    pub sector_limits: BTreeMap<String, Decimal>,
    /// Per-asset-class gross limits.
    pub asset_class_limits: BTreeMap<String, Decimal>,
    /// Per-currency gross limits.
    pub currency_limits: BTreeMap<String, Decimal>,
    /// Per-strategy gross limits.
    pub strategy_limits: BTreeMap<String, Decimal>,
    /// Optional maximum news slippage allowed in basis points.
    pub max_news_slippage_bps: Option<Decimal>,
    /// Optional maximum spread multiplier allowed in basis points.
    pub max_spread_multiplier_bps: Option<Decimal>,
}

impl PortfolioRiskPolicy {
    /// Every field of the policy, rendered in a fixed order, for a
    /// configuration fingerprint.
    ///
    /// The destructuring names every field and has no `..`, so a field added
    /// later does not compile until it is rendered here too. PAPER and LIVE
    /// each listed the fields by hand and both left out five limits, so a
    /// journal reopened, and a LIVE approval stayed valid, under changed limits
    /// (delivery state E7.4).
    pub fn canonical_parts(&self) -> Vec<String> {
        let PortfolioRiskPolicy {
            version,
            global_kill_switch,
            max_gross_exposure,
            max_abs_net_exposure,
            max_leverage_bps,
            max_concentration_bps,
            max_daily_loss,
            max_drawdown_bps,
            max_margin_utilization_bps,
            max_abs_delta,
            max_abs_gamma,
            max_open_orders,
            max_order_rate,
            allowed_instruments,
            restricted_instruments,
            sector_limits,
            asset_class_limits,
            currency_limits,
            strategy_limits,
            max_news_slippage_bps,
            max_spread_multiplier_bps,
        } = self;
        let limits = |map: &BTreeMap<String, Decimal>| {
            map.iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("|")
        };
        let set = |values: &BTreeSet<String>| values.iter().cloned().collect::<Vec<_>>().join("|");
        let optional = |value: &Option<Decimal>| {
            value.map_or_else(|| "none".to_owned(), |value| value.to_string())
        };
        vec![
            format!("version={version}"),
            format!("global_kill_switch={global_kill_switch}"),
            format!("max_gross_exposure={max_gross_exposure}"),
            format!("max_abs_net_exposure={max_abs_net_exposure}"),
            format!("max_leverage_bps={max_leverage_bps}"),
            format!("max_concentration_bps={max_concentration_bps}"),
            format!("max_daily_loss={max_daily_loss}"),
            format!("max_drawdown_bps={max_drawdown_bps}"),
            format!("max_margin_utilization_bps={max_margin_utilization_bps}"),
            format!("max_abs_delta={max_abs_delta}"),
            format!("max_abs_gamma={max_abs_gamma}"),
            format!("max_open_orders={max_open_orders}"),
            format!("max_order_rate={max_order_rate}"),
            format!("allowed_instruments={}", set(allowed_instruments)),
            format!("restricted_instruments={}", set(restricted_instruments)),
            format!("sector_limits={}", limits(sector_limits)),
            format!("asset_class_limits={}", limits(asset_class_limits)),
            format!("currency_limits={}", limits(currency_limits)),
            format!("strategy_limits={}", limits(strategy_limits)),
            format!("max_news_slippage_bps={}", optional(max_news_slippage_bps)),
            format!(
                "max_spread_multiplier_bps={}",
                optional(max_spread_multiplier_bps)
            ),
        ]
    }

    /// Validates policy identity, ranges, and bucket keys.
    pub fn validate(&self) -> Result<(), RiskError> {
        validate_canonical_id("portfolio risk version", &self.version)?;
        let ten_thousand = Decimal::from_integer(10_000)?;
        if self.max_gross_exposure <= Decimal::ZERO
            || self.max_abs_net_exposure <= Decimal::ZERO
            || self.max_leverage_bps <= Decimal::ZERO
            || self.max_leverage_bps > ten_thousand.checked_mul(Decimal::from_integer(100)?)?
            || self.max_concentration_bps <= Decimal::ZERO
            || self.max_concentration_bps > ten_thousand
            || self.max_daily_loss < Decimal::ZERO
            || self.max_drawdown_bps < Decimal::ZERO
            || self.max_drawdown_bps > ten_thousand
            || self.max_margin_utilization_bps < Decimal::ZERO
            || self.max_margin_utilization_bps > ten_thousand
            || self.max_abs_delta < Decimal::ZERO
            || self.max_abs_gamma < Decimal::ZERO
            || self.max_open_orders == 0
            || self.max_order_rate == 0
        {
            return Err(RiskError("invalid portfolio risk limits".to_owned()));
        }
        for instrument in self
            .allowed_instruments
            .iter()
            .chain(&self.restricted_instruments)
        {
            validate_canonical_id("risk instrument permission", instrument)?;
        }
        if self
            .allowed_instruments
            .iter()
            .any(|instrument| self.restricted_instruments.contains(instrument))
        {
            return Err(RiskError(
                "an instrument cannot be both allowed and restricted".to_owned(),
            ));
        }
        for limits in [
            &self.sector_limits,
            &self.asset_class_limits,
            &self.currency_limits,
            &self.strategy_limits,
        ] {
            for (bucket, limit) in limits {
                if bucket.is_empty() || *limit <= Decimal::ZERO {
                    return Err(RiskError("invalid aggregate bucket limit".to_owned()));
                }
            }
        }
        if let Some(slippage) = self.max_news_slippage_bps {
            if slippage <= Decimal::ZERO || slippage > ten_thousand {
                return Err(RiskError("invalid max_news_slippage_bps limit".to_owned()));
            }
        }
        if let Some(spread_mult) = self.max_spread_multiplier_bps {
            if spread_mult <= Decimal::ZERO {
                return Err(RiskError(
                    "invalid max_spread_multiplier_bps limit".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// Evaluates news shock price collar and spread widening protections.
pub fn evaluate_news_shock_collar(
    policy: &PortfolioRiskPolicy,
    reference_price: Decimal,
    requested_price: Decimal,
    current_spread: Option<Decimal>,
    baseline_spread: Option<Decimal>,
) -> Result<Vec<String>, RiskError> {
    policy.validate()?;
    let mut reasons = Vec::new();
    if let Some(max_slippage) = policy.max_news_slippage_bps {
        let deviation = follon_domain::price_deviation_bps(reference_price, requested_price)?;
        if deviation > max_slippage {
            reasons.push("NEWS_SLIPPAGE_EXCEEDED".to_owned());
        }
    }
    if let (Some(max_mult_bps), Some(spread), Some(baseline)) = (
        policy.max_spread_multiplier_bps,
        current_spread,
        baseline_spread,
    ) {
        if baseline > Decimal::ZERO {
            let max_allowed = baseline
                .checked_mul(max_mult_bps)?
                .checked_div(Decimal::from_integer(10_000)?)?;
            if spread > max_allowed {
                reasons.push("LIQUIDITY_HOLE_DETECTED".to_owned());
            }
        }
    }
    Ok(reasons)
}
