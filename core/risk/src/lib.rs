//! Portfolio-wide deterministic risk aggregation and pre-trade controls.
//!
//! The evaluator combines every account/strategy/asset/currency exposure before
//! deciding. It produces evidence only; OMS remains the sole order authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use follon_domain::{validate_canonical_id, Decimal, Side};
use follon_fx::{FxPricingSnapshot, FxValueDate};

pub mod allocation;

pub use allocation::{
    CapitalAllocationCouncil, CapitalAllocationProposal, ProposalStatus,
    StrategyAllocationRecommendation,
};

/// Aggregate risk evaluation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskError(pub String);

impl fmt::Display for RiskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RiskError {}

impl From<follon_domain::DecimalError> for RiskError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DomainError> for RiskError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}

/// Fully attributed marked position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskPosition {
    /// Account identity.
    pub account_id: String,
    /// Strategy identity.
    pub strategy_id: String,
    /// Instrument identity.
    pub instrument_id: String,
    /// Stable asset-class label.
    pub asset_class: String,
    /// Stable sector or risk-bucket label.
    pub sector: String,
    /// Three-letter currency.
    pub currency: String,
    /// Signed quantity; shorts are negative.
    pub quantity: Decimal,
    /// Positive mark in instrument currency.
    pub mark_price: Decimal,
    /// Positive contract multiplier.
    pub multiplier: Decimal,
    /// Signed position delta in underlying units.
    pub delta: Decimal,
    /// Signed position gamma.
    pub gamma: Decimal,
}

impl RiskPosition {
    fn validate(&self) -> Result<(), RiskError> {
        for (name, value) in [
            ("risk account_id", &self.account_id),
            ("risk strategy_id", &self.strategy_id),
            ("risk instrument_id", &self.instrument_id),
            ("risk asset_class", &self.asset_class),
            ("risk sector", &self.sector),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.mark_price <= Decimal::ZERO
            || self.multiplier <= Decimal::ZERO
        {
            return Err(RiskError("invalid marked risk position".to_owned()));
        }
        Ok(())
    }

    fn signed_exposure(&self) -> Result<Decimal, RiskError> {
        Ok(self
            .quantity
            .checked_mul(self.mark_price)?
            .checked_mul(self.multiplier)?)
    }
}

/// Existing working order used for order-rate and self-trade protection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestingOrder {
    /// Order identity.
    pub order_id: String,
    /// Account identity.
    pub account_id: String,
    /// Instrument identity.
    pub instrument_id: String,
    /// Resting side.
    pub side: Side,
}

/// Candidate order evaluated as part of the aggregate portfolio.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateOrder {
    /// Stable intent identity.
    pub intent_id: String,
    /// Account identity.
    pub account_id: String,
    /// Strategy identity.
    pub strategy_id: String,
    /// Instrument identity.
    pub instrument_id: String,
    /// Asset-class bucket.
    pub asset_class: String,
    /// Sector bucket.
    pub sector: String,
    /// Currency bucket.
    pub currency: String,
    /// Side.
    pub side: Side,
    /// Positive quantity.
    pub quantity: Decimal,
    /// Positive fresh mark.
    pub mark_price: Decimal,
    /// Positive multiplier.
    pub multiplier: Decimal,
    /// Candidate delta contribution.
    pub delta: Decimal,
    /// Candidate gamma contribution.
    pub gamma: Decimal,
}

impl CandidateOrder {
    fn validate(&self) -> Result<(), RiskError> {
        let position = RiskPosition {
            account_id: self.account_id.clone(),
            strategy_id: self.strategy_id.clone(),
            instrument_id: self.instrument_id.clone(),
            asset_class: self.asset_class.clone(),
            sector: self.sector.clone(),
            currency: self.currency.clone(),
            quantity: self.quantity,
            mark_price: self.mark_price,
            multiplier: self.multiplier,
            delta: self.delta,
            gamma: self.gamma,
        };
        validate_canonical_id("candidate intent_id", &self.intent_id)?;
        position.validate()?;
        if self.quantity <= Decimal::ZERO {
            return Err(RiskError("candidate quantity must be positive".to_owned()));
        }
        Ok(())
    }

