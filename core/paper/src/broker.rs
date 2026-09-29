//! Broker-neutral PAPER adapter contract: routes, requests, results, events and snapshots.

use follon_control_plane::{OmsComboOrder, OmsOrder};
use follon_domain::{validate_canonical_id, Decimal, OrderState, Side};

use crate::*;

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
    pub(crate) fn from_combo_order(order: &OmsComboOrder) -> Result<Self, PaperError> {
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

    pub(crate) fn validate(&self) -> Result<(), PaperError> {
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
    pub(crate) fn from_order(order: &OmsOrder) -> Self {
        Self {
            client_order_id: order.order_id.clone(),
            account_id: order.intent.account_id.clone(),
            instrument_id: order.intent.instrument_id.clone(),
            side: order.intent.side,
            quantity: order.intent.quantity,
            limit_price: order.intent.limit_price,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), PaperError> {
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
    pub(crate) fn validate(&self) -> Result<(), PaperError> {
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
    pub(crate) fn validate(&self) -> Result<(), PaperError> {
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

/// What one PAPER broker route can carry to its venue.
///
/// The service consults this before it evaluates risk or creates an order,
/// so a request the route cannot carry is refused with nothing recorded,
/// nothing transmitted and no `UNKNOWN` order left behind. Without it, a
/// route whose adapter cannot execute a request reported the refusal as a
/// transport failure: the order became `UNKNOWN`, the session disconnected,
/// and nothing could ever clear it (delivery state E5.1).
///
/// The derived default is the narrowest set, single DAY orders. An adapter
/// declares anything more.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PaperBrokerCapabilities {
    /// Executes an atomic multi-leg combination as one order.
    pub combinations: bool,
    /// Carries a good-til-cancelled time in force to the venue as such,
    /// rather than dropping or rewriting it.
    pub good_til_cancelled: bool,
    /// Replaces a working order's limit price.
    pub replacement: bool,
}

/// Paper-only broker boundary. It has no live environment parameter.
pub trait PaperBrokerAdapter {
    /// Declares what this adapter can carry for one account.
    ///
    /// The default is [`PaperBrokerCapabilities::default`], single DAY
    /// orders, so an adapter that declares nothing is never handed a
    /// combination, a GTC order or a replacement it would refuse, drop or
    /// rewrite.
    fn capabilities(&self, _account_id: &str) -> Result<PaperBrokerCapabilities, PaperError> {
        Ok(PaperBrokerCapabilities::default())
    }
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
