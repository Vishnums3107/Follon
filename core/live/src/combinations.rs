//! Controlled-LIVE multi-leg combination contract and risk gate.
//!
//! This mirrors `core/paper`'s combination gate rule for rule, and then adds
//! what controlled-LIVE requires on top: the canary notional and order-count
//! ceilings, the deployed-capital ceiling, and the unresolved-incident block.
//! It is a separate implementation rather than shared code because the two
//! environments carry separate policies, separate approvals and separate
//! review, and a change that loosened PAPER must never be able to loosen LIVE
//! as a side effect.
use super::*;

/// One mark per leg of a combination, evaluated as a single observation.
///
/// Kept as a collection of [`LiveMarketData`] rather than a new shape so a
/// durable record can reuse the existing per-mark serialisation without a
/// second format to migrate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveComboMarketData {
    /// Exactly one validated mark per combination leg.
    pub marks: Vec<LiveMarketData>,
}

impl LiveComboMarketData {
    /// Validates the observation set against the combination it prices.
    ///
    /// Every leg must be quoted. A combination priced from a partial set would
    /// have to guess at least one leg's notional, and guessing is what a
    /// pre-trade gate exists to prevent -- with real capital behind it.
    pub fn validate_for(&self, intent: &ComboIntent) -> Result<(), LiveError> {
        if self.marks.len() != intent.legs.len() {
            return Err(LiveError(
                "live combo observation must carry exactly one mark per leg".to_owned(),
            ));
        }
        for mark in &self.marks {
            mark.validate()?;
        }
        for leg in &intent.legs {
            if self.mark_for(&leg.instrument_id).is_none() {
                return Err(LiveError(format!(
                    "live combo observation is missing a mark for {}",
                    leg.instrument_id
                )));
            }
        }
        Ok(())
    }

    /// The validated mark for one leg instrument, if present.
    pub fn mark_for(&self, instrument_id: &str) -> Option<&LiveMarketData> {
        self.marks
            .iter()
            .find(|mark| mark.instrument_id == instrument_id)
    }

    /// The oldest observation time across every leg.
    ///
    /// Freshness for a combination is the freshness of its *stalest* leg: the
    /// group executes atomically, so one stale leg makes the whole priced
    /// structure stale. Taking the newest would let a single fresh quote
    /// launder an arbitrarily old one beside it.
    pub fn oldest_observed_at(&self) -> Result<&str, LiveError> {
        self.marks
            .iter()
            .map(|mark| mark.observed_at.as_str())
            // Canonical second-precision UTC sorts lexicographically in exactly
            // timestamp order, so no parsing is needed to find the oldest.
            .min()
            .ok_or_else(|| LiveError("live combo observation is empty".to_owned()))
    }

    /// Deterministic fingerprint of the whole observation set.
    fn fingerprint(&self) -> String {
        let mut parts = vec!["live-combo-market-v1".to_owned()];
        // Sorted, so an approval cannot be defeated by reordering the marks.
        let mut marks = self.marks.clone();
        marks.sort_by(|a, b| a.instrument_id.cmp(&b.instrument_id));
        for mark in &marks {
            parts.push(mark.instrument_id.clone());
            parts.push(mark.mark_price.to_string());
            parts.push(mark.observed_at.clone());
        }
        hash_fingerprint_parts(&parts.iter().map(String::as_str).collect::<Vec<_>>())
    }
}

