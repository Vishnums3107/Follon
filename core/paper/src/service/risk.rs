//! Single-order and combination pre-trade risk evaluation.

use follon_domain::{
    price_deviation_bps, reduces_position_with_working, validate_utc_timestamp, ComboIntent,
    Decimal, OrderIntent, OrderState, RiskDecision, Side,
};
use std::collections::BTreeMap;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    pub(super) fn evaluate_risk(
        &mut self,
        intent: &OrderIntent,
        market: &PaperMarketData,
        decided_at: &str,
    ) -> Result<RiskDecision, PaperError> {
        self.marks
            .insert(intent.instrument_id.clone(), market.mark_price);
        // Peak-equity tracking runs unconditionally, independent of whether
        // Slice-2 composition is even configured, so enabling it later does
        // not start drawdown tracking from a fresh, artificially favorable
        // baseline -- the same reasoning as the unconditional `marks` update
        // above.
        let observed_equity = self.current_equity()?;
        if observed_equity > self.peak_equity {
            self.peak_equity = observed_equity;
        }
        // Session-start daily-loss baseline: reset (not maxed) whenever the
        // UTC calendar date of this decision differs from the stored
        // baseline date -- including the very first evaluation ever
        // (`daily_baseline_date` starts `None`). `decided_at` is already
        // `validate_utc_timestamp`-checked by every caller (canonical
        // second-precision UTC, `YYYY-MM-DDTHH:MM:SSZ`), so its first 10
        // bytes are exactly its UTC calendar date.
        let decision_date = &decided_at[..10];
        if self.daily_baseline_date.as_deref() != Some(decision_date) {
            self.daily_baseline_date = Some(decision_date.to_owned());
            self.daily_baseline_equity = observed_equity;
        }
        let current_position = self
            .portfolios
            .get(&intent.instrument_id)
            .map(|portfolio| portfolio.position_snapshot().quantity)
            .unwrap_or(Decimal::ZERO);
        let realized_pnl = self
            .portfolios
            .values()
            .try_fold(Decimal::ZERO, |total, portfolio| {
                total.checked_add(portfolio.position_snapshot().realized_pnl)
            })?;
        let reserved_cash = self.total_reserved_cash()?;
        let context = PaperRiskContext {
            open_orders: self.working_order_count(),
            position_quantity: current_position,
            available_cash: self.cash.checked_sub(reserved_cash)?,
            realized_pnl,
        };
        let estimated_notional = intent.quantity.checked_mul(market.mark_price)?;
        let requested_price_deviation_bps = intent
            .limit_price
            .map(|price| price_deviation_bps(market.mark_price, price))
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        let projected_position = match intent.side {
            Side::Buy => context.position_quantity.checked_add(intent.quantity)?,
            Side::Sell => context.position_quantity.checked_sub(intent.quantity)?,
        };
        let working_position_delta = self.working_position_delta(&intent.instrument_id)?;
        let committed_position = projected_position.checked_add(working_position_delta)?;
        let recent_order_count = self.recent_order_count(decided_at)?;
        let mut reasons = self.kill_switches.rejection_reasons(intent);
        if intent.quantity > self.risk_policy.max_order_quantity {
            reasons.push("MAX_ORDER_QUANTITY_EXCEEDED".to_owned());
        }
        if estimated_notional > self.risk_policy.max_order_notional {
            reasons.push("MAX_ORDER_NOTIONAL_EXCEEDED".to_owned());
        }
        if requested_price_deviation_bps > self.risk_policy.max_price_deviation_bps {
            reasons.push("PRICE_COLLAR_EXCEEDED".to_owned());
        }
        if let Some(reason) = self
            .risk_policy
            .tick_rejection(&intent.instrument_id, intent.limit_price)
        {
            reasons.push(reason.to_owned());
        }
        if let Some(reason) = self
            .risk_policy
            .lot_rejection(&intent.instrument_id, intent.quantity)
        {
            reasons.push(reason.to_owned());
        }
        if self.conflicts_with_working_order(&intent.instrument_id, intent.side) {
            reasons.push("SELF_TRADE_RISK".to_owned());
        }
        if recent_order_count >= self.risk_policy.max_order_rate {
            reasons.push("MAX_ORDER_RATE_EXCEEDED".to_owned());
        }
        if self.has_unknown_order() {
            reasons.push("UNKNOWN_ORDER_REQUIRES_RECONCILIATION".to_owned());
        }
        if self.unexplained_incident_count() > 0 {
            reasons.push("UNEXPLAINED_INCIDENTS_REQUIRE_REVIEW".to_owned());
        }
        if context.open_orders >= self.risk_policy.max_open_orders {
            reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
        }
        if self
            .risk_policy
            .breaches_position_limit(committed_position)?
        {
            reasons.push("POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned());
        }
        if intent.side == Side::Buy && estimated_notional > context.available_cash {
            reasons.push("INSUFFICIENT_INTERNAL_CASH".to_owned());
        }
        let realized_loss = if context.realized_pnl < Decimal::ZERO {
            Decimal::ZERO.checked_sub(context.realized_pnl)?
        } else {
            Decimal::ZERO
        };
        if realized_loss > self.risk_policy.max_realized_loss {
            reasons.push("MAX_REALIZED_LOSS_EXCEEDED".to_owned());
        }
        let mut portfolio_risk_limits = String::new();
        if let Some(composition) = self.risk_policy.portfolio_risk.as_ref() {
            match self.portfolio_risk_decision(composition, intent, market, decided_at)? {
                Some((decision, margin_used)) => {
                    reasons.extend(
                        decision
                            .reason_codes
                            .into_iter()
                            // `SELF_TRADE_RISK` is already independently detected above from
                            // the same working-order state; every other reason this composed
                            // kernel can produce is new coverage (see `PortfolioRiskComposition`).
                            .filter(|reason| reason != "APPROVED" && reason != "SELF_TRADE_RISK"),
                    );
                    portfolio_risk_limits = format!(
                    ",portfolio_exposure_basis=filled_working_candidate_v2,portfolio_risk_policy_version={},portfolio_gross_exposure={},portfolio_net_exposure={},portfolio_leverage_bps={},portfolio_concentration_bps={},portfolio_peak_equity={},portfolio_drawdown_bps={},portfolio_daily_baseline_equity={},portfolio_daily_pnl={},portfolio_margin_used={},portfolio_margin_utilization_bps={},portfolio_sector_gross={},portfolio_asset_class_gross={},portfolio_currency_gross={},portfolio_strategy_gross={}",
                    decision.policy_version,
                    decision.metrics.gross_exposure,
                    decision.metrics.net_exposure,
                    decision.metrics.leverage_bps,
                    decision.metrics.concentration_bps,
                    self.peak_equity,
                    decision.metrics.drawdown_bps,
                    self.daily_baseline_equity,
                    observed_equity.checked_sub(self.daily_baseline_equity)?,
                    margin_used,
                    decision.metrics.margin_utilization_bps,
                    render_bucket_map(&decision.metrics.sector_gross),
                    render_bucket_map(&decision.metrics.asset_class_gross),
                    render_bucket_map(&decision.metrics.currency_gross),
                    render_bucket_map(&decision.metrics.strategy_gross),
                );
                    portfolio_risk_limits.push_str(&format!(
                        ",portfolio_possible_abs_net_exposure={},portfolio_possible_concentration_bps={},portfolio_possible_abs_delta={},portfolio_possible_abs_gamma={}",
                        decision.metrics.possible_abs_net_exposure,
                        decision.metrics.possible_concentration_bps,
                        decision.metrics.possible_abs_delta,
                        decision.metrics.possible_abs_gamma,
                    ));
                }
                // Equity is not positive, so no aggregate ratio exists to check.
                // Skipping the check outright would let an underwater account
                // open more exposure past every aggregate limit exactly when
                // they matter, so only a trade that moves this position toward
                // flat may pass, counting what working orders already claim of
                // it. The rest is refused (delivery state E7.4b).
                None => {
                    if !self.reduces_open_position(
                        &intent.instrument_id,
                        current_position,
                        projected_position,
                    )? {
                        reasons.push("PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned());
                    }
                }
            }
        }
        let approved = reasons.is_empty();
        if approved {
            reasons.push("APPROVED".to_owned());
        }
        Ok(RiskDecision {
            decision_id: format!("paper-risk-{}", intent.intent_id),
            intent_id: intent.intent_id.clone(),
            approved,
            reason_codes: reasons,
            policy_version: self.risk_policy.version.clone(),
            decided_at: decided_at.to_owned(),
            correlation_id: intent.correlation_id.clone(),
            actor: "paper_risk_engine".to_owned(),
            evaluated_limits: format!(
                "max_order_quantity={},max_order_notional={},max_price_deviation_bps={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},max_order_rate={},order_rate_window_seconds={},recent_order_count={},market_instrument_id={},market_mark_price={},market_observed_at={},requested_price={},requested_price_deviation_bps={},estimated_notional={},projected_position={},working_position_delta={},committed_position={},available_cash={},instrument_tick_size={},instrument_lot_size={}{}",
                self.risk_policy.max_order_quantity,
                self.risk_policy.max_order_notional,
                self.risk_policy.max_price_deviation_bps,
                self.risk_policy.max_open_orders,
                self.risk_policy.max_position_quantity,
                self.risk_policy.max_realized_loss,
                self.risk_policy.max_market_data_age_seconds,
                self.risk_policy.max_order_rate,
                self.risk_policy.order_rate_window_seconds,
                recent_order_count,
                market.instrument_id,
                market.mark_price,
                market.observed_at,
                intent.limit_price.map_or_else(|| "MARKET".to_owned(), |price| price.to_string()),
                requested_price_deviation_bps,
                estimated_notional,
                projected_position,
                working_position_delta,
                committed_position,
                context.available_cash,
                self.risk_policy
                    .instrument_tick_sizes
                    .get(&intent.instrument_id)
                    .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string),
                self.risk_policy
                    .instrument_lot_sizes
                    .get(&intent.instrument_id)
                    .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string),
                portfolio_risk_limits,
            ),
        })
    }

    /// Assesses a multi-leg combination against the full paper risk policy.
    ///
    /// This is assessment only: it creates no order, contacts no broker, and
    /// leaves the OMS untouched. It exists as a separate entry point rather
    /// than a variant of the single-order gate because a combination is one
    /// economic unit with several instruments, and almost every rule has to be
    /// restated in those terms — the notional is the *gross* of all legs, the
    /// price collar is per leg against that leg's own mark, and the position
    /// projection is per instrument.
    ///
    /// Two things are deliberately *not* restated, because a combination is one
    /// order: it counts once against the open-order limit and once against the
    /// order-rate limit.
    pub fn evaluate_combo_risk(
        &mut self,
        intent: &ComboIntent,
        market: &PaperComboMarketData,
        decided_at: &str,
    ) -> Result<RiskDecision, PaperError> {
        intent.validate()?;
        validate_utc_timestamp("paper combo risk decision time", decided_at)?;
        if intent.combo_quantity.scaled() % follon_domain::DECIMAL_SCALE != 0 {
            return Err(PaperError(
                "PAPER combinations require whole combination units".to_owned(),
            ));
        }
        market.validate_for(intent)?;
        if intent.account_id != self.account.account_id {
            return Err(PaperError(
                "paper combo intent account does not match service".to_owned(),
            ));
        }
        // Staleness is a hard error rather than a rejection reason, matching
        // `submit_intent`: a decision made against an observation the policy
        // considers unusable is not a "no", it is not a decision at all, and
        // must not be recorded as evidence of one.
        let observed_at = OffsetDateTime::parse(market.oldest_observed_at()?, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let decision_time = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let age = (decision_time - observed_at).whole_seconds();
        if age < 0
            || u64::try_from(age).unwrap_or(u64::MAX) > self.risk_policy.max_market_data_age_seconds
        {
            return Err(PaperError(
                "paper combo observation is stale or later than the risk decision".to_owned(),
            ));
        }

        // Unconditional, exactly as the single-order gate does it: the mark
        // cache, the peak-equity high-water-mark and the daily-loss baseline
        // must not depend on whether aggregate composition happens to be
        // configured, or enabling it later would start from an artificially
        // favourable baseline.
        for mark in &market.marks {
            self.marks
                .insert(mark.instrument_id.clone(), mark.mark_price);
        }
        let observed_equity = self.current_equity()?;
        if observed_equity > self.peak_equity {
            self.peak_equity = observed_equity;
        }
        let decision_date = &decided_at[..10];
        if self.daily_baseline_date.as_deref() != Some(decision_date) {
            self.daily_baseline_date = Some(decision_date.to_owned());
            self.daily_baseline_equity = observed_equity;
        }

        let reserved_cash = self.total_reserved_cash()?;
        let available_cash = self.cash.checked_sub(reserved_cash)?;
        let realized_pnl = self
            .portfolios
            .values()
            .try_fold(Decimal::ZERO, |total, portfolio| {
                total.checked_add(portfolio.position_snapshot().realized_pnl)
            })?;
        let open_orders = self.working_order_count();
        let recent_order_count = self.recent_order_count(decided_at)?;

        let gross_notional = intent.gross_notional()?;
        let net_price = intent.protected_net_price()?;
        // A debit is cash out the door now; a credit is not cash in that may be
        // spent, so only a debit is charged against available cash. The short
        // leg's obligation is covered by the gross-notional and aggregate
        // limits, not by this check.
        let net_debit = if net_price > Decimal::ZERO {
            net_price.checked_mul(intent.combo_quantity)?
        } else {
            Decimal::ZERO
        };

        let mut reasons = self.kill_switches.combo_rejection_reasons(intent);
        reasons.extend(
            self.risk_policy
                .combo_tick_rejections(intent)
                .into_iter()
                .map(str::to_owned),
        );

        // Each leg's own contract quantity is what the broker sees, so the
        // per-order quantity limit binds the largest leg rather than the
        // combination unit count. A ten-lot butterfly is not a ten-lot order.
        let mut largest_leg_quantity = Decimal::ZERO;
        let mut widest_leg_deviation_bps = Decimal::ZERO;
        let mut every_leg_reduces = true;
        let mut leg_evidence = Vec::with_capacity(intent.legs.len());
        for leg in &intent.legs {
            let mark = market.mark_for(&leg.instrument_id).ok_or_else(|| {
                PaperError(format!(
                    "paper combo observation is missing a mark for {}",
                    leg.instrument_id
                ))
            })?;
            let leg_quantity = intent.leg_quantity(leg)?;
            // The lot rule binds what the broker sees, each leg's own contract
            // quantity, exactly as it binds a plain order's (E3.6c).
            if let Some(reason) = self
                .risk_policy
                .lot_rejection(&leg.instrument_id, leg_quantity)
            {
                reasons.push(reason.to_owned());
            }
            largest_leg_quantity = largest_leg_quantity.max(leg_quantity);
            let deviation_bps = price_deviation_bps(mark.mark_price, leg.limit_price)?;
            widest_leg_deviation_bps = widest_leg_deviation_bps.max(deviation_bps);

            let held = self
                .portfolios
                .get(&leg.instrument_id)
                .map(|portfolio| portfolio.position_snapshot().quantity)
                .unwrap_or(Decimal::ZERO);
            let projected = held.checked_add(intent.projected_leg_delta(leg)?)?;
            let working_delta = self.working_position_delta(&leg.instrument_id)?;
            let committed = projected.checked_add(working_delta)?;
            every_leg_reduces &= self.reduces_open_position(&leg.instrument_id, held, projected)?;
            if self.risk_policy.breaches_position_limit(committed)? {
                reasons.push("POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned());
            }
            // Self-trade is assessed per leg against every working order, and a
            // breach on any one leg rejects the whole structure: the group is
            // atomic, so there is no version of it that omits the offending leg.
            if self.conflicts_with_working_order(&leg.instrument_id, leg.side) {
                reasons.push("SELF_TRADE_RISK".to_owned());
            }
            leg_evidence.push(format!(
                "{}:{}:{}:{}:{}:{}:{}:{}",
                leg.instrument_id,
                leg.side.as_str(),
                leg_quantity,
                leg.limit_price,
                mark.mark_price,
                deviation_bps,
                working_delta,
                committed
            ));
        }

        if largest_leg_quantity > self.risk_policy.max_order_quantity {
            reasons.push("MAX_ORDER_QUANTITY_EXCEEDED".to_owned());
        }
        if gross_notional > self.risk_policy.max_order_notional {
            reasons.push("MAX_ORDER_NOTIONAL_EXCEEDED".to_owned());
        }
        if widest_leg_deviation_bps > self.risk_policy.max_price_deviation_bps {
            reasons.push("PRICE_COLLAR_EXCEEDED".to_owned());
        }
        if recent_order_count >= self.risk_policy.max_order_rate {
            reasons.push("MAX_ORDER_RATE_EXCEEDED".to_owned());
        }
        if self.has_unknown_order() {
            reasons.push("UNKNOWN_ORDER_REQUIRES_RECONCILIATION".to_owned());
        }
        if self.unexplained_incident_count() > 0 {
            reasons.push("UNEXPLAINED_INCIDENTS_REQUIRE_REVIEW".to_owned());
        }
        if open_orders >= self.risk_policy.max_open_orders {
            reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
        }
        if net_debit > available_cash {
            reasons.push("INSUFFICIENT_INTERNAL_CASH".to_owned());
        }
        let realized_loss = if realized_pnl < Decimal::ZERO {
            Decimal::ZERO.checked_sub(realized_pnl)?
        } else {
            Decimal::ZERO
        };
        if realized_loss > self.risk_policy.max_realized_loss {
            reasons.push("MAX_REALIZED_LOSS_EXCEEDED".to_owned());
        }

        let mut portfolio_risk_limits = String::new();
        if let Some(composition) = self.risk_policy.portfolio_risk.as_ref() {
            match self.combo_portfolio_risk_decision(composition, intent, market, decided_at)? {
                Some((decision, margin_used)) => {
                    reasons.extend(
                        decision
                            .reason_codes
                            .into_iter()
                            // `SELF_TRADE_RISK` is already detected per leg above
                            // from the same working-order state.
                            .filter(|reason| reason != "APPROVED" && reason != "SELF_TRADE_RISK"),
                    );
                    portfolio_risk_limits = format!(
                    ",portfolio_exposure_basis=filled_working_candidate_v2,portfolio_risk_policy_version={},portfolio_gross_exposure={},portfolio_net_exposure={},portfolio_leverage_bps={},portfolio_concentration_bps={},portfolio_peak_equity={},portfolio_drawdown_bps={},portfolio_daily_baseline_equity={},portfolio_daily_pnl={},portfolio_margin_used={},portfolio_margin_utilization_bps={}",
                    decision.policy_version,
                    decision.metrics.gross_exposure,
                    decision.metrics.net_exposure,
                    decision.metrics.leverage_bps,
                    decision.metrics.concentration_bps,
                    self.peak_equity,
                    decision.metrics.drawdown_bps,
                    self.daily_baseline_equity,
                    observed_equity.checked_sub(self.daily_baseline_equity)?,
                    margin_used,
                    decision.metrics.margin_utilization_bps,
                );
                    portfolio_risk_limits.push_str(&format!(
                        ",portfolio_possible_abs_net_exposure={},portfolio_possible_concentration_bps={},portfolio_possible_abs_delta={},portfolio_possible_abs_gamma={}",
                        decision.metrics.possible_abs_net_exposure,
                        decision.metrics.possible_concentration_bps,
                        decision.metrics.possible_abs_delta,
                        decision.metrics.possible_abs_gamma,
                    ));
                }
                // The group is atomic, so every leg must move its own position
                // toward flat for the structure to reduce risk (E7.4b).
                None if !every_leg_reduces => {
                    reasons.push("PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned());
                }
                None => {}
            }
        }

        reasons.sort();
        reasons.dedup();
        let approved = reasons.is_empty();
        if approved {
            reasons.push("APPROVED".to_owned());
        }
        Ok(RiskDecision {
            // A distinct prefix from the single-order gate's `paper-risk-`, so
            // a combination decision can never be mistaken for, or collide
            // with, a plain order's decision under the same intent identity.
            decision_id: format!("paper-combo-risk-{}", intent.intent_id),
            intent_id: intent.intent_id.clone(),
            approved,
            reason_codes: reasons,
            policy_version: self.risk_policy.version.clone(),
            decided_at: decided_at.to_owned(),
            correlation_id: intent.correlation_id.clone(),
            actor: "paper_risk_engine".to_owned(),
            evaluated_limits: format!(
                "combo_legs={},combo_quantity={},combo_price_limit_kind={},combo_price_limit_amount={},combo_protected_net_price={},combo_net_debit={},combo_gross_notional={},largest_leg_quantity={},widest_leg_deviation_bps={},max_order_quantity={},max_order_notional={},max_price_deviation_bps={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},max_order_rate={},order_rate_window_seconds={},recent_order_count={},available_cash={},oldest_observed_at={},legs=[{}],combo_tick_sizes=[{}],combo_lot_sizes=[{}]{}",
                intent.legs.len(),
                intent.combo_quantity,
                intent.price_limit.kind(),
                intent.price_limit.amount(),
                net_price,
                net_debit,
                gross_notional,
                largest_leg_quantity,
                widest_leg_deviation_bps,
                self.risk_policy.max_order_quantity,
                self.risk_policy.max_order_notional,
                self.risk_policy.max_price_deviation_bps,
                self.risk_policy.max_open_orders,
                self.risk_policy.max_position_quantity,
                self.risk_policy.max_realized_loss,
                self.risk_policy.max_market_data_age_seconds,
                self.risk_policy.max_order_rate,
                self.risk_policy.order_rate_window_seconds,
                recent_order_count,
                available_cash,
                market.oldest_observed_at()?,
                leg_evidence.join("|"),
                self.risk_policy.combo_tick_evidence(intent),
                self.risk_policy.combo_lot_evidence(intent),
                portfolio_risk_limits,
            ),
        })
    }

    /// Counts orders whose own risk decision falls inside the rate window.
    ///
    /// Rate-window membership is keyed off each order's own risk *decision*
    /// time (`RiskDecision::decided_at`, sourced from `risk_evidence`), not the
    /// caller-supplied `OrderIntent::created_at`. `created_at` is stamped by
    /// the strategy or operator that originated the intent and is not otherwise
    /// constrained to reflect wall-clock reality, so counting against it would
    /// let a caller understate its own submission rate and silently bypass
    /// `MAX_ORDER_RATE_EXCEEDED` by backdating `created_at` on new intents.
    pub(crate) fn recent_order_count(&self, decided_at: &str) -> Result<u32, PaperError> {
        let decision_time = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let rate_window_start = decision_time
            - time::Duration::seconds(self.risk_policy.order_rate_window_seconds as i64);
        let within_window = |order_decided_at: &str| -> Result<bool, PaperError> {
            let parsed = OffsetDateTime::parse(order_decided_at, &Rfc3339)
                .map_err(|error| PaperError(error.to_string()))?;
            Ok(parsed > rate_window_start && parsed <= decision_time)
        };
        let mut count = 0u32;
        for order in self.orders.values() {
            let decision_id = format!("paper-risk-{}", order.oms.intent.intent_id);
            let order_decided_at = self
                .risk_evidence
                .get(&decision_id)
                .map(|evidence| evidence.decision.decided_at.as_str())
                .ok_or_else(|| {
                    PaperError("paper order is missing its originating risk evidence".to_owned())
                })?;
            if within_window(order_decided_at)? {
                count += 1;
            }
        }
        // A combination counts once, not once per leg: it is one broker
        // submission. Leaving it out entirely would let an operator submit an
        // unlimited number of combinations inside a rate window that a plain
        // order would be refused in.
        for order in self.combo_orders.values() {
            let decision_id = format!("paper-combo-risk-{}", order.oms.intent.intent_id);
            let order_decided_at = self
                .combo_risk_evidence
                .get(&decision_id)
                .map(|evidence| evidence.decision.decided_at.as_str())
                .ok_or_else(|| {
                    PaperError(
                        "paper combination order is missing its originating risk evidence"
                            .to_owned(),
                    )
                })?;
            if within_window(order_decided_at)? {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Non-terminal orders of both kinds. A combination counts once.
    pub(crate) fn working_order_count(&self) -> usize {
        self.orders.values().filter(|order| order.working()).count()
            + self
                .combo_orders
                .values()
                .filter(|order| order.working())
                .count()
    }

    /// Whether any order of either kind is in the `UNKNOWN` safety state.
    pub(crate) fn has_unknown_order(&self) -> bool {
        self.orders
            .values()
            .any(|order| order.oms.state == OrderState::Unknown)
            || self.combo_orders.values().any(|order| {
                order.oms.state == OrderState::Unknown || order.evidence_error.is_some()
            })
    }

    /// Cash committed by every working order of either kind.
    pub(crate) fn total_reserved_cash(&self) -> Result<Decimal, PaperError> {
        let mut reserved = Decimal::ZERO;
        for order in self.orders.values() {
            reserved = reserved.checked_add(order.reserved_cash()?)?;
        }
        for order in self.combo_orders.values() {
            reserved = reserved.checked_add(order.reserved_cash()?)?;
        }
        Ok(reserved)
    }

    /// Whether trading `instrument_id` from `current` to `projected` moves the position
    /// toward flat, counting what working orders of either kind already claim of it.
    pub(crate) fn reduces_open_position(
        &self,
        instrument_id: &str,
        current: Decimal,
        projected: Decimal,
    ) -> Result<bool, PaperError> {
        let working = self.working_reduction(instrument_id, current)?;
        Ok(reduces_position_with_working(current, projected, working)?)
    }

    /// Signed unfilled quantity of every working plain order and combination leg.
    /// This is added to a new candidate before checking the position limit.
    fn working_position_delta(&self, instrument_id: &str) -> Result<Decimal, PaperError> {
        self.working_quantity(instrument_id, Side::Buy)?
            .checked_sub(self.working_quantity(instrument_id, Side::Sell)?)
            .map_err(Into::into)
    }

    /// The quantity working orders of either kind will still take off `position` in
    /// `instrument_id`: sells against a long, buys against a short. A combination's legs
    /// count individually, by the combination units still unfilled.
    fn working_reduction(
        &self,
        instrument_id: &str,
        position: Decimal,
    ) -> Result<Decimal, PaperError> {
        let reducing = if position > Decimal::ZERO {
            Side::Sell
        } else if position < Decimal::ZERO {
            Side::Buy
        } else {
            return Ok(Decimal::ZERO);
        };
        self.working_quantity(instrument_id, reducing)
    }

    /// Unfilled working quantity on one side of one instrument, across both order kinds.
    fn working_quantity(&self, instrument_id: &str, side: Side) -> Result<Decimal, PaperError> {
        let mut total = Decimal::ZERO;
        for order in self.orders.values() {
            let intent = &order.oms.intent;
            if order.working() && intent.instrument_id == instrument_id && intent.side == side {
                total = total.checked_add(intent.quantity.checked_sub(order.filled_quantity)?)?;
            }
        }
        for order in self.combo_orders.values() {
            if !order.working() {
                continue;
            }
            let intent = &order.oms.intent;
            let unfilled = intent.combo_quantity.checked_sub(order.filled_quantity)?;
            for leg in &intent.legs {
                if leg.instrument_id == instrument_id && leg.side == side {
                    let ratio = Decimal::from_integer(i64::from(leg.ratio))?;
                    total = total.checked_add(unfilled.checked_mul(ratio)?)?;
                }
            }
        }
        Ok(total)
    }

    /// Whether a working order of either kind would trade against `side` on
    /// `instrument_id`.
    ///
    /// A combination's legs count individually here: a working short leg is a
    /// real resting sell on that instrument however the group is labelled, and
    /// a plain buy submitted against it is the same self-trade it would be
    /// against a plain sell.
    pub(crate) fn conflicts_with_working_order(&self, instrument_id: &str, side: Side) -> bool {
        self.orders.values().any(|order| {
            order.working()
                && order.oms.intent.instrument_id == instrument_id
                && order.oms.intent.side != side
        }) || self.combo_orders.values().any(|order| {
            order.working()
                && order
                    .oms
                    .intent
                    .legs
                    .iter()
                    .any(|leg| leg.instrument_id == instrument_id && leg.side != side)
        })
    }
}

/// Renders a bucket-exposure map deterministically (`BTreeMap` iteration is
/// already sorted) for the `evaluated_limits` evidence string and the
/// configuration fingerprint.
fn render_bucket_map(buckets: &BTreeMap<String, Decimal>) -> String {
    buckets
        .iter()
        .map(|(bucket, amount)| format!("{bucket}:{amount}"))
        .collect::<Vec<_>>()
        .join("|")
}