    fn signed_exposure(&self) -> Result<Decimal, RiskError> {
        let unsigned = self
            .quantity
            .checked_mul(self.mark_price)?
            .checked_mul(self.multiplier)?;
        match self.side {
            Side::Buy => Ok(unsigned),
            Side::Sell => negate(unsigned),
        }
    }
}

/// Identifiers and bucket selection for one FX candidate created from frozen pricing evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxRiskOrderIdentity {
    /// Stable intent identity.
    pub intent_id: String,
    /// Target account identity.
    pub account_id: String,
    /// Originating strategy identity.
    pub strategy_id: String,
    /// Canonical target FX instrument identity.
    pub instrument_id: String,
    /// Stable risk sector, normally `fx`.
    pub sector: String,
}

/// Explicit replay-time context for selecting an FX risk mark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxRiskPricingContext {
    /// Contract value date selected for the mark.
    pub value_date: FxValueDate,
    /// Explicit risk-evaluation timestamp from the replay or request context.
    pub as_of: String,
    /// Maximum accepted source-receive age.
    pub maximum_quote_age_seconds: i64,
}

/// A generic risk candidate with the exact FX price evidence that created it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxRiskCandidate {
    /// Candidate supplied to the ordinary portfolio risk evaluator.
    pub candidate: CandidateOrder,
    /// Immutable market-data snapshot used to derive the candidate mark.
    pub pricing_snapshot_id: String,
    /// Immutable pricing/reference contract version.
    pub pricing_reference_version: String,
    /// Contract value date selected for the mark.
    pub value_date: FxValueDate,
}

impl FxRiskCandidate {
    /// Creates a candidate from an explicit-time FX price snapshot.
    ///
    /// The result contains no order transport. It must still be supplied to
    /// [`evaluate_portfolio_risk`] and then enter the normal Risk/OMS flow.
    pub fn from_pricing_snapshot(
        identity: FxRiskOrderIdentity,
        snapshot: &FxPricingSnapshot,
        side: Side,
        quantity: Decimal,
        multiplier: Decimal,
        pricing: FxRiskPricingContext,
    ) -> Result<Self, RiskError> {
        snapshot.validate().map_err(|error| RiskError(error.0))?;
        if identity.instrument_id != snapshot.instrument_id {
            return Err(RiskError(
                "FX risk candidate instrument does not match pricing evidence".to_owned(),
            ));
        }
        let mark_price = snapshot
            .midpoint_at(
                &pricing.value_date,
                &pricing.as_of,
                pricing.maximum_quote_age_seconds,
            )
            .map_err(|error| RiskError(error.0))?;
        if quantity <= Decimal::ZERO || multiplier <= Decimal::ZERO {
            return Err(RiskError(
                "FX risk candidate quantity and multiplier must be positive".to_owned(),
            ));
        }
        let absolute_delta = quantity.checked_mul(multiplier)?;
        let delta = match side {
            Side::Buy => absolute_delta,
            Side::Sell => negate(absolute_delta)?,
        };
        let candidate = CandidateOrder {
            intent_id: identity.intent_id,
            account_id: identity.account_id,
            strategy_id: identity.strategy_id,
            instrument_id: identity.instrument_id,
            asset_class: snapshot.product.risk_bucket().to_owned(),
            sector: identity.sector,
            currency: snapshot.pair.quote_currency().to_owned(),
            side,
            quantity,
            mark_price,
            multiplier,
            delta,
            gamma: Decimal::ZERO,
        };
        candidate.validate()?;
        Ok(Self {
            candidate,
            pricing_snapshot_id: snapshot.snapshot_id.clone(),
            pricing_reference_version: snapshot.reference_version.clone(),
            value_date: pricing.value_date,
        })
    }
}

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
    /// Fully attributed positions.
    pub positions: Vec<RiskPosition>,
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

