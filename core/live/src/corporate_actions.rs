//! Operator-attested corporate actions in controlled LIVE (delivery state E8.4b).
//!
//! A separate implementation of PAPER's rules, as every LIVE gate is (E1.4a): the
//! environments are configured and reviewed independently, and loosening PAPER must
//! never loosen LIVE. LIVE is also stricter in one respect: a split is applied only
//! once the broker shows the position it leaves.

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use follon_market_data::CorporateAction;
use serde::{Deserialize, Serialize};

use super::*;

/// A corporate action an operator applies to the controlled-LIVE account.
///
/// `held_quantity` is the position the action was stated against, as the broker's
/// notice gives it; the service refuses the request unless the account holds exactly
/// that.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveCorporateAction {
    /// The split or cash dividend, in the contract replay and backtests use.
    pub action: CorporateAction,
    /// The signed position in the action's instrument the action applies to.
    pub held_quantity: Decimal,
    /// The authenticated operator applying it, recorded as the audit actor.
    pub applied_by: String,
    /// Canonical UTC time it is applied, no earlier than it took effect.
    pub applied_at: String,
}

/// What applying one corporate action did to the controlled-LIVE account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveCorporateActionReceipt {
    /// The action applied.
    pub action: CorporateAction,
    /// The authenticated operator who applied it.
    pub applied_by: String,
    /// Canonical UTC time it was applied.
    pub applied_at: String,
    /// The signed position in the instrument before the action.
    pub quantity_before: Decimal,
    /// The signed position after it: scaled by a split, unchanged by a dividend.
    pub quantity_after: Decimal,
    /// Cash a dividend credited to a long or debited from a short; zero for a split.
    pub cash_delta: Decimal,
}

/// One applied corporate action, as the LIVE journal records it. `action_type` and
/// `value` follow the corporate-action CSV contract.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistentLiveCorporateAction {
    action_id: String,
    action_type: String,
    instrument_id: String,
    effective_at: String,
    value: String,
    applied_by: String,
    applied_at: String,
    quantity_before: String,
    quantity_after: String,
    cash_delta: String,
}

impl From<&LiveCorporateActionReceipt> for PersistentLiveCorporateAction {
    fn from(receipt: &LiveCorporateActionReceipt) -> Self {
        let (action_type, value) = match &receipt.action {
            CorporateAction::Split { ratio, .. } => ("SPLIT", ratio),
            CorporateAction::CashDividend { amount, .. } => ("CASH_DIVIDEND", amount),
        };
        Self {
            action_id: receipt.action.action_id().to_owned(),
            action_type: action_type.to_owned(),
            instrument_id: receipt.action.instrument_id().to_owned(),
            effective_at: receipt.action.effective_at().to_owned(),
            value: value.to_string(),
            applied_by: receipt.applied_by.clone(),
            applied_at: receipt.applied_at.clone(),
            quantity_before: receipt.quantity_before.to_string(),
            quantity_after: receipt.quantity_after.to_string(),
            cash_delta: receipt.cash_delta.to_string(),
        }
    }
}

impl TryFrom<PersistentLiveCorporateAction> for LiveCorporateActionReceipt {
    type Error = LiveError;

    /// Restores a receipt, refusing one whose effect is not what its action does to
    /// the quantity it records.
    fn try_from(persisted: PersistentLiveCorporateAction) -> Result<Self, Self::Error> {
        let value = decimal("persisted live corporate action value", &persisted.value)?;
        let action = match persisted.action_type.as_str() {
            "SPLIT" => CorporateAction::Split {
                action_id: persisted.action_id,
                instrument_id: persisted.instrument_id,
                effective_at: persisted.effective_at,
                ratio: value,
            },
            "CASH_DIVIDEND" => CorporateAction::CashDividend {
                action_id: persisted.action_id,
                instrument_id: persisted.instrument_id,
                effective_at: persisted.effective_at,
                amount: value,
            },
            _ => {
                return Err(LiveError(
                    "persisted live corporate action type is invalid".to_owned(),
                ))
            }
        };
        action.validate()?;
        validate_canonical_id(
            "persisted live corporate action operator",
            &persisted.applied_by,
        )?;
        validate_utc_timestamp(
            "persisted live corporate action time",
            &persisted.applied_at,
        )?;
        if persisted.applied_at.as_str() < action.effective_at() {
            return Err(LiveError(
                "persisted live corporate action was applied before it took effect".to_owned(),
            ));
        }
        let quantity_before = decimal(
            "persisted live corporate action quantity before",
            &persisted.quantity_before,
        )?;
        let quantity_after = decimal(
            "persisted live corporate action quantity after",
            &persisted.quantity_after,
        )?;
        let cash_delta = decimal(
            "persisted live corporate action cash",
            &persisted.cash_delta,
        )?;
        let consistent = match &action {
            CorporateAction::Split { ratio, .. } => {
                quantity_after == quantity_before.checked_mul(*ratio)?
                    && cash_delta == Decimal::ZERO
            }
            CorporateAction::CashDividend { amount, .. } => {
                quantity_after == quantity_before
                    && cash_delta == quantity_before.checked_mul(*amount)?
            }
        };
        if !consistent {
            return Err(LiveError(
                "persisted live corporate action effect does not follow from its action".to_owned(),
            ));
        }
        Ok(Self {
            action,
            applied_by: persisted.applied_by,
            applied_at: persisted.applied_at,
            quantity_before,
            quantity_after,
            cash_delta,
        })
    }
}

