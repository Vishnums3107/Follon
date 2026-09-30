//! PAPER pre-trade risk policy and portfolio-risk composition.

use follon_domain::{validate_canonical_id, ComboIntent, Decimal};
use std::collections::BTreeMap;

use crate::*;

/// Operator-attested reference data the paper OMS cannot otherwise derive:
/// there is no sector/asset-class taxonomy anywhere in this codebase, so
/// composing `core/risk`'s bucket checks requires the operator to supply one
/// directly, exactly like every other existing caller of
/// `follon_risk::evaluate_portfolio_risk` (the `follon-risk-benchmark` CLI and
/// the gRPC `EvaluatePortfolioRisk` RPC) already does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentBucket {
    /// Stable asset-class label fed to the aggregate risk kernel.
    pub asset_class: String,
    /// Three-letter currency fed to the aggregate risk kernel.
    pub currency: String,
    /// Stable sector or risk-bucket label fed to the aggregate risk kernel.
    pub sector: String,
}

/// Composes `core/risk`'s aggregate portfolio kernel into the real paper
/// order-gating path: gross/net exposure, leverage, concentration, and
/// sector/asset-class/currency bucket limits (Slice 1); drawdown, via a
/// durable peak-equity high-water-mark (Slice 2a); and daily loss, via a
/// durable session-start equity baseline (Slice 2b). Present only when an
/// operator has explicitly configured it; `evaluate_risk` is byte-for-byte
/// unchanged when this is `None`.
///
/// Margin-utilization and strategy-bucket limits are deliberately not
/// exposed here: `core/paper` has no wired margin model, and `Portfolio` does
/// not attribute existing positions to a strategy, so those two specific
/// checks in `follon_risk::PortfolioRiskPolicy` would either be permanently
/// unreachable or actively misleading if wired in now. `evaluate_risk`
/// supplies fixed, non-configurable neutral values for exactly those fields
/// so they can never fire, rather than exposing operator-facing knobs that
/// silently do nothing. See
/// docs/06-delivery/14-master-plan-conformance-audit.md (row 5.7) for the
/// full boundary and the remaining Slice 2 scope.
#[derive(Clone, Debug, PartialEq)]
pub struct PortfolioRiskComposition {
    /// The aggregate policy's real, operator-configured bucket/exposure limits.
    pub policy: follon_risk::PortfolioRiskPolicy,
    /// Reference data for every instrument this account may hold or trade.
    /// An instrument missing from this map is treated as `"unclassified"` for
    /// asset class and sector, and as the account's own currency for
    /// currency -- an honest reflection of missing reference data, not a
    /// fabricated guess.
    pub instrument_buckets: BTreeMap<String, InstrumentBucket>,
    /// Slice-2c margin-utilization composition: an operator-authored initial/
    /// maintenance margin rate per asset class, reused from the same
    /// classification already required for bucket/exposure composition.
    /// `None` preserves `max_margin_utilization_bps`'s inert behavior exactly
    /// (`margin_used` stays fixed at zero). `core/paper` positions and cash
    /// are always denominated in the account's own currency -- this codebase
    /// has no cross-currency position support -- so this deliberately does
    /// not expose a base currency or FX freshness window: valuation always
    /// converts within a single currency, which requires no FX quote at all.
    pub margin_rates: Option<BTreeMap<String, follon_accounting::MarginRate>>,
}

/// Versioned pre-trade paper risk policy.
#[derive(Clone, Debug)]
pub struct PaperRiskPolicy {
    /// Immutable policy version emitted with every decision.
    pub version: String,
    /// Immutable exchange calendar version that authorizes paper-day evidence.
    pub trading_calendar_id: String,
    /// Maximum one-order quantity.
    pub max_order_quantity: Decimal,
    /// Maximum one-order estimated notional.
    pub max_order_notional: Decimal,
    /// Maximum absolute limit-price distance from the fresh mark in basis points.
    pub max_price_deviation_bps: Decimal,
    /// Maximum number of non-terminal OMS orders.
    pub max_open_orders: usize,
    /// Maximum absolute position quantity per instrument.
    pub max_position_quantity: Decimal,
    /// Maximum observed realized loss before new entry is blocked.
    pub max_realized_loss: Decimal,
    /// Maximum permitted age of the exact market observation used for an order decision.
    pub max_market_data_age_seconds: u64,
    /// Maximum order submissions permitted within `order_rate_window_seconds`.
    pub max_order_rate: u32,
    /// Rolling window, in seconds, over which `max_order_rate` is enforced.
    pub order_rate_window_seconds: u64,
    /// Slice-1 aggregate portfolio-risk composition; `None` preserves today's
    /// behavior exactly.
    pub portfolio_risk: Option<PortfolioRiskComposition>,
    /// Operator permission for net short exposure; `None` refuses every short,
    /// which is the behavior every configuration had before this field existed.
    pub short_exposure: Option<ShortExposurePolicy>,
    /// Venue tick size per tradable instrument. A plain order for an
    /// instrument that is not listed is refused, and so is a limit price off
    /// its instrument's grid, before a broker can reject it.
    pub instrument_tick_sizes: BTreeMap<String, Decimal>,
    /// Venue lot size per tradable instrument: the exact quantity increment.
    /// A plain order or a combination leg whose quantity is not a whole
    /// number of lots is refused, and so is one on an unlisted instrument,
    /// before a broker can reject it.
    pub instrument_lot_sizes: BTreeMap<String, Decimal>,
}