/// Exact aggregate metrics retained with each decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateRiskMetrics {
    /// Gross marked exposure.
    pub gross_exposure: Decimal,
    /// Signed net marked exposure.
    pub net_exposure: Decimal,
    /// Gross/equity ratio in basis points.
    pub leverage_bps: Decimal,
    /// Largest-position/gross ratio in basis points.
    pub concentration_bps: Decimal,
    /// Peak-to-current drawdown in basis points.
    pub drawdown_bps: Decimal,
    /// Margin/equity ratio in basis points.
    pub margin_utilization_bps: Decimal,
    /// Total signed delta.
    pub total_delta: Decimal,
    /// Total signed gamma.
    pub total_gamma: Decimal,
    /// Per-sector gross exposure.
    pub sector_gross: BTreeMap<String, Decimal>,
    /// Per-asset-class gross exposure.
    pub asset_class_gross: BTreeMap<String, Decimal>,
    /// Per-currency gross exposure.
    pub currency_gross: BTreeMap<String, Decimal>,
    /// Per-strategy gross exposure.
    pub strategy_gross: BTreeMap<String, Decimal>,
}

/// Explainable aggregate decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortfolioRiskDecision {
    /// Whether every policy condition passed.
    pub approved: bool,
    /// Stable machine-readable reasons.
    pub reason_codes: Vec<String>,
    /// Policy identity.
    pub policy_version: String,
    /// Exact post-candidate aggregate metrics.
    pub metrics: AggregateRiskMetrics,
}

