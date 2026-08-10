//! Controlled-live safety boundary, approvals, audited canary execution, and recovery evidence.
//!
//! This crate intentionally contains no concrete broker wire client and no credential source.
//! A deployment must supply both an audited [`LiveBrokerAdapter`] and a managed
//! [`SecretProvider`]. The core defaults to shadow mode, requires separate human
//! requester/approver identities for every canary order, and fails closed on any
//! audit-journal uncertainty.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_control_plane::{EngineError, OmsOrder, Portfolio};
use follon_domain::{
    validate_canonical_id, validate_utc_timestamp, Decimal, Fill, OrderIntent, OrderState,
    RiskDecision, Side,
};
use follon_instrument::{TradingCalendar, TradingSession};
use follon_secrets::{SecretMaterial, SecretProvider, SecretReference};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const LIVE_JOURNAL_SCHEMA_VERSION: u32 = 1;
const MAX_LIVE_JOURNAL_BYTES: u64 = 128 * 1024 * 1024;

/// Controlled-live configuration, authorization, persistence, or broker failure.
#[derive(Debug)]
pub struct LiveError(pub String);

impl std::fmt::Display for LiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LiveError {}

impl From<EngineError> for LiveError {
    fn from(error: EngineError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DomainError> for LiveError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DecimalError> for LiveError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

impl From<follon_secrets::SecretError> for LiveError {
    fn from(error: follon_secrets::SecretError) -> Self {
        Self(error.0)
    }
}

/// Explicitly bounded LIVE account. It is never constructed from an environment variable.
#[derive(Clone, Debug)]
pub struct LiveAccount {
    /// Canonical broker account identity.
    pub account_id: String,
    /// Single reporting currency for this controlled-live phase.
    pub currency: String,
    /// Independently tracked opening cash balance.
    pub initial_cash: Decimal,
    /// Hard ceiling on deployed live capital, independent of broker buying power.
    pub max_deployed_capital: Decimal,
    /// Must be the literal `LIVE` value.
    pub environment: String,
    /// Managed credential reference. Secret bytes never enter configuration or audit records.
    pub credential_reference: SecretReference,
}

impl LiveAccount {
    /// Validates the controlled-live account boundary.
    pub fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("live account_id", &self.account_id)?;
        if self.currency.len() != 3
            || !self
                .currency
                .bytes()
                .all(|value| value.is_ascii_uppercase())
            || self.initial_cash < Decimal::ZERO
            || self.max_deployed_capital <= Decimal::ZERO
            || self.max_deployed_capital > self.initial_cash
            || self.environment != "LIVE"
        {
            return Err(LiveError(
                "live account must have uppercase currency, bounded capital, and LIVE environment"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// Immutable live risk and canary limits.
#[derive(Clone, Debug)]
pub struct LiveRiskPolicy {
    /// Immutable policy version recorded in every decision.
    pub version: String,
    /// Versioned session calendar governing the daily live gate.
    pub trading_calendar_id: String,
    /// Maximum one-order quantity.
    pub max_order_quantity: Decimal,
    /// Maximum one-order estimated notional.
    pub max_order_notional: Decimal,
    /// Maximum one-order canary notional; this must not exceed `max_order_notional`.
    pub canary_max_order_notional: Decimal,
    /// Maximum number of live canary submissions since activation.
    pub canary_max_orders: u32,
    /// Maximum number of non-terminal orders.
    pub max_open_orders: usize,
    /// Maximum long-only position quantity per instrument.
    pub max_position_quantity: Decimal,
    /// Maximum aggregate realized loss before new live entries are blocked.
    pub max_realized_loss: Decimal,
    /// Maximum age of the exact market observation at decision time.
    pub max_market_data_age_seconds: u64,
}

impl LiveRiskPolicy {
    /// Validates all non-negotiable live limits.
    pub fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("live trading_calendar_id", &self.trading_calendar_id)?;
        if self.version.is_empty()
            || self.max_order_quantity <= Decimal::ZERO
            || self.max_order_notional <= Decimal::ZERO
            || self.canary_max_order_notional <= Decimal::ZERO
            || self.canary_max_order_notional > self.max_order_notional
            || self.canary_max_orders == 0
            || self.max_open_orders == 0
            || self.max_position_quantity <= Decimal::ZERO
            || self.max_realized_loss < Decimal::ZERO
            || self.max_market_data_age_seconds == 0
        {
            return Err(LiveError("invalid controlled-live risk policy".to_owned()));
        }
        Ok(())
    }
}

/// Exact market observation evaluated at the controlled-live boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveMarketData {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Positive fixed-point mark.
    pub mark_price: Decimal,
    /// Canonical UTC observation time.
    pub observed_at: String,
}

impl LiveMarketData {
    fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("live market instrument_id", &self.instrument_id)?;
        validate_utc_timestamp("live market observed_at", &self.observed_at)?;
        if self.mark_price <= Decimal::ZERO {
            return Err(LiveError("live market mark must be positive".to_owned()));
        }
        Ok(())
    }
}

/// Mode selected before an operational run begins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRunMode {
    /// Records production-market decisions but cannot connect to or submit at a broker.
    Shadow,
    /// Allows bounded LIVE submissions only after connection and per-order approval.
    Canary,
}

impl LiveRunMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Shadow => "SHADOW",
            Self::Canary => "CANARY",
        }
    }
}

/// Time-bounded, four-eyes activation for one shadow or canary run.
#[derive(Clone, Debug)]
pub struct LiveActivation {
    /// Canonical immutable activation identity.
    pub activation_id: String,
    /// Shadow or canary scope.
    pub mode: LiveRunMode,
    /// SHA-256 fingerprint of the exact account and policy configuration.
    pub configuration_fingerprint: String,
    /// Operator requesting activation.
    pub requested_by: String,
    /// Different operator approving activation.
    pub approved_by: String,
    /// Canonical UTC activation time.
    pub activated_at: String,
    /// Canonical UTC expiry. New live work fails after this instant.
    pub expires_at: String,
}

/// Human approval material used to bind a controlled-live activation to immutable controls.
#[derive(Clone, Debug)]
pub struct LiveActivationRequest {
    /// Canonical immutable activation identity.
    pub activation_id: String,
    /// Shadow or canary scope.
    pub mode: LiveRunMode,
    /// Operator requesting activation.
    pub requested_by: String,
    /// Different operator approving activation.
    pub approved_by: String,
    /// Canonical UTC activation time.
    pub activated_at: String,
    /// Canonical UTC expiry.
    pub expires_at: String,
}

impl LiveActivation {
    /// Creates an activation cryptographically bound to the supplied immutable live controls.
    ///
    /// This is the preferred deployment entry point: callers cannot accidentally copy a
    /// fingerprint from a different account, risk policy, or kill-switch revision.
    pub fn for_configuration(
        request: LiveActivationRequest,
        account: &LiveAccount,
        policy: &LiveRiskPolicy,
        kill_switches: &LiveKillSwitchRegistry,
    ) -> Result<Self, LiveError> {
        let activation = Self {
            activation_id: request.activation_id,
            mode: request.mode,
            configuration_fingerprint: configuration_fingerprint(account, policy, kill_switches),
            requested_by: request.requested_by,
            approved_by: request.approved_by,
            activated_at: request.activated_at,
            expires_at: request.expires_at,
        };
        activation.validate(&configuration_fingerprint(account, policy, kill_switches))?;
        Ok(activation)
    }