/// Binds an operator approval to one exact combination.
///
/// Every leg's instrument, side, ratio and protected price is covered, along
/// with the unit count and the price-limit kind and amount. An approval that
/// bound less than this would let an operator who approved a two-leg debit
/// spread have a different structure submitted under the same approval.
pub fn combo_intent_fingerprint(intent: &ComboIntent) -> Result<String, LiveError> {
    intent.validate()?;
    let combo_quantity = intent.combo_quantity.to_string();
    let price_limit_amount = intent.price_limit.amount().to_string();
    let leg_count = intent.legs.len().to_string();
    let mut parts = vec![
        "live-combo-intent-v1",
        &intent.intent_id,
        &intent.account_id,
        &intent.strategy_id,
        &intent.correlation_id,
        &combo_quantity,
        intent.price_limit.kind(),
        &price_limit_amount,
        intent.time_in_force.as_str(),
        &intent.rationale,
        &intent.created_at,
        &intent.strategy_version,
        &intent.configuration_version,
        &intent.environment,
        // Included explicitly so a fingerprint cannot be matched by a
        // combination with a different number of legs.
        &leg_count,
    ];
    // Legs are hashed in their declared order, not sorted: leg order is part of
    // what the operator approved, and `ComboIntent::validate` already refuses
    // duplicate instruments, so a reordering is a different document.
    let leg_parts: Vec<String> = intent
        .legs
        .iter()
        .flat_map(|leg| {
            [
                leg.instrument_id.clone(),
                leg.side.as_str().to_owned(),
                leg.ratio.to_string(),
                leg.limit_price.to_string(),
            ]
        })
        .collect();
    parts.extend(leg_parts.iter().map(String::as_str));
    Ok(hash_fingerprint_parts(&parts))
}