/// Operator permission for net short exposure, absent by default.
///
/// The short-sell guard exists because an uncovered short has unbounded loss.
/// A defined-risk combination bounds that loss with its own long leg — but
/// `core/paper` sees canonical instrument identities only. It holds no option
/// reference data, so it cannot *prove* that a short leg is covered by the long
/// one, and this repository does not approve risk on an assumption it cannot
/// check. So it does not try to infer coverage. Shorting stays refused unless
/// an operator explicitly permits it and states an absolute bound, and that
/// bound is enforced per instrument exactly like the long-side limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortExposurePolicy {
    /// Maximum absolute net short quantity permitted per instrument.
    pub max_short_quantity: Decimal,
}

impl PaperRiskPolicy {
    /// The tick-grid rejection for one plain order, if any.
    pub(crate) fn tick_rejection(
        &self,
        instrument_id: &str,
        limit_price: Option<Decimal>,
    ) -> Option<&'static str> {
        match self.instrument_tick_sizes.get(instrument_id) {
            None => Some("INSTRUMENT_TICK_SIZE_UNCONFIGURED"),
            Some(tick) => limit_price
                .is_some_and(|price| price.scaled() % tick.scaled() != 0)
                .then_some("LIMIT_PRICE_OFF_TICK_GRID"),
        }
    }

    /// The lot-size rejection for one order quantity, if any (E3.6c). A plain
    /// order's quantity and each combination leg's contract quantity meet
    /// this same rule.
    pub(crate) fn lot_rejection(
        &self,
        instrument_id: &str,
        quantity: Decimal,
    ) -> Option<&'static str> {
        match self.instrument_lot_sizes.get(instrument_id) {
            None => Some("INSTRUMENT_LOT_SIZE_UNCONFIGURED"),
            Some(lot) => {
                (quantity.scaled() % lot.scaled() != 0).then_some("ORDER_QUANTITY_OFF_LOT_SIZE")
            }
        }
    }

    /// Tick-grid rejections for a combination (E3.6b). Each leg meets exactly
    /// the rule a plain order on its instrument meets: it must be listed and
    /// its protected limit price must sit on its grid. The net price limit
    /// must then sit on the finest grid among the legs. That errs toward
    /// refusal; a venue's own complex-order increment is not modelled.
    pub(crate) fn combo_tick_rejections(&self, intent: &ComboIntent) -> Vec<&'static str> {
        let mut reasons: Vec<&'static str> = intent
            .legs
            .iter()
            .filter_map(|leg| self.tick_rejection(&leg.instrument_id, Some(leg.limit_price)))
            .collect();
        let finest = intent
            .legs
            .iter()
            .filter_map(|leg| self.instrument_tick_sizes.get(&leg.instrument_id))
            .min();
        if finest.is_some_and(|tick| intent.price_limit.amount().scaled() % tick.scaled() != 0) {
            reasons.push("COMBO_NET_PRICE_OFF_TICK_GRID");
        }
        reasons
    }

    /// Each leg's configured tick, for the decision evidence.
    pub(crate) fn combo_tick_evidence(&self, intent: &ComboIntent) -> String {
        render_leg_entries(intent, &self.instrument_tick_sizes)
    }

    /// Each leg's configured lot size, for the decision evidence.
    pub(crate) fn combo_lot_evidence(&self, intent: &ComboIntent) -> String {
        render_leg_entries(intent, &self.instrument_lot_sizes)
    }

    /// Whether a projected per-instrument position breaches this policy.
    ///
    /// Shared by the single-order and combination gates so a combination leg is
    /// judged by exactly the rule a plain order on the same instrument would
    /// meet — neither stricter nor looser.
    pub(crate) fn breaches_position_limit(&self, projected: Decimal) -> Result<bool, PaperError> {
        if projected > self.max_position_quantity {
            return Ok(true);
        }
        if projected < Decimal::ZERO {
            return Ok(match self.short_exposure.as_ref() {
                None => true,
                Some(permission) => {
                    Decimal::ZERO.checked_sub(projected)? > permission.max_short_quantity
                }
            });
        }
        Ok(false)
    }
}

