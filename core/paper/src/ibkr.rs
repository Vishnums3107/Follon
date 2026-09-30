//! PAPER adapter over the IBKR paper transport.

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, OrderState, Side};
use std::collections::{BTreeMap, VecDeque};

use crate::*;

#[derive(Clone, Debug)]
struct IbkrOrder {
    broker_order_id: String,
    request: BrokerOrderRequest,
    state: OrderState,
    filled_quantity: Decimal,
}

/// One accepted atomic combination in the paper-bridge model.
#[derive(Clone, Debug)]
pub(crate) struct IbkrCombo {
    pub(crate) broker_order_id: String,
    pub(crate) request: BrokerComboRequest,
    pub(crate) state: OrderState,
    pub(crate) filled_quantity: Decimal,
}

/// Deterministic local model of the Interactive Brokers paper-order contract.
///
/// This adapter is deliberately named and configured as paper-only. It models
/// idempotent client IDs, delayed acknowledgements, fills, disconnects, and
/// account snapshots. A production TWS/Gateway transport must implement this
/// same [`PaperBrokerAdapter`] contract; no live endpoint is accepted here.
pub struct IbkrPaperAdapter {
    account_id: String,
    connected: bool,
    next_order: u64,
    next_execution: u64,
    orders: BTreeMap<String, IbkrOrder>,
    /// Accepted atomic combinations, keyed by client identity. Separate from
    /// `orders` because a combination is one broker order over several
    /// instruments and has no single `BrokerOrderRequest` shape.
    pub(crate) combos: BTreeMap<String, IbkrCombo>,
    pub(crate) positions: BTreeMap<String, Decimal>,
    pub(crate) cash: Decimal,
    pub(crate) pending_events: VecDeque<BrokerEvent>,
}

impl IbkrPaperAdapter {
    /// Creates a paper-only IBKR adapter model for one configured account.
    pub fn new(account: &PaperAccount) -> Result<Self, PaperError> {
        account.validate()?;
        Ok(Self {
            account_id: account.account_id.clone(),
            connected: true,
            next_order: 1,
            next_execution: 1,
            orders: BTreeMap::new(),
            combos: BTreeMap::new(),
            positions: BTreeMap::new(),
            cash: account.initial_cash,
            pending_events: VecDeque::new(),
        })
    }

    /// Simulates an interrupted paper gateway. Later reconciliation is mandatory.
    pub fn disconnect(&mut self) {
        self.connected = false;
    }

    /// Queues an exact IBKR-paper execution for a previously acknowledged order.
    pub fn queue_fill(
        &mut self,
        client_order_id: &str,
        quantity: Decimal,
        price: Decimal,
        fee: Decimal,
        executed_at: &str,
    ) -> Result<String, PaperError> {
        validate_canonical_id("client_order_id", client_order_id)?;
        validate_utc_timestamp("paper execution time", executed_at)?;
        if quantity <= Decimal::ZERO || price <= Decimal::ZERO || fee < Decimal::ZERO {
            return Err(PaperError("invalid paper execution values".to_owned()));
        }
        let order = self
            .orders
            .get_mut(client_order_id)
            .ok_or_else(|| PaperError("paper broker does not know client order".to_owned()))?;
        let next_total = order.filled_quantity.checked_add(quantity)?;
        if next_total > order.request.quantity {
            return Err(PaperError(
                "paper execution would exceed requested quantity".to_owned(),
            ));
        }
        order.filled_quantity = next_total;
        order.state = if next_total == order.request.quantity {
            OrderState::Filled
        } else {
            OrderState::PartiallyFilled
        };
        let position = self
            .positions
            .entry(order.request.instrument_id.clone())
            .or_insert(Decimal::ZERO);
        *position = match order.request.side {
            Side::Buy => position.checked_add(quantity)?,
            Side::Sell => position.checked_sub(quantity)?,
        };
        let gross = price.checked_mul(quantity)?;
        self.cash = match order.request.side {
            Side::Buy => self.cash.checked_sub(gross.checked_add(fee)?)?,
            Side::Sell => self.cash.checked_add(gross.checked_sub(fee)?)?,
        };
        let execution_id = format!("ibkr-paper-exec-{:08}", self.next_execution);
        self.next_execution += 1;
        self.pending_events.push_back(BrokerEvent::Execution {
            execution_id: execution_id.clone(),
            client_order_id: client_order_id.to_owned(),
            broker_order_id: order.broker_order_id.clone(),
            quantity,
            price,
            fee,
            executed_at: executed_at.to_owned(),
        });
        Ok(execution_id)
    }
}