impl<B: LiveBrokerAdapter> LiveTradingService<B> {
    /// Assesses a multi-leg combination against the full controlled-live risk
    /// policy.
    ///
    /// Assessment only: it creates no order, consumes no approval, contacts no
    /// broker and touches no OMS state. `shadow` relaxes exactly the checks that
    /// are meaningless without a real submission -- the canary ceilings, the
    /// `UNKNOWN`-order block and the unresolved-incident block -- matching
    /// `evaluate_risk`'s own convention.
    ///
    /// Two things are deliberately *not* restated per leg, because a
    /// combination is one order: it counts once against the open-order limit
    /// and once against the order-rate limit.
    pub fn evaluate_combo_risk(
        &mut self,
        intent: &ComboIntent,
        market: &LiveComboMarketData,
        decided_at: &str,
        shadow: bool,
    ) -> Result<LiveRiskDecision, LiveError> {
        intent.validate()?;
        validate_utc_timestamp("live combo risk decided_at", decided_at)?;
        market.validate_for(intent)?;
        if intent.environment != "LIVE" || intent.account_id != self.account.account_id {
            return Err(LiveError(
                "live combination gate accepts only matching LIVE account intents".to_owned(),
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

        // Staleness is a hard error rather than a rejection reason, matching
        // `evaluate_risk`: a decision made against an observation the policy
        // considers unusable is not a "no", it is not a decision at all, and
        // must not be recorded as evidence of one.
        let observed_at = OffsetDateTime::parse(market.oldest_observed_at()?, &Rfc3339)
            .map_err(|error| LiveError(error.to_string()))?;
        let decision_at = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| LiveError(error.to_string()))?;
        let age = (decision_at - observed_at).whole_seconds();
        if age < 0
            || u64::try_from(age).unwrap_or(u64::MAX) > self.policy.max_market_data_age_seconds
        {
            return Err(LiveError(
                "live combo observation is stale or later than decision".to_owned(),
            ));
        }

        let realized_pnl = self
            .portfolios
            .values()
            .try_fold(Decimal::ZERO, |total, portfolio| {
                total.checked_add(portfolio.position_snapshot().realized_pnl)
            })?;
        let reserved_cash = self.total_reserved_cash()?;
        let available_cash = self.cash.checked_sub(reserved_cash)?;
        let rate_window_start =
            decision_at - time::Duration::seconds(self.policy.order_rate_window_seconds as i64);
        let recent_order_count = self.recent_order_count(rate_window_start, decision_at)?;

        let gross_notional = intent.gross_notional()?;
        let net_price = intent.protected_net_price()?;
        // A debit is cash out the door now; a credit is not cash in that may be
        // spent, so only a debit is charged against available cash and against
        // the deployed-capital ceiling. The short leg's obligation is covered by
        // the gross-notional and aggregate limits, not by the cash check.
        let net_debit = if net_price > Decimal::ZERO {
            net_price.checked_mul(intent.combo_quantity)?
        } else {
            Decimal::ZERO
        };

        let mut reasons = self.kill_switches.combo_rejection_reasons(intent);

        // Each leg's own contract quantity is what the broker sees, so the
        // per-order quantity limit binds the largest leg rather than the
        // combination unit count. A ten-lot butterfly is not a ten-lot order.
        let mut largest_leg_quantity = Decimal::ZERO;
        let mut widest_leg_deviation_bps = Decimal::ZERO;
        let mut leg_evidence = Vec::with_capacity(intent.legs.len());
        for leg in &intent.legs {
            let mark = market.mark_for(&leg.instrument_id).ok_or_else(|| {
                LiveError(format!(
                    "live combo observation is missing a mark for {}",
                    leg.instrument_id
                ))
            })?;
            let leg_quantity = intent.leg_quantity(leg)?;
            largest_leg_quantity = largest_leg_quantity.max(leg_quantity);
            let deviation_bps = price_deviation_bps(mark.mark_price, leg.limit_price)?;
            widest_leg_deviation_bps = widest_leg_deviation_bps.max(deviation_bps);

            let held = self
                .portfolios
                .get(&leg.instrument_id)
                .map(|portfolio| portfolio.position_snapshot().quantity)
                .unwrap_or(Decimal::ZERO);
            let projected = held.checked_add(intent.projected_leg_delta(leg)?)?;
            if self.policy.breaches_position_limit(projected)? {
                reasons.push("POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned());
            }
            // Self-trade is assessed per leg against every working order, and a
            // breach on any one leg rejects the whole structure: the group is
            // atomic, so there is no version of it that omits the offending leg.
            if self.conflicts_with_working_order(&leg.instrument_id, leg.side) {
                reasons.push("SELF_TRADE_RISK".to_owned());
            }
            leg_evidence.push(format!(
                "{}:{}:{}:{}:{}:{}",
                leg.instrument_id,
                leg.side.as_str(),
                leg_quantity,
                leg.limit_price,
                mark.mark_price,
                deviation_bps
            ));
        }

        if largest_leg_quantity > self.policy.max_order_quantity {
            reasons.push("MAX_ORDER_QUANTITY_EXCEEDED".to_owned());
        }
        if gross_notional > self.policy.max_order_notional {
            reasons.push("MAX_ORDER_NOTIONAL_EXCEEDED".to_owned());
        }
        if widest_leg_deviation_bps > self.policy.max_price_deviation_bps {
            reasons.push("PRICE_COLLAR_EXCEEDED".to_owned());
        }
        if recent_order_count >= self.policy.max_order_rate {
            reasons.push("MAX_ORDER_RATE_EXCEEDED".to_owned());
        }
        // The canary notional ceiling is charged the **gross**, not the net, for
        // the same reason the order-notional ceiling is: the canary exists to
        // bound how much capital a controlled-LIVE order can put at risk, and a
        // two-sided structure puts both legs at risk until it is closed. A net
        // view would let an arbitrarily large spread through a small ceiling.
        if !shadow && gross_notional > self.policy.canary_max_order_notional {
            reasons.push("CANARY_NOTIONAL_EXCEEDED".to_owned());
        }
        if !shadow && self.canary_submissions >= self.policy.canary_max_orders {
            reasons.push("CANARY_ORDER_COUNT_EXCEEDED".to_owned());
        }
        if !shadow && self.has_unknown_order() {
            reasons.push("UNKNOWN_ORDER_REQUIRES_RECONCILIATION".to_owned());
        }
        if !shadow && self.unresolved_incident_count() > 0 {
            reasons.push("UNRESOLVED_INCIDENTS_REQUIRE_REVIEW".to_owned());
        }
        if self.working_order_count() >= self.policy.max_open_orders {
            reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
        }
        if net_debit > available_cash {
            reasons.push("INSUFFICIENT_INTERNAL_CASH".to_owned());
        }
        if net_debit > Decimal::ZERO {
            let deployed_capital = if self.cash < self.account.initial_cash {
                self.account.initial_cash.checked_sub(self.cash)?
            } else {
                Decimal::ZERO
            };
            let projected_deployed_capital = deployed_capital
                .checked_add(reserved_cash)?
                .checked_add(net_debit)?;
            if projected_deployed_capital > self.account.max_deployed_capital {
                reasons.push("DEPLOYED_CAPITAL_CEILING_EXCEEDED".to_owned());
            }
        }
        let realized_loss = if realized_pnl < Decimal::ZERO {
            Decimal::ZERO.checked_sub(realized_pnl)?
        } else {
            Decimal::ZERO
        };
        if realized_loss > self.policy.max_realized_loss {
            reasons.push("MAX_REALIZED_LOSS_EXCEEDED".to_owned());
        }

        let mut portfolio_risk_limits = String::new();
        if let Some(composition) = self.policy.portfolio_risk.as_ref() {
            if let Some((decision, margin_used)) =
                self.combo_portfolio_risk_decision(composition, intent, market, decided_at)?
            {
                reasons.extend(
                    decision
                        .reason_codes
                        .into_iter()
                        // `SELF_TRADE_RISK` is already detected per leg above
                        // from the same working-order state.
                        .filter(|reason| reason != "APPROVED" && reason != "SELF_TRADE_RISK"),
                );
                portfolio_risk_limits = format!(
                    ",portfolio_risk_policy_version={},portfolio_gross_exposure={},portfolio_net_exposure={},portfolio_leverage_bps={},portfolio_concentration_bps={},portfolio_peak_equity={},portfolio_drawdown_bps={},portfolio_daily_baseline_equity={},portfolio_daily_pnl={},portfolio_margin_used={},portfolio_margin_utilization_bps={}",
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
            }
        }

        reasons.sort();
        reasons.dedup();
        let approved = reasons.is_empty();
        if approved {
            reasons.push("APPROVED".to_owned());
        }
        let evaluated_limits = format!(
            "combo_legs={},combo_quantity={},combo_price_limit_kind={},combo_price_limit_amount={},combo_protected_net_price={},combo_net_debit={},combo_gross_notional={},largest_leg_quantity={},widest_leg_deviation_bps={},max_order_quantity={},max_order_notional={},max_price_deviation_bps={},canary_max_order_notional={},canary_max_orders={},canary_submissions={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},max_order_rate={},order_rate_window_seconds={},recent_order_count={},available_cash={},oldest_observed_at={},legs=[{}]{}",
            intent.legs.len(),
            intent.combo_quantity,
            intent.price_limit.kind(),
            intent.price_limit.amount(),
            net_price,
            net_debit,
            gross_notional,
            largest_leg_quantity,
            widest_leg_deviation_bps,
            self.policy.max_order_quantity,
            self.policy.max_order_notional,
            self.policy.max_price_deviation_bps,
            self.policy.canary_max_order_notional,
            self.policy.canary_max_orders,
            self.canary_submissions,
            self.policy.max_open_orders,
            self.policy.max_position_quantity,
            self.policy.max_realized_loss,
            self.policy.max_market_data_age_seconds,
            self.policy.max_order_rate,
            self.policy.order_rate_window_seconds,
            recent_order_count,
            available_cash,
            market.oldest_observed_at()?,
            leg_evidence.join("|"),
            portfolio_risk_limits,
        );
        Ok(LiveRiskDecision {
            // A distinct prefix from the single-order gate's `live-risk-`, so a
            // combination decision can never be mistaken for, or collide with,
            // a plain order's decision under the same intent identity.
            decision_id: format!("live-combo-risk-{}", intent.intent_id),
            approved,
            reason_codes: reasons,
            policy_version: self.policy.version.clone(),
            decided_at: decided_at.to_owned(),
            market_fingerprint: market.fingerprint(),
            evaluated_limits,
        })
    }

    /// The composed aggregate-risk decision for a whole combination.
    ///
    /// Every leg is submitted to the kernel as a simultaneous candidate, so
    /// bucket, concentration and exposure limits see the structure as it will
    /// actually exist after an atomic fill. Assessing legs one at a time would
    /// approve a group that breaches a limit only jointly.
    fn combo_portfolio_risk_decision(
        &self,
        composition: &PortfolioRiskComposition,
        intent: &ComboIntent,
        market: &LiveComboMarketData,
        decided_at: &str,
    ) -> Result<Option<(follon_risk::PortfolioRiskDecision, Decimal)>, LiveError> {
        let Some((snapshot, margin_used)) = self.portfolio_risk_state(composition, decided_at)?
        else {
            return Ok(None);
        };
        let mut candidates = Vec::with_capacity(intent.legs.len());
        for leg in &intent.legs {
            let mark = market.mark_for(&leg.instrument_id).ok_or_else(|| {
                LiveError(format!(
                    "live combo observation is missing a mark for {}",
                    leg.instrument_id
                ))
            })?;
            candidates.push(self.risk_candidate(
                composition,
                &intent.intent_id,
                &intent.account_id,
                &intent.strategy_id,
                &leg.instrument_id,
                leg.side,
                intent.leg_quantity(leg)?,
                mark.mark_price,
            )?);
        }
        let decision = follon_risk::evaluate_portfolio_risk_with_candidates(
            &composition.policy,
            &snapshot,
            &candidates,
        )?;
        Ok(Some((decision, margin_used)))
    }
}

/// Normalized LIVE combination submission request, created only by the
/// controlled OMS after a risk approval and a consumed operator approval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerComboRequest {
    /// OMS client idempotency key for the whole group.
    pub client_order_id: String,
    /// Configured live account identity.
    pub account_id: String,
    /// Exact combination legs, executed as one atomic group.
    pub legs: Vec<LiveBrokerComboLeg>,
    /// Protection amount the operator approved, whose direction is fixed by
    /// the combination's own price-limit kind.
    pub limit_price: Decimal,
    /// `MAXIMUM_DEBIT` or `MINIMUM_CREDIT`, carried explicitly so an adapter
    /// never has to infer the direction of the protection from its sign.
    pub price_limit_kind: String,
}

/// One leg of a LIVE combination request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerComboLeg {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Approved side.
    pub side: Side,
    /// Approved contracts per combination unit.
    pub ratio: u32,
    /// Exact contract quantity, ratio already applied, so no adapter has to
    /// re-derive it from a unit count.
    pub quantity: Decimal,
    /// Approved protected price for this leg.
    pub limit_price: Decimal,
}

impl LiveBrokerComboRequest {
    fn from_combo_order(order: &OmsComboOrder) -> Result<Self, LiveError> {
        let mut legs = Vec::with_capacity(order.intent.legs.len());
        for leg in &order.intent.legs {
            legs.push(LiveBrokerComboLeg {
                instrument_id: leg.instrument_id.clone(),
                side: leg.side,
                ratio: leg.ratio,
                quantity: order.intent.leg_quantity(leg)?,
                limit_price: leg.limit_price,
            });
        }
        Ok(Self {
            client_order_id: order.order_id.clone(),
            account_id: order.intent.account_id.clone(),
            legs,
            limit_price: order.intent.price_limit.amount(),
            price_limit_kind: order.intent.price_limit.kind().to_owned(),
        })
    }

    fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("live combo client_order_id", &self.client_order_id)?;
        validate_canonical_id("live combo account_id", &self.account_id)?;
        if !follon_domain::COMBO_LEG_BOUNDS.contains(&self.legs.len())
            || self.limit_price <= Decimal::ZERO
        {
            return Err(LiveError(
                "live combination request must carry a bounded leg set and positive protection"
                    .to_owned(),
            ));
        }
        for leg in &self.legs {
            validate_canonical_id("live combo leg instrument_id", &leg.instrument_id)?;
            if !follon_domain::COMBO_LEG_RATIO_BOUNDS.contains(&leg.ratio)
                || leg.quantity <= Decimal::ZERO
                || leg.limit_price <= Decimal::ZERO
            {
                return Err(LiveError("live combination leg is invalid".to_owned()));
            }
        }
        Ok(())
    }
}

/// Durable internal OMS record for one canary combination order.
#[derive(Clone, Debug)]
pub struct LiveComboOrder {
    /// Legal OMS lifecycle state for the whole group.
    pub oms: OmsComboOrder,
    /// Single-use approval consumed by this combination.
    pub approval_id: String,
    /// Exact per-leg observation used for risk and cash reservation.
    pub market: LiveComboMarketData,
    /// Immutable risk outcome that authorized this exact combination.
    pub decision: LiveRiskDecision,
    /// Broker identity after it becomes known. One identity for the whole
    /// group: an atomic combination is a single broker order, not one per leg.
    pub broker_order_id: Option<String>,
    /// Every broker-native order identity issued for this immutable OMS order.
    pub broker_order_versions: Vec<String>,
    /// Exact filled combination units, in whole units.
    pub filled_quantity: Decimal,
}

impl LiveComboOrder {
    fn working(&self) -> bool {
        !matches!(
            self.oms.state,
            OrderState::RiskRejected
                | OrderState::Filled
                | OrderState::Cancelled
                | OrderState::Rejected
                | OrderState::Expired
        )
    }