    fn validate(&self, expected_configuration_fingerprint: &str) -> Result<(), LiveError> {
        for (name, value) in [
            ("live activation_id", self.activation_id.as_str()),
            ("live activation requester", self.requested_by.as_str()),
            ("live activation approver", self.approved_by.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_utc_timestamp("live activation time", &self.activated_at)?;
        validate_utc_timestamp("live activation expiry", &self.expires_at)?;
        if self.requested_by == self.approved_by
            || self.activated_at >= self.expires_at
            || self.configuration_fingerprint != expected_configuration_fingerprint
        {
            return Err(LiveError(
                "live activation requires distinct approver, valid interval, and exact configuration"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn active_at(&self, timestamp: &str) -> Result<bool, LiveError> {
        validate_utc_timestamp("live activation check time", timestamp)?;
        Ok(self.activated_at.as_str() <= timestamp && timestamp < self.expires_at.as_str())
    }
}

/// Four-eyes approval bound to exact intent bytes and configuration.
#[derive(Clone, Debug)]
pub struct LiveApproval {
    /// Canonical approval identity.
    pub approval_id: String,
    /// Intent identity being approved.
    pub intent_id: String,
    /// SHA-256 fingerprint of the exact intent.
    pub intent_fingerprint: String,
    /// Matching operational configuration fingerprint.
    pub configuration_fingerprint: String,
    /// Strategy/operator requester identity.
    pub requested_by: String,
    /// Human approval identity; it must differ from the requester.
    pub approved_by: String,
    /// Canonical UTC approval time.
    pub approved_at: String,
    /// Canonical UTC expiry; each approval is single-use and time bounded.
    pub expires_at: String,
}

impl LiveApproval {
    fn validate(&self, expected_configuration_fingerprint: &str) -> Result<(), LiveError> {
        for (name, value) in [
            ("live approval_id", self.approval_id.as_str()),
            ("live approval intent_id", self.intent_id.as_str()),
            ("live approval requester", self.requested_by.as_str()),
            ("live approval approver", self.approved_by.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_utc_timestamp("live approval time", &self.approved_at)?;
        validate_utc_timestamp("live approval expiry", &self.expires_at)?;
        if self.requested_by == self.approved_by
            || self.approved_at >= self.expires_at
            || self.intent_fingerprint.len() != 64
            || !self
                .intent_fingerprint
                .bytes()
                .all(|value| value.is_ascii_hexdigit())
            || self.configuration_fingerprint != expected_configuration_fingerprint
        {
            return Err(LiveError(
                "live approval must bind exact configuration and use distinct approver".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Normalized LIVE broker submission request created only by the controlled OMS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerOrderRequest {
    /// OMS client idempotency key.
    pub client_order_id: String,
    /// Configured live account identity.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Requested side.
    pub side: Side,
    /// Exact requested quantity.
    pub quantity: Decimal,
    /// Optional limit price.
    pub limit_price: Option<Decimal>,
}

impl LiveBrokerOrderRequest {
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

    fn validate(&self) -> Result<(), LiveError> {
        for (name, value) in [
            ("live broker client_order_id", self.client_order_id.as_str()),
            ("live broker account_id", self.account_id.as_str()),
            ("live broker instrument_id", self.instrument_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.quantity <= Decimal::ZERO
            || self.limit_price.is_some_and(|value| value <= Decimal::ZERO)
        {
            return Err(LiveError("invalid live broker request".to_owned()));
        }
        Ok(())
    }
}

/// Broker result for one idempotent live submission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveBrokerSubmitResult {
    /// Broker definitively accepted the client order identity.
    Acknowledged {
        /// Broker-native immutable order identity.
        broker_order_id: String,
    },
    /// Broker definitively refused the request.
    Rejected {
        /// Stable rejection reason safe for audit logs.
        reason: String,
    },
    /// Submission outcome cannot be proved until reconciliation.
    Unknown {
        /// Stable ambiguity reason safe for audit logs.
        reason: String,
    },
}

/// Normalized asynchronous evidence from an audited live broker adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveBrokerEvent {
    /// Broker acknowledgement.
    Acknowledged {
        /// OMS client idempotency key.
        client_order_id: String,
        /// Broker-native immutable order identity.
        broker_order_id: String,
    },
    /// One exact broker execution.
    Execution {
        /// Broker execution identity.
        execution_id: String,
        /// OMS client idempotency key.
        client_order_id: String,
        /// Broker-native order identity.
        broker_order_id: String,
        /// Filled quantity.
        quantity: Decimal,
        /// Execution price.
        price: Decimal,
        /// Commission in account currency.
        fee: Decimal,
        /// Canonical UTC execution time.
        executed_at: String,
    },
    /// Broker cancellation confirmation.
    Cancelled {
        /// OMS client idempotency key.
        client_order_id: String,
        /// Stable cancellation reason.
        reason: String,
    },
    /// Broker rejection confirmation.
    Rejected {
        /// OMS client idempotency key.
        client_order_id: String,
        /// Stable rejection reason.
        reason: String,
    },
}

/// One order in an independent broker snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerOrderSnapshot {
    /// OMS client idempotency identity.
    pub client_order_id: String,
    /// Broker-native order identity.
    pub broker_order_id: String,
    /// Normalized broker lifecycle state.
    pub state: OrderState,
    /// Exact broker-reported filled quantity.
    pub filled_quantity: Decimal,
}

/// One position in an independent broker snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerPositionSnapshot {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Signed exact quantity reported by the broker.
    pub quantity: Decimal,
}

/// Independent broker view required for live reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerAccountSnapshot {
    /// Orders visible at broker.
    pub orders: Vec<LiveBrokerOrderSnapshot>,
    /// Positions visible at broker.
    pub positions: Vec<LiveBrokerPositionSnapshot>,
    /// Broker-reported cash in account currency.
    pub cash: Decimal,
}

/// Audited live broker interface. Implementations must be deployment-edge code.
pub trait LiveBrokerAdapter {
    /// Establishes a live session using secret bytes only at the adapter boundary.
    fn connect(&mut self, account_id: &str, credential: &SecretMaterial) -> Result<(), LiveError>;
    /// Submits one OMS-generated idempotent live order.
    fn submit(
        &mut self,
        request: &LiveBrokerOrderRequest,
    ) -> Result<LiveBrokerSubmitResult, LiveError>;
    /// Requests cancellation by client idempotency identity.
    fn cancel(&mut self, client_order_id: &str) -> Result<(), LiveError>;
    /// Drains normalized asynchronous broker evidence.
    fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError>;
    /// Gets independent broker state for reconciliation.
    fn snapshot(&mut self, account_id: &str) -> Result<LiveBrokerAccountSnapshot, LiveError>;
    /// Re-establishes a previously configured live session after transport loss.
    fn reconnect(&mut self, account_id: &str, credential: &SecretMaterial)
        -> Result<(), LiveError>;
}

/// Scope for an independently operated controlled-live kill switch.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LiveKillSwitchScope {
    /// Stops every new controlled-live order.
    Global,
    /// Stops one account.
    Account(String),
    /// Stops one strategy.
    Strategy(String),
    /// Stops one instrument.
    Instrument(String),
}

impl LiveKillSwitchScope {
    /// Stable audit and dashboard identity.
    pub fn as_key(&self) -> String {
        match self {
            Self::Global => "global".to_owned(),
            Self::Account(value) => format!("account:{value}"),
            Self::Strategy(value) => format!("strategy:{value}"),
            Self::Instrument(value) => format!("instrument:{value}"),
        }
    }

    fn validate(&self) -> Result<(), LiveError> {
        match self {
            Self::Global => Ok(()),
            Self::Account(value) => {
                validate_canonical_id("live kill account", value).map_err(Into::into)
            }
            Self::Strategy(value) => {
                validate_canonical_id("live kill strategy", value).map_err(Into::into)
            }
            Self::Instrument(value) => {
                validate_canonical_id("live kill instrument", value).map_err(Into::into)
            }
        }
    }
}

/// Versioned independent live kill-switch registry.
#[derive(Clone, Debug)]
pub struct LiveKillSwitchRegistry {
    /// Immutable registry revision.
    pub version: String,
    active: BTreeSet<LiveKillSwitchScope>,
}

impl LiveKillSwitchRegistry {
    /// Creates an empty registry with an immutable revision.
    pub fn new(version: impl Into<String>) -> Result<Self, LiveError> {
        let registry = Self {
            version: version.into(),
            active: BTreeSet::new(),
        };
        if registry.version.is_empty() {
            return Err(LiveError("live kill-switch version is required".to_owned()));
        }
        Ok(registry)
    }

    /// Activates one scope independently of strategy and broker health.
    pub fn activate(&mut self, scope: LiveKillSwitchScope) -> Result<bool, LiveError> {
        scope.validate()?;
        Ok(self.active.insert(scope))
    }

    /// Explicitly deactivates one scope.
    pub fn deactivate(&mut self, scope: &LiveKillSwitchScope) -> bool {
        self.active.remove(scope)
    }

    /// Lists active scopes deterministically.
    pub fn active_keys(&self) -> Vec<String> {
        self.active
            .iter()
            .map(LiveKillSwitchScope::as_key)
            .collect()
    }

    fn rejection_reasons(&self, intent: &OrderIntent) -> Vec<String> {
        [
            LiveKillSwitchScope::Global,
            LiveKillSwitchScope::Account(intent.account_id.clone()),
            LiveKillSwitchScope::Strategy(intent.strategy_id.clone()),
            LiveKillSwitchScope::Instrument(intent.instrument_id.clone()),
        ]
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

/// Durable internal OMS record for one canary order.
#[derive(Clone, Debug)]
pub struct LiveOrder {
    /// Legal OMS lifecycle state.
    pub oms: OmsOrder,
    /// Single-use approval consumed by this order.
    pub approval_id: String,
    /// Exact market observation used for risk and cash reservation.
    pub market: LiveMarketData,
    /// Immutable risk outcome that authorized this exact order.
    pub decision: LiveRiskDecision,
    /// Broker identity after it becomes known.
    pub broker_order_id: Option<String>,
    /// Exact independently-accounted fill quantity.
    pub filled_quantity: Decimal,
}

impl LiveOrder {
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

    fn reserved_cash(&self) -> Result<Decimal, LiveError> {
        if !self.working() || self.oms.intent.side != Side::Buy {
            return Ok(Decimal::ZERO);
        }
        self.oms
            .intent
            .quantity
            .checked_sub(self.filled_quantity)?
            .checked_mul(self.market.mark_price)
            .map_err(Into::into)
    }
}

/// Registered approval and whether it has already authorized one canary submission.
#[derive(Clone, Debug)]
pub struct RegisteredLiveApproval {
    /// Immutable approval material.
    pub approval: LiveApproval,
    /// Single-use replay guard.
    pub consumed: bool,
}

/// Live risk decision emitted before any canary OMS order exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveRiskDecision {
    /// Stable decision identity.
    pub decision_id: String,
    /// Approved state.
    pub approved: bool,
    /// Machine-readable outcomes.
    pub reason_codes: Vec<String>,
    /// Policy revision evaluated.
    pub policy_version: String,
    /// UTC decision time.
    pub decided_at: String,
    /// Exact market observation fingerprint.
    pub market_fingerprint: String,
}

/// Result of a shadow recording or controlled-live canary submission.
#[derive(Clone, Debug)]
pub enum LiveSubmitOutcome {
    /// A production-data shadow decision was recorded and never reached a broker.
    ShadowRecorded {
        /// Risk result recorded in the audit chain.
        decision: LiveRiskDecision,
    },
    /// A canary risk rejection created no broker order.
    RiskRejected {
        /// Rejection result recorded in the audit chain.
        decision: LiveRiskDecision,
    },
    /// A canary order was attempted under one consumed approval.
    CanaryOrder {
        /// Original risk result bound to this order.
        decision: LiveRiskDecision,
        /// OMS order identity.
        order_id: String,
        /// Current OMS state.
        state: OrderState,
    },
}

/// One immutable reconciliation difference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveReconciliationIssue {
    /// Stable issue identity.
    pub incident_id: String,
    /// Machine-readable discrepancy category.
    pub category: String,
    /// Canonical order, instrument, or account subject.
    pub subject: String,
    /// Deterministic comparison detail.
    pub detail: String,
}

/// Independent broker reconciliation result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveReconciliationReport {
    /// Stable reconciliation identity.
    pub reconciliation_id: String,
    /// UTC reconciliation completion time.
    pub reconciled_at: String,
    /// Differences which were observed at this checkpoint.
    pub issues: Vec<LiveReconciliationIssue>,
}

impl LiveReconciliationReport {
    /// Whether all independent broker and internal values agreed.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// A discrepancy remains blocking until it has an explicit accountable explanation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveIncident {
    /// Original observed issue.
    pub issue: LiveReconciliationIssue,
    /// An attributable operator explanation, if one has been recorded.
    pub explanation: Option<String>,
}

impl LiveIncident {
    fn unexplained(&self) -> bool {
        self.explanation.is_none()
    }
}

/// Measured 60-small-capital-live-day gate state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivePromotionStatus {
    /// Number of distinct clean closed live sessions.
    pub clean_live_days: u32,
    /// Required controlled-live sessions, always sixty.
    pub required_live_days: u32,
    /// Outstanding incidents without an explanation.
    pub unresolved_incidents: u32,
    /// Whether the service has journal evidence for all recorded state changes.
    pub complete_auditability: bool,
    /// Whether the measured gate is complete.
    pub eligible_for_next_gate: bool,
}

/// Read-only controlled-live monitoring projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LiveMonitoringDashboard {
    /// Schema version.
    pub dashboard_schema_version: u32,
    /// Always `LIVE` for this projection.
    pub environment: String,
    /// `SHADOW` or `CANARY`.
    pub mode: String,
    /// Canonical account ID.
    pub account_id: String,
    /// Exact configuration fingerprint.
    pub configuration_fingerprint: String,
    /// Whether an adapter session is currently established.
    pub broker_connected: bool,
    /// Whether durable audit writes have stayed healthy in this process.
    pub audit_healthy: bool,
    /// Current append-only audit sequence number.
    pub audit_sequence: u64,
    /// SHA-256 hash at the head of the audit chain.
    pub audit_head_hash: String,
    /// Active safety controls.
    pub active_kill_switches: Vec<String>,
    /// Number of non-terminal live orders.
    pub working_orders: u32,
    /// Number of ambiguous orders requiring reconciliation.
    pub unknown_orders: u32,
    /// Unexplained reconciliation incident count.
    pub unresolved_incidents: u32,
    /// Latest reconciliation time, if any.
    pub last_reconciled_at: Option<String>,
    /// Cleanliness of latest reconciliation, if any.
    pub last_reconciliation_clean: Option<bool>,
    /// Measured clean controlled-live days.
    pub clean_live_days: u32,
    /// Required controlled-live days.
    pub required_live_days: u32,
    /// Whether the next gate is eligible.
    pub promotion_eligible: bool,
    /// Whether every retained live-state transition is covered by the durable audit chain.
    pub complete_auditability: bool,
    /// Exact internal cash.
    pub internal_cash: String,
    /// Current position rows, deterministic by instrument.
    pub positions: Vec<LiveMonitoringPosition>,
}

/// One monitored internal live position.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LiveMonitoringPosition {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Exact quantity.
    pub quantity: String,
    /// Exact average cost.
    pub average_cost: String,
    /// Exact realized P&L.
    pub realized_pnl: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LiveJournalRecord {
    schema_version: u32,
    sequence: u64,
    previous_hash: String,
    event_type: String,
    occurred_at: String,
    actor: String,
    correlation_id: String,
    state: PersistentLiveState,
    entry_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentLiveState {
    configuration_fingerprint: String,
    account_id: String,
    currency: String,
    cash: String,
    orders: BTreeMap<String, PersistentLiveOrder>,
    approvals: BTreeMap<String, PersistentLiveApproval>,
    positions: BTreeMap<String, PersistentPosition>,
    execution_ids: Vec<String>,
    active_kill_switches: Vec<String>,
    incidents: BTreeMap<String, PersistentIncident>,
    live_days: BTreeMap<String, PersistentLiveDay>,
    canary_submissions: u32,
    next_reconciliation: u64,
    last_reconciled_at: Option<String>,
    last_reconciliation_clean: Option<bool>,
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
struct PersistentLiveOrder {
    intent: PersistentIntent,
    approval_id: String,
    state: String,
    market: PersistentMarketData,
    decision: PersistentRiskDecision,
    broker_order_id: Option<String>,
    filled_quantity: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentRiskDecision {
    decision_id: String,
    approved: bool,
    reason_codes: Vec<String>,
    policy_version: String,
    decided_at: String,
    market_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentLiveApproval {
    approval: PersistentApproval,
    consumed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentApproval {
    approval_id: String,
    intent_id: String,
    intent_fingerprint: String,
    configuration_fingerprint: String,
    requested_by: String,
    approved_by: String,
    approved_at: String,
    expires_at: String,
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
struct PersistentMarketData {
    instrument_id: String,
    mark_price: String,
    observed_at: String,
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
struct PersistentLiveDay {
    calendar_id: String,
    session_opens_at: String,
    session_closes_at: String,
    clean: bool,
    audit_head_hash: String,
}

/// Append-only, hash-chained, process-exclusive controlled-live audit journal.
pub struct LiveAuditJournal {
    path: PathBuf,
    file: File,
    next_sequence: u64,
    previous_hash: String,
    latest: Option<PersistentLiveState>,
}

impl LiveAuditJournal {
    /// Opens and validates the full journal before any controlled-live action is allowed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LiveError> {
        let path = path.as_ref().to_path_buf();
        if path.exists()
            && fs::symlink_metadata(&path)
                .map_err(|error| LiveError(error.to_string()))?
                .file_type()
                .is_symlink()
        {
            return Err(LiveError(
                "live audit journal path must not be a symbolic link".to_owned(),
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| LiveError(error.to_string()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .map_err(|error| LiveError(error.to_string()))?;
        file.try_lock_exclusive().map_err(|error| {
            LiveError(format!(
                "live audit journal is already open by another process: {error}"
            ))
        })?;
        let mut latest = None;
        let mut next_sequence = 1;
        let mut previous_hash = "0".repeat(64);
        let metadata = file
            .metadata()
            .map_err(|error| LiveError(error.to_string()))?;
        if metadata.len() > MAX_LIVE_JOURNAL_BYTES {
            return Err(LiveError(format!(
                "live audit journal exceeds {} bytes; archive and verify it before recovery",
                MAX_LIVE_JOURNAL_BYTES
            )));
        }
        if metadata.len() > 0 {
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .map_err(|error| LiveError(error.to_string()))?;
            for (index, line) in contents.lines().enumerate() {
                if line.is_empty() {
                    return Err(LiveError(format!(
                        "live journal has an empty line at {}",
                        index + 1
                    )));
                }
                let record: LiveJournalRecord = serde_json::from_str(line).map_err(|error| {
                    LiveError(format!("invalid live journal line {}: {error}", index + 1))
                })?;
                if serde_json::to_string(&record).map_err(|error| LiveError(error.to_string()))?
                    != line
                {
                    return Err(LiveError(format!(
                        "live journal line {} is not canonical JSON",
                        index + 1
                    )));
                }
                if record.schema_version != LIVE_JOURNAL_SCHEMA_VERSION
                    || record.sequence != next_sequence
                    || record.previous_hash != previous_hash
                    || record.entry_hash != audit_record_hash(&record)?
                {
                    return Err(LiveError(format!(
                        "live journal integrity check failed at line {}",
                        index + 1
                    )));
                }
                validate_audit_metadata(
                    &record.event_type,
                    &record.occurred_at,
                    &record.actor,
                    &record.correlation_id,
                )?;
                previous_hash = record.entry_hash.clone();
                next_sequence += 1;
                latest = Some(record.state);
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

    /// Returns the journal path for backup and disaster-recovery procedures.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn latest(&self) -> Option<&PersistentLiveState> {
        self.latest.as_ref()
    }

    fn sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    fn head_hash(&self) -> &str {
        &self.previous_hash
    }

    fn append(
        &mut self,
        event_type: &str,
        occurred_at: &str,
        actor: &str,
        correlation_id: &str,
        state: PersistentLiveState,
    ) -> Result<(), LiveError> {
        validate_audit_metadata(event_type, occurred_at, actor, correlation_id)?;
        let mut record = LiveJournalRecord {
            schema_version: LIVE_JOURNAL_SCHEMA_VERSION,
            sequence: self.next_sequence,
            previous_hash: self.previous_hash.clone(),
            event_type: event_type.to_owned(),
            occurred_at: occurred_at.to_owned(),
            actor: actor.to_owned(),
            correlation_id: correlation_id.to_owned(),
            state,
            entry_hash: String::new(),
        };
        record.entry_hash = audit_record_hash(&record)?;
        let serialized =
            serde_json::to_string(&record).map_err(|error| LiveError(error.to_string()))?;
        self.file
            .write_all(serialized.as_bytes())
            .and_then(|_| self.file.write_all(b"\n"))
            .and_then(|_| self.file.sync_data())
            .map_err(|error| LiveError(error.to_string()))?;
        self.next_sequence += 1;
        self.previous_hash = record.entry_hash;
        self.latest = Some(record.state);
        Ok(())
    }
}

fn audit_record_hash(record: &LiveJournalRecord) -> Result<String, LiveError> {
    let mut unsigned = record.clone();
    unsigned.entry_hash.clear();
    let canonical =
        serde_json::to_string(&unsigned).map_err(|error| LiveError(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

fn validate_audit_metadata(
    event_type: &str,
    occurred_at: &str,
    actor: &str,
    correlation_id: &str,
) -> Result<(), LiveError> {
    if !event_type.starts_with("live.") || event_type.len() > 128 {
        return Err(LiveError("live audit event_type is invalid".to_owned()));
    }
    validate_utc_timestamp("live audit occurred_at", occurred_at)?;
    validate_canonical_id("live audit actor", actor)?;
    validate_canonical_id("live audit correlation_id", correlation_id)?;
    Ok(())
}

/// Controlled-live service with shadow/canary separation and append-only audit evidence.
pub struct LiveTradingService<B> {
    account: LiveAccount,
    policy: LiveRiskPolicy,
    activation: LiveActivation,
    kill_switches: LiveKillSwitchRegistry,
    broker: B,
    broker_connected: bool,
    cash: Decimal,
    orders: BTreeMap<String, LiveOrder>,
    approvals: BTreeMap<String, RegisteredLiveApproval>,
    portfolios: BTreeMap<String, Portfolio>,
    execution_ids: BTreeSet<String>,
    incidents: BTreeMap<String, LiveIncident>,
    live_days: BTreeMap<String, PersistentLiveDay>,
    canary_submissions: u32,
    next_reconciliation: u64,
    last_reconciled_at: Option<String>,
    last_reconciliation_clean: Option<bool>,
    latest_reconciliation: Option<LiveReconciliationReport>,
    journal: LiveAuditJournal,
    audit_healthy: bool,
}

impl<B: LiveBrokerAdapter> LiveTradingService<B> {
    /// Opens a fail-closed controlled-live service from an integrity-checked audit journal.
    pub fn open_durable(
        account: LiveAccount,
        policy: LiveRiskPolicy,
        activation: LiveActivation,
        kill_switches: LiveKillSwitchRegistry,
        broker: B,
        journal_path: impl AsRef<Path>,
        opened_at: &str,
    ) -> Result<Self, LiveError> {
        account.validate()?;
        policy.validate()?;
        validate_utc_timestamp("live service opened_at", opened_at)?;
        let configuration_fingerprint =
            configuration_fingerprint(&account, &policy, &kill_switches);
        activation.validate(&configuration_fingerprint)?;
        let journal = LiveAuditJournal::open(journal_path)?;
        let latest = journal.latest().cloned();
        let mut service = Self {
            cash: account.initial_cash,
            account,
            policy,
            activation,
            kill_switches,
            broker,
            broker_connected: false,
            orders: BTreeMap::new(),
            approvals: BTreeMap::new(),
            portfolios: BTreeMap::new(),
            execution_ids: BTreeSet::new(),
            incidents: BTreeMap::new(),
            live_days: BTreeMap::new(),
            canary_submissions: 0,
            next_reconciliation: 1,
            last_reconciled_at: None,
            last_reconciliation_clean: None,
            latest_reconciliation: None,
            journal,
            audit_healthy: true,
        };
        if let Some(state) = latest {
            service.restore(state)?;
            // A broker session never survives process recovery. Persist this fact before any reconnect.
            service.persist(
                "live.service.restarted.v1",
                &service.activation.approved_by.clone(),
                opened_at,
                &service.activation.activation_id.clone(),
            )?;
        } else {
            service.persist(
                "live.service.initialized.v1",
                &service.activation.approved_by.clone(),
                opened_at,
                &service.activation.activation_id.clone(),
            )?;
        }
        Ok(service)
    }

    /// Returns the immutable LIVE configuration fingerprint bound to activation and approvals.
    pub fn configuration_fingerprint(&self) -> String {
        configuration_fingerprint(&self.account, &self.policy, &self.kill_switches)
    }

    /// Returns the mode chosen by the time-bounded activation.
    pub fn mode(&self) -> LiveRunMode {
        self.activation.mode
    }

    /// Returns mutable adapter access only for tightly scoped operational tests.
    pub fn broker_mut(&mut self) -> &mut B {
        &mut self.broker
    }

    /// Returns the independently controlled kill-switch registry.
    pub fn kill_switches(&self) -> &LiveKillSwitchRegistry {
        &self.kill_switches
    }

    /// Registers one unconsumed four-eyes approval before a canary submission.
    pub fn register_approval(
        &mut self,
        approval: LiveApproval,
        recorded_at: &str,
        actor: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        validate_utc_timestamp("live approval recorded_at", recorded_at)?;
        validate_canonical_id("live approval recording actor", actor)?;
        approval.validate(&self.configuration_fingerprint())?;
        if actor != approval.approved_by {
            return Err(LiveError(
                "live approval must be recorded by its distinct approving operator".to_owned(),
            ));
        }
        if approval.approved_at.as_str() > recorded_at
            || approval.expires_at.as_str() <= recorded_at
        {
            return Err(LiveError(
                "live approval is not active when registered".to_owned(),
            ));
        }
        match self.approvals.get(&approval.approval_id) {
            Some(existing) if existing.approval_equivalent(&approval) => return Ok(()),
            Some(_) => {
                return Err(LiveError(
                    "live approval ID was reused with different data".to_owned(),
                ))
            }
            None => {}
        }
        let approval_id = approval.approval_id.clone();
        self.approvals.insert(
            approval_id.clone(),
            RegisteredLiveApproval {
                approval,
                consumed: false,
            },
        );
        self.persist(
            "live.approval.registered.v1",
            actor,
            recorded_at,
            &approval_id,
        )
    }

    /// Activates a live kill switch without requiring broker connectivity.
    pub fn activate_kill_switch(
        &mut self,
        scope: LiveKillSwitchScope,
        actor: &str,
        occurred_at: &str,
    ) -> Result<bool, LiveError> {
        self.ensure_audit_healthy()?;
        validate_canonical_id("live kill actor", actor)?;
        validate_utc_timestamp("live kill time", occurred_at)?;
        let changed = self.kill_switches.activate(scope)?;
        self.persist(
            "live.kill_switch.activated.v1",
            actor,
            occurred_at,
            "kill-switch",
        )?;
        Ok(changed)
    }

    /// Deactivates a live kill switch explicitly and durably.
    pub fn deactivate_kill_switch(
        &mut self,
        scope: &LiveKillSwitchScope,
        actor: &str,
        occurred_at: &str,
    ) -> Result<bool, LiveError> {
        self.ensure_audit_healthy()?;
        validate_canonical_id("live kill actor", actor)?;
        validate_utc_timestamp("live kill time", occurred_at)?;
        let changed = self.kill_switches.deactivate(scope);
        if changed {
            self.persist(
                "live.kill_switch.deactivated.v1",
                actor,
                occurred_at,
                "kill-switch",
            )?;
        }
        Ok(changed)
    }

    /// Connects only a canary run through a managed secret provider.
    ///
    /// Secret bytes are never placed in an audit record, configuration, error, or dashboard.
    pub fn connect<S: SecretProvider>(
        &mut self,
        provider: &S,
        actor: &str,
        occurred_at: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(occurred_at)?;
        validate_canonical_id("live connection actor", actor)?;
        self.persist(
            "live.credential.requested.v1",
            actor,
            occurred_at,
            "credential",
        )?;
        let credential = match provider.resolve(&self.account.credential_reference) {
            Ok(credential) => credential,
            Err(error) => {
                self.persist(
                    "live.credential.unavailable.v1",
                    actor,
                    occurred_at,
                    "credential",
                )?;
                return Err(error.into());
            }
        };
        if let Err(error) = self.broker.connect(&self.account.account_id, &credential) {
            self.broker_connected = false;
            self.persist(
                "live.broker.connection_failed.v1",
                actor,
                occurred_at,
                "connection",
            )?;
            return Err(error);
        }
        self.broker_connected = true;
        self.persist("live.broker.connected.v1", actor, occurred_at, "connection")
    }

    /// Reconnects a canary session, then immediately synchronizes and reconciles.
    pub fn reconnect_and_reconcile<S: SecretProvider>(
        &mut self,
        provider: &S,
        actor: &str,
        reconciled_at: &str,
    ) -> Result<LiveReconciliationReport, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(reconciled_at)?;
        validate_canonical_id("live reconnect actor", actor)?;
        self.persist(
            "live.reconnect.requested.v1",
            actor,
            reconciled_at,
            "reconnect",
        )?;
        let credential = match provider.resolve(&self.account.credential_reference) {
            Ok(credential) => credential,
            Err(error) => {
                self.broker_connected = false;
                self.persist(
                    "live.credential.unavailable.v1",
                    actor,
                    reconciled_at,
                    "credential",
                )?;
                return Err(error.into());
            }
        };
        if let Err(error) = self.broker.reconnect(&self.account.account_id, &credential) {
            self.broker_connected = false;
            self.persist(
                "live.reconnect.failed.v1",
                actor,
                reconciled_at,
                "reconnect",
            )?;
            return Err(error);
        }
        self.broker_connected = true;
        self.persist(
            "live.reconnect.succeeded.v1",
            actor,
            reconciled_at,
            "reconnect",
        )?;
        self.synchronize(actor, reconciled_at)?;
        self.reconcile(actor, reconciled_at)
    }

    /// Records a production-data shadow decision. Shadow mode has no adapter submit path.
    pub fn record_shadow_intent(
        &mut self,
        intent: OrderIntent,
        market: LiveMarketData,
        decided_at: &str,
        actor: &str,
    ) -> Result<LiveSubmitOutcome, LiveError> {
        self.ensure_audit_healthy()?;
        if self.activation.mode != LiveRunMode::Shadow {
            return Err(LiveError(
                "shadow recording is disabled for a canary run".to_owned(),
            ));
        }
        if intent.environment != "SHADOW" {
            return Err(LiveError(
                "shadow service accepts only SHADOW intents".to_owned(),
            ));
        }
        if intent.account_id != self.account.account_id {
            return Err(LiveError(
                "shadow intent account does not match controlled-live account".to_owned(),
            ));
        }
        if !self.activation.active_at(decided_at)? {
            return Err(LiveError(
                "controlled-live shadow activation is absent or expired".to_owned(),
            ));
        }
        validate_canonical_id("shadow actor", actor)?;
        let decision = self.evaluate_risk(&intent, &market, decided_at, true)?;
        self.persist(
            "live.shadow.intent_recorded.v1",
            actor,
            decided_at,
            &intent.correlation_id,
        )?;
        Ok(LiveSubmitOutcome::ShadowRecorded { decision })
    }

    /// Performs a bounded live canary submission after exact per-order approval.
    pub fn submit_canary_intent(
        &mut self,
        intent: OrderIntent,
        market: LiveMarketData,
        approval_id: &str,
        decided_at: &str,
        actor: &str,
    ) -> Result<LiveSubmitOutcome, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(decided_at)?;
        validate_canonical_id("live submit actor", actor)?;
        validate_canonical_id("live approval_id", approval_id)?;
        if !self.broker_connected {
            return Err(LiveError(
                "live canary broker session is not connected".to_owned(),
            ));
        }
        if intent.environment != "LIVE" || intent.account_id != self.account.account_id {
            return Err(LiveError(
                "canary accepts only matching LIVE account intents".to_owned(),
            ));
        }
        intent.validate()?;
        let expected_fingerprint = intent_fingerprint(&intent)?;
        let order_id = format!("order-{}", intent.intent_id);
        if let Some(existing) = self.orders.get(&order_id) {
            if existing.oms.intent != intent
                || existing.approval_id != approval_id
                || existing.market != market
                || existing.decision.decided_at != decided_at
            {
                return Err(LiveError(
                    "live idempotency key was reused with different data".to_owned(),
                ));
            }
            return Ok(LiveSubmitOutcome::CanaryOrder {
                decision: existing.decision.clone(),
                order_id,
                state: existing.oms.state,
            });
        }
        let registered = self.approvals.get(approval_id).ok_or_else(|| {
            LiveError("live canary submission lacks a registered approval".to_owned())
        })?;
        if registered.consumed
            || registered.approval.intent_id != intent.intent_id
            || registered.approval.intent_fingerprint != expected_fingerprint
            || registered.approval.configuration_fingerprint != self.configuration_fingerprint()
            || registered.approval.approved_at.as_str() > decided_at
            || registered.approval.expires_at.as_str() <= decided_at
        {
            return Err(LiveError(
                "live approval is expired, consumed, or does not bind this exact intent".to_owned(),
            ));
        }
        let decision = self.evaluate_risk(&intent, &market, decided_at, false)?;
        if !decision.approved {
            self.persist(
                "live.risk.rejected.v1",
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
            evaluated_limits: format!(
                "max_order_quantity={},max_order_notional={},canary_max_order_notional={},canary_max_orders={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},mark_price={}",
                self.policy.max_order_quantity,
                self.policy.max_order_notional,
                self.policy.canary_max_order_notional,
                self.policy.canary_max_orders,
                self.policy.max_open_orders,
                self.policy.max_position_quantity,
                self.policy.max_realized_loss,
                self.policy.max_market_data_age_seconds,
                market.mark_price,
            ),
        };
        let mut oms = OmsOrder::from_approved_intent(intent, &core_decision)?;
        oms.transition(OrderState::Approved, "LIVE_RISK_APPROVED")?;
        oms.transition(
            OrderState::PendingSubmit,
            "LIVE_CANARY_SUBMISSION_REQUESTED",
        )?;
        let request = LiveBrokerOrderRequest::from_order(&oms);
        request.validate()?;
        let correlation_id = oms.intent.correlation_id.clone();
        self.orders.insert(
            oms.order_id.clone(),
            LiveOrder {
                oms,
                approval_id: approval_id.to_owned(),
                market,
                decision: decision.clone(),
                broker_order_id: None,
                filled_quantity: Decimal::ZERO,
            },
        );
        self.approvals
            .get_mut(approval_id)
            .ok_or_else(|| {
                LiveError("registered live approval disappeared before consumption".to_owned())
            })?
            .consumed = true;
        self.canary_submissions = self
            .canary_submissions
            .checked_add(1)
            .ok_or_else(|| LiveError("live canary submission counter overflowed".to_owned()))?;
        // The durable audit record precedes the irreversible external broker call.
        self.persist(
            "live.order.pending_submission.v1",
            actor,
            decided_at,
            &correlation_id,
        )?;
        match self.broker.submit(&request) {
            Ok(LiveBrokerSubmitResult::Acknowledged { broker_order_id }) => {
                validate_canonical_id("live broker_order_id", &broker_order_id)?;
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "LIVE_CANARY_SUBMISSION_SENT")?;
                order
                    .oms
                    .transition(OrderState::Acknowledged, "LIVE_BROKER_ACKNOWLEDGED")?;
                order.broker_order_id = Some(broker_order_id);
                let state = order.oms.state;
                self.persist(
                    "live.order.acknowledged.v1",
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
                validate_reason("live broker rejection reason", &reason)?;
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "LIVE_CANARY_SUBMISSION_SENT")?;
                order.oms.transition(OrderState::Rejected, reason)?;
                let state = order.oms.state;
                self.persist("live.order.rejected.v1", actor, decided_at, &correlation_id)?;
                Ok(LiveSubmitOutcome::CanaryOrder {
                    decision,
                    order_id: request.client_order_id,
                    state,
                })
            }
            Ok(LiveBrokerSubmitResult::Unknown { reason }) => {
                validate_reason("live broker unknown reason", &reason)?;
                self.order_mut(&request.client_order_id)?
                    .oms
                    .transition(OrderState::Unknown, reason)?;
                self.persist("live.order.unknown.v1", actor, decided_at, &correlation_id)?;
                Ok(LiveSubmitOutcome::CanaryOrder {
                    decision,
                    order_id: request.client_order_id,
                    state: OrderState::Unknown,
                })
            }
            Err(error) => {
                self.order_mut(&request.client_order_id)?
                    .oms
                    .transition(OrderState::Unknown, "LIVE_TRANSPORT_OUTCOME_UNKNOWN")?;
                self.broker_connected = false;
                self.persist(
                    "live.order.transport_unknown.v1",
                    actor,
                    decided_at,
                    &correlation_id,
                )?;
                Err(error)
            }
        }
    }

    /// Requests cancellation; a transport failure remains explicitly `UNKNOWN`.
    pub fn cancel_order(
        &mut self,
        order_id: &str,
        actor: &str,
        occurred_at: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(occurred_at)?;
        validate_canonical_id("live cancel actor", actor)?;
        let state = self.order_mut(order_id)?.oms.state;
        if !matches!(
            state,
            OrderState::Acknowledged | OrderState::PartiallyFilled
        ) {
            return Err(LiveError(
                "only acknowledged or partially filled live orders may cancel".to_owned(),
            ));
        }
        self.order_mut(order_id)?
            .oms
            .transition(OrderState::PendingCancel, "LIVE_CANCEL_REQUESTED")?;
        self.persist("live.order.pending_cancel.v1", actor, occurred_at, order_id)?;
        if let Err(error) = self.broker.cancel(order_id) {
            self.order_mut(order_id)?
                .oms
                .transition(OrderState::Unknown, "LIVE_CANCEL_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist("live.order.cancel_unknown.v1", actor, occurred_at, order_id)?;
            return Err(error);
        }
        self.persist("live.order.cancel_sent.v1", actor, occurred_at, order_id)
    }

    /// Drains broker evidence and applies each unique execution exactly once.
    pub fn synchronize(&mut self, actor: &str, occurred_at: &str) -> Result<usize, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(occurred_at)?;
        validate_canonical_id("live synchronization actor", actor)?;
        if !self.broker_connected {
            return Err(LiveError(
                "live canary broker session is not connected".to_owned(),
            ));
        }
        let events = match self.broker.poll() {
            Ok(events) => events,
            Err(error) => {
                self.broker_connected = false;
                self.persist(
                    "live.broker.poll_failed.v1",
                    actor,
                    occurred_at,
                    "broker-events",
                )?;
                return Err(error);
            }
        };
        let count = events.len();
        for event in events {
            self.apply_broker_event(event)?;
        }
        self.persist(
            "live.broker.events_synchronized.v1",
            actor,
            occurred_at,
            "broker-events",
        )?;
        Ok(count)
    }

    /// Compares independently tracked and broker state without overwriting either side.
    pub fn reconcile(
        &mut self,
        actor: &str,
        reconciled_at: &str,
    ) -> Result<LiveReconciliationReport, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(reconciled_at)?;
        validate_canonical_id("live reconciliation actor", actor)?;
        if !self.broker_connected {
            return Err(LiveError(
                "live canary broker session is not connected".to_owned(),
            ));
        }
        let snapshot = match self.broker.snapshot(&self.account.account_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.broker_connected = false;
                self.persist(
                    "live.broker.snapshot_failed.v1",
                    actor,
                    reconciled_at,
                    "reconciliation",
                )?;
                return Err(error);
            }
        };
        validate_broker_snapshot(&snapshot)?;
        let reconciliation_id = format!("reconciliation-{:08}", self.next_reconciliation);
        self.next_reconciliation += 1;
        let mut raw_issues = Vec::new();
        let broker_orders: BTreeMap<_, _> = snapshot
            .orders
            .iter()
            .map(|order| (order.client_order_id.as_str(), order))
            .collect();
        for (order_id, internal) in &self.orders {
            match broker_orders.get(order_id.as_str()) {
                None if internal.working() => raw_issues.push((
                    "MISSING_BROKER_ORDER",
                    order_id.clone(),
                    "internal working order is absent from broker snapshot".to_owned(),
                )),
                Some(broker) => {
                    if internal.broker_order_id.as_deref() != Some(broker.broker_order_id.as_str())
                    {
                        raw_issues.push((
                            "BROKER_ORDER_ID_MISMATCH",
                            order_id.clone(),
                            format!(
                                "internal={:?},broker={}",
                                internal.broker_order_id, broker.broker_order_id
                            ),
                        ));
                    }
                    if internal.filled_quantity != broker.filled_quantity {
                        raw_issues.push((
                            "FILLED_QUANTITY_MISMATCH",
                            order_id.clone(),
                            format!(
                                "internal={},broker={}",
                                internal.filled_quantity, broker.filled_quantity
                            ),
                        ));
                    }
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
        let instruments: BTreeSet<_> = self
            .portfolios
            .keys()
            .map(String::as_str)
            .chain(broker_positions.keys().copied())
            .collect();
        for instrument_id in instruments {
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
                let issue = LiveReconciliationIssue {
                    incident_id: incident_id.clone(),
                    category: category.to_owned(),
                    subject,
                    detail,
                };
                self.incidents
                    .entry(incident_id)
                    .or_insert_with(|| LiveIncident {
                        issue: issue.clone(),
                        explanation: None,
                    });
                issue
            })
            .collect();
        let report = LiveReconciliationReport {
            reconciliation_id,
            reconciled_at: reconciled_at.to_owned(),
            issues,
        };
        self.last_reconciled_at = Some(reconciled_at.to_owned());
        self.last_reconciliation_clean = Some(report.is_clean());
        self.latest_reconciliation = Some(report.clone());
        self.persist(
            "live.reconciliation.completed.v1",
            actor,
            reconciled_at,
            &report.reconciliation_id,
        )?;
        Ok(report)
    }

    /// Attaches an attributable explanation to an incident without changing broker or ledger state.
    pub fn explain_incident(
        &mut self,
        incident_id: &str,
        explanation: impl Into<String>,
        actor: &str,
        occurred_at: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        validate_canonical_id("live incident_id", incident_id)?;
        validate_canonical_id("live incident actor", actor)?;
        validate_utc_timestamp("live incident explanation time", occurred_at)?;
        let explanation = explanation.into();
        if explanation.trim().is_empty() || explanation.len() > 1_024 {
            return Err(LiveError(
                "live incident explanation must contain 1 to 1024 characters".to_owned(),
            ));
        }
        self.incidents
            .get_mut(incident_id)
            .ok_or_else(|| LiveError("unknown live incident".to_owned()))?
            .explanation = Some(explanation);
        self.persist(
            "live.incident.explained.v1",
            actor,
            occurred_at,
            incident_id,
        )
    }

    /// Records one closed, independently reconciled controlled-live session toward the 60-day gate.
    pub fn record_live_session(
        &mut self,
        session: &TradingSession,
        report: &LiveReconciliationReport,
        actor: &str,
        calendar: &dyn TradingCalendar,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(&report.reconciled_at)?;
        validate_canonical_id("live day actor", actor)?;
        session.validate()?;
        validate_exchange_date(&session.exchange_date)?;
        if calendar.calendar_id() != self.policy.trading_calendar_id
            || calendar.session_for_exchange_date(&session.exchange_date) != Some(session)
        {
            return Err(LiveError(
                "live-day gate requires the exact session from the configured calendar".to_owned(),
            ));
        }
        if self.activation.mode != LiveRunMode::Canary
            || !report.is_clean()
            || self.latest_reconciliation.as_ref() != Some(report)
            || self.last_reconciled_at.as_deref() != Some(report.reconciled_at.as_str())
            || self.last_reconciliation_clean != Some(report.is_clean())
            || report.reconciled_at < session.closes_at
            || report.reconciliation_id
                != format!(
                    "reconciliation-{:08}",
                    self.next_reconciliation.saturating_sub(1)
                )
        {
            return Err(LiveError(
                "live-day gate requires the latest clean canary reconciliation after session close"
                    .to_owned(),
            ));
        }
        let day = PersistentLiveDay {
            calendar_id: self.policy.trading_calendar_id.clone(),
            session_opens_at: session.opens_at.clone(),
            session_closes_at: session.closes_at.clone(),
            clean: report.is_clean() && self.unresolved_incident_count() == 0 && self.audit_healthy,
            audit_head_hash: self.journal.head_hash().to_owned(),
        };
        match self.live_days.get(&session.exchange_date) {
            Some(existing) if *existing == day => return Ok(()),
            Some(_) => {
                return Err(LiveError(
                    "live-day evidence cannot be overwritten".to_owned(),
                ))
            }
            None => {}
        }
        self.live_days.insert(session.exchange_date.clone(), day);
        self.persist(
            "live.gate.session_recorded.v1",
            actor,
            &report.reconciled_at,
            &report.reconciliation_id,
        )
    }

    /// Returns the exact measured controlled-live promotion state.
    pub fn promotion_status(&self) -> LivePromotionStatus {
        let clean_live_days = self.live_days.values().filter(|day| day.clean).count() as u32;
        let unresolved_incidents = self.unresolved_incident_count();
        let complete_auditability = self.audit_healthy && self.journal.sequence() > 0;
        LivePromotionStatus {
            clean_live_days,
            required_live_days: 60,
            unresolved_incidents,
            complete_auditability,
            eligible_for_next_gate: clean_live_days >= 60
                && unresolved_incidents == 0
                && complete_auditability,
        }
    }

    /// Creates a deterministic monitoring projection. It has no trading controls.
    pub fn monitoring_dashboard(&self) -> LiveMonitoringDashboard {
        let promotion = self.promotion_status();
        let positions = self
            .portfolios
            .iter()
            .map(|(instrument_id, portfolio)| {
                let position = portfolio.position_snapshot();
                LiveMonitoringPosition {
                    instrument_id: instrument_id.clone(),
                    quantity: position.quantity.to_string(),
                    average_cost: position.average_cost.to_string(),
                    realized_pnl: position.realized_pnl.to_string(),
                }
            })
            .collect();
        LiveMonitoringDashboard {
            dashboard_schema_version: 2,
            environment: self.account.environment.clone(),
            mode: self.activation.mode.as_str().to_owned(),
            account_id: self.account.account_id.clone(),
            configuration_fingerprint: self.configuration_fingerprint(),
            broker_connected: self.broker_connected,
            audit_healthy: self.audit_healthy,
            audit_sequence: self.journal.sequence(),
            audit_head_hash: self.journal.head_hash().to_owned(),
            active_kill_switches: self.kill_switches.active_keys(),
            working_orders: self.orders.values().filter(|order| order.working()).count() as u32,
            unknown_orders: self
                .orders
                .values()
                .filter(|order| order.oms.state == OrderState::Unknown)
                .count() as u32,
            unresolved_incidents: promotion.unresolved_incidents,
            last_reconciled_at: self.last_reconciled_at.clone(),
            last_reconciliation_clean: self.last_reconciliation_clean,
            clean_live_days: promotion.clean_live_days,
            required_live_days: promotion.required_live_days,
            promotion_eligible: promotion.eligible_for_next_gate,
            complete_auditability: promotion.complete_auditability,
            internal_cash: self.cash.to_string(),
            positions,
        }
    }

    /// Serializes the strict read-only monitoring contract deterministically.
    pub fn canonical_monitoring_json(&self) -> Result<String, LiveError> {
        serde_json::to_string(&self.monitoring_dashboard())
            .map_err(|error| LiveError(error.to_string()))
    }

    /// Returns the local audit location and integrity state for disaster-recovery monitoring.
    pub fn disaster_recovery_status(&self) -> DisasterRecoveryStatus {
        DisasterRecoveryStatus {
            journal_path: self.journal.path().display().to_string(),
            audit_sequence: self.journal.sequence(),
            audit_head_hash: self.journal.head_hash().to_owned(),
            audit_healthy: self.audit_healthy,
            broker_session_requires_reconnect: !self.broker_connected,
        }
    }

    fn require_canary_active(&self, occurred_at: &str) -> Result<(), LiveError> {
        if self.activation.mode != LiveRunMode::Canary || !self.activation.active_at(occurred_at)? {
            return Err(LiveError(
                "controlled-live canary activation is absent or expired".to_owned(),
            ));
        }
        Ok(())
    }

    fn order_mut(&mut self, order_id: &str) -> Result<&mut LiveOrder, LiveError> {
        self.orders
            .get_mut(order_id)
            .ok_or_else(|| LiveError("unknown live OMS order".to_owned()))
    }

    fn evaluate_risk(
        &self,
        intent: &OrderIntent,
        market: &LiveMarketData,
        decided_at: &str,
        shadow: bool,
    ) -> Result<LiveRiskDecision, LiveError> {
        intent.validate()?;
        validate_utc_timestamp("live risk decided_at", decided_at)?;
        market.validate()?;
        if market.instrument_id != intent.instrument_id {
            return Err(LiveError(
                "live market observation does not match intent instrument".to_owned(),
            ));
        }
        let observed_at = OffsetDateTime::parse(&market.observed_at, &Rfc3339)
            .map_err(|error| LiveError(error.to_string()))?;
        let decision_at = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| LiveError(error.to_string()))?;
        let age = (decision_at - observed_at).whole_seconds();
        if age < 0
            || u64::try_from(age).unwrap_or(u64::MAX) > self.policy.max_market_data_age_seconds
        {
            return Err(LiveError(
                "live market observation is stale or later than decision".to_owned(),
            ));
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
        let reserved_cash = self
            .orders
            .values()
            .try_fold(Decimal::ZERO, |total, order| {
                total
                    .checked_add(order.reserved_cash()?)
                    .map_err(LiveError::from)
            })?;
        let available_cash = self.cash.checked_sub(reserved_cash)?;
        let estimated_notional = intent.quantity.checked_mul(market.mark_price)?;
        let projected_position = match intent.side {
            Side::Buy => current_position.checked_add(intent.quantity)?,
            Side::Sell => current_position.checked_sub(intent.quantity)?,
        };
        let mut reasons = self.kill_switches.rejection_reasons(intent);
        if intent.quantity > self.policy.max_order_quantity {
            reasons.push("MAX_ORDER_QUANTITY_EXCEEDED".to_owned());
        }
        if estimated_notional > self.policy.max_order_notional {
            reasons.push("MAX_ORDER_NOTIONAL_EXCEEDED".to_owned());
        }
        if !shadow && estimated_notional > self.policy.canary_max_order_notional {
            reasons.push("CANARY_NOTIONAL_EXCEEDED".to_owned());
        }
        if !shadow && self.canary_submissions >= self.policy.canary_max_orders {
            reasons.push("CANARY_ORDER_COUNT_EXCEEDED".to_owned());
        }
        if !shadow
            && self
                .orders
                .values()
                .any(|order| order.oms.state == OrderState::Unknown)
        {
            reasons.push("UNKNOWN_ORDER_REQUIRES_RECONCILIATION".to_owned());
        }
        if !shadow && self.unresolved_incident_count() > 0 {
            reasons.push("UNRESOLVED_INCIDENTS_REQUIRE_REVIEW".to_owned());
        }
        if self.orders.values().filter(|order| order.working()).count()
            >= self.policy.max_open_orders
        {
            reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
        }
        if projected_position > self.policy.max_position_quantity
            || projected_position < Decimal::ZERO
        {
            reasons.push("POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned());
        }
        if intent.side == Side::Buy && estimated_notional > available_cash {
            reasons.push("INSUFFICIENT_INTERNAL_CASH".to_owned());
        }
        if intent.side == Side::Buy {
            let deployed_capital = if self.cash < self.account.initial_cash {
                self.account.initial_cash.checked_sub(self.cash)?
            } else {
                Decimal::ZERO
            };
            let projected_deployed_capital = deployed_capital
                .checked_add(reserved_cash)?
                .checked_add(estimated_notional)?;
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
        let approved = reasons.is_empty();
        if approved {
            reasons.push("APPROVED".to_owned());
        }
        Ok(LiveRiskDecision {
            decision_id: format!("live-risk-{}", intent.intent_id),
            approved,
            reason_codes: reasons,
            policy_version: self.policy.version.clone(),
            decided_at: decided_at.to_owned(),
            market_fingerprint: market_fingerprint(market),
        })
    }

    fn apply_broker_event(&mut self, event: LiveBrokerEvent) -> Result<(), LiveError> {
        match event {
            LiveBrokerEvent::Acknowledged {
                client_order_id,
                broker_order_id,
            } => {
                validate_canonical_id("live broker client_order_id", &client_order_id)?;
                validate_canonical_id("live broker_order_id", &broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if let Some(existing) = &order.broker_order_id {
                    if existing != &broker_order_id {
                        return Err(LiveError(
                            "broker reused a live client ID with different broker ID".to_owned(),
                        ));
                    }
                }
                order.broker_order_id = Some(broker_order_id);
                transition_to_acknowledged(order, "LIVE_BROKER_ACKNOWLEDGEMENT")?;
            }
            LiveBrokerEvent::Execution {
                execution_id,
                client_order_id,
                broker_order_id,
                quantity,
                price,
                fee,
                executed_at,
            } => {
                validate_canonical_id("live execution_id", &execution_id)?;
                validate_canonical_id("live execution client_order_id", &client_order_id)?;
                validate_canonical_id("live execution broker_order_id", &broker_order_id)?;
                validate_utc_timestamp("live execution time", &executed_at)?;
                if quantity <= Decimal::ZERO || price <= Decimal::ZERO || fee < Decimal::ZERO {
                    return Err(LiveError("live execution values are invalid".to_owned()));
                }
                if !self.execution_ids.insert(execution_id.clone()) {
                    return Ok(());
                }
                let (instrument_id, side, order_id) = {
                    let order = self.order_mut(&client_order_id)?;
                    if let Some(existing) = &order.broker_order_id {
                        if existing != &broker_order_id {
                            return Err(LiveError(
                                "live execution broker ID does not match order".to_owned(),
                            ));
                        }
                    } else {
                        order.broker_order_id = Some(broker_order_id);
                    }
                    transition_to_acknowledged(order, "LIVE_EXECUTION_CONFIRMED_ORDER")?;
                    let total = order.filled_quantity.checked_add(quantity)?;
                    if total > order.oms.intent.quantity {
                        return Err(LiveError(
                            "live execution exceeds requested quantity".to_owned(),
                        ));
                    }
                    order.filled_quantity = total;
                    if total == order.oms.intent.quantity {
                        if order.oms.state != OrderState::Filled {
                            order
                                .oms
                                .transition(OrderState::Filled, "LIVE_BROKER_FULL_FILL")?;
                        }
                    } else if order.oms.state == OrderState::Acknowledged {
                        order
                            .oms
                            .transition(OrderState::PartiallyFilled, "LIVE_BROKER_PARTIAL_FILL")?;
                    }
                    (
                        order.oms.intent.instrument_id.clone(),
                        order.oms.intent.side,
                        order.oms.order_id.clone(),
                    )
                };
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
                if self.cash < Decimal::ZERO {
                    self.record_internal_incident(
                        "LIVE_CASH_OVERDRAFT",
                        self.account.account_id.clone(),
                        "a broker execution exceeded independently available cash".to_owned(),
                    );
                }
            }
            LiveBrokerEvent::Cancelled {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("live cancellation client_order_id", &client_order_id)?;
                validate_reason("live cancellation reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                match order.oms.state {
                    OrderState::Acknowledged | OrderState::PartiallyFilled => {
                        order
                            .oms
                            .transition(OrderState::PendingCancel, "LIVE_BROKER_CANCEL_OBSERVED")?;
                        order.oms.transition(OrderState::Cancelled, reason)?;
                    }
                    OrderState::PendingCancel | OrderState::Unknown => {
                        order.oms.transition(OrderState::Cancelled, reason)?;
                    }
                    OrderState::Cancelled => {}
                    _ => {
                        return Err(LiveError(
                            "live cancellation is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            LiveBrokerEvent::Rejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("live rejection client_order_id", &client_order_id)?;
                validate_reason("live rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                match order.oms.state {
                    OrderState::PendingSubmit => {
                        order
                            .oms
                            .transition(OrderState::Submitted, "LIVE_BROKER_REJECTION_OBSERVED")?;
                        order.oms.transition(OrderState::Rejected, reason)?;
                    }
                    OrderState::Submitted | OrderState::Unknown => {
                        order.oms.transition(OrderState::Rejected, reason)?;
                    }
                    OrderState::Rejected => {}
                    _ => {
                        return Err(LiveError(
                            "live rejection is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
        }
        Ok(())
    }

    fn record_internal_incident(&mut self, category: &str, subject: String, detail: String) {
        let incident_id = format!("incident-internal-{:03}", self.incidents.len() + 1);
        self.incidents
            .entry(incident_id.clone())
            .or_insert_with(|| LiveIncident {
                issue: LiveReconciliationIssue {
                    incident_id,
                    category: category.to_owned(),
                    subject,
                    detail,
                },
                explanation: None,
            });
    }

    fn persist(
        &mut self,
        event_type: &str,
        actor: &str,
        occurred_at: &str,
        correlation_id: &str,
    ) -> Result<(), LiveError> {
        let state = self.persistent_state();
        if let Err(error) =
            self.journal
                .append(event_type, occurred_at, actor, correlation_id, state)
        {
            self.audit_healthy = false;
            self.broker_connected = false;
            return Err(error);
        }
        Ok(())
    }

    fn ensure_audit_healthy(&self) -> Result<(), LiveError> {
        if self.audit_healthy {
            Ok(())
        } else {
            Err(LiveError(
                "controlled-live service is halted after an audit write failure".to_owned(),
            ))
        }
    }

    fn unresolved_incident_count(&self) -> u32 {
        self.incidents
            .values()
            .filter(|incident| incident.unexplained())
            .count() as u32
    }

    fn persistent_state(&self) -> PersistentLiveState {
        PersistentLiveState {
            configuration_fingerprint: self.configuration_fingerprint(),
            account_id: self.account.account_id.clone(),
            currency: self.account.currency.clone(),
            cash: self.cash.to_string(),
            orders: self
                .orders
                .iter()
                .map(|(order_id, order)| {
                    (
                        order_id.clone(),
                        PersistentLiveOrder {
                            intent: PersistentIntent::from(&order.oms.intent),
                            approval_id: order.approval_id.clone(),
                            state: order.oms.state.as_str().to_owned(),
                            market: PersistentMarketData::from(&order.market),
                            decision: PersistentRiskDecision::from(&order.decision),
                            broker_order_id: order.broker_order_id.clone(),
                            filled_quantity: order.filled_quantity.to_string(),
                        },
                    )
                })
                .collect(),
            approvals: self
                .approvals
                .iter()
                .map(|(approval_id, registered)| {
                    (
                        approval_id.clone(),
                        PersistentLiveApproval {
                            approval: PersistentApproval::from(&registered.approval),
                            consumed: registered.consumed,
                        },
                    )
                })
                .collect(),
            positions: self
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
                .collect(),
            execution_ids: self.execution_ids.iter().cloned().collect(),
            active_kill_switches: self.kill_switches.active_keys(),
            incidents: self
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
                .collect(),
            live_days: self.live_days.clone(),
            canary_submissions: self.canary_submissions,
            next_reconciliation: self.next_reconciliation,
            last_reconciled_at: self.last_reconciled_at.clone(),
            last_reconciliation_clean: self.last_reconciliation_clean,
            latest_reconciliation: self
                .latest_reconciliation
                .as_ref()
                .map(PersistentReconciliationReport::from),
        }
    }

    fn restore(&mut self, state: PersistentLiveState) -> Result<(), LiveError> {
        if state.configuration_fingerprint != self.configuration_fingerprint()
            || state.account_id != self.account.account_id
            || state.currency != self.account.currency
        {
            return Err(LiveError(
                "live audit journal configuration does not match supplied activation".to_owned(),
            ));
        }
        self.cash = decimal("persisted live cash", &state.cash)?;
        let mut approvals = BTreeMap::new();
        for (approval_id, persisted) in state.approvals {
            validate_canonical_id("persisted live approval_id", &approval_id)?;
            let approval = LiveApproval::try_from(persisted.approval)?;
            approval.validate(&self.configuration_fingerprint())?;
            if approval.approval_id != approval_id {
                return Err(LiveError(
                    "persisted live approval key does not match data".to_owned(),
                ));
            }
            approvals.insert(
                approval_id,
                RegisteredLiveApproval {
                    approval,
                    consumed: persisted.consumed,
                },
            );
        }
        let mut orders = BTreeMap::new();
        for (order_id, persisted) in state.orders {
            let intent = OrderIntent::try_from(persisted.intent)?;
            if intent.environment != "LIVE" || intent.account_id != self.account.account_id {
                return Err(LiveError(
                    "persisted live order has incompatible environment or account".to_owned(),
                ));
            }
            validate_canonical_id("persisted order approval_id", &persisted.approval_id)?;
            let approval = approvals.get(&persisted.approval_id).ok_or_else(|| {
                LiveError("persisted live order is missing its approval".to_owned())
            })?;
            if !approval.consumed || approval.approval.intent_id != intent.intent_id {
                return Err(LiveError(
                    "persisted live order approval is invalid".to_owned(),
                ));
            }
            let market = LiveMarketData::try_from(persisted.market)?;
            if market.instrument_id != intent.instrument_id {
                return Err(LiveError(
                    "persisted live market does not match order instrument".to_owned(),
                ));
            }
            let decision = LiveRiskDecision::try_from(persisted.decision)?;
            if !decision.approved
                || decision.decision_id != format!("live-risk-{}", intent.intent_id)
                || decision.policy_version != self.policy.version
                || decision.market_fingerprint != market_fingerprint(&market)
            {
                return Err(LiveError(
                    "persisted live risk decision does not bind the order and policy".to_owned(),
                ));
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                validate_canonical_id("persisted live broker_order_id", broker_order_id)?;
            }
            let filled_quantity =
                decimal("persisted live filled quantity", &persisted.filled_quantity)?;
            if filled_quantity < Decimal::ZERO || filled_quantity > intent.quantity {
                return Err(LiveError(
                    "persisted live filled quantity is invalid".to_owned(),
                ));
            }
            let oms = OmsOrder::recover(
                order_id.clone(),
                intent,
                parse_order_state(&persisted.state)?,
            )?;
            orders.insert(
                order_id,
                LiveOrder {
                    oms,
                    approval_id: persisted.approval_id,
                    market,
                    decision,
                    broker_order_id: persisted.broker_order_id,
                    filled_quantity,
                },
            );
        }
        let mut portfolios = BTreeMap::new();
        for (instrument_id, position) in state.positions {
            portfolios.insert(
                instrument_id.clone(),
                Portfolio::recover(
                    &self.account.account_id,
                    instrument_id,
                    decimal("persisted live position quantity", &position.quantity)?,
                    decimal("persisted live average cost", &position.average_cost)?,
                    decimal("persisted live realized pnl", &position.realized_pnl)?,
                )?,
            );
        }
        let mut execution_ids = BTreeSet::new();
        for execution_id in state.execution_ids {
            validate_canonical_id("persisted live execution_id", &execution_id)?;
            if !execution_ids.insert(execution_id) {
                return Err(LiveError(
                    "persisted live execution ID is duplicated".to_owned(),
                ));
            }
        }
        let mut switches = LiveKillSwitchRegistry::new(self.kill_switches.version.clone())?;
        for scope in state.active_kill_switches {
            switches.activate(parse_kill_switch_scope(&scope)?)?;
        }
        let mut incidents = BTreeMap::new();
        for (incident_id, incident) in state.incidents {
            validate_canonical_id("persisted live incident_id", &incident_id)?;
            if incident.category.is_empty()
                || incident.subject.is_empty()
                || incident.detail.is_empty()
            {
                return Err(LiveError("persisted live incident is invalid".to_owned()));
            }
            incidents.insert(
                incident_id.clone(),
                LiveIncident {
                    issue: LiveReconciliationIssue {
                        incident_id,
                        category: incident.category,
                        subject: incident.subject,
                        detail: incident.detail,
                    },
                    explanation: incident.explanation,
                },
            );
        }
        for (date, day) in &state.live_days {
            validate_exchange_date(date)?;
            validate_canonical_id("persisted live calendar_id", &day.calendar_id)?;
            if day.calendar_id != self.policy.trading_calendar_id || day.audit_head_hash.len() != 64
            {
                return Err(LiveError(
                    "persisted live-day evidence is invalid".to_owned(),
                ));
            }
            TradingSession {
                exchange_date: date.clone(),
                opens_at: day.session_opens_at.clone(),
                closes_at: day.session_closes_at.clone(),
            }
            .validate()?;
        }
        if state.canary_submissions > self.policy.canary_max_orders
            || state.next_reconciliation == 0
        {
            return Err(LiveError("persisted live counters are invalid".to_owned()));
        }
        match (&state.last_reconciled_at, state.last_reconciliation_clean) {
            (Some(timestamp), Some(_)) => {
                validate_utc_timestamp("persisted live reconciliation time", timestamp)?
            }
            (None, None) => {}
            _ => {
                return Err(LiveError(
                    "persisted live reconciliation marker is invalid".to_owned(),
                ))
            }
        }
        let latest_reconciliation = state
            .latest_reconciliation
            .map(LiveReconciliationReport::try_from)
            .transpose()?;
        match (
            &latest_reconciliation,
            &state.last_reconciled_at,
            state.last_reconciliation_clean,
        ) {
            (Some(report), Some(reconciled_at), Some(clean))
                if report.reconciled_at == *reconciled_at && report.is_clean() == clean => {}
            (None, None, None) => {}
            _ => {
                return Err(LiveError(
                    "persisted latest reconciliation evidence is inconsistent".to_owned(),
                ))
            }
        }
        if let Some(report) = &latest_reconciliation {
            let expected = format!(
                "reconciliation-{:08}",
                state.next_reconciliation.saturating_sub(1)
            );
            if report.reconciliation_id != expected {
                return Err(LiveError(
                    "persisted latest reconciliation identity is invalid".to_owned(),
                ));
            }
        }
        self.orders = orders;
        self.approvals = approvals;
        self.portfolios = portfolios;
        self.execution_ids = execution_ids;
        self.kill_switches = switches;
        self.incidents = incidents;
        self.live_days = state.live_days;
        self.canary_submissions = state.canary_submissions;
        self.next_reconciliation = state.next_reconciliation;
        self.last_reconciled_at = state.last_reconciled_at;
        self.last_reconciliation_clean = state.last_reconciliation_clean;
        self.latest_reconciliation = latest_reconciliation;
        // Restart recovery never assumes a still-valid external session.
        self.broker_connected = false;
        Ok(())
    }
}

/// Read-only recovery/backup state emitted for monitoring and runbooks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisasterRecoveryStatus {
    /// Local append-only journal path.
    pub journal_path: String,
    /// Last durable audit sequence.
    pub audit_sequence: u64,
    /// SHA-256 audit-chain head.
    pub audit_head_hash: String,
    /// Whether the running service has had any audit-write failure.
    pub audit_healthy: bool,
    /// Whether an adapter session must be re-established before canary work.
    pub broker_session_requires_reconnect: bool,
}

impl RegisteredLiveApproval {
    fn approval_equivalent(&self, approval: &LiveApproval) -> bool {
        self.approval.approval_id == approval.approval_id
            && self.approval.intent_id == approval.intent_id
            && self.approval.intent_fingerprint == approval.intent_fingerprint
            && self.approval.configuration_fingerprint == approval.configuration_fingerprint
            && self.approval.requested_by == approval.requested_by
            && self.approval.approved_by == approval.approved_by
            && self.approval.approved_at == approval.approved_at
            && self.approval.expires_at == approval.expires_at
    }
}

fn configuration_fingerprint(
    account: &LiveAccount,
    policy: &LiveRiskPolicy,
    kill_switches: &LiveKillSwitchRegistry,
) -> String {
    let initial_cash = account.initial_cash.to_string();
    let max_deployed_capital = account.max_deployed_capital.to_string();
    let max_order_quantity = policy.max_order_quantity.to_string();
    let max_order_notional = policy.max_order_notional.to_string();
    let canary_max_order_notional = policy.canary_max_order_notional.to_string();
    let canary_max_orders = policy.canary_max_orders.to_string();
    let max_open_orders = policy.max_open_orders.to_string();
    let max_position_quantity = policy.max_position_quantity.to_string();
    let max_realized_loss = policy.max_realized_loss.to_string();
    let max_market_data_age_seconds = policy.max_market_data_age_seconds.to_string();
    hash_fingerprint_parts(&[
        "live-configuration-v1",
        &account.account_id,
        &account.currency,
        &initial_cash,
        &max_deployed_capital,
        &account.environment,
        account.credential_reference.as_str(),
        &policy.version,
        &policy.trading_calendar_id,
        &max_order_quantity,
        &max_order_notional,
        &canary_max_order_notional,
        &canary_max_orders,
        &max_open_orders,
        &max_position_quantity,
        &max_realized_loss,
        &max_market_data_age_seconds,
        &kill_switches.version,
    ])
}

fn intent_fingerprint(intent: &OrderIntent) -> Result<String, LiveError> {
    intent.validate()?;
    let quantity = intent.quantity.to_string();
    let limit_price = intent
        .limit_price
        .map(|price| price.to_string())
        .unwrap_or_default();
    Ok(hash_fingerprint_parts(&[
        "live-intent-v1",
        &intent.intent_id,
        &intent.account_id,
        &intent.strategy_id,
        &intent.instrument_id,
        &intent.correlation_id,
        intent.side.as_str(),
        &quantity,
        intent.order_type.as_str(),
        &limit_price,
        intent.time_in_force.as_str(),
        &intent.rationale,
        &intent.created_at,
        &intent.strategy_version,
        &intent.configuration_version,
        &intent.environment,
    ]))
}

fn market_fingerprint(market: &LiveMarketData) -> String {
    let mark_price = market.mark_price.to_string();
    hash_fingerprint_parts(&[
        "live-market-v1",
        &market.instrument_id,
        &mark_price,
        &market.observed_at,
    ])
}

/// Hashes ordered fields with explicit byte lengths, avoiding delimiter ambiguity.
fn hash_fingerprint_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn transition_to_acknowledged(order: &mut LiveOrder, reason: &str) -> Result<(), LiveError> {
    match order.oms.state {
        OrderState::PendingSubmit => {
            order.oms.transition(OrderState::Submitted, reason)?;
            order.oms.transition(OrderState::Acknowledged, reason)?;
        }
        OrderState::Submitted | OrderState::Unknown => {
            order.oms.transition(OrderState::Acknowledged, reason)?;
        }
        OrderState::Acknowledged | OrderState::PartiallyFilled | OrderState::Filled => {}
        _ => {
            return Err(LiveError(
                "broker acknowledgement is incompatible with live OMS state".to_owned(),
            ))
        }
    }
    Ok(())
}

fn validate_reason(name: &str, value: &str) -> Result<(), LiveError> {
    if value.trim().is_empty() || value.len() > 1_024 {
        return Err(LiveError(format!(
            "{name} must contain 1 to 1024 characters"
        )));
    }
    Ok(())
}

fn validate_broker_snapshot(snapshot: &LiveBrokerAccountSnapshot) -> Result<(), LiveError> {
    let mut client_order_ids = BTreeSet::new();
    let mut broker_order_ids = BTreeSet::new();
    for order in &snapshot.orders {
        validate_canonical_id("live snapshot client_order_id", &order.client_order_id)?;
        validate_canonical_id("live snapshot broker_order_id", &order.broker_order_id)?;
        if order.filled_quantity < Decimal::ZERO
            || !client_order_ids.insert(order.client_order_id.as_str())
            || !broker_order_ids.insert(order.broker_order_id.as_str())
        {
            return Err(LiveError(
                "live snapshot has duplicate order IDs or negative fills".to_owned(),
            ));
        }
    }
    let mut instrument_ids = BTreeSet::new();
    for position in &snapshot.positions {
        validate_canonical_id("live snapshot instrument_id", &position.instrument_id)?;
        if !instrument_ids.insert(position.instrument_id.as_str()) {
            return Err(LiveError(
                "live snapshot has duplicate instrument position".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_exchange_date(value: &str) -> Result<(), LiveError> {
    if value.len() != 10
        || !value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7) && byte == b'-'
                || !matches!(index, 4 | 7) && byte.is_ascii_digit()
        })
    {
        return Err(LiveError("exchange date must be YYYY-MM-DD".to_owned()));
    }
    validate_utc_timestamp("live exchange date", &format!("{value}T00:00:00Z"))?;
    Ok(())
}

fn decimal(name: &str, value: &str) -> Result<Decimal, LiveError> {
    Decimal::from_str(value).map_err(|error| LiveError(format!("invalid {name}: {error}")))
}

fn parse_order_state(value: &str) -> Result<OrderState, LiveError> {
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
        "CANCELLED" => Ok(OrderState::Cancelled),
        "REJECTED" => Ok(OrderState::Rejected),
        "EXPIRED" => Ok(OrderState::Expired),
        "UNKNOWN" => Ok(OrderState::Unknown),
        _ => Err(LiveError("persisted live OMS state is invalid".to_owned())),
    }
}

fn parse_kill_switch_scope(value: &str) -> Result<LiveKillSwitchScope, LiveError> {
    if value == "global" {
        return Ok(LiveKillSwitchScope::Global);
    }
    for (prefix, constructor) in [
        (
            "account:",
            LiveKillSwitchScope::Account as fn(String) -> LiveKillSwitchScope,
        ),
        (
            "strategy:",
            LiveKillSwitchScope::Strategy as fn(String) -> LiveKillSwitchScope,
        ),
        (
            "instrument:",
            LiveKillSwitchScope::Instrument as fn(String) -> LiveKillSwitchScope,
        ),
    ] {
        if let Some(identifier) = value.strip_prefix(prefix) {
            let scope = constructor(identifier.to_owned());
            scope.validate()?;
            return Ok(scope);
        }
    }
    Err(LiveError(
        "persisted live kill-switch scope is invalid".to_owned(),
    ))
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
            limit_price: intent.limit_price.map(|value| value.to_string()),
            time_in_force: intent.time_in_force.as_str().to_owned(),
            rationale: intent.rationale.clone(),
            created_at: intent.created_at.clone(),
            strategy_version: intent.strategy_version.clone(),
            configuration_version: intent.configuration_version.clone(),
            environment: intent.environment.clone(),
        }
    }
}

impl TryFrom<PersistentIntent> for OrderIntent {
    type Error = LiveError;

    fn try_from(value: PersistentIntent) -> Result<Self, Self::Error> {
        let intent = Self {
            intent_id: value.intent_id,
            account_id: value.account_id,
            strategy_id: value.strategy_id,
            instrument_id: value.instrument_id,
            correlation_id: value.correlation_id,
            side: match value.side.as_str() {
                "BUY" => Side::Buy,
                "SELL" => Side::Sell,
                _ => {
                    return Err(LiveError(
                        "persisted live intent side is invalid".to_owned(),
                    ))
                }
            },
            quantity: decimal("persisted live intent quantity", &value.quantity)?,
            order_type: match value.order_type.as_str() {
                "MARKET" => follon_domain::OrderType::Market,
                "LIMIT" => follon_domain::OrderType::Limit,
                _ => return Err(LiveError("persisted live order type is invalid".to_owned())),
            },
            limit_price: value
                .limit_price
                .as_deref()
                .map(|price| decimal("persisted live limit price", price))
                .transpose()?,
            time_in_force: match value.time_in_force.as_str() {
                "DAY" => follon_domain::TimeInForce::Day,
                "GTC" => follon_domain::TimeInForce::GoodTilCancelled,
                _ => {
                    return Err(LiveError(
                        "persisted live time in force is invalid".to_owned(),
                    ))
                }
            },
            rationale: value.rationale,
            created_at: value.created_at,
            strategy_version: value.strategy_version,
            configuration_version: value.configuration_version,
            environment: value.environment,
        };
        intent.validate()?;
        Ok(intent)
    }
}

impl From<&LiveMarketData> for PersistentMarketData {
    fn from(market: &LiveMarketData) -> Self {
        Self {
            instrument_id: market.instrument_id.clone(),
            mark_price: market.mark_price.to_string(),
            observed_at: market.observed_at.clone(),
        }
    }
}

impl TryFrom<PersistentMarketData> for LiveMarketData {
    type Error = LiveError;

    fn try_from(value: PersistentMarketData) -> Result<Self, Self::Error> {
        let market = Self {
            instrument_id: value.instrument_id,
            mark_price: decimal("persisted live market mark", &value.mark_price)?,
            observed_at: value.observed_at,
        };
        market.validate()?;
        Ok(market)
    }
}

impl From<&LiveRiskDecision> for PersistentRiskDecision {
    fn from(decision: &LiveRiskDecision) -> Self {
        Self {
            decision_id: decision.decision_id.clone(),
            approved: decision.approved,
            reason_codes: decision.reason_codes.clone(),
            policy_version: decision.policy_version.clone(),
            decided_at: decision.decided_at.clone(),
            market_fingerprint: decision.market_fingerprint.clone(),
        }
    }
}

impl TryFrom<PersistentRiskDecision> for LiveRiskDecision {
    type Error = LiveError;

    fn try_from(value: PersistentRiskDecision) -> Result<Self, Self::Error> {
        validate_canonical_id("persisted live risk decision_id", &value.decision_id)?;
        validate_utc_timestamp("persisted live risk decision time", &value.decided_at)?;
        if value.reason_codes.is_empty()
            || value.policy_version.is_empty()
            || value.market_fingerprint.len() != 64
            || !value
                .market_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(LiveError(
                "persisted live risk decision is invalid".to_owned(),
            ));
        }
        for reason in &value.reason_codes {
            validate_reason("persisted live risk reason", reason)?;
        }
        if value.approved != (value.reason_codes.len() == 1 && value.reason_codes[0] == "APPROVED")
        {
            return Err(LiveError(
                "persisted live risk approval outcome is inconsistent".to_owned(),
            ));
        }
        Ok(Self {
            decision_id: value.decision_id,
            approved: value.approved,
            reason_codes: value.reason_codes,
            policy_version: value.policy_version,
            decided_at: value.decided_at,
            market_fingerprint: value.market_fingerprint,
        })
    }
}

impl From<&LiveReconciliationReport> for PersistentReconciliationReport {
    fn from(report: &LiveReconciliationReport) -> Self {
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

impl TryFrom<PersistentReconciliationReport> for LiveReconciliationReport {
    type Error = LiveError;

    fn try_from(value: PersistentReconciliationReport) -> Result<Self, Self::Error> {
        validate_canonical_id("persisted live reconciliation_id", &value.reconciliation_id)?;
        validate_utc_timestamp("persisted live reconciliation time", &value.reconciled_at)?;
        let mut incident_ids = BTreeSet::new();
        let mut issues = Vec::with_capacity(value.issues.len());
        for issue in value.issues {
            validate_canonical_id(
                "persisted live reconciliation incident_id",
                &issue.incident_id,
            )?;
            validate_reason("persisted live reconciliation category", &issue.category)?;
            validate_reason("persisted live reconciliation subject", &issue.subject)?;
            validate_reason("persisted live reconciliation detail", &issue.detail)?;
            if !incident_ids.insert(issue.incident_id.clone()) {
                return Err(LiveError(
                    "persisted live reconciliation repeats an incident ID".to_owned(),
                ));
            }
            issues.push(LiveReconciliationIssue {
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

impl From<&LiveApproval> for PersistentApproval {
    fn from(approval: &LiveApproval) -> Self {
        Self {
            approval_id: approval.approval_id.clone(),
            intent_id: approval.intent_id.clone(),
            intent_fingerprint: approval.intent_fingerprint.clone(),
            configuration_fingerprint: approval.configuration_fingerprint.clone(),
            requested_by: approval.requested_by.clone(),
            approved_by: approval.approved_by.clone(),
            approved_at: approval.approved_at.clone(),
            expires_at: approval.expires_at.clone(),
        }
    }
}

impl TryFrom<PersistentApproval> for LiveApproval {
    type Error = LiveError;

    fn try_from(value: PersistentApproval) -> Result<Self, Self::Error> {
        Ok(Self {
            approval_id: value.approval_id,
            intent_id: value.intent_id,
            intent_fingerprint: value.intent_fingerprint,
            configuration_fingerprint: value.configuration_fingerprint,
            requested_by: value.requested_by,
            approved_by: value.approved_by,
            approved_at: value.approved_at,
            expires_at: value.expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::mem;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use follon_domain::{OrderType, TimeInForce};
    use follon_instrument::StaticTradingCalendar;

    use super::*;

    static JOURNAL_SEQUENCE: AtomicUsize = AtomicUsize::new(1);

    #[derive(Debug)]
    struct TestBroker {
        connected: bool,
        submitted: u32,
        events: Vec<LiveBrokerEvent>,
        snapshot: LiveBrokerAccountSnapshot,
    }

    impl TestBroker {
        fn new() -> Self {
            Self {
                connected: false,
                submitted: 0,
                events: Vec::new(),
                snapshot: LiveBrokerAccountSnapshot {
                    orders: Vec::new(),
                    positions: Vec::new(),
                    cash: amount("1000"),
                },
            }
        }
    }

    impl LiveBrokerAdapter for TestBroker {
        fn connect(
            &mut self,
            account_id: &str,
            credential: &SecretMaterial,
        ) -> Result<(), LiveError> {
            assert_eq!(account_id, "acct.live.001");
            assert_eq!(credential.expose_to(|bytes| bytes.len()), 4);
            self.connected = true;
            Ok(())
        }

        fn submit(
            &mut self,
            request: &LiveBrokerOrderRequest,
        ) -> Result<LiveBrokerSubmitResult, LiveError> {
            assert!(self.connected);
            self.submitted += 1;
            let broker_order_id = format!("broker-{}", request.client_order_id);
            self.snapshot.orders.push(LiveBrokerOrderSnapshot {
                client_order_id: request.client_order_id.clone(),
                broker_order_id: broker_order_id.clone(),
                state: OrderState::Acknowledged,
                filled_quantity: Decimal::ZERO,
            });
            Ok(LiveBrokerSubmitResult::Acknowledged { broker_order_id })
        }

        fn cancel(&mut self, _client_order_id: &str) -> Result<(), LiveError> {
            Ok(())
        }

        fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
            Ok(mem::take(&mut self.events))
        }

        fn snapshot(&mut self, account_id: &str) -> Result<LiveBrokerAccountSnapshot, LiveError> {
            assert_eq!(account_id, "acct.live.001");
            Ok(self.snapshot.clone())
        }

        fn reconnect(
            &mut self,
            account_id: &str,
            credential: &SecretMaterial,
        ) -> Result<(), LiveError> {
            self.connect(account_id, credential)
        }
    }

    struct TestSecrets;

    impl SecretProvider for TestSecrets {
        fn resolve(
            &self,
            reference: &SecretReference,
        ) -> Result<SecretMaterial, follon_secrets::SecretError> {
            assert_eq!(reference.as_str(), "secret.broker.test.acct-live-001");
            SecretMaterial::new(b"test".to_vec())
        }
    }

    fn amount(value: &str) -> Decimal {
        Decimal::from_str(value).expect("test decimal")
    }

    fn journal_path(label: &str) -> PathBuf {
        let sequence = JOURNAL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "follon-live-{label}-{}-{sequence}.ndjson",
            std::process::id()
        ))
    }

    fn account() -> LiveAccount {
        LiveAccount {
            account_id: "acct.live.001".to_owned(),
            currency: "USD".to_owned(),
            initial_cash: amount("1000"),
            max_deployed_capital: amount("100"),
            environment: "LIVE".to_owned(),
            credential_reference: SecretReference::new("secret.broker.test.acct-live-001")
                .expect("test secret reference"),
        }
    }

    fn policy() -> LiveRiskPolicy {
        LiveRiskPolicy {
            version: "live-risk-v1".to_owned(),
            trading_calendar_id: "calendar.nyse.v1".to_owned(),
            max_order_quantity: amount("10"),
            max_order_notional: amount("100"),
            canary_max_order_notional: amount("50"),
            canary_max_orders: 2,
            max_open_orders: 2,
            max_position_quantity: amount("10"),
            max_realized_loss: amount("100"),
            max_market_data_age_seconds: 60,
        }
    }

    fn activation(
        mode: LiveRunMode,
        account: &LiveAccount,
        policy: &LiveRiskPolicy,
        switches: &LiveKillSwitchRegistry,
    ) -> LiveActivation {
        LiveActivation {
            activation_id: "activation.live.001".to_owned(),
            mode,
            configuration_fingerprint: configuration_fingerprint(account, policy, switches),
            requested_by: "operator.requester.001".to_owned(),
            approved_by: "operator.approver.001".to_owned(),
            activated_at: "2026-01-02T14:00:00Z".to_owned(),
            expires_at: "2026-12-31T23:59:59Z".to_owned(),
        }
    }

    fn test_service(mode: LiveRunMode, path: &Path) -> LiveTradingService<TestBroker> {
        let account = account();
        let policy = policy();
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(mode, &account, &policy, &switches);
        LiveTradingService::open_durable(
            account,
            policy,
            activation,
            switches,
            TestBroker::new(),
            path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service")
    }

    fn intent(environment: &str, intent_id: &str) -> OrderIntent {
        OrderIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.live.001".to_owned(),
            strategy_id: "strategy.live.001".to_owned(),
            instrument_id: "inst.us_equity.spy".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            side: Side::Buy,
            quantity: amount("2"),
            order_type: OrderType::Market,
            limit_price: None,
            time_in_force: TimeInForce::Day,
            rationale: "controlled-live test intent".to_owned(),
            created_at: "2026-01-02T14:30:00Z".to_owned(),
            strategy_version: "strategy-live-v1".to_owned(),
            configuration_version: "config-live-v1".to_owned(),
            environment: environment.to_owned(),
        }
    }

    fn market() -> LiveMarketData {
        LiveMarketData {
            instrument_id: "inst.us_equity.spy".to_owned(),
            mark_price: amount("10"),
            observed_at: "2026-01-02T14:30:00Z".to_owned(),
        }
    }

    fn live_session(exchange_date: &str) -> TradingSession {
        let (opens_at, closes_at) = if exchange_date >= "2026-03-09" {
            ("13:30:00Z", "20:00:00Z")
        } else {
            ("14:30:00Z", "21:00:00Z")
        };
        TradingSession {
            exchange_date: exchange_date.to_owned(),
            opens_at: format!("{exchange_date}T{opens_at}"),
            closes_at: format!("{exchange_date}T{closes_at}"),
        }
    }

    fn live_calendar(sessions: &[TradingSession]) -> StaticTradingCalendar {
        StaticTradingCalendar::new("calendar.nyse.v1", sessions.to_vec())
            .expect("test live calendar")
    }

    fn approval_for(
        service: &LiveTradingService<TestBroker>,
        intent: &OrderIntent,
    ) -> LiveApproval {
        LiveApproval {
            approval_id: "approval.live.001".to_owned(),
            intent_id: intent.intent_id.clone(),
            intent_fingerprint: intent_fingerprint(intent).expect("test intent fingerprint"),
            configuration_fingerprint: service.configuration_fingerprint(),
            requested_by: "operator.requester.001".to_owned(),
            approved_by: "operator.approver.001".to_owned(),
            approved_at: "2026-01-02T14:30:00Z".to_owned(),
            expires_at: "2026-01-02T15:00:00Z".to_owned(),
        }
    }

    #[test]
    fn canary_is_four_eyes_durable_and_reconciles_before_a_live_day_counts() {
        let path = journal_path("canary");
        let mut service = test_service(LiveRunMode::Canary, &path);
        let order_intent = intent("LIVE", "intent.live.001");
        let approval = approval_for(&service, &order_intent);
        assert!(service
            .register_approval(
                approval.clone(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001"
            )
            .is_err());
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent.clone(),
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let repeated = service
            .submit_canary_intent(
                order_intent,
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("idempotent repeat");
        assert!(matches!(repeated, LiveSubmitOutcome::CanaryOrder { .. }));
        assert_eq!(service.broker_mut().submitted, 1);

        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.001".to_owned(),
            client_order_id: "order-intent.live.001".to_owned(),
            broker_order_id: "broker-order-intent.live.001".to_owned(),
            quantity: amount("2"),
            price: amount("10"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("2");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("2"),
        });
        broker.snapshot.cash = amount("980");
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");
        let report = service
            .reconcile("operator.approver.001", "2026-01-02T21:01:00Z")
            .expect("independent reconciliation");
        assert!(report.is_clean());
        let session = live_session("2026-01-02");
        let calendar = live_calendar(std::slice::from_ref(&session));
        service
            .record_live_session(&session, &report, "operator.approver.001", &calendar)
            .expect("post-close clean evidence");
        let dashboard = service.monitoring_dashboard();
        assert_eq!(dashboard.clean_live_days, 1);
        assert_eq!(dashboard.unresolved_incidents, 0);
        assert!(!dashboard.promotion_eligible);

        drop(service);
        let recovered = test_service(LiveRunMode::Canary, &path);
        assert!(!recovered.monitoring_dashboard().broker_connected);
        assert_eq!(recovered.monitoring_dashboard().clean_live_days, 1);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn sixty_configured_clean_sessions_are_required_and_recoverable() {
        let path = journal_path("sixty-day-gate");
        let dates = [
            "2026-01-02",
            "2026-01-05",
            "2026-01-06",
            "2026-01-07",
            "2026-01-08",
            "2026-01-09",
            "2026-01-12",
            "2026-01-13",
            "2026-01-14",
            "2026-01-15",
            "2026-01-16",
            "2026-01-20",
            "2026-01-21",
            "2026-01-22",
            "2026-01-23",
            "2026-01-26",
            "2026-01-27",
            "2026-01-28",
            "2026-01-29",
            "2026-01-30",
            "2026-02-02",
            "2026-02-03",
            "2026-02-04",
            "2026-02-05",
            "2026-02-06",
            "2026-02-09",
            "2026-02-10",
            "2026-02-11",
            "2026-02-12",
            "2026-02-13",
            "2026-02-17",
            "2026-02-18",
            "2026-02-19",
            "2026-02-20",
            "2026-02-23",
            "2026-02-24",
            "2026-02-25",
            "2026-02-26",
            "2026-02-27",
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
        ];
        assert_eq!(dates.len(), 60);
        let sessions: Vec<_> = dates.iter().map(|date| live_session(date)).collect();
        let calendar = live_calendar(&sessions);
        let mut service = test_service(LiveRunMode::Canary, &path);
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:01:00Z",
            )
            .expect("managed-secret connection");

        for (index, session) in sessions.iter().enumerate() {
            let report = service
                .reconcile("operator.approver.001", &session.closes_at)
                .expect("clean independent reconciliation");
            assert!(report.is_clean());
            service
                .record_live_session(session, &report, "operator.approver.001", &calendar)
                .expect("calendar-backed session evidence");
            assert_eq!(service.promotion_status().clean_live_days, index as u32 + 1);
            assert_eq!(
                service.promotion_status().eligible_for_next_gate,
                index == 59
            );
        }
        drop(service);

        let recovered = test_service(LiveRunMode::Canary, &path);
        let promotion = recovered.promotion_status();
        assert_eq!(promotion.clean_live_days, 60);
        assert!(promotion.complete_auditability);
        assert!(promotion.eligible_for_next_gate);
        assert!(!recovered.monitoring_dashboard().broker_connected);
        drop(recovered);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn shadow_records_a_decision_without_requesting_a_broker_connection_or_submit() {
        let path = journal_path("shadow");
        let mut service = test_service(LiveRunMode::Shadow, &path);
        let outcome = service
            .record_shadow_intent(
                intent("SHADOW", "intent.shadow.001"),
                market(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("shadow decision");
        assert!(matches!(outcome, LiveSubmitOutcome::ShadowRecorded { .. }));
        assert!(!service.monitoring_dashboard().broker_connected);
        assert_eq!(service.broker_mut().submitted, 0);
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn tampered_audit_journal_refuses_recovery() {
        let path = journal_path("tampered");
        let service = test_service(LiveRunMode::Shadow, &path);
        drop(service);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open test journal")
            .write_all(b"{not-json}\n")
            .expect("append tamper evidence");
        let account = account();
        let policy = policy();
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(LiveRunMode::Shadow, &account, &policy, &switches);
        assert!(LiveTradingService::open_durable(
            account,
            policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:31:00Z",
        )
        .is_err());
        std::fs::remove_file(path).expect("remove test journal");
    }
}
