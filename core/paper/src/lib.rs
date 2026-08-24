//! Paper-only operational OMS, risk controls, broker fault injection, and reconciliation.
//!
//! This crate is intentionally incapable of live trading. It owns the state
//! between a validated paper intent and a normalized broker response, preserving
//! the safe `UNKNOWN` lifecycle state whenever submission or cancellation cannot
//! be proven. Broker snapshots are compared against independent internal state;
//! reconciliation never silently overwrites that state.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_control_plane::{EngineError, OmsOrder, Portfolio};
use follon_domain::{
    price_deviation_bps, validate_canonical_id, validate_utc_timestamp, Decimal, Fill, OrderIntent,
    OrderState, OrderType, RiskDecision, Side, TimeInForce,
};
use follon_instrument::{TradingCalendar, TradingSession};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Paper-operations construction, adapter, accounting, or reconciliation failure.
#[derive(Debug)]
pub struct PaperError(pub String);

impl std::fmt::Display for PaperError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PaperError {}

impl From<EngineError> for PaperError {
    fn from(error: EngineError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DomainError> for PaperError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DecimalError> for PaperError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

/// Explicit account configuration for the paper-only operational service.
#[derive(Clone, Debug)]
pub struct PaperAccount {
    /// Canonical account identity supplied to every broker request.
    pub account_id: String,
    /// Single reporting currency for this milestone.
    pub currency: String,
    /// Independent internal opening cash balance.
    pub initial_cash: Decimal,
    /// Must be the literal value `PAPER`.
    pub environment: String,
}

impl PaperAccount {
    /// Validates that an account cannot be accidentally configured for live execution.
    pub fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("paper account_id", &self.account_id)?;
        if self.currency.len() != 3
            || !self
                .currency
                .bytes()
                .all(|character| character.is_ascii_uppercase())
            || self.initial_cash < Decimal::ZERO
            || self.environment != "PAPER"
        {
            return Err(PaperError(
                "paper account must use a currency, non-negative cash, and PAPER environment"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// A normalized order request sent only by the OMS to a paper broker adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerOrderRequest {
    /// OMS-generated immutable client idempotency key.
    pub client_order_id: String,
    /// Paper account selected for the operation.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Requested side.
    pub side: Side,
    /// Exact requested quantity.
    pub quantity: Decimal,
    /// Optional limit price; `None` denotes a market order.
    pub limit_price: Option<Decimal>,
}

impl BrokerOrderRequest {
    fn from_order(order: &OmsOrder) -> Self {
        Self {
            client_order_id: order.order_id.clone(),
            account_id: order.intent.account_id.clone(),
            instrument_id: order.intent.instrument_id.clone(),
            side: order.intent.side,
            quantity: order.intent.quantity,
            limit_price: order.intent.limit_price,
        }
    }

    fn validate(&self) -> Result<(), PaperError> {
        for (name, value) in [
            ("client_order_id", self.client_order_id.as_str()),
            ("account_id", self.account_id.as_str()),
            ("instrument_id", self.instrument_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.quantity <= Decimal::ZERO
            || self.limit_price.is_some_and(|price| price <= Decimal::ZERO)
        {
            return Err(PaperError("invalid broker order request".to_owned()));
        }
        Ok(())
    }
}

/// A price-only, risk-preserving modification of a working broker order.
///
/// Quantity, side, instrument, and client idempotency identity never change.
/// A broker issues a new native order id for the replacement; both versions
/// remain attributable to the same immutable OMS order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerReplaceRequest {
    /// Immutable OMS client identity.
    pub client_order_id: String,
    /// Current broker-native identity being superseded.
    pub previous_broker_order_id: String,
    /// New positive limit price.
    pub limit_price: Decimal,
}

impl BrokerReplaceRequest {
    fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("replace client_order_id", &self.client_order_id)?;
        validate_canonical_id(
            "replace previous_broker_order_id",
            &self.previous_broker_order_id,
        )?;
        if self.limit_price <= Decimal::ZERO {
            return Err(PaperError(
                "replacement limit price must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Definite or deliberately ambiguous outcome of a paper-broker submission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrokerSubmitResult {
    /// The broker acknowledged the client order id.
    Acknowledged {
        /// Broker-side immutable order identity.
        broker_order_id: String,
    },
    /// The broker explicitly rejected the request.
    Rejected {
        /// Broker-supplied stable rejection reason.
        reason: String,
    },
    /// The network outcome is unknown and must be reconciled before retrying.
    Unknown {
        /// Evidence-safe explanation of the ambiguity.
        reason: String,
    },
}

/// Normalized asynchronous broker evidence consumed by the paper OMS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrokerEvent {
    /// Broker acknowledgement, possibly arriving after reconnect.
    Acknowledged {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Broker order identity.
        broker_order_id: String,
    },
    /// Broker-accepted execution. Duplicate execution ids are safe no-ops.
    Execution {
        /// Broker execution identity.
        execution_id: String,
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Broker order identity.
        broker_order_id: String,
        /// Exact filled quantity.
        quantity: Decimal,
        /// Exact execution price.
        price: Decimal,
        /// Exact commission in account currency.
        fee: Decimal,
        /// Canonical UTC execution timestamp.
        executed_at: String,
    },
    /// Broker cancellation confirmation.
    Cancelled {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Stable broker reason.
        reason: String,
    },
    /// Broker rejected a requested cancellation; the order remains working.
    CancelRejected {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Stable broker reason.
        reason: String,
    },
    /// Broker observed time-in-force expiry.
    Expired {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Stable broker reason.
        reason: String,
    },
    /// Broker observed a modification request before its result is known.
    ReplaceRequested {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Current broker order identity being replaced.
        previous_broker_order_id: String,
    },
    /// Broker accepted a modification and assigned a new native order identity.
    Replaced {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Previous broker order identity.
        previous_broker_order_id: String,
        /// Replacement broker order identity.
        broker_order_id: String,
    },
    /// Broker rejected a modification; the prior broker version remains working.
    ReplaceRejected {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Stable broker reason.
        reason: String,
    },
    /// Broker rejection, including an asynchronous rejection after a submit acknowledgement.
    Rejected {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Stable broker reason.
        reason: String,
    },
}

/// One broker order included in a reconciled account snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerOrderSnapshot {
    /// OMS idempotency key as echoed by the broker adapter.
    pub client_order_id: String,
    /// Broker-native order id.
    pub broker_order_id: String,
    /// Normalized lifecycle state.
    pub state: OrderState,
    /// Exact total executed quantity observed by the broker.
    pub filled_quantity: Decimal,
}

/// One broker position included in a reconciled account snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerPositionSnapshot {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Signed exact position quantity.
    pub quantity: Decimal,
}

/// Independent broker account view used for reconciliation only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerAccountSnapshot {
    /// Broker orders keyed by client idempotency identity.
    pub orders: Vec<BrokerOrderSnapshot>,
    /// Broker positions keyed by canonical instrument identity.
    pub positions: Vec<BrokerPositionSnapshot>,
    /// Broker-reported cash in the configured account currency.
    pub cash: Decimal,
}

/// Paper-only broker boundary. It has no live environment parameter.
pub trait PaperBrokerAdapter {
    /// Submits exactly one client-idempotent paper order.
    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError>;
    /// Requests cancellation by the immutable client idempotency key.
    fn cancel(&mut self, client_order_id: &str) -> Result<(), PaperError>;
    /// Requests a price-only replacement. The result arrives through [`BrokerEvent`].
    fn replace(&mut self, _request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        Err(PaperError(
            "paper broker adapter does not support order replacement".to_owned(),
        ))
    }
    /// Drains normalized asynchronous evidence in arrival order.
    fn poll(&mut self) -> Result<Vec<BrokerEvent>, PaperError>;
    /// Returns an independent broker-side account snapshot for reconciliation.
    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError>;
    /// Re-establishes the paper connection. The caller must reconcile afterwards.
    fn reconnect(&mut self) -> Result<(), PaperError>;
}

#[derive(Clone, Debug)]
struct IbkrOrder {
    broker_order_id: String,
    request: BrokerOrderRequest,
    state: OrderState,
    filled_quantity: Decimal,
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
    positions: BTreeMap<String, Decimal>,
    cash: Decimal,
    pending_events: VecDeque<BrokerEvent>,
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

    fn cancel(&mut self, client_order_id: &str) -> Result<(), PaperError> {
        validate_canonical_id("client_order_id", client_order_id)?;
        if !self.connected {
            return Err(PaperError(
                "IBKR paper connection is unavailable; cancellation outcome is unknown".to_owned(),
            ));
        }
        let order = self
            .orders
            .get_mut(client_order_id)
            .ok_or_else(|| PaperError("paper broker does not know client order".to_owned()))?;
        if matches!(
            order.state,
            OrderState::Filled | OrderState::Cancelled | OrderState::Rejected
        ) {
            return Err(PaperError("paper order is already terminal".to_owned()));
        }
        order.state = OrderState::Cancelled;
        self.pending_events.push_back(BrokerEvent::Cancelled {
            client_order_id: client_order_id.to_owned(),
            reason: "IBKR_PAPER_CANCELLED".to_owned(),
        });
        Ok(())
    }

    fn replace(&mut self, request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        request.validate()?;
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

    fn poll(&mut self) -> Result<Vec<BrokerEvent>, PaperError> {
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

    fn reconnect(&mut self) -> Result<(), PaperError> {
        self.connected = true;
        Ok(())
    }
}

/// One broker operation that can receive an injected reliability fault.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BrokerOperation {
    /// Submission of a new order.
    Submit,
    /// Cancellation of an existing order.
    Cancel,
    /// Replacement of a working limit order.
    Replace,
    /// Polling asynchronous broker evidence.
    Poll,
    /// Reconnection attempt.
    Reconnect,
}

/// Deterministic fault modes exercised before real paper promotion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerFault {
    /// No broker call is made; the operation is disconnected before a known outcome.
    Disconnect,
    /// The inner broker accepts a submit, but the caller receives an ambiguous failure.
    AmbiguousAfterSubmit,
    /// Repeats the first broker event returned by a poll operation.
    DuplicateFirstEvent,
}

/// Fault-injection wrapper for any paper broker adapter.
pub struct FaultInjectingBroker<B> {
    inner: B,
    faults: BTreeMap<BrokerOperation, VecDeque<BrokerFault>>,
}

impl<B> FaultInjectingBroker<B> {
    /// Wraps a concrete paper adapter without changing its normal behavior.
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            faults: BTreeMap::new(),
        }
    }

    /// Schedules one deterministic fault for a future operation.
    pub fn inject(&mut self, operation: BrokerOperation, fault: BrokerFault) {
        self.faults.entry(operation).or_default().push_back(fault);
    }

    /// Returns the underlying adapter after a test scenario completes.
    pub fn into_inner(self) -> B {
        self.inner
    }

    fn next_fault(&mut self, operation: BrokerOperation) -> Option<BrokerFault> {
        self.faults
            .get_mut(&operation)
            .and_then(VecDeque::pop_front)
    }
}

impl<B: PaperBrokerAdapter> PaperBrokerAdapter for FaultInjectingBroker<B> {
    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        match self.next_fault(BrokerOperation::Submit) {
            Some(BrokerFault::Disconnect) => Err(PaperError(
                "fault injection disconnected before paper submission".to_owned(),
            )),
            Some(BrokerFault::AmbiguousAfterSubmit) => {
                let _ = self.inner.submit(request)?;
                Err(PaperError(
                    "fault injection made paper submission outcome ambiguous".to_owned(),
                ))
            }
            Some(BrokerFault::DuplicateFirstEvent) | None => self.inner.submit(request),
        }
    }

    fn cancel(&mut self, client_order_id: &str) -> Result<(), PaperError> {
        match self.next_fault(BrokerOperation::Cancel) {
            Some(BrokerFault::Disconnect) | Some(BrokerFault::AmbiguousAfterSubmit) => Err(
                PaperError("fault injection made paper cancellation outcome ambiguous".to_owned()),
            ),
            Some(BrokerFault::DuplicateFirstEvent) | None => self.inner.cancel(client_order_id),
        }
    }

    fn replace(&mut self, request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        match self.next_fault(BrokerOperation::Replace) {
            Some(BrokerFault::Disconnect) | Some(BrokerFault::AmbiguousAfterSubmit) => Err(
                PaperError("fault injection made paper replacement outcome ambiguous".to_owned()),
            ),
            Some(BrokerFault::DuplicateFirstEvent) | None => self.inner.replace(request),
        }
    }

    fn poll(&mut self) -> Result<Vec<BrokerEvent>, PaperError> {
        match self.next_fault(BrokerOperation::Poll) {
            Some(BrokerFault::Disconnect) | Some(BrokerFault::AmbiguousAfterSubmit) => Err(
                PaperError("fault injection disconnected paper polling".to_owned()),
            ),
            Some(BrokerFault::DuplicateFirstEvent) => {
                let mut events = self.inner.poll()?;
                if let Some(first) = events.first().cloned() {
                    events.push(first);
                }
                Ok(events)
            }
            None => self.inner.poll(),
        }
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        self.inner.snapshot(account_id)
    }

    fn reconnect(&mut self) -> Result<(), PaperError> {
        match self.next_fault(BrokerOperation::Reconnect) {
            Some(_) => Err(PaperError(
                "fault injection rejected paper reconnect".to_owned(),
            )),
            None => self.inner.reconnect(),
        }
    }
}

/// Scope at which a kill switch independently blocks new paper orders.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum KillSwitchScope {
    /// Blocks all paper accounts and strategies.
    Global,
    /// Blocks one account.
    Account(String),
    /// Blocks one strategy.
    Strategy(String),
    /// Blocks one instrument.
    Instrument(String),
}

impl KillSwitchScope {
    /// Stable display and dashboard identity for this operational control.
    pub fn as_key(&self) -> String {
        match self {
            Self::Global => "global".to_owned(),
            Self::Account(account_id) => format!("account:{account_id}"),
            Self::Strategy(strategy_id) => format!("strategy:{strategy_id}"),
            Self::Instrument(instrument_id) => format!("instrument:{instrument_id}"),
        }
    }

