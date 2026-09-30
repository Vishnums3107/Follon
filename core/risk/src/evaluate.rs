//! Aggregate metrics and the portfolio risk evaluation entry points.

use std::collections::{BTreeMap, BTreeSet};

use follon_domain::{Decimal, Side};

use crate::*;

/// Exact aggregate metrics retained with each decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateRiskMetrics {
    /// Gross marked exposure.
    pub gross_exposure: Decimal,
    /// Signed net marked exposure.
    pub net_exposure: Decimal,
    /// Largest absolute net exposure under any subset of working fills.
    pub possible_abs_net_exposure: Decimal,
    /// Gross/equity ratio in basis points.
    pub leverage_bps: Decimal,
    /// Largest instrument's gross contribution / total gross, in basis points.
    pub concentration_bps: Decimal,
    /// Largest concentration under any subset of working fills.
    pub possible_concentration_bps: Decimal,
    /// Peak-to-current drawdown in basis points.
    pub drawdown_bps: Decimal,
    /// Margin/equity ratio in basis points.
    pub margin_utilization_bps: Decimal,
    /// Total signed delta.
    pub total_delta: Decimal,
    /// Largest absolute delta under any subset of working fills.
    pub possible_abs_delta: Decimal,
    /// Total signed gamma.
    pub total_gamma: Decimal,
    /// Largest absolute gamma under any subset of working fills.
    pub possible_abs_gamma: Decimal,
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
    match candidate {
        Some(order) => {
            evaluate_portfolio_risk_with_candidates(policy, snapshot, std::slice::from_ref(order))
        }
        None => evaluate_portfolio_risk_with_candidates(policy, snapshot, &[]),
    }
}

