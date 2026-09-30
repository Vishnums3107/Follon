//! Order queries, cancellation and replacement.

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, OrderState, Side};

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    /// The durable combination order for one client identity, if known.
    pub fn combo_order(&self, order_id: &str) -> Option<&PaperComboOrder> {
        self.combo_orders.get(order_id)
    }

    /// The immutable combination risk evidence for one decision identity.
    pub fn combo_risk_evidence(&self, decision_id: &str) -> Option<&PaperComboRiskEvidence> {
        self.combo_risk_evidence.get(decision_id)
    }

    pub(crate) fn combo_order_mut(
        &mut self,
        order_id: &str,
    ) -> Result<&mut PaperComboOrder, PaperError> {
        self.combo_orders
            .get_mut(order_id)
            .ok_or_else(|| PaperError("paper OMS does not know combination order".to_owned()))
    }

    /// Requests cancellation. A transport failure leaves the order explicitly `UNKNOWN`.
    pub fn cancel_order(&mut self, order_id: &str) -> Result<(), PaperError> {
        self.cancel_order_attributed(order_id, None)
    }

    /// Requests cancellation of a single or combination order for an
    /// authenticated operator. The request is journaled with the operator and
    /// time, together with the order's move to `PENDING_CANCEL` and before the
    /// broker is asked. A retry of a cancellation already accepted changes
    /// nothing and journals nothing, as a repeated kill-switch change does
    /// (delivery state E5.2b).
    pub fn cancel_order_as(
        &mut self,
        order_id: &str,
        operator: &str,
        operated_at: &str,
    ) -> Result<(), PaperError> {
        validate_canonical_id("order operator", operator)?;
        validate_utc_timestamp("order operation time", operated_at)?;
        self.cancel_order_attributed(
            order_id,
            Some(OrderOperation {
                order_id: order_id.to_owned(),
                action: OrderOperationAction::CancelRequested,
                operator: operator.to_owned(),
                operated_at: operated_at.to_owned(),
            }),
        )
    }

    fn cancel_order_attributed(
        &mut self,
        order_id: &str,
        operation: Option<OrderOperation>,
    ) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        if self.combo_orders.contains_key(order_id) {
            return self.cancel_combo_order(order_id, operation);
        }
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect and reconcile before cancellation"
                    .to_owned(),
            ));
        }
        let state = self
            .orders
            .get(order_id)
            .ok_or_else(|| PaperError("paper OMS does not know order".to_owned()))?
            .oms
            .state;
        // A retry after the original cancellation was durably accepted or
        // completed is an idempotent success. Re-sending to the adapter could
        // manufacture an avoidable broker error and turn authoritative
        // cancellation evidence into ambiguity.
        if matches!(state, OrderState::PendingCancel | OrderState::Cancelled) {
            return Ok(());
        }
        if !matches!(
            state,
            OrderState::Acknowledged | OrderState::PartiallyFilled
        ) {
            return Err(PaperError(
                "only acknowledged or partially filled paper orders can be cancelled".to_owned(),
            ));
        }
        self.order_mut(order_id)?
            .oms
            .transition(OrderState::PendingCancel, "PAPER_CANCEL_REQUESTED")?;
        if let Some(operation) = operation {
            self.order_operations.push(operation);
        }
        // Durable before the external command, as submission and combination
        // cancellation are: a crash after the broker accepts the cancellation
        // cannot leave the journal saying the order still works, or losing
        // who asked for it (E5.2b).
        self.persist()?;
        let cancel = BrokerCancelRequest {
            account_id: self.account.account_id.clone(),
            client_order_id: order_id.to_owned(),
        };
        if let Err(error) = self.broker.cancel(&cancel) {
            self.order_mut(order_id)?
                .oms
                .transition(OrderState::Unknown, "PAPER_CANCEL_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist()?;
            return Err(error);
        }
        Ok(())
    }

    /// Requests a price-only replacement that cannot increase the approved limit risk.
    ///
    /// The immutable order intent and client identity remain unchanged. A broker
    /// result is resolved asynchronously as `Replaced` or `ReplaceRejected`.
    pub fn replace_order(
        &mut self,
        order_id: &str,
        replacement_limit_price: Decimal,
    ) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect and reconcile before replacement"
                    .to_owned(),
            ));
        }
        // Before the order moves to `PENDING_REPLACE`: a route that cannot
        // replace would otherwise leave it `UNKNOWN` and the session
        // disconnected (delivery state E5.1).
        if !self
            .broker
            .capabilities(&self.account.account_id)?
            .replacement
        {
            return Err(PaperError(
                "the configured paper broker route cannot replace orders; the order was left unchanged"
                    .to_owned(),
            ));
        }
        let request = {
            let order = self.order_mut(order_id)?;
            if !matches!(
                order.oms.state,
                OrderState::Acknowledged | OrderState::PartiallyFilled
            ) {
                return Err(PaperError(
                    "only acknowledged or partially filled paper orders can be replaced".to_owned(),
                ));
            }
            let prior_limit = order.oms.intent.limit_price.ok_or_else(|| {
                PaperError("only paper limit orders support price replacement".to_owned())
            })?;
            if replacement_limit_price <= Decimal::ZERO
                || (order.oms.intent.side == Side::Buy && replacement_limit_price > prior_limit)
                || (order.oms.intent.side == Side::Sell && replacement_limit_price < prior_limit)
            {
                return Err(PaperError(
                    "replacement may only reduce the originally approved limit risk".to_owned(),
                ));
            }
            let previous_broker_order_id = order.broker_order_id.clone().ok_or_else(|| {
                PaperError("paper replacement requires an acknowledged broker order ID".to_owned())
            })?;
            order.replace_return_state = Some(order.oms.state);
            order
                .oms
                .transition(OrderState::PendingReplace, "PAPER_REPLACE_REQUESTED")?;
            BrokerReplaceRequest {
                account_id: order.oms.intent.account_id.clone(),
                client_order_id: order.oms.order_id.clone(),
                previous_broker_order_id,
                limit_price: replacement_limit_price,
            }
        };
        if let Err(error) = self.broker.replace(&request) {
            self.order_mut(order_id)?
                .oms
                .transition(OrderState::Unknown, "PAPER_REPLACE_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist()?;
            return Err(error);
        }
        self.persist()
    }

    pub(crate) fn order_mut(&mut self, order_id: &str) -> Result<&mut PaperOrder, PaperError> {
        self.orders
            .get_mut(order_id)
            .ok_or_else(|| PaperError("paper OMS does not know order".to_owned()))
    }
}