impl PaperBrokerAdapter for IbkrPaperAdapter {
    /// The model executes combinations and replacements, and never drops or
    /// rewrites a time in force. These are model capabilities: the real
    /// official-API bridge declares none of them (delivery state E5.1).
    fn capabilities(&self, account_id: &str) -> Result<PaperBrokerCapabilities, PaperError> {
        if account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match adapter configuration".to_owned(),
            ));
        }
        Ok(PaperBrokerCapabilities {
            combinations: true,
            good_til_cancelled: true,
            replacement: true,
        })
    }

    fn adapter_configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        if account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match adapter configuration".to_owned(),
            ));
        }
        Ok(format!("ibkr-paper-model-v1|{}", self.account_id))
    }

    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        request.validate()?;
        if !self.connected {
            return Err(PaperError(
                "IBKR paper connection is unavailable; submission outcome is unknown".to_owned(),
            ));
        }
        if request.account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match request".to_owned(),
            ));
        }
        if let Some(existing) = self.orders.get(&request.client_order_id) {
            if existing.request != *request {
                return Err(PaperError(
                    "client order id was reused with different paper request data".to_owned(),
                ));
            }
            return Ok(BrokerSubmitResult::Acknowledged {
                broker_order_id: existing.broker_order_id.clone(),
            });
        }
        let broker_order_id = format!("ibkr-paper-order-{:08}", self.next_order);
        self.next_order += 1;
        self.orders.insert(
            request.client_order_id.clone(),
            IbkrOrder {
                broker_order_id: broker_order_id.clone(),
                request: request.clone(),
                state: OrderState::Acknowledged,
                filled_quantity: Decimal::ZERO,
            },
        );
        self.pending_events.push_back(BrokerEvent::Acknowledged {
            client_order_id: request.client_order_id.clone(),
            broker_order_id: broker_order_id.clone(),
        });
        Ok(BrokerSubmitResult::Acknowledged { broker_order_id })
    }

    /// Accepts an atomic combination. This is a model capability only.
    ///
    /// It was added on the belief that the genuine IBKR paper transport
    /// supports native BAG combinations. It does not: its
    /// `submit_paper_combo` forwards a `submit_combo` request that the Python
    /// bridge's dispatch does not implement, and nothing in the repository
    /// builds a BAG contract (delivery state E5.1, corrected 2026-09-28). The
    /// model keeps combinations so the OMS combination path stays testable end
    /// to end; that is not evidence the real bridge can execute one.
    fn submit_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        request.validate()?;
        if !self.connected {
            return Err(PaperError(
                "IBKR paper connection is unavailable; combination outcome is unknown".to_owned(),
            ));
        }
        if request.account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match combination request".to_owned(),
            ));
        }
        if let Some(existing) = self.combos.get(&request.client_order_id) {
            if existing.request != *request {
                return Err(PaperError(
                    "client order id was reused with different paper combination data".to_owned(),
                ));
            }
            return Ok(BrokerSubmitResult::Acknowledged {
                broker_order_id: existing.broker_order_id.clone(),
            });
        }
        let broker_order_id = format!("ibkr-paper-combo-{:08}", self.next_order);
        self.next_order += 1;
        self.combos.insert(
            request.client_order_id.clone(),
            IbkrCombo {
                broker_order_id: broker_order_id.clone(),
                request: request.clone(),
                state: OrderState::Acknowledged,
                filled_quantity: Decimal::ZERO,
            },
        );
        Ok(BrokerSubmitResult::Acknowledged { broker_order_id })
    }

    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        request.validate()?;
        if request.account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match cancellation request".to_owned(),
            ));
        }
        if !self.connected {
            return Err(PaperError(
                "IBKR paper connection is unavailable; cancellation outcome is unknown".to_owned(),
            ));
        }
        if let Some(combo) = self.combos.get_mut(&request.client_order_id) {
            if !matches!(
                combo.state,
                OrderState::Acknowledged | OrderState::PartiallyFilled
            ) {
                return Err(PaperError(
                    "paper combination is already terminal".to_owned(),
                ));
            }
            combo.state = OrderState::Cancelled;
            self.pending_events.push_back(BrokerEvent::Cancelled {
                client_order_id: request.client_order_id.clone(),
                reason: "IBKR_PAPER_COMBO_CANCELLED".to_owned(),
            });
            return Ok(());
        }
        let order = self
            .orders
            .get_mut(&request.client_order_id)
            .ok_or_else(|| PaperError("paper broker does not know client order".to_owned()))?;
        if matches!(
            order.state,
            OrderState::Filled | OrderState::Cancelled | OrderState::Rejected
        ) {
            return Err(PaperError("paper order is already terminal".to_owned()));
        }
        order.state = OrderState::Cancelled;
        self.pending_events.push_back(BrokerEvent::Cancelled {
            client_order_id: request.client_order_id.clone(),
            reason: "IBKR_PAPER_CANCELLED".to_owned(),
        });
        Ok(())
    }

    fn replace(&mut self, request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        request.validate()?;
        if request.account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match replacement request".to_owned(),
            ));
        }
        if !self.connected {
            return Err(PaperError(
                "IBKR paper connection is unavailable; replacement outcome is unknown".to_owned(),
            ));
        }
        let order = self
            .orders
            .get_mut(&request.client_order_id)
            .ok_or_else(|| PaperError("paper broker does not know client order".to_owned()))?;
        if order.broker_order_id != request.previous_broker_order_id
            || !matches!(
                order.state,
                OrderState::Acknowledged | OrderState::PartiallyFilled
            )
        {
            return Err(PaperError(
                "paper replacement does not match a working broker order".to_owned(),
            ));
        }
        let broker_order_id = format!("ibkr-paper-order-{:08}", self.next_order);
        self.next_order += 1;
        let previous_broker_order_id =
            std::mem::replace(&mut order.broker_order_id, broker_order_id.clone());
        order.request.limit_price = Some(request.limit_price);
        self.pending_events.push_back(BrokerEvent::Replaced {
            client_order_id: request.client_order_id.clone(),
            previous_broker_order_id,
            broker_order_id,
        });
        Ok(())
    }

    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        if account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match poll request".to_owned(),
            ));
        }
        if !self.connected {
            return Err(PaperError(
                "IBKR paper connection is unavailable".to_owned(),
            ));
        }
        Ok(self.pending_events.drain(..).collect())
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        if account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match snapshot request".to_owned(),
            ));
        }
        Ok(BrokerAccountSnapshot {
            orders: self
                .orders
                .iter()
                .map(|(client_order_id, order)| BrokerOrderSnapshot {
                    client_order_id: client_order_id.clone(),
                    broker_order_id: order.broker_order_id.clone(),
                    state: order.state,
                    filled_quantity: order.filled_quantity,
                })
                .chain(self.combos.iter().map(|(id, order)| BrokerOrderSnapshot {
                    client_order_id: id.clone(),
                    broker_order_id: order.broker_order_id.clone(),
                    state: order.state,
                    filled_quantity: order.filled_quantity,
                }))
                .collect(),
            positions: self
                .positions
                .iter()
                .map(|(instrument_id, quantity)| BrokerPositionSnapshot {
                    instrument_id: instrument_id.clone(),
                    quantity: *quantity,
                })
                .collect(),
            cash: self.cash,
        })
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        if account_id != self.account_id {
            return Err(PaperError(
                "IBKR paper account does not match reconnect request".to_owned(),
            ));
        }
        self.connected = true;
        Ok(())
    }
}