/// Evaluates a candidate against every account and portfolio bucket.
pub fn evaluate_portfolio_risk(
    policy: &PortfolioRiskPolicy,
    snapshot: &PortfolioRiskSnapshot,
    candidate: Option<&CandidateOrder>,
) -> Result<PortfolioRiskDecision, RiskError> {
    policy.validate()?;
    if snapshot.equity <= Decimal::ZERO
        || snapshot.peak_equity <= Decimal::ZERO
        || snapshot.margin_used < Decimal::ZERO
        || snapshot.positions.len() > 1_000_000
        || snapshot.resting_orders.len() > 1_000_000
    {
        return Err(RiskError("invalid portfolio risk snapshot".to_owned()));
    }
    let mut positions = snapshot.positions.clone();
    let mut reasons = Vec::new();
    if let Some(order) = candidate {
        order.validate()?;
        if policy.restricted_instruments.contains(&order.instrument_id) {
            reasons.push("RESTRICTED_INSTRUMENT".to_owned());
        }
        if !policy.allowed_instruments.is_empty()
            && !policy.allowed_instruments.contains(&order.instrument_id)
        {
            reasons.push("INSTRUMENT_NOT_PERMITTED".to_owned());
        }
        if snapshot.resting_orders.iter().any(|resting| {
            resting.account_id == order.account_id
                && resting.instrument_id == order.instrument_id
                && resting.side != order.side
        }) {
            reasons.push("SELF_TRADE_RISK".to_owned());
        }
        let signed_quantity = match order.side {
            Side::Buy => order.quantity,
            Side::Sell => negate(order.quantity)?,
        };
        positions.push(RiskPosition {
            account_id: order.account_id.clone(),
            strategy_id: order.strategy_id.clone(),
            instrument_id: order.instrument_id.clone(),
            asset_class: order.asset_class.clone(),
            sector: order.sector.clone(),
            currency: order.currency.clone(),
            quantity: signed_quantity,
            mark_price: order.mark_price,
            multiplier: order.multiplier,
            delta: order.delta,
            gamma: order.gamma,
        });
        let _ = order.signed_exposure()?;
    }
    let metrics = aggregate_metrics(
        &positions,
        snapshot.equity,
        snapshot.peak_equity,
        snapshot.margin_used,
    )?;
    if policy.global_kill_switch {
        reasons.push("GLOBAL_KILL_SWITCH_ACTIVE".to_owned());
    }
    if metrics.gross_exposure > policy.max_gross_exposure {
        reasons.push("MAX_GROSS_EXPOSURE_EXCEEDED".to_owned());
    }
    if absolute(metrics.net_exposure)? > policy.max_abs_net_exposure {
        reasons.push("MAX_NET_EXPOSURE_EXCEEDED".to_owned());
    }
    if metrics.leverage_bps > policy.max_leverage_bps {
        reasons.push("MAX_LEVERAGE_EXCEEDED".to_owned());
    }
    if metrics.concentration_bps > policy.max_concentration_bps {
        reasons.push("MAX_CONCENTRATION_EXCEEDED".to_owned());
    }
    if snapshot.daily_pnl < negate(policy.max_daily_loss)? {
        reasons.push("MAX_DAILY_LOSS_EXCEEDED".to_owned());
    }
    if metrics.drawdown_bps > policy.max_drawdown_bps {
        reasons.push("MAX_DRAWDOWN_EXCEEDED".to_owned());
    }
    if metrics.margin_utilization_bps > policy.max_margin_utilization_bps {
        reasons.push("MAX_MARGIN_UTILIZATION_EXCEEDED".to_owned());
    }
    if absolute(metrics.total_delta)? > policy.max_abs_delta {
        reasons.push("MAX_DELTA_EXCEEDED".to_owned());
    }
    if absolute(metrics.total_gamma)? > policy.max_abs_gamma {
        reasons.push("MAX_GAMMA_EXCEEDED".to_owned());
    }
    if snapshot.resting_orders.len() + usize::from(candidate.is_some()) > policy.max_open_orders {
        reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
    }
    if snapshot
        .recent_order_count
        .saturating_add(u32::from(candidate.is_some()))
        > policy.max_order_rate
    {
        reasons.push("MAX_ORDER_RATE_EXCEEDED".to_owned());
    }
    apply_bucket_limits(
        "SECTOR_LIMIT_EXCEEDED",
        &metrics.sector_gross,
        &policy.sector_limits,
        &mut reasons,
    );
    apply_bucket_limits(
        "ASSET_CLASS_LIMIT_EXCEEDED",
        &metrics.asset_class_gross,
        &policy.asset_class_limits,
        &mut reasons,
    );
    apply_bucket_limits(
        "CURRENCY_LIMIT_EXCEEDED",
        &metrics.currency_gross,
        &policy.currency_limits,
        &mut reasons,
    );
    apply_bucket_limits(
        "STRATEGY_LIMIT_EXCEEDED",
        &metrics.strategy_gross,
        &policy.strategy_limits,
        &mut reasons,
    );
    reasons.sort();
    reasons.dedup();
    let approved = reasons.is_empty();
    if approved {
        reasons.push("APPROVED".to_owned());
    }
    Ok(PortfolioRiskDecision {
        approved,
        reason_codes: reasons,
        policy_version: policy.version.clone(),
        metrics,
    })
}

fn aggregate_metrics(
    positions: &[RiskPosition],
    equity: Decimal,
    peak_equity: Decimal,
    margin_used: Decimal,
) -> Result<AggregateRiskMetrics, RiskError> {
    let mut gross = Decimal::ZERO;
    let mut net = Decimal::ZERO;
    let mut largest = Decimal::ZERO;
    let mut delta = Decimal::ZERO;
    let mut gamma = Decimal::ZERO;
    let mut sector = BTreeMap::new();
    let mut asset = BTreeMap::new();
    let mut currency = BTreeMap::new();
    let mut strategy = BTreeMap::new();
    for position in positions {
        position.validate()?;
        let signed = position.signed_exposure()?;
        let absolute_exposure = absolute(signed)?;
        gross = gross.checked_add(absolute_exposure)?;
        net = net.checked_add(signed)?;
        largest = largest.max(absolute_exposure);
        delta = delta.checked_add(position.delta)?;
        gamma = gamma.checked_add(position.gamma)?;
        add_bucket(&mut sector, &position.sector, absolute_exposure)?;
        add_bucket(&mut asset, &position.asset_class, absolute_exposure)?;
        add_bucket(&mut currency, &position.currency, absolute_exposure)?;
        add_bucket(&mut strategy, &position.strategy_id, absolute_exposure)?;
    }
    let drawdown = if peak_equity > equity {
        ratio_bps(peak_equity.checked_sub(equity)?, peak_equity)?
    } else {
        Decimal::ZERO
    };
    Ok(AggregateRiskMetrics {
        gross_exposure: gross,
        net_exposure: net,
        leverage_bps: ratio_bps(gross, equity)?,
        concentration_bps: if gross == Decimal::ZERO {
            Decimal::ZERO
        } else {
            ratio_bps(largest, gross)?
        },
        drawdown_bps: drawdown,
        margin_utilization_bps: ratio_bps(margin_used, equity)?,
        total_delta: delta,
        total_gamma: gamma,
        sector_gross: sector,
        asset_class_gross: asset,
        currency_gross: currency,
        strategy_gross: strategy,
    })
}