    fn validate(&self) -> Result<(), PaperError> {
        match self {
            Self::Global => Ok(()),
            Self::Account(value) => {
                validate_canonical_id("kill-switch account", value).map_err(Into::into)
            }
            Self::Strategy(value) => {
                validate_canonical_id("kill-switch strategy", value).map_err(Into::into)
            }
            Self::Instrument(value) => {
                validate_canonical_id("kill-switch instrument", value).map_err(Into::into)
            }
        }
    }
}

/// Versioned independently-operable paper kill-switch registry.
#[derive(Clone, Debug)]
pub struct KillSwitchRegistry {
    /// Immutable registry/policy revision used in operations evidence.
    pub version: String,
    active: BTreeSet<KillSwitchScope>,
}

impl KillSwitchRegistry {
    /// Creates an initially clear registry with an immutable version identity.
    pub fn new(version: impl Into<String>) -> Result<Self, PaperError> {
        let registry = Self {
            version: version.into(),
            active: BTreeSet::new(),
        };
        if registry.version.is_empty() {
            return Err(PaperError("kill-switch version is required".to_owned()));
        }
        Ok(registry)
    }

    /// Activates a kill switch independently of strategy or broker health.
    pub fn activate(&mut self, scope: KillSwitchScope) -> Result<bool, PaperError> {
        scope.validate()?;
        Ok(self.active.insert(scope))
    }

    /// Deactivates a kill switch explicitly; it never changes historical evidence.
    pub fn deactivate(&mut self, scope: &KillSwitchScope) -> bool {
        self.active.remove(scope)
    }

    /// Lists active scopes in deterministic operational-display order.
    pub fn active_keys(&self) -> Vec<String> {
        self.active.iter().map(KillSwitchScope::as_key).collect()
    }

    fn rejection_reasons(&self, intent: &OrderIntent) -> Vec<String> {
        let scopes = [
            KillSwitchScope::Global,
            KillSwitchScope::Account(intent.account_id.clone()),
            KillSwitchScope::Strategy(intent.strategy_id.clone()),
            KillSwitchScope::Instrument(intent.instrument_id.clone()),
        ];
        scopes
            .iter()
            .filter(|scope| self.active.contains(*scope))
            .map(|scope| {
                format!(
                    "KILL_SWITCH_{}",
                    scope.as_key().to_ascii_uppercase().replace(':', "_")
                )
            })
            .collect()
    }
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
        {
            return Err(PaperError("invalid paper risk policy".to_owned()));
        }
        Ok(())
    }
}

/// Exact market observation required at the paper pre-trade boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperMarketData {
    /// Canonical instrument identity for this mark.
    pub instrument_id: String,
    /// Exact positive mark used to estimate notional and reserve cash.
    pub mark_price: Decimal,
    /// Canonical UTC observation time supplied by the market-data boundary.
    pub observed_at: String,
}

impl PaperMarketData {
    fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("paper market instrument_id", &self.instrument_id)?;
        validate_utc_timestamp("paper market observation time", &self.observed_at)?;
        if self.mark_price <= Decimal::ZERO {
            return Err(PaperError(
                "paper market mark price must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
struct PaperRiskContext {
    open_orders: usize,
    position_quantity: Decimal,
    available_cash: Decimal,
    realized_pnl: Decimal,
}

/// Internal paper OMS record with exact independently-accounted fill quantity.
#[derive(Clone, Debug)]
pub struct PaperOrder {
    /// Durable OMS order and legal lifecycle state.
    pub oms: OmsOrder,
    /// Broker order identity once known.
    pub broker_order_id: Option<String>,
    /// Every broker-native order identity issued for this immutable OMS order.
    pub broker_order_versions: Vec<String>,
    /// State to restore after a replacement acceptance or rejection.
    replace_return_state: Option<OrderState>,
    /// Exact fill quantity observed through normalized broker execution IDs.
    pub filled_quantity: Decimal,
    /// Exact fresh market observation used for approval and outstanding-cash reservation.
    pub market: PaperMarketData,
}

impl PaperOrder {
    fn is_terminal(&self) -> bool {
        matches!(
            self.oms.state,
            OrderState::RiskRejected
                | OrderState::Filled
                | OrderState::Cancelled
                | OrderState::Rejected
                | OrderState::Expired
        )
    }

    fn working(&self) -> bool {
        !self.is_terminal()
    }

    fn reserved_cash(&self) -> Result<Decimal, PaperError> {
        if !self.working() || self.oms.intent.side != Side::Buy {
            return Ok(Decimal::ZERO);
        }
        let remaining = self.oms.intent.quantity.checked_sub(self.filled_quantity)?;
        remaining
            .checked_mul(self.market.mark_price)
            .map_err(Into::into)
    }
}

/// Immutable record of the decision and exact market observation that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperRiskEvidence {
    /// The complete immutable intent evaluated by risk, including rejected intents.
    pub intent: OrderIntent,
    /// The versioned risk result.
    pub decision: RiskDecision,
    /// The exact validated price observation evaluated by risk.
    pub market: PaperMarketData,
}

/// An authoritative exchange session that may count toward the paper gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperTradingSession {
    /// Immutable calendar identity that produced the session.
    pub calendar_id: String,
    /// Explicit regular exchange session including the exchange-local date.
    pub session: TradingSession,
}

impl PaperTradingSession {
    fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("paper calendar_id", &self.calendar_id)?;
        self.session.validate()?;
        validate_exchange_date(&self.session.exchange_date)?;
        Ok(())
    }
}

/// Result of a paper OMS submission attempt, including mandatory risk evidence.
#[derive(Clone, Debug)]
pub struct PaperSubmitOutcome {
    /// Risk decision emitted whether or not an executable order exists.
    pub decision: RiskDecision,
    /// OMS idempotency identity when the decision was approved.
    pub order_id: Option<String>,
    /// Resulting OMS lifecycle state when an order exists.
    pub state: Option<OrderState>,
}

/// One immutable reconciliation issue that must be explained rather than overwritten.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationIssue {
    /// Stable incident identity.
    pub incident_id: String,
    /// Machine-readable category.
    pub category: String,
    /// Canonical order/instrument/account subject.
    pub subject: String,
    /// Deterministic observed internal/broker comparison.
    pub detail: String,
}

/// Result of comparing independent internal and broker state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationReport {
    /// Stable reconciliation operation identity.
    pub reconciliation_id: String,
    /// Canonical UTC time of the broker snapshot.
    pub reconciled_at: String,
    /// All differences, including already-known unresolved incidents.
    pub issues: Vec<ReconciliationIssue>,
}

impl ReconciliationReport {
    /// Whether internal and broker state agreed completely at this checkpoint.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// An operational incident remains unresolved until an explicit attributable explanation exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationIncident {
    /// The reconciliation issue identity.
    pub issue: ReconciliationIssue,
    /// Operator-supplied explanation, if any.
    pub explanation: Option<String>,
}

impl ReconciliationIncident {
    /// Whether this difference is still unexplained and blocks paper promotion.
    pub fn unexplained(&self) -> bool {
        self.explanation.is_none()
    }
}

/// Measured promotion state for the 30-paper-trading-day acceptance gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperPromotionStatus {
    /// Number of distinct clean reconciled paper dates.
    pub clean_paper_days: u32,
    /// Required count, always thirty for this gate.
    pub required_paper_days: u32,
    /// Count of differences that still lack an explanation.
    pub unexplained_incidents: u32,
    /// Whether the complete gate history is protected by the durable audit chain.
    pub complete_auditability: bool,
    /// Whether the evidence gate is complete.
    pub eligible_for_next_gate: bool,
}

/// Read-only dashboard projection for paper operations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PaperDashboard {
    /// Schema version for the independent dashboard contract.
    pub dashboard_schema_version: u32,
    /// Always `PAPER` for this service.
    pub environment: String,
    /// Canonical account identifier.
    pub account_id: String,
    /// SHA-256 fingerprint of the immutable operational configuration.
    pub configuration_fingerprint: String,
    /// Whether the broker session is currently considered usable.
    pub broker_connected: bool,
    /// Whether every required local journal write has completed successfully in this process.
    pub persistence_healthy: bool,
    /// Current durable audit record sequence, or zero for a non-durable test service.
    pub audit_sequence: u64,
    /// SHA-256 audit-chain head, or the all-zero genesis hash before durable initialization.
    pub audit_head_hash: String,
    /// Exact internally-accounted cash rendered as a decimal string.
    pub internal_cash: String,
    /// Number of non-terminal orders.
    pub working_orders: u32,
    /// Number of explicitly ambiguous orders requiring reconciliation.
    pub unknown_orders: u32,
    /// Active independently-controlled kill switches.
    pub active_kill_switches: Vec<String>,
    /// Outstanding unexplained reconciliation incidents.
    pub unexplained_incidents: u32,
    /// UTC timestamp of the latest independent broker reconciliation, if one occurred.
    pub last_reconciled_at: Option<String>,
    /// Whether that latest reconciliation matched exactly.
    pub last_reconciliation_clean: Option<bool>,
    /// Distinct clean paper trading days observed by the gate tracker.
    pub clean_paper_days: u32,
    /// Required clean-day threshold.
    pub required_paper_days: u32,
    /// Whether the measured paper gate has completed.
    pub promotion_eligible: bool,
    /// Whether the gate has continuous durable audit evidence.
    pub complete_auditability: bool,
    /// Positions rendered deterministically by instrument.
    pub positions: Vec<PaperDashboardPosition>,
}

