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

use follon_accounting::{
    Currency, FxBook, MarginPolicy, MarginPosition, ShortTaxLot, TaxLot, TaxLotBook,
    TaxLotBookSnapshot, TaxLotSelection,
};
use follon_control_plane::{EngineError, OmsComboOrder, OmsOrder, Portfolio};
use follon_domain::{
    price_deviation_bps, validate_canonical_id, validate_utc_timestamp, ComboIntent, Decimal, Fill,
    OrderIntent, OrderState, OrderType, RiskDecision, Side, TimeInForce,
};
use follon_instrument::{TradingCalendar, TradingSession};
use follon_risk::{CandidateOrder, PortfolioRiskSnapshot, RestingOrder, RiskPosition};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

mod combinations;
pub mod qualification;
use combinations::PersistentComboExecutionState;
pub use combinations::{BrokerComboExecution, BrokerComboExecutionLeg};

pub use qualification::{
    GatewayQualificationError, GatewayQualificationMatrix, QualificationState, QualifiedCapability,
};

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

impl From<follon_accounting::AccountingError> for PaperError {
    fn from(error: follon_accounting::AccountingError) -> Self {
        Self(error.0)
    }
}

impl From<follon_risk::RiskError> for PaperError {
    fn from(error: follon_risk::RiskError) -> Self {
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

/// Version of the normalized PAPER adapter contract implemented in this crate.
///
/// Version 2 makes every stateful adapter operation explicitly account scoped,
/// allowing a deployment composition to route several isolated broker accounts
/// without making a client order id globally meaningful.
pub const PAPER_BROKER_ADAPTER_CONTRACT_VERSION: u32 = 3;

/// Controlled deployment binding of a PAPER account to one adapter instance and venue.
///
/// This is configuration metadata only. It grants no client, strategy, or UI
/// access to the adapter and does not contain a credential or endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperBrokerRoute {
    /// Canonical OMS account identity routed by this binding.
    pub account_id: String,
    /// Deployment-local canonical identity for exactly one adapter instance.
    pub adapter_id: String,
    /// Canonical venue identity used for evidence and operational configuration.
    pub venue_id: String,
    /// Must be the literal value `PAPER`.
    pub environment: String,
}

impl PaperBrokerRoute {
    /// Validates an account-isolated PAPER route before it is registered.
    pub fn validate(&self) -> Result<(), PaperError> {
        for (name, value) in [
            ("paper broker route account_id", self.account_id.as_str()),
            ("paper broker route adapter_id", self.adapter_id.as_str()),
            ("paper broker route venue_id", self.venue_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.environment != "PAPER" {
            return Err(PaperError(
                "paper broker routes must use the PAPER environment".to_owned(),
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

/// A normalized request for an atomic option combination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerComboRequest {
    /// OMS-generated immutable client idempotency key.
    pub client_order_id: String,
    /// Paper account selected for the operation.
    pub account_id: String,
    /// Exact combination legs.
    pub legs: Vec<BrokerComboLeg>,
    /// Limit price for the entire combination.
    pub limit_price: Option<Decimal>,
}

impl BrokerComboRequest {
    /// Normalizes an approved OMS combination order into a broker request.
    ///
    /// The broker sees each leg's **exact contract quantity**, ratio already
    /// applied, so no adapter has to re-derive it from a unit count and a
    /// ratio. `limit_price` carries the protection amount the operator stated,
    /// whose direction is fixed by the combination's own price-limit kind.
    fn from_combo_order(order: &OmsComboOrder) -> Result<Self, PaperError> {
        let mut legs = Vec::with_capacity(order.intent.legs.len());
        for leg in &order.intent.legs {
            let quantity = order.intent.leg_quantity(leg)?;
            legs.push(BrokerComboLeg {
                instrument_id: leg.instrument_id.clone(),
                side: leg.side,
                ratio: leg.ratio,
                quantity,
            });
        }
        Ok(Self {
            client_order_id: order.order_id.clone(),
            account_id: order.intent.account_id.clone(),
            legs,
            limit_price: Some(order.intent.price_limit.amount()),
        })
    }

    fn validate(&self) -> Result<(), PaperError> {
        for (name, value) in [
            ("combo client_order_id", self.client_order_id.as_str()),
            ("combo account_id", self.account_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.legs.is_empty() || self.legs.len() > 16 {
            return Err(PaperError(
                "paper combo request must contain between one and sixteen legs".to_owned(),
            ));
        }
        if self.limit_price.is_some_and(|price| price <= Decimal::ZERO) {
            return Err(PaperError(
                "paper combo limit price must be positive".to_owned(),
            ));
        }
        for leg in &self.legs {
            validate_canonical_id("combo leg instrument_id", &leg.instrument_id)?;
            if leg.ratio == 0 {
                return Err(PaperError(
                    "paper combo leg ratio must be positive".to_owned(),
                ));
            }
            if leg.quantity <= Decimal::ZERO {
                return Err(PaperError(
                    "paper combo leg quantity must be positive".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// A leg within a combo request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerComboLeg {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Requested side.
    pub side: Side,
    /// Exact requested quantity ratio.
    pub ratio: u32,
    /// Exact contract quantity for this leg, ratio already applied.
    ///
    /// Carried alongside the ratio so no adapter has to re-derive it from a
    /// unit count, which is the kind of duplicated arithmetic that drifts.
    pub quantity: Decimal,
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
    /// Account selected by the OMS for this immutable client identity.
    pub account_id: String,
    /// Immutable OMS client identity.
    pub client_order_id: String,
    /// Current broker-native identity being superseded.
    pub previous_broker_order_id: String,
    /// New positive limit price.
    pub limit_price: Decimal,
}

impl BrokerReplaceRequest {
    fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("replace account_id", &self.account_id)?;
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

/// An account-scoped request to cancel one immutable OMS client identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerCancelRequest {
    /// Account selected by the OMS for this immutable client identity.
    pub account_id: String,
    /// Immutable OMS client identity to cancel.
    pub client_order_id: String,
}

impl BrokerCancelRequest {
    fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("cancel account_id", &self.account_id)?;
        validate_canonical_id("cancel client_order_id", &self.client_order_id)?;
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
    /// Complete atomic fill evidence in whole combination units (adapter contract v3).
    /// Adapters must assemble and identify every leg before emitting this event.
    ComboExecution(BrokerComboExecution),
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
    /// Returns a stable non-secret fingerprint of this adapter implementation
    /// and its account-specific transport configuration.
    ///
    /// Registries require a non-empty value at registration so a durable route
    /// cannot be recovered against a different adapter configuration under the
    /// same user-facing labels.
    fn adapter_configuration_fingerprint(&self, _account_id: &str) -> Result<String, PaperError> {
        Ok(String::new())
    }
    /// Returns stable non-secret routing configuration evidence for one account.
    ///
    /// The legacy single-adapter default is empty so existing PAPER journals
    /// retain their configuration fingerprint. Multi-route implementations
    /// must return a stable account route fingerprint so recovery cannot use a
    /// different adapter or venue unnoticed.
    fn configuration_fingerprint(&self, _account_id: &str) -> Result<String, PaperError> {
        Ok(String::new())
    }
    /// Indicates whether this composition may initialize a new empty journal.
    ///
    /// The default permits a new PAPER service. A legacy migration composition
    /// must be restricted to reopening a pre-existing journal.
    fn permits_empty_journal(&self, _account_id: &str) -> bool {
        true
    }
    /// Submits exactly one client-idempotent paper order.
    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError>;
    /// Submits an atomic combination order.
    fn submit_combo(
        &mut self,
        _request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        Err(PaperError(
            "broker adapter does not support native combos".to_owned(),
        ))
    }
    /// Requests cancellation by the account-scoped immutable client identity.
    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError>;
    /// Requests a price-only replacement. The result arrives through [`BrokerEvent`].
    fn replace(&mut self, _request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        Err(PaperError(
            "paper broker adapter does not support order replacement".to_owned(),
        ))
    }
    /// Drains one account's normalized asynchronous evidence in arrival order.
    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError>;
    /// Returns an independent broker-side account snapshot for reconciliation.
    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError>;
    /// Re-establishes one account's paper connection. The caller must reconcile afterwards.
    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError>;
}

/// Deterministic composition root for isolated PAPER broker-account routes.
///
/// The registry is only an OMS-side adapter selection mechanism. A route has
/// no credentials and only delegates normalized requests after the caller's
/// existing risk/OMS processing. `BTreeMap` storage makes route enumeration
/// stable and unknown or duplicate bindings fail closed.
pub struct PaperBrokerRegistry {
    routes: BTreeMap<String, PaperBrokerRoute>,
    adapters: BTreeMap<String, Box<dyn PaperBrokerAdapter>>,
    adapter_configuration_fingerprints: BTreeMap<String, String>,
    legacy_fingerprint_accounts: BTreeSet<String>,
}

impl Default for PaperBrokerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PaperBrokerRegistry {
    /// Creates an empty registry; a deployment must register every PAPER route explicitly.
    pub fn new() -> Self {
        Self {
            routes: BTreeMap::new(),
            adapters: BTreeMap::new(),
            adapter_configuration_fingerprints: BTreeMap::new(),
            legacy_fingerprint_accounts: BTreeSet::new(),
        }
    }

    /// Registers one isolated adapter instance for one canonical PAPER account.
    pub fn register(
        &mut self,
        route: PaperBrokerRoute,
        adapter: Box<dyn PaperBrokerAdapter>,
    ) -> Result<(), PaperError> {
        self.register_inner(route, adapter, false)
    }

    /// Registers the exact legacy local-IBKR PAPER composition without changing
    /// its journal fingerprint.
    ///
    /// This is only a migration bridge for configuration schema v1 and can
    /// reopen, but not initialize, a journal. New configurations must use
    /// [`Self::register`] so the durable journal binds the route and actual
    /// adapter configuration fingerprints.
    pub fn register_legacy_ibkr_paper_route(
        &mut self,
        account: &PaperAccount,
    ) -> Result<(), PaperError> {
        account.validate()?;
        let route = PaperBrokerRoute {
            account_id: account.account_id.clone(),
            adapter_id: format!("adapter.ibkr.paper.{}", account.account_id),
            venue_id: "venue.ibkr.paper".to_owned(),
            environment: "PAPER".to_owned(),
        };
        self.register_inner(route, Box::new(IbkrPaperAdapter::new(account)?), true)
    }

    fn register_inner(
        &mut self,
        route: PaperBrokerRoute,
        adapter: Box<dyn PaperBrokerAdapter>,
        preserve_legacy_fingerprint: bool,
    ) -> Result<(), PaperError> {
        route.validate()?;
        if self.routes.contains_key(&route.account_id) {
            return Err(PaperError(
                "paper broker route already exists for account".to_owned(),
            ));
        }
        if self.adapters.contains_key(&route.adapter_id) {
            return Err(PaperError(
                "paper broker adapter_id is already registered".to_owned(),
            ));
        }
        let adapter_configuration_fingerprint =
            adapter.adapter_configuration_fingerprint(&route.account_id)?;
        if adapter_configuration_fingerprint.is_empty() {
            return Err(PaperError(
                "paper broker adapter must expose a non-secret configuration fingerprint"
                    .to_owned(),
            ));
        }
        self.adapters.insert(route.adapter_id.clone(), adapter);
        self.adapter_configuration_fingerprints
            .insert(route.adapter_id.clone(), adapter_configuration_fingerprint);
        if preserve_legacy_fingerprint {
            self.legacy_fingerprint_accounts
                .insert(route.account_id.clone());
        }
        self.routes.insert(route.account_id.clone(), route);
        Ok(())
    }

    /// Returns the configured routes in stable account-id order.
    pub fn routes(&self) -> Vec<PaperBrokerRoute> {
        self.routes.values().cloned().collect()
    }

    fn adapter_for_account(
        &mut self,
        account_id: &str,
    ) -> Result<&mut (dyn PaperBrokerAdapter + '_), PaperError> {
        validate_canonical_id("paper broker route account_id", account_id)?;
        let adapter_id = self
            .routes
            .get(account_id)
            .ok_or_else(|| {
                PaperError("paper broker route is not configured for account".to_owned())
            })?
            .adapter_id
            .clone();
        match self.adapters.get_mut(&adapter_id) {
            Some(adapter) => Ok(adapter.as_mut()),
            None => Err(PaperError(
                "paper broker route adapter is unavailable".to_owned(),
            )),
        }
    }
}

impl PaperBrokerAdapter for PaperBrokerRegistry {
    fn configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        validate_canonical_id("paper broker route account_id", account_id)?;
        let route = self.routes.get(account_id).ok_or_else(|| {
            PaperError("paper broker route is not configured for account".to_owned())
        })?;
        if self.legacy_fingerprint_accounts.contains(account_id) {
            return Ok(String::new());
        }
        let adapter_configuration_fingerprint = self
            .adapter_configuration_fingerprints
            .get(&route.adapter_id)
            .ok_or_else(|| PaperError("paper broker route adapter is unavailable".to_owned()))?;
        Ok(hash_fingerprint_parts(&[
            "paper-broker-route-v2",
            &route.account_id,
            &route.adapter_id,
            &route.venue_id,
            &route.environment,
            adapter_configuration_fingerprint,
        ]))
    }

    fn permits_empty_journal(&self, account_id: &str) -> bool {
        !self.legacy_fingerprint_accounts.contains(account_id)
    }

    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        request.validate()?;
        self.adapter_for_account(&request.account_id)?
            .submit(request)
    }

    fn submit_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        request.validate()?;
        self.adapter_for_account(&request.account_id)?
            .submit_combo(request)
    }

    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        request.validate()?;
        self.adapter_for_account(&request.account_id)?
            .cancel(request)
    }

    fn replace(&mut self, request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        request.validate()?;
        self.adapter_for_account(&request.account_id)?
            .replace(request)
    }

    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        self.adapter_for_account(account_id)?.poll(account_id)
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        self.adapter_for_account(account_id)?.snapshot(account_id)
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        self.adapter_for_account(account_id)?.reconnect(account_id)
    }
}

#[derive(Clone, Debug)]
struct IbkrOrder {
    broker_order_id: String,
    request: BrokerOrderRequest,
    state: OrderState,
    filled_quantity: Decimal,
}

/// One accepted atomic combination in the paper-bridge model.
#[derive(Clone, Debug)]
struct IbkrCombo {
    broker_order_id: String,
    request: BrokerComboRequest,
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
    /// Accepted atomic combinations, keyed by client identity. Separate from
    /// `orders` because a combination is one broker order over several
    /// instruments and has no single `BrokerOrderRequest` shape.
    combos: BTreeMap<String, IbkrCombo>,
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

    /// Accepts an atomic combination, mirroring the real paper bridge.
    ///
    /// The genuine IBKR paper transport does support native BAG combinations
    /// (`adapters/brokers/ibkr::submit_paper_combo`), so this deterministic
    /// model of that same bridge supports them too. A model that refused what
    /// the thing it models accepts would make the combination path untestable
    /// against anything but a rejection.
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
    fn adapter_configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        self.inner.adapter_configuration_fingerprint(account_id)
    }

    fn configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        self.inner.configuration_fingerprint(account_id)
    }

    fn permits_empty_journal(&self, account_id: &str) -> bool {
        self.inner.permits_empty_journal(account_id)
    }

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

    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        match self.next_fault(BrokerOperation::Cancel) {
            Some(BrokerFault::Disconnect) | Some(BrokerFault::AmbiguousAfterSubmit) => Err(
                PaperError("fault injection made paper cancellation outcome ambiguous".to_owned()),
            ),
            Some(BrokerFault::DuplicateFirstEvent) | None => self.inner.cancel(request),
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

    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        match self.next_fault(BrokerOperation::Poll) {
            Some(BrokerFault::Disconnect) | Some(BrokerFault::AmbiguousAfterSubmit) => Err(
                PaperError("fault injection disconnected paper polling".to_owned()),
            ),
            Some(BrokerFault::DuplicateFirstEvent) => {
                let mut events = self.inner.poll(account_id)?;
                if let Some(first) = events.first().cloned() {
                    events.push(first);
                }
                Ok(events)
            }
            None => self.inner.poll(account_id),
        }
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        self.inner.snapshot(account_id)
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        match self.next_fault(BrokerOperation::Reconnect) {
            Some(_) => Err(PaperError(
                "fault injection rejected paper reconnect".to_owned(),
            )),
            None => self.inner.reconnect(account_id),
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
        self.reasons_for(
            &intent.account_id,
            &intent.strategy_id,
            std::slice::from_ref(&intent.instrument_id),
        )
    }

    /// Kill-switch reasons for a combination.
    ///
    /// An instrument switch on *any* leg blocks the whole combination. There is
    /// no partial execution to fall back on — the group is atomic — so a single
    /// halted leg halts the structure.
    fn combo_rejection_reasons(&self, intent: &ComboIntent) -> Vec<String> {
        let instruments = intent
            .legs
            .iter()
            .map(|leg| leg.instrument_id.clone())
            .collect::<Vec<_>>();
        self.reasons_for(&intent.account_id, &intent.strategy_id, &instruments)
    }

    fn reasons_for(
        &self,
        account_id: &str,
        strategy_id: &str,
        instrument_ids: &[String],
    ) -> Vec<String> {
        let mut scopes = vec![
            KillSwitchScope::Global,
            KillSwitchScope::Account(account_id.to_owned()),
            KillSwitchScope::Strategy(strategy_id.to_owned()),
        ];
        scopes.extend(
            instrument_ids
                .iter()
                .map(|instrument_id| KillSwitchScope::Instrument(instrument_id.clone())),
        );
        let mut reasons = scopes
            .iter()
            .filter(|scope| self.active.contains(*scope))
            .map(|scope| {
                format!(
                    "KILL_SWITCH_{}",
                    scope.as_key().to_ascii_uppercase().replace(':', "_")
                )
            })
            .collect::<Vec<_>>();
        reasons.dedup();
        reasons
    }
}

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
    fn tick_rejection(
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
    fn lot_rejection(&self, instrument_id: &str, quantity: Decimal) -> Option<&'static str> {
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
    fn combo_tick_rejections(&self, intent: &ComboIntent) -> Vec<&'static str> {
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
    fn combo_tick_evidence(&self, intent: &ComboIntent) -> String {
        render_leg_entries(intent, &self.instrument_tick_sizes)
    }

    /// Each leg's configured lot size, for the decision evidence.
    fn combo_lot_evidence(&self, intent: &ComboIntent) -> String {
        render_leg_entries(intent, &self.instrument_lot_sizes)
    }

    /// Whether a projected per-instrument position breaches this policy.
    ///
    /// Shared by the single-order and combination gates so a combination leg is
    /// judged by exactly the rule a plain order on the same instrument would
    /// meet — neither stricter nor looser.
    fn breaches_position_limit(&self, projected: Decimal) -> Result<bool, PaperError> {
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

/// One mark per leg of a combination, evaluated as a single observation.
///
/// Kept as a collection of [`PaperMarketData`] rather than a new shape so the
/// durable journal record can reuse the existing per-mark serialisation without
/// a second format to migrate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperComboMarketData {
    /// Exactly one validated mark per combination leg, in leg order.
    pub marks: Vec<PaperMarketData>,
}

impl PaperComboMarketData {
    /// Validates the observation set against the combination it prices.
    ///
    /// Every leg must be quoted. A combination priced from a partial set would
    /// have to guess at least one leg's notional, and guessing is exactly what
    /// a pre-trade gate exists to prevent.
    pub fn validate_for(&self, intent: &ComboIntent) -> Result<(), PaperError> {
        if self.marks.len() != intent.legs.len() {
            return Err(PaperError(
                "paper combo observation must carry exactly one mark per leg".to_owned(),
            ));
        }
        for mark in &self.marks {
            mark.validate()?;
        }
        for leg in &intent.legs {
            if self.mark_for(&leg.instrument_id).is_none() {
                return Err(PaperError(format!(
                    "paper combo observation is missing a mark for {}",
                    leg.instrument_id
                )));
            }
        }
        Ok(())
    }

    /// The validated mark for one leg instrument, if present.
    pub fn mark_for(&self, instrument_id: &str) -> Option<&PaperMarketData> {
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
    pub fn oldest_observed_at(&self) -> Result<&str, PaperError> {
        self.marks
            .iter()
            .map(|mark| mark.observed_at.as_str())
            // Canonical second-precision UTC (`validate_utc_timestamp`) sorts
            // lexicographically in exactly timestamp order, so no parsing is
            // needed to find the oldest.
            .min()
            .ok_or_else(|| PaperError("paper combo observation is empty".to_owned()))
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

/// Internal paper OMS record for one atomic multi-leg combination.
#[derive(Clone, Debug)]
pub struct PaperComboOrder {
    /// Durable OMS combination order and legal lifecycle state.
    pub oms: OmsComboOrder,
    /// Broker order identity once known. One identity for the whole group: an
    /// atomic combination is a single broker order, not one per leg.
    pub broker_order_id: Option<String>,
    /// Every broker-native order identity issued for this immutable OMS order.
    pub broker_order_versions: Vec<String>,
    /// Exact per-leg observation used for approval and cash reservation.
    pub market: PaperComboMarketData,
    /// Independently accounted whole combination units, never a leg quantity.
    pub filled_quantity: Decimal,
    /// Accepted complete execution receipts, retained for exact retry validation.
    executions: BTreeMap<String, BrokerComboExecution>,
    /// Malformed/drained evidence cannot be cleared by a later status message.
    evidence_error: Option<String>,
}

impl PaperComboOrder {
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

    /// Cash a working combination still has committed.
    ///
    /// Only the unfilled net debit reserves cash, matching the approval gate.
    fn reserved_cash(&self) -> Result<Decimal, PaperError> {
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

/// Immutable record of a combination decision and the observation behind it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperComboRiskEvidence {
    /// The complete immutable combination intent evaluated by risk, including
    /// rejected ones.
    pub intent: ComboIntent,
    /// The versioned risk result.
    pub decision: RiskDecision,
    /// The exact validated per-leg observation evaluated by risk.
    pub market: PaperComboMarketData,
    /// The authenticated operator who submitted the combination, when it
    /// arrived through an authenticated route; `None` for a direct caller.
    pub submitted_by: Option<String>,
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
    /// Absent in a document written before this field existed; an empty book
    /// is exactly correct for one, since no fill could have been applied to a
    /// tax-lot ledger that did not yet exist.
    ///
    /// This default makes the *type* tolerant. It does not, on its own, let
    /// `FilePaperJournal::open` read an older journal **file**: that reader
    /// additionally requires every line to re-serialize byte-for-byte, so a
    /// line missing any field the current serializer writes is rejected before
    /// a default can apply. Verified directly rather than assumed. The same
    /// correction applies to every `#[serde(default)]` field below.
    #[serde(default)]
    tax_lots: PersistentTaxLotBook,
    /// Last observed mark per instrument (Decimal-as-string, matching every
    /// other persisted decimal field). Absent in a document written before
    /// this field existed; an empty map is correct for one -- every position
    /// simply falls back to its own average cost until re-quoted. See
    /// `tax_lots` above on what this default does and does not achieve.
    #[serde(default)]
    marks: BTreeMap<String, String>,
    /// Highest observed real equity (Decimal-as-string). `None` on a journal
    /// written before this field existed; `restore()` bootstraps it to the
    /// account's real equity computed from the rest of the just-restored
    /// state, which is the honest value for a peak that was never tracked
    /// before now.
    #[serde(default)]
    peak_equity: Option<String>,
    /// UTC calendar date of the current session-start daily-loss baseline
    /// (Decimal-as-string equity paired below). `None` on a journal written
    /// before this field existed, or before any risk evaluation has ever run.
    #[serde(default)]
    daily_baseline_date: Option<String>,
    /// Equity observed at the first risk evaluation of `daily_baseline_date`
    /// (Decimal-as-string). Present if and only if `daily_baseline_date` is.
    #[serde(default)]
    daily_baseline_equity: Option<String>,
    /// Net signed per-strategy contribution to each instrument
    /// (`instrument_id -> strategy_id -> quantity`, Decimal-as-string).
    /// Missing or absent entries on a journal written before this field
    /// existed are correct as empty: no fill could have been attributed to a
    /// strategy-attribution ledger that did not yet exist.
    #[serde(default)]
    strategy_attribution: BTreeMap<String, BTreeMap<String, String>>,
    /// Atomic multi-leg combination orders. Absent in a document written
    /// before the combination path existed; an empty map is exactly correct
    /// for one, since no combination could have been submitted through a path
    /// that did not yet exist. See `tax_lots` above on what this default does
    /// and does not achieve.
    #[serde(default)]
    combo_orders: BTreeMap<String, PersistentComboOrder>,
    /// Combination risk evidence, including refusals. Empty on an older
    /// journal for the same reason.
    #[serde(default)]
    combo_risk_evidence: BTreeMap<String, PersistentComboRiskEvidence>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PersistentTaxLotBook {
    lots: BTreeMap<String, Vec<PersistentTaxLot>>,
    applied_lot_ids: Vec<String>,
    applied_disposal_ids: Vec<String>,
    realized_by_currency: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    short_lots: BTreeMap<String, Vec<PersistentTaxLot>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    applied_short_lot_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    applied_cover_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentTaxLot {
    lot_id: String,
    opened_at: String,
    remaining_quantity: String,
    unit_cost: String,
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
struct PersistentComboOrder {
    intent: PersistentComboIntent,
    state: String,
    broker_order_id: Option<String>,
    #[serde(default)]
    broker_order_versions: Vec<String>,
    /// One persisted mark per leg, reusing the single-instrument observation
    /// shape rather than inventing a second format to migrate later.
    market: Vec<PersistentMarketData>,
    /// Versioned extension; absence retains the exact E1.3a serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution_state: Option<PersistentComboExecutionState>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentComboRiskEvidence {
    intent: PersistentComboIntent,
    approved: bool,
    reason_codes: Vec<String>,
    policy_version: String,
    decided_at: String,
    correlation_id: String,
    actor: String,
    evaluated_limits: String,
    market: Vec<PersistentMarketData>,
    /// Versioned extension; absence retains the exact pre-E3.3a serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    submitted_by: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentComboIntent {
    intent_id: String,
    account_id: String,
    strategy_id: String,
    correlation_id: String,
    legs: Vec<PersistentComboLeg>,
    combo_quantity: String,
    /// `MAXIMUM_DEBIT` or `MINIMUM_CREDIT`, matching `ComboPriceLimit::kind`.
    price_limit_kind: String,
    price_limit_amount: String,
    time_in_force: String,
    rationale: String,
    created_at: String,
    strategy_version: String,
    configuration_version: String,
    environment: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentComboLeg {
    instrument_id: String,
    side: String,
    ratio: u32,
    limit_price: String,
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
    broker_route_fingerprint: String,
    broker_connected: bool,
    cash: Decimal,
    orders: BTreeMap<String, PaperOrder>,
    risk_evidence: BTreeMap<String, PaperRiskEvidence>,
    /// Atomic multi-leg combinations, tracked separately from `orders` because
    /// a combination is one order over several instruments and does not fit the
    /// single-instrument shape of [`PaperOrder`]. Every risk counter that reads
    /// `orders` reads this map too -- a combination that were invisible to the
    /// single-order gate would be a hole in exactly the limits it is subject to.
    combo_orders: BTreeMap<String, PaperComboOrder>,
    combo_risk_evidence: BTreeMap<String, PaperComboRiskEvidence>,
    portfolios: BTreeMap<String, Portfolio>,
    /// Independent FIFO long-lot cost-basis ledger, kept in lockstep with
    /// `portfolios` from the same fills. `Portfolio` tracks a single running
    /// average cost for OMS/risk decisions; this book instead retains
    /// individual acquisition lots so a real disposal reports an auditable,
    /// tax-lot-accurate realized gain/loss, not just the average-cost figure.
    tax_lots: TaxLotBook,
    /// Last observed mark per instrument, from every past risk evaluation this
    /// service has performed. Feeds the Slice-1 aggregate risk composition's
    /// multi-instrument exposure calculation; a position with no cached mark
    /// yet falls back to its own average cost. Not a live market-data feed --
    /// see [`PortfolioRiskComposition`].
    marks: BTreeMap<String, Decimal>,
    /// Highest real equity (cash plus every marked position) this service has
    /// ever observed, updated unconditionally from every risk evaluation --
    /// independent of whether Slice-1 composition is even configured, so
    /// enabling it later does not start drawdown tracking from a fresh,
    /// artificially favorable baseline. Feeds the Slice-2 `PortfolioRiskSnapshot`'s
    /// real `peak_equity` (see [`PortfolioRiskComposition`]).
    peak_equity: Decimal,
    /// UTC calendar date (`YYYY-MM-DD`, sliced from a risk decision's own
    /// canonical `decided_at`) of the current session-start daily-loss
    /// baseline. `None` until the first risk evaluation this service has ever
    /// performed. Reset -- not maxed, unlike `peak_equity` -- at the first
    /// evaluation whose decision time falls on a new UTC calendar day.
    daily_baseline_date: Option<String>,
    /// Real equity observed at the first risk evaluation of
    /// `daily_baseline_date`. Meaningless while `daily_baseline_date` is
    /// `None`. Feeds the Slice-2b `PortfolioRiskSnapshot`'s real `daily_pnl`
    /// (`equity - daily_baseline_equity`) -- see [`PortfolioRiskComposition`].
    daily_baseline_equity: Decimal,
    /// Net signed quantity each strategy has contributed to each instrument
    /// (`instrument_id -> strategy_id -> quantity`), updated from every real
    /// fill. Feeds the Slice-2d aggregate-risk snapshot's real per-strategy
    /// `RiskPosition` rows -- see [`PortfolioRiskComposition`] and
    /// [`Self::apply_strategy_attribution_fill`].
    strategy_attribution: BTreeMap<String, BTreeMap<String, Decimal>>,
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
        let broker_route_fingerprint = broker.configuration_fingerprint(&account.account_id)?;
        Ok(Self {
            cash: account.initial_cash,
            peak_equity: account.initial_cash,
            daily_baseline_date: None,
            daily_baseline_equity: Decimal::ZERO,
            strategy_attribution: BTreeMap::new(),
            account,
            risk_policy,
            kill_switches,
            broker,
            broker_route_fingerprint,
            broker_connected: true,
            orders: BTreeMap::new(),
            risk_evidence: BTreeMap::new(),
            combo_orders: BTreeMap::new(),
            combo_risk_evidence: BTreeMap::new(),
            portfolios: BTreeMap::new(),
            tax_lots: TaxLotBook::default(),
            marks: BTreeMap::new(),
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
        if latest.is_none() && !broker.permits_empty_journal(&account.account_id) {
            return Err(PaperError(
                "legacy PAPER adapter routing may only reopen an existing journal".to_owned(),
            ));
        }
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

    /// Returns the canonical account identity owned by this service instance.
    /// Delivery boundaries use this before account-scoped cancel/close actions
    /// so a syntactically valid but mismatched account can never act on an
    /// order or position belonging to the configured route.
    pub fn account_id(&self) -> &str {
        &self.account.account_id
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

    /// Submits one atomic multi-leg combination through the full risk gate.
    ///
    /// This is the combination analogue of [`Self::submit_intent`] and follows
    /// the same order of operations for the same reasons: validate, refuse a
    /// disconnected session, answer an idempotent retry from stored evidence,
    /// evaluate risk, persist `PENDING_SUBMIT` **before** crossing the broker
    /// boundary, then record the normalized outcome. A transport failure leaves
    /// the combination explicitly `UNKNOWN` rather than guessing.
    ///
    /// A combination is submitted as one broker order. If the adapter does not
    /// support a native atomic combination it rejects the whole request, and no
    /// leg is ever transmitted on its own — that boundary is the adapter's, and
    /// this method does not work around it by splitting the group.
    pub fn submit_combo_intent(
        &mut self,
        intent: ComboIntent,
        market: PaperComboMarketData,
        decided_at: &str,
    ) -> Result<PaperSubmitOutcome, PaperError> {
        self.submit_combo_intent_as(intent, market, decided_at, None)
    }

    /// Submits a combination on behalf of an authenticated operator, whose
    /// identity is journaled with the risk evidence. An idempotent retry must
    /// come from the same submitter, so one operator cannot claim, or replay
    /// as their own, another operator's order.
    pub fn submit_combo_intent_as(
        &mut self,
        intent: ComboIntent,
        market: PaperComboMarketData,
        decided_at: &str,
        submitted_by: Option<&str>,
    ) -> Result<PaperSubmitOutcome, PaperError> {
        self.ensure_persistence_healthy()?;
        if let Some(operator) = submitted_by {
            validate_canonical_id("paper combo submitted_by", operator)?;
        }
        intent.validate()?;
        validate_utc_timestamp("paper combo risk decision time", decided_at)?;
        market.validate_for(&intent)?;
        if intent.environment != "PAPER" {
            return Err(PaperError(
                "paper OMS refuses a combination outside the PAPER environment".to_owned(),
            ));
        }
        if intent.account_id != self.account.account_id {
            return Err(PaperError(
                "paper combo intent account does not match service".to_owned(),
            ));
        }
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect and reconcile before submission"
                    .to_owned(),
            ));
        }

        let order_id = OmsComboOrder::order_id_for(&intent.intent_id);
        if let Some(existing) = self.combo_orders.get(&order_id) {
            if existing.oms.intent != intent {
                return Err(PaperError(
                    "paper combination idempotency key was reused with different intent data"
                        .to_owned(),
                ));
            }
            let evidence = self
                .combo_risk_evidence
                .get(&format!("paper-combo-risk-{}", intent.intent_id))
                .ok_or_else(|| {
                    PaperError(
                        "existing paper combination order is missing its risk evidence".to_owned(),
                    )
                })?;
            if evidence.market != market || evidence.decision.decided_at != decided_at {
                return Err(PaperError(
                    "paper combination retry must use the original market observation and decision time"
                        .to_owned(),
                ));
            }
            if evidence.submitted_by.as_deref() != submitted_by {
                return Err(PaperError(
                    "paper combination retry must come from the original submitter".to_owned(),
                ));
            }
            return Ok(PaperSubmitOutcome {
                decision: evidence.decision.clone(),
                order_id: Some(order_id),
                state: Some(existing.oms.state),
            });
        }

        // `evaluate_combo_risk` performs its own staleness check and updates
        // the mark cache, peak equity and daily baseline, exactly as the
        // single-order path relies on `evaluate_risk` to do.
        let decision = self.evaluate_combo_risk(&intent, &market, decided_at)?;
        let evidence = PaperComboRiskEvidence {
            intent: intent.clone(),
            decision: decision.clone(),
            market: market.clone(),
            submitted_by: submitted_by.map(str::to_owned),
        };
        if !decision.approved {
            self.combo_risk_evidence
                .insert(decision.decision_id.clone(), evidence);
            self.persist()?;
            return Ok(PaperSubmitOutcome {
                decision,
                order_id: None,
                state: None,
            });
        }

        let mut oms = OmsComboOrder::from_approved_intent(intent, &decision)?;
        oms.transition(OrderState::Approved, "PAPER_COMBO_RISK_APPROVED")?;
        oms.transition(
            OrderState::PendingSubmit,
            "PAPER_COMBO_SUBMISSION_REQUESTED",
        )?;
        let request = BrokerComboRequest::from_combo_order(&oms)?;
        self.combo_orders.insert(
            oms.order_id.clone(),
            PaperComboOrder {
                oms,
                broker_order_id: None,
                broker_order_versions: Vec::new(),
                market,
                filled_quantity: Decimal::ZERO,
                executions: BTreeMap::new(),
                evidence_error: None,
            },
        );
        self.combo_risk_evidence
            .insert(decision.decision_id.clone(), evidence);
        // Persist `PENDING_SUBMIT` before crossing the external broker boundary.
        self.persist()?;

        let submission = self.broker.submit_combo(&request);
        match submission {
            Ok(BrokerSubmitResult::Acknowledged { broker_order_id }) => {
                validate_canonical_id("broker order_id", &broker_order_id)?;
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "PAPER_COMBO_SUBMISSION_SENT")?;
                order
                    .oms
                    .transition(OrderState::Acknowledged, "IBKR_PAPER_COMBO_ACKNOWLEDGED")?;
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
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "PAPER_COMBO_SUBMISSION_SENT")?;
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
                let order = self.combo_order_mut(&request.client_order_id)?;
                order.oms.transition(OrderState::Unknown, reason)?;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(OrderState::Unknown),
                })
            }
            Err(error) => {
                // An adapter that cannot execute an atomic combination returns
                // an error here, and that is a *transport* outcome like any
                // other: the request may or may not have reached the venue, so
                // the combination goes to `UNKNOWN` and the session is marked
                // disconnected rather than assumed untouched.
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Unknown, "PAPER_COMBO_TRANSPORT_OUTCOME_UNKNOWN")?;
                self.broker_connected = false;
                self.persist()?;
                Err(error)
            }
        }
    }

    /// The durable combination order for one client identity, if known.
    pub fn combo_order(&self, order_id: &str) -> Option<&PaperComboOrder> {
        self.combo_orders.get(order_id)
    }

    /// The immutable combination risk evidence for one decision identity.
    pub fn combo_risk_evidence(&self, decision_id: &str) -> Option<&PaperComboRiskEvidence> {
        self.combo_risk_evidence.get(decision_id)
    }

    fn combo_order_mut(&mut self, order_id: &str) -> Result<&mut PaperComboOrder, PaperError> {
        self.combo_orders
            .get_mut(order_id)
            .ok_or_else(|| PaperError("paper OMS does not know combination order".to_owned()))
    }

    /// Requests cancellation. A transport failure leaves the order explicitly `UNKNOWN`.
    pub fn cancel_order(&mut self, order_id: &str) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        if self.combo_orders.contains_key(order_id) {
            return self.cancel_combo_order(order_id);
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

    /// Drains broker evidence, applies unique executions, and preserves every ambiguity.
    pub fn synchronize(&mut self) -> Result<usize, PaperError> {
        self.ensure_persistence_healthy()?;
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected".to_owned(),
            ));
        }
        let events = match self.broker.poll(&self.account.account_id) {
            Ok(events) => events,
            Err(error) => {
                self.broker_connected = false;
                self.persist()?;
                return Err(error);
            }
        };
        let count = events.len();
        let mut apply_errors = Vec::new();
        for event in events {
            // `poll` drains broker evidence, so an event that fails to apply can
            // never be re-fetched: aborting the whole loop on the first failure
            // would silently and permanently lose every event still queued
            // behind it. Snapshot the mutable state first so a failed
            // application leaves no partial mutation in memory, then keep
            // applying the remaining events in the batch.
            let orders_snapshot = self.orders.clone();
            let combo_snapshot = self.combo_orders.clone();
            let combo_id = event.client_order_id().to_owned();
            let portfolios_snapshot = self.portfolios.clone();
            let tax_lots_snapshot = self.tax_lots.clone();
            let strategy_attribution_snapshot = self.strategy_attribution.clone();
            let execution_ids_snapshot = self.execution_ids.clone();
            let cash_snapshot = self.cash;
            if let Err(error) = self.apply_broker_event(event) {
                self.orders = orders_snapshot;
                self.combo_orders = combo_snapshot;
                self.portfolios = portfolios_snapshot;
                self.tax_lots = tax_lots_snapshot;
                self.strategy_attribution = strategy_attribution_snapshot;
                self.execution_ids = execution_ids_snapshot;
                self.cash = cash_snapshot;
                if let Some(order) = self.combo_orders.get_mut(&combo_id) {
                    order.evidence_error = Some(error.0.clone());
                    if order.oms.state != OrderState::Unknown
                        && order.oms.state != OrderState::Filled
                    {
                        order
                            .oms
                            .transition(OrderState::Unknown, "COMBINATION_EXECUTION_ANOMALY")?;
                    }
                    self.persist()?;
                }
                apply_errors.push(error.0);
                continue;
            }
            // Broker evidence is append-only. Persist each arrival so resolving an
            // UNKNOWN or correcting an earlier terminal observation never rewrites
            // the durable history.
            self.persist()?;
        }
        if !apply_errors.is_empty() {
            return Err(PaperError(format!(
                "paper broker synchronize failed to apply {} of {count} broker events (each rolled back cleanly): {}",
                apply_errors.len(),
                apply_errors.join("; "),
            )));
        }
        Ok(count)
    }

    /// Reconnects, drains delayed broker evidence, then immediately reconciles.
    pub fn reconnect_and_reconcile(
        &mut self,
        reconciled_at: &str,
    ) -> Result<ReconciliationReport, PaperError> {
        self.ensure_persistence_healthy()?;
        if let Err(error) = self.broker.reconnect(&self.account.account_id) {
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
        let order_views = self
            .orders
            .iter()
            .map(|(id, o)| {
                (
                    id,
                    o.working(),
                    &o.broker_order_id,
                    &o.broker_order_versions,
                    o.filled_quantity,
                    o.oms.state,
                )
            })
            .chain(self.combo_orders.iter().map(|(id, o)| {
                (
                    id,
                    o.working(),
                    &o.broker_order_id,
                    &o.broker_order_versions,
                    o.filled_quantity,
                    o.oms.state,
                )
            }));
        for (order_id, working, broker_order_id, versions, filled_quantity, state) in order_views {
            match broker_orders.get(order_id.as_str()) {
                None if working => raw_issues.push((
                    "MISSING_BROKER_ORDER",
                    order_id.clone(),
                    "internal working order is absent from broker snapshot".to_owned(),
                )),
                Some(brokers) => {
                    let broker = broker_order_id.as_ref().and_then(|current| {
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
                                broker_order_id,
                                brokers
                                    .iter()
                                    .map(|candidate| candidate.broker_order_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                        ));
                    }
                    if brokers
                        .iter()
                        .any(|candidate| !versions.contains(&candidate.broker_order_id))
                    {
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
                    if filled_quantity != broker_filled {
                        raw_issues.push((
                            "FILLED_QUANTITY_MISMATCH",
                            order_id.clone(),
                            format!("internal={},broker={broker_filled}", filled_quantity,),
                        ));
                    }
                    if let Some(broker) = broker {
                        if state != broker.state {
                            raw_issues.push((
                                "ORDER_STATE_MISMATCH",
                                order_id.clone(),
                                format!(
                                    "internal={},broker={}",
                                    state.as_str(),
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
            if !self.orders.contains_key(&broker.client_order_id)
                && !self.combo_orders.contains_key(&broker.client_order_id)
            {
                raw_issues.push((
                    "UNEXPECTED_BROKER_ORDER",
                    broker.client_order_id.clone(),
                    "broker snapshot contains no matching internal order".to_owned(),
                ));
            }
        }
        for (order_id, internal) in &self.combo_orders {
            if let Some(error) = &internal.evidence_error {
                raw_issues.push((
                    "COMBINATION_EXECUTION_ANOMALY",
                    order_id.clone(),
                    error.clone(),
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
            working_orders: self.working_order_count() as u32,
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
            .breaches_position_limit(projected_position)?
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
            if let Some((decision, margin_used)) =
                self.portfolio_risk_decision(composition, intent, market, decided_at)?
            {
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
                    ",portfolio_risk_policy_version={},portfolio_gross_exposure={},portfolio_net_exposure={},portfolio_leverage_bps={},portfolio_concentration_bps={},portfolio_peak_equity={},portfolio_drawdown_bps={},portfolio_daily_baseline_equity={},portfolio_daily_pnl={},portfolio_margin_used={},portfolio_margin_utilization_bps={},portfolio_sector_gross={},portfolio_asset_class_gross={},portfolio_currency_gross={},portfolio_strategy_gross={}",
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
                "max_order_quantity={},max_order_notional={},max_price_deviation_bps={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},max_order_rate={},order_rate_window_seconds={},recent_order_count={},market_instrument_id={},market_mark_price={},market_observed_at={},requested_price={},requested_price_deviation_bps={},estimated_notional={},projected_position={},available_cash={},instrument_tick_size={},instrument_lot_size={}{}",
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
            if self.risk_policy.breaches_position_limit(projected)? {
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
    fn recent_order_count(&self, decided_at: &str) -> Result<u32, PaperError> {
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
    fn working_order_count(&self) -> usize {
        self.orders.values().filter(|order| order.working()).count()
            + self
                .combo_orders
                .values()
                .filter(|order| order.working())
                .count()
    }

    /// Whether any order of either kind is in the `UNKNOWN` safety state.
    fn has_unknown_order(&self) -> bool {
        self.orders
            .values()
            .any(|order| order.oms.state == OrderState::Unknown)
            || self.combo_orders.values().any(|order| {
                order.oms.state == OrderState::Unknown || order.evidence_error.is_some()
            })
    }

    /// Cash committed by every working order of either kind.
    fn total_reserved_cash(&self) -> Result<Decimal, PaperError> {
        let mut reserved = Decimal::ZERO;
        for order in self.orders.values() {
            reserved = reserved.checked_add(order.reserved_cash()?)?;
        }
        for order in self.combo_orders.values() {
            reserved = reserved.checked_add(order.reserved_cash()?)?;
        }
        Ok(reserved)
    }

    /// Whether a working order of either kind would trade against `side` on
    /// `instrument_id`.
    ///
    /// A combination's legs count individually here: a working short leg is a
    /// real resting sell on that instrument however the group is labelled, and
    /// a plain buy submitted against it is the same self-trade it would be
    /// against a plain sell.
    fn conflicts_with_working_order(&self, instrument_id: &str, side: Side) -> bool {
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

    /// Real point-in-time equity: cash plus every non-zero position marked at
    /// its cached observed mark, falling back to average cost when this
    /// instrument has never been independently quoted. Shared by peak-equity
    /// tracking (always) and the Slice-1/2 aggregate-risk snapshot (when
    /// composed).
    fn current_equity(&self) -> Result<Decimal, PaperError> {
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
    /// computed equity is not yet positive (e.g. a brand-new, zero-funded,
    /// zero-position account) -- a benign boundary condition, not an error;
    /// the existing per-order checks still apply on their own. On `Some`, the
    /// second tuple element is the real margin requirement computed for the
    /// decision (`Decimal::ZERO` when `margin_rates` is not configured),
    /// returned alongside the decision because `core/risk::AggregateRiskMetrics`
    /// only ever reports the *ratio* (`margin_utilization_bps`), not the raw
    /// currency amount that produced it.
    fn portfolio_risk_decision(
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
            resting_orders,
            recent_order_count: 0,
        };
        Ok(Some((snapshot, margin_used)))
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
    fn combo_portfolio_risk_decision(
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

    fn apply_broker_event(&mut self, event: BrokerEvent) -> Result<(), PaperError> {
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
    fn apply_tax_lot_fill(&mut self, fill: &Fill) -> Result<(), PaperError> {
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
    fn apply_strategy_attribution_fill(
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
        let combo_orders = self
            .combo_orders
            .iter()
            .map(|(order_id, order)| {
                (
                    order_id.clone(),
                    PersistentComboOrder {
                        intent: PersistentComboIntent::from(&order.oms.intent),
                        state: order.oms.state.as_str().to_owned(),
                        broker_order_id: order.broker_order_id.clone(),
                        broker_order_versions: order.broker_order_versions.clone(),
                        execution_state: order.execution_state(),
                        market: order
                            .market
                            .marks
                            .iter()
                            .map(PersistentMarketData::from)
                            .collect(),
                    },
                )
            })
            .collect();
        let combo_risk_evidence = self
            .combo_risk_evidence
            .iter()
            .map(|(decision_id, evidence)| {
                (
                    decision_id.clone(),
                    PersistentComboRiskEvidence::from(evidence),
                )
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
        let tax_lot_snapshot = self.tax_lots.snapshot();
        let tax_lots = PersistentTaxLotBook {
            short_lots: tax_lot_snapshot
                .short_lots
                .into_iter()
                .map(|(instrument, lots)| {
                    (
                        instrument,
                        lots.into_iter()
                            .map(|lot| PersistentTaxLot {
                                lot_id: lot.lot_id,
                                opened_at: lot.opened_at,
                                remaining_quantity: lot.remaining_quantity.to_string(),
                                unit_cost: lot.unit_proceeds.to_string(),
                            })
                            .collect(),
                    )
                })
                .collect(),
            applied_short_lot_ids: tax_lot_snapshot.applied_short_lot_ids.into_iter().collect(),
            applied_cover_ids: tax_lot_snapshot.applied_cover_ids.into_iter().collect(),
            lots: tax_lot_snapshot
                .lots
                .into_iter()
                .map(|(instrument_id, lots)| {
                    let persisted = lots
                        .into_iter()
                        .map(|lot| PersistentTaxLot {
                            lot_id: lot.lot_id,
                            opened_at: lot.opened_at,
                            remaining_quantity: lot.remaining_quantity.to_string(),
                            unit_cost: lot.unit_cost.to_string(),
                        })
                        .collect();
                    (instrument_id, persisted)
                })
                .collect(),
            applied_lot_ids: tax_lot_snapshot.applied_lot_ids.into_iter().collect(),
            applied_disposal_ids: tax_lot_snapshot.applied_disposal_ids.into_iter().collect(),
            realized_by_currency: tax_lot_snapshot
                .realized_by_currency
                .into_iter()
                .map(|(currency, amount)| (currency.as_str().to_owned(), amount.to_string()))
                .collect(),
        };
        PersistentPaperState {
            configuration_fingerprint: self.configuration_fingerprint(),
            account_id: self.account.account_id.clone(),
            currency: self.account.currency.clone(),
            cash: self.cash.to_string(),
            orders,
            risk_evidence,
            combo_orders,
            combo_risk_evidence,
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
            tax_lots,
            marks: self
                .marks
                .iter()
                .map(|(instrument_id, mark)| (instrument_id.clone(), mark.to_string()))
                .collect(),
            peak_equity: Some(self.peak_equity.to_string()),
            daily_baseline_date: self.daily_baseline_date.clone(),
            daily_baseline_equity: self
                .daily_baseline_date
                .as_ref()
                .map(|_| self.daily_baseline_equity.to_string()),
            strategy_attribution: self
                .strategy_attribution
                .iter()
                .map(|(instrument_id, strategies)| {
                    (
                        instrument_id.clone(),
                        strategies
                            .iter()
                            .map(|(strategy_id, quantity)| {
                                (strategy_id.clone(), quantity.to_string())
                            })
                            .collect(),
                    )
                })
                .collect(),
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
        let mut combo_orders = BTreeMap::new();
        for (order_id, persisted) in state.combo_orders {
            let intent = ComboIntent::try_from(persisted.intent)?;
            if intent.account_id != self.account.account_id || intent.environment != "PAPER" {
                return Err(PaperError(
                    "persisted paper combination has an incompatible account or environment"
                        .to_owned(),
                ));
            }
            let mut broker_order_versions = persisted.broker_order_versions;
            if let Some(broker_order_id) = &persisted.broker_order_id {
                validate_canonical_id("persisted combo broker_order_id", broker_order_id)?;
                if broker_order_versions.is_empty() {
                    broker_order_versions.push(broker_order_id.clone());
                }
            }
            let mut unique_versions = BTreeSet::new();
            for broker_order_id in &broker_order_versions {
                validate_canonical_id("persisted combo broker_order_version", broker_order_id)?;
                if !unique_versions.insert(broker_order_id) {
                    return Err(PaperError(
                        "persisted combination broker order versions are duplicated".to_owned(),
                    ));
                }
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if !unique_versions.contains(broker_order_id) {
                    return Err(PaperError(
                        "persisted active combination broker ID is absent from versions".to_owned(),
                    ));
                }
            }
            let market = combo_market_from_persisted(persisted.market)?;
            // The observation must still price exactly the legs it was stored
            // against. A journal that lost a leg's mark, or gained one, cannot
            // reproduce the decision that approved the combination.
            market.validate_for(&intent)?;
            let oms = OmsComboOrder::recover(
                order_id.clone(),
                intent,
                parse_order_state(&persisted.state)?,
            )?;
            let mut order = PaperComboOrder {
                oms,
                broker_order_id: persisted.broker_order_id,
                broker_order_versions,
                market,
                filled_quantity: Decimal::ZERO,
                executions: BTreeMap::new(),
                evidence_error: None,
            };
            order.restore_executions(persisted.execution_state)?;
            combo_orders.insert(order_id, order);
        }
        let mut combo_risk_evidence = BTreeMap::new();
        for (decision_id, persisted) in state.combo_risk_evidence {
            validate_canonical_id("persisted combo risk decision_id", &decision_id)?;
            let evidence = PaperComboRiskEvidence::try_from(persisted)?;
            if decision_id != evidence.decision.decision_id {
                return Err(PaperError(
                    "persisted combination risk decision identity does not match its key"
                        .to_owned(),
                ));
            }
            if evidence.decision.policy_version != self.risk_policy.version
                || evidence.decision.actor != "paper_risk_engine"
            {
                return Err(PaperError(
                    "persisted combination risk evidence is incompatible with supplied policy"
                        .to_owned(),
                ));
            }
            if combo_risk_evidence.contains_key(&decision_id) {
                return Err(PaperError(
                    "duplicate persisted combination risk evidence".to_owned(),
                ));
            }
            combo_risk_evidence.insert(decision_id, evidence);
        }
        for order in combo_orders.values() {
            let decision_id = format!("paper-combo-risk-{}", order.oms.intent.intent_id);
            let evidence = combo_risk_evidence.get(&decision_id).ok_or_else(|| {
                PaperError("persisted paper combination is missing risk evidence".to_owned())
            })?;
            if !evidence.decision.approved
                || evidence.intent != order.oms.intent
                || evidence.market != order.market
            {
                return Err(PaperError(
                    "persisted paper combination does not match approved risk evidence".to_owned(),
                ));
            }
        }
        let mut portfolios = BTreeMap::new();
        for (instrument_id, persisted) in state.positions {
            portfolios.insert(
                instrument_id.clone(),
                if self.risk_policy.short_exposure.is_some() {
                    Portfolio::recover_signed
                } else {
                    Portfolio::recover
                }(
                    &self.account.account_id,
                    instrument_id,
                    decimal("persisted position quantity", &persisted.quantity)?,
                    decimal("persisted average cost", &persisted.average_cost)?,
                    decimal("persisted realized pnl", &persisted.realized_pnl)?,
                )?,
            );
        }
        let account_currency = Currency::new(self.account.currency.clone())?;
        let mut lots = BTreeMap::new();
        for (instrument_id, persisted_lots) in state.tax_lots.lots {
            let mut instrument_lots = Vec::with_capacity(persisted_lots.len());
            for persisted in persisted_lots {
                instrument_lots.push(TaxLot {
                    lot_id: persisted.lot_id,
                    instrument_id: instrument_id.clone(),
                    currency: account_currency.clone(),
                    opened_at: persisted.opened_at,
                    remaining_quantity: decimal(
                        "persisted tax lot remaining quantity",
                        &persisted.remaining_quantity,
                    )?,
                    unit_cost: decimal("persisted tax lot unit cost", &persisted.unit_cost)?,
                });
            }
            lots.insert(instrument_id, instrument_lots);
        }
        let mut realized_by_currency = BTreeMap::new();
        for (currency, amount) in state.tax_lots.realized_by_currency {
            realized_by_currency.insert(
                Currency::new(currency)?,
                decimal("persisted realized tax pnl", &amount)?,
            );
        }
        let tax_lots = TaxLotBook::recover(TaxLotBookSnapshot {
            lots,
            applied_lot_ids: state.tax_lots.applied_lot_ids.into_iter().collect(),
            applied_disposal_ids: state.tax_lots.applied_disposal_ids.into_iter().collect(),
            realized_by_currency,
            short_lots: state
                .tax_lots
                .short_lots
                .into_iter()
                .map(|(instrument_id, lots)| {
                    let lots = lots
                        .into_iter()
                        .map(|lot| {
                            Ok(ShortTaxLot {
                                lot_id: lot.lot_id,
                                instrument_id: instrument_id.clone(),
                                currency: account_currency.clone(),
                                opened_at: lot.opened_at,
                                remaining_quantity: decimal(
                                    "short lot quantity",
                                    &lot.remaining_quantity,
                                )?,
                                unit_proceeds: decimal("short lot proceeds", &lot.unit_cost)?,
                            })
                        })
                        .collect::<Result<Vec<_>, PaperError>>()?;
                    Ok((instrument_id, lots))
                })
                .collect::<Result<_, PaperError>>()?,
            applied_short_lot_ids: state.tax_lots.applied_short_lot_ids.into_iter().collect(),
            applied_cover_ids: state.tax_lots.applied_cover_ids.into_iter().collect(),
        })?;
        let mut execution_ids = BTreeSet::new();
        for execution_id in state.execution_ids {
            validate_canonical_id("persisted execution_id", &execution_id)?;
            if !execution_ids.insert(execution_id) {
                return Err(PaperError(
                    "paper journal contains duplicate execution identity".to_owned(),
                ));
            }
        }
        let mut combo_execution_ids = BTreeSet::new();
        for order in combo_orders.values() {
            for execution in order.executions.values() {
                for id in std::iter::once(&execution.execution_id)
                    .chain(execution.legs.iter().map(|leg| &leg.execution_id))
                {
                    if !execution_ids.contains(id) || !combo_execution_ids.insert(id) {
                        return Err(PaperError("persisted combination execution is missing or duplicated in account receipts".to_owned()));
                    }
                }
            }
        }
        let mut marks = BTreeMap::new();
        for (instrument_id, mark) in state.marks {
            validate_canonical_id("persisted mark instrument_id", &instrument_id)?;
            let mark = decimal("persisted mark price", &mark)?;
            if mark <= Decimal::ZERO {
                return Err(PaperError("persisted mark price is invalid".to_owned()));
            }
            marks.insert(instrument_id, mark);
        }
        let mut strategy_attribution = BTreeMap::new();
        for (instrument_id, strategies) in state.strategy_attribution {
            validate_canonical_id("persisted attribution instrument_id", &instrument_id)?;
            let mut parsed_strategies = BTreeMap::new();
            for (strategy_id, quantity) in strategies {
                validate_canonical_id("persisted attribution strategy_id", &strategy_id)?;
                // Deliberately signed, not required positive: a strategy's
                // own tracked contribution can legitimately be negative (see
                // `apply_strategy_attribution_fill`).
                let quantity = decimal("persisted attribution quantity", &quantity)?;
                parsed_strategies.insert(strategy_id, quantity);
            }
            strategy_attribution.insert(instrument_id, parsed_strategies);
        }
        let persisted_peak_equity = state
            .peak_equity
            .map(|value| decimal("persisted peak equity", &value))
            .transpose()?;
        if persisted_peak_equity.is_some_and(|value| value <= Decimal::ZERO) {
            return Err(PaperError("persisted peak equity is invalid".to_owned()));
        }
        let persisted_daily_baseline_equity = state
            .daily_baseline_equity
            .map(|value| decimal("persisted daily-loss baseline equity", &value))
            .transpose()?;
        match (&state.daily_baseline_date, &persisted_daily_baseline_equity) {
            (Some(date), Some(_)) => validate_exchange_date(date)?,
            (None, None) => {}
            _ => {
                return Err(PaperError(
                    "persisted daily-loss baseline date and equity must be present together"
                        .to_owned(),
                ))
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
        self.combo_orders = combo_orders;
        self.combo_risk_evidence = combo_risk_evidence;
        self.portfolios = portfolios;
        self.tax_lots = tax_lots;
        self.marks = marks;
        self.strategy_attribution = strategy_attribution;
        self.execution_ids = execution_ids;
        self.kill_switches = restored_switches;
        self.incidents = incidents;
        self.last_reconciled_at = state.last_reconciled_at;
        self.last_reconciliation_clean = state.last_reconciliation_clean;
        self.latest_reconciliation = latest_reconciliation;
        self.paper_days = state.paper_days;
        self.next_reconciliation = state.next_reconciliation;
        self.broker_connected = state.broker_connected;
        // `cash`/`portfolios`/`marks` are already restored above, so
        // `current_equity()` reflects real recovered state here. A journal
        // that never tracked peak equity (`persisted_peak_equity` absent)
        // bootstraps its peak to that real equity -- the honest value for a
        // peak that starts being tracked only from this reopen onward.
        let recovered_equity = self.current_equity()?;
        self.peak_equity = persisted_peak_equity
            .unwrap_or(recovered_equity)
            .max(recovered_equity);
        // Unlike peak equity, a daily-loss baseline is never maxed against
        // recovered equity: it is either the exact value durably persisted
        // from earlier the same UTC day, or (absent -- a legacy journal, or
        // one that has never evaluated risk) left unset so the very next
        // `evaluate_risk` call establishes an honest fresh baseline from real
        // recovered state.
        self.daily_baseline_date = state.daily_baseline_date;
        self.daily_baseline_equity = persisted_daily_baseline_equity.unwrap_or(Decimal::ZERO);
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
        let max_order_rate = self.risk_policy.max_order_rate.to_string();
        let order_rate_window_seconds = self.risk_policy.order_rate_window_seconds.to_string();
        // Absent for every configuration that does not opt into Slice-1
        // aggregate-risk composition, so this leaves the fingerprint of an
        // unconfigured operator byte-for-byte unchanged -- the same
        // conditional-append convention already used for
        // `broker_route_fingerprint` below.
        let portfolio_risk_parts: Vec<String> = self
            .risk_policy
            .portfolio_risk
            .as_ref()
            .map(|composition| {
                vec![
                    composition.policy.version.clone(),
                    composition.policy.max_gross_exposure.to_string(),
                    composition.policy.max_abs_net_exposure.to_string(),
                    composition.policy.max_leverage_bps.to_string(),
                    composition.policy.max_concentration_bps.to_string(),
                    render_bucket_map(&composition.policy.sector_limits),
                    render_bucket_map(&composition.policy.asset_class_limits),
                    render_bucket_map(&composition.policy.currency_limits),
                    composition
                        .policy
                        .allowed_instruments
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("|"),
                    composition
                        .policy
                        .restricted_instruments
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("|"),
                    composition
                        .instrument_buckets
                        .iter()
                        .map(|(instrument_id, bucket)| {
                            format!(
                                "{instrument_id}:{}:{}:{}",
                                bucket.asset_class, bucket.currency, bucket.sector
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("|"),
                ]
            })
            .unwrap_or_default();
        let mut parts = vec![
            "paper-configuration-v3",
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
            &max_order_rate,
            &order_rate_window_seconds,
            &self.kill_switches.version,
        ];
        let short_bound = self
            .risk_policy
            .short_exposure
            .as_ref()
            .map(|policy| policy.max_short_quantity.to_string());
        if let Some(bound) = &short_bound {
            parts.push("paper-short-exposure-v1");
            parts.push(bound);
        }
        if !self.broker_route_fingerprint.is_empty() {
            parts.push("paper-broker-route-fingerprint-v1");
            parts.push(&self.broker_route_fingerprint);
        }
        if !portfolio_risk_parts.is_empty() {
            parts.push("paper-portfolio-risk-v1");
            for part in &portfolio_risk_parts {
                parts.push(part);
            }
        }
        // Always present: every configuration now lists its tick sizes, and a
        // journal or decision made under one tick table must not be reopened
        // under another.
        let tick_sizes = render_instrument_table(&self.risk_policy.instrument_tick_sizes);
        parts.push("paper-instrument-ticks-v1");
        parts.push(&tick_sizes);
        // Likewise the lot table (E3.6c).
        let lot_sizes = render_instrument_table(&self.risk_policy.instrument_lot_sizes);
        parts.push("paper-instrument-lots-v1");
        parts.push(&lot_sizes);
        hash_fingerprint_parts(&parts)
    }

    fn unexplained_incident_count(&self) -> u32 {
        self.incidents
            .values()
            .filter(|incident| incident.unexplained())
            .count() as u32
    }
}

/// A per-instrument reference table (ticks or lots) in its canonical,
/// instrument-ordered form, for the configuration fingerprint.
fn render_instrument_table(table: &BTreeMap<String, Decimal>) -> String {
    table
        .iter()
        .map(|(instrument_id, value)| format!("{instrument_id}:{value}"))
        .collect::<Vec<_>>()
        .join("|")
}

/// Each combination leg's entry in a per-instrument reference table, in leg
/// order, with `UNCONFIGURED` for an instrument the table does not list.
fn render_leg_entries(intent: &ComboIntent, table: &BTreeMap<String, Decimal>) -> String {
    intent
        .legs
        .iter()
        .map(|leg| {
            format!(
                "{}:{}",
                leg.instrument_id,
                table
                    .get(&leg.instrument_id)
                    .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn hash_fingerprint_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
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

impl From<&ComboIntent> for PersistentComboIntent {
    fn from(intent: &ComboIntent) -> Self {
        Self {
            intent_id: intent.intent_id.clone(),
            account_id: intent.account_id.clone(),
            strategy_id: intent.strategy_id.clone(),
            correlation_id: intent.correlation_id.clone(),
            legs: intent
                .legs
                .iter()
                .map(|leg| PersistentComboLeg {
                    instrument_id: leg.instrument_id.clone(),
                    side: leg.side.as_str().to_owned(),
                    ratio: leg.ratio,
                    limit_price: leg.limit_price.to_string(),
                })
                .collect(),
            combo_quantity: intent.combo_quantity.to_string(),
            price_limit_kind: intent.price_limit.kind().to_owned(),
            price_limit_amount: intent.price_limit.amount().to_string(),
            time_in_force: intent.time_in_force.as_str().to_owned(),
            rationale: intent.rationale.clone(),
            created_at: intent.created_at.clone(),
            strategy_version: intent.strategy_version.clone(),
            configuration_version: intent.configuration_version.clone(),
            environment: intent.environment.clone(),
        }
    }
}

impl TryFrom<PersistentComboIntent> for ComboIntent {
    type Error = PaperError;

    fn try_from(intent: PersistentComboIntent) -> Result<Self, Self::Error> {
        let amount = decimal("persisted combo price limit", &intent.price_limit_amount)?;
        let mut legs = Vec::with_capacity(intent.legs.len());
        for leg in intent.legs {
            legs.push(follon_domain::ComboIntentLeg {
                instrument_id: leg.instrument_id,
                side: match leg.side.as_str() {
                    "BUY" => Side::Buy,
                    "SELL" => Side::Sell,
                    _ => return Err(PaperError("persisted combo leg side is invalid".to_owned())),
                },
                ratio: leg.ratio,
                limit_price: decimal("persisted combo leg limit price", &leg.limit_price)?,
            });
        }
        let result = Self {
            intent_id: intent.intent_id,
            account_id: intent.account_id,
            strategy_id: intent.strategy_id,
            correlation_id: intent.correlation_id,
            legs,
            combo_quantity: decimal("persisted combo quantity", &intent.combo_quantity)?,
            price_limit: match intent.price_limit_kind.as_str() {
                "MAXIMUM_DEBIT" => follon_domain::ComboPriceLimit::MaximumDebit(amount),
                "MINIMUM_CREDIT" => follon_domain::ComboPriceLimit::MinimumCredit(amount),
                _ => {
                    return Err(PaperError(
                        "persisted combo price limit kind is invalid".to_owned(),
                    ))
                }
            },
            time_in_force: match intent.time_in_force.as_str() {
                "DAY" => TimeInForce::Day,
                "GTC" => TimeInForce::GoodTilCancelled,
                _ => {
                    return Err(PaperError(
                        "persisted combo time in force is invalid".to_owned(),
                    ))
                }
            },
            rationale: intent.rationale,
            created_at: intent.created_at,
            strategy_version: intent.strategy_version,
            configuration_version: intent.configuration_version,
            environment: intent.environment,
        };
        // Re-validated on the way back in, not trusted because it was once
        // written: a journal is a file on disk and may have been edited.
        result.validate()?;
        Ok(result)
    }
}

impl From<&PaperComboRiskEvidence> for PersistentComboRiskEvidence {
    fn from(evidence: &PaperComboRiskEvidence) -> Self {
        Self {
            intent: PersistentComboIntent::from(&evidence.intent),
            approved: evidence.decision.approved,
            reason_codes: evidence.decision.reason_codes.clone(),
            policy_version: evidence.decision.policy_version.clone(),
            decided_at: evidence.decision.decided_at.clone(),
            correlation_id: evidence.decision.correlation_id.clone(),
            actor: evidence.decision.actor.clone(),
            evaluated_limits: evidence.decision.evaluated_limits.clone(),
            market: evidence
                .market
                .marks
                .iter()
                .map(PersistentMarketData::from)
                .collect(),
            submitted_by: evidence.submitted_by.clone(),
        }
    }
}

impl TryFrom<PersistentComboRiskEvidence> for PaperComboRiskEvidence {
    type Error = PaperError;

    fn try_from(evidence: PersistentComboRiskEvidence) -> Result<Self, Self::Error> {
        let intent = ComboIntent::try_from(evidence.intent)?;
        let market = combo_market_from_persisted(evidence.market)?;
        if let Some(operator) = &evidence.submitted_by {
            validate_canonical_id("persisted paper combo submitted_by", operator)?;
        }
        Ok(Self {
            decision: RiskDecision {
                decision_id: format!("paper-combo-risk-{}", intent.intent_id),
                intent_id: intent.intent_id.clone(),
                approved: evidence.approved,
                reason_codes: evidence.reason_codes,
                policy_version: evidence.policy_version,
                decided_at: evidence.decided_at,
                correlation_id: evidence.correlation_id,
                actor: evidence.actor,
                evaluated_limits: evidence.evaluated_limits,
            },
            intent,
            market,
            submitted_by: evidence.submitted_by,
        })
    }
}

fn combo_market_from_persisted(
    marks: Vec<PersistentMarketData>,
) -> Result<PaperComboMarketData, PaperError> {
    let mut restored = Vec::with_capacity(marks.len());
    for mark in marks {
        restored.push(PaperMarketData::try_from(mark)?);
    }
    Ok(PaperComboMarketData { marks: restored })
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
    include!("combo_lifecycle_tests.rs");
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
            max_order_rate: 20,
            order_rate_window_seconds: 60,
            portfolio_risk: None,
            short_exposure: None,
            instrument_tick_sizes: test_tick_sizes(),
            instrument_lot_sizes: test_lot_sizes(),
        }
    }

    fn test_tick_sizes() -> BTreeMap<String, Decimal> {
        [
            "inst.us_equity.spy",
            "inst.us_equity.qqq",
            "inst.us_option.spy.near",
            "inst.us_option.spy.far",
        ]
        .into_iter()
        .map(|instrument| (instrument.to_owned(), decimal("tick", "0.01").unwrap()))
        .collect()
    }

    /// A lot of one for each listed instrument: every whole quantity passes,
    /// so only a test that sets a coarser lot exercises the lot rule.
    fn test_lot_sizes() -> BTreeMap<String, Decimal> {
        test_tick_sizes()
            .into_keys()
            .map(|instrument| (instrument, decimal("lot", "1").unwrap()))
            .collect()
    }

    /// The same policy with net short exposure explicitly permitted, bounded at
    /// 1,000 per instrument. Combination tests that need a short leg use this;
    /// none of them may quietly widen `policy()` instead.
    fn policy_permitting_shorts() -> PaperRiskPolicy {
        PaperRiskPolicy {
            short_exposure: Some(ShortExposurePolicy {
                max_short_quantity: decimal("short bound", "1000").unwrap(),
            }),
            ..policy()
        }
    }

    fn service_permitting_shorts() -> PaperTradingService<IbkrPaperAdapter> {
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        PaperTradingService::new(
            account,
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            adapter,
        )
        .unwrap()
    }

    fn service() -> PaperTradingService<IbkrPaperAdapter> {
        service_with(policy())
    }

    fn service_with(policy: PaperRiskPolicy) -> PaperTradingService<IbkrPaperAdapter> {
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        PaperTradingService::new(
            account,
            policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            adapter,
        )
        .unwrap()
    }

    fn broker_registry() -> PaperBrokerRegistry {
        let account = account();
        let second_account = PaperAccount {
            account_id: "acct.paper.002".to_owned(),
            currency: "USD".to_owned(),
            initial_cash: decimal("initial cash", "25000").unwrap(),
            environment: "PAPER".to_owned(),
        };
        let mut registry = PaperBrokerRegistry::new();
        registry
            .register(
                PaperBrokerRoute {
                    account_id: second_account.account_id.clone(),
                    adapter_id: "adapter.ibkr.paper.002".to_owned(),
                    venue_id: "venue.ibkr.paper".to_owned(),
                    environment: "PAPER".to_owned(),
                },
                Box::new(IbkrPaperAdapter::new(&second_account).unwrap()),
            )
            .unwrap();
        registry
            .register(
                PaperBrokerRoute {
                    account_id: account.account_id.clone(),
                    adapter_id: "adapter.ibkr.paper.001".to_owned(),
                    venue_id: "venue.ibkr.paper".to_owned(),
                    environment: "PAPER".to_owned(),
                },
                Box::new(IbkrPaperAdapter::new(&account).unwrap()),
            )
            .unwrap();
        registry
    }

    fn registry_service() -> PaperTradingService<PaperBrokerRegistry> {
        PaperTradingService::new(
            account(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            broker_registry(),
        )
        .unwrap()
    }

    fn modern_registry(account: &PaperAccount, venue_id: &str) -> PaperBrokerRegistry {
        let mut registry = PaperBrokerRegistry::new();
        registry
            .register(
                PaperBrokerRoute {
                    account_id: account.account_id.clone(),
                    adapter_id: format!("adapter.ibkr.paper.{}", account.account_id),
                    venue_id: venue_id.to_owned(),
                    environment: "PAPER".to_owned(),
                },
                Box::new(IbkrPaperAdapter::new(account).unwrap()),
            )
            .unwrap();
        registry
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

    /// A long call vertical on two distinct option instruments: buy the near
    /// strike at 7.50, sell the far strike at 5.00, for a 2.50 net debit per
    /// combination unit.
    fn combo_intent(intent_id: &str, created_at: &str) -> ComboIntent {
        ComboIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.paper.001".to_owned(),
            strategy_id: "strategy.paper.001".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            legs: vec![
                follon_domain::ComboIntentLeg {
                    instrument_id: "inst.us_option.spy.near".to_owned(),
                    side: Side::Buy,
                    ratio: 1,
                    limit_price: decimal("near", "7.50").unwrap(),
                },
                follon_domain::ComboIntentLeg {
                    instrument_id: "inst.us_option.spy.far".to_owned(),
                    side: Side::Sell,
                    ratio: 1,
                    limit_price: decimal("far", "5").unwrap(),
                },
            ],
            combo_quantity: decimal("units", "4").unwrap(),
            price_limit: follon_domain::ComboPriceLimit::MaximumDebit(
                decimal("cap", "2.50").unwrap(),
            ),
            time_in_force: TimeInForce::Day,
            rationale: "paper combo acceptance test".to_owned(),
            created_at: created_at.to_owned(),
            strategy_version: "strategy-paper-v1".to_owned(),
            configuration_version: "config-paper-v1".to_owned(),
            environment: "PAPER".to_owned(),
        }
    }

    /// Marks that sit exactly on each leg's own limit price, so the per-leg
    /// price collar reads zero deviation unless a test moves one.
    fn combo_market(observed_at: &str) -> PaperComboMarketData {
        PaperComboMarketData {
            marks: vec![
                PaperMarketData {
                    instrument_id: "inst.us_option.spy.near".to_owned(),
                    mark_price: decimal("mark", "7.50").unwrap(),
                    observed_at: observed_at.to_owned(),
                },
                PaperMarketData {
                    instrument_id: "inst.us_option.spy.far".to_owned(),
                    mark_price: decimal("mark", "5").unwrap(),
                    observed_at: observed_at.to_owned(),
                },
            ],
        }
    }

    #[test]
    fn combo_risk_approves_a_priced_vertical_and_records_exact_evidence() {
        let mut service = service_permitting_shorts();
        let decision = service
            .evaluate_combo_risk(
                &combo_intent("combo-000001", "2026-01-02T14:30:00Z"),
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(decision.approved, "{:?}", decision.reason_codes);
        assert_eq!(decision.reason_codes, vec!["APPROVED".to_owned()]);
        // A combination decision must never collide with a plain order's.
        assert_eq!(decision.decision_id, "paper-combo-risk-combo-000001");
        assert!(decision.evaluated_limits.contains("combo_legs=2"));
        assert!(decision
            .evaluated_limits
            .contains("combo_price_limit_kind=MAXIMUM_DEBIT"));
        // 4 units * (1 * 7.50 + 1 * 5.00) = 50 gross; net debit 4 * 2.50 = 10.
        assert!(decision
            .evaluated_limits
            .contains("combo_gross_notional=50.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("combo_net_debit=10.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("combo_protected_net_price=2.50000000"));
    }

    #[test]
    fn combo_legs_meet_the_plain_order_tick_rule_and_the_net_the_finest_grid() {
        let decide = |ticks: &[(&str, &str)], near: &str, far: &str, cap: &str| {
            let mut service = service_permitting_shorts();
            service
                .risk_policy
                .instrument_tick_sizes
                .remove("inst.us_option.spy.near");
            service
                .risk_policy
                .instrument_tick_sizes
                .remove("inst.us_option.spy.far");
            for (instrument, tick) in ticks {
                service
                    .risk_policy
                    .instrument_tick_sizes
                    .insert((*instrument).to_owned(), decimal("tick", tick).unwrap());
            }
            let mut intent = combo_intent("combo-000090", "2026-01-02T14:30:00Z");
            intent.legs[0].limit_price = decimal("near", near).unwrap();
            intent.legs[1].limit_price = decimal("far", far).unwrap();
            intent.price_limit =
                follon_domain::ComboPriceLimit::MaximumDebit(decimal("cap", cap).unwrap());
            let mut market = combo_market("2026-01-02T14:30:00Z");
            market.marks[0].mark_price = decimal("mark", near).unwrap();
            market.marks[1].mark_price = decimal("mark", far).unwrap();
            service
                .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z")
                .unwrap()
        };
        let near = "inst.us_option.spy.near";
        let far = "inst.us_option.spy.far";

        // On every grid: approved, with each leg's tick in the evidence.
        let on_grid = decide(&[(near, "0.05"), (far, "0.01")], "7.50", "5", "2.51");
        assert!(on_grid.approved, "{:?}", on_grid.reason_codes);
        assert!(on_grid.evaluated_limits.contains(
            "combo_tick_sizes=[inst.us_option.spy.near:0.05000000|inst.us_option.spy.far:0.01000000]"
        ));
        // An unlisted leg is refused exactly as a plain order on it would be.
        let unlisted = decide(&[(near, "0.01")], "7.50", "5", "2.50");
        assert!(!unlisted.approved);
        assert!(unlisted
            .reason_codes
            .contains(&"INSTRUMENT_TICK_SIZE_UNCONFIGURED".to_owned()));
        assert!(unlisted
            .evaluated_limits
            .contains("inst.us_option.spy.far:UNCONFIGURED"));
        // A leg price off its own grid.
        let off_leg = decide(&[(near, "0.05"), (far, "0.01")], "7.52", "5.02", "2.50");
        assert_eq!(
            off_leg.reason_codes,
            vec!["LIMIT_PRICE_OFF_TICK_GRID".to_owned()]
        );
        // A net limit off the finest leg grid, with every leg on its own.
        let off_net = decide(&[(near, "0.01"), (far, "0.01")], "7.50", "5", "2.505");
        assert_eq!(
            off_net.reason_codes,
            vec!["COMBO_NET_PRICE_OFF_TICK_GRID".to_owned()]
        );
        // The finest grid binds: a cent-stepped net against two nickel legs is refused.
        let coarse = decide(&[(near, "0.05"), (far, "0.05")], "7.50", "5", "2.51");
        assert_eq!(
            coarse.reason_codes,
            vec!["COMBO_NET_PRICE_OFF_TICK_GRID".to_owned()]
        );
    }

    #[test]
    fn combo_leg_quantities_meet_the_plain_order_lot_rule() {
        let near = "inst.us_option.spy.near";
        let far = "inst.us_option.spy.far";
        // The near leg carries a ratio of 2 at 6.00 and the far leg a ratio of
        // 1 at 5.00, a 7.00 net debit: each unit sends two near contracts and
        // one far contract to the broker.
        let decide = |lots: &[(&str, &str)], units: &str| {
            let mut service = service_permitting_shorts();
            service.risk_policy.instrument_lot_sizes.remove(near);
            service.risk_policy.instrument_lot_sizes.remove(far);
            for (instrument, lot) in lots {
                service
                    .risk_policy
                    .instrument_lot_sizes
                    .insert((*instrument).to_owned(), decimal("lot", lot).unwrap());
            }
            let mut intent = combo_intent("combo-000091", "2026-01-02T14:30:00Z");
            intent.combo_quantity = decimal("units", units).unwrap();
            intent.legs[0].ratio = 2;
            intent.legs[0].limit_price = decimal("near", "6").unwrap();
            intent.price_limit =
                follon_domain::ComboPriceLimit::MaximumDebit(decimal("cap", "7").unwrap());
            let mut market = combo_market("2026-01-02T14:30:00Z");
            market.marks[0].mark_price = decimal("mark", "6").unwrap();
            service
                .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z")
                .unwrap()
        };

        // Two units send four near and two far contracts: whole lots of 4 and
        // 2. Two units are not a whole near lot, so only each leg's own
        // quantity can approve this.
        let on_lot = decide(&[(near, "4"), (far, "2")], "2");
        assert!(on_lot.approved, "{:?}", on_lot.reason_codes);
        assert!(on_lot.evaluated_limits.contains(
            "combo_lot_sizes=[inst.us_option.spy.near:4.00000000|inst.us_option.spy.far:2.00000000]"
        ));
        // One unit sends a single far contract against a lot of 2.
        let off_lot = decide(&[(near, "2"), (far, "2")], "1");
        assert_eq!(
            off_lot.reason_codes,
            vec!["ORDER_QUANTITY_OFF_LOT_SIZE".to_owned()]
        );
        // A leg with no lot size is refused exactly as a plain order on it would be.
        let unlisted = decide(&[(near, "1")], "2");
        assert_eq!(
            unlisted.reason_codes,
            vec!["INSTRUMENT_LOT_SIZE_UNCONFIGURED".to_owned()]
        );
        assert!(unlisted.evaluated_limits.contains(
            "combo_lot_sizes=[inst.us_option.spy.near:1.00000000|inst.us_option.spy.far:UNCONFIGURED]"
        ));
    }

    #[test]
    fn combo_risk_charges_the_order_notional_limit_the_gross_not_the_net() {
        let mut service = service_permitting_shorts();
        // 400 units: gross 400 * 12.50 = 5,000... raise it until gross crosses
        // the 50,000 policy limit while the *net* stays far below it. Net here
        // is 4,001 * 2.50 = 10,002.50, which a net-based check would approve.
        let mut intent = combo_intent("combo-000002", "2026-01-02T14:30:00Z");
        intent.combo_quantity = decimal("units", "4001").unwrap();
        let decision = service
            .evaluate_combo_risk(
                &intent,
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"MAX_ORDER_NOTIONAL_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("combo_gross_notional=50012.50000000"));
        assert!(decision
            .evaluated_limits
            .contains("combo_net_debit=10002.50000000"));
    }

    #[test]
    fn an_order_off_its_instruments_tick_grid_is_refused_before_the_broker() {
        let mut service = service();
        let mut off_grid = intent("intent-tick-001", "2026-01-02T14:30:00Z");
        off_grid.order_type = OrderType::Limit;
        off_grid.limit_price = Some(decimal("limit", "100.005").unwrap());
        let outcome = service
            .submit_intent(
                off_grid,
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:01Z",
            )
            .unwrap();
        assert!(!outcome.decision.approved);
        assert_eq!(
            outcome.decision.reason_codes,
            vec!["LIMIT_PRICE_OFF_TICK_GRID".to_owned()]
        );
        assert!(
            outcome.order_id.is_none(),
            "an off-grid order reached the OMS"
        );
        assert!(outcome
            .decision
            .evaluated_limits
            .contains("instrument_tick_size=0.01"));

        let mut on_grid = intent("intent-tick-002", "2026-01-02T14:30:00Z");
        on_grid.order_type = OrderType::Limit;
        on_grid.limit_price = Some(decimal("limit", "100.01").unwrap());
        assert!(
            service
                .submit_intent(
                    on_grid,
                    market("2026-01-02T14:30:00Z"),
                    "2026-01-02T14:30:02Z"
                )
                .unwrap()
                .decision
                .approved,
            "an on-grid limit was refused"
        );
    }

    #[test]
    fn an_order_for_an_instrument_without_a_configured_tick_is_refused() {
        // iwm's lot is listed, so only its missing tick can refuse it.
        let mut policy = policy();
        policy.instrument_lot_sizes.insert(
            "inst.us_equity.iwm".to_owned(),
            decimal("lot", "1").unwrap(),
        );
        let mut service = service_with(policy);
        let mut unlisted = intent("intent-tick-003", "2026-01-02T14:30:00Z");
        unlisted.instrument_id = "inst.us_equity.iwm".to_owned();
        let mut iwm = market("2026-01-02T14:30:00Z");
        iwm.instrument_id = "inst.us_equity.iwm".to_owned();
        // A market order carries no limit, but its instrument is still unknown
        // reference data, so it fails closed.
        let outcome = service
            .submit_intent(unlisted, iwm, "2026-01-02T14:30:01Z")
            .unwrap();
        assert!(!outcome.decision.approved);
        assert_eq!(
            outcome.decision.reason_codes,
            vec!["INSTRUMENT_TICK_SIZE_UNCONFIGURED".to_owned()]
        );
        assert!(outcome
            .decision
            .evaluated_limits
            .contains("instrument_tick_size=UNCONFIGURED"));

        // A listed instrument's market order is unaffected.
        assert!(
            service
                .submit_intent(
                    intent("intent-tick-004", "2026-01-02T14:30:00Z"),
                    market("2026-01-02T14:30:00Z"),
                    "2026-01-02T14:30:02Z"
                )
                .unwrap()
                .decision
                .approved
        );
    }

    #[test]
    fn a_tick_size_table_must_be_nonempty_positive_and_canonical() {
        assert!(policy().validate().is_ok());
        for broken in [
            BTreeMap::new(),
            BTreeMap::from([("inst.us_equity.spy".to_owned(), Decimal::ZERO)]),
            BTreeMap::from([("INST.SPY".to_owned(), decimal("tick", "0.01").unwrap())]),
        ] {
            let policy = PaperRiskPolicy {
                instrument_tick_sizes: broken.clone(),
                ..policy()
            };
            assert!(policy.validate().is_err(), "accepted tick table {broken:?}");
        }
        let mut coarser = policy();
        coarser.instrument_tick_sizes.insert(
            "inst.us_equity.spy".to_owned(),
            decimal("tick", "0.05").unwrap(),
        );
        let coarser_service = PaperTradingService::new(
            account(),
            coarser,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account()).unwrap(),
        )
        .unwrap();
        assert_ne!(
            service().configuration_fingerprint(),
            coarser_service.configuration_fingerprint(),
            "a journal must not reopen under a changed tick table"
        );
    }

    #[test]
    fn an_order_off_its_instruments_lot_size_is_refused_before_the_broker() {
        let mut policy = policy();
        policy.instrument_lot_sizes.insert(
            "inst.us_equity.spy".to_owned(),
            decimal("lot", "5").unwrap(),
        );
        let mut service = service_with(policy);
        // Three shares is a whole number but not a whole number of five-share
        // lots, and every other limit passes.
        let mut off_lot = intent("intent-lot-001", "2026-01-02T14:30:00Z");
        off_lot.quantity = decimal("quantity", "3").unwrap();
        let outcome = service
            .submit_intent(
                off_lot,
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:01Z",
            )
            .unwrap();
        assert!(!outcome.decision.approved);
        assert_eq!(
            outcome.decision.reason_codes,
            vec!["ORDER_QUANTITY_OFF_LOT_SIZE".to_owned()]
        );
        assert!(
            outcome.order_id.is_none(),
            "an off-lot order reached the OMS"
        );
        assert!(outcome
            .decision
            .evaluated_limits
            .contains("instrument_lot_size=5.00000000"));

        let mut whole_lots = intent("intent-lot-002", "2026-01-02T14:30:00Z");
        whole_lots.quantity = decimal("quantity", "10").unwrap();
        assert!(
            service
                .submit_intent(
                    whole_lots,
                    market("2026-01-02T14:30:00Z"),
                    "2026-01-02T14:30:02Z"
                )
                .unwrap()
                .decision
                .approved,
            "a whole number of lots was refused"
        );
    }

    #[test]
    fn an_order_for_an_instrument_without_a_configured_lot_size_is_refused() {
        let mut policy = policy();
        policy.instrument_lot_sizes.remove("inst.us_equity.spy");
        let mut service = service_with(policy);
        // spy's tick is listed and a market order carries no limit, so only
        // the missing lot size can refuse it.
        let outcome = service
            .submit_intent(
                intent("intent-lot-003", "2026-01-02T14:30:00Z"),
                market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:01Z",
            )
            .unwrap();
        assert!(!outcome.decision.approved);
        assert_eq!(
            outcome.decision.reason_codes,
            vec!["INSTRUMENT_LOT_SIZE_UNCONFIGURED".to_owned()]
        );
        assert!(outcome.order_id.is_none());
        assert!(outcome
            .decision
            .evaluated_limits
            .contains("instrument_lot_size=UNCONFIGURED"));

        // A listed instrument's order is unaffected.
        let mut qqq = intent("intent-lot-004", "2026-01-02T14:30:00Z");
        qqq.instrument_id = "inst.us_equity.qqq".to_owned();
        let mut qqq_market = market("2026-01-02T14:30:00Z");
        qqq_market.instrument_id = "inst.us_equity.qqq".to_owned();
        assert!(
            service
                .submit_intent(qqq, qqq_market, "2026-01-02T14:30:02Z")
                .unwrap()
                .decision
                .approved
        );
    }

    #[test]
    fn a_lot_size_table_must_be_nonempty_positive_and_canonical() {
        for broken in [
            BTreeMap::new(),
            BTreeMap::from([("inst.us_equity.spy".to_owned(), Decimal::ZERO)]),
            BTreeMap::from([("INST.SPY".to_owned(), decimal("lot", "1").unwrap())]),
        ] {
            let policy = PaperRiskPolicy {
                instrument_lot_sizes: broken.clone(),
                ..policy()
            };
            assert!(policy.validate().is_err(), "accepted lot table {broken:?}");
        }
        let mut round_lots = policy();
        round_lots.instrument_lot_sizes.insert(
            "inst.us_equity.spy".to_owned(),
            decimal("lot", "100").unwrap(),
        );
        assert_ne!(
            service().configuration_fingerprint(),
            service_with(round_lots).configuration_fingerprint(),
            "a journal must not reopen under a changed lot table"
        );
    }

    #[test]
    fn combo_risk_collars_each_leg_against_its_own_mark() {
        let mut service = service_permitting_shorts();
        let intent = combo_intent("combo-000003", "2026-01-02T14:30:00Z");
        let mut market = combo_market("2026-01-02T14:30:00Z");
        // Move only the second leg's mark. The first leg is still exactly on
        // its own mark, so a single blended or first-leg-only collar would
        // miss this entirely.
        market.marks[1].mark_price = decimal("mark", "4").unwrap();
        let decision = service
            .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z")
            .unwrap();
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"PRICE_COLLAR_EXCEEDED".to_owned()));
        // 5.00 requested against a 4.00 mark is 2,500 bps, far past the 100 bps
        // policy limit, and it is the *widest* leg that is recorded.
        assert!(decision
            .evaluated_limits
            .contains("widest_leg_deviation_bps=2500.00000000"));
    }

    /// A combination's short leg is refused unless an operator permitted it.
    ///
    /// This is the default. `core/paper` holds no option reference data, so it
    /// cannot prove that the short far-strike leg is covered by the long near
    /// one, and it does not assume it. Almost every real spread has a short
    /// leg, so the practical effect is that spreads require an explicit,
    /// bounded operator permission — which is the intended behaviour, not an
    /// oversight.
    #[test]
    fn combo_risk_refuses_a_short_leg_until_an_operator_permits_it() {
        let mut default_service = service();
        let decision = default_service
            .evaluate_combo_risk(
                &combo_intent("combo-000004", "2026-01-02T14:30:00Z"),
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned()));

        // The same structure, with the permission present, is approved.
        let mut permitting = service_permitting_shorts();
        let decision = permitting
            .evaluate_combo_risk(
                &combo_intent("combo-000004", "2026-01-02T14:30:00Z"),
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(decision.approved, "{:?}", decision.reason_codes);
    }

    #[test]
    fn combo_risk_projects_each_leg_against_its_own_position_bound() {
        let mut service = service_permitting_shorts();
        // 2,000 units: the long leg projects +2,000 against a 1,000 long limit
        // and the short leg projects -2,000 against a 1,000 short bound. Both
        // legs breach, and the per-instrument projection is what sees it.
        let mut oversized = combo_intent("combo-000005", "2026-01-02T14:30:00Z");
        oversized.combo_quantity = decimal("units", "2000").unwrap();
        let decision = service
            .evaluate_combo_risk(
                &oversized,
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(decision
            .reason_codes
            .contains(&"POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned()));

        // Exactly at both bounds is permitted; the guard is a ceiling, not an
        // off-by-one refusal.
        let mut exact = combo_intent("combo-000012", "2026-01-02T14:30:00Z");
        exact.combo_quantity = decimal("units", "1000").unwrap();
        let decision = service
            .evaluate_combo_risk(
                &exact,
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(!decision
            .reason_codes
            .contains(&"POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned()));
    }

    #[test]
    fn combo_submission_creates_one_acknowledged_order_for_the_whole_group() {
        let mut service = service_permitting_shorts();
        let outcome = service
            .submit_combo_intent(
                combo_intent("combo-000020", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(
            outcome.decision.approved,
            "{:?}",
            outcome.decision.reason_codes
        );
        assert_eq!(
            outcome.order_id.as_deref(),
            Some("combo-order-combo-000020")
        );
        assert_eq!(outcome.state, Some(OrderState::Acknowledged));

        let order = service.combo_order("combo-order-combo-000020").unwrap();
        assert_eq!(order.oms.intent.legs.len(), 2);
        assert!(order.broker_order_id.is_some());
        // One broker order for the whole group, not one per leg.
        assert_eq!(order.broker_order_versions.len(), 1);
        // And one open order, not two.
        assert_eq!(service.dashboard().working_orders, 1);
        assert!(service.orders.is_empty());
    }

    #[test]
    fn combo_submission_is_idempotent_and_refuses_a_changed_retry() {
        let mut service = service_permitting_shorts();
        let intent = combo_intent("combo-000021", "2026-01-02T14:30:00Z");
        let market = combo_market("2026-01-02T14:30:00Z");
        let first = service
            .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
            .unwrap();
        let replay = service
            .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
            .unwrap();
        assert_eq!(first.order_id, replay.order_id);
        assert_eq!(first.decision, replay.decision);
        assert_eq!(service.combo_orders.len(), 1);

        // The same identity with different economics is refused outright, not
        // silently treated as the original.
        let mut tampered = intent.clone();
        tampered.combo_quantity = decimal("units", "5").unwrap();
        assert!(service
            .submit_combo_intent(tampered, market.clone(), "2026-01-02T14:30:02Z")
            .is_err());

        // And a retry that re-prices the original is refused too: a retry is a
        // retry, not a new decision wearing an old identity.
        let mut moved = market;
        moved.marks[0].mark_price = decimal("mark", "7.51").unwrap();
        assert!(service
            .submit_combo_intent(intent, moved, "2026-01-02T14:30:02Z")
            .is_err());
    }

    #[test]
    fn combo_refusal_records_evidence_and_creates_no_order() {
        let mut service = service_permitting_shorts();
        service
            .activate_kill_switch(KillSwitchScope::Global)
            .unwrap();
        let outcome = service
            .submit_combo_intent(
                combo_intent("combo-000022", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(!outcome.decision.approved);
        assert!(outcome.order_id.is_none());
        assert!(service.combo_orders.is_empty());
        // The refusal itself is durable evidence: a rejected combination is
        // still a decision that was made and has to be auditable.
        assert!(service
            .combo_risk_evidence("paper-combo-risk-combo-000022")
            .is_some());
    }

    #[test]
    fn combo_transport_failure_leaves_the_group_unknown_and_disconnects() {
        let account = account();
        let adapter = IbkrPaperAdapter::new(&account).unwrap();
        let mut faulted = FaultInjectingBroker::new(adapter);
        faulted.inject(BrokerOperation::Submit, BrokerFault::Disconnect);
        let mut service = PaperTradingService::new(
            account,
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            faulted,
        )
        .unwrap();
        let result = service.submit_combo_intent(
            combo_intent("combo-000023", "2026-01-02T14:30:00Z"),
            combo_market("2026-01-02T14:30:00Z"),
            "2026-01-02T14:30:02Z",
        );
        // The outcome is genuinely unknown -- the request may or may not have
        // reached the venue -- so it is never reported as a clean rejection.
        assert!(result.is_err());
        let order = service.combo_order("combo-order-combo-000023").unwrap();
        assert_eq!(order.oms.state, OrderState::Unknown);
        // And an UNKNOWN combination blocks the next decision of either kind,
        // exactly as an UNKNOWN plain order does.
        assert!(service.has_unknown_order());
    }

    #[test]
    fn a_working_combination_is_visible_to_every_single_order_risk_counter() {
        let mut service = service_permitting_shorts();
        service
            .submit_combo_intent(
                combo_intent("combo-000024", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();

        // Open-order count, rate window, and reserved cash all see it.
        assert_eq!(service.working_order_count(), 1);
        assert_eq!(
            service.recent_order_count("2026-01-02T14:30:30Z").unwrap(),
            1
        );
        // 4 units * 2.50 net debit.
        assert_eq!(
            service.total_reserved_cash().unwrap(),
            decimal("reserved", "10").unwrap()
        );

        // And self-trade: the combination's short far-strike leg is a real
        // resting sell, so a plain buy on that instrument is a self-trade.
        assert!(service.conflicts_with_working_order("inst.us_option.spy.far", Side::Buy));
        assert!(!service.conflicts_with_working_order("inst.us_option.spy.far", Side::Sell));
        // The long near-strike leg is the mirror image.
        assert!(service.conflicts_with_working_order("inst.us_option.spy.near", Side::Sell));
    }

    #[test]
    fn a_working_combination_reconciles_against_its_independent_broker_order() {
        let mut service = service_permitting_shorts();
        service
            .submit_combo_intent(
                combo_intent("combo-000025", "2026-01-02T14:30:00Z"),
                combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        let report = service.reconcile("2026-01-02T21:00:00Z").unwrap();
        assert!(report.is_clean(), "{:?}", report.issues);
        service.broker.combos.clear();
        let missing = service.reconcile("2026-01-02T21:00:01Z").unwrap();
        assert!(missing
            .issues
            .iter()
            .any(|issue| issue.category == "MISSING_BROKER_ORDER"));
    }

    #[test]
    fn an_authenticated_submitter_is_journaled_and_owns_the_retry() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "combo-submitted-by"
        ));
        let _ = fs::remove_file(&journal_path);
        let account = account();
        let open = || {
            PaperTradingService::open_durable(
                account.clone(),
                policy_permitting_shorts(),
                KillSwitchRegistry::new("paper-kills-v1").unwrap(),
                IbkrPaperAdapter::new(&account).unwrap(),
                &journal_path,
            )
            .unwrap()
        };
        let mut service = open();
        let intent = combo_intent("combo-000027", "2026-01-02T14:30:00Z");
        let market = combo_market("2026-01-02T14:30:00Z");
        let decided_at = "2026-01-02T14:30:02Z";
        let outcome = service
            .submit_combo_intent_as(
                intent.clone(),
                market.clone(),
                decided_at,
                Some("user.trader"),
            )
            .unwrap();
        assert!(outcome.decision.approved);
        let decision_id = outcome.decision.decision_id.clone();
        assert_eq!(
            service
                .combo_risk_evidence(&decision_id)
                .unwrap()
                .submitted_by
                .as_deref(),
            Some("user.trader")
        );
        // Neither another operator nor an unattributed caller may claim it.
        for other in [Some("user.other"), None] {
            assert!(service
                .submit_combo_intent_as(intent.clone(), market.clone(), decided_at, other)
                .is_err());
        }
        assert!(service
            .submit_combo_intent_as(
                combo_intent("combo-000028", "2026-01-02T14:30:00Z"),
                market.clone(),
                decided_at,
                Some("User Trader"),
            )
            .is_err());
        // The original submitter's retry is the idempotent original.
        let retry = service
            .submit_combo_intent_as(intent, market.clone(), decided_at, Some("user.trader"))
            .unwrap();
        assert_eq!(retry.order_id, outcome.order_id);
        // A direct, unattributed submission journals no `submitted_by` key at
        // all, keeping the earlier serialization byte for byte.
        service
            .submit_combo_intent(
                combo_intent("combo-000029", "2026-01-02T14:30:00Z"),
                market,
                decided_at,
            )
            .unwrap();
        drop(service);

        // The attribution survives a restart.
        let reopened = open();
        assert_eq!(
            reopened
                .combo_risk_evidence(&decision_id)
                .unwrap()
                .submitted_by
                .as_deref(),
            Some("user.trader")
        );
        drop(reopened);
        let journal = fs::read_to_string(&journal_path).unwrap();
        let last: serde_json::Value =
            serde_json::from_str(journal.lines().last().unwrap()).unwrap();
        fn find<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
            match value {
                serde_json::Value::Object(map) => map
                    .get(key)
                    .or_else(|| map.values().find_map(|child| find(child, key))),
                serde_json::Value::Array(items) => items.iter().find_map(|child| find(child, key)),
                _ => None,
            }
        }
        let attributed = find(&last, "paper-combo-risk-combo-000027").unwrap();
        assert_eq!(attributed["submitted_by"], "user.trader");
        let unattributed = find(&last, "paper-combo-risk-combo-000029").unwrap();
        assert!(
            unattributed.get("submitted_by").is_none(),
            "an unattributed decision must not carry submitted_by"
        );
        let _ = fs::remove_file(&journal_path);
    }

    #[test]
    fn a_combination_survives_a_durable_journal_reopen() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-{}.ndjson",
            std::process::id(),
            "combo-recovery"
        ));
        let _ = fs::remove_file(&journal_path);
        let account = account();
        let mut service = PaperTradingService::open_durable(
            account.clone(),
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        let intent = combo_intent("combo-000026", "2026-01-02T14:30:00Z");
        let market = combo_market("2026-01-02T14:30:00Z");
        service
            .submit_combo_intent(intent.clone(), market.clone(), "2026-01-02T14:30:02Z")
            .unwrap();
        let before = service
            .combo_order("combo-order-combo-000026")
            .unwrap()
            .clone();
        drop(service);

        let reopened = PaperTradingService::open_durable(
            account.clone(),
            policy_permitting_shorts(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        let after = reopened.combo_order("combo-order-combo-000026").unwrap();
        // The whole structure comes back exactly: every leg, its ratio, its
        // protected price, the price-limit kind, and the per-leg observation
        // that priced it.
        assert_eq!(after.oms.intent, intent);
        assert_eq!(after.market, market);
        assert_eq!(after.oms.state, before.oms.state);
        assert_eq!(after.broker_order_id, before.broker_order_id);
        assert_eq!(
            after.oms.intent.price_limit.kind(),
            follon_domain::ComboPriceLimit::MaximumDebit(Decimal::ZERO).kind()
        );
        // And the reservation it implies survives with it, so a restart does
        // not free cash the combination still has committed.
        assert_eq!(
            reopened.total_reserved_cash().unwrap(),
            decimal("reserved", "10").unwrap()
        );
        let _ = fs::remove_file(&journal_path);
    }

    /// An absent combination field deserializes as "no combinations".
    ///
    /// This checks the persisted *type*, not a reopened journal file, and the
    /// distinction is real. `FilePaperJournal::open` additionally requires each
    /// line to re-serialize byte-for-byte, so a file missing any field the
    /// current serializer writes is rejected before `#[serde(default)]` can
    /// apply. That is pre-existing behaviour, not a consequence of the
    /// combination fields: deleting `tax_lots` -- which predates them -- from a
    /// journal line fails exactly the same way, verified directly. So the
    /// defaults make the type tolerant, which is what a future format change
    /// needs, while whole-file compatibility across a schema change remains an
    /// open question recorded in `docs/06-delivery/16-delivery-state.md`.
    #[test]
    fn an_absent_combination_field_deserializes_as_no_combinations() {
        let without_combinations = serde_json::json!({
            "configuration_fingerprint": "",
            "account_id": "acct.paper.001",
            "currency": "USD",
            "cash": "100000.00000000",
            "orders": {},
            "risk_evidence": {},
            "positions": {},
            "execution_ids": [],
            "active_kill_switches": [],
            "incidents": {},
            "last_reconciled_at": serde_json::Value::Null,
            "last_reconciliation_clean": serde_json::Value::Null,
            "paper_days": {},
            "next_reconciliation": 0,
        });
        let state: PersistentPaperState = serde_json::from_value(without_combinations).unwrap();
        assert!(state.combo_orders.is_empty());
        assert!(state.combo_risk_evidence.is_empty());
    }

    #[test]
    fn short_permission_must_state_a_positive_bound() {
        let invalid = PaperRiskPolicy {
            short_exposure: Some(ShortExposurePolicy {
                max_short_quantity: Decimal::ZERO,
            }),
            ..policy()
        };
        // `None` already means "no shorting", so a zero bound can only be a
        // configuration mistake and fails closed rather than silently agreeing.
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn combo_risk_binds_the_quantity_limit_to_the_largest_leg_not_the_unit_count() {
        let mut service = service_permitting_shorts();
        // 60 combination units, but the near leg carries a ratio of 2, so the
        // broker sees a 120-contract order against a 100 limit. Counting units
        // alone would approve it.
        let mut intent = combo_intent("combo-000006", "2026-01-02T14:30:00Z");
        intent.combo_quantity = decimal("units", "60").unwrap();
        intent.legs[0].ratio = 2;
        intent.legs[0].limit_price = decimal("near", "6").unwrap();
        intent.price_limit =
            follon_domain::ComboPriceLimit::MaximumDebit(decimal("cap", "7").unwrap());
        let mut market = combo_market("2026-01-02T14:30:00Z");
        market.marks[0].mark_price = decimal("mark", "6").unwrap();
        let decision = service
            .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z")
            .unwrap();
        assert!(decision
            .reason_codes
            .contains(&"MAX_ORDER_QUANTITY_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("largest_leg_quantity=120.00000000"));
    }

    #[test]
    fn combo_risk_charges_only_the_net_debit_against_available_cash() {
        let mut service = service_permitting_shorts();
        // A credit structure takes no cash out, so it must not be refused for
        // insufficient cash however large its gross notional is.
        let mut credit = combo_intent("combo-000007", "2026-01-02T14:30:00Z");
        credit.legs[0].side = Side::Sell;
        credit.legs[1].side = Side::Buy;
        credit.price_limit =
            follon_domain::ComboPriceLimit::MinimumCredit(decimal("floor", "2").unwrap());
        let decision = service
            .evaluate_combo_risk(
                &credit,
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(!decision
            .reason_codes
            .contains(&"INSUFFICIENT_INTERNAL_CASH".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("combo_net_debit=0.00000000"));

        // A debit structure larger than the 100,000 account does not.
        let mut expensive = combo_intent("combo-000008", "2026-01-02T14:30:00Z");
        expensive.combo_quantity = decimal("units", "50000").unwrap();
        let decision = service
            .evaluate_combo_risk(
                &expensive,
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(decision
            .reason_codes
            .contains(&"INSUFFICIENT_INTERNAL_CASH".to_owned()));
    }

    #[test]
    fn combo_risk_is_halted_by_a_kill_switch_on_any_single_leg() {
        let mut service = service_permitting_shorts();
        service
            .activate_kill_switch(KillSwitchScope::Instrument(
                "inst.us_option.spy.far".to_owned(),
            ))
            .unwrap();
        let decision = service
            .evaluate_combo_risk(
                &combo_intent("combo-000009", "2026-01-02T14:30:00Z"),
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"KILL_SWITCH_INSTRUMENT_INST.US_OPTION.SPY.FAR".to_owned()));
    }

    #[test]
    fn combo_risk_refuses_an_incomplete_or_stale_observation() {
        let mut service = service_permitting_shorts();
        let intent = combo_intent("combo-000010", "2026-01-02T14:30:00Z");

        // One leg unquoted: the gate cannot price the structure and must not
        // guess.
        let mut partial = combo_market("2026-01-02T14:30:00Z");
        partial.marks.truncate(1);
        assert!(service
            .evaluate_combo_risk(&intent, &partial, "2026-01-02T14:30:02Z")
            .is_err());

        // One leg quoted twice, the other not at all: the count matches but
        // the coverage does not.
        let mut duplicated = combo_market("2026-01-02T14:30:00Z");
        duplicated.marks[1] = duplicated.marks[0].clone();
        assert!(service
            .evaluate_combo_risk(&intent, &duplicated, "2026-01-02T14:30:02Z")
            .is_err());

        // Freshness is the *stalest* leg's. One fresh quote beside an old one
        // must not launder it.
        let mut half_stale = combo_market("2026-01-02T14:30:00Z");
        half_stale.marks[0].observed_at = "2026-01-02T14:20:00Z".to_owned();
        assert!(service
            .evaluate_combo_risk(&intent, &half_stale, "2026-01-02T14:30:02Z")
            .is_err());

        // And an observation from after the decision is refused outright.
        assert!(service
            .evaluate_combo_risk(
                &intent,
                &combo_market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:30:02Z"
            )
            .is_err());
    }

    #[test]
    fn combo_risk_creates_no_order_and_contacts_no_broker() {
        let mut service = service_permitting_shorts();
        let decision = service
            .evaluate_combo_risk(
                &combo_intent("combo-000011", "2026-01-02T14:30:00Z"),
                &combo_market("2026-01-02T14:30:00Z"),
                "2026-01-02T14:30:02Z",
            )
            .unwrap();
        assert!(decision.approved, "{:?}", decision.reason_codes);
        // Assessment only, until E1.3 lands a submission path.
        assert!(service.order("order-combo-000011").is_none());
        assert_eq!(service.dashboard().working_orders, 0);
        assert_eq!(service.dashboard().positions.len(), 0);
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

        fn cancel(&mut self, _request: &BrokerCancelRequest) -> Result<(), PaperError> {
            Err(PaperError("not used by reconciliation test".to_owned()))
        }

        fn poll(&mut self, _account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
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

        fn reconnect(&mut self, _account_id: &str) -> Result<(), PaperError> {
            Ok(())
        }
    }

    #[test]
    fn paper_fifo_tax_lots_track_disposal_cost_basis_independent_of_average_cost() {
        let mut service = service();

        // First lot: 2 units @ $100.
        let first = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "2").unwrap(),
                    ..intent("intent-tax-lot-001", "2026-01-02T14:31:00Z")
                },
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        let first_order_id = first.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &first_order_id,
                decimal("quantity", "2").unwrap(),
                decimal("price", "100").unwrap(),
                decimal("fee", "0.20").unwrap(),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // Second lot: 2 units @ $120, a distinctly higher price than the
        // first, so a FIFO disposal's cost basis provably differs from what
        // a blended average cost would report.
        let second = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "2").unwrap(),
                    ..intent("intent-tax-lot-002", "2026-01-02T14:32:00Z")
                },
                market("2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        let second_order_id = second.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &second_order_id,
                decimal("quantity", "2").unwrap(),
                decimal("price", "120").unwrap(),
                decimal("fee", "0.20").unwrap(),
                "2026-01-02T14:32:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);
        assert_eq!(service.tax_lots("inst.us_equity.spy").len(), 2);

        // Dispose 2 units at $130. FIFO must consume the $100 lot first.
        let sell = service
            .submit_intent(
                OrderIntent {
                    side: Side::Sell,
                    quantity: decimal("quantity", "2").unwrap(),
                    ..intent("intent-tax-lot-003", "2026-01-02T14:33:00Z")
                },
                market("2026-01-02T14:33:00Z"),
                "2026-01-02T14:33:00Z",
            )
            .unwrap();
        let sell_order_id = sell.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &sell_order_id,
                decimal("quantity", "2").unwrap(),
                decimal("price", "130").unwrap(),
                decimal("fee", "0.20").unwrap(),
                "2026-01-02T14:33:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // The FIFO book fully consumed the $100 lot (all-in unit cost
        // 100.10) and left the $120 lot (all-in unit cost 120.10) untouched.
        let remaining = service.tax_lots("inst.us_equity.spy");
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].unit_cost,
            decimal("unit cost", "120.10").unwrap()
        );
        // realized = proceeds(260) - fifo cost basis(200.20) - fee(0.20) = 59.60
        assert_eq!(
            service.realized_tax_pnl().unwrap(),
            decimal("realized", "59.60").unwrap()
        );

        // Portfolio's own blended average-cost realized P&L is a distinct
        // figure: average cost across both lots is 110.10/unit, so
        // realized = (130 - 110.10) * 2 - 0.20 = 39.60 -- proving the
        // tax-lot book is an independent ledger, not a relabeling of the
        // existing average-cost figure.
        let dashboard = service.dashboard();
        assert_eq!(dashboard.positions[0].realized_pnl, "39.60000000");
    }

    #[test]
    fn paper_tax_lots_survive_a_durable_journal_reopen() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-tax-lots.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let account = account();
        let mut durable = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        let submitted = durable
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "2").unwrap(),
                    ..intent("intent-tax-lot-durable-001", "2026-01-02T14:31:00Z")
                },
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        let order_id = submitted.order_id.unwrap();
        durable
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "2").unwrap(),
                decimal("price", "100").unwrap(),
                decimal("fee", "0.20").unwrap(),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(durable.synchronize().unwrap(), 2);
        assert_eq!(durable.tax_lots("inst.us_equity.spy").len(), 1);
        drop(durable);

        let recovered = PaperTradingService::open_durable(
            account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account).unwrap(),
            &journal_path,
        )
        .unwrap();
        let remaining = recovered.tax_lots("inst.us_equity.spy");
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].unit_cost,
            decimal("unit cost", "100.10").unwrap()
        );
        assert_eq!(
            remaining[0].remaining_quantity,
            decimal("qty", "2").unwrap()
        );
        assert_eq!(recovered.realized_tax_pnl().unwrap(), Decimal::ZERO);
        drop(recovered);
        let _ = fs::remove_file(&journal_path);
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
    fn paper_broker_registry_keeps_submission_behind_oms_risk() {
        let mut service = registry_service();
        assert_eq!(
            service
                .broker_mut()
                .routes()
                .into_iter()
                .map(|route| route.account_id)
                .collect::<Vec<_>>(),
            vec!["acct.paper.001".to_owned(), "acct.paper.002".to_owned()]
        );
        let first_route_fingerprint = service
            .broker_mut()
            .configuration_fingerprint("acct.paper.001")
            .unwrap();
        let second_route_fingerprint = service
            .broker_mut()
            .configuration_fingerprint("acct.paper.002")
            .unwrap();
        assert_ne!(first_route_fingerprint, second_route_fingerprint);
        let submitted = service
            .submit_intent(
                intent("intent-paper-registry", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(submitted.decision.approved);
        assert_eq!(submitted.state, Some(OrderState::Acknowledged));
        assert_eq!(service.synchronize().unwrap(), 1);
    }

    #[test]
    fn paper_broker_registry_is_account_isolated_and_refuses_invalid_routes() {
        let mut registry = broker_registry();
        let request = BrokerOrderRequest {
            client_order_id: "shared.client.order".to_owned(),
            account_id: "acct.paper.001".to_owned(),
            instrument_id: "inst.us_equity.spy".to_owned(),
            side: Side::Buy,
            quantity: decimal("quantity", "1").unwrap(),
            limit_price: None,
        };
        assert!(matches!(
            registry.submit(&request).unwrap(),
            BrokerSubmitResult::Acknowledged { .. }
        ));
        let second_submit = registry
            .submit(&BrokerOrderRequest {
                account_id: "acct.paper.002".to_owned(),
                ..request.clone()
            })
            .unwrap();
        assert!(matches!(
            second_submit,
            BrokerSubmitResult::Acknowledged { .. }
        ));
        registry
            .cancel(&BrokerCancelRequest {
                account_id: "acct.paper.001".to_owned(),
                client_order_id: request.client_order_id.clone(),
            })
            .unwrap();
        assert_eq!(registry.poll("acct.paper.001").unwrap().len(), 2);
        assert_eq!(registry.poll("acct.paper.002").unwrap().len(), 1);
        assert!(registry
            .submit(&BrokerOrderRequest {
                account_id: "acct.paper.404".to_owned(),
                ..request
            })
            .is_err());
        let duplicate_account = account();
        assert!(registry
            .register(
                PaperBrokerRoute {
                    account_id: duplicate_account.account_id.clone(),
                    adapter_id: "adapter.ibkr.paper.duplicate".to_owned(),
                    venue_id: "venue.ibkr.paper".to_owned(),
                    environment: "PAPER".to_owned(),
                },
                Box::new(IbkrPaperAdapter::new(&duplicate_account).unwrap()),
            )
            .is_err());
        assert!(registry
            .register(
                PaperBrokerRoute {
                    account_id: "acct.paper.003".to_owned(),
                    adapter_id: "adapter.unfingerprinted.paper.003".to_owned(),
                    venue_id: "venue.unfingerprinted.paper".to_owned(),
                    environment: "PAPER".to_owned(),
                },
                Box::new(CashAndPositionMismatchBroker),
            )
            .is_err());
    }

    #[test]
    fn legacy_single_account_journal_reopens_through_registry_without_rewriting_evidence() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-registry-migration-{}.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let paper_account = account();
        let mut legacy_service = PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();
        legacy_service
            .activate_kill_switch(KillSwitchScope::Global)
            .unwrap();
        drop(legacy_service);

        let mut registry = PaperBrokerRegistry::new();
        registry
            .register_legacy_ibkr_paper_route(&paper_account)
            .unwrap();
        let recovered = PaperTradingService::open_durable(
            paper_account,
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            registry,
            &journal_path,
        )
        .unwrap();
        assert_eq!(
            recovered.kill_switches().active_keys(),
            vec!["global".to_owned()]
        );
        drop(recovered);
        let _ = fs::remove_file(&journal_path);
    }

    #[test]
    fn legacy_registry_refuses_empty_journals_and_modern_route_changes_fail_recovery() {
        let legacy_path = std::env::temp_dir().join(format!(
            "follon-paper-legacy-empty-{}.ndjson",
            std::process::id()
        ));
        let modern_path = std::env::temp_dir().join(format!(
            "follon-paper-route-mismatch-{}.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&legacy_path);
        let _ = fs::remove_file(&modern_path);
        let paper_account = account();

        let mut legacy_registry = PaperBrokerRegistry::new();
        legacy_registry
            .register_legacy_ibkr_paper_route(&paper_account)
            .unwrap();
        assert!(PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            legacy_registry,
            &legacy_path,
        )
        .is_err());

        let mut modern_service = PaperTradingService::open_durable(
            paper_account.clone(),
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            modern_registry(&paper_account, "venue.ibkr.paper"),
            &modern_path,
        )
        .unwrap();
        modern_service
            .activate_kill_switch(KillSwitchScope::Global)
            .unwrap();
        drop(modern_service);
        assert!(PaperTradingService::open_durable(
            paper_account,
            policy(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            modern_registry(&account(), "venue.ibkr.paper.changed"),
            &modern_path,
        )
        .is_err());
        let _ = fs::remove_file(&legacy_path);
        let _ = fs::remove_file(&modern_path);
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
    fn paper_self_trade_risk_rejects_a_new_order_opposite_an_existing_working_order() {
        let mut service = service();
        let filled = service
            .submit_intent(
                intent("intent-paper-self-trade-position", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let position_order_id = filled.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &position_order_id,
                decimal("quantity", "1").unwrap(),
                decimal("price", "100").unwrap(),
                decimal("fee", "0.10").unwrap(),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);
        assert_eq!(
            service.order(&position_order_id).unwrap().oms.state,
            OrderState::Filled
        );

        let resting_buy = service
            .submit_intent(
                intent("intent-paper-self-trade-buy", "2026-01-02T14:32:00Z"),
                market("2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        assert!(resting_buy.decision.approved);
        assert_eq!(
            service
                .order(&resting_buy.order_id.unwrap())
                .unwrap()
                .oms
                .state,
            OrderState::Acknowledged
        );

        let mut opposite_sell = intent("intent-paper-self-trade-sell", "2026-01-02T14:32:01Z");
        opposite_sell.side = Side::Sell;
        let sell_result = service
            .submit_intent(
                opposite_sell,
                market("2026-01-02T14:32:01Z"),
                "2026-01-02T14:32:01Z",
            )
            .unwrap();
        assert!(!sell_result.decision.approved);
        assert!(sell_result.order_id.is_none());
        assert!(sell_result
            .decision
            .reason_codes
            .contains(&"SELF_TRADE_RISK".to_owned()));
    }

    /// A permissive aggregate policy: every real limit is wide enough that
    /// nothing rejects until a test deliberately tightens one field.
    /// `max_drawdown_bps` at `10000` (100%) can never trigger -- the ratio is
    /// always strictly below `10000` since equity must be positive to reach
    /// this kernel at all -- unlike a `0` sentinel, which would be the
    /// tightest possible drawdown limit now that drawdown is genuinely
    /// computed (Slice 2), not permanently zeroed (Slice 1).
    fn permissive_portfolio_risk_policy() -> follon_risk::PortfolioRiskPolicy {
        follon_risk::PortfolioRiskPolicy {
            version: "portfolio-risk-v1".to_owned(),
            global_kill_switch: false,
            max_gross_exposure: decimal("gross", "100000").unwrap(),
            max_abs_net_exposure: decimal("net", "100000").unwrap(),
            max_leverage_bps: decimal("leverage", "10000").unwrap(),
            max_concentration_bps: decimal("concentration", "10000").unwrap(),
            // Never trip on daily loss unless a test deliberately overrides
            // it: `i64::MAX`, the same "no real limit" sentinel the CLI
            // loader uses when an operator omits `max_daily_loss`.
            max_daily_loss: Decimal::from_integer(i64::MAX).unwrap(),
            max_drawdown_bps: decimal("drawdown", "10000").unwrap(),
            max_margin_utilization_bps: Decimal::ZERO,
            max_abs_delta: Decimal::ZERO,
            max_abs_gamma: Decimal::ZERO,
            max_open_orders: usize::MAX,
            max_order_rate: u32::MAX,
            allowed_instruments: BTreeSet::new(),
            restricted_instruments: BTreeSet::new(),
            sector_limits: BTreeMap::new(),
            asset_class_limits: BTreeMap::new(),
            currency_limits: BTreeMap::new(),
            strategy_limits: BTreeMap::new(),
            max_news_slippage_bps: None,
            max_spread_multiplier_bps: None,
        }
    }

    #[test]
    fn paper_portfolio_risk_composition_rejects_when_gross_exposure_limit_is_exceeded() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_gross_exposure = decimal("gross", "50").unwrap();
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();
        let result = service
            .submit_intent(
                intent("intent-portfolio-risk-gross", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result.order_id.is_none());
        assert!(result
            .decision
            .reason_codes
            .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_gross_exposure=100.00000000"));
    }

    #[test]
    fn paper_portfolio_risk_composition_rejects_when_sector_bucket_limit_is_exceeded() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy
            .sector_limits
            .insert("index".to_owned(), decimal("sector limit", "50").unwrap());
        let mut instrument_buckets = BTreeMap::new();
        instrument_buckets.insert(
            "inst.us_equity.spy".to_owned(),
            InstrumentBucket {
                asset_class: "equity".to_owned(),
                currency: "USD".to_owned(),
                sector: "index".to_owned(),
            },
        );
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets,
            margin_rates: None,
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();
        let result = service
            .submit_intent(
                intent("intent-portfolio-risk-sector", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"SECTOR_LIMIT_EXCEEDED:index".to_owned()));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_sector_gross=index:100.00000000"));
    }

    #[test]
    fn paper_portfolio_risk_composition_rejects_a_restricted_instrument() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy
            .restricted_instruments
            .insert("inst.us_equity.spy".to_owned());
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();
        let result = service
            .submit_intent(
                intent("intent-portfolio-risk-restricted", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"RESTRICTED_INSTRUMENT".to_owned()));
    }

    #[test]
    fn paper_portfolio_risk_composition_is_skipped_when_equity_is_not_positive() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        // Tight enough that a positive-equity account would certainly reject
        // on this limit -- its absence from the rejection below is what
        // proves composition was skipped, not merely satisfied.
        portfolio_policy.max_gross_exposure = decimal("gross", "0.01").unwrap();
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let zero_cash_account = PaperAccount {
            initial_cash: Decimal::ZERO,
            ..account()
        };
        let mut service = PaperTradingService::new(
            zero_cash_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&zero_cash_account).unwrap(),
        )
        .unwrap();
        let result = service
            .submit_intent(
                intent("intent-portfolio-risk-zero-equity", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"INSUFFICIENT_INTERNAL_CASH".to_owned()));
        assert!(!result
            .decision
            .reason_codes
            .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
        assert!(!result
            .decision
            .evaluated_limits
            .contains("portfolio_gross_exposure"));
    }

    #[test]
    fn paper_portfolio_risk_composition_rejects_when_drawdown_limit_is_exceeded() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_drawdown_bps = decimal("drawdown", "2000").unwrap();
        // Never trip on leverage/concentration; this test only wants
        // drawdown to be the exercised check.
        portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();

        // Buy and fill 1,000 shares at 100 -- real equity is ~100,000 cash
        // moved into a position of equal value, establishing a 100,000 peak.
        let filled = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "1000").unwrap(),
                    ..intent("intent-drawdown-buy", "2026-01-02T14:31:00Z")
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1000").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // The mark drops to 70: a real, computed 30% drawdown from the
        // 100,000 peak (equity is now 1,000 * 70 = 70,000), which exceeds the
        // configured 20% limit.
        let mut sell = intent("intent-drawdown-sell", "2026-01-02T14:32:00Z");
        sell.side = Side::Sell;
        sell.quantity = decimal("quantity", "1").unwrap();
        let result = service
            .submit_intent(
                sell,
                market_at_price("70", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"MAX_DRAWDOWN_EXCEEDED".to_owned()));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_peak_equity=100000.00000000"));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_drawdown_bps=3000.00000000"));
    }

    #[test]
    fn paper_portfolio_risk_composition_rejects_when_daily_loss_limit_is_exceeded() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_daily_loss = decimal("daily_loss", "2000").unwrap();
        // Never trip on leverage/concentration/drawdown; this test only wants
        // daily loss to be the exercised check.
        portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();

        // The very first risk evaluation of the day establishes the
        // session-start baseline at pure cash equity (100,000; no position
        // exists yet).
        let filled = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "1000").unwrap(),
                    ..intent("intent-daily-loss-buy", "2026-01-02T14:31:00Z")
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1000").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // The mark drops to 70 later the same UTC day: real equity falls from
        // the baseline's 100,000 (pure cash, observed before the buy filled)
        // to 70,000 -- a genuine 30,000 daily loss exceeding the configured
        // 2,000 limit.
        let mut sell = intent("intent-daily-loss-sell", "2026-01-02T14:32:00Z");
        sell.side = Side::Sell;
        sell.quantity = decimal("quantity", "1").unwrap();
        let result = service
            .submit_intent(
                sell,
                market_at_price("70", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"MAX_DAILY_LOSS_EXCEEDED".to_owned()));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=100000.00000000"));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_daily_pnl=-30000.00000000"));
    }

    #[test]
    fn paper_daily_loss_baseline_resets_at_a_new_utc_calendar_day() {
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1001").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();

        // Buy and fill 1,000 shares at 100 -- the very first evaluation ever
        // establishes the day-1 baseline at pure cash equity (100,000).
        let filled = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "1000").unwrap(),
                    ..intent("intent-daily-reset-buy", "2026-01-02T14:31:00Z")
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1000").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // Re-quote to 150 later the same UTC day: equity rises to 150,000
        // relative to the still-standing day-1 baseline of 100,000, a real
        // +50,000 daily gain. This order is rejected on quantity alone, but
        // the mark update (and hence the daily-P&L read) is unconditional.
        let mut spike = intent("intent-daily-reset-spike", "2026-01-02T20:00:00Z");
        spike.quantity = decimal("quantity", "1002").unwrap();
        let spiked = service
            .submit_intent(
                spike,
                market_at_price("150", "2026-01-02T20:00:00Z"),
                "2026-01-02T20:00:00Z",
            )
            .unwrap();
        assert!(!spiked.decision.approved);
        assert!(spiked
            .decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=100000.00000000"));
        assert!(spiked
            .decision
            .evaluated_limits
            .contains("portfolio_daily_pnl=50000.00000000"));

        // The next UTC calendar day resets the baseline to that day's own
        // first observed equity (150,000, the mark is unchanged), not the
        // prior day's 100,000 -- proving a genuine reset rather than a
        // carried-over accumulation.
        let mut next_day = intent("intent-daily-reset-day2", "2026-01-03T09:00:00Z");
        next_day.quantity = decimal("quantity", "1002").unwrap();
        let after_rollover = service
            .submit_intent(
                next_day,
                market_at_price("150", "2026-01-03T09:00:00Z"),
                "2026-01-03T09:00:00Z",
            )
            .unwrap();
        assert!(!after_rollover.decision.approved);
        assert!(after_rollover
            .decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=150000.00000000"));
        assert!(after_rollover
            .decision
            .evaluated_limits
            .contains("portfolio_daily_pnl=0.00000000"));
    }

    #[test]
    fn paper_daily_loss_baseline_survives_a_durable_journal_reopen() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-daily-loss-baseline.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1001").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut durable = PaperTradingService::open_durable(
            paper_account.clone(),
            risk_policy.clone(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();

        // Buy and fill 1,000 shares at 100 -- the very first evaluation ever
        // establishes the baseline at pure cash equity (100,000).
        let filled = durable
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "1000").unwrap(),
                    ..intent("intent-daily-durability-buy", "2026-01-02T14:31:00Z")
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        durable
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1000").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(durable.synchronize().unwrap(), 2);

        // Re-quote to 150: equity rises to 150,000 -- a real +50,000 gain
        // against the day's still-standing 100,000 baseline. This order is
        // rejected on quantity alone, but the mark update (and hence the
        // daily-P&L read) is unconditional.
        let mut spike = intent("intent-daily-durability-spike", "2026-01-02T20:00:00Z");
        spike.quantity = decimal("quantity", "1002").unwrap();
        let spiked = durable
            .submit_intent(
                spike,
                market_at_price("150", "2026-01-02T20:00:00Z"),
                "2026-01-02T20:00:00Z",
            )
            .unwrap();
        assert!(!spiked.decision.approved);
        assert!(spiked
            .decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=100000.00000000"));
        drop(durable);

        // Reopen later the same UTC day: the durable baseline (100,000) must
        // survive, not reset to today's current equity (150,000).
        let mut reopened = PaperTradingService::open_durable(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();
        reopened
            .reconnect_and_reconcile("2026-01-02T21:00:00Z")
            .unwrap();
        let mut probe = intent(
            "intent-daily-durability-after-reopen",
            "2026-01-02T21:00:00Z",
        );
        probe.quantity = decimal("quantity", "1002").unwrap();
        let result = reopened
            .submit_intent(
                probe,
                market_at_price("150", "2026-01-02T21:00:00Z"),
                "2026-01-02T21:00:00Z",
            )
            .unwrap();
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=100000.00000000"));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_daily_pnl=50000.00000000"));
        fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn paper_portfolio_risk_composition_rejects_when_margin_utilization_limit_is_exceeded() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_margin_utilization_bps = decimal("margin", "4000").unwrap();
        // Never trip on leverage/concentration; this test only wants margin
        // utilization to be the exercised check.
        portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
        let mut instrument_buckets = BTreeMap::new();
        instrument_buckets.insert(
            "inst.us_equity.spy".to_owned(),
            InstrumentBucket {
                asset_class: "equity".to_owned(),
                currency: "USD".to_owned(),
                sector: "index".to_owned(),
            },
        );
        let mut margin_rates = BTreeMap::new();
        margin_rates.insert(
            "equity".to_owned(),
            follon_accounting::MarginRate {
                initial_bps: 5000,
                maintenance_bps: 2500,
            },
        );
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets,
            margin_rates: Some(margin_rates),
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();

        // Buy and fill 1,000 shares at 100 -- cash is fully spent, so equity
        // (100,000) equals the position's mark value exactly.
        let filled = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "1000").unwrap(),
                    ..intent("intent-margin-buy", "2026-01-02T14:31:00Z")
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1000").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // A second order against the same mark: real margin_used is now
        // 100,000 * 50% = 50,000 against equity of 100,000, a genuine 50%
        // utilization exceeding the configured 40% limit.
        let mut sell = intent("intent-margin-sell", "2026-01-02T14:32:00Z");
        sell.side = Side::Sell;
        sell.quantity = decimal("quantity", "1").unwrap();
        let result = service
            .submit_intent(
                sell,
                market_at_price("100", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"MAX_MARGIN_UTILIZATION_EXCEEDED".to_owned()));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_margin_used=50000.00000000"));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_margin_utilization_bps=5000.00000000"));
    }

    #[test]
    fn paper_portfolio_risk_composition_fails_closed_when_a_held_position_has_no_margin_rate() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        let mut instrument_buckets = BTreeMap::new();
        instrument_buckets.insert(
            "inst.us_equity.spy".to_owned(),
            InstrumentBucket {
                asset_class: "equity".to_owned(),
                currency: "USD".to_owned(),
                sector: "index".to_owned(),
            },
        );
        // A margin_rates map that covers a *different* asset class than the
        // one actually held: `value_margin_account` requires a rate for
        // every asset class among currently held positions, so this must
        // fail closed with a technical error rather than silently treating
        // the uncovered position as zero margin.
        let mut margin_rates = BTreeMap::new();
        margin_rates.insert(
            "option".to_owned(),
            follon_accounting::MarginRate {
                initial_bps: 5000,
                maintenance_bps: 2500,
            },
        );
        portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets,
            margin_rates: Some(margin_rates),
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();
        let filled = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "1000").unwrap(),
                    ..intent("intent-margin-gap-buy", "2026-01-02T14:31:00Z")
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1000").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        let mut sell = intent("intent-margin-gap-sell", "2026-01-02T14:32:00Z");
        sell.side = Side::Sell;
        sell.quantity = decimal("quantity", "1").unwrap();
        let error = service
            .submit_intent(
                sell,
                market_at_price("100", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap_err();
        assert!(error.0.contains("missing margin policy"));
    }

    #[test]
    fn paper_portfolio_risk_composition_rejects_when_strategy_limit_is_exceeded() {
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        // Never trip on leverage/concentration/gross; this test only wants
        // the strategy-bucket check exercised.
        portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
        portfolio_policy.max_gross_exposure = decimal("gross", "1000000").unwrap();
        let mut strategy_limits = BTreeMap::new();
        strategy_limits.insert(
            "strategy.beta".to_owned(),
            decimal("limit", "25000").unwrap(),
        );
        portfolio_policy.strategy_limits = strategy_limits;
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut service = PaperTradingService::new(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
        )
        .unwrap();

        // Strategy "strategy.paper.001" (the default test strategy) buys and
        // fills 100 shares at 100 -- a real, durably attributed 10,000
        // position for that strategy alone.
        let filled = service
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "100").unwrap(),
                    ..intent("intent-strategy-alpha-buy", "2026-01-02T14:31:00Z")
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        service
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "100").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(service.synchronize().unwrap(), 2);

        // A second, distinct strategy ("strategy.beta") submits a 300-share
        // buy of the *same* instrument at the same mark. Its own candidate
        // notional alone (30,000) already exceeds the configured 25,000
        // strategy.beta limit, independent of strategy.paper.001's already-
        // filled 10,000 position -- proving the two strategies are tracked
        // and limited separately, not pooled into one aggregate bucket.
        let mut beta_buy = intent("intent-strategy-beta-buy", "2026-01-02T14:32:00Z");
        beta_buy.strategy_id = "strategy.beta".to_owned();
        beta_buy.quantity = decimal("quantity", "300").unwrap();
        let result = service
            .submit_intent(
                beta_buy,
                market_at_price("100", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"STRATEGY_LIMIT_EXCEEDED:strategy.beta".to_owned()));
        // Total gross exposure reflects both strategies exactly (10,000 from
        // the filled strategy.paper.001 position plus 30,000 from the
        // rejected strategy.beta candidate), proving the per-strategy split
        // never mis-states the true aggregate.
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_gross_exposure=40000.00000000"));
        assert!(result
            .decision
            .evaluated_limits
            .contains("strategy.beta:30000.00000000"));
    }

    #[test]
    fn paper_strategy_attribution_survives_a_durable_journal_reopen() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-strategy-attribution.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_leverage_bps = decimal("leverage", "1000000").unwrap();
        portfolio_policy.max_gross_exposure = decimal("gross", "1000000").unwrap();
        let mut strategy_limits = BTreeMap::new();
        strategy_limits.insert(
            "strategy.beta".to_owned(),
            decimal("limit", "25000").unwrap(),
        );
        portfolio_policy.strategy_limits = strategy_limits;
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1000").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut durable = PaperTradingService::open_durable(
            paper_account.clone(),
            risk_policy.clone(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();

        // Strategy "strategy.paper.001" buys and fills 100 shares at 100.
        let filled = durable
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "100").unwrap(),
                    ..intent(
                        "intent-attribution-durability-alpha",
                        "2026-01-02T14:31:00Z",
                    )
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        durable
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "100").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(durable.synchronize().unwrap(), 2);
        drop(durable);

        // Reopen: the durable per-strategy attribution (100 shares owned by
        // strategy.paper.001) must survive, so a fresh strategy.beta candidate
        // still sees the correct pre-existing gross exposure and its own
        // limit is still evaluated against exactly its own contribution.
        let mut reopened = PaperTradingService::open_durable(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();
        reopened
            .reconnect_and_reconcile("2026-01-02T14:32:00Z")
            .unwrap();
        let mut beta_buy = intent("intent-attribution-durability-beta", "2026-01-02T14:32:00Z");
        beta_buy.strategy_id = "strategy.beta".to_owned();
        beta_buy.quantity = decimal("quantity", "300").unwrap();
        let result = reopened
            .submit_intent(
                beta_buy,
                market_at_price("100", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"STRATEGY_LIMIT_EXCEEDED:strategy.beta".to_owned()));
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_gross_exposure=40000.00000000"));
        fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn paper_portfolio_risk_composition_uses_a_durable_mark_cache_after_journal_reopen() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-portfolio-risk-marks.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_gross_exposure = decimal("gross", "200").unwrap();
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut durable = PaperTradingService::open_durable(
            paper_account.clone(),
            risk_policy.clone(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();

        // Buy and fill instrument A (spy) at 90, so its average cost is 90.
        let filled = durable
            .submit_intent(
                intent("intent-portfolio-risk-marks-a", "2026-01-02T14:31:00Z"),
                market_at_price("90", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        durable
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1").unwrap(),
                decimal("price", "90").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(durable.synchronize().unwrap(), 2);

        // Re-quote A at 150. This order is rejected on quantity alone, but the
        // mark observation is cached unconditionally before any check runs.
        let mut requote = intent(
            "intent-portfolio-risk-marks-requote",
            "2026-01-02T14:32:00Z",
        );
        requote.quantity = decimal("quantity", "1000").unwrap();
        let requoted = durable
            .submit_intent(
                requote,
                market_at_price("150", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        assert!(!requoted.decision.approved);
        assert!(requoted
            .decision
            .reason_codes
            .contains(&"MAX_ORDER_QUANTITY_EXCEEDED".to_owned()));
        drop(durable);

        // Reopen: the cached mark for A (150) must survive and be used, not
        // fall back to its average cost (90).
        let mut reopened = PaperTradingService::open_durable(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();
        reopened
            .reconnect_and_reconcile("2026-01-02T14:33:00Z")
            .unwrap();
        let mut second_intent = intent("intent-portfolio-risk-marks-b", "2026-01-02T14:33:00Z");
        second_intent.instrument_id = "inst.us_equity.qqq".to_owned();
        let second_market = PaperMarketData {
            instrument_id: "inst.us_equity.qqq".to_owned(),
            mark_price: decimal("mark", "60").unwrap(),
            observed_at: "2026-01-02T14:33:00Z".to_owned(),
        };
        let result = reopened
            .submit_intent(second_intent, second_market, "2026-01-02T14:33:00Z")
            .unwrap();
        assert!(!result.decision.approved);
        assert!(result
            .decision
            .reason_codes
            .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
        // 1 (A) * 150 (cached mark, not the 90 average cost) + 1 (B) * 60.
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_gross_exposure=210.00000000"));
        fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn paper_peak_equity_survives_a_durable_journal_reopen() {
        let journal_path = std::env::temp_dir().join(format!(
            "follon-paper-journal-{}-peak-equity.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&journal_path);
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = decimal("quantity", "1001").unwrap();
        risk_policy.max_order_notional = decimal("notional", "200000").unwrap();
        risk_policy.max_position_quantity = decimal("position", "2000").unwrap();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let paper_account = account();
        let mut durable = PaperTradingService::open_durable(
            paper_account.clone(),
            risk_policy.clone(),
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();

        // Buy and fill 1,000 shares at 100 -- equity starts at 100,000.
        let filled = durable
            .submit_intent(
                OrderIntent {
                    quantity: decimal("quantity", "1000").unwrap(),
                    ..intent("intent-peak-equity-buy", "2026-01-02T14:31:00Z")
                },
                market_at_price("100", "2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(filled.decision.approved);
        let order_id = filled.order_id.unwrap();
        durable
            .broker_mut()
            .queue_fill(
                &order_id,
                decimal("quantity", "1000").unwrap(),
                decimal("price", "100").unwrap(),
                Decimal::ZERO,
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert_eq!(durable.synchronize().unwrap(), 2);

        // Re-quote to 150: a real mark-to-market gain pushes equity to
        // 150,000, raising the peak. This order is rejected on quantity
        // alone, but the peak-equity update is unconditional (see
        // `evaluate_risk`).
        let mut spike = intent("intent-peak-equity-spike", "2026-01-02T14:32:00Z");
        spike.quantity = decimal("quantity", "1002").unwrap();
        let spiked = durable
            .submit_intent(
                spike,
                market_at_price("150", "2026-01-02T14:32:00Z"),
                "2026-01-02T14:32:00Z",
            )
            .unwrap();
        assert!(!spiked.decision.approved);
        assert!(spiked
            .decision
            .evaluated_limits
            .contains("portfolio_peak_equity=150000.00000000"));

        // Re-quote back down to 100: equity falls back to 100,000, but the
        // peak must not fall with it.
        let mut retreat = intent("intent-peak-equity-retreat", "2026-01-02T14:33:00Z");
        retreat.quantity = decimal("quantity", "1002").unwrap();
        let retreated = durable
            .submit_intent(
                retreat,
                market_at_price("100", "2026-01-02T14:33:00Z"),
                "2026-01-02T14:33:00Z",
            )
            .unwrap();
        assert!(!retreated.decision.approved);
        assert!(retreated
            .decision
            .evaluated_limits
            .contains("portfolio_peak_equity=150000.00000000"));
        drop(durable);

        // Reopen: the durable peak (150,000) must survive, not reset to
        // today's current equity (100,000).
        let mut reopened = PaperTradingService::open_durable(
            paper_account.clone(),
            risk_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&paper_account).unwrap(),
            &journal_path,
        )
        .unwrap();
        reopened
            .reconnect_and_reconcile("2026-01-02T14:34:00Z")
            .unwrap();
        let mut probe = intent("intent-peak-equity-after-reopen", "2026-01-02T14:34:00Z");
        probe.quantity = decimal("quantity", "1002").unwrap();
        let result = reopened
            .submit_intent(
                probe,
                market_at_price("100", "2026-01-02T14:34:00Z"),
                "2026-01-02T14:34:00Z",
            )
            .unwrap();
        assert!(result
            .decision
            .evaluated_limits
            .contains("portfolio_peak_equity=150000.00000000"));
        fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn paper_order_rate_limit_rejects_submissions_beyond_the_configured_window() {
        let mut rate_limited_policy = policy();
        rate_limited_policy.max_order_rate = 2;
        let mut service = PaperTradingService::new(
            account(),
            rate_limited_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account()).unwrap(),
        )
        .unwrap();
        let first = service
            .submit_intent(
                intent("intent-paper-rate-001", "2026-01-02T14:31:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(first.decision.approved);
        let second = service
            .submit_intent(
                intent("intent-paper-rate-002", "2026-01-02T14:31:01Z"),
                market("2026-01-02T14:31:01Z"),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert!(second.decision.approved);
        let third = service
            .submit_intent(
                intent("intent-paper-rate-003", "2026-01-02T14:31:02Z"),
                market("2026-01-02T14:31:02Z"),
                "2026-01-02T14:31:02Z",
            )
            .unwrap();
        assert!(!third.decision.approved);
        assert!(third.order_id.is_none());
        assert!(third
            .decision
            .reason_codes
            .contains(&"MAX_ORDER_RATE_EXCEEDED".to_owned()));
        assert!(third
            .decision
            .evaluated_limits
            .contains("recent_order_count=2"));
    }

    #[test]
    fn paper_order_rate_limit_counts_decision_time_not_caller_supplied_created_at() {
        let mut rate_limited_policy = policy();
        rate_limited_policy.max_order_rate = 2;
        let mut service = PaperTradingService::new(
            account(),
            rate_limited_policy,
            KillSwitchRegistry::new("paper-kills-v1").unwrap(),
            IbkrPaperAdapter::new(&account()).unwrap(),
        )
        .unwrap();
        // Every intent claims a `created_at` far outside the rate window, but each
        // is actually decided within it: a caller must not be able to understate
        // its own submission rate and bypass `MAX_ORDER_RATE_EXCEEDED` by
        // backdating the intent's self-reported `created_at`.
        let first = service
            .submit_intent(
                intent("intent-paper-rate-backdate-001", "2020-01-01T00:00:00Z"),
                market("2026-01-02T14:31:00Z"),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        assert!(first.decision.approved);
        let second = service
            .submit_intent(
                intent("intent-paper-rate-backdate-002", "2020-01-01T00:00:00Z"),
                market("2026-01-02T14:31:01Z"),
                "2026-01-02T14:31:01Z",
            )
            .unwrap();
        assert!(second.decision.approved);
        let third = service
            .submit_intent(
                intent("intent-paper-rate-backdate-003", "2020-01-01T00:00:00Z"),
                market("2026-01-02T14:31:02Z"),
                "2026-01-02T14:31:02Z",
            )
            .unwrap();
        assert!(!third.decision.approved);
        assert!(third.order_id.is_none());
        assert!(third
            .decision
            .reason_codes
            .contains(&"MAX_ORDER_RATE_EXCEEDED".to_owned()));
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