fn add_bucket(
    buckets: &mut BTreeMap<String, Decimal>,
    key: &str,
    amount: Decimal,
) -> Result<(), RiskError> {
    let current = buckets.get(key).copied().unwrap_or(Decimal::ZERO);
    buckets.insert(key.to_owned(), current.checked_add(amount)?);
    Ok(())
}

fn apply_bucket_limits(
    code: &str,
    actual: &BTreeMap<String, Decimal>,
    limits: &BTreeMap<String, Decimal>,
    reasons: &mut Vec<String>,
) {
    for (bucket, amount) in actual {
        if limits.get(bucket).is_some_and(|limit| amount > limit) {
            reasons.push(format!("{code}:{bucket}"));
        }
    }
}

fn ratio_bps(numerator: Decimal, denominator: Decimal) -> Result<Decimal, RiskError> {
    if numerator < Decimal::ZERO || denominator <= Decimal::ZERO {
        return Err(RiskError("risk ratio requires valid inputs".to_owned()));
    }
    Ok(numerator
        .checked_mul(Decimal::from_integer(10_000)?)?
        .checked_div(denominator)?)
}

fn absolute(value: Decimal) -> Result<Decimal, RiskError> {
    if value >= Decimal::ZERO {
        Ok(value)
    } else {
        negate(value)
    }
}