    /// Cash a working combination still has committed.
    ///
    /// Only a net debit reserves cash, matching the gate that approved it, and
    /// it shrinks with filled units so a partially filled combination stops
    /// over-reserving against every later decision.
    fn reserved_cash(&self) -> Result<Decimal, LiveError> {
        if !self.working() {
            return Ok(Decimal::ZERO);
        }
        let net_price = self.oms.intent.protected_net_price()?;
        if net_price <= Decimal::ZERO {
            return Ok(Decimal::ZERO);
        }
        net_price
            .checked_mul(
                self.oms
                    .intent
                    .combo_quantity
                    .checked_sub(self.filled_quantity)?,
            )
            .map_err(Into::into)
    }
}

impl<B: LiveBrokerAdapter> LiveTradingService<B> {
    /// Submits one atomic multi-leg combination under one consumed approval.
    ///
    /// The combination analogue of [`Self::submit_canary_intent`], and subject
    /// to every control that path is: the canary activation window, a connected
    /// broker session, a registered single-use approval that binds this exact
    /// structure, the full combination risk gate, and a durable audit record
    /// written *before* the irreversible broker call.
    ///
    /// The approval must carry `combo_intent_fingerprint(&intent)`. That
    /// function is domain-separated from `intent_fingerprint`, so an approval
    /// issued for a plain order can never authorize a combination, or the
    /// reverse.
    pub fn submit_canary_combo_intent(
        &mut self,
        intent: ComboIntent,
        market: LiveComboMarketData,
        approval_id: &str,
        decided_at: &str,
        actor: &str,
    ) -> Result<LiveSubmitOutcome, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(decided_at)?;
        validate_canonical_id("live combo submit actor", actor)?;
        validate_canonical_id("live combo approval_id", approval_id)?;
        if !self.broker_connected {
            return Err(LiveError(
                "live canary broker session is not connected".to_owned(),
            ));
        }
        if intent.environment != "LIVE" || intent.account_id != self.account.account_id {
            return Err(LiveError(
                "canary accepts only matching LIVE account combinations".to_owned(),
            ));
        }
        intent.validate()?;
        market.validate_for(&intent)?;

