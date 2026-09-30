//! Portfolio-wide risk composition for single orders and combinations.

use follon_accounting::{Currency, FxBook, MarginPolicy, MarginPosition};
use follon_domain::{ComboIntent, Decimal, OrderIntent, Side};
use follon_risk::{CandidateOrder, PortfolioRiskSnapshot, RestingOrder, RiskPosition};
use std::collections::BTreeMap;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    /// Real point-in-time equity: cash plus every non-zero position marked at
    /// its cached observed mark, falling back to average cost when this
    /// instrument has never been independently quoted. Shared by peak-equity
    /// tracking (always) and the Slice-1/2 aggregate-risk snapshot (when
    /// composed).
    pub(super) fn current_equity(&self) -> Result<Decimal, PaperError> {
        let mut equity = self.cash;
        for portfolio in self.portfolios.values() {
            let snapshot = portfolio.position_snapshot();
            if snapshot.quantity == Decimal::ZERO {
                continue;
            }
            let mark = self
                .marks
                .get(&snapshot.instrument_id)
                .copied()
                .unwrap_or(snapshot.average_cost);
            equity = equity.checked_add(snapshot.quantity.checked_mul(mark)?)?;
        }
        Ok(equity)
    }

    /// Builds the aggregate-risk snapshot/candidate from real service state
    /// and calls the composed `core/risk` kernel. Returns `Ok(None)` when
    /// computed equity is not positive: the kernel's ratios (leverage,
    /// drawdown, concentration) mean nothing against zero or negative equity.
    /// That is not permission to skip the limits. The caller refuses every
    /// order that does not reduce a position (`PORTFOLIO_EQUITY_NOT_POSITIVE`,
    /// delivery state E7.4b), and the per-order checks apply as always. On `Some`, the
    /// second tuple element is the real margin requirement computed for the
    /// decision (`Decimal::ZERO` when `margin_rates` is not configured),
    /// returned alongside the decision because `core/risk::AggregateRiskMetrics`
    /// only ever reports the *ratio* (`margin_utilization_bps`), not the raw
    /// currency amount that produced it.
    pub(super) fn portfolio_risk_decision(
        &self,
        composition: &PortfolioRiskComposition,
        intent: &OrderIntent,
        market: &PaperMarketData,
        decided_at: &str,
    ) -> Result<Option<(follon_risk::PortfolioRiskDecision, Decimal)>, PaperError> {
        let Some((snapshot, margin_used)) = self.portfolio_risk_state(composition, decided_at)?
        else {
            return Ok(None);
        };
        let candidate = self.risk_candidate(
            composition,
            &intent.intent_id,
            &intent.account_id,
            &intent.strategy_id,
            &intent.instrument_id,
            intent.side,
            intent.quantity,
            market.mark_price,
        )?;
        let decision = follon_risk::evaluate_portfolio_risk_with_candidates(
            &composition.policy,
            &snapshot,
            std::slice::from_ref(&candidate),
        )?;
        Ok(Some((decision, margin_used)))
    }

    /// Builds the real aggregate-risk snapshot from service state, without any
    /// candidate.
    ///
    /// Extracted so the single-order and combination paths observe exactly the
    /// same portfolio, equity, peak, daily baseline and margin figures; the
    /// only difference between them is which candidates are then added.
    fn portfolio_risk_state(
        &self,
        composition: &PortfolioRiskComposition,
        decided_at: &str,
    ) -> Result<Option<(PortfolioRiskSnapshot, Decimal)>, PaperError> {
        let equity = self.current_equity()?;
        if equity <= Decimal::ZERO {
            return Ok(None);
        }
        let mut positions = Vec::new();
        for portfolio in self.portfolios.values() {
            let snapshot = portfolio.position_snapshot();
            if snapshot.quantity == Decimal::ZERO {
                continue;
            }
            let mark = self
                .marks
                .get(&snapshot.instrument_id)
                .copied()
                .unwrap_or(snapshot.average_cost);
            let bucket = composition.instrument_buckets.get(&snapshot.instrument_id);
            let asset_class = bucket
                .map(|bucket| bucket.asset_class.clone())
                .unwrap_or_else(|| "unclassified".to_owned());
            let sector = bucket
                .map(|bucket| bucket.sector.clone())
                .unwrap_or_else(|| "unclassified".to_owned());
            let currency = bucket
                .map(|bucket| bucket.currency.clone())
                .unwrap_or_else(|| self.account.currency.clone());
            // Slice 2d: split the aggregate position into one `RiskPosition`
            // row per strategy that has ever traded this instrument, plus
            // one "unattributed" remainder row for whatever the tracked
            // strategies do not account for (a legacy journal, or a fill
            // predating this ledger). Every row's quantity always
            // reconciles exactly against `snapshot.quantity` (see
            // `apply_strategy_attribution_fill`), so gross/net exposure can
            // never be mis-stated by this split, only how it is attributed.
            let mut attributed_total = Decimal::ZERO;
            if let Some(strategies) = self.strategy_attribution.get(&snapshot.instrument_id) {
                for (strategy_id, quantity) in strategies {
                    if *quantity == Decimal::ZERO {
                        continue;
                    }
                    attributed_total = attributed_total.checked_add(*quantity)?;
                    positions.push(RiskPosition {
                        account_id: snapshot.account_id.clone(),
                        strategy_id: strategy_id.clone(),
                        instrument_id: snapshot.instrument_id.clone(),
                        asset_class: asset_class.clone(),
                        sector: sector.clone(),
                        currency: currency.clone(),
                        quantity: *quantity,
                        mark_price: mark,
                        multiplier: Decimal::from_integer(1)?,
                        delta: Decimal::ZERO,
                        gamma: Decimal::ZERO,
                    });
                }
            }
            let remainder = snapshot.quantity.checked_sub(attributed_total)?;
            if remainder != Decimal::ZERO {
                positions.push(RiskPosition {
                    account_id: snapshot.account_id,
                    strategy_id: "unattributed".to_owned(),
                    instrument_id: snapshot.instrument_id,
                    asset_class,
                    sector,
                    currency,
                    quantity: remainder,
                    mark_price: mark,
                    multiplier: Decimal::from_integer(1)?,
                    delta: Decimal::ZERO,
                    gamma: Decimal::ZERO,
                });
            }
        }
        let mut working_positions = Vec::new();
        // A working order has not changed cash or the filled position yet, but its
        // unfilled quantity can become exposure. Add it at the current mark before
        // the new candidate, so a sequence of individually legal orders cannot
        // pass a portfolio limit in aggregate (E7.14).
        for order in self.orders.values().filter(|order| order.working()) {
            let intent = &order.oms.intent;
            let remaining = intent.quantity.checked_sub(order.filled_quantity)?;
            if remaining > Decimal::ZERO {
                let mark = self
                    .marks
                    .get(&intent.instrument_id)
                    .copied()
                    .unwrap_or(order.market.mark_price);
                working_positions.push(self.working_risk_position(
                    composition,
                    &intent.account_id,
                    &intent.strategy_id,
                    &intent.instrument_id,
                    intent.side,
                    remaining,
                    mark,
                )?);
            }
        }
        for order in self.combo_orders.values().filter(|order| order.working()) {
            let intent = &order.oms.intent;
            let unfilled = intent.combo_quantity.checked_sub(order.filled_quantity)?;
            if unfilled == Decimal::ZERO {
                continue;
            }
            for leg in &intent.legs {
                let mark = self
                    .marks
                    .get(&leg.instrument_id)
                    .copied()
                    .or_else(|| {
                        order
                            .market
                            .mark_for(&leg.instrument_id)
                            .map(|seen| seen.mark_price)
                    })
                    .ok_or_else(|| PaperError("working combo leg has no market mark".to_owned()))?;
                let quantity =
                    unfilled.checked_mul(Decimal::from_integer(i64::from(leg.ratio))?)?;
                working_positions.push(self.working_risk_position(
                    composition,
                    &intent.account_id,
                    &intent.strategy_id,
                    &leg.instrument_id,
                    leg.side,
                    quantity,
                    mark,
                )?);
            }
        }
        let mut resting_orders = self
            .orders
            .values()
            .filter(|order| order.working())
            .map(|order| RestingOrder {
                order_id: order.oms.order_id.clone(),
                account_id: order.oms.intent.account_id.clone(),
                instrument_id: order.oms.intent.instrument_id.clone(),
                side: order.oms.intent.side,
            })
            .collect::<Vec<_>>();
        // A working combination contributes one resting row *per leg*, because
        // the aggregate kernel's self-trade check is per instrument and a
        // working short leg is a real resting sell on that instrument however
        // the group is labelled. Every one of those rows carries the same
        // `order_id`, and the kernel counts *distinct* order identities against
        // `max_open_orders`, so a four-leg combination stays one open order.
        for order in self.combo_orders.values().filter(|order| order.working()) {
            for leg in &order.oms.intent.legs {
                resting_orders.push(RestingOrder {
                    order_id: order.oms.order_id.clone(),
                    account_id: order.oms.intent.account_id.clone(),
                    instrument_id: leg.instrument_id.clone(),
                    side: leg.side,
                });
            }
        }
        // Real, computed only when `margin_rates` is configured (Slice 2c):
        // the *currently held* margin requirement, not a projection that
        // includes the candidate order -- the same "pre-trade observed, not
        // post-trade projected" convention `equity`/`peak_equity`/
        // `daily_baseline_equity` already use above. Every asset class among
        // currently held positions must have a configured rate or this fails
        // closed with a technical error rather than silently under-counting
        // margin -- an intentional operator-configuration requirement, not a
        // soft risk rejection.
        let margin_used = if let Some(rates) = composition.margin_rates.as_ref() {
            let mut margin_positions = Vec::new();
            for portfolio in self.portfolios.values() {
                let snapshot = portfolio.position_snapshot();
                if snapshot.quantity == Decimal::ZERO {
                    continue;
                }
                let mark = self
                    .marks
                    .get(&snapshot.instrument_id)
                    .copied()
                    .unwrap_or(snapshot.average_cost);
                let bucket = composition.instrument_buckets.get(&snapshot.instrument_id);
                margin_positions.push(MarginPosition {
                    instrument_id: snapshot.instrument_id,
                    asset_class: bucket
                        .map(|bucket| bucket.asset_class.clone())
                        .unwrap_or_else(|| "unclassified".to_owned()),
                    currency: Currency::new(
                        bucket
                            .map(|bucket| bucket.currency.clone())
                            .unwrap_or_else(|| self.account.currency.clone()),
                    )?,
                    quantity: snapshot.quantity,
                    mark_price: mark,
                    multiplier: Decimal::from_integer(1)?,
                });
            }
            let account_currency = Currency::new(self.account.currency.clone())?;
            let mut cash_by_currency = BTreeMap::new();
            cash_by_currency.insert(account_currency.clone(), self.cash);
            let margin_policy = MarginPolicy {
                base_currency: account_currency,
                // Never actually consulted: every position and cash balance
                // here is denominated in the account's own currency, so
                // `FxBook::convert` always takes its same-currency fast path
                // and never reaches a freshness check.
                maximum_fx_age_seconds: i64::MAX,
                rates: rates.clone(),
            };
            let as_of_epoch_seconds = OffsetDateTime::parse(decided_at, &Rfc3339)
                .map_err(|error| PaperError(error.to_string()))?
                .unix_timestamp();
            follon_accounting::value_margin_account(
                &cash_by_currency,
                &margin_positions,
                &FxBook::default(),
                &margin_policy,
                as_of_epoch_seconds,
            )?
            .initial_margin
        } else {
            Decimal::ZERO
        };
        let snapshot = PortfolioRiskSnapshot {
            equity,
            // Real, durable running high-water-mark (see `peak_equity` on
            // `PaperTradingService`) -- never below `equity` itself, since
            // `evaluate_risk` updates it from the same observation before
            // this function ever runs.
            peak_equity: self.peak_equity.max(equity),
            // Real, durable session-start baseline (see `daily_baseline_equity`
            // on `PaperTradingService`) -- `evaluate_risk` updates it from the
            // same observation before this function ever runs.
            daily_pnl: equity.checked_sub(self.daily_baseline_equity)?,
            margin_used,
            positions,
            working_positions,
            resting_orders,
            recent_order_count: 0,
        };
        Ok(Some((snapshot, margin_used)))
    }

    #[allow(clippy::too_many_arguments)]
    fn working_risk_position(
        &self,
        composition: &PortfolioRiskComposition,
        account_id: &str,
        strategy_id: &str,
        instrument_id: &str,
        side: Side,
        quantity: Decimal,
        mark_price: Decimal,
    ) -> Result<RiskPosition, PaperError> {
        let candidate = self.risk_candidate(
            composition,
            "working.exposure",
            account_id,
            strategy_id,
            instrument_id,
            side,
            quantity,
            mark_price,
        )?;
        Ok(RiskPosition {
            account_id: candidate.account_id,
            strategy_id: candidate.strategy_id,
            instrument_id: candidate.instrument_id,
            asset_class: candidate.asset_class,
            sector: candidate.sector,
            currency: candidate.currency,
            quantity: match side {
                Side::Buy => quantity,
                Side::Sell => Decimal::ZERO.checked_sub(quantity)?,
            },
            mark_price: candidate.mark_price,
            multiplier: candidate.multiplier,
            delta: candidate.delta,
            gamma: candidate.gamma,
        })
    }

    /// Builds one aggregate-risk candidate row, classified by the operator's
    /// attested bucket table. Shared by the single-order and combination paths
    /// so a combination leg is classified exactly as the same instrument would
    /// be on its own.
    #[allow(clippy::too_many_arguments)]
    fn risk_candidate(
        &self,
        composition: &PortfolioRiskComposition,
        intent_id: &str,
        account_id: &str,
        strategy_id: &str,
        instrument_id: &str,
        side: Side,
        quantity: Decimal,
        mark_price: Decimal,
    ) -> Result<CandidateOrder, PaperError> {
        let bucket = composition.instrument_buckets.get(instrument_id);
        Ok(CandidateOrder {
            intent_id: intent_id.to_owned(),
            account_id: account_id.to_owned(),
            strategy_id: strategy_id.to_owned(),
            instrument_id: instrument_id.to_owned(),
            asset_class: bucket
                .map(|bucket| bucket.asset_class.clone())
                .unwrap_or_else(|| "unclassified".to_owned()),
            sector: bucket
                .map(|bucket| bucket.sector.clone())
                .unwrap_or_else(|| "unclassified".to_owned()),
            currency: bucket
                .map(|bucket| bucket.currency.clone())
                .unwrap_or_else(|| self.account.currency.clone()),
            side,
            quantity,
            mark_price,
            multiplier: Decimal::from_integer(1)?,
            delta: Decimal::ZERO,
            gamma: Decimal::ZERO,
        })
    }

    /// The composed aggregate-risk decision for a whole combination.
    ///
    /// Every leg is submitted to the kernel as a simultaneous candidate, so
    /// bucket, concentration and exposure limits see the structure as it will
    /// actually exist after an atomic fill. Assessing legs one at a time would
    /// approve a group that breaches a limit only jointly — see
    /// `follon_risk::evaluate_portfolio_risk_with_candidates`.
    pub(super) fn combo_portfolio_risk_decision(
        &self,
        composition: &PortfolioRiskComposition,
        intent: &ComboIntent,
        market: &PaperComboMarketData,
        decided_at: &str,
    ) -> Result<Option<(follon_risk::PortfolioRiskDecision, Decimal)>, PaperError> {
        let Some((snapshot, margin_used)) = self.portfolio_risk_state(composition, decided_at)?
        else {
            return Ok(None);
        };
        let mut candidates = Vec::with_capacity(intent.legs.len());
        for leg in &intent.legs {
            let mark = market.mark_for(&leg.instrument_id).ok_or_else(|| {
                PaperError(format!(
                    "paper combo observation is missing a mark for {}",
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