impl PaperRiskPolicy {
    /// Validates immutable paper-risk limits.
    pub fn validate(&self) -> Result<(), PaperError> {
        let ten_thousand = Decimal::from_integer(10_000)?;
        if self.version.is_empty()
            || validate_canonical_id("paper trading_calendar_id", &self.trading_calendar_id)
                .is_err()
            || self.max_order_quantity <= Decimal::ZERO
            || self.max_order_notional <= Decimal::ZERO
            || self.max_price_deviation_bps < Decimal::ZERO
            || self.max_price_deviation_bps >= ten_thousand
            || self.max_open_orders == 0
            || self.max_position_quantity <= Decimal::ZERO
            || self.max_realized_loss < Decimal::ZERO
            || self.max_market_data_age_seconds == 0
            || self.max_order_rate == 0
            || self.order_rate_window_seconds == 0
        {
            return Err(PaperError("invalid paper risk policy".to_owned()));
        }
        // An empty table would refuse every order; that is a configuration
        // mistake, not a stricter policy.
        if self.instrument_tick_sizes.is_empty()
            || self
                .instrument_tick_sizes
                .iter()
                .any(|(instrument_id, tick)| {
                    validate_canonical_id("tick-size instrument_id", instrument_id).is_err()
                        || *tick <= Decimal::ZERO
                })
        {
            return Err(PaperError(
                "paper risk policy needs a positive tick size per listed instrument".to_owned(),
            ));
        }
        // The same holds for the lot table (E3.6c).
        if self.instrument_lot_sizes.is_empty()
            || self
                .instrument_lot_sizes
                .iter()
                .any(|(instrument_id, lot)| {
                    validate_canonical_id("lot-size instrument_id", instrument_id).is_err()
                        || *lot <= Decimal::ZERO
                })
        {
            return Err(PaperError(
                "paper risk policy needs a positive lot size per listed instrument".to_owned(),
            ));
        }
        // Each listed instrument needs both increments (E3.6f). One listed in
        // a single table would pass startup, then be refused at order time
        // with the other table's `..._UNCONFIGURED` code.
        if let Some(instrument_id) =
            unpaired_instrument(&self.instrument_tick_sizes, &self.instrument_lot_sizes)
        {
            return Err(PaperError(format!(
                "paper risk policy lists {instrument_id} in only one of its tick and lot tables"
            )));
        }
        if self
            .short_exposure
            .as_ref()
            .is_some_and(|permission| permission.max_short_quantity <= Decimal::ZERO)
        {
            // A permission that permits nothing is a configuration mistake, not
            // a stricter policy: `None` already expresses "no shorting", so a
            // zero bound can only mean the operator meant to set a real one.
            return Err(PaperError(
                "paper short-exposure permission must state a positive bound".to_owned(),
            ));
        }
        if let Some(composition) = &self.portfolio_risk {
            composition.policy.validate()?;
            for bucket in composition.instrument_buckets.values() {
                if validate_canonical_id("instrument bucket asset_class", &bucket.asset_class)
                    .is_err()
                    || validate_canonical_id("instrument bucket sector", &bucket.sector).is_err()
                    || bucket.currency.len() != 3
                    || !bucket
                        .currency
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase())
                {
                    return Err(PaperError(
                        "invalid instrument bucket reference data".to_owned(),
                    ));
                }
            }
            if let Some(rates) = &composition.margin_rates {
                if rates.is_empty() {
                    return Err(PaperError(
                        "configured margin_rates must not be empty".to_owned(),
                    ));
                }
                for (asset_class, rate) in rates {
                    if validate_canonical_id("margin rate asset_class", asset_class).is_err()
                        || rate.initial_bps > 10_000
                        || rate.maintenance_bps > rate.initial_bps
                        || rate.maintenance_bps == 0
                    {
                        return Err(PaperError("invalid margin rate".to_owned()));
                    }
                }
            }
        }
        Ok(())
    }
}