        let expected_fingerprint = combo_intent_fingerprint(&intent)?;
        let order_id = OmsComboOrder::order_id_for(&intent.intent_id);
        if let Some(existing) = self.combo_orders.get(&order_id) {
            if existing.oms.intent != intent
                || existing.approval_id != approval_id
                || existing.market != market
                || existing.decision.decided_at != decided_at
            {
                return Err(LiveError(
                    "live combination idempotency key was reused with different data".to_owned(),
                ));
            }
            return Ok(LiveSubmitOutcome::CanaryOrder {
                decision: existing.decision.clone(),
                order_id,
                state: existing.oms.state,
            });
        }

        let registered = self.approvals.get(approval_id).ok_or_else(|| {
            LiveError("live canary combination lacks a registered approval".to_owned())
        })?;
        if registered.consumed
            || registered.approval.intent_id != intent.intent_id
            || registered.approval.intent_fingerprint != expected_fingerprint
            || registered.approval.configuration_fingerprint != self.configuration_fingerprint()
            || registered.approval.approved_at.as_str() > decided_at
            || registered.approval.expires_at.as_str() <= decided_at
        {
            return Err(LiveError(
                "live approval is expired, consumed, or does not bind this exact combination"
                    .to_owned(),
            ));
        }

        let decision = self.evaluate_combo_risk(&intent, &market, decided_at, false)?;
        if !decision.approved {
            self.persist(
                "live.combo.risk.rejected.v1",
                actor,
                decided_at,
                &intent.correlation_id,
            )?;
            return Ok(LiveSubmitOutcome::RiskRejected { decision });
        }