fn negate(value: Decimal) -> Result<Decimal, RiskError> {
    value
        .scaled()
        .checked_neg()
        .map(Decimal::from_scaled)
        .ok_or_else(|| RiskError("risk decimal negation overflowed".to_owned()))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use follon_fx::{FxOutrightQuote, FxPair, FxPriceTerms, FxProduct};

    use super::*;

    fn amount(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn policy() -> PortfolioRiskPolicy {
        PortfolioRiskPolicy {
            version: "portfolio.risk.v1".to_owned(),
            global_kill_switch: false,
            max_gross_exposure: amount("100000"),
            max_abs_net_exposure: amount("50000"),
            max_leverage_bps: amount("20000"),
            max_concentration_bps: amount("9000"),
            max_daily_loss: amount("5000"),
            max_drawdown_bps: amount("1000"),
            max_margin_utilization_bps: amount("5000"),
            max_abs_delta: amount("1000"),
            max_abs_gamma: amount("100"),
            max_open_orders: 10,
            max_order_rate: 20,
            allowed_instruments: BTreeSet::new(),
            restricted_instruments: BTreeSet::new(),
            sector_limits: BTreeMap::from([("technology".to_owned(), amount("40000"))]),
            asset_class_limits: BTreeMap::new(),
            currency_limits: BTreeMap::from([("USD".to_owned(), amount("100000"))]),
            strategy_limits: BTreeMap::new(),
            max_news_slippage_bps: None,
            max_spread_multiplier_bps: None,
        }
    }

    fn position(instrument: &str, quantity: &str, price: &str) -> RiskPosition {
        RiskPosition {
            account_id: "account.main".to_owned(),
            strategy_id: "strategy.alpha".to_owned(),
            instrument_id: instrument.to_owned(),
            asset_class: "equity".to_owned(),
            sector: "technology".to_owned(),
            currency: "USD".to_owned(),
            quantity: amount(quantity),
            mark_price: amount(price),
            multiplier: amount("1"),
            delta: amount(quantity),
            gamma: Decimal::ZERO,
        }
    }

    fn fx_spot_snapshot() -> FxPricingSnapshot {
        FxPricingSnapshot {
            snapshot_id: "fx.snapshot.001".to_owned(),
            reference_version: "fx.price.v1".to_owned(),
            instrument_id: "instrument.fx.eur-usd".to_owned(),
            product: FxProduct::Spot,
            pair: FxPair::new("EUR", "USD").unwrap(),
            terms: FxPriceTerms::Outright {
                value_date: FxValueDate::new("2026-01-06").unwrap(),
                quote: FxOutrightQuote {
                    bid: amount("1.1000"),
                    ask: amount("1.1002"),
                },
            },
            source_id: "source.fixture".to_owned(),
            source_sequence: 1,
            source_time: "2026-01-02T10:00:00Z".to_owned(),
            received_at: "2026-01-02T10:00:01Z".to_owned(),
        }
    }

    #[test]
    fn aggregates_long_short_and_bucket_exposure_exactly() {
        let snapshot = PortfolioRiskSnapshot {
            equity: amount("50000"),
            peak_equity: amount("52000"),
            daily_pnl: amount("-1000"),
            margin_used: amount("10000"),
            positions: vec![
                position("instrument.a", "200", "100"),
                position("instrument.b", "-100", "50"),
            ],
            resting_orders: vec![],
            recent_order_count: 0,
        };
        let decision = evaluate_portfolio_risk(&policy(), &snapshot, None).expect("decision");
        assert!(decision.approved);
        assert_eq!(decision.metrics.gross_exposure, amount("25000"));
        assert_eq!(decision.metrics.net_exposure, amount("15000"));
        assert_eq!(decision.metrics.leverage_bps, amount("5000"));
        assert_eq!(decision.metrics.drawdown_bps, amount("384.61538461"));
    }

    #[test]
    fn candidate_is_checked_for_permissions_self_trade_rate_and_aggregate_limits() {
        let mut policy = policy();
        policy
            .restricted_instruments
            .insert("instrument.blocked".to_owned());
        let snapshot = PortfolioRiskSnapshot {
            equity: amount("10000"),
            peak_equity: amount("12000"),
            daily_pnl: amount("-6000"),
            margin_used: amount("6000"),
            positions: vec![position("instrument.existing", "300", "100")],
            resting_orders: vec![RestingOrder {
                order_id: "order.resting".to_owned(),
                account_id: "account.main".to_owned(),
                instrument_id: "instrument.blocked".to_owned(),
                side: Side::Sell,
            }],
            recent_order_count: 20,
        };
        let candidate = CandidateOrder {
            intent_id: "intent.candidate".to_owned(),
            account_id: "account.main".to_owned(),
            strategy_id: "strategy.alpha".to_owned(),
            instrument_id: "instrument.blocked".to_owned(),
            asset_class: "equity".to_owned(),
            sector: "technology".to_owned(),
            currency: "USD".to_owned(),
            side: Side::Buy,
            quantity: amount("200"),
            mark_price: amount("100"),
            multiplier: amount("1"),
            delta: amount("200"),
            gamma: Decimal::ZERO,
        };
        let decision =
            evaluate_portfolio_risk(&policy, &snapshot, Some(&candidate)).expect("decision");
        assert!(!decision.approved);
        for code in [
            "RESTRICTED_INSTRUMENT",
            "SELF_TRADE_RISK",
            "MAX_ORDER_RATE_EXCEEDED",
            "MAX_DAILY_LOSS_EXCEEDED",
            "MAX_MARGIN_UTILIZATION_EXCEEDED",
            "SECTOR_LIMIT_EXCEEDED:technology",
        ] {
            assert!(
                decision.reason_codes.contains(&code.to_owned()),
                "missing {code}"
            );
        }
    }

    #[test]
    fn test_news_shock_collar_protection() {
        let mut policy = policy();
        policy.max_news_slippage_bps = Some(amount("50")); // 50 BPS max price drift
        policy.max_spread_multiplier_bps = Some(amount("30000")); // 3.0x max spread expansion

        // 1. Normal price (100 -> 100.40 = 40 BPS deviation): Passed
        let clean_reasons = evaluate_news_shock_collar(
            &policy,
            amount("100"),
            amount("100.40"),
            Some(amount("0.05")),
            Some(amount("0.02")),
        )
        .expect("reasons");
        assert!(clean_reasons.is_empty());

        // 2. High slippage (100 -> 101.00 = 100 BPS deviation > 50 BPS max): Failed
        // 3. Liquidity hole (current spread 0.10 > 3.0 * baseline 0.02 = 0.06): Failed
        let shock_reasons = evaluate_news_shock_collar(
            &policy,
            amount("100"),
            amount("101.00"),
            Some(amount("0.10")),
            Some(amount("0.02")),
        )
        .expect("reasons");
        assert!(shock_reasons.contains(&"NEWS_SLIPPAGE_EXCEEDED".to_owned()));
        assert!(shock_reasons.contains(&"LIQUIDITY_HOLE_DETECTED".to_owned()));
    }

    #[test]
    fn fx_candidate_uses_frozen_price_evidence_and_normal_risk_policy() {
        let snapshot = fx_spot_snapshot();
        let candidate = FxRiskCandidate::from_pricing_snapshot(
            FxRiskOrderIdentity {
                intent_id: "intent.fx.001".to_owned(),
                account_id: "account.paper.001".to_owned(),
                strategy_id: "strategy.fx.alpha".to_owned(),
                instrument_id: "instrument.fx.eur-usd".to_owned(),
                sector: "fx".to_owned(),
            },
            &snapshot,
            Side::Buy,
            amount("10"),
            amount("1"),
            FxRiskPricingContext {
                value_date: FxValueDate::new("2026-01-06").unwrap(),
                as_of: "2026-01-02T10:00:03Z".to_owned(),
                maximum_quote_age_seconds: 5,
            },
        )
        .unwrap();
        assert_eq!(candidate.candidate.asset_class, "fx_spot");
        assert_eq!(candidate.candidate.currency, "USD");
        assert_eq!(candidate.candidate.mark_price, amount("1.1001"));
        assert_eq!(candidate.candidate.delta, amount("10"));

        let mut fx_limited_policy = policy();
        fx_limited_policy.asset_class_limits =
            BTreeMap::from([("fx_spot".to_owned(), amount("10"))]);
        let decision = evaluate_portfolio_risk(
            &fx_limited_policy,
            &PortfolioRiskSnapshot {
                equity: amount("50000"),
                peak_equity: amount("50000"),
                daily_pnl: Decimal::ZERO,
                margin_used: Decimal::ZERO,
                positions: vec![],
                resting_orders: vec![],
                recent_order_count: 0,
            },
            Some(&candidate.candidate),
        )
        .unwrap();
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"ASSET_CLASS_LIMIT_EXCEEDED:fx_spot".to_owned()));

        assert!(FxRiskCandidate::from_pricing_snapshot(
            FxRiskOrderIdentity {
                instrument_id: "instrument.fx.other".to_owned(),
                ..FxRiskOrderIdentity {
                    intent_id: "intent.fx.002".to_owned(),
                    account_id: "account.paper.001".to_owned(),
                    strategy_id: "strategy.fx.alpha".to_owned(),
                    instrument_id: "instrument.fx.eur-usd".to_owned(),
                    sector: "fx".to_owned(),
                }
            },
            &snapshot,
            Side::Buy,
            amount("1"),
            amount("1"),
            FxRiskPricingContext {
                value_date: FxValueDate::new("2026-01-06").unwrap(),
                as_of: "2026-01-02T10:00:03Z".to_owned(),
                maximum_quote_age_seconds: 5,
            },
        )
        .is_err());
    }
}
