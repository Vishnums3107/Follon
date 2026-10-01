//! Operator-attested corporate actions (delivery state E8.4b).

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use follon_market_data::CorporateAction;

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    /// Applies a split or a cash dividend to the OMS's own books, attributed to an
    /// operator and journaled before it returns.
    ///
    /// The broker applies the action on its books; this keeps the OMS's position,
    /// average cost, FIFO lots, strategy attribution, cached mark and cash in step,
    /// so a split does not leave the OMS selling a quantity the account no longer
    /// holds, and reconciliation compares like with like. A split scales them by
    /// the arithmetic the backtest ledger and the replay engine use.
    ///
    /// Nothing changes unless every check passes:
    ///
    /// * the action is well formed and applied no earlier than it took effect;
    /// * the account holds exactly `held_quantity` of the instrument, the position
    ///   the action was stated against;
    /// * for a split, no order of either kind is working in the instrument, because
    ///   its quantity and limit are in pre-split units and a venue's own response to
    ///   a split is not modelled (E8.2's decision for replay, applied here); and the
    ///   position it leaves is a whole number of the instrument's lots, because cash
    ///   paid in lieu of a fractional share is not modelled either.
    ///
    /// Each action applies once. Repeating an applied action with the same terms
    /// returns its receipt and changes nothing; the same identity with other terms
    /// is refused.
    pub fn apply_corporate_action(
        &mut self,
        request: PaperCorporateAction,
    ) -> Result<PaperCorporateActionReceipt, PaperError> {
        self.ensure_persistence_healthy()?;
        let PaperCorporateAction {
            action,
            held_quantity,
            applied_by,
            applied_at,
        } = request;
        action.validate()?;
        validate_canonical_id("corporate action operator", &applied_by)?;
        validate_utc_timestamp("corporate action applied_at", &applied_at)?;
        // Both are canonical second-precision UTC, so text order is time order.
        if applied_at.as_str() < action.effective_at() {
            return Err(PaperError(format!(
                "corporate action {} takes effect at {}; it cannot be applied at {applied_at}",
                action.action_id(),
                action.effective_at()
            )));
        }
        if let Some(applied) = self
            .corporate_actions
            .iter()
            .find(|receipt| receipt.action.action_id() == action.action_id())
        {
            if applied.action == action && applied.quantity_before == held_quantity {
                return Ok(applied.clone());
            }
            return Err(PaperError(format!(
                "corporate action {} was already applied with other terms",
                action.action_id()
            )));
        }
        let instrument_id = action.instrument_id().to_owned();
        let quantity_before = self.held_quantity(&instrument_id);
        if quantity_before != held_quantity {
            return Err(PaperError(format!(
                "the account holds {quantity_before} of {instrument_id}, not the {held_quantity} corporate action {} was stated against; reconcile before applying it",
                action.action_id()
            )));
        }
        let receipt = match &action {
            CorporateAction::Split { ratio, .. } => {
                self.apply_split(&instrument_id, action.action_id(), *ratio)?;
                PaperCorporateActionReceipt {
                    quantity_after: self.held_quantity(&instrument_id),
                    cash_delta: Decimal::ZERO,
                    action: action.clone(),
                    applied_by,
                    applied_at,
                    quantity_before,
                }
            }
            CorporateAction::CashDividend { amount, .. } => {
                // Signed: a short position pays the dividend it is lent against.
                let cash_delta = quantity_before.checked_mul(*amount)?;
                self.cash = self.cash.checked_add(cash_delta)?;
                PaperCorporateActionReceipt {
                    quantity_after: quantity_before,
                    cash_delta,
                    action: action.clone(),
                    applied_by,
                    applied_at,
                    quantity_before,
                }
            }
        };
        self.corporate_actions.push(receipt.clone());
        self.persist()?;
        Ok(receipt)
    }

    /// Every corporate action applied to the account, in the order applied.
    pub fn corporate_actions(&self) -> &[PaperCorporateActionReceipt] {
        &self.corporate_actions
    }

    /// The signed position the account holds in `instrument_id`, zero when none.
    fn held_quantity(&self, instrument_id: &str) -> Decimal {
        self.portfolios
            .get(instrument_id)
            .map_or(Decimal::ZERO, |portfolio| {
                portfolio.position_snapshot().quantity
            })
    }

    /// Scales every book that holds the instrument by `ratio`, or none of them.
    fn apply_split(
        &mut self,
        instrument_id: &str,
        action_id: &str,
        ratio: Decimal,
    ) -> Result<(), PaperError> {
        if let Some(order_id) = self.working_order_in(instrument_id) {
            return Err(PaperError(format!(
                "working order {order_id} cannot rest across {instrument_id}'s split {action_id}; cancel it before applying the split"
            )));
        }
        // Every book is scaled on a copy first, so a refusal leaves all of them unchanged.
        let portfolio = match self.portfolios.get(instrument_id) {
            Some(portfolio) => {
                let mut scaled = portfolio.clone();
                scaled.apply_split(ratio)?;
                let quantity = scaled.position_snapshot().quantity;
                let magnitude = if quantity < Decimal::ZERO {
                    Decimal::ZERO.checked_sub(quantity)?
                } else {
                    quantity
                };
                if self
                    .risk_policy
                    .lot_rejection(instrument_id, magnitude)
                    .is_some()
                {
                    return Err(PaperError(format!(
                        "split {action_id} would leave {quantity} of {instrument_id}, not a whole number of its lots; cash in lieu of a fraction is not modelled"
                    )));
                }
                Some(scaled)
            }
            None => None,
        };
        let mut tax_lots = self.tax_lots.clone();
        tax_lots.apply_split(instrument_id, ratio)?;
        let mut attribution = self.strategy_attribution.get(instrument_id).cloned();
        if let Some(strategies) = attribution.as_mut() {
            for quantity in strategies.values_mut() {
                *quantity = quantity.checked_mul(ratio)?;
            }
        }
        let mark = match self.marks.get(instrument_id) {
            Some(mark) => {
                let rebased = mark.checked_div(ratio)?;
                if rebased <= Decimal::ZERO {
                    return Err(PaperError(format!(
                        "split {action_id} would round {instrument_id}'s mark of {mark} down to nothing"
                    )));
                }
                Some(rebased)
            }
            None => None,
        };
        if let Some(portfolio) = portfolio {
            self.portfolios.insert(instrument_id.to_owned(), portfolio);
        }
        self.tax_lots = tax_lots;
        if let Some(strategies) = attribution {
            self.strategy_attribution
                .insert(instrument_id.to_owned(), strategies);
        }
        if let Some(mark) = mark {
            self.marks.insert(instrument_id.to_owned(), mark);
        }
        Ok(())
    }

    /// The first order of either kind, plain or combination, still working in
    /// `instrument_id`. An `UNKNOWN` order counts: it may be live at the venue.
    fn working_order_in(&self, instrument_id: &str) -> Option<&str> {
        self.orders
            .values()
            .find(|order| order.working() && order.oms.intent.instrument_id == instrument_id)
            .map(|order| order.oms.order_id.as_str())
            .or_else(|| {
                self.combo_orders
                    .values()
                    .find(|order| {
                        order.working()
                            && order
                                .oms
                                .intent
                                .legs
                                .iter()
                                .any(|leg| leg.instrument_id == instrument_id)
                    })
                    .map(|order| order.oms.order_id.as_str())
            })
    }
}