        let core_decision = RiskDecision {
            decision_id: decision.decision_id.clone(),
            intent_id: intent.intent_id.clone(),
            approved: true,
            reason_codes: decision.reason_codes.clone(),
            policy_version: self.policy.version.clone(),
            decided_at: decided_at.to_owned(),
            correlation_id: intent.correlation_id.clone(),
            actor: "live_risk_engine".to_owned(),
            evaluated_limits: decision.evaluated_limits.clone(),
        };
        let mut oms = OmsComboOrder::from_approved_intent(intent, &core_decision)?;
        oms.transition(OrderState::Approved, "LIVE_COMBO_RISK_APPROVED")?;
        oms.transition(
            OrderState::PendingSubmit,
            "LIVE_CANARY_COMBO_SUBMISSION_REQUESTED",
        )?;
        let request = LiveBrokerComboRequest::from_combo_order(&oms)?;
        request.validate()?;
        let correlation_id = oms.intent.correlation_id.clone();
        self.combo_orders.insert(
            oms.order_id.clone(),
            LiveComboOrder {
                oms,
                approval_id: approval_id.to_owned(),
                market,
                decision: decision.clone(),
                broker_order_id: None,
                broker_order_versions: Vec::new(),
                filled_quantity: Decimal::ZERO,
            },
        );
        self.approvals
            .get_mut(approval_id)
            .ok_or_else(|| {
                LiveError("registered live approval disappeared before consumption".to_owned())
            })?
            .consumed = true;
        // One combination is one canary submission, not one per leg. Counting
        // legs would exhaust an operator's canary budget on a single ordinary
        // four-leg structure.
        self.canary_submissions = self
            .canary_submissions
            .checked_add(1)
            .ok_or_else(|| LiveError("live canary submission counter overflowed".to_owned()))?;
        // The durable audit record precedes the irreversible external call.
        self.persist(
            "live.combo.pending_submission.v1",
            actor,
            decided_at,
            &correlation_id,
        )?;