/// One read-only paper dashboard position row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PaperDashboardPosition {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Exact quantity string.
    pub quantity: String,
    /// Exact average cost string.
    pub average_cost: String,
    /// Exact realized P&L string.
    pub realized_pnl: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum PersistentJournalRecord {
    V3(PersistentJournalRecordV3),
    V2(PersistentJournalRecordV2),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistentJournalRecordV3 {
    schema_version: u32,
    sequence: u64,
    previous_hash: String,
    state: PersistentPaperState,
    entry_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistentJournalRecordV2 {
    schema_version: u32,
    sequence: u64,
    state: PersistentPaperState,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentPaperState {
    configuration_fingerprint: String,
    account_id: String,
    currency: String,
    cash: String,
    orders: BTreeMap<String, PersistentOrder>,
    risk_evidence: BTreeMap<String, PersistentRiskEvidence>,
    positions: BTreeMap<String, PersistentPosition>,
    execution_ids: Vec<String>,
    active_kill_switches: Vec<String>,
    incidents: BTreeMap<String, PersistentIncident>,
    last_reconciled_at: Option<String>,
    last_reconciliation_clean: Option<bool>,
    paper_days: BTreeMap<String, PersistentPaperDay>,
    next_reconciliation: u64,
    #[serde(default)]
    broker_connected: bool,
    #[serde(default)]
    latest_reconciliation: Option<PersistentReconciliationReport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentReconciliationReport {
    reconciliation_id: String,
    reconciled_at: String,
    issues: Vec<PersistentReconciliationIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentReconciliationIssue {
    incident_id: String,
    category: String,
    subject: String,
    detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentOrder {
    intent: PersistentIntent,
    state: String,
    broker_order_id: Option<String>,
    #[serde(default)]
    broker_order_versions: Vec<String>,
    #[serde(default)]
    replace_return_state: Option<String>,
    filled_quantity: String,
    market: PersistentMarketData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentMarketData {
    instrument_id: String,
    mark_price: String,
    observed_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentRiskEvidence {
    intent: PersistentIntent,
    approved: bool,
    reason_codes: Vec<String>,
    policy_version: String,
    decided_at: String,
    correlation_id: String,
    actor: String,
    evaluated_limits: String,
    market: PersistentMarketData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentIntent {
    intent_id: String,
    account_id: String,
    strategy_id: String,
    instrument_id: String,
    correlation_id: String,
    side: String,
    quantity: String,
    order_type: String,
    limit_price: Option<String>,
    time_in_force: String,
    rationale: String,
    created_at: String,
    strategy_version: String,
    configuration_version: String,
    environment: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentPosition {
    quantity: String,
    average_cost: String,
    realized_pnl: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentIncident {
    category: String,
    subject: String,
    detail: String,
    explanation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct PersistentPaperDay {
    calendar_id: String,
    session_opens_at: String,
    session_closes_at: String,
    clean: bool,
}

const PAPER_JOURNAL_SCHEMA_VERSION: u32 = 3;
const LEGACY_PAPER_JOURNAL_SCHEMA_VERSION: u32 = 2;
const MAX_PAPER_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;

/// Durable append-only state journal for one paper OMS account.
///
/// Every line is a canonical complete snapshot. Retaining full snapshots makes
/// restart recovery fail-closed and auditable without relying on a mutable
/// database row. A deployment may rotate this local adapter into versioned
/// object storage after preserving its immutable sequence.
pub struct FilePaperJournal {
    path: PathBuf,
    file: File,
    next_sequence: u64,
    previous_hash: String,
    latest: Option<PersistentPaperState>,
}

impl FilePaperJournal {
    /// Opens and verifies an existing journal before accepting new snapshots.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PaperError> {
        let path = path.as_ref().to_path_buf();
        if path.exists()
            && fs::symlink_metadata(&path)
                .map_err(|error| PaperError(error.to_string()))?
                .file_type()
                .is_symlink()
        {
            return Err(PaperError(
                "paper journal path must not be a symbolic link".to_owned(),
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| PaperError(error.to_string()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .map_err(|error| PaperError(error.to_string()))?;
        file.try_lock_exclusive().map_err(|error| {
            PaperError(format!(
                "paper journal is already open by another operator/process: {error}"
            ))
        })?;
        let mut latest = None;
        let mut next_sequence = 1;
        let mut previous_hash = "0".repeat(64);
        let mut modern_record_seen = false;
        let byte_count = file
            .metadata()
            .map_err(|error| PaperError(error.to_string()))?
            .len();
        if byte_count > MAX_PAPER_JOURNAL_BYTES {
            return Err(PaperError(format!(
                "paper journal exceeds the {} byte recovery limit; rotate and archive it before restart",
                MAX_PAPER_JOURNAL_BYTES
            )));
        }
        if byte_count > 0 {
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .map_err(|error| PaperError(error.to_string()))?;
            for (index, line) in contents.lines().enumerate() {
                if line.is_empty() {
                    return Err(PaperError(format!(
                        "paper journal contains an empty line at {}",
                        index + 1
                    )));
                }
                let record: PersistentJournalRecord =
                    serde_json::from_str(line).map_err(|error| {
                        PaperError(format!("invalid paper journal line {}: {error}", index + 1))
                    })?;
                if serde_json::to_string(&record).map_err(|error| PaperError(error.to_string()))?
                    != line
                {
                    return Err(PaperError(format!(
                        "paper journal line {} is not canonical JSON",
                        index + 1
                    )));
                }
                match record {
                    PersistentJournalRecord::V3(record) => {
                        if record.schema_version != PAPER_JOURNAL_SCHEMA_VERSION
                            || record.sequence != next_sequence
                            || record.previous_hash != previous_hash
                            || record.entry_hash != paper_record_hash(&record)?
                        {
                            return Err(PaperError(format!(
                                "paper journal integrity check failed at line {}",
                                index + 1
                            )));
                        }
                        modern_record_seen = true;
                        previous_hash = record.entry_hash;
                        latest = Some(record.state);
                    }
                    PersistentJournalRecord::V2(record) => {
                        if modern_record_seen
                            || record.schema_version != LEGACY_PAPER_JOURNAL_SCHEMA_VERSION
                            || record.sequence != next_sequence
                        {
                            return Err(PaperError(format!(
                                "paper journal legacy record is invalid at line {}",
                                index + 1
                            )));
                        }
                        previous_hash = legacy_paper_record_hash(&previous_hash, line);
                        latest = Some(record.state);
                    }
                }
                next_sequence += 1;
            }
        }
        Ok(Self {
            path,
            file,
            next_sequence,
            previous_hash,
            latest,
        })
    }

    /// Returns the journal location for deployment backup and restore controls.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn latest(&self) -> Option<&PersistentPaperState> {
        self.latest.as_ref()
    }

    fn sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    fn head_hash(&self) -> &str {
        &self.previous_hash
    }

    fn append(&mut self, state: PersistentPaperState) -> Result<(), PaperError> {
        let mut record = PersistentJournalRecordV3 {
            schema_version: PAPER_JOURNAL_SCHEMA_VERSION,
            sequence: self.next_sequence,
            previous_hash: self.previous_hash.clone(),
            state,
            entry_hash: String::new(),
        };
        record.entry_hash = paper_record_hash(&record)?;
        let serialized = serde_json::to_string(&PersistentJournalRecord::V3(record.clone()))
            .map_err(|error| PaperError(error.to_string()))?;
        self.file
            .write_all(serialized.as_bytes())
            .and_then(|_| self.file.write_all(b"\n"))
            .and_then(|_| self.file.sync_data())
            .map_err(|error| PaperError(error.to_string()))?;
        self.next_sequence += 1;
        self.previous_hash = record.entry_hash;
        self.latest = Some(record.state);
        Ok(())
    }
}

fn paper_record_hash(record: &PersistentJournalRecordV3) -> Result<String, PaperError> {
    let mut unsigned = record.clone();
    unsigned.entry_hash.clear();
    let canonical = serde_json::to_string(&PersistentJournalRecord::V3(unsigned))
        .map_err(|error| PaperError(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

fn legacy_paper_record_hash(previous_hash: &str, canonical_line: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"follon-paper-journal-v2-anchor");
    hasher.update(previous_hash.as_bytes());
    hasher.update((canonical_line.len() as u64).to_be_bytes());
    hasher.update(canonical_line.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Paper-only OMS service with independent accounting and mandatory reconciliation.
pub struct PaperTradingService<B> {
    account: PaperAccount,
    risk_policy: PaperRiskPolicy,
    kill_switches: KillSwitchRegistry,
    broker: B,
    broker_connected: bool,
    cash: Decimal,
    orders: BTreeMap<String, PaperOrder>,
    risk_evidence: BTreeMap<String, PaperRiskEvidence>,
    portfolios: BTreeMap<String, Portfolio>,
    execution_ids: BTreeSet<String>,
    incidents: BTreeMap<String, ReconciliationIncident>,
    last_reconciled_at: Option<String>,
    last_reconciliation_clean: Option<bool>,
    latest_reconciliation: Option<ReconciliationReport>,
    paper_days: BTreeMap<String, PersistentPaperDay>,
    next_reconciliation: u64,
    persistence_healthy: bool,
    journal: Option<FilePaperJournal>,
}

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    /// Creates a paper-only OMS service with explicit account, risk, and kill controls.
    pub fn new(
        account: PaperAccount,
        risk_policy: PaperRiskPolicy,
        kill_switches: KillSwitchRegistry,
        broker: B,
    ) -> Result<Self, PaperError> {
        account.validate()?;
        risk_policy.validate()?;
        Ok(Self {
            cash: account.initial_cash,
            account,
            risk_policy,
            kill_switches,
            broker,
            broker_connected: true,
            orders: BTreeMap::new(),
            risk_evidence: BTreeMap::new(),
            portfolios: BTreeMap::new(),
            execution_ids: BTreeSet::new(),
            incidents: BTreeMap::new(),
            last_reconciled_at: None,
            last_reconciliation_clean: None,
            latest_reconciliation: None,
            paper_days: BTreeMap::new(),
            next_reconciliation: 1,
            persistence_healthy: true,
            journal: None,
        })
    }

    /// Opens a paper service from a fully validated durable journal snapshot.
    pub fn open_durable(
        account: PaperAccount,
        risk_policy: PaperRiskPolicy,
        kill_switches: KillSwitchRegistry,
        broker: B,
        journal_path: impl AsRef<Path>,
    ) -> Result<Self, PaperError> {
        let journal = FilePaperJournal::open(journal_path)?;
        let latest = journal.latest().cloned();
        let mut service = Self::new(account, risk_policy, kill_switches, broker)?;
        if let Some(state) = latest {
            service.restore(state)?;
            // An external broker session never survives process recovery.
            service.broker_connected = false;
        }
        service.journal = Some(journal);
        if service
            .journal
            .as_ref()
            .is_some_and(|journal| journal.latest().is_none())
        {
            service.persist()?;
        }
        Ok(service)
    }

    /// Returns the independently controlled kill-switch registry.
    pub fn kill_switches(&self) -> &KillSwitchRegistry {
        &self.kill_switches
    }

    /// Returns the broker adapter only for controlled paper operational actions/tests.
    pub fn broker_mut(&mut self) -> &mut B {
        &mut self.broker
    }

    /// Returns an OMS order by its immutable client identity.
    pub fn order(&self, order_id: &str) -> Option<&PaperOrder> {
        self.orders.get(order_id)
    }

    /// Returns immutable risk and market evidence by its paper decision identity.
    pub fn risk_evidence(&self, decision_id: &str) -> Option<&PaperRiskEvidence> {
        self.risk_evidence.get(decision_id)
    }

    /// Activates a kill switch without involving strategy or broker processes.
    pub fn activate_kill_switch(&mut self, scope: KillSwitchScope) -> Result<bool, PaperError> {
        self.ensure_persistence_healthy()?;
        let changed = self.kill_switches.activate(scope)?;
        self.persist()?;
        Ok(changed)
    }

    /// Explicitly deactivates one kill switch. It does not mutate past decisions.
    pub fn deactivate_kill_switch(&mut self, scope: &KillSwitchScope) -> Result<bool, PaperError> {
        self.ensure_persistence_healthy()?;
        let changed = self.kill_switches.deactivate(scope);
        if changed {
            self.persist()?;
        }
        Ok(changed)
    }

    /// Applies risk, creates a client-idempotent order, then attempts paper submission.
    ///
    /// A transport error changes the newly-created order to `UNKNOWN` before
    /// returning an error. Call [`Self::reconnect_and_reconcile`] rather than
    /// blindly retrying an ambiguous submission.
    pub fn submit_intent(
        &mut self,
        intent: OrderIntent,
        market: PaperMarketData,
        decided_at: &str,
    ) -> Result<PaperSubmitOutcome, PaperError> {
        self.ensure_persistence_healthy()?;
        intent.validate()?;
        validate_utc_timestamp("paper risk decision time", decided_at)?;
        market.validate()?;
        if intent.environment != "PAPER" {
            return Err(PaperError(
                "paper OMS refuses an intent outside the PAPER environment".to_owned(),
            ));
        }
        if intent.account_id != self.account.account_id {
            return Err(PaperError(
                "paper intent account does not match service".to_owned(),
            ));
        }
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect and reconcile before submission"
                    .to_owned(),
            ));
        }
        if market.instrument_id != intent.instrument_id {
            return Err(PaperError(
                "paper market observation instrument does not match intent".to_owned(),
            ));
        }
        let observed_at = OffsetDateTime::parse(&market.observed_at, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let decision_time = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let age = (decision_time - observed_at).whole_seconds();
        if age < 0
            || u64::try_from(age).unwrap_or(u64::MAX) > self.risk_policy.max_market_data_age_seconds
        {
            return Err(PaperError(
                "paper market observation is stale or later than the risk decision".to_owned(),
            ));
        }
        let order_id = format!("order-{}", intent.intent_id);
        if let Some(existing) = self.orders.get(&order_id) {
            if existing.oms.intent != intent {
                return Err(PaperError(
                    "paper order idempotency key was reused with different intent data".to_owned(),
                ));
            }
            let evidence = self
                .risk_evidence
                .get(&format!("paper-risk-{}", intent.intent_id))
                .ok_or_else(|| {
                    PaperError("existing paper order is missing its risk evidence".to_owned())
                })?;
            if evidence.market != market || evidence.decision.decided_at != decided_at {
                return Err(PaperError(
                    "paper order retry must use the original market observation and decision time"
                        .to_owned(),
                ));
            }
            return Ok(PaperSubmitOutcome {
                decision: evidence.decision.clone(),
                order_id: Some(order_id),
                state: Some(existing.oms.state),
            });
        }

        let decision = self.evaluate_risk(&intent, &market, decided_at)?;
        let evidence = PaperRiskEvidence {
            intent: intent.clone(),
            decision: decision.clone(),
            market: market.clone(),
        };
        if !decision.approved {
            self.risk_evidence
                .insert(decision.decision_id.clone(), evidence);
            self.persist()?;
            return Ok(PaperSubmitOutcome {
                decision,
                order_id: None,
                state: None,
            });
        }
        let mut oms = OmsOrder::from_approved_intent(intent, &decision)?;
        oms.transition(OrderState::Approved, "PAPER_RISK_APPROVED")?;
        oms.transition(OrderState::PendingSubmit, "PAPER_SUBMISSION_REQUESTED")?;
        let request = BrokerOrderRequest::from_order(&oms);
        self.orders.insert(
            oms.order_id.clone(),
            PaperOrder {
                oms,
                broker_order_id: None,
                broker_order_versions: Vec::new(),
                replace_return_state: None,
                filled_quantity: Decimal::ZERO,
                market,
            },
        );
        self.risk_evidence
            .insert(decision.decision_id.clone(), evidence);
        // Persist `PENDING_SUBMIT` before crossing the external broker boundary.
        self.persist()?;

        let submission = self.broker.submit(&request);
        match submission {
            Ok(BrokerSubmitResult::Acknowledged { broker_order_id }) => {
                validate_canonical_id("broker order_id", &broker_order_id)?;
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "PAPER_SUBMISSION_SENT")?;
                order
                    .oms
                    .transition(OrderState::Acknowledged, "IBKR_PAPER_ACKNOWLEDGED")?;
                order.broker_order_id = Some(broker_order_id.clone());
                order.broker_order_versions.push(broker_order_id);
                let state = order.oms.state;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(state),
                })
            }
            Ok(BrokerSubmitResult::Rejected { reason }) => {
                validate_broker_reason("broker rejection reason", &reason)?;
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "PAPER_SUBMISSION_SENT")?;
                order.oms.transition(OrderState::Rejected, reason)?;
                let state = order.oms.state;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(state),
                })
            }
            Ok(BrokerSubmitResult::Unknown { reason }) => {
                validate_broker_reason("broker unknown-outcome reason", &reason)?;
                let order = self.order_mut(&request.client_order_id)?;
                order.oms.transition(OrderState::Unknown, reason)?;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(OrderState::Unknown),
                })
            }
            Err(error) => {
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Unknown, "PAPER_TRANSPORT_OUTCOME_UNKNOWN")?;
                self.broker_connected = false;
                self.persist()?;
                Err(error)
            }
        }
    }

    /// Requests cancellation. A transport failure leaves the order explicitly `UNKNOWN`.
    pub fn cancel_order(&mut self, order_id: &str) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
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
        if let Err(error) = self.broker.cancel(order_id) {
            self.order_mut(order_id)?
                .oms
                .transition(OrderState::Unknown, "PAPER_CANCEL_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist()?;
            return Err(error);
        }
        self.persist()?;
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

    /// Drains broker evidence, applies unique executions, and preserves every ambiguity.
    pub fn synchronize(&mut self) -> Result<usize, PaperError> {
        self.ensure_persistence_healthy()?;
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected".to_owned(),
            ));
        }
        let events = match self.broker.poll() {
            Ok(events) => events,
            Err(error) => {
                self.broker_connected = false;
                self.persist()?;
                return Err(error);
            }
        };
        let count = events.len();
        for event in events {
            self.apply_broker_event(event)?;
            // Broker evidence is append-only. Persist each arrival so resolving an
            // UNKNOWN or correcting an earlier terminal observation never rewrites
            // the durable history.
            self.persist()?;
        }
        Ok(count)
    }

    /// Reconnects, drains delayed broker evidence, then immediately reconciles.
    pub fn reconnect_and_reconcile(
        &mut self,
        reconciled_at: &str,
    ) -> Result<ReconciliationReport, PaperError> {
        self.ensure_persistence_healthy()?;
        if let Err(error) = self.broker.reconnect() {
            self.broker_connected = false;
            self.persist()?;
            return Err(error);
        }
        self.broker_connected = true;
        self.synchronize()?;
        self.reconcile(reconciled_at)
    }

    /// Compares the broker snapshot with independent OMS, position, and cash state.
    pub fn reconcile(&mut self, reconciled_at: &str) -> Result<ReconciliationReport, PaperError> {
        self.ensure_persistence_healthy()?;
        validate_utc_timestamp("paper reconciliation time", reconciled_at)?;
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect before reconciliation".to_owned(),
            ));
        }
        let snapshot = match self.broker.snapshot(&self.account.account_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.broker_connected = false;
                self.persist()?;
                return Err(error);
            }
        };
        validate_broker_snapshot(&snapshot)?;
        let reconciliation_id = format!("reconciliation-{:08}", self.next_reconciliation);
        self.next_reconciliation += 1;
        let mut raw_issues = Vec::new();
        let mut broker_orders: BTreeMap<&str, Vec<&BrokerOrderSnapshot>> = BTreeMap::new();
        for broker_order in &snapshot.orders {
            broker_orders
                .entry(broker_order.client_order_id.as_str())
                .or_default()
                .push(broker_order);
        }
        for (order_id, internal) in &self.orders {
            match broker_orders.get(order_id.as_str()) {
                None if internal.working() => raw_issues.push((
                    "MISSING_BROKER_ORDER",
                    order_id.clone(),
                    "internal working order is absent from broker snapshot".to_owned(),
                )),
                Some(brokers) => {
                    let broker = internal.broker_order_id.as_ref().and_then(|current| {
                        brokers
                            .iter()
                            .copied()
                            .find(|candidate| candidate.broker_order_id == *current)
                    });
                    if broker.is_none() {
                        raw_issues.push((
                            "BROKER_ORDER_ID_MISMATCH",
                            order_id.clone(),
                            format!(
                                "internal_current={:?},broker_versions={}",
                                internal.broker_order_id,
                                brokers
                                    .iter()
                                    .map(|candidate| candidate.broker_order_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                        ));
                    }
                    if brokers.iter().any(|candidate| {
                        !internal
                            .broker_order_versions
                            .contains(&candidate.broker_order_id)
                    }) {
                        raw_issues.push((
                            "BROKER_ORDER_VERSION_MISMATCH",
                            order_id.clone(),
                            "broker snapshot includes an unrecognized broker order version"
                                .to_owned(),
                        ));
                    }
                    let broker_filled =
                        brokers.iter().try_fold(Decimal::ZERO, |total, candidate| {
                            total
                                .checked_add(candidate.filled_quantity)
                                .map_err(PaperError::from)
                        })?;
                    if internal.filled_quantity != broker_filled {
                        raw_issues.push((
                            "FILLED_QUANTITY_MISMATCH",
                            order_id.clone(),
                            format!(
                                "internal={},broker={broker_filled}",
                                internal.filled_quantity,
                            ),
                        ));
                    }
                    if let Some(broker) = broker {
                        if internal.oms.state != broker.state {
                            raw_issues.push((
                                "ORDER_STATE_MISMATCH",
                                order_id.clone(),
                                format!(
                                    "internal={},broker={}",
                                    internal.oms.state.as_str(),
                                    broker.state.as_str()
                                ),
                            ));
                        }
                    }
                }
                None => {}
            }
        }
        for broker in &snapshot.orders {
            if !self.orders.contains_key(&broker.client_order_id) {
                raw_issues.push((
                    "UNEXPECTED_BROKER_ORDER",
                    broker.client_order_id.clone(),
                    "broker snapshot contains no matching internal order".to_owned(),
                ));
            }
        }

        let broker_positions: BTreeMap<_, _> = snapshot
            .positions
            .iter()
            .map(|position| (position.instrument_id.as_str(), position.quantity))
            .collect();
        let instrument_ids: BTreeSet<_> = self
            .portfolios
            .keys()
            .map(String::as_str)
            .chain(broker_positions.keys().copied())
            .collect();
        for instrument_id in instrument_ids {
            let internal = self
                .portfolios
                .get(instrument_id)
                .map(|portfolio| portfolio.position_snapshot().quantity)
                .unwrap_or(Decimal::ZERO);
            let broker = broker_positions
                .get(instrument_id)
                .copied()
                .unwrap_or(Decimal::ZERO);
            if internal != broker {
                raw_issues.push((
                    "POSITION_QUANTITY_MISMATCH",
                    instrument_id.to_owned(),
                    format!("internal={internal},broker={broker}"),
                ));
            }
        }
        if self.cash != snapshot.cash {
            raw_issues.push((
                "CASH_MISMATCH",
                self.account.account_id.clone(),
                format!("internal={},broker={}", self.cash, snapshot.cash),
            ));
        }

        let issues = raw_issues
            .into_iter()
            .enumerate()
            .map(|(index, (category, subject, detail))| {
                let existing = self
                    .incidents
                    .values()
                    .find(|incident| {
                        incident.issue.category == category
                            && incident.issue.subject == subject
                            && incident.unexplained()
                    })
                    .map(|incident| incident.issue.incident_id.clone());
                let incident_id = existing.unwrap_or_else(|| {
                    format!(
                        "incident-{}-{:03}",
                        reconciliation_id.trim_start_matches("reconciliation-"),
                        index + 1
                    )
                });
                let issue = ReconciliationIssue {
                    incident_id: incident_id.clone(),
                    category: category.to_owned(),
                    subject,
                    detail,
                };
                self.incidents
                    .entry(incident_id)
                    .or_insert_with(|| ReconciliationIncident {
                        issue: issue.clone(),
                        explanation: None,
                    });
                issue
            })
            .collect();
        let report = ReconciliationReport {
            reconciliation_id,
            reconciled_at: reconciled_at.to_owned(),
            issues,
        };
        self.last_reconciled_at = Some(reconciled_at.to_owned());
        self.last_reconciliation_clean = Some(report.is_clean());
        self.latest_reconciliation = Some(report.clone());
        self.persist()?;
        Ok(report)
    }

    /// Records an attributable explanation for one reconciliation incident.
    pub fn explain_incident(
        &mut self,
        incident_id: &str,
        explanation: impl Into<String>,
    ) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        validate_canonical_id("incident_id", incident_id)?;
        let explanation = explanation.into();
        if explanation.trim().is_empty() || explanation.len() > 1_024 {
            return Err(PaperError(
                "incident explanation must contain 1 to 1024 characters".to_owned(),
            ));
        }
        let incident = self
            .incidents
            .get_mut(incident_id)
            .ok_or_else(|| PaperError("unknown reconciliation incident".to_owned()))?;
        incident.explanation = Some(explanation);
        self.persist()?;
        Ok(())
    }

    /// Records one closed exchange session for the measured 30-paper-day gate.
    ///
    /// The caller must supply the exact session selected from its versioned exchange
    /// calendar. A checkpoint taken before the session closes cannot count.
    pub fn record_paper_session(
        &mut self,
        paper_session: &PaperTradingSession,
        report: &ReconciliationReport,
        calendar: &dyn TradingCalendar,
    ) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        paper_session.validate()?;
        if calendar.calendar_id() != self.risk_policy.trading_calendar_id
            || calendar.calendar_id() != paper_session.calendar_id
            || calendar.session_for_exchange_date(&paper_session.session.exchange_date)
                != Some(&paper_session.session)
        {
            return Err(PaperError(
                "paper-day gate requires the exact session from the configured calendar".to_owned(),
            ));
        }
        if paper_session.calendar_id != self.risk_policy.trading_calendar_id {
            return Err(PaperError(
                "paper-session calendar does not match the configured paper calendar".to_owned(),
            ));
        }
        let expected_reconciliation_id = format!(
            "reconciliation-{:08}",
            self.next_reconciliation.checked_sub(1).ok_or_else(|| {
                PaperError("paper-session gate has no completed reconciliation".to_owned())
            })?
        );
        if self.last_reconciled_at.as_deref() != Some(report.reconciled_at.as_str())
            || self.last_reconciliation_clean != Some(report.is_clean())
            || self.latest_reconciliation.as_ref() != Some(report)
            || report.reconciliation_id != expected_reconciliation_id
            || !report.is_clean()
        {
            return Err(PaperError(
                "paper-session gate requires the latest actual reconciliation report".to_owned(),
            ));
        }
        if report.reconciled_at < paper_session.session.closes_at {
            return Err(PaperError(
                "paper-session reconciliation must occur at or after the session close".to_owned(),
            ));
        }
        if self.unexplained_incident_count() != 0 {
            return Err(PaperError(
                "paper-session gate refuses unresolved reconciliation incidents".to_owned(),
            ));
        }
        let exchange_date = &paper_session.session.exchange_date;
        let day = PersistentPaperDay {
            calendar_id: paper_session.calendar_id.clone(),
            session_opens_at: paper_session.session.opens_at.clone(),
            session_closes_at: paper_session.session.closes_at.clone(),
            clean: true,
        };
        match self.paper_days.get(exchange_date) {
            Some(existing) if *existing == day => Ok(()),
            Some(_) => Err(PaperError(
                "paper-session gate record cannot be overwritten with conflicting evidence"
                    .to_owned(),
            )),
            None => {
                self.paper_days.insert(exchange_date.to_owned(), day);
                self.persist()?;
                Ok(())
            }
        }
    }

    /// Returns the measured 30-day promotion state, never a predictive estimate.
    pub fn promotion_status(&self) -> PaperPromotionStatus {
        let clean_paper_days = self.paper_days.values().filter(|day| day.clean).count() as u32;
        let unexplained_incidents = self.unexplained_incident_count();
        let complete_auditability = self.persistence_healthy
            && self
                .journal
                .as_ref()
                .is_some_and(|journal| journal.sequence() > 0);
        PaperPromotionStatus {
            clean_paper_days,
            required_paper_days: 30,
            unexplained_incidents,
            complete_auditability,
            eligible_for_next_gate: clean_paper_days >= 30
                && unexplained_incidents == 0
                && complete_auditability,
        }
    }

    /// Creates a deterministic read-only dashboard projection for paper operations.
    pub fn dashboard(&self) -> PaperDashboard {
        let status = self.promotion_status();
        let positions = self
            .portfolios
            .iter()
            .map(|(instrument_id, portfolio)| {
                let position = portfolio.position_snapshot();
                PaperDashboardPosition {
                    instrument_id: instrument_id.clone(),
                    quantity: position.quantity.to_string(),
                    average_cost: position.average_cost.to_string(),
                    realized_pnl: position.realized_pnl.to_string(),
                }
            })
            .collect();
        PaperDashboard {
            dashboard_schema_version: 2,
            environment: self.account.environment.clone(),
            account_id: self.account.account_id.clone(),
            configuration_fingerprint: self.configuration_fingerprint(),
            broker_connected: self.broker_connected,
            persistence_healthy: self.persistence_healthy,
            audit_sequence: self
                .journal
                .as_ref()
                .map(FilePaperJournal::sequence)
                .unwrap_or(0),
            audit_head_hash: self
                .journal
                .as_ref()
                .map(|journal| journal.head_hash().to_owned())
                .unwrap_or_else(|| "0".repeat(64)),
            internal_cash: self.cash.to_string(),
            working_orders: self.orders.values().filter(|order| order.working()).count() as u32,
            unknown_orders: self
                .orders
                .values()
                .filter(|order| order.oms.state == OrderState::Unknown)
                .count() as u32,
            active_kill_switches: self.kill_switches.active_keys(),
            unexplained_incidents: status.unexplained_incidents,
            last_reconciled_at: self.last_reconciled_at.clone(),
            last_reconciliation_clean: self.last_reconciliation_clean,
            clean_paper_days: status.clean_paper_days,
            required_paper_days: status.required_paper_days,
            promotion_eligible: status.eligible_for_next_gate,
            complete_auditability: status.complete_auditability,
            positions,
        }
    }

    /// Returns canonical JSON for a server-owned read-only dashboard stream.
    pub fn canonical_dashboard_json(&self) -> Result<String, PaperError> {
        serde_json::to_string(&self.dashboard()).map_err(|error| PaperError(error.to_string()))
    }

    fn order_mut(&mut self, order_id: &str) -> Result<&mut PaperOrder, PaperError> {
        self.orders
            .get_mut(order_id)
            .ok_or_else(|| PaperError("paper OMS does not know order".to_owned()))
    }

    fn evaluate_risk(
        &self,
        intent: &OrderIntent,
        market: &PaperMarketData,
        decided_at: &str,
    ) -> Result<RiskDecision, PaperError> {
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
        let reserved_cash = self.orders.values().try_fold(
            Decimal::ZERO,
            |total, order| -> Result<Decimal, PaperError> {
                total
                    .checked_add(order.reserved_cash()?)
                    .map_err(Into::into)
            },
        )?;
        let context = PaperRiskContext {
            open_orders: self.orders.values().filter(|order| order.working()).count(),
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
        if self
            .orders
            .values()
            .any(|order| order.oms.state == OrderState::Unknown)
        {
            reasons.push("UNKNOWN_ORDER_REQUIRES_RECONCILIATION".to_owned());
        }
        if self.unexplained_incident_count() > 0 {
            reasons.push("UNEXPLAINED_INCIDENTS_REQUIRE_REVIEW".to_owned());
        }
        if context.open_orders >= self.risk_policy.max_open_orders {
            reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
        }
        if projected_position > self.risk_policy.max_position_quantity
            || projected_position < Decimal::ZERO
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
                "max_order_quantity={},max_order_notional={},max_price_deviation_bps={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},market_instrument_id={},market_mark_price={},market_observed_at={},requested_price={},requested_price_deviation_bps={},estimated_notional={},projected_position={},available_cash={}",
                self.risk_policy.max_order_quantity,
                self.risk_policy.max_order_notional,
                self.risk_policy.max_price_deviation_bps,
                self.risk_policy.max_open_orders,
                self.risk_policy.max_position_quantity,
                self.risk_policy.max_realized_loss,
                self.risk_policy.max_market_data_age_seconds,
                market.instrument_id,
                market.mark_price,
                market.observed_at,
                intent.limit_price.map_or_else(|| "MARKET".to_owned(), |price| price.to_string()),
                requested_price_deviation_bps,
                estimated_notional,
                projected_position,
                context.available_cash,
            ),
        })
    }

    fn apply_broker_event(&mut self, event: BrokerEvent) -> Result<(), PaperError> {
        match event {
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
                let (instrument_id, side, order_id) = {
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
                let portfolio = self.portfolios.entry(instrument_id).or_insert_with(|| {
                    Portfolio::new(&self.account.account_id, &fill.instrument_id)
                });
                portfolio.apply_fill(&fill)?;
                let gross = fill.price.checked_mul(fill.quantity)?;
                self.cash = match fill.side {
                    Side::Buy => self.cash.checked_sub(gross.checked_add(fill.fee)?)?,
                    Side::Sell => self.cash.checked_add(gross.checked_sub(fill.fee)?)?,
                };
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

    fn persist(&mut self) -> Result<(), PaperError> {
        let state = self.persistent_state();
        if let Some(journal) = &mut self.journal {
            if let Err(error) = journal.append(state) {
                self.persistence_healthy = false;
                return Err(error);
            }
        }
        Ok(())
    }

    fn ensure_persistence_healthy(&self) -> Result<(), PaperError> {
        if self.persistence_healthy {
            Ok(())
        } else {
            Err(PaperError(
                "paper OMS is fail-closed after a durable journal write failure".to_owned(),
            ))
        }
    }

    fn persistent_state(&self) -> PersistentPaperState {
        let orders = self
            .orders
            .iter()
            .map(|(order_id, order)| {
                (
                    order_id.clone(),
                    PersistentOrder {
                        intent: PersistentIntent::from(&order.oms.intent),
                        state: order.oms.state.as_str().to_owned(),
                        broker_order_id: order.broker_order_id.clone(),
                        broker_order_versions: order.broker_order_versions.clone(),
                        replace_return_state: order
                            .replace_return_state
                            .map(OrderState::as_str)
                            .map(str::to_owned),
                        filled_quantity: order.filled_quantity.to_string(),
                        market: PersistentMarketData::from(&order.market),
                    },
                )
            })
            .collect();
        let risk_evidence = self
            .risk_evidence
            .iter()
            .map(|(decision_id, evidence)| {
                (decision_id.clone(), PersistentRiskEvidence::from(evidence))
            })
            .collect();
        let positions = self
            .portfolios
            .iter()
            .map(|(instrument_id, portfolio)| {
                let position = portfolio.position_snapshot();
                (
                    instrument_id.clone(),
                    PersistentPosition {
                        quantity: position.quantity.to_string(),
                        average_cost: position.average_cost.to_string(),
                        realized_pnl: position.realized_pnl.to_string(),
                    },
                )
            })
            .collect();
        let incidents = self
            .incidents
            .iter()
            .map(|(incident_id, incident)| {
                (
                    incident_id.clone(),
                    PersistentIncident {
                        category: incident.issue.category.clone(),
                        subject: incident.issue.subject.clone(),
                        detail: incident.issue.detail.clone(),
                        explanation: incident.explanation.clone(),
                    },
                )
            })
            .collect();
        PersistentPaperState {
            configuration_fingerprint: self.configuration_fingerprint(),
            account_id: self.account.account_id.clone(),
            currency: self.account.currency.clone(),
            cash: self.cash.to_string(),
            orders,
            risk_evidence,
            positions,
            execution_ids: self.execution_ids.iter().cloned().collect(),
            active_kill_switches: self.kill_switches.active_keys(),
            incidents,
            last_reconciled_at: self.last_reconciled_at.clone(),
            last_reconciliation_clean: self.last_reconciliation_clean,
            paper_days: self.paper_days.clone(),
            next_reconciliation: self.next_reconciliation,
            broker_connected: self.broker_connected,
            latest_reconciliation: self
                .latest_reconciliation
                .as_ref()
                .map(PersistentReconciliationReport::from),
        }
    }

    fn restore(&mut self, state: PersistentPaperState) -> Result<(), PaperError> {
        if state.account_id != self.account.account_id || state.currency != self.account.currency {
            return Err(PaperError(
                "paper journal account or currency does not match supplied configuration"
                    .to_owned(),
            ));
        }
        if state.configuration_fingerprint != self.configuration_fingerprint() {
            return Err(PaperError(
                "paper journal configuration fingerprint does not match supplied configuration"
                    .to_owned(),
            ));
        }
        self.cash = decimal("journal cash", &state.cash)?;
        let mut orders = BTreeMap::new();
        for (order_id, persisted) in state.orders {
            let intent = OrderIntent::try_from(persisted.intent)?;
            if intent.account_id != self.account.account_id || intent.environment != "PAPER" {
                return Err(PaperError(
                    "persisted paper order has an incompatible account or environment".to_owned(),
                ));
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                validate_canonical_id("persisted broker_order_id", broker_order_id)?;
            }
            let mut broker_order_versions = persisted.broker_order_versions;
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if broker_order_versions.is_empty() {
                    broker_order_versions.push(broker_order_id.clone());
                }
            }
            let mut unique_versions = BTreeSet::new();
            for broker_order_id in &broker_order_versions {
                validate_canonical_id("persisted broker_order_version", broker_order_id)?;
                if !unique_versions.insert(broker_order_id) {
                    return Err(PaperError(
                        "persisted broker order versions are duplicated".to_owned(),
                    ));
                }
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if !unique_versions.contains(broker_order_id) {
                    return Err(PaperError(
                        "persisted active broker ID is absent from versions".to_owned(),
                    ));
                }
            }
            let filled_quantity = decimal("persisted filled quantity", &persisted.filled_quantity)?;
            if filled_quantity < Decimal::ZERO || filled_quantity > intent.quantity {
                return Err(PaperError(
                    "persisted filled quantity is invalid".to_owned(),
                ));
            }
            let market = PaperMarketData::try_from(persisted.market)?;
            if market.instrument_id != intent.instrument_id {
                return Err(PaperError(
                    "persisted paper market observation does not match intent instrument"
                        .to_owned(),
                ));
            }
            let oms = OmsOrder::recover(
                order_id.clone(),
                intent,
                parse_order_state(&persisted.state)?,
            )?;
            let replace_return_state = persisted
                .replace_return_state
                .as_deref()
                .map(parse_order_state)
                .transpose()?;
            if oms.state == OrderState::PendingReplace && replace_return_state.is_none() {
                return Err(PaperError(
                    "persisted pending replacement has no return state".to_owned(),
                ));
            }
            orders.insert(
                order_id,
                PaperOrder {
                    oms,
                    broker_order_id: persisted.broker_order_id,
                    broker_order_versions,
                    replace_return_state,
                    filled_quantity,
                    market,
                },
            );
        }
        let mut risk_evidence = BTreeMap::new();
        for (decision_id, persisted) in state.risk_evidence {
            validate_canonical_id("persisted risk decision_id", &decision_id)?;
            let evidence = PaperRiskEvidence::try_from(persisted)?;
            if decision_id != evidence.decision.decision_id {
                return Err(PaperError(
                    "persisted risk decision identity does not match its key".to_owned(),
                ));
            }
            if evidence.decision.policy_version != self.risk_policy.version
                || evidence.decision.actor != "paper_risk_engine"
            {
                return Err(PaperError(
                    "persisted risk evidence is incompatible with supplied policy".to_owned(),
                ));
            }
            if risk_evidence.contains_key(&decision_id) {
                return Err(PaperError("duplicate persisted risk evidence".to_owned()));
            }
            risk_evidence.insert(decision_id, evidence);
        }
        for order in orders.values() {
            let decision_id = format!("paper-risk-{}", order.oms.intent.intent_id);
            let evidence = risk_evidence.get(&decision_id).ok_or_else(|| {
                PaperError("persisted paper order is missing risk evidence".to_owned())
            })?;
            if !evidence.decision.approved
                || evidence.intent != order.oms.intent
                || evidence.market != order.market
            {
                return Err(PaperError(
                    "persisted paper order does not match approved risk evidence".to_owned(),
                ));
            }
        }
        let mut portfolios = BTreeMap::new();
        for (instrument_id, persisted) in state.positions {
            portfolios.insert(
                instrument_id.clone(),
                Portfolio::recover(
                    &self.account.account_id,
                    instrument_id,
                    decimal("persisted position quantity", &persisted.quantity)?,
                    decimal("persisted average cost", &persisted.average_cost)?,
                    decimal("persisted realized pnl", &persisted.realized_pnl)?,
                )?,
            );
        }
        let mut execution_ids = BTreeSet::new();
        for execution_id in state.execution_ids {
            validate_canonical_id("persisted execution_id", &execution_id)?;
            if !execution_ids.insert(execution_id) {
                return Err(PaperError(
                    "paper journal contains duplicate execution identity".to_owned(),
                ));
            }
        }
        let mut restored_switches = KillSwitchRegistry::new(self.kill_switches.version.clone())?;
        for scope in state.active_kill_switches {
            restored_switches.activate(parse_kill_switch_scope(&scope)?)?;
        }
        let mut incidents = BTreeMap::new();
        for (incident_id, persisted) in state.incidents {
            validate_canonical_id("persisted incident_id", &incident_id)?;
            if persisted.category.is_empty()
                || persisted.subject.is_empty()
                || persisted.detail.is_empty()
            {
                return Err(PaperError(
                    "persisted reconciliation incident is invalid".to_owned(),
                ));
            }
            incidents.insert(
                incident_id.clone(),
                ReconciliationIncident {
                    issue: ReconciliationIssue {
                        incident_id,
                        category: persisted.category,
                        subject: persisted.subject,
                        detail: persisted.detail,
                    },
                    explanation: persisted.explanation,
                },
            );
        }
        for (date, paper_day) in &state.paper_days {
            validate_exchange_date(date)?;
            validate_canonical_id("persisted paper calendar_id", &paper_day.calendar_id)?;
            if paper_day.calendar_id != self.risk_policy.trading_calendar_id {
                return Err(PaperError(
                    "persisted paper-day calendar does not match supplied policy".to_owned(),
                ));
            }
            let session = TradingSession {
                exchange_date: date.clone(),
                opens_at: paper_day.session_opens_at.clone(),
                closes_at: paper_day.session_closes_at.clone(),
            };
            session.validate()?;
        }
        if let Some(last_reconciled_at) = &state.last_reconciled_at {
            validate_utc_timestamp("persisted last reconciliation time", last_reconciled_at)?;
            if state.last_reconciliation_clean.is_none() {
                return Err(PaperError(
                    "persisted last reconciliation has no cleanliness result".to_owned(),
                ));
            }
        } else if state.last_reconciliation_clean.is_some() {
            return Err(PaperError(
                "persisted reconciliation cleanliness has no timestamp".to_owned(),
            ));
        }
        if state.next_reconciliation == 0 {
            return Err(PaperError(
                "persisted reconciliation sequence is invalid".to_owned(),
            ));
        }
        let latest_reconciliation = state
            .latest_reconciliation
            .map(ReconciliationReport::try_from)
            .transpose()?;
        match (
            &latest_reconciliation,
            &state.last_reconciled_at,
            state.last_reconciliation_clean,
        ) {
            (Some(report), Some(reconciled_at), Some(clean))
                if report.reconciled_at == *reconciled_at && report.is_clean() == clean => {}
            (None, None, None) => {}
            // Legacy v2 journals did not retain the exact report. Recovery is allowed,
            // but that legacy checkpoint cannot be used to record a new gate day.
            (None, Some(_), Some(_)) => {}
            _ => {
                return Err(PaperError(
                    "persisted paper reconciliation evidence is inconsistent".to_owned(),
                ))
            }
        }
        if let Some(report) = &latest_reconciliation {
            let expected = format!(
                "reconciliation-{:08}",
                state.next_reconciliation.saturating_sub(1)
            );
            if report.reconciliation_id != expected {
                return Err(PaperError(
                    "persisted latest paper reconciliation identity is invalid".to_owned(),
                ));
            }
        }
        self.orders = orders;
        self.risk_evidence = risk_evidence;
        self.portfolios = portfolios;
        self.execution_ids = execution_ids;
        self.kill_switches = restored_switches;
        self.incidents = incidents;
        self.last_reconciled_at = state.last_reconciled_at;
        self.last_reconciliation_clean = state.last_reconciliation_clean;
        self.latest_reconciliation = latest_reconciliation;
        self.paper_days = state.paper_days;
        self.next_reconciliation = state.next_reconciliation;
        self.broker_connected = state.broker_connected;
        Ok(())
    }

    fn configuration_fingerprint(&self) -> String {
        let initial_cash = self.account.initial_cash.to_string();
        let max_order_quantity = self.risk_policy.max_order_quantity.to_string();
        let max_order_notional = self.risk_policy.max_order_notional.to_string();
        let max_price_deviation_bps = self.risk_policy.max_price_deviation_bps.to_string();
        let max_open_orders = self.risk_policy.max_open_orders.to_string();
        let max_position_quantity = self.risk_policy.max_position_quantity.to_string();
        let max_realized_loss = self.risk_policy.max_realized_loss.to_string();
        let max_market_data_age_seconds = self.risk_policy.max_market_data_age_seconds.to_string();
        hash_fingerprint_parts(&[
            "paper-configuration-v2",
            &self.account.account_id,
            &self.account.currency,
            &initial_cash,
            &self.account.environment,
            &self.risk_policy.version,
            &self.risk_policy.trading_calendar_id,
            &max_order_quantity,
            &max_order_notional,
            &max_price_deviation_bps,
            &max_open_orders,
            &max_position_quantity,
            &max_realized_loss,
            &max_market_data_age_seconds,
            &self.kill_switches.version,
        ])
    }

    fn unexplained_incident_count(&self) -> u32 {
        self.incidents
            .values()
            .filter(|incident| incident.unexplained())
            .count() as u32
    }
}

fn hash_fingerprint_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
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

fn validate_exchange_date(value: &str) -> Result<(), PaperError> {
    if value.len() != 10
        || !value.bytes().enumerate().all(|(index, character)| {
            matches!(index, 4 | 7) && character == b'-'
                || !matches!(index, 4 | 7) && character.is_ascii_digit()
        })
    {
        return Err(PaperError("exchange date must be YYYY-MM-DD".to_owned()));
    }
    let timestamp = format!("{value}T00:00:00Z");
    validate_utc_timestamp("exchange date", &timestamp)?;
    Ok(())
}

impl From<&OrderIntent> for PersistentIntent {
    fn from(intent: &OrderIntent) -> Self {
        Self {
            intent_id: intent.intent_id.clone(),
            account_id: intent.account_id.clone(),
            strategy_id: intent.strategy_id.clone(),
            instrument_id: intent.instrument_id.clone(),
            correlation_id: intent.correlation_id.clone(),
            side: intent.side.as_str().to_owned(),
            quantity: intent.quantity.to_string(),
            order_type: intent.order_type.as_str().to_owned(),
            limit_price: intent.limit_price.map(|price| price.to_string()),
            time_in_force: intent.time_in_force.as_str().to_owned(),
            rationale: intent.rationale.clone(),
            created_at: intent.created_at.clone(),
            strategy_version: intent.strategy_version.clone(),
            configuration_version: intent.configuration_version.clone(),
            environment: intent.environment.clone(),
        }
    }
}

impl From<&PaperMarketData> for PersistentMarketData {
    fn from(market: &PaperMarketData) -> Self {
        Self {
            instrument_id: market.instrument_id.clone(),
            mark_price: market.mark_price.to_string(),
            observed_at: market.observed_at.clone(),
        }
    }
}

impl TryFrom<PersistentMarketData> for PaperMarketData {
    type Error = PaperError;

    fn try_from(market: PersistentMarketData) -> Result<Self, Self::Error> {
        let market = Self {
            instrument_id: market.instrument_id,
            mark_price: decimal("persisted market mark price", &market.mark_price)?,
            observed_at: market.observed_at,
        };
        market.validate()?;
        Ok(market)
    }
}

impl From<&PaperRiskEvidence> for PersistentRiskEvidence {
    fn from(evidence: &PaperRiskEvidence) -> Self {
        Self {
            intent: PersistentIntent::from(&evidence.intent),
            approved: evidence.decision.approved,
            reason_codes: evidence.decision.reason_codes.clone(),
            policy_version: evidence.decision.policy_version.clone(),
            decided_at: evidence.decision.decided_at.clone(),
            correlation_id: evidence.decision.correlation_id.clone(),
            actor: evidence.decision.actor.clone(),
            evaluated_limits: evidence.decision.evaluated_limits.clone(),
            market: PersistentMarketData::from(&evidence.market),
        }
    }
}

impl TryFrom<PersistentRiskEvidence> for PaperRiskEvidence {
    type Error = PaperError;

    fn try_from(evidence: PersistentRiskEvidence) -> Result<Self, Self::Error> {
        let intent = OrderIntent::try_from(evidence.intent)?;
        validate_canonical_id("persisted risk correlation_id", &evidence.correlation_id)?;
        validate_utc_timestamp("persisted risk decided_at", &evidence.decided_at)?;
        if evidence.reason_codes.is_empty()
            || evidence.reason_codes.iter().any(|reason| reason.is_empty())
            || evidence.policy_version.is_empty()
            || evidence.actor.is_empty()
            || evidence.evaluated_limits.is_empty()
        {
            return Err(PaperError("persisted risk evidence is invalid".to_owned()));
        }
        if evidence.correlation_id != intent.correlation_id {
            return Err(PaperError(
                "persisted risk correlation does not match its intent".to_owned(),
            ));
        }
        let decision = RiskDecision {
            decision_id: format!("paper-risk-{}", intent.intent_id),
            intent_id: intent.intent_id.clone(),
            approved: evidence.approved,
            reason_codes: evidence.reason_codes,
            policy_version: evidence.policy_version,
            decided_at: evidence.decided_at,
            correlation_id: evidence.correlation_id,
            actor: evidence.actor,
            evaluated_limits: evidence.evaluated_limits,
        };
        Ok(Self {
            intent,
            decision,
            market: PaperMarketData::try_from(evidence.market)?,
        })
    }
}

impl From<&ReconciliationReport> for PersistentReconciliationReport {
    fn from(report: &ReconciliationReport) -> Self {
        Self {
            reconciliation_id: report.reconciliation_id.clone(),
            reconciled_at: report.reconciled_at.clone(),
            issues: report
                .issues
                .iter()
                .map(|issue| PersistentReconciliationIssue {
                    incident_id: issue.incident_id.clone(),
                    category: issue.category.clone(),
                    subject: issue.subject.clone(),
                    detail: issue.detail.clone(),
                })
                .collect(),
        }
    }
}

impl TryFrom<PersistentReconciliationReport> for ReconciliationReport {
    type Error = PaperError;

    fn try_from(value: PersistentReconciliationReport) -> Result<Self, Self::Error> {
        validate_canonical_id(
            "persisted paper reconciliation_id",
            &value.reconciliation_id,
        )?;
        validate_utc_timestamp("persisted paper reconciliation time", &value.reconciled_at)?;
        let mut incident_ids = BTreeSet::new();
        let mut issues = Vec::with_capacity(value.issues.len());
        for issue in value.issues {
            validate_canonical_id("persisted paper incident_id", &issue.incident_id)?;
            validate_broker_reason("persisted paper issue category", &issue.category)?;
            validate_broker_reason("persisted paper issue subject", &issue.subject)?;
            validate_broker_reason("persisted paper issue detail", &issue.detail)?;
            if !incident_ids.insert(issue.incident_id.clone()) {
                return Err(PaperError(
                    "persisted paper reconciliation repeats an incident ID".to_owned(),
                ));
            }
            issues.push(ReconciliationIssue {
                incident_id: issue.incident_id,
                category: issue.category,
                subject: issue.subject,
                detail: issue.detail,
            });
        }
        Ok(Self {
            reconciliation_id: value.reconciliation_id,
            reconciled_at: value.reconciled_at,
            issues,
        })
    }
}

impl TryFrom<PersistentIntent> for OrderIntent {
    type Error = PaperError;

    fn try_from(intent: PersistentIntent) -> Result<Self, Self::Error> {
        let result = Self {
            intent_id: intent.intent_id,
            account_id: intent.account_id,
            strategy_id: intent.strategy_id,
            instrument_id: intent.instrument_id,
            correlation_id: intent.correlation_id,
            side: match intent.side.as_str() {
                "BUY" => Side::Buy,
                "SELL" => Side::Sell,
                _ => return Err(PaperError("persisted intent side is invalid".to_owned())),
            },
            quantity: decimal("persisted intent quantity", &intent.quantity)?,
            order_type: match intent.order_type.as_str() {
                "MARKET" => OrderType::Market,
                "LIMIT" => OrderType::Limit,
                _ => {
                    return Err(PaperError(
                        "persisted intent order type is invalid".to_owned(),
                    ))
                }
            },
            limit_price: intent
                .limit_price
                .as_deref()
                .map(|price| decimal("persisted intent limit price", price))
                .transpose()?,
            time_in_force: match intent.time_in_force.as_str() {
                "DAY" => TimeInForce::Day,
                "GTC" => TimeInForce::GoodTilCancelled,
                _ => {
                    return Err(PaperError(
                        "persisted intent time in force is invalid".to_owned(),
                    ))
                }
            },
            rationale: intent.rationale,
            created_at: intent.created_at,
            strategy_version: intent.strategy_version,
            configuration_version: intent.configuration_version,
            environment: intent.environment,
        };
        result.validate()?;
        Ok(result)
    }
}

fn decimal(name: &str, value: &str) -> Result<Decimal, PaperError> {
    Decimal::from_str(value).map_err(|error| PaperError(format!("invalid {name}: {error}")))
}

fn validate_broker_reason(name: &str, value: &str) -> Result<(), PaperError> {
    if value.trim().is_empty() || value.len() > 1_024 {
        return Err(PaperError(format!(
            "{name} must contain 1 to 1024 characters"
        )));
    }
    Ok(())
}

fn validate_broker_snapshot(snapshot: &BrokerAccountSnapshot) -> Result<(), PaperError> {
    let mut order_versions = BTreeSet::new();
    let mut broker_order_ids = BTreeSet::new();
    for order in &snapshot.orders {
        validate_canonical_id("broker snapshot client_order_id", &order.client_order_id)?;
        validate_canonical_id("broker snapshot broker_order_id", &order.broker_order_id)?;
        if order.filled_quantity < Decimal::ZERO
            || !order_versions.insert((
                order.client_order_id.as_str(),
                order.broker_order_id.as_str(),
            ))
            || !broker_order_ids.insert(order.broker_order_id.as_str())
        {
            return Err(PaperError(
                "broker snapshot has duplicate broker order versions or negative filled quantity"
                    .to_owned(),
            ));
        }
    }
    let mut instrument_ids = BTreeSet::new();
    for position in &snapshot.positions {
        validate_canonical_id("broker snapshot instrument_id", &position.instrument_id)?;
        if !instrument_ids.insert(position.instrument_id.as_str()) {
            return Err(PaperError(
                "broker snapshot contains duplicate instrument position".to_owned(),
            ));
        }
    }
    Ok(())
}

fn parse_order_state(value: &str) -> Result<OrderState, PaperError> {
    match value {
        "CREATED" => Ok(OrderState::Created),
        "PENDING_RISK" => Ok(OrderState::PendingRisk),
        "RISK_REJECTED" => Ok(OrderState::RiskRejected),
        "APPROVED" => Ok(OrderState::Approved),
        "PENDING_SUBMIT" => Ok(OrderState::PendingSubmit),
        "SUBMITTED" => Ok(OrderState::Submitted),
        "ACKNOWLEDGED" => Ok(OrderState::Acknowledged),
        "PARTIALLY_FILLED" => Ok(OrderState::PartiallyFilled),
        "FILLED" => Ok(OrderState::Filled),
        "PENDING_CANCEL" => Ok(OrderState::PendingCancel),
        "PENDING_REPLACE" => Ok(OrderState::PendingReplace),
        "CANCELLED" => Ok(OrderState::Cancelled),
        "REJECTED" => Ok(OrderState::Rejected),
        "EXPIRED" => Ok(OrderState::Expired),
        "UNKNOWN" => Ok(OrderState::Unknown),
        _ => Err(PaperError("persisted OMS state is invalid".to_owned())),
    }
}

fn parse_kill_switch_scope(value: &str) -> Result<KillSwitchScope, PaperError> {
    if value == "global" {
        return Ok(KillSwitchScope::Global);
    }
    for (prefix, builder) in [
        (
            "account:",
            KillSwitchScope::Account as fn(String) -> KillSwitchScope,
        ),
        (
            "strategy:",
            KillSwitchScope::Strategy as fn(String) -> KillSwitchScope,
        ),
        (
            "instrument:",
            KillSwitchScope::Instrument as fn(String) -> KillSwitchScope,
        ),
    ] {
        if let Some(target) = value.strip_prefix(prefix) {
            let scope = builder(target.to_owned());
            scope.validate()?;
            return Ok(scope);
        }
    }
    Err(PaperError(
        "persisted kill-switch scope is invalid".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use follon_instrument::StaticTradingCalendar;

    fn account() -> PaperAccount {
        PaperAccount {
            account_id: "acct.paper.001".to_owned(),
            currency: "USD".to_owned(),
            initial_cash: decimal("initial cash", "100000").unwrap(),
            environment: "PAPER".to_owned(),
        }
    }

    fn policy() -> PaperRiskPolicy {
        PaperRiskPolicy {
            version: "paper-risk-v1".to_owned(),
            trading_calendar_id: "cal.us_equities.nyse.v1".to_owned(),
            max_order_quantity: decimal("quantity", "100").unwrap(),
            max_order_notional: decimal("notional", "50000").unwrap(),
            max_price_deviation_bps: decimal("price collar", "100").unwrap(),
            max_open_orders: 10,
            max_position_quantity: decimal("position", "1000").unwrap(),
            max_realized_loss: decimal("loss", "10000").unwrap(),
            max_market_data_age_seconds: 5,
        }
    }

    fn service() -> PaperTradingService<IbkrPaperAdapter> {
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        PaperTradingService::new(
            account,
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            adapter,
        )
        .unwrap()
    }

    fn intent(intent_id: &str, created_at: &str) -> OrderIntent {
        OrderIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.paper.001".to_owned(),
            strategy_id: "strategy.paper.001".to_owned(),
            instrument_id: "inst.us_equity.spy".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            side: Side::Buy,
            quantity: decimal("quantity", "1").unwrap(),
            order_type: OrderType::Market,
            limit_price: None,
            time_in_force: TimeInForce::Day,
            rationale: "paper acceptance test".to_owned(),
            created_at: created_at.to_owned(),
            strategy_version: "strategy-paper-v1".to_owned(),
            configuration_version: "config-paper-v1".to_owned(),
            environment: "PAPER".to_owned(),
        }
    }

    fn market(observed_at: &str) -> PaperMarketData {
        PaperMarketData {
            instrument_id: "inst.us_equity.spy".to_owned(),
            mark_price: decimal("mark", "100").unwrap(),
            observed_at: observed_at.to_owned(),
        }
    }

    fn market_at_price(price: &str, observed_at: &str) -> PaperMarketData {
        PaperMarketData {
            instrument_id: "inst.us_equity.spy".to_owned(),
            mark_price: decimal("mark", price).unwrap(),
            observed_at: observed_at.to_owned(),
        }
    }

    fn paper_session(exchange_date: &str) -> PaperTradingSession {
        let (opens_at, closes_at) = if exchange_date >= "2026-03-09" {
            ("13:30:00Z", "20:00:00Z")
        } else {
            ("14:30:00Z", "21:00:00Z")
        };
        PaperTradingSession {
            calendar_id: "cal.us_equities.nyse.v1".to_owned(),
            session: TradingSession {
                exchange_date: exchange_date.to_owned(),
                opens_at: format!("{exchange_date}T{opens_at}"),
                closes_at: format!("{exchange_date}T{closes_at}"),
            },
        }
    }

    fn paper_calendar(sessions: &[PaperTradingSession]) -> StaticTradingCalendar {
        StaticTradingCalendar::new(
            "cal.us_equities.nyse.v1",
            sessions.iter().map(|value| value.session.clone()).collect(),
        )
        .expect("test paper calendar")
    }

    struct CashAndPositionMismatchBroker;

    impl PaperBrokerAdapter for CashAndPositionMismatchBroker {
        fn submit(
            &mut self,
            _request: &BrokerOrderRequest,
        ) -> Result<BrokerSubmitResult, PaperError> {
            Err(PaperError("not used by reconciliation test".to_owned()))
        }

        fn cancel(&mut self, _client_order_id: &str) -> Result<(), PaperError> {
            Err(PaperError("not used by reconciliation test".to_owned()))
        }

        fn poll(&mut self) -> Result<Vec<BrokerEvent>, PaperError> {
            Ok(Vec::new())
        }

        fn snapshot(&mut self, _account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
            Ok(BrokerAccountSnapshot {
                orders: Vec::new(),
                positions: vec![BrokerPositionSnapshot {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    quantity: decimal("quantity", "1").unwrap(),
                }],
                cash: Decimal::ZERO,
            })
        }

        fn reconnect(&mut self) -> Result<(), PaperError> {
            Ok(())
        }
    }

    #[test]
    fn paper_oms_applies_one_execution_and_reconciles_independent_state() {
        let mut service = service();
        let submitted = service
            .submit_intent(
                intent("intent-paper-001", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert_eq!(submitted.state, Some(OrderState::Acknowledged));
        let order_id = submitted.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1").unwrap(),
                decimal("price", "100").unwrap(),
                decimal("fee", "0.10").unwrap(),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);
        assert_eq!(
            service.order(&order_id).unwrap().oms.state,
            OrderState::Filled
        );
        let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
        assert!(report.is_clean());
        let mut wrong_calendar = paper_session("2026-01-02");
        wrong_calendar.calendar_id = "cal.other.v1".to_owned();
        let configured_session = paper_session("2026-01-02");
        let calendar = paper_calendar(std::slice::from_ref(&configured_session));
        assert!(service
            .record_paper_session(&wrong_calendar, &report, &calendar)
            .is_err());
        service
            .record_paper_session(&configured_session, &report, &calendar)
            .unwrap();
        let dashboard = service.dashboard();
        assert_eq!(dashboard.working_orders, 0);
        assert_eq!(dashboard.positions[0].quantity, "1.00000000");
        assert_eq!(dashboard.clean_paper_days, 1);
    }

    #[test]
    fn broker_evidence_handles_out_of_order_fills_pending_cancel_and_late_terminal_messages() {
        let mut service = service();
        let submitted = service
            .submit_intent(
                intent("intent-paper-edge-fill", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        let order_id = submitted.order_id.unwrap();
        let broker_order_id = service
            .order(&order_id)
            .unwrap()
            .broker_order_id
            .clone()
            .unwrap();

        // An execution is sufficient evidence to establish acknowledgement even
        // when its acknowledgement message has not yet been applied.
        service
            .order_mut(&order_id)
            .unwrap()
            .oms
            .transition(OrderState::Unknown, "TEST_OUT_OF_ORDER")
            .unwrap();
        service
            .apply_broker_event(BrokerEvent::Execution {
                execution_id: "exec-paper-before-ack".to_owned(),
                client_order_id: order_id.clone(),
                broker_order_id: broker_order_id.clone(),
                quantity: decimal("quantity", "0.5").unwrap(),
                price: decimal("price", "100").unwrap(),
                fee: Decimal::ZERO,
                executed_at: "2026-01-02T14:31:01Z".to_owned(),
            })
            .unwrap();
        assert_eq!(
            service.order(&order_id).unwrap().oms.state,
            OrderState::PartiallyFilled
        );

        service.cancel_order(&order_id).unwrap();
        assert_eq!(
            service.order(&order_id).unwrap().oms.state,
            OrderState::PendingCancel
        );
        service
            .apply_broker_event(BrokerEvent::Execution {
                execution_id: "exec-paper-during-cancel".to_owned(),
                client_order_id: order_id.clone(),
                broker_order_id,
                quantity: decimal("quantity", "0.5").unwrap(),
                price: decimal("price", "100").unwrap(),
                fee: Decimal::ZERO,
                executed_at: "2026-01-02T14:31:02Z".to_owned(),
            })
            .unwrap();
        assert_eq!(
            service.order(&order_id).unwrap().oms.state,
            OrderState::Filled
        );
        service
            .apply_broker_event(BrokerEvent::Cancelled {
                client_order_id: order_id.clone(),
                reason: "LATE_CANCEL".to_owned(),
            })
            .unwrap();
        assert_eq!(
            service.order(&order_id).unwrap().oms.state,
            OrderState::Filled
        );
        assert_eq!(
            service.order(&order_id).unwrap().filled_quantity,
            decimal("quantity", "1").unwrap()
        );
    }

    #[test]
    fn broker_terminal_and_replacement_paths_preserve_partial_fills_and_versions() {
        let mut service = service();
        let submit = |service: &mut PaperTradingService<IbkrPaperAdapter>, suffix: &str| {
            let outcome = service
                .submit_intent(
                    intent(
                        &format!("intent-paper-edge-{suffix}"),
                        "2026-01-02T14:31:00Z",
                    ),
                    market("2026-01-02T14:31:00Z"),
                    "2026-01-02T14:31:00Z",
                )
                .unwrap();
            outcome.order_id.unwrap()
        };
        let partial = |service: &mut PaperTradingService<IbkrPaperAdapter>,
                       order_id: &str,
                       execution_id: &str| {
            let broker_order_id = service
                .order(order_id)
                .unwrap()
                .broker_order_id
                .clone()
                .unwrap();
            service
                .apply_broker_event(BrokerEvent::Execution {
                    execution_id: execution_id.to_owned(),
                    client_order_id: order_id.to_owned(),
                    broker_order_id,
                    quantity: decimal("quantity", "0.5").unwrap(),
                    price: decimal("price", "100").unwrap(),
                    fee: Decimal::ZERO,
                    executed_at: "2026-01-02T14:31:01Z".to_owned(),
                })
                .unwrap();
        };

        let cancel_rejected = submit(&mut service, "cancel-rejected");
        partial(&mut service, &cancel_rejected, "exec-paper-cancel-rejected");
        service.cancel_order(&cancel_rejected).unwrap();
        service
            .apply_broker_event(BrokerEvent::CancelRejected {
                client_order_id: cancel_rejected.clone(),
                reason: "CANCEL_TOO_LATE".to_owned(),
            })
            .unwrap();
        assert_eq!(
            service.order(&cancel_rejected).unwrap().oms.state,
            OrderState::PartiallyFilled
        );

        let rejected = submit(&mut service, "rejected");
        partial(&mut service, &rejected, "exec-paper-rejected");
        service
            .apply_broker_event(BrokerEvent::Rejected {
                client_order_id: rejected.clone(),
                reason: "BROKER_REJECTED_REMAINDER".to_owned(),
            })
            .unwrap();
        assert_eq!(
            service.order(&rejected).unwrap().oms.state,
            OrderState::Rejected
        );
        assert_eq!(
            service.order(&rejected).unwrap().filled_quantity,
            decimal("quantity", "0.5").unwrap()
        );

        let expired = submit(&mut service, "expired");
        partial(&mut service, &expired, "exec-paper-expired");
        service
            .apply_broker_event(BrokerEvent::Expired {
                client_order_id: expired.clone(),
                reason: "DAY_EXPIRED".to_owned(),
            })
            .unwrap();
        assert_eq!(
            service.order(&expired).unwrap().oms.state,
            OrderState::Expired
        );

        let mut limit = intent("intent-paper-edge-replace", "2026-01-02T14:31:00Z");
        limit.order_type = OrderType::Limit;
        limit.limit_price = Some(decimal("limit", "100").unwrap());
        let replacing = service
            .submit_intent(
                limit,
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap()
            .order_id
            .unwrap();
        service
            .replace_order(&replacing, decimal("limit", "90").unwrap())
            .unwrap();
        assert_eq!(
            service.order(&replacing).unwrap().oms.state,
            OrderState::PendingReplace
        );
        service.synchronize().unwrap();
        let replaced = service.order(&replacing).unwrap();
        assert_eq!(replaced.oms.state, OrderState::Acknowledged);
        assert_eq!(replaced.broker_order_versions.len(), 2);
        service
            .apply_broker_event(BrokerEvent::ReplaceRequested {
                client_order_id: replacing.clone(),
                previous_broker_order_id: replaced.broker_order_id.clone().unwrap(),
            })
            .unwrap();
        service
            .apply_broker_event(BrokerEvent::ReplaceRejected {
                client_order_id: replacing.clone(),
                reason: "REPLACE_REJECTED".to_owned(),
            })
            .unwrap();
        assert_eq!(
            service.order(&replacing).unwrap().oms.state,
            OrderState::Acknowledged
        );
    }

    #[test]
    fn kill_switch_blocks_new_paper_orders_without_strategy_or_broker_health() {
        let mut service = service();
        service
            .activate_kill_switch(KillSwitchScope::Global)
            .unwrap();
        let result = service
            .submit_intent(
                intent("intent-paper-002", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .iter()
            .any(|reason| reason == "KILL_SWITCH_GLOBAL"));
        assert!(result.order_id.is_none());
    }

    #[test]
    fn paper_risk_requires_fresh_data_reserves_cash_and_persists_rejections() {
        let mut limited_account = account();
        limited_account.initial_cash = decimal("cash", "100").unwrap();
        let adapter = IbkrPaperAdapter::new(&limited_account).unwrap();
        let mut service = PaperTradingService::new(
            limited_account,
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            adapter,
        )
        .unwrap();
        assert!(
            service
                .submit_intent(
                    intent("intent-paper-005", "2026-01-02T14:31:00Z"),
                    market_at_price("60", "2026-01-02T14:31:00Z"),
                    "2026-01-02T14:31:00Z",
                )
                .unwrap()
                .decision
                .approved
        );
        let rejected = service
            .submit_intent(
                intent("intent-paper-006", "2026-01-02T14:31:01Z"),
                market_at_price("60", "2026-01-02T14:31:01Z"),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert!(!rejected.decision.approved);
        assert!(rejected
            .decision
            .reason_codes
            .iter()
            .any(|reason| reason == "INSUFFICIENT_INTERNAL_CASH"));
        assert!(service
            .risk_evidence("paper-risk-intent-paper-006")
            .is_some());
        assert!(service
            .submit_intent(
                intent("intent-paper-007", "2026-01-02T14:31:10Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:10Z",
            )
            .is_err());
    }

    #[test]
    fn paper_price_collar_rejection_is_explainable_and_creates_no_order() {
        let mut service = service();
        let mut far_limit = intent("intent-paper-price-collar", "2026-01-02T14:31:00Z");
        far_limit.order_type = OrderType::Limit;
        far_limit.limit_price = Some(decimal("limit", "105").unwrap());
        let result = service
            .submit_intent(
                far_limit,
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result.order_id.is_none());
        assert!(result
            .decision
            .reason_codes
            .contains(&"PRICE_COLLAR_EXCEEDED".to_owned()));
        assert!(result
            .decision
            .evaluated_limits
            .contains("requested_price_deviation_bps=500.00000000"));
    }

    #[test]
    fn reconciliation_creates_durable_incidents_instead_of_overwriting_truth() {
        let mut service = PaperTradingService::new(
            account(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            CashAndPositionMismatchBroker,
        )
        .unwrap();
        let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
        assert!(!report.is_clean());
        assert_eq!(report.issues.len(), 2);
        assert_eq!(service.dashboard().unexplained_incidents, 2);
        for issue in report.issues {
            service
                .explain_incident(&issue.incident_id, "operator reviewed independent snapshot")
                .unwrap();
        }
        assert_eq!(service.dashboard().unexplained_incidents, 0);
        assert_eq!(service.dashboard().internal_cash, "100000.00000000");
    }

    #[test]
    fn broker_snapshot_ingress_allows_versions_but_rejects_duplicate_or_malformed_identity() {
        let duplicate = BrokerAccountSnapshot {
            orders: vec![
                BrokerOrderSnapshot {
                    client_order_id: "order-intent-paper-001".to_owned(),
                    broker_order_id: "ibkr-paper-order-00000001".to_owned(),
                    state: OrderState::Acknowledged,
                    filled_quantity: Decimal::ZERO,
                },
                BrokerOrderSnapshot {
                    client_order_id: "order-intent-paper-001".to_owned(),
                    broker_order_id: "ibkr-paper-order-00000002".to_owned(),
                    state: OrderState::Acknowledged,
                    filled_quantity: Decimal::ZERO,
                },
            ],
            positions: Vec::new(),
            cash: Decimal::ZERO,
        };
        assert!(validate_broker_snapshot(&duplicate).is_ok());
        let duplicated_version = BrokerAccountSnapshot {
            orders: vec![duplicate.orders[0].clone(), duplicate.orders[0].clone()],
            positions: Vec::new(),
            cash: Decimal::ZERO,
        };
        assert!(validate_broker_snapshot(&duplicated_version).is_err());
        assert!(validate_broker_reason("reason", "").is_err());
    }

    #[test]
    fn paper_journal_allows_exactly_one_open_operator_process() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "exclusive-lock"
        ));
        let _ = fs::remove_file(&journal_path);
        let first = FilePaperJournal::open(&journal_path).unwrap();
        assert!(FilePaperJournal::open(&journal_path).is_err());
        drop(first);
        let reopened = FilePaperJournal::open(&journal_path).unwrap();
        drop(reopened);
        fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn paper_journal_refuses_a_tampered_hash_chain() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-tampered.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let paper_account = account();
        let service = PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .expect("durable service");
        drop(service);
        let original = fs::read_to_string(&journal_path).expect("read journal");
        let tampered = original.replacen("100000.00000000", "100001.00000000", 1);
        assert_ne!(tampered, original);
        fs::write(&journal_path, tampered).expect("tamper journal");

        assert!(PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .is_err());
        fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn duplicate_broker_execution_is_idempotent_and_ambiguous_submit_reconciles() {
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        let mut faulted = FaultInjectingBroker::new(adapter);
        faulted.inject(BrokerOperation::Submit, BrokerFault::AmbiguousAfterSubmit);
        let mut service = PaperTradingService::new(
            account,
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            faulted,
        )
        .unwrap();
        assert!(service
            .submit_intent(
                intent("intent-paper-003", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .is_err());
        let order_id = "order-intent-paper-003";
        assert_eq!(
            service.order(order_id).unwrap().oms.state,
            OrderState::Unknown
        );
        let clean = service
            .reconnect_and_reconcile("2026-01-02T14:32:00Z")
            .unwrap();
        assert!(clean.is_clean());
        assert_eq!(
            service.order(order_id).unwrap().oms.state,
            OrderState::Acknowledged
        );

        service
            .broker_mut()
            .inner
            .queue_fill(
                order_id,
                decimal("quantity", "1").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:32:01Z",
            )
            .unwrap();
        service
            .broker_mut()
            .inject(BrokerOperation::Poll, BrokerFault::DuplicateFirstEvent);
        service.synchronize().unwrap();
        assert_eq!(
            service.order(order_id).unwrap().filled_quantity,
            decimal("filled", "1").unwrap()
        );
        assert!(service
            .reconcile("2026-01-02T14:33:00Z")
            .unwrap()
            .is_clean());
    }

    #[test]
    fn durable_journal_recovers_unknown_orders_and_30_clean_day_gate_is_measured() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "recovery"
        ));
        let _ = fs::remove_file(&journal_path);
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        let mut faulted = FaultInjectingBroker::new(adapter);
        faulted.inject(BrokerOperation::Submit, BrokerFault::Disconnect);
        let mut durable_service = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            faulted,
            &journal_path,
        )
        .unwrap();
        assert!(durable_service
            .submit_intent(
                intent("intent-paper-004", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .is_err());
        drop(durable_service);

        let recovered = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        assert_eq!(
            recovered.order("order-intent-paper-004").unwrap().oms.state,
            OrderState::Unknown
        );
        drop(recovered);
        let mut incompatible_policy = policy();
        incompatible_policy.max_order_notional = decimal("notional", "40000").unwrap();
        assert!(PaperTradingService::open_durable(
            account.clone(),
            incompatible_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .is_err());
        let gate_journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "thirty-day-gate"
        ));
        let _ = fs::remove_file(&gate_journal_path);
        let gate_account = account.clone();
        let mut gate_service = PaperTradingService::open_durable(
            gate_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&gate_account).unwrap(),
            &gate_journal_path,
        )
        .unwrap();
        let dates = [
            "2026-03-02",
            "2026-03-03",
            "2026-03-04",
            "2026-03-05",
            "2026-03-06",
            "2026-03-09",
            "2026-03-10",
            "2026-03-11",
            "2026-03-12",
            "2026-03-13",
            "2026-03-16",
            "2026-03-17",
            "2026-03-18",
            "2026-03-19",
            "2026-03-20",
            "2026-03-23",
            "2026-03-24",
            "2026-03-25",
            "2026-03-26",
            "2026-03-27",
            "2026-03-30",
            "2026-03-31",
            "2026-04-01",
            "2026-04-02",
            "2026-04-06",
            "2026-04-07",
            "2026-04-08",
            "2026-04-09",
            "2026-04-10",
            "2026-04-13",
        ];
        let gate_sessions: Vec<_> = dates.iter().map(|date| paper_session(date)).collect();
        let gate_calendar = paper_calendar(&gate_sessions);
        for (date, session) in dates.into_iter().zip(&gate_sessions) {
            let report = gate_service
                .reconcile(&format!("{date}T21:00:00Z"))
                .unwrap();
            assert!(report.is_clean());
            gate_service
                .record_paper_session(session, &report, &gate_calendar)
                .unwrap();
        }
        assert!(gate_service.promotion_status().eligible_for_next_gate);
        drop(gate_service);
        fs::remove_file(gate_journal_path).unwrap();
        fs::remove_file(journal_path).unwrap();
    }
}
