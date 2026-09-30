//! Applying normalized broker events, fills, tax lots and strategy attribution.

use follon_accounting::{Currency, ShortTaxLot, TaxLot, TaxLotSelection};
use follon_domain::{
    validate_canonical_id, validate_utc_timestamp, Decimal, Fill, OrderState, Side,
};

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    pub(crate) fn apply_broker_event(&mut self, event: BrokerEvent) -> Result<(), PaperError> {
        if self.combo_orders.contains_key(event.client_order_id()) {
            return self.apply_combo_event(event);
        }
        match event {
            BrokerEvent::ComboExecution(_) => {
                return Err(PaperError(
                    "combo execution does not name a combination".to_owned(),
                ))
            }
            BrokerEvent::Acknowledged {
                client_order_id,
                broker_order_id,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_canonical_id("broker order_id", &broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if let Some(existing) = &order.broker_order_id {
                    if existing != &broker_order_id {
                        if order.broker_order_versions.contains(&broker_order_id) {
                            // An acknowledgement for an earlier broker version is late
                            // evidence, not permission to roll the active version back.
                            return Ok(());
                        }
                        return Err(PaperError(
                            "broker reused client order ID with an unrecognized broker ID"
                                .to_owned(),
                        ));
                    }
                }
                if order.broker_order_id.is_none() {
                    order.broker_order_id = Some(broker_order_id.clone());
                }
                if !order.broker_order_versions.contains(&broker_order_id) {
                    order.broker_order_versions.push(broker_order_id);
                }
                if order.is_terminal() {
                    return Ok(());
                }
                transition_to_acknowledged(order, "IBKR_PAPER_ACKNOWLEDGED_EVENT")?;
            }
            BrokerEvent::Execution {
                execution_id,
                client_order_id,
                broker_order_id,
                quantity,
                price,
                fee,
                executed_at,
            } => {
                validate_canonical_id("broker execution_id", &execution_id)?;
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_canonical_id("broker order_id", &broker_order_id)?;
                validate_utc_timestamp("broker execution time", &executed_at)?;
                if quantity <= Decimal::ZERO || price <= Decimal::ZERO || fee < Decimal::ZERO {
                    return Err(PaperError("invalid broker execution values".to_owned()));
                }
                if self.execution_ids.contains(&execution_id) {
                    return Ok(());
                }
                let (instrument_id, side, order_id, strategy_id) = {
                    let order = self.order_mut(&client_order_id)?;
                    if let Some(existing) = &order.broker_order_id {
                        if existing != &broker_order_id
                            && !order.broker_order_versions.contains(&broker_order_id)
                        {
                            return Err(PaperError(
                                "broker execution has an unrecognized broker order version"
                                    .to_owned(),
                            ));
                        }
                    } else {
                        order.broker_order_id = Some(broker_order_id.clone());
                    }
                    if !order.broker_order_versions.contains(&broker_order_id) {
                        order.broker_order_versions.push(broker_order_id.clone());
                    }
                    if matches!(
                        order.oms.state,
                        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
                    ) {
                        order.oms.transition(
                            OrderState::Unknown,
                            "LATE_BROKER_EXECUTION_AFTER_TERMINAL",
                        )?;
                    }
                    transition_to_acknowledged(order, "BROKER_EXECUTION_CONFIRMED_ORDER")?;
                    let total = order.filled_quantity.checked_add(quantity)?;
                    if total > order.oms.intent.quantity {
                        return Err(PaperError(
                            "broker execution exceeds internal requested quantity".to_owned(),
                        ));
                    }
                    order.filled_quantity = total;
                    if total == order.oms.intent.quantity {
                        if order.oms.state != OrderState::Filled {
                            order
                                .oms
                                .transition(OrderState::Filled, "BROKER_FULL_FILL")?;
                        }
                    } else if order.oms.state == OrderState::Acknowledged {
                        order
                            .oms
                            .transition(OrderState::PartiallyFilled, "BROKER_PARTIAL_FILL")?;
                    }
                    (
                        order.oms.intent.instrument_id.clone(),
                        order.oms.intent.side,
                        order.oms.order_id.clone(),
                        order.oms.intent.strategy_id.clone(),
                    )
                };
                self.execution_ids.insert(execution_id.clone());
                let fill = Fill {
                    execution_id,
                    order_id,
                    instrument_id: instrument_id.clone(),
                    side,
                    quantity,
                    price,
                    fee,
                    executed_at,
                };
                self.apply_accounted_fill(&fill, &strategy_id)?;
            }
            BrokerEvent::Cancelled {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker cancellation reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                transition_to_terminal(order, OrderState::Cancelled, &reason)?;
            }
            BrokerEvent::CancelRejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker cancel rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                match order.oms.state {
                    OrderState::PendingCancel | OrderState::Unknown => {
                        restore_working_state(order, "BROKER_CANCEL_REJECTED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(PaperError(
                            "broker cancel rejection is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            BrokerEvent::Expired {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker expiry reason", &reason)?;
                transition_to_terminal(
                    self.order_mut(&client_order_id)?,
                    OrderState::Expired,
                    &reason,
                )?;
            }
            BrokerEvent::ReplaceRequested {
                client_order_id,
                previous_broker_order_id,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_canonical_id("broker previous_order_id", &previous_broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if order.broker_order_id.as_deref() != Some(previous_broker_order_id.as_str()) {
                    return Err(PaperError(
                        "broker replacement does not match active broker order".to_owned(),
                    ));
                }
                match order.oms.state {
                    OrderState::Acknowledged | OrderState::PartiallyFilled => {
                        order.replace_return_state = Some(order.oms.state);
                        order
                            .oms
                            .transition(OrderState::PendingReplace, "BROKER_REPLACE_REQUESTED")?;
                    }
                    OrderState::PendingReplace => {}
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(PaperError(
                            "broker replacement is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            BrokerEvent::Replaced {
                client_order_id,
                previous_broker_order_id,
                broker_order_id,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_canonical_id("broker previous_order_id", &previous_broker_order_id)?;
                validate_canonical_id("broker replacement_order_id", &broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if order.broker_order_id.as_deref() != Some(previous_broker_order_id.as_str()) {
                    if order.broker_order_versions.contains(&broker_order_id) {
                        return Ok(());
                    }
                    return Err(PaperError(
                        "broker replacement does not match active broker order".to_owned(),
                    ));
                }
                match order.oms.state {
                    OrderState::PendingReplace | OrderState::Unknown => {
                        if !order.broker_order_versions.contains(&broker_order_id) {
                            order.broker_order_versions.push(broker_order_id.clone());
                        }
                        order.broker_order_id = Some(broker_order_id);
                        restore_replacement_state(order, "BROKER_REPLACED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(PaperError(
                            "broker replacement is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            BrokerEvent::ReplaceRejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker replacement rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                match order.oms.state {
                    OrderState::PendingReplace | OrderState::Unknown => {
                        restore_replacement_state(order, "BROKER_REPLACE_REJECTED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(PaperError(
                            "broker replacement rejection is incompatible with OMS state"
                                .to_owned(),
                        ))
                    }
                }
            }
            BrokerEvent::Rejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                transition_to_terminal(order, OrderState::Rejected, &reason)?;
            }
        }
        Ok(())
    }

    /// Applies a fill to the FIFO long/short books, closing opposite inventory
    /// first. A crossing fill splits its fee exactly once; the remainder is
    /// assigned to its opening portion, preserving total fees at fixed precision.
    pub(crate) fn apply_tax_lot_fill(&mut self, fill: &Fill) -> Result<(), PaperError> {
        let currency = Currency::new(self.account.currency.clone())?;
        let available = match fill.side {
            Side::Buy => self
                .tax_lots
                .short_lots(&fill.instrument_id)
                .iter()
                .try_fold(Decimal::ZERO, |total, lot| {
                    total.checked_add(lot.remaining_quantity)
                })?,
            Side::Sell => self
                .tax_lots
                .lots(&fill.instrument_id)
                .iter()
                .try_fold(Decimal::ZERO, |total, lot| {
                    total.checked_add(lot.remaining_quantity)
                })?,
        };
        let closing = available.min(fill.quantity);
        let opening = fill.quantity.checked_sub(closing)?;
        let close_fee = if closing == fill.quantity {
            fill.fee
        } else {
            fill.fee.checked_mul(closing)?.checked_div(fill.quantity)?
        };
        if closing > Decimal::ZERO {
            match fill.side {
                Side::Buy => {
                    self.tax_lots.cover(
                        &format!("taxcover-{}", fill.execution_id),
                        &fill.instrument_id,
                        &currency,
                        closing,
                        fill.price,
                        close_fee,
                        &fill.executed_at,
                        TaxLotSelection::Fifo,
                    )?;
                }
                Side::Sell => {
                    self.tax_lots.dispose(
                        &format!("taxdisposal-{}", fill.execution_id),
                        &fill.instrument_id,
                        &currency,
                        closing,
                        fill.price,
                        close_fee,
                        &fill.executed_at,
                        TaxLotSelection::Fifo,
                    )?;
                }
            }
        }
        if opening == Decimal::ZERO {
            return Ok(());
        }
        let open_fee = fill.fee.checked_sub(close_fee)?;
        match fill.side {
            Side::Buy => {
                let gross = fill.price.checked_mul(opening)?;
                let unit_cost = gross.checked_add(open_fee)?.checked_div(opening)?;
                self.tax_lots.acquire(TaxLot {
                    lot_id: format!("taxlot-{}", fill.execution_id),
                    instrument_id: fill.instrument_id.clone(),
                    currency,
                    opened_at: fill.executed_at.clone(),
                    remaining_quantity: opening,
                    unit_cost,
                })?;
            }
            Side::Sell => {
                self.tax_lots.open_short(ShortTaxLot {
                    lot_id: format!("taxshort-{}", fill.execution_id),
                    instrument_id: fill.instrument_id.clone(),
                    currency,
                    opened_at: fill.executed_at.clone(),
                    remaining_quantity: opening,
                    unit_proceeds: fill
                        .price
                        .checked_mul(opening)?
                        .checked_sub(open_fee)?
                        .checked_div(opening)?,
                })?;
            }
        }
        Ok(())
    }

    /// Updates each strategy's own net signed contribution to one
    /// instrument's aggregate position: a buy adds, a sell subtracts, never
    /// clamped or floored at zero. This is a deliberately separate,
    /// paper-local bookkeeping layer, not a change to `Portfolio` or
    /// `PositionSnapshot` (the canonical position of record, also part of
    /// the audit event schema) -- see
    /// docs/06-delivery/14-master-plan-conformance-audit.md (row 5.7) for why
    /// that remains explicitly out of scope. Letting a value go negative
    /// (a strategy net-sold more than it net-bought, e.g. because another
    /// strategy holds shares of the same instrument) is intentional: summed
    /// with every other strategy's tracked value, it always reconciles
    /// exactly against `Portfolio`'s own aggregate quantity, so
    /// `portfolio_risk_decision`'s per-strategy `RiskPosition` rows plus one
    /// "unattributed" remainder row can never mis-state total gross/net
    /// exposure, only how it is attributed across strategies.
    pub(crate) fn apply_strategy_attribution_fill(
        &mut self,
        fill: &Fill,
        strategy_id: &str,
    ) -> Result<(), PaperError> {
        let instrument_attribution = self
            .strategy_attribution
            .entry(fill.instrument_id.clone())
            .or_default();
        let current = instrument_attribution
            .get(strategy_id)
            .copied()
            .unwrap_or(Decimal::ZERO);
        let updated = match fill.side {
            Side::Buy => current.checked_add(fill.quantity)?,
            Side::Sell => current.checked_sub(fill.quantity)?,
        };
        instrument_attribution.insert(strategy_id.to_owned(), updated);
        Ok(())
    }

    /// Remaining open FIFO tax lots for one instrument, oldest first.
    pub fn tax_lots(&self, instrument_id: &str) -> &[TaxLot] {
        self.tax_lots.lots(instrument_id)
    }

    /// Cumulative FIFO-realized tax P&L in the account's reporting currency,
    /// independent of `Portfolio`'s average-cost realized P&L.
    pub fn realized_tax_pnl(&self) -> Result<Decimal, PaperError> {
        let currency = Currency::new(self.account.currency.clone())?;
        Ok(self.tax_lots.realized(&currency))
    }
}

fn transition_to_acknowledged(order: &mut PaperOrder, reason: &str) -> Result<(), PaperError> {
    match order.oms.state {
        OrderState::PendingSubmit => {
            order.oms.transition(OrderState::Submitted, reason)?;
            order.oms.transition(OrderState::Acknowledged, reason)?;
        }
        OrderState::Submitted | OrderState::Unknown => {
            order.oms.transition(OrderState::Acknowledged, reason)?;
        }
        OrderState::Acknowledged
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::PendingReplace => {}
        _ => {
            return Err(PaperError(
                "broker acknowledgement is incompatible with internal OMS state".to_owned(),
            ))
        }
    }
    Ok(())
}

fn restore_working_state(order: &mut PaperOrder, reason: &str) -> Result<(), PaperError> {
    let state = if order.filled_quantity == Decimal::ZERO {
        OrderState::Acknowledged
    } else {
        OrderState::PartiallyFilled
    };
    order.oms.transition(state, reason)?;
    Ok(())
}

fn restore_replacement_state(order: &mut PaperOrder, reason: &str) -> Result<(), PaperError> {
    let state = order.replace_return_state.take().unwrap_or_else(|| {
        if order.filled_quantity == Decimal::ZERO {
            OrderState::Acknowledged
        } else {
            OrderState::PartiallyFilled
        }
    });
    order.oms.transition(state, reason)?;
    Ok(())
}

fn transition_to_terminal(
    order: &mut PaperOrder,
    terminal: OrderState,
    reason: &str,
) -> Result<(), PaperError> {
    debug_assert!(matches!(
        terminal,
        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
    ));
    match order.oms.state {
        OrderState::PendingSubmit => {
            order.oms.transition(OrderState::Submitted, reason)?;
            order.oms.transition(terminal, reason)?;
        }
        OrderState::Submitted
        | OrderState::Acknowledged
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel
        | OrderState::PendingReplace
        | OrderState::Unknown => {
            order.oms.transition(terminal, reason)?;
        }
        state if state == terminal => {}
        // A later, conflicting terminal status is retained as late evidence but
        // never overwrites the original terminal conclusion.
        OrderState::Filled | OrderState::Cancelled | OrderState::Rejected | OrderState::Expired => {
        }
        _ => {
            return Err(PaperError(
                "broker terminal event is incompatible with OMS state".to_owned(),
            ))
        }
    }
    if matches!(
        order.oms.state,
        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
    ) && order.filled_quantity >= order.oms.intent.quantity
    {
        return Err(PaperError(
            "non-filled terminal state cannot have cumulative quantity equal to requested quantity"
                .to_owned(),
        ));
    }
    Ok(())
}