        match self.broker.submit_combo(&request) {
            Ok(LiveBrokerSubmitResult::Acknowledged { broker_order_id }) => {
                validate_canonical_id("live combo broker_order_id", &broker_order_id)?;
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "LIVE_CANARY_COMBO_SUBMISSION_SENT")?;
                order
                    .oms
                    .transition(OrderState::Acknowledged, "LIVE_BROKER_COMBO_ACKNOWLEDGED")?;
                order.broker_order_id = Some(broker_order_id.clone());
                order.broker_order_versions.push(broker_order_id);
                let state = order.oms.state;
                self.persist(
                    "live.combo.acknowledged.v1",
                    actor,
                    decided_at,
                    &correlation_id,
                )?;
                Ok(LiveSubmitOutcome::CanaryOrder {
                    decision,
                    order_id: request.client_order_id,
                    state,
                })
            }
            Ok(LiveBrokerSubmitResult::Rejected { reason }) => {
                validate_reason("live combo rejection reason", &reason)?;
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "LIVE_CANARY_COMBO_SUBMISSION_SENT")?;
                order.oms.transition(OrderState::Rejected, reason)?;
                let state = order.oms.state;
                self.persist("live.combo.rejected.v1", actor, decided_at, &correlation_id)?;
                Ok(LiveSubmitOutcome::CanaryOrder {
                    decision,
                    order_id: request.client_order_id,
                    state,
                })
            }
            Ok(LiveBrokerSubmitResult::Unknown { reason }) => {
                validate_reason("live combo unknown-outcome reason", &reason)?;
                let order = self.combo_order_mut(&request.client_order_id)?;
                order.oms.transition(OrderState::Unknown, reason)?;
                self.persist("live.combo.unknown.v1", actor, decided_at, &correlation_id)?;
                Ok(LiveSubmitOutcome::CanaryOrder {
                    decision,
                    order_id: request.client_order_id,
                    state: OrderState::Unknown,
                })
            }
            Err(error) => {
                // An adapter that cannot execute an atomic combination returns
                // an error here, and that is a *transport* outcome like any
                // other: the request may or may not have reached the venue, so
                // the combination goes to UNKNOWN and the session is marked
                // disconnected rather than assumed untouched. The approval
                // stays consumed and the canary counter stays incremented --
                // an attempt was made, and pretending otherwise would let one
                // approval authorize a second attempt.
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Unknown, "LIVE_COMBO_TRANSPORT_OUTCOME_UNKNOWN")?;
                self.broker_connected = false;
                self.persist("live.combo.unknown.v1", actor, decided_at, &correlation_id)?;
                Err(error)
            }
        }
    }

    /// The durable combination order for one client identity, if known.
    pub fn combo_order(&self, order_id: &str) -> Option<&LiveComboOrder> {
        self.combo_orders.get(order_id)
    }

    pub(super) fn combo_order_mut(
        &mut self,
        order_id: &str,
    ) -> Result<&mut LiveComboOrder, LiveError> {
        self.combo_orders
            .get_mut(order_id)
            .ok_or_else(|| LiveError("unknown live OMS combination order".to_owned()))
    }

    /// Non-terminal orders of both kinds. A combination counts once.
    pub(super) fn working_order_count(&self) -> usize {
        self.orders.values().filter(|order| order.working()).count()
            + self
                .combo_orders
                .values()
                .filter(|order| order.working())
                .count()
    }

    /// Cash committed by every working order of either kind.
    pub(super) fn total_reserved_cash(&self) -> Result<Decimal, LiveError> {
        let mut reserved = Decimal::ZERO;
        for order in self.orders.values() {
            reserved = reserved.checked_add(order.reserved_cash()?)?;
        }
        for order in self.combo_orders.values() {
            reserved = reserved.checked_add(order.reserved_cash()?)?;
        }
        Ok(reserved)
    }

    /// Whether any order of either kind is in the `UNKNOWN` safety state.
    pub(super) fn has_unknown_order(&self) -> bool {
        self.orders
            .values()
            .any(|order| order.oms.state == OrderState::Unknown)
            || self
                .combo_orders
                .values()
                .any(|order| order.oms.state == OrderState::Unknown)
    }

    /// Whether a working order of either kind would trade against `side` on
    /// `instrument_id`.
    ///
    /// A combination's legs count individually: a working short leg is a real
    /// resting sell on that instrument however the group is labelled.
    pub(super) fn conflicts_with_working_order(&self, instrument_id: &str, side: Side) -> bool {
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

    /// Counts orders of both kinds whose own risk decision falls inside the
    /// rate window. A combination counts once.
    pub(super) fn recent_order_count(
        &self,
        rate_window_start: OffsetDateTime,
        decision_at: OffsetDateTime,
    ) -> Result<u32, LiveError> {
        let within = |decided_at: &str| -> Result<bool, LiveError> {
            let parsed = OffsetDateTime::parse(decided_at, &Rfc3339)
                .map_err(|error| LiveError(error.to_string()))?;
            Ok(parsed > rate_window_start && parsed <= decision_at)
        };
        let mut count = 0u32;
        for order in self.orders.values() {
            if within(&order.decision.decided_at)? {
                count += 1;
            }
        }
        for order in self.combo_orders.values() {
            if within(&order.decision.decided_at)? {
                count += 1;
            }
        }
        Ok(count)
    }
}