/// Evaluates several simultaneous candidate legs as one atomic group.
///
/// A multi-leg combination executes atomically or not at all, so its legs have
/// to be assessed *together*: two legs that each sit under a concentration or
/// bucket limit on their own can breach it jointly, and evaluating them one at
/// a time would approve exactly that. Every candidate is therefore added to the
/// position set before any aggregate metric is computed.
///
/// The group still counts as **one** order against the open-order and
/// order-rate limits, because the OMS submits and tracks an atomic combination
/// as a single order. Counting legs there would make an ordinary four-leg
/// structure look like a rate breach.
pub fn evaluate_portfolio_risk_with_candidates(
    policy: &PortfolioRiskPolicy,
    snapshot: &PortfolioRiskSnapshot,
    candidates: &[CandidateOrder],
) -> Result<PortfolioRiskDecision, RiskError> {
    policy.validate()?;
    if snapshot.equity <= Decimal::ZERO
        || snapshot.peak_equity <= Decimal::ZERO
        || snapshot.margin_used < Decimal::ZERO
        || snapshot
            .positions
            .len()
            .saturating_add(snapshot.working_positions.len())
            > 1_000_000
        || snapshot.resting_orders.len() > 1_000_000
    {
        return Err(RiskError("invalid portfolio risk snapshot".to_owned()));
    }
    let mut base_positions = snapshot.positions.clone();
    let mut reasons = Vec::new();
    for order in candidates {
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
        base_positions.push(RiskPosition {
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
    let base_metrics = aggregate_metrics(
        &base_positions,
        snapshot.equity,
        snapshot.peak_equity,
        snapshot.margin_used,
    )?;
    let mut positions = base_positions.clone();
    positions.extend(snapshot.working_positions.iter().cloned());
    let mut metrics = aggregate_metrics(
        &positions,
        snapshot.equity,
        snapshot.peak_equity,
        snapshot.margin_used,
    )?;
    apply_working_bounds(
        &mut metrics,
        &base_metrics,
        &base_positions,
        &snapshot.working_positions,
    )?;
    if policy.global_kill_switch {
        reasons.push("GLOBAL_KILL_SWITCH_ACTIVE".to_owned());
    }
    if metrics.gross_exposure > policy.max_gross_exposure {
        reasons.push("MAX_GROSS_EXPOSURE_EXCEEDED".to_owned());
    }
    if metrics.possible_abs_net_exposure > policy.max_abs_net_exposure {
        reasons.push("MAX_NET_EXPOSURE_EXCEEDED".to_owned());
    }
    if metrics.leverage_bps > policy.max_leverage_bps {
        reasons.push("MAX_LEVERAGE_EXCEEDED".to_owned());
    }
    if metrics.possible_concentration_bps > policy.max_concentration_bps {
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
    if metrics.possible_abs_delta > policy.max_abs_delta {
        reasons.push("MAX_DELTA_EXCEEDED".to_owned());
    }
    if metrics.possible_abs_gamma > policy.max_abs_gamma {
        reasons.push("MAX_GAMMA_EXCEEDED".to_owned());
    }
    // One atomic group is one order here, however many legs it carries -- see
    // this function's own contract. Two things make that true. `!is_empty()`
    // rather than `len()` counts the candidate group once, and is exactly the
    // previous single-candidate behaviour for zero or one candidate. And
    // resting orders are counted by *distinct* identity, because a caller
    // reports one resting row per leg so the per-instrument self-trade check
    // above can see every leg -- those rows share one `order_id` and must not
    // inflate the open-order count. Plain orders each carry their own identity,
    // so this is unchanged for them.
    let resting_identities = snapshot
        .resting_orders
        .iter()
        .map(|resting| resting.order_id.as_str())
        .collect::<BTreeSet<_>>();
    if resting_identities.len() + usize::from(!candidates.is_empty()) > policy.max_open_orders {
        reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
    }
    if snapshot
        .recent_order_count
        .saturating_add(u32::from(!candidates.is_empty()))
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

/// Bound each signed metric by allowing any working row to fill or remain open.
/// Gross and bucket limits already sum every row's absolute contribution, so
/// adding all working rows is their upper bound. Concentration is different:
/// an unrelated working instrument must not dilute a concentrated candidate.
fn apply_working_bounds(
    metrics: &mut AggregateRiskMetrics,
    base: &AggregateRiskMetrics,
    base_positions: &[RiskPosition],
    working_positions: &[RiskPosition],
) -> Result<(), RiskError> {
    let mut net_low = base.net_exposure;
    let mut net_high = base.net_exposure;
    let mut delta_low = base.total_delta;
    let mut delta_high = base.total_delta;
    let mut gamma_low = base.total_gamma;
    let mut gamma_high = base.total_gamma;
    let mut base_instrument_gross = BTreeMap::new();
    let mut working_instrument_gross = BTreeMap::new();
    for position in base_positions {
        add_bucket(
            &mut base_instrument_gross,
            &position.instrument_id,
            absolute(position.signed_exposure()?)?,
        )?;
    }
    for position in working_positions {
        let signed = position.signed_exposure()?;
        if signed >= Decimal::ZERO {
            net_high = net_high.checked_add(signed)?;
        } else {
            net_low = net_low.checked_add(signed)?;
        }
        if position.delta >= Decimal::ZERO {
            delta_high = delta_high.checked_add(position.delta)?;
        } else {
            delta_low = delta_low.checked_add(position.delta)?;
        }
        if position.gamma >= Decimal::ZERO {
            gamma_high = gamma_high.checked_add(position.gamma)?;
        } else {
            gamma_low = gamma_low.checked_add(position.gamma)?;
        }
        add_bucket(
            &mut working_instrument_gross,
            &position.instrument_id,
            absolute(signed)?,
        )?;
    }
    metrics.possible_abs_net_exposure = absolute(net_low)?.max(absolute(net_high)?);
    metrics.possible_abs_delta = absolute(delta_low)?.max(absolute(delta_high)?);
    metrics.possible_abs_gamma = absolute(gamma_low)?.max(absolute(gamma_high)?);
    metrics.possible_concentration_bps = base.concentration_bps;
    for (instrument, working_gross) in working_instrument_gross {
        let instrument_gross = base_instrument_gross
            .get(&instrument)
            .copied()
            .unwrap_or(Decimal::ZERO)
            .checked_add(working_gross)?;
        let scenario_gross = base.gross_exposure.checked_add(working_gross)?;
        metrics.possible_concentration_bps = metrics
            .possible_concentration_bps
            .max(ratio_bps(instrument_gross, scenario_gross)?);
    }
    Ok(())
}

fn aggregate_metrics(
    positions: &[RiskPosition],
    equity: Decimal,
    peak_equity: Decimal,
    margin_used: Decimal,
) -> Result<AggregateRiskMetrics, RiskError> {
    let mut gross = Decimal::ZERO;
    let mut net = Decimal::ZERO;
    // A portfolio may hold several strategy rows and several working orders in
    // one instrument. Concentration is by instrument, so those rows must not
    // make the same exposure appear diversified (E7.14).
    let mut instrument_gross = BTreeMap::<String, Decimal>::new();
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
        add_bucket(
            &mut instrument_gross,
            &position.instrument_id,
            absolute_exposure,
        )?;
        delta = delta.checked_add(position.delta)?;
        gamma = gamma.checked_add(position.gamma)?;
        add_bucket(&mut sector, &position.sector, absolute_exposure)?;
        add_bucket(&mut asset, &position.asset_class, absolute_exposure)?;
        add_bucket(&mut currency, &position.currency, absolute_exposure)?;
        add_bucket(&mut strategy, &position.strategy_id, absolute_exposure)?;
    }
    let largest = instrument_gross
        .values()
        .copied()
        .max()
        .unwrap_or(Decimal::ZERO);
    let drawdown = if peak_equity > equity {
        ratio_bps(peak_equity.checked_sub(equity)?, peak_equity)?
    } else {
        Decimal::ZERO
    };
    let concentration_bps = if gross == Decimal::ZERO {
        Decimal::ZERO
    } else {
        ratio_bps(largest, gross)?
    };
    Ok(AggregateRiskMetrics {
        gross_exposure: gross,
        net_exposure: net,
        possible_abs_net_exposure: absolute(net)?,
        leverage_bps: ratio_bps(gross, equity)?,
        concentration_bps,
        possible_concentration_bps: concentration_bps,
        drawdown_bps: drawdown,
        margin_utilization_bps: ratio_bps(margin_used, equity)?,
        total_delta: delta,
        possible_abs_delta: absolute(delta)?,
        total_gamma: gamma,
        possible_abs_gamma: absolute(gamma)?,
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