/// Restores the journaled corporate actions in the order applied, refusing a
/// journal that applies one action twice: each action applies once.
pub(crate) fn restore_live_corporate_actions(
    persisted: Vec<PersistentLiveCorporateAction>,
) -> Result<Vec<LiveCorporateActionReceipt>, LiveError> {
    let mut action_ids = BTreeSet::new();
    let mut receipts = Vec::with_capacity(persisted.len());
    for record in persisted {
        let receipt = LiveCorporateActionReceipt::try_from(record)?;
        if !action_ids.insert(receipt.action.action_id().to_owned()) {
            return Err(LiveError(
                "live audit journal applies one corporate action twice".to_owned(),
            ));
        }
        receipts.push(receipt);
    }
    Ok(receipts)
}

impl<B: LiveBrokerAdapter> LiveTradingService<B> {
    /// Applies a split or a cash dividend to the OMS's own books, audited with the
    /// operator as actor before it returns.
    ///
    /// The checks are PAPER's: a well-formed action applied no earlier than it took
    /// effect, to exactly the position it was stated against; for a split, no order
    /// of either kind working in the instrument and a position left in whole lots.
    /// LIVE adds one: a split is applied only while the broker session is connected
    /// and the broker already shows the position the split leaves. Applied early, the
    /// OMS would let a canary sell shares the account does not yet hold.
    ///
    /// Each action applies once; repeating it with the same terms returns its receipt.
    pub fn apply_corporate_action(
        &mut self,
        request: LiveCorporateAction,
    ) -> Result<LiveCorporateActionReceipt, LiveError> {
        self.ensure_audit_healthy()?;
        let LiveCorporateAction {
            action,
            held_quantity,
            applied_by,
            applied_at,
        } = request;
        action.validate()?;
        validate_canonical_id("live corporate action operator", &applied_by)?;
        validate_utc_timestamp("live corporate action applied_at", &applied_at)?;
        if applied_at.as_str() < action.effective_at() {
            return Err(LiveError(format!(
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
            return Err(LiveError(format!(
                "corporate action {} was already applied with other terms",
                action.action_id()
            )));
        }
        let instrument_id = action.instrument_id().to_owned();
        let quantity_before = self.held_quantity(&instrument_id);
        if quantity_before != held_quantity {
            return Err(LiveError(format!(
                "the account holds {quantity_before} of {instrument_id}, not the {held_quantity} corporate action {} was stated against; reconcile before applying it",
                action.action_id()
            )));
        }
        let (quantity_after, cash_delta) = match &action {
            CorporateAction::Split { ratio, .. } => {
                self.apply_live_split(&instrument_id, action.action_id(), *ratio)?;
                (self.held_quantity(&instrument_id), Decimal::ZERO)
            }
            CorporateAction::CashDividend { amount, .. } => {
                let cash_delta = quantity_before.checked_mul(*amount)?;
                self.cash = self.cash.checked_add(cash_delta)?;
                (quantity_before, cash_delta)
            }
        };
        let correlation_id = format!("corr-corporate-action-{}", action.action_id());
        let receipt = LiveCorporateActionReceipt {
            action,
            applied_by,
            applied_at,
            quantity_before,
            quantity_after,
            cash_delta,
        };
        self.corporate_actions.push(receipt.clone());
        self.persist(
            "live.corporate_action.applied.v1",
            &receipt.applied_by,
            &receipt.applied_at,
            &correlation_id,
        )?;
        Ok(receipt)
    }

    /// Every corporate action applied to the account, in the order applied.
    pub fn corporate_actions(&self) -> &[LiveCorporateActionReceipt] {
        &self.corporate_actions
    }

    fn held_quantity(&self, instrument_id: &str) -> Decimal {
        self.portfolios
            .get(instrument_id)
            .map_or(Decimal::ZERO, |portfolio| {
                portfolio.position_snapshot().quantity
            })
    }

    /// Scales every book that holds the instrument by `ratio`, or none of them.
    fn apply_live_split(
        &mut self,
        instrument_id: &str,
        action_id: &str,
        ratio: Decimal,
    ) -> Result<(), LiveError> {
        if let Some(order_id) = self.live_working_order_in(instrument_id) {
            return Err(LiveError(format!(
                "working order {order_id} cannot rest across {instrument_id}'s split {action_id}; cancel it before applying the split"
            )));
        }
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
                    .policy
                    .lot_rejection(instrument_id, magnitude)
                    .is_some()
                {
                    return Err(LiveError(format!(
                        "split {action_id} would leave {quantity} of {instrument_id}, not a whole number of its lots; cash in lieu of a fraction is not modelled"
                    )));
                }
                Some(scaled)
            }
            None => None,
        };
        let expected = portfolio
            .as_ref()
            .map_or(Decimal::ZERO, |scaled| scaled.position_snapshot().quantity);
        if !self.broker_connected {
            return Err(LiveError(format!(
                "split {action_id} is applied only against the broker's own position; connect the session first"
            )));
        }
        let snapshot = self.broker.snapshot(&self.account.account_id)?;
        let broker_quantity = snapshot
            .positions
            .iter()
            .filter(|position| position.instrument_id == instrument_id)
            .try_fold(Decimal::ZERO, |total, position| {
                total.checked_add(position.quantity)
            })?;
        if broker_quantity != expected {
            return Err(LiveError(format!(
                "the broker holds {broker_quantity} of {instrument_id}, not the {expected} split {action_id} leaves; apply it once the broker has"
            )));
        }
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
                    return Err(LiveError(format!(
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

    /// The first order of either kind still working in `instrument_id`, an
    /// `UNKNOWN` one included.
    fn live_working_order_in(&self, instrument_id: &str) -> Option<&str> {
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
