//! Deterministic, read-only operational workbench primitives.
//!
//! This crate deliberately has no broker adapter, credential provider, wall-clock
//! lookup, background executor, or order mutation API. It turns a versioned
//! operational snapshot plus an explicitly supplied UTC instant into risk,
//! attribution, alert, scheduling, journal, and report evidence. That boundary
//! makes the operator-facing projection repeatable and safe to render locally.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use fs2::FileExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, Time};

pub mod attention;
pub mod game_day;

pub use attention::{AttentionBudget, AttentionBudgetController};
pub use game_day::{GameDayCompiler, InjectedFault, RecoveryDrillResult};

/// Version of the durable operations-journal line contract.
pub const OPERATIONS_JOURNAL_SCHEMA_VERSION: u32 = 1;
/// Version of the portable operations-dashboard contract.
pub const OPERATIONS_DASHBOARD_SCHEMA_VERSION: u32 = 1;
/// Typed journal event used for a configuration-bound schedule completion.
pub const SCHEDULE_COMPLETION_EVENT_TYPE: &str = "operations.schedule_completed.v2";
/// Typed immutable model-governance decision record.
pub const MODEL_RISK_EVENT_TYPE: &str = "operations.model_risk_recorded.v1";
/// Typed immutable operational game-day result record.
pub const GAME_DAY_EVENT_TYPE: &str = "operations.game_day_recorded.v1";
const LEGACY_SCHEDULE_COMPLETION_EVENT_TYPE: &str = "operations.schedule_completed.v1";
const EMPTY_JOURNAL_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const MAX_JOURNAL_BYTES: u64 = 128 * 1024 * 1024;
const JOURNAL_READ_ATTEMPTS: usize = 4;

/// A configuration, evidence, persistence, or arithmetic failure in operations tooling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationsError(pub String);

impl std::fmt::Display for OperationsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for OperationsError {}

impl From<follon_domain::DomainError> for OperationsError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DecimalError> for OperationsError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

/// Immutable strategy, data, replay, and configuration identities needed to reproduce a view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReproducibilityStamp {
    /// Canonical strategy identity.
    pub strategy_id: String,
    /// Immutable strategy version.
    pub strategy_version: String,
    /// SHA-256 of the strategy bundle.
    pub strategy_bundle_hash: String,
    /// Canonical dataset identity.
    pub dataset_id: String,
    /// Immutable dataset version.
    pub dataset_version: String,
    /// SHA-256 of normalized historical input data.
    pub dataset_hash: String,
    /// SHA-256 of the replay/event output selected for this view.
    pub replay_event_hash: String,
}

impl ReproducibilityStamp {
    /// Validates all durable identities before they become report evidence.
    pub fn validate(&self) -> Result<(), OperationsError> {
        for (name, value) in [
            ("strategy_id", self.strategy_id.as_str()),
            ("dataset_id", self.dataset_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.strategy_version.is_empty() || self.dataset_version.is_empty() {
            return Err(OperationsError(
                "strategy_version and dataset_version are required".to_owned(),
            ));
        }
        for (name, value) in [
            ("strategy_bundle_hash", self.strategy_bundle_hash.as_str()),
            ("dataset_hash", self.dataset_hash.as_str()),
            ("replay_event_hash", self.replay_event_hash.as_str()),
        ] {
            validate_sha256(name, value)?;
        }
        Ok(())
    }
}

/// The control classification of one parameter revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterControl {
    /// A bounded strategy parameter that needs no four-eyes record in this local tool.
    Standard,
    /// A risk-sensitive parameter requiring distinct requester and approver identities.
    TwoPerson,
}

impl ParameterControl {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "STANDARD",
            Self::TwoPerson => "TWO_PERSON",
        }
    }
}

/// Recorded approval for a risk-sensitive parameter revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterApproval {
    /// Canonical immutable external approval-record identity.
    pub approval_id: String,
    /// Canonical requester identity.
    pub requested_by: String,
    /// Canonical approver identity, distinct from requester.
    pub approved_by: String,
    /// UTC time at which the revision was approved.
    pub approved_at: String,
    /// SHA-256 of the exact parameter economics/control subject approved.
    pub approval_subject_hash: String,
    /// SHA-256 of the authorization policy selected for this approval.
    pub authorization_policy_hash: String,
    /// SHA-256 of the durable external approval evidence (for example a
    /// signed approval record retained by the authorization system).
    pub approval_evidence_hash: String,
}

/// One exact bounded configuration parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterValue {
    /// Canonical parameter identity.
    pub parameter_id: String,
    /// Exact selected value.
    pub value: Decimal,
    /// Inclusive exact lower bound.
    pub minimum: Decimal,
    /// Inclusive exact upper bound.
    pub maximum: Decimal,
    /// Required control for this value.
    pub control: ParameterControl,
    /// Approval for `TWO_PERSON` values; absent for standard parameters.
    pub approval: Option<ParameterApproval>,
}

/// Immutable, validated parameter revision presented to an operator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterSet {
    /// Canonical parameter-set identity.
    pub parameter_set_id: String,
    /// Human-assigned immutable revision.
    pub revision: String,
    /// Immutable direct predecessor revision. `None` is valid only for the
    /// first known revision of a parameter set.
    pub previous_revision: Option<String>,
    /// Exact semantic fingerprint of the direct predecessor. This prevents a
    /// reused human revision label from creating an ambiguous lineage.
    pub previous_parameter_set_fingerprint: Option<String>,
    /// Exact values. Their canonical ordering is independent of source order.
    pub values: Vec<ParameterValue>,
}

impl ParameterSet {
    /// Validates bounds, unique parameter identities, and required two-person approvals.
    pub fn validate(&self) -> Result<(), OperationsError> {
        validate_canonical_id("parameter_set_id", &self.parameter_set_id)?;
        validate_canonical_id("parameter revision", &self.revision)?;
        if let Some(previous_revision) = &self.previous_revision {
            validate_canonical_id("parameter previous_revision", previous_revision)?;
        }
        if self.revision.is_empty()
            || self
                .previous_revision
                .as_deref()
                .is_some_and(|previous| previous.is_empty() || previous == self.revision)
            || self.previous_revision.is_some() != self.previous_parameter_set_fingerprint.is_some()
            || self.values.is_empty()
        {
            return Err(OperationsError(
                "parameter set needs a revision, a distinct predecessor revision and fingerprint when present, and at least one value"
                    .to_owned(),
            ));
        }
        if let Some(previous_fingerprint) = &self.previous_parameter_set_fingerprint {
            validate_sha256(
                "parameter previous_parameter_set_fingerprint",
                previous_fingerprint,
            )?;
        }
        let approval_subject_hash = self.approval_subject_fingerprint()?;
        let mut ids = BTreeSet::new();
        for value in &self.values {
            validate_canonical_id("parameter_id", &value.parameter_id)?;
            if !ids.insert(&value.parameter_id) {
                return Err(OperationsError("duplicate parameter_id".to_owned()));
            }
            if value.minimum > value.maximum
                || value.value < value.minimum
                || value.value > value.maximum
            {
                return Err(OperationsError(format!(
                    "parameter {} is outside its inclusive bounds",
                    value.parameter_id
                )));
            }
            match (&value.control, &value.approval) {
                (ParameterControl::Standard, None) => {}
                (ParameterControl::Standard, Some(_)) => {
                    return Err(OperationsError(format!(
                        "standard parameter {} must not carry an approval",
                        value.parameter_id
                    )));
                }
                (ParameterControl::TwoPerson, Some(approval)) => {
                    validate_canonical_id("parameter approval_id", &approval.approval_id)?;
                    validate_canonical_id("parameter requester", &approval.requested_by)?;
                    validate_canonical_id("parameter approver", &approval.approved_by)?;
                    validate_utc_timestamp("parameter approved_at", &approval.approved_at)?;
                    for (name, value) in [
                        (
                            "parameter approval_subject_hash",
                            approval.approval_subject_hash.as_str(),
                        ),
                        (
                            "parameter authorization_policy_hash",
                            approval.authorization_policy_hash.as_str(),
                        ),
                        (
                            "parameter approval_evidence_hash",
                            approval.approval_evidence_hash.as_str(),
                        ),
                    ] {
                        validate_sha256(name, value)?;
                    }
                    if approval.requested_by == approval.approved_by {
                        return Err(OperationsError(
                            "two-person parameter approval requires different people".to_owned(),
                        ));
                    }
                    if approval.approval_subject_hash != approval_subject_hash {
                        return Err(OperationsError(format!(
                            "two-person approval is not bound to this exact parameter revision: expected {approval_subject_hash}"
                        )));
                    }
                }
                (ParameterControl::TwoPerson, None) => {
                    return Err(OperationsError(format!(
                        "risk-sensitive parameter {} requires two-person approval",
                        value.parameter_id
                    )));
                }
            }
        }
        Ok(())
    }

    /// Stable SHA-256 of the parameter economics, bounds, controls, revision,
    /// and exact predecessor pointer without approval metadata. An external
    /// approval record must bind this value before it can authorize a
    /// `TWO_PERSON` parameter.
    pub fn approval_subject_fingerprint(&self) -> Result<String, OperationsError> {
        validate_canonical_id("parameter_set_id", &self.parameter_set_id)?;
        validate_canonical_id("parameter revision", &self.revision)?;
        if let Some(previous_revision) = &self.previous_revision {
            validate_canonical_id("parameter previous_revision", previous_revision)?;
        }
        if self.revision.is_empty()
            || self
                .previous_revision
                .as_deref()
                .is_some_and(|previous| previous.is_empty() || previous == self.revision)
            || self.previous_revision.is_some() != self.previous_parameter_set_fingerprint.is_some()
            || self.values.is_empty()
        {
            return Err(OperationsError(
                "parameter approval subject needs a valid revision lineage and at least one value"
                    .to_owned(),
            ));
        }
        if let Some(previous_fingerprint) = &self.previous_parameter_set_fingerprint {
            validate_sha256(
                "parameter previous_parameter_set_fingerprint",
                previous_fingerprint,
            )?;
        }
        let mut values = self.values.clone();
        values.sort_by(|left, right| left.parameter_id.cmp(&right.parameter_id));
        let mut ids = BTreeSet::new();
        let mut canonical = format!(
            "parameter_set_id={}\nrevision={}\nprevious_revision={}\nprevious_parameter_set_fingerprint={}\n",
            self.parameter_set_id,
            self.revision,
            self.previous_revision.as_deref().unwrap_or(""),
            self.previous_parameter_set_fingerprint.as_deref().unwrap_or(""),
        );
        for value in values {
            validate_canonical_id("parameter_id", &value.parameter_id)?;
            if !ids.insert(value.parameter_id.clone())
                || value.minimum > value.maximum
                || value.value < value.minimum
                || value.value > value.maximum
            {
                return Err(OperationsError(
                    "parameter approval subject has duplicate IDs or invalid bounds".to_owned(),
                ));
            }
            canonical.push_str(&format!(
                "id={}\nvalue={}\nminimum={}\nmaximum={}\ncontrol={}\n",
                value.parameter_id,
                value.value,
                value.minimum,
                value.maximum,
                value.control.as_str(),
            ));
        }
        Ok(sha256(&canonical))
    }

    /// Stable SHA-256 fingerprint of this semantically complete revision.
    pub fn fingerprint(&self) -> Result<String, OperationsError> {
        self.validate()?;
        let mut values = self.values.clone();
        values.sort_by(|left, right| left.parameter_id.cmp(&right.parameter_id));
        let mut canonical = format!(
            "parameter_set_id={}\nrevision={}\nprevious_revision={}\nprevious_parameter_set_fingerprint={}\n",
            self.parameter_set_id,
            self.revision,
            self.previous_revision.as_deref().unwrap_or(""),
            self.previous_parameter_set_fingerprint.as_deref().unwrap_or(""),
        );
        for value in values {
            canonical.push_str(&format!(
                "id={}\nvalue={}\nminimum={}\nmaximum={}\ncontrol={}\n",
                value.parameter_id,
                value.value,
                value.minimum,
                value.maximum,
                value.control.as_str()
            ));
            if let Some(approval) = value.approval {
                canonical.push_str(&format!(
                    "approval_id={}\nrequested_by={}\napproved_by={}\napproved_at={}\napproval_subject_hash={}\nauthorization_policy_hash={}\napproval_evidence_hash={}\n",
                    approval.approval_id,
                    approval.requested_by,
                    approval.approved_by,
                    approval.approved_at,
                    approval.approval_subject_hash,
                    approval.authorization_policy_hash,
                    approval.approval_evidence_hash,
                ));
            }
        }
        Ok(sha256(&canonical))
    }

    /// Compares this immutable revision to its declared immediate predecessor
    /// without relying on source-file ordering. Both revisions must belong to
    /// the same parameter set and the target must directly name the source
    /// revision as its predecessor.
    pub fn diff_from(&self, previous: &Self) -> Result<Vec<ParameterChange>, OperationsError> {
        self.validate()?;
        previous.validate()?;
        if self.parameter_set_id != previous.parameter_set_id {
            return Err(OperationsError(
                "parameter revisions must belong to the same parameter_set_id".to_owned(),
            ));
        }
        if self.revision == previous.revision {
            return Err(OperationsError(
                "parameter revisions must have different revision labels".to_owned(),
            ));
        }
        if self.previous_revision.as_deref() != Some(previous.revision.as_str()) {
            return Err(OperationsError(
                "target parameter revision must declare the supplied source revision as its predecessor"
                    .to_owned(),
            ));
        }
        let previous_fingerprint = previous.fingerprint()?;
        if self.previous_parameter_set_fingerprint.as_deref() != Some(previous_fingerprint.as_str())
        {
            return Err(OperationsError(
                "target parameter revision must bind the supplied source parameter fingerprint"
                    .to_owned(),
            ));
        }
        let current_by_id = self
            .values
            .iter()
            .map(|value| (value.parameter_id.as_str(), value))
            .collect::<BTreeMap<_, _>>();
        let previous_by_id = previous
            .values
            .iter()
            .map(|value| (value.parameter_id.as_str(), value))
            .collect::<BTreeMap<_, _>>();
        let parameter_ids = current_by_id
            .keys()
            .chain(previous_by_id.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        Ok(parameter_ids
            .into_iter()
            .filter_map(|parameter_id| {
                let before = previous_by_id.get(parameter_id).cloned().cloned();
                let after = current_by_id.get(parameter_id).cloned().cloned();
                let kind = match (&before, &after) {
                    (None, Some(_)) => ParameterChangeKind::Added,
                    (Some(_), None) => ParameterChangeKind::Removed,
                    (Some(before), Some(after)) if before != after => ParameterChangeKind::Modified,
                    (None, None) | (Some(_), Some(_)) => return None,
                };
                Some(ParameterChange {
                    parameter_id: parameter_id.to_owned(),
                    kind,
                    before,
                    after,
                })
            })
            .collect())
    }
}

/// The semantic class of a parameter revision change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterChangeKind {
    /// A parameter was introduced by the target revision.
    Added,
    /// A parameter was removed by the target revision.
    Removed,
    /// Economics, bounds, control, or approval evidence changed.
    Modified,
}

impl ParameterChangeKind {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Added => "ADDED",
            Self::Removed => "REMOVED",
            Self::Modified => "MODIFIED",
        }
    }
}

/// One canonical difference between two controlled parameter revisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterChange {
    /// Canonical parameter identity.
    pub parameter_id: String,
    /// Whether the parameter was added, removed, or modified.
    pub kind: ParameterChangeKind,
    /// Previous complete value, if present.
    pub before: Option<ParameterValue>,
    /// Target complete value, if present.
    pub after: Option<ParameterValue>,
}

/// One marked position used only for deterministic risk and attribution projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationalPosition {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Signed exact quantity.
    pub quantity: Decimal,
    /// Positive exact mark at `OperationsSnapshot::as_of`.
    pub mark_price: Decimal,
    /// Non-negative average cost used to derive unrealized P&L.
    pub average_cost: Decimal,
    /// Exact realized P&L supplied by the accounted ledger.
    pub realized_pnl: Decimal,
}

/// Exact risk limits that are visible to an operator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskLimits {
    /// Maximum gross marked exposure in the reporting currency.
    pub max_gross_exposure: Decimal,
    /// Maximum absolute marked exposure in one instrument.
    pub max_single_instrument_exposure: Decimal,
    /// Maximum peak-to-trough drawdown in basis points.
    pub max_drawdown_bps: Decimal,
    /// Maximum non-terminal orders.
    pub max_working_orders: usize,
    /// Maximum ambiguous order states.
    pub max_unknown_orders: usize,
    /// Maximum unreconciled incidents.
    pub max_unresolved_incidents: usize,
}

impl RiskLimits {
    /// Validates non-negative, meaningful bounds.
    pub fn validate(&self) -> Result<(), OperationsError> {
        if self.max_gross_exposure < Decimal::ZERO
            || self.max_single_instrument_exposure < Decimal::ZERO
            || self.max_drawdown_bps < Decimal::ZERO
        {
            return Err(OperationsError("risk limits cannot be negative".to_owned()));
        }
        Ok(())
    }
}

/// Reserved parameter IDs which exclusively govern the displayed hard risk
/// limits. Their policy is compiled into the evidence engine, rather than
/// supplied by a mutable configuration file.
const RESERVED_RISK_LIMIT_PARAMETER_IDS: [&str; 6] = [
    "risk.max_gross_exposure",
    "risk.max_single_instrument_exposure",
    "risk.max_drawdown_bps",
    "risk.max_working_orders",
    "risk.max_unknown_orders",
    "risk.max_unresolved_incidents",
];

/// SHA-256 identity of the immutable v1 risk-limit authorization policy.
/// Every reserved risk-limit parameter must cite this policy and have a
/// four-eyes approval that binds the full parameter revision.
pub fn risk_limit_authorization_policy_fingerprint() -> String {
    sha256(
        "policy=follon.operations.risk-limit-authorization.v1\n\
risk.max_gross_exposure=TWO_PERSON\n\
risk.max_single_instrument_exposure=TWO_PERSON\n\
risk.max_drawdown_bps=TWO_PERSON\n\
risk.max_working_orders=TWO_PERSON\n\
risk.max_unknown_orders=TWO_PERSON\n\
risk.max_unresolved_incidents=TWO_PERSON\n",
    )
}

/// Ensures the independent risk-cockpit fields cannot drift from the exact
/// risk values approved in the parameter revision. This intentionally has no
/// configuration-defined aliases: the mapping is a trusted policy boundary.
fn validate_risk_limit_parameter_policy(
    parameters: &ParameterSet,
    limits: &RiskLimits,
) -> Result<(), OperationsError> {
    let expected = [
        ("risk.max_gross_exposure", limits.max_gross_exposure),
        (
            "risk.max_single_instrument_exposure",
            limits.max_single_instrument_exposure,
        ),
        ("risk.max_drawdown_bps", limits.max_drawdown_bps),
        (
            "risk.max_working_orders",
            count_decimal(limits.max_working_orders)?,
        ),
        (
            "risk.max_unknown_orders",
            count_decimal(limits.max_unknown_orders)?,
        ),
        (
            "risk.max_unresolved_incidents",
            count_decimal(limits.max_unresolved_incidents)?,
        ),
    ];
    let required_policy_hash = risk_limit_authorization_policy_fingerprint();
    for (parameter_id, expected_value) in expected {
        let parameter = parameters
            .values
            .iter()
            .find(|value| value.parameter_id == parameter_id)
            .ok_or_else(|| {
                OperationsError(format!(
                    "missing required approved risk-limit parameter {parameter_id}"
                ))
            })?;
        if parameter.control != ParameterControl::TwoPerson {
            return Err(OperationsError(format!(
                "reserved risk-limit parameter {parameter_id} must require TWO_PERSON control"
            )));
        }
        let approval = parameter.approval.as_ref().ok_or_else(|| {
            OperationsError(format!(
                "reserved risk-limit parameter {parameter_id} lacks approval"
            ))
        })?;
        if approval.authorization_policy_hash != required_policy_hash {
            return Err(OperationsError(format!(
                "reserved risk-limit parameter {parameter_id} cites an untrusted authorization policy"
            )));
        }
        if parameter.value != expected_value {
            return Err(OperationsError(format!(
                "reserved risk-limit parameter {parameter_id} does not match its enforced risk limit"
            )));
        }
    }
    for parameter in &parameters.values {
        if parameter.parameter_id.starts_with("risk.")
            && !RESERVED_RISK_LIMIT_PARAMETER_IDS.contains(&parameter.parameter_id.as_str())
        {
            return Err(OperationsError(format!(
                "unknown reserved risk parameter {}",
                parameter.parameter_id
            )));
        }
    }
    Ok(())
}

/// Read-only health signals gathered by independently owned runtime controls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationalHealth {
    /// Whether the upstream audit chain verified before this projection.
    pub audit_healthy: bool,
    /// Whether the most recent independent reconciliation is clean.
    pub reconciliation_healthy: bool,
    /// Whether the broker transport is currently connected.
    pub broker_connected: bool,
    /// Current active kill-switch scopes, if any.
    pub active_kill_switches: Vec<String>,
    /// Non-terminal OMS orders.
    pub working_orders: usize,
    /// Deliberately ambiguous OMS order states.
    pub unknown_orders: usize,
    /// Discrepancies that require a recorded resolution.
    pub unresolved_incidents: usize,
}

impl OperationalHealth {
    /// Validates kill-switch scope labels as durable, non-secret evidence.
    pub fn validate(&self) -> Result<(), OperationsError> {
        let mut scopes = BTreeSet::new();
        for scope in &self.active_kill_switches {
            if scope.is_empty() || scope.len() > 256 || !scopes.insert(scope) {
                return Err(OperationsError(
                    "kill-switch scopes must be non-empty and unique".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// Categories whose exact amounts explain a period's attributed P&L.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AttributionCategory {
    /// Closed-lot profit or loss after the accounting policy's execution treatment.
    RealizedPnl,
    /// Mark-to-market open-lot profit or loss.
    UnrealizedPnl,
    /// Trading and carrying charges. Fees normally use negative amounts.
    Fees,
    /// Cash dividends or distributions.
    Dividends,
    /// Other recognized corporate-action economics.
    CorporateActions,
}

impl AttributionCategory {
    /// Parses the stable external contract representation.
    pub fn parse(value: &str) -> Result<Self, OperationsError> {
        match value {
            "REALIZED_PNL" => Ok(Self::RealizedPnl),
            "UNREALIZED_PNL" => Ok(Self::UnrealizedPnl),
            "FEES" => Ok(Self::Fees),
            "DIVIDENDS" => Ok(Self::Dividends),
            "CORPORATE_ACTIONS" => Ok(Self::CorporateActions),
            _ => Err(OperationsError(
                "unsupported attribution category".to_owned(),
            )),
        }
    }

    /// Stable external contract representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RealizedPnl => "REALIZED_PNL",
            Self::UnrealizedPnl => "UNREALIZED_PNL",
            Self::Fees => "FEES",
            Self::Dividends => "DIVIDENDS",
            Self::CorporateActions => "CORPORATE_ACTIONS",
        }
    }
}

/// Immutable accounted movement selected for an attribution report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributionEntry {
    /// Canonical idempotency identity.
    pub entry_id: String,
    /// Economic recognition time.
    pub occurred_at: String,
    /// Canonical affected instrument.
    pub instrument_id: String,
    /// Economic attribution family.
    pub category: AttributionCategory,
    /// Exact signed effect in the report currency.
    pub amount: Decimal,
}

/// A daily UTC schedule definition; the planner never executes external work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DailySchedule {
    /// Canonical schedule identity.
    pub schedule_id: String,
    /// Operator-visible concise purpose.
    pub purpose: String,
    /// Daily UTC time in `HH:MM` form.
    pub time_utc: String,
    /// Whether this schedule is intentionally enabled.
    pub enabled: bool,
    /// Latest durable completion time, if one has been journaled.
    pub last_completed_at: Option<String>,
}

/// Complete immutable operator-workbench input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationsSnapshot {
    /// Exact UTC moment selected by the caller; no wall clock is consulted.
    pub as_of: String,
    /// `SIMULATION`, `PAPER`, or `LIVE` environment identity.
    pub environment: String,
    /// Canonical account identity.
    pub account_id: String,
    /// Three-letter reporting currency.
    pub currency: String,
    /// Canonical immutable configuration identity.
    pub configuration_id: String,
    /// Immutable human version of the selected configuration.
    pub configuration_version: String,
    /// SHA-256 of exact configuration source bytes.
    pub configuration_content_hash: String,
    /// Strategy, data, and replay input identities.
    pub reproducibility: ReproducibilityStamp,
    /// Validated strategy/risk parameter revision.
    pub parameters: ParameterSet,
    /// Exact starting equity of the reported evaluation.
    pub starting_equity: Decimal,
    /// Exact cash at `as_of`.
    pub cash: Decimal,
    /// Highest known equity including or preceding `as_of`.
    pub peak_equity: Decimal,
    /// Exact marked positions.
    pub positions: Vec<OperationalPosition>,
    /// Explicit risk limits.
    pub risk_limits: RiskLimits,
    /// Independently collected operational health.
    pub health: OperationalHealth,
    /// Immutable accounting attribution movements.
    pub attribution_entries: Vec<AttributionEntry>,
    /// Declarative daily schedules.
    pub schedules: Vec<DailySchedule>,
}

impl OperationsSnapshot {
    /// Validates the snapshot before any risk, report, or scheduling derivation.
    pub fn validate(&self) -> Result<(), OperationsError> {
        validate_utc_timestamp("operations as_of", &self.as_of)?;
        if !matches!(self.environment.as_str(), "SIMULATION" | "PAPER" | "LIVE") {
            return Err(OperationsError(
                "operations environment must be SIMULATION, PAPER, or LIVE".to_owned(),
            ));
        }
        for (name, value) in [
            ("operations account_id", self.account_id.as_str()),
            (
                "operations configuration_id",
                self.configuration_id.as_str(),
            ),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.configuration_version.is_empty()
        {
            return Err(OperationsError(
                "operations currency and configuration version are invalid".to_owned(),
            ));
        }
        validate_sha256(
            "operations configuration_content_hash",
            &self.configuration_content_hash,
        )?;
        self.reproducibility.validate()?;
        self.parameters.validate()?;
        for parameter in &self.parameters.values {
            if let Some(approval) = &parameter.approval {
                if approval.approved_at > self.as_of {
                    return Err(OperationsError(format!(
                        "parameter {} approval cannot be after snapshot as_of",
                        parameter.parameter_id
                    )));
                }
            }
        }
        if self.starting_equity < Decimal::ZERO
            || self.cash < Decimal::ZERO
            || self.peak_equity <= Decimal::ZERO
        {
            return Err(OperationsError(
                "starting equity and cash must be non-negative; peak equity must be positive"
                    .to_owned(),
            ));
        }
        self.risk_limits.validate()?;
        validate_risk_limit_parameter_policy(&self.parameters, &self.risk_limits)?;
        self.health.validate()?;
        let mut positions = BTreeSet::new();
        for position in &self.positions {
            validate_canonical_id("position instrument_id", &position.instrument_id)?;
            if !positions.insert(&position.instrument_id)
                || position.mark_price <= Decimal::ZERO
                || position.average_cost < Decimal::ZERO
            {
                return Err(OperationsError(
                    "positions must be unique with positive marks and non-negative costs"
                        .to_owned(),
                ));
            }
        }
        let mut entry_ids = BTreeSet::new();
        for entry in &self.attribution_entries {
            validate_canonical_id("attribution entry_id", &entry.entry_id)?;
            validate_canonical_id("attribution instrument_id", &entry.instrument_id)?;
            validate_utc_timestamp("attribution occurred_at", &entry.occurred_at)?;
            if !entry_ids.insert(&entry.entry_id) {
                return Err(OperationsError("duplicate attribution entry_id".to_owned()));
            }
        }
        let mut schedule_ids = BTreeSet::new();
        for schedule in &self.schedules {
            validate_canonical_id("schedule_id", &schedule.schedule_id)?;
            if !schedule_ids.insert(&schedule.schedule_id)
                || schedule.purpose.trim().is_empty()
                || schedule.purpose.len() > 256
                || schedule.purpose.contains(['\r', '\n'])
            {
                return Err(OperationsError(
                    "schedules need unique IDs and one-line concise purposes".to_owned(),
                ));
            }
            parse_schedule_time(&schedule.time_utc)?;
            if let Some(completed) = &schedule.last_completed_at {
                validate_schedule_completion_time(schedule, completed)?;
                if completed > &self.as_of {
                    return Err(OperationsError(
                        "schedule last_completed_at cannot be after snapshot as_of".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Stable content hash used to bind an operational report to the selected source bytes.
    pub fn fingerprint(&self) -> Result<String, OperationsError> {
        self.validate()?;
        Ok(sha256(&format!(
            "config={}\nparameters={}\nstrategy={}\ndataset={}\nreplay={}\nas_of={}\n",
            self.configuration_content_hash,
            self.parameters.fingerprint()?,
            self.reproducibility.strategy_bundle_hash,
            self.reproducibility.dataset_hash,
            self.reproducibility.replay_event_hash,
            self.as_of,
        )))
    }
}

/// Stable fingerprint of an operator projection, including the verified
/// journal cursor that can affect schedule state and alerts. This is distinct
/// from [`OperationsSnapshot::fingerprint`], which identifies source inputs
/// before journal-derived projection.
pub fn projection_fingerprint(
    snapshot: &OperationsSnapshot,
    journal: &JournalInspection,
) -> Result<String, OperationsError> {
    snapshot.validate()?;
    validate_journal_inspection(journal)?;
    Ok(sha256(&format!(
        "source_fingerprint={}\njournal_healthy={}\njournal_sequence={}\njournal_head_hash={}\njournal_failure_reason={}\n",
        snapshot.fingerprint()?,
        journal.healthy,
        journal.sequence,
        journal.head_hash,
        json_string(journal.failure_reason.as_deref().unwrap_or("")),
    )))
}

/// One explicit current-versus-limit risk measurement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskLimitUsage {
    /// Stable metric identity.
    pub limit_id: String,
    /// Exact observed amount (counts are represented exactly as integral decimals).
    pub current: Decimal,
    /// Exact configured hard limit.
    pub limit: Decimal,
    /// Whether the observed amount exceeds its limit.
    pub breached: bool,
}

/// Read-only deterministic risk cockpit projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskCockpit {
    /// `NORMAL`, `WARNING`, or `CRITICAL` derived state.
    pub state: String,
    /// Exact cash plus net marked positions.
    pub current_equity: Decimal,
    /// Higher of input peak and current equity.
    pub effective_peak_equity: Decimal,
    /// Sum of absolute marked position values.
    pub gross_exposure: Decimal,
    /// Largest absolute marked exposure of one instrument.
    pub largest_position_exposure: Decimal,
    /// Exact peak-to-current drawdown in basis points.
    pub drawdown_bps: Decimal,
    /// Count of non-zero marked positions.
    pub open_positions: usize,
    /// Explicit visible risk-limit usage.
    pub limits: Vec<RiskLimitUsage>,
}

/// Derives the risk cockpit only from immutable snapshot input.
pub fn derive_risk_cockpit(snapshot: &OperationsSnapshot) -> Result<RiskCockpit, OperationsError> {
    snapshot.validate()?;
    let mut net_market_value = Decimal::ZERO;
    let mut gross_exposure = Decimal::ZERO;
    let mut largest_position_exposure = Decimal::ZERO;
    let mut open_positions = 0usize;
    for position in &snapshot.positions {
        let market_value = position.quantity.checked_mul(position.mark_price)?;
        net_market_value = net_market_value.checked_add(market_value)?;
        let absolute_value = absolute(market_value)?;
        gross_exposure = gross_exposure.checked_add(absolute_value)?;
        largest_position_exposure = largest_position_exposure.max(absolute_value);
        if position.quantity != Decimal::ZERO {
            open_positions = open_positions
                .checked_add(1)
                .ok_or_else(|| OperationsError("too many open positions".to_owned()))?;
        }
    }
    let current_equity = snapshot.cash.checked_add(net_market_value)?;
    let effective_peak_equity = snapshot.peak_equity.max(current_equity);
    let drawdown_bps =
        if effective_peak_equity > Decimal::ZERO && current_equity < effective_peak_equity {
            effective_peak_equity
                .checked_sub(current_equity)?
                .checked_mul(Decimal::from_integer(10_000)?)?
                .checked_div(effective_peak_equity)?
        } else {
            Decimal::ZERO
        };
    let limits = vec![
        usage(
            "gross_exposure",
            gross_exposure,
            snapshot.risk_limits.max_gross_exposure,
        ),
        usage(
            "single_instrument_exposure",
            largest_position_exposure,
            snapshot.risk_limits.max_single_instrument_exposure,
        ),
        usage(
            "drawdown_bps",
            drawdown_bps,
            snapshot.risk_limits.max_drawdown_bps,
        ),
        usage(
            "working_orders",
            count_decimal(snapshot.health.working_orders)?,
            count_decimal(snapshot.risk_limits.max_working_orders)?,
        ),
        usage(
            "unknown_orders",
            count_decimal(snapshot.health.unknown_orders)?,
            count_decimal(snapshot.risk_limits.max_unknown_orders)?,
        ),
        usage(
            "unresolved_incidents",
            count_decimal(snapshot.health.unresolved_incidents)?,
            count_decimal(snapshot.risk_limits.max_unresolved_incidents)?,
        ),
    ];
    let state = if !snapshot.health.audit_healthy
        || !snapshot.health.reconciliation_healthy
        || !snapshot.health.active_kill_switches.is_empty()
        || limits.iter().any(|limit| limit.breached)
    {
        "CRITICAL"
    } else if !snapshot.health.broker_connected {
        "WARNING"
    } else {
        "NORMAL"
    };
    Ok(RiskCockpit {
        state: state.to_owned(),
        current_equity,
        effective_peak_equity,
        gross_exposure,
        largest_position_exposure,
        drawdown_bps,
        open_positions,
        limits,
    })
}

/// One grouped attributed amount for an instrument and category.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributionRow {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Attribution category.
    pub category: AttributionCategory,
    /// Exact sum of supporting entries.
    pub amount: Decimal,
}

/// Exact, deterministic P&L attribution projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributionReport {
    /// Sum of all included signed amounts.
    pub net_pnl: Decimal,
    /// Exact total by category.
    pub totals: BTreeMap<AttributionCategory, Decimal>,
    /// Canonically grouped instrument/category rows.
    pub rows: Vec<AttributionRow>,
}

/// Groups immutable accounting evidence into exact P&L attribution.
pub fn derive_attribution(
    snapshot: &OperationsSnapshot,
) -> Result<AttributionReport, OperationsError> {
    snapshot.validate()?;
    let mut grouped = BTreeMap::<(String, AttributionCategory), Decimal>::new();
    let mut totals = BTreeMap::<AttributionCategory, Decimal>::new();
    let mut net_pnl = Decimal::ZERO;
    for entry in &snapshot.attribution_entries {
        let group = grouped
            .entry((entry.instrument_id.clone(), entry.category))
            .or_insert(Decimal::ZERO);
        *group = group.checked_add(entry.amount)?;
        let total = totals.entry(entry.category).or_insert(Decimal::ZERO);
        *total = total.checked_add(entry.amount)?;
        net_pnl = net_pnl.checked_add(entry.amount)?;
    }
    let rows = grouped
        .into_iter()
        .map(|((instrument_id, category), amount)| AttributionRow {
            instrument_id,
            category,
            amount,
        })
        .collect();
    Ok(AttributionReport {
        net_pnl,
        totals,
        rows,
    })
}

/// A schedule's deterministic due-state at a caller-selected instant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleStatus {
    /// Canonical schedule identity.
    pub schedule_id: String,
    /// Operator-visible purpose.
    pub purpose: String,
    /// Daily UTC time.
    pub time_utc: String,
    /// Whether this schedule is enabled.
    pub enabled: bool,
    /// Exact next eligible UTC instant.
    pub next_due_at: String,
    /// Whether the schedule is due at the explicit `as_of` time.
    pub due: bool,
    /// Latest declared completion, if any.
    pub last_completed_at: Option<String>,
}

/// Computes daily schedule status without executing or acknowledging a job.
pub fn derive_schedule_statuses(
    snapshot: &OperationsSnapshot,
) -> Result<Vec<ScheduleStatus>, OperationsError> {
    snapshot.validate()?;
    let as_of = parse_utc(&snapshot.as_of)?;
    let mut statuses = Vec::with_capacity(snapshot.schedules.len());
    for schedule in &snapshot.schedules {
        let (hour, minute) = parse_schedule_time(&schedule.time_utc)?;
        let time = Time::from_hms(hour, minute, 0)
            .map_err(|_| OperationsError("invalid schedule time".to_owned()))?;
        let today = as_of.replace_time(time);
        let next = match &schedule.last_completed_at {
            None => today,
            Some(value) => {
                let completed = parse_utc(value)?;
                if completed.date() == as_of.date() {
                    let tomorrow = as_of.date().next_day().ok_or_else(|| {
                        OperationsError("schedule date exceeds supported range".to_owned())
                    })?;
                    today.replace_date(tomorrow)
                } else {
                    today
                }
            }
        };
        statuses.push(ScheduleStatus {
            schedule_id: schedule.schedule_id.clone(),
            purpose: schedule.purpose.clone(),
            time_utc: schedule.time_utc.clone(),
            enabled: schedule.enabled,
            next_due_at: format_utc(next)?,
            due: schedule.enabled && next <= as_of,
            last_completed_at: schedule.last_completed_at.clone(),
        });
    }
    statuses.sort_by(|left, right| left.schedule_id.cmp(&right.schedule_id));
    Ok(statuses)
}

/// Applies verified, configuration-bound schedule-completion facts to a
/// snapshot. This projection never executes a job and it does not mutate the
/// journal; callers append a completion only after the declared work finished.
///
/// Completion records are accepted only when they name the exact configuration
/// content hash and parameter fingerprint selected by the snapshot. That makes
/// a completion from an older configuration incapable of suppressing work due
/// under a revised configuration.
pub fn apply_schedule_completions(
    snapshot: &OperationsSnapshot,
    records: &[JournalRecord],
) -> Result<OperationsSnapshot, OperationsError> {
    snapshot.validate()?;
    let as_of = parse_utc(&snapshot.as_of)?;
    let parameter_set_fingerprint = snapshot.parameters.fingerprint()?;
    let schedules_by_id = snapshot
        .schedules
        .iter()
        .map(|schedule| (schedule.schedule_id.as_str(), schedule))
        .collect::<BTreeMap<_, _>>();
    let mut completions = BTreeMap::<&str, OffsetDateTime>::new();
    for record in records {
        if record.event_type == LEGACY_SCHEDULE_COMPLETION_EVENT_TYPE {
            // v1 lacked a scheduled-for instant and was intentionally never
            // trusted to suppress work. Preserve it as journal history only.
            continue;
        }
        if record.event_type != SCHEDULE_COMPLETION_EVENT_TYPE {
            continue;
        }
        let completion = schedule_completion_evidence(record)?;
        let completed_at = parse_utc(&record.occurred_at)?;
        if completed_at > as_of {
            continue;
        }
        if completion.configuration_hash != snapshot.configuration_content_hash {
            // Completion facts are scoped to immutable configuration bytes.
            // A normal approved revision therefore starts with a clean schedule
            // state instead of making the shared journal unusable.
            continue;
        }
        if completion.parameter_set_fingerprint != parameter_set_fingerprint {
            return Err(OperationsError(format!(
                "schedule completion {} does not match the selected parameter revision",
                record.entry_id
            )));
        }
        let schedule = schedules_by_id.get(completion.schedule_id).ok_or_else(|| {
            OperationsError(format!(
                "schedule completion {} names a schedule absent from this configuration",
                record.entry_id
            ))
        })?;
        let expected_scheduled_for = scheduled_instant_for(schedule, completed_at)?;
        if completion.scheduled_for != expected_scheduled_for {
            return Err(OperationsError(format!(
                "schedule completion {} does not bind the configured daily due instant",
                record.entry_id
            )));
        }
        if completed_at < completion.scheduled_for {
            return Err(OperationsError(format!(
                "schedule completion {} was recorded before its configured due instant",
                record.entry_id
            )));
        }
        completions
            .entry(completion.schedule_id)
            .and_modify(|latest| *latest = (*latest).max(completed_at))
            .or_insert(completed_at);
    }
    let mut projected = snapshot.clone();
    for schedule in &mut projected.schedules {
        let Some(completed_at) = completions.get(schedule.schedule_id.as_str()) else {
            continue;
        };
        let completed_at = format_utc(*completed_at)?;
        if schedule
            .last_completed_at
            .as_ref()
            .is_none_or(|existing| existing < &completed_at)
        {
            schedule.last_completed_at = Some(completed_at);
        }
    }
    projected.validate()?;
    Ok(projected)
}

/// Derives a completion-aware schedule plan from verified journal records.
pub fn derive_schedule_statuses_with_completions(
    snapshot: &OperationsSnapshot,
    records: &[JournalRecord],
) -> Result<Vec<ScheduleStatus>, OperationsError> {
    derive_schedule_statuses(&apply_schedule_completions(snapshot, records)?)
}

fn required_completion_detail<'a>(
    record: &'a JournalRecord,
    key: &str,
) -> Result<&'a str, OperationsError> {
    record
        .details
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            OperationsError(format!(
                "schedule completion {} lacks required {} evidence",
                record.entry_id, key
            ))
        })
}

struct ScheduleCompletionEvidence<'a> {
    schedule_id: &'a str,
    configuration_hash: &'a str,
    parameter_set_fingerprint: &'a str,
    scheduled_for: OffsetDateTime,
}

fn schedule_completion_evidence(
    record: &JournalRecord,
) -> Result<ScheduleCompletionEvidence<'_>, OperationsError> {
    let expected_keys = [
        "configuration_hash",
        "parameter_set_fingerprint",
        "schedule_id",
        "scheduled_for",
    ];
    if record.details.len() != expected_keys.len()
        || record
            .details
            .keys()
            .any(|key| !expected_keys.contains(&key.as_str()))
    {
        return Err(OperationsError(format!(
            "schedule completion {} has an invalid evidence shape",
            record.entry_id
        )));
    }
    let schedule_id = required_completion_detail(record, "schedule_id")?;
    validate_canonical_id("schedule completion schedule_id", schedule_id)?;
    let configuration_hash = required_completion_detail(record, "configuration_hash")?;
    validate_sha256("schedule completion configuration_hash", configuration_hash)?;
    let parameter_set_fingerprint =
        required_completion_detail(record, "parameter_set_fingerprint")?;
    validate_sha256(
        "schedule completion parameter_set_fingerprint",
        parameter_set_fingerprint,
    )?;
    let scheduled_for = parse_utc(required_completion_detail(record, "scheduled_for")?)?;
    Ok(ScheduleCompletionEvidence {
        schedule_id,
        configuration_hash,
        parameter_set_fingerprint,
        scheduled_for,
    })
}

/// Severity of one deterministic alert projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AlertSeverity {
    /// A non-blocking reminder requiring operator attention.
    Warning,
    /// A safety or evidence failure requiring operational resolution.
    Critical,
}

impl AlertSeverity {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "WARNING",
            Self::Critical => "CRITICAL",
        }
    }
}

/// An idempotent, actionable operational alert.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationalAlert {
    /// Stable SHA-256 fingerprint; repeated evaluation yields the same alert identity.
    pub alert_id: String,
    /// Alert severity.
    pub severity: AlertSeverity,
    /// Stable alert code.
    pub code: String,
    /// Canonical subject/resource identifier.
    pub subject: String,
    /// Concise operator explanation.
    pub summary: String,
}

/// Evaluates risk, operational health, and due schedules into deduplicated alerts.
pub fn derive_alerts(
    snapshot: &OperationsSnapshot,
) -> Result<Vec<OperationalAlert>, OperationsError> {
    let cockpit = derive_risk_cockpit(snapshot)?;
    let schedules = derive_schedule_statuses(snapshot)?;
    let mut alerts = Vec::new();
    if !snapshot.health.audit_healthy {
        alerts.push(alert(
            AlertSeverity::Critical,
            "audit_chain_unhealthy",
            &snapshot.account_id,
            "Audit-chain verification failed; operations require investigation.",
        ));
    }
    if !snapshot.health.reconciliation_healthy {
        alerts.push(alert(
            AlertSeverity::Critical,
            "reconciliation_unhealthy",
            &snapshot.account_id,
            "The latest independent reconciliation is not clean.",
        ));
    }
    if !snapshot.health.broker_connected {
        alerts.push(alert(
            AlertSeverity::Warning,
            "broker_disconnected",
            &snapshot.account_id,
            "Broker connectivity is unavailable; verify reconnect and reconciliation evidence.",
        ));
    }
    for scope in &snapshot.health.active_kill_switches {
        alerts.push(alert(
            AlertSeverity::Critical,
            "kill_switch_active",
            scope,
            "A kill switch is active for this scope.",
        ));
    }
    for limit in &cockpit.limits {
        if limit.breached {
            alerts.push(alert(
                AlertSeverity::Critical,
                "risk_limit_breached",
                &limit.limit_id,
                &format!(
                    "Risk limit {} is breached: current {} exceeds {}.",
                    limit.limit_id, limit.current, limit.limit
                ),
            ));
        }
    }
    for schedule in schedules.iter().filter(|schedule| schedule.due) {
        alerts.push(alert(
            AlertSeverity::Warning,
            "scheduled_work_due",
            &schedule.schedule_id,
            &format!("Scheduled work is due: {}.", schedule.purpose),
        ));
    }
    alerts.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.subject.cmp(&right.subject))
    });
    Ok(alerts)
}

/// Evaluates alerts with the journal verification result that backed the
/// selected projection. An unverifiable journal is never displayed as a quiet
/// cockpit condition.
pub fn derive_alerts_with_journal(
    snapshot: &OperationsSnapshot,
    journal: &JournalInspection,
) -> Result<Vec<OperationalAlert>, OperationsError> {
    validate_journal_inspection(journal)?;
    let mut alerts = derive_alerts(snapshot)?;
    if !journal.healthy {
        alerts.push(alert(
            AlertSeverity::Critical,
            "operations_journal_unhealthy",
            &snapshot.account_id,
            "The operations journal could not be verified; journal-derived schedule evidence is unavailable.",
        ));
    }
    alerts.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.subject.cmp(&right.subject))
    });
    Ok(alerts)
}

/// Verified journal cursor included in reports without exposing a mutable handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalInspection {
    /// Whether every existing line verified, or the journal has not yet been created.
    pub healthy: bool,
    /// Number of verified records.
    pub sequence: u64,
    /// SHA-256 of the last verified record, or the all-zero genesis hash.
    pub head_hash: String,
    /// Evidence-safe verification failure, if no trustworthy cursor could be established.
    pub failure_reason: Option<String>,
}

impl JournalInspection {
    /// Returns the deterministic empty-journal cursor.
    pub fn empty() -> Self {
        Self {
            healthy: true,
            sequence: 0,
            head_hash: EMPTY_JOURNAL_HASH.to_owned(),
            failure_reason: None,
        }
    }

    /// Safely represents a failed verification without pretending to know a head hash.
    pub fn unhealthy(reason: impl Into<String>) -> Self {
        Self {
            healthy: false,
            sequence: 0,
            head_hash: EMPTY_JOURNAL_HASH.to_owned(),
            failure_reason: Some(reason.into()),
        }
    }
}

fn validate_journal_inspection(journal: &JournalInspection) -> Result<(), OperationsError> {
    validate_sha256("operations journal head_hash", &journal.head_hash)?;
    match (journal.healthy, journal.failure_reason.as_deref()) {
        (true, None) => Ok(()),
        (false, Some(reason)) if !reason.is_empty() && journal.head_hash == EMPTY_JOURNAL_HASH => {
            Ok(())
        }
        (true, Some(_)) => Err(OperationsError(
            "healthy operations journal cannot carry a verification failure".to_owned(),
        )),
        (false, _) => Err(OperationsError(
            "unhealthy operations journal must carry a failure and no trusted head hash".to_owned(),
        )),
    }
}

/// One caller-supplied operation-journal record awaiting durable append.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalEntryInput {
    /// Canonical idempotency key. A duplicate id is refused.
    pub entry_id: String,
    /// Canonical operations event type such as `operations.report_generated.v1`.
    pub event_type: String,
    /// Canonical UTC occurrence time supplied by the caller.
    pub occurred_at: String,
    /// Canonical non-secret actor identity.
    pub actor: String,
    /// Small non-secret evidence fields in sorted deterministic order.
    pub details: BTreeMap<String, String>,
}

/// One verified durable operations-journal record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalRecord {
    /// Journal schema version.
    pub journal_schema_version: u32,
    /// Strictly increasing durable sequence.
    pub sequence: u64,
    /// Caller-selected idempotency key.
    pub entry_id: String,
    /// Stable event type.
    pub event_type: String,
    /// UTC occurrence time.
    pub occurred_at: String,
    /// Actor identity.
    pub actor: String,
    /// Non-secret evidence details.
    pub details: BTreeMap<String, String>,
    /// Previous record hash, or genesis hash at sequence one.
    pub prev_hash: String,
    /// SHA-256 of the canonical record body.
    pub record_hash: String,
}

/// A process-exclusive, fsynced, SHA-256 chained operation journal.
pub struct OperationalJournal {
    path: PathBuf,
    file: File,
    inspection: JournalInspection,
    entry_ids: BTreeSet<String>,
}

impl OperationalJournal {
    /// Opens or creates a process-exclusive journal after verifying every existing record.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, OperationsError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_error)?;
        FileExt::try_lock_exclusive(&file).map_err(|error| {
            OperationsError(format!(
                "operations journal is already in use: {} ({error})",
                path.display()
            ))
        })?;
        let (inspection, entry_ids, _) = verify_journal_file(&mut file)?;
        file.seek(SeekFrom::End(0)).map_err(io_error)?;
        Ok(Self {
            path,
            file,
            inspection,
            entry_ids,
        })
    }

    /// Verifies a journal read-only. A missing journal has the valid empty cursor.
    pub fn inspect(path: impl AsRef<Path>) -> Result<JournalInspection, OperationsError> {
        Self::read_verified(path).map(|(inspection, _)| inspection)
    }

    /// Reads all records only after verifying the complete journal hash chain.
    /// A missing journal produces the valid empty record sequence.
    pub fn read_verified_records(
        path: impl AsRef<Path>,
    ) -> Result<Vec<JournalRecord>, OperationsError> {
        Self::read_verified(path).map(|(_, records)| records)
    }

    /// Reads one internally consistent verified journal evidence snapshot.
    /// The returned cursor and records originate from the same validated file
    /// contents, so reports need not perform a separate, racy inspection.
    pub fn read_verified(
        path: impl AsRef<Path>,
    ) -> Result<(JournalInspection, Vec<JournalRecord>), OperationsError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok((JournalInspection::empty(), Vec::new()));
        }
        for attempt in 0..JOURNAL_READ_ATTEMPTS {
            let mut file = File::open(path).map_err(io_error)?;
            match FileExt::try_lock_shared(&file) {
                Ok(()) => {
                    return verify_journal_file(&mut file)
                        .map(|(inspection, _, records)| (inspection, records));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if attempt + 1 < JOURNAL_READ_ATTEMPTS {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
                Err(error) => return Err(io_error(error)),
            }
        }
        Err(OperationsError(format!(
            "operations journal is busy after {JOURNAL_READ_ATTEMPTS} stable-read attempts: {}",
            path.display()
        )))
    }

    /// Returns the journal path retained by this exclusive handle.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the latest verified cursor.
    pub fn inspection(&self) -> &JournalInspection {
        &self.inspection
    }

    /// Appends one validated record and fsyncs it before exposing the new cursor.
    pub fn append(&mut self, input: JournalEntryInput) -> Result<JournalRecord, OperationsError> {
        validate_journal_input(&input)?;
        if self.entry_ids.contains(&input.entry_id) {
            return Err(OperationsError(
                "duplicate operations journal entry_id".to_owned(),
            ));
        }
        let sequence =
            self.inspection.sequence.checked_add(1).ok_or_else(|| {
                OperationsError("operations journal sequence overflow".to_owned())
            })?;
        let prev_hash = self.inspection.head_hash.clone();
        let record_hash = sha256(&canonical_journal_body(
            sequence,
            &input.entry_id,
            &input.event_type,
            &input.occurred_at,
            &input.actor,
            &input.details,
            &prev_hash,
        ));
        let record = JournalRecord {
            journal_schema_version: OPERATIONS_JOURNAL_SCHEMA_VERSION,
            sequence,
            entry_id: input.entry_id,
            event_type: input.event_type,
            occurred_at: input.occurred_at,
            actor: input.actor,
            details: input.details,
            prev_hash,
            record_hash,
        };
        let line = record.canonical_json();
        let next_size = self
            .file
            .metadata()
            .map_err(io_error)?
            .len()
            .checked_add(u64::try_from(line.len() + 1).map_err(|_| {
                OperationsError("operations journal line length overflow".to_owned())
            })?)
            .ok_or_else(|| OperationsError("operations journal size overflow".to_owned()))?;
        if next_size > MAX_JOURNAL_BYTES {
            return Err(OperationsError(
                "operations journal exceeds its configured 128 MiB safety limit".to_owned(),
            ));
        }
        self.file.write_all(line.as_bytes()).map_err(io_error)?;
        self.file.write_all(b"\n").map_err(io_error)?;
        self.file.sync_all().map_err(io_error)?;
        self.entry_ids.insert(record.entry_id.clone());
        self.inspection.sequence = record.sequence;
        self.inspection.head_hash = record.record_hash.clone();
        self.inspection.healthy = true;
        self.inspection.failure_reason = None;
        Ok(record)
    }
}

impl JournalRecord {
    /// Stable JSON line persisted by the journal.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"actor\":{},\"details\":{},\"entry_id\":{},\"event_type\":{},\"journal_schema_version\":{},\"occurred_at\":{},\"prev_hash\":{},\"record_hash\":{},\"sequence\":{}}}",
            json_string(&self.actor),
            details_json(&self.details),
            json_string(&self.entry_id),
            json_string(&self.event_type),
            self.journal_schema_version,
            json_string(&self.occurred_at),
            json_string(&self.prev_hash),
            json_string(&self.record_hash),
            self.sequence,
        )
    }
}

/// Decision recorded by the accountable owner of a versioned strategy model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelRiskDecision {
    /// The bounded evidence supports promotion only within the active gate.
    Promote,
    /// The strategy version is no longer permitted to advance.
    Demote,
    /// The version remains under review with no promotion decision.
    Hold,
}

impl ModelRiskDecision {
    /// Stable evidence representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Promote => "PROMOTE",
            Self::Demote => "DEMOTE",
            Self::Hold => "HOLD",
        }
    }

    fn parse(value: &str) -> Result<Self, OperationsError> {
        match value {
            "PROMOTE" => Ok(Self::Promote),
            "DEMOTE" => Ok(Self::Demote),
            "HOLD" => Ok(Self::Hold),
            _ => Err(OperationsError(
                "model-risk decision must be PROMOTE, DEMOTE, or HOLD".to_owned(),
            )),
        }
    }
}

/// Verified, append-only model-risk record reconstructed from the operations journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelRiskRecord {
    /// Journal idempotency identity.
    pub record_id: String,
    /// UTC decision time retained by the journal.
    pub occurred_at: String,
    /// Accountable operator identity retained by the journal.
    pub actor: String,
    /// Strategy identity under review.
    pub strategy_id: String,
    /// Immutable strategy version under review.
    pub strategy_version: String,
    /// SHA-256 of the exact declared strategy bundle.
    pub strategy_bundle_hash: String,
    /// SHA-256 of the immutable backtest artifact used as decision evidence.
    pub backtest_artifact_hash: String,
    /// Promote, demote, or hold decision.
    pub decision: ModelRiskDecision,
    /// Concise, non-secret summary of the model change.
    pub change_summary: String,
    /// SHA-256 binding the change summary to the record.
    pub change_summary_hash: String,
    /// Concise, non-secret evidence-based reasoning.
    pub reason: String,
}

/// Verified, append-only fault-injection game-day record reconstructed from the journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDayRecord {
    /// Journal idempotency identity.
    pub record_id: String,
    /// UTC completion time retained by the journal.
    pub occurred_at: String,
    /// Accountable operator identity retained by the journal.
    pub actor: String,
    /// Canonical scenario identity from the approved exercise plan.
    pub scenario_id: String,
    /// Whether the declared exercise passed every acceptance assertion.
    pub passed: bool,
    /// SHA-256 of the immutable fault plan used for the exercise.
    pub fault_plan_hash: String,
    /// SHA-256 of the immutable run/test evidence artifact.
    pub evidence_hash: String,
    /// SHA-256 of the reconciliation evidence produced after recovery.
    pub reconciliation_hash: String,
    /// Concise, non-secret blameless postmortem or follow-up summary.
    pub postmortem_summary: String,
}

/// Projects only verified typed model-risk records from an already verified journal.
pub fn model_risk_records(
    records: &[JournalRecord],
) -> Result<Vec<ModelRiskRecord>, OperationsError> {
    let mut projected = records
        .iter()
        .filter(|record| record.event_type == MODEL_RISK_EVENT_TYPE)
        .map(model_risk_record)
        .collect::<Result<Vec<_>, _>>()?;
    projected.sort_by(|left, right| {
        left.strategy_id
            .cmp(&right.strategy_id)
            .then_with(|| left.strategy_version.cmp(&right.strategy_version))
            .then_with(|| left.occurred_at.cmp(&right.occurred_at))
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    Ok(projected)
}

/// Projects only verified typed game-day records from an already verified journal.
pub fn game_day_records(records: &[JournalRecord]) -> Result<Vec<GameDayRecord>, OperationsError> {
    let mut projected = records
        .iter()
        .filter(|record| record.event_type == GAME_DAY_EVENT_TYPE)
        .map(game_day_record)
        .collect::<Result<Vec<_>, _>>()?;
    projected.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.scenario_id.cmp(&right.scenario_id))
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    Ok(projected)
}

fn model_risk_record(record: &JournalRecord) -> Result<ModelRiskRecord, OperationsError> {
    validate_model_risk_details(&record.details)?;
    Ok(ModelRiskRecord {
        record_id: record.entry_id.clone(),
        occurred_at: record.occurred_at.clone(),
        actor: record.actor.clone(),
        strategy_id: required_record_detail(record, "strategy_id")?.to_owned(),
        strategy_version: required_record_detail(record, "strategy_version")?.to_owned(),
        strategy_bundle_hash: required_record_detail(record, "strategy_bundle_hash")?.to_owned(),
        backtest_artifact_hash: required_record_detail(record, "backtest_artifact_hash")?
            .to_owned(),
        decision: ModelRiskDecision::parse(required_record_detail(record, "decision")?)?,
        change_summary: required_record_detail(record, "change_summary")?.to_owned(),
        change_summary_hash: required_record_detail(record, "change_summary_hash")?.to_owned(),
        reason: required_record_detail(record, "reason")?.to_owned(),
    })
}

fn game_day_record(record: &JournalRecord) -> Result<GameDayRecord, OperationsError> {
    validate_game_day_details(&record.details)?;
    Ok(GameDayRecord {
        record_id: record.entry_id.clone(),
        occurred_at: record.occurred_at.clone(),
        actor: record.actor.clone(),
        scenario_id: required_record_detail(record, "scenario_id")?.to_owned(),
        passed: required_record_detail(record, "result")? == "PASS",
        fault_plan_hash: required_record_detail(record, "fault_plan_hash")?.to_owned(),
        evidence_hash: required_record_detail(record, "evidence_hash")?.to_owned(),
        reconciliation_hash: required_record_detail(record, "reconciliation_hash")?.to_owned(),
        postmortem_summary: required_record_detail(record, "postmortem_summary")?.to_owned(),
    })
}

fn required_record_detail<'a>(
    record: &'a JournalRecord,
    key: &str,
) -> Result<&'a str, OperationsError> {
    record
        .details
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            OperationsError(format!("{} lacks required {key} evidence", record.entry_id))
        })
}

/// Renders a portable, read-only operations dashboard contract.
pub fn canonical_dashboard_json(
    snapshot: &OperationsSnapshot,
    journal: &JournalInspection,
) -> Result<String, OperationsError> {
    snapshot.validate()?;
    validate_journal_inspection(journal)?;
    let cockpit = derive_risk_cockpit(snapshot)?;
    let attribution = derive_attribution(snapshot)?;
    let schedules = derive_schedule_statuses(snapshot)?;
    let alerts = derive_alerts_with_journal(snapshot, journal)?;
    let parameter_fingerprint = snapshot.parameters.fingerprint()?;
    let fingerprint = snapshot.fingerprint()?;
    let projection = projection_fingerprint(snapshot, journal)?;
    let positions = canonical_positions_json(&snapshot.positions);
    let limits = cockpit
        .limits
        .iter()
        .map(|limit| {
            format!(
                "{{\"breached\":{},\"current\":\"{}\",\"limit\":\"{}\",\"limit_id\":{}}}",
                limit.breached,
                limit.current,
                limit.limit,
                json_string(&limit.limit_id),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let attribution_rows = attribution
        .rows
        .iter()
        .map(|row| {
            format!(
                "{{\"amount\":\"{}\",\"category\":{},\"instrument_id\":{}}}",
                row.amount,
                json_string(row.category.as_str()),
                json_string(&row.instrument_id),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let attribution_totals = AttributionCategory::all()
        .iter()
        .map(|category| {
            format!(
                "{}:\"{}\"",
                json_string(category.as_str()),
                attribution
                    .totals
                    .get(category)
                    .copied()
                    .unwrap_or(Decimal::ZERO),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let alerts = alerts
        .iter()
        .map(|alert| {
            format!(
                "{{\"alert_id\":{},\"code\":{},\"severity\":{},\"subject\":{},\"summary\":{}}}",
                json_string(&alert.alert_id),
                json_string(&alert.code),
                json_string(alert.severity.as_str()),
                json_string(&alert.subject),
                json_string(&alert.summary),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let schedules = schedules
        .iter()
        .map(|schedule| {
            format!(
                "{{\"due\":{},\"enabled\":{},\"last_completed_at\":{},\"next_due_at\":{},\"purpose\":{},\"schedule_id\":{},\"time_utc\":{}}}",
                schedule.due,
                schedule.enabled,
                optional_json_string(schedule.last_completed_at.as_deref()),
                json_string(&schedule.next_due_at),
                json_string(&schedule.purpose),
                json_string(&schedule.schedule_id),
                json_string(&schedule.time_utc),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"account_id\":{},\"alerts\":[{}],\"as_of\":{},\"attribution\":{{\"net_pnl\":\"{}\",\"rows\":[{}],\"totals\":{{{}}}}},\"configuration\":{{\"configuration_content_hash\":{},\"configuration_id\":{},\"configuration_version\":{},\"fingerprint\":{},\"parameter_set_fingerprint\":{}}},\"currency\":{},\"dashboard_schema_version\":{},\"environment\":{},\"journal\":{{\"failure_reason\":{},\"head_hash\":{},\"healthy\":{},\"sequence\":{}}},\"operational_health\":{{\"active_kill_switches\":{},\"audit_healthy\":{},\"broker_connected\":{},\"reconciliation_healthy\":{},\"unknown_orders\":{},\"unresolved_incidents\":{},\"working_orders\":{}}},\"positions\":{},\"projection_fingerprint\":{},\"reproducibility\":{{\"dataset_hash\":{},\"dataset_id\":{},\"dataset_version\":{},\"replay_event_hash\":{},\"strategy_bundle_hash\":{},\"strategy_id\":{},\"strategy_version\":{}}},\"risk\":{{\"cash\":\"{}\",\"current_equity\":\"{}\",\"drawdown_bps\":\"{}\",\"effective_peak_equity\":\"{}\",\"gross_exposure\":\"{}\",\"largest_position_exposure\":\"{}\",\"limits\":[{}],\"open_positions\":{},\"state\":{}}},\"schedules\":[{}],\"starting_equity\":\"{}\"}}",
        json_string(&snapshot.account_id),
        alerts,
        json_string(&snapshot.as_of),
        attribution.net_pnl,
        attribution_rows,
        attribution_totals,
        json_string(&snapshot.configuration_content_hash),
        json_string(&snapshot.configuration_id),
        json_string(&snapshot.configuration_version),
        json_string(&fingerprint),
        json_string(&parameter_fingerprint),
        json_string(&snapshot.currency),
        OPERATIONS_DASHBOARD_SCHEMA_VERSION,
        json_string(&snapshot.environment),
        optional_json_string(journal.failure_reason.as_deref()),
        json_string(&journal.head_hash),
        journal.healthy,
        journal.sequence,
        string_array_json(&snapshot.health.active_kill_switches),
        snapshot.health.audit_healthy,
        snapshot.health.broker_connected,
        snapshot.health.reconciliation_healthy,
        snapshot.health.unknown_orders,
        snapshot.health.unresolved_incidents,
        snapshot.health.working_orders,
        positions,
        json_string(&projection),
        json_string(&snapshot.reproducibility.dataset_hash),
        json_string(&snapshot.reproducibility.dataset_id),
        json_string(&snapshot.reproducibility.dataset_version),
        json_string(&snapshot.reproducibility.replay_event_hash),
        json_string(&snapshot.reproducibility.strategy_bundle_hash),
        json_string(&snapshot.reproducibility.strategy_id),
        json_string(&snapshot.reproducibility.strategy_version),
        snapshot.cash,
        cockpit.current_equity,
        cockpit.drawdown_bps,
        cockpit.effective_peak_equity,
        cockpit.gross_exposure,
        cockpit.largest_position_exposure,
        limits,
        cockpit.open_positions,
        json_string(&cockpit.state),
        schedules,
        snapshot.starting_equity,
    ))
}

/// Renders a concise deterministic Markdown report from the same immutable view.
pub fn markdown_report(
    snapshot: &OperationsSnapshot,
    journal: &JournalInspection,
) -> Result<String, OperationsError> {
    validate_journal_inspection(journal)?;
    let cockpit = derive_risk_cockpit(snapshot)?;
    let attribution = derive_attribution(snapshot)?;
    let schedules = derive_schedule_statuses(snapshot)?;
    let alerts = derive_alerts_with_journal(snapshot, journal)?;
    let parameter_fingerprint = snapshot.parameters.fingerprint()?;
    let source_fingerprint = snapshot.fingerprint()?;
    let projection = projection_fingerprint(snapshot, journal)?;
    let mut report = format!(
        "# Follon Operations Report\n\n- As of: `{}`\n- Environment: `{}`\n- Account: `{}`\n- Source fingerprint: `{}`\n- Projection fingerprint: `{}`\n- Configuration: `{}` / `{}` (`{}`)\n- Parameter revision fingerprint: `{}`\n- Strategy: `{}` / `{}` (`{}`)\n- Dataset: `{}` / `{}` (`{}`)\n- Replay event hash: `{}`\n\n## Risk cockpit\n\n| Metric | Exact value |\n| --- | ---: |\n| State | {} |\n| Starting equity | {} {} |\n| Current equity | {} {} |\n| Gross exposure | {} {} |\n| Largest position exposure | {} {} |\n| Drawdown | {} bps |\n| Open positions | {} |\n\n| Limit | Current | Limit | Status |\n| --- | ---: | ---: | --- |\n",
        snapshot.as_of,
        snapshot.environment,
        snapshot.account_id,
        source_fingerprint,
        projection,
        snapshot.configuration_id,
        snapshot.configuration_version,
        snapshot.configuration_content_hash,
        parameter_fingerprint,
        snapshot.reproducibility.strategy_id,
        snapshot.reproducibility.strategy_version,
        snapshot.reproducibility.strategy_bundle_hash,
        snapshot.reproducibility.dataset_id,
        snapshot.reproducibility.dataset_version,
        snapshot.reproducibility.dataset_hash,
        snapshot.reproducibility.replay_event_hash,
        cockpit.state,
        snapshot.starting_equity,
        snapshot.currency,
        cockpit.current_equity,
        snapshot.currency,
        cockpit.gross_exposure,
        snapshot.currency,
        cockpit.largest_position_exposure,
        snapshot.currency,
        cockpit.drawdown_bps,
        cockpit.open_positions,
    );
    for limit in &cockpit.limits {
        report.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            limit.limit_id,
            limit.current,
            limit.limit,
            if limit.breached {
                "BREACHED"
            } else {
                "Within limit"
            },
        ));
    }
    report
        .push_str("\n## Attribution\n\n| Instrument | Category | Amount |\n| --- | --- | ---: |\n");
    for row in &attribution.rows {
        report.push_str(&format!(
            "| {} | {} | {} {} |\n",
            row.instrument_id,
            row.category.as_str(),
            row.amount,
            snapshot.currency,
        ));
    }
    report.push_str(&format!(
        "\nNet attributed P&L: **{} {}**.\n\n## Alerts\n\n",
        attribution.net_pnl, snapshot.currency
    ));
    if alerts.is_empty() {
        report.push_str("No active deterministic alerts.\n");
    } else {
        for alert in &alerts {
            report.push_str(&format!(
                "- {} `{}` on `{}` — {}\n",
                alert.severity.as_str(),
                alert.code,
                alert.subject,
                markdown_text(&alert.summary),
            ));
        }
    }
    report.push_str(
        "\n## Schedule\n\n| Schedule | Next due | State | Purpose |\n| --- | --- | --- | --- |\n",
    );
    for schedule in &schedules {
        report.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            schedule.schedule_id,
            schedule.next_due_at,
            if !schedule.enabled {
                "Disabled"
            } else if schedule.due {
                "Due"
            } else {
                "Scheduled"
            },
            markdown_text(&schedule.purpose),
        ));
    }
    report.push_str(&format!(
        "\n## Operations journal\n\n- Healthy: {}\n- Verified sequence: {}\n- Verified head hash: `{}`\n",
        if journal.healthy { "Yes" } else { "No" },
        journal.sequence,
        journal.head_hash,
    ));
    if let Some(reason) = &journal.failure_reason {
        report.push_str(&format!(
            "- Verification failure: {}\n",
            markdown_text(reason)
        ));
    }
    Ok(report)
}

impl AttributionCategory {
    fn all() -> [Self; 5] {
        [
            Self::RealizedPnl,
            Self::UnrealizedPnl,
            Self::Fees,
            Self::Dividends,
            Self::CorporateActions,
        ]
    }
}

fn usage(limit_id: &str, current: Decimal, limit: Decimal) -> RiskLimitUsage {
    RiskLimitUsage {
        limit_id: limit_id.to_owned(),
        current,
        limit,
        breached: current > limit,
    }
}

fn count_decimal(value: usize) -> Result<Decimal, OperationsError> {
    Decimal::from_integer(
        i64::try_from(value)
            .map_err(|_| OperationsError("count cannot fit exact decimal".to_owned()))?,
    )
    .map_err(Into::into)
}

fn absolute(value: Decimal) -> Result<Decimal, OperationsError> {
    if value < Decimal::ZERO {
        Decimal::ZERO.checked_sub(value).map_err(Into::into)
    } else {
        Ok(value)
    }
}

fn alert(severity: AlertSeverity, code: &str, subject: &str, summary: &str) -> OperationalAlert {
    OperationalAlert {
        alert_id: sha256(&format!("{}\n{}\n{}", severity.as_str(), code, subject)),
        severity,
        code: code.to_owned(),
        subject: subject.to_owned(),
        summary: summary.to_owned(),
    }
}

fn validate_sha256(name: &str, value: &str) -> Result<(), OperationsError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(OperationsError(format!(
            "{name} must be a lowercase SHA-256 hash"
        )));
    }
    Ok(())
}

fn parse_utc(value: &str) -> Result<OffsetDateTime, OperationsError> {
    validate_utc_timestamp("UTC time", value)?;
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| OperationsError("invalid UTC time".to_owned()))
}

fn format_utc(value: OffsetDateTime) -> Result<String, OperationsError> {
    value
        .format(&Rfc3339)
        .map_err(|_| OperationsError("cannot format UTC time".to_owned()))
}

fn parse_schedule_time(value: &str) -> Result<(u8, u8), OperationsError> {
    if value.len() != 5 || value.as_bytes()[2] != b':' {
        return Err(OperationsError(
            "schedule time must use HH:MM UTC".to_owned(),
        ));
    }
    let hour = value[..2]
        .parse::<u8>()
        .map_err(|_| OperationsError("schedule hour is invalid".to_owned()))?;
    let minute = value[3..]
        .parse::<u8>()
        .map_err(|_| OperationsError("schedule minute is invalid".to_owned()))?;
    if hour > 23
        || minute > 59
        || !value
            .as_bytes()
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
    {
        return Err(OperationsError(
            "schedule time must use HH:MM UTC".to_owned(),
        ));
    }
    Ok((hour, minute))
}

fn scheduled_instant_for(
    schedule: &DailySchedule,
    occurrence: OffsetDateTime,
) -> Result<OffsetDateTime, OperationsError> {
    let (hour, minute) = parse_schedule_time(&schedule.time_utc)?;
    let time = Time::from_hms(hour, minute, 0)
        .map_err(|_| OperationsError("invalid schedule time".to_owned()))?;
    Ok(occurrence.replace_time(time))
}

fn validate_schedule_completion_time(
    schedule: &DailySchedule,
    completed_at: &str,
) -> Result<(), OperationsError> {
    let completed_at = parse_utc(completed_at)?;
    let scheduled_for = scheduled_instant_for(schedule, completed_at)?;
    if completed_at < scheduled_for {
        return Err(OperationsError(format!(
            "schedule {} cannot complete before its daily UTC due instant",
            schedule.schedule_id
        )));
    }
    Ok(())
}

fn canonical_positions_json(positions: &[OperationalPosition]) -> String {
    let mut positions = positions.to_vec();
    positions.sort_by(|left, right| left.instrument_id.cmp(&right.instrument_id));
    format!(
        "[{}]",
        positions
            .iter()
            .map(|position| format!(
                "{{\"average_cost\":\"{}\",\"instrument_id\":{},\"mark_price\":\"{}\",\"quantity\":\"{}\",\"realized_pnl\":\"{}\"}}",
                position.average_cost,
                json_string(&position.instrument_id),
                position.mark_price,
                position.quantity,
                position.realized_pnl,
            ))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn string_array_json(values: &[String]) -> String {
    let mut values = values.to_vec();
    values.sort();
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

fn optional_json_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_owned())
}

fn details_json(details: &BTreeMap<String, String>) -> String {
    format!(
        "{{{}}}",
        details
            .iter()
            .map(|(key, value)| format!("{}:{}", json_string(key), json_string(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn canonical_journal_body(
    sequence: u64,
    entry_id: &str,
    event_type: &str,
    occurred_at: &str,
    actor: &str,
    details: &BTreeMap<String, String>,
    prev_hash: &str,
) -> String {
    format!(
        "actor={}\ndetails={}\nentry_id={}\nevent_type={}\njournal_schema_version={}\noccurred_at={}\nprev_hash={}\nsequence={}\n",
        actor,
        details_json(details),
        entry_id,
        event_type,
        OPERATIONS_JOURNAL_SCHEMA_VERSION,
        occurred_at,
        prev_hash,
        sequence,
    )
}

fn validate_journal_input(input: &JournalEntryInput) -> Result<(), OperationsError> {
    validate_canonical_id("journal entry_id", &input.entry_id)?;
    validate_canonical_id("journal actor", &input.actor)?;
    validate_utc_timestamp("journal occurred_at", &input.occurred_at)?;
    if !is_event_type(&input.event_type) {
        return Err(OperationsError(
            "journal event_type must be a lowercase dotted vN contract type".to_owned(),
        ));
    }
    if input.details.len() > 32 {
        return Err(OperationsError(
            "journal details support at most 32 non-secret fields".to_owned(),
        ));
    }
    for (key, value) in &input.details {
        validate_canonical_id("journal detail key", key)?;
        if key.contains("secret")
            || key.contains("credential")
            || key.contains("password")
            || key.contains("token")
            || key.ends_with("_key")
            || value.len() > 512
            || value.contains(['\r', '\n'])
        {
            return Err(OperationsError(
                "journal details must be concise, one-line, and must not be credential-like"
                    .to_owned(),
            ));
        }
    }
    if input.event_type == SCHEDULE_COMPLETION_EVENT_TYPE {
        validate_schedule_completion_input(input)?;
    }
    if input.event_type == MODEL_RISK_EVENT_TYPE {
        validate_model_risk_details(&input.details)?;
    }
    if input.event_type == GAME_DAY_EVENT_TYPE {
        validate_game_day_details(&input.details)?;
    }
    Ok(())
}

fn validate_schedule_completion_input(input: &JournalEntryInput) -> Result<(), OperationsError> {
    let expected_keys = [
        "configuration_hash",
        "parameter_set_fingerprint",
        "schedule_id",
        "scheduled_for",
    ];
    if input.details.len() != expected_keys.len()
        || input
            .details
            .keys()
            .any(|key| !expected_keys.contains(&key.as_str()))
    {
        return Err(OperationsError(
            "schedule completion must carry exactly configuration_hash, parameter_set_fingerprint, schedule_id, and scheduled_for"
                .to_owned(),
        ));
    }
    let schedule_id = input
        .details
        .get("schedule_id")
        .ok_or_else(|| OperationsError("schedule completion lacks schedule_id".to_owned()))?;
    validate_canonical_id("schedule completion schedule_id", schedule_id)?;
    let configuration_hash = input.details.get("configuration_hash").ok_or_else(|| {
        OperationsError("schedule completion lacks configuration_hash".to_owned())
    })?;
    validate_sha256("schedule completion configuration_hash", configuration_hash)?;
    let parameter_set_fingerprint =
        input
            .details
            .get("parameter_set_fingerprint")
            .ok_or_else(|| {
                OperationsError("schedule completion lacks parameter_set_fingerprint".to_owned())
            })?;
    validate_sha256(
        "schedule completion parameter_set_fingerprint",
        parameter_set_fingerprint,
    )?;
    let scheduled_for = input
        .details
        .get("scheduled_for")
        .ok_or_else(|| OperationsError("schedule completion lacks scheduled_for".to_owned()))?;
    let scheduled_for = parse_utc(scheduled_for)?;
    if parse_utc(&input.occurred_at)? < scheduled_for {
        return Err(OperationsError(
            "schedule completion cannot occur before its declared scheduled_for instant".to_owned(),
        ));
    }
    Ok(())
}

fn validate_model_risk_details(details: &BTreeMap<String, String>) -> Result<(), OperationsError> {
    let expected_keys = [
        "backtest_artifact_hash",
        "change_summary",
        "change_summary_hash",
        "decision",
        "reason",
        "strategy_bundle_hash",
        "strategy_id",
        "strategy_version",
    ];
    validate_exact_detail_keys(details, &expected_keys, "model-risk record")?;
    validate_canonical_id(
        "model-risk strategy_id",
        required_detail(details, "strategy_id", "model-risk record")?,
    )?;
    let strategy_version = required_detail(details, "strategy_version", "model-risk record")?;
    if strategy_version.len() > 128 || strategy_version.contains(['\r', '\n']) {
        return Err(OperationsError(
            "model-risk strategy_version must be a concise one-line value".to_owned(),
        ));
    }
    for key in [
        "strategy_bundle_hash",
        "backtest_artifact_hash",
        "change_summary_hash",
    ] {
        validate_sha256(
            &format!("model-risk {key}"),
            required_detail(details, key, "model-risk record")?,
        )?;
    }
    ModelRiskDecision::parse(required_detail(details, "decision", "model-risk record")?)?;
    let change_summary = required_detail(details, "change_summary", "model-risk record")?;
    let reason = required_detail(details, "reason", "model-risk record")?;
    if change_summary.len() > 512
        || reason.len() > 512
        || change_summary.contains(['\r', '\n'])
        || reason.contains(['\r', '\n'])
        || sha256(change_summary)
            != required_detail(details, "change_summary_hash", "model-risk record")?
    {
        return Err(OperationsError(
            "model-risk record has invalid change-summary or reason evidence".to_owned(),
        ));
    }
    Ok(())
}

fn validate_game_day_details(details: &BTreeMap<String, String>) -> Result<(), OperationsError> {
    let expected_keys = [
        "evidence_hash",
        "fault_plan_hash",
        "postmortem_summary",
        "reconciliation_hash",
        "result",
        "scenario_id",
    ];
    validate_exact_detail_keys(details, &expected_keys, "game-day record")?;
    validate_canonical_id(
        "game-day scenario_id",
        required_detail(details, "scenario_id", "game-day record")?,
    )?;
    if !matches!(
        required_detail(details, "result", "game-day record")?,
        "PASS" | "FAIL"
    ) {
        return Err(OperationsError(
            "game-day result must be PASS or FAIL".to_owned(),
        ));
    }
    for key in ["fault_plan_hash", "evidence_hash", "reconciliation_hash"] {
        validate_sha256(
            &format!("game-day {key}"),
            required_detail(details, key, "game-day record")?,
        )?;
    }
    let summary = required_detail(details, "postmortem_summary", "game-day record")?;
    if summary.len() > 512 || summary.contains(['\r', '\n']) {
        return Err(OperationsError(
            "game-day postmortem_summary must be concise and one-line".to_owned(),
        ));
    }
    Ok(())
}

fn validate_exact_detail_keys(
    details: &BTreeMap<String, String>,
    expected_keys: &[&str],
    evidence_name: &str,
) -> Result<(), OperationsError> {
    if details.len() != expected_keys.len()
        || details
            .keys()
            .any(|key| !expected_keys.contains(&key.as_str()))
    {
        return Err(OperationsError(format!(
            "{evidence_name} has an invalid evidence shape"
        )));
    }
    Ok(())
}

fn required_detail<'a>(
    details: &'a BTreeMap<String, String>,
    key: &str,
    evidence_name: &str,
) -> Result<&'a str, OperationsError> {
    details
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OperationsError(format!("{evidence_name} lacks {key}")))
}

fn is_event_type(value: &str) -> bool {
    let mut components = value.split('.').peekable();
    let mut count = 0usize;
    while let Some(component) = components.next() {
        count += 1;
        if components.peek().is_none() {
            return count >= 2
                && component.len() >= 2
                && component.starts_with('v')
                && component[1..].bytes().all(|byte| byte.is_ascii_digit());
        }
        if component.is_empty()
            || !component
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        {
            return false;
        }
    }
    false
}

fn verify_journal_file(
    file: &mut File,
) -> Result<(JournalInspection, BTreeSet<String>, Vec<JournalRecord>), OperationsError> {
    let length = file.metadata().map_err(io_error)?.len();
    if length > MAX_JOURNAL_BYTES {
        return Err(OperationsError(
            "operations journal exceeds its configured 128 MiB safety limit".to_owned(),
        ));
    }
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut content = String::new();
    file.read_to_string(&mut content).map_err(io_error)?;
    if content.is_empty() {
        return Ok((JournalInspection::empty(), BTreeSet::new(), Vec::new()));
    }
    if !content.ends_with('\n') {
        return Err(OperationsError(
            "operations journal must end on a complete newline-delimited record".to_owned(),
        ));
    }
    let mut inspection = JournalInspection::empty();
    let mut entry_ids = BTreeSet::new();
    let mut records = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        if line.is_empty() {
            return Err(OperationsError(format!(
                "operations journal contains an empty line at {}",
                line_number + 1
            )));
        }
        let persisted: PersistedJournalRecord = serde_json::from_str(line).map_err(|_| {
            OperationsError(format!(
                "operations journal line {} is not valid JSON",
                line_number + 1
            ))
        })?;
        let record = persisted.into_record()?;
        let expected_sequence = inspection
            .sequence
            .checked_add(1)
            .ok_or_else(|| OperationsError("operations journal sequence overflow".to_owned()))?;
        if record.sequence != expected_sequence
            || record.prev_hash != inspection.head_hash
            || !entry_ids.insert(record.entry_id.clone())
        {
            return Err(OperationsError(format!(
                "operations journal chain failed at line {}",
                line_number + 1
            )));
        }
        let expected_hash = sha256(&canonical_journal_body(
            record.sequence,
            &record.entry_id,
            &record.event_type,
            &record.occurred_at,
            &record.actor,
            &record.details,
            &record.prev_hash,
        ));
        if record.record_hash != expected_hash {
            return Err(OperationsError(format!(
                "operations journal hash failed at line {}",
                line_number + 1
            )));
        }
        inspection.sequence = record.sequence;
        inspection.head_hash = record.record_hash.clone();
        records.push(record);
    }
    Ok((inspection, entry_ids, records))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedJournalRecord {
    actor: String,
    details: BTreeMap<String, String>,
    entry_id: String,
    event_type: String,
    journal_schema_version: u32,
    occurred_at: String,
    prev_hash: String,
    record_hash: String,
    sequence: u64,
}

impl PersistedJournalRecord {
    fn into_record(self) -> Result<JournalRecord, OperationsError> {
        if self.journal_schema_version != OPERATIONS_JOURNAL_SCHEMA_VERSION {
            return Err(OperationsError(
                "unsupported operations journal schema version".to_owned(),
            ));
        }
        validate_journal_input(&JournalEntryInput {
            entry_id: self.entry_id.clone(),
            event_type: self.event_type.clone(),
            occurred_at: self.occurred_at.clone(),
            actor: self.actor.clone(),
            details: self.details.clone(),
        })?;
        validate_sha256("journal prev_hash", &self.prev_hash)?;
        validate_sha256("journal record_hash", &self.record_hash)?;
        if self.sequence == 0 {
            return Err(OperationsError(
                "operations journal sequence must start at one".to_owned(),
            ));
        }
        Ok(JournalRecord {
            journal_schema_version: self.journal_schema_version,
            sequence: self.sequence,
            entry_id: self.entry_id,
            event_type: self.event_type,
            occurred_at: self.occurred_at,
            actor: self.actor,
            details: self.details,
            prev_hash: self.prev_hash,
            record_hash: self.record_hash,
        })
    }
}

fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn markdown_text(value: &str) -> String {
    value.replace('\\', "\\\\").replace('|', "\\|")
}

fn io_error(error: std::io::Error) -> OperationsError {
    OperationsError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn decimal(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn approved_risk_parameter(
        parameter_id: &str,
        value: &str,
        minimum: &str,
        maximum: &str,
    ) -> ParameterValue {
        ParameterValue {
            parameter_id: parameter_id.to_owned(),
            value: decimal(value),
            minimum: decimal(minimum),
            maximum: decimal(maximum),
            control: ParameterControl::TwoPerson,
            approval: Some(ParameterApproval {
                approval_id: "approval.risk-limits.7".to_owned(),
                requested_by: "operator.alice".to_owned(),
                approved_by: "operator.bob".to_owned(),
                approved_at: "2026-08-10T15:00:00Z".to_owned(),
                approval_subject_hash: String::new(),
                authorization_policy_hash: risk_limit_authorization_policy_fingerprint(),
                approval_evidence_hash: "f".repeat(64),
            }),
        }
    }

    fn snapshot() -> OperationsSnapshot {
        let mut snapshot = OperationsSnapshot {
            as_of: "2026-08-10T16:30:00Z".to_owned(),
            environment: "PAPER".to_owned(),
            account_id: "acct.paper.001".to_owned(),
            currency: "USD".to_owned(),
            configuration_id: "config.ops.001".to_owned(),
            configuration_version: "2026.08.10.1".to_owned(),
            configuration_content_hash: "a".repeat(64),
            reproducibility: ReproducibilityStamp {
                strategy_id: "strategy.mean_revert".to_owned(),
                strategy_version: "1.2.0".to_owned(),
                strategy_bundle_hash: "b".repeat(64),
                dataset_id: "dataset.us_equities".to_owned(),
                dataset_version: "2026.08.10".to_owned(),
                dataset_hash: "c".repeat(64),
                replay_event_hash: "d".repeat(64),
            },
            parameters: ParameterSet {
                parameter_set_id: "params.mean_revert".to_owned(),
                revision: "7".to_owned(),
                previous_revision: None,
                previous_parameter_set_fingerprint: None,
                values: vec![
                    ParameterValue {
                        parameter_id: "entry_zscore".to_owned(),
                        value: decimal("2.0"),
                        minimum: decimal("1.0"),
                        maximum: decimal("3.0"),
                        control: ParameterControl::Standard,
                        approval: None,
                    },
                    approved_risk_parameter("risk.max_gross_exposure", "6000", "100", "10000"),
                    approved_risk_parameter(
                        "risk.max_single_instrument_exposure",
                        "6000",
                        "100",
                        "10000",
                    ),
                    approved_risk_parameter("risk.max_drawdown_bps", "1000", "100", "5000"),
                    approved_risk_parameter("risk.max_working_orders", "5", "1", "100"),
                    approved_risk_parameter("risk.max_unknown_orders", "0", "0", "100"),
                    approved_risk_parameter("risk.max_unresolved_incidents", "0", "0", "100"),
                ],
            },
            starting_equity: decimal("10000"),
            cash: decimal("4500"),
            peak_equity: decimal("10200"),
            positions: vec![OperationalPosition {
                instrument_id: "inst.us_equity.spy".to_owned(),
                quantity: decimal("50"),
                mark_price: decimal("110"),
                average_cost: decimal("100"),
                realized_pnl: decimal("50"),
            }],
            risk_limits: RiskLimits {
                max_gross_exposure: decimal("6000"),
                max_single_instrument_exposure: decimal("6000"),
                max_drawdown_bps: decimal("1000"),
                max_working_orders: 5,
                max_unknown_orders: 0,
                max_unresolved_incidents: 0,
            },
            health: OperationalHealth {
                audit_healthy: true,
                reconciliation_healthy: true,
                broker_connected: true,
                active_kill_switches: Vec::new(),
                working_orders: 1,
                unknown_orders: 0,
                unresolved_incidents: 0,
            },
            attribution_entries: vec![
                AttributionEntry {
                    entry_id: "entry.1".to_owned(),
                    occurred_at: "2026-08-10T15:00:00Z".to_owned(),
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    category: AttributionCategory::RealizedPnl,
                    amount: decimal("50"),
                },
                AttributionEntry {
                    entry_id: "entry.2".to_owned(),
                    occurred_at: "2026-08-10T16:00:00Z".to_owned(),
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    category: AttributionCategory::UnrealizedPnl,
                    amount: decimal("500"),
                },
                AttributionEntry {
                    entry_id: "entry.3".to_owned(),
                    occurred_at: "2026-08-10T16:00:00Z".to_owned(),
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    category: AttributionCategory::Fees,
                    amount: decimal("-10"),
                },
            ],
            schedules: vec![DailySchedule {
                schedule_id: "schedule.reconcile".to_owned(),
                purpose: "Reconcile paper account".to_owned(),
                time_utc: "16:00".to_owned(),
                enabled: true,
                last_completed_at: None,
            }],
        };
        let approval_subject_hash = snapshot.parameters.approval_subject_fingerprint().unwrap();
        for value in &mut snapshot.parameters.values {
            if let Some(approval) = &mut value.approval {
                approval.approval_subject_hash = approval_subject_hash.clone();
            }
        }
        snapshot
    }

    #[test]
    fn dashboard_and_report_are_repeatable_and_expose_due_work() {
        let input = snapshot();
        let journal = JournalInspection::empty();
        let first = canonical_dashboard_json(&input, &journal).unwrap();
        let second = canonical_dashboard_json(&input, &journal).unwrap();
        assert_eq!(first, second);
        assert!(first.contains("scheduled_work_due"));
        assert!(markdown_report(&input, &journal)
            .unwrap()
            .contains("Net attributed P&L: **540.00000000 USD**"));
        let cockpit = derive_risk_cockpit(&input).unwrap();
        assert_eq!(cockpit.current_equity, decimal("10000"));
        assert_eq!(cockpit.drawdown_bps, decimal("196.07843137"));
    }

    #[test]
    fn parameter_two_person_control_is_enforced() {
        let mut input = snapshot();
        input.parameters.values[1]
            .approval
            .as_mut()
            .unwrap()
            .approved_by = "operator.alice".to_owned();
        assert!(input.validate().is_err());
    }

    #[test]
    fn reserved_risk_limits_are_approved_and_cannot_drift() {
        let mut drifted = snapshot();
        drifted.risk_limits.max_gross_exposure = decimal("6001");
        assert!(drifted
            .validate()
            .unwrap_err()
            .to_string()
            .contains("does not match its enforced risk limit"));

        let mut downgraded = snapshot();
        let parameter = downgraded
            .parameters
            .values
            .iter_mut()
            .find(|value| value.parameter_id == "risk.max_gross_exposure")
            .unwrap();
        parameter.control = ParameterControl::Standard;
        parameter.approval = None;
        let subject_hash = downgraded
            .parameters
            .approval_subject_fingerprint()
            .unwrap();
        for value in &mut downgraded.parameters.values {
            if let Some(approval) = &mut value.approval {
                approval.approval_subject_hash = subject_hash.clone();
            }
        }
        assert!(downgraded
            .validate()
            .unwrap_err()
            .to_string()
            .contains("must require TWO_PERSON control"));
    }

    #[test]
    fn parameter_approvals_cannot_be_from_the_future() {
        let mut input = snapshot();
        input.parameters.values[1]
            .approval
            .as_mut()
            .unwrap()
            .approved_at = "2026-08-10T16:31:00Z".to_owned();
        assert!(input
            .validate()
            .unwrap_err()
            .to_string()
            .contains("approval cannot be after snapshot as_of"));
    }

    #[test]
    fn parameter_revision_labels_are_canonical_hash_tokens() {
        let mut input = snapshot().parameters;
        input.revision = "8\nprevious_revision=7".to_owned();
        assert!(input.validate().is_err());
    }

    #[test]
    fn parameter_revision_diff_is_canonical_and_includes_control_evidence() {
        let previous = snapshot().parameters;
        let mut target = previous.clone();
        target.revision = "8".to_owned();
        target.previous_revision = Some("7".to_owned());
        target.previous_parameter_set_fingerprint = Some(previous.fingerprint().unwrap());
        target.values[0].value = decimal("2.25");
        let target_subject_hash = target.approval_subject_fingerprint().unwrap();
        for value in &mut target.values {
            if let Some(approval) = &mut value.approval {
                approval.approval_subject_hash = target_subject_hash.clone();
            }
        }
        let changes = target.diff_from(&previous).unwrap();
        assert_eq!(changes.len(), 7);
        assert_eq!(changes[0].parameter_id, "entry_zscore");
        assert_eq!(changes[0].kind, ParameterChangeKind::Modified);
        assert_eq!(changes[0].before.as_ref().unwrap().value, decimal("2.0"));
        assert_eq!(changes[0].after.as_ref().unwrap().value, decimal("2.25"));
        assert_eq!(changes[1].parameter_id, "risk.max_drawdown_bps");
        assert_eq!(changes[1].kind, ParameterChangeKind::Modified);
        assert!(previous.diff_from(&previous).is_err());
        let mut non_successor = target.clone();
        non_successor.previous_revision = Some("6".to_owned());
        assert!(non_successor.diff_from(&previous).is_err());
        let mut wrong_parent = target.clone();
        wrong_parent.previous_parameter_set_fingerprint = Some("0".repeat(64));
        let wrong_parent_subject_hash = wrong_parent.approval_subject_fingerprint().unwrap();
        for value in &mut wrong_parent.values {
            if let Some(approval) = &mut value.approval {
                approval.approval_subject_hash = wrong_parent_subject_hash.clone();
            }
        }
        assert!(wrong_parent.diff_from(&previous).is_err());
    }

    #[test]
    fn journal_is_fsynced_and_tamper_evident() {
        let path = std::env::temp_dir().join(format!(
            "follon-operations-journal-{}-{}.ndjson",
            std::process::id(),
            "hash-chain"
        ));
        let _ = fs::remove_file(&path);
        let mut journal = OperationalJournal::open(&path).unwrap();
        let record = journal
            .append(JournalEntryInput {
                entry_id: "journal.entry.1".to_owned(),
                event_type: "operations.report_generated.v1".to_owned(),
                occurred_at: "2026-08-10T16:30:00Z".to_owned(),
                actor: "operator.alice".to_owned(),
                details: BTreeMap::from([("report_hash".to_owned(), "a".repeat(64))]),
            })
            .unwrap();
        assert_eq!(record.sequence, 1);
        assert_eq!(journal.inspection().sequence, 1);
        assert!(journal
            .append(JournalEntryInput {
                entry_id: "journal.entry.1".to_owned(),
                event_type: "operations.report_generated.v1".to_owned(),
                occurred_at: "2026-08-10T16:30:00Z".to_owned(),
                actor: "operator.alice".to_owned(),
                details: BTreeMap::new(),
            })
            .is_err());
        drop(journal);
        assert_eq!(OperationalJournal::inspect(&path).unwrap().sequence, 1);
        let mut content = fs::read_to_string(&path).unwrap();
        content = content.replace("report_generated", "report_edited");
        fs::write(&path, content).unwrap();
        assert!(OperationalJournal::inspect(&path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn model_risk_and_game_day_records_are_typed_and_append_only() {
        let path = std::env::temp_dir().join(format!(
            "follon-operations-governance-{}-{}.ndjson",
            std::process::id(),
            "typed-records"
        ));
        let _ = fs::remove_file(&path);
        let change_summary = "Adjusted entry threshold after reproducible replay review";
        let mut journal = OperationalJournal::open(&path).unwrap();
        journal
            .append(JournalEntryInput {
                entry_id: "model.risk.001".to_owned(),
                event_type: MODEL_RISK_EVENT_TYPE.to_owned(),
                occurred_at: "2026-08-10T17:00:00Z".to_owned(),
                actor: "operator.alice".to_owned(),
                details: BTreeMap::from([
                    ("strategy_id".to_owned(), "strategy.mean_revert".to_owned()),
                    ("strategy_version".to_owned(), "1.2.1".to_owned()),
                    ("strategy_bundle_hash".to_owned(), "a".repeat(64)),
                    ("backtest_artifact_hash".to_owned(), "b".repeat(64)),
                    ("decision".to_owned(), "HOLD".to_owned()),
                    ("change_summary".to_owned(), change_summary.to_owned()),
                    ("change_summary_hash".to_owned(), sha256(change_summary)),
                    (
                        "reason".to_owned(),
                        "Awaiting the next independently reconciled PAPER session.".to_owned(),
                    ),
                ]),
            })
            .unwrap();
        journal
            .append(JournalEntryInput {
                entry_id: "game.day.001".to_owned(),
                event_type: GAME_DAY_EVENT_TYPE.to_owned(),
                occurred_at: "2026-08-10T18:00:00Z".to_owned(),
                actor: "operator.alice".to_owned(),
                details: BTreeMap::from([
                    (
                        "scenario_id".to_owned(),
                        "game_day.reconnect.001".to_owned(),
                    ),
                    ("result".to_owned(), "PASS".to_owned()),
                    ("fault_plan_hash".to_owned(), "c".repeat(64)),
                    ("evidence_hash".to_owned(), "d".repeat(64)),
                    ("reconciliation_hash".to_owned(), "e".repeat(64)),
                    (
                        "postmortem_summary".to_owned(),
                        "Connection loss recovered with a clean independent reconciliation."
                            .to_owned(),
                    ),
                ]),
            })
            .unwrap();
        assert!(journal
            .append(JournalEntryInput {
                entry_id: "model.risk.invalid".to_owned(),
                event_type: MODEL_RISK_EVENT_TYPE.to_owned(),
                occurred_at: "2026-08-10T19:00:00Z".to_owned(),
                actor: "operator.alice".to_owned(),
                details: BTreeMap::from([(
                    "strategy_id".to_owned(),
                    "strategy.mean_revert".to_owned(),
                )]),
            })
            .is_err());
        drop(journal);
        let records = OperationalJournal::read_verified_records(&path).unwrap();
        let model_risk = model_risk_records(&records).unwrap();
        let game_days = game_day_records(&records).unwrap();
        assert_eq!(model_risk.len(), 1);
        assert_eq!(model_risk[0].decision, ModelRiskDecision::Hold);
        assert_eq!(game_days.len(), 1);
        assert!(game_days[0].passed);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn completed_daily_schedule_is_not_due_again_until_tomorrow() {
        let mut input = snapshot();
        input.schedules[0].last_completed_at = Some("2026-08-10T16:05:00Z".to_owned());
        let schedule = derive_schedule_statuses(&input).unwrap().remove(0);
        assert!(!schedule.due);
        assert_eq!(schedule.next_due_at, "2026-08-11T16:00:00Z");
    }

    #[test]
    fn pre_due_schedule_completion_fails_closed() {
        let input = snapshot();
        let path = std::env::temp_dir().join(format!(
            "follon-operations-predue-{}-{}.ndjson",
            std::process::id(),
            "completion"
        ));
        let _ = fs::remove_file(&path);
        let mut journal = OperationalJournal::open(&path).unwrap();
        assert!(journal
            .append(JournalEntryInput {
                entry_id: "journal.schedule.predue.1".to_owned(),
                event_type: SCHEDULE_COMPLETION_EVENT_TYPE.to_owned(),
                occurred_at: "2026-08-10T15:59:00Z".to_owned(),
                actor: "operator.alice".to_owned(),
                details: BTreeMap::from([
                    ("schedule_id".to_owned(), "schedule.reconcile".to_owned()),
                    (
                        "configuration_hash".to_owned(),
                        input.configuration_content_hash.clone(),
                    ),
                    (
                        "parameter_set_fingerprint".to_owned(),
                        input.parameters.fingerprint().unwrap(),
                    ),
                    (
                        "scheduled_for".to_owned(),
                        "2026-08-10T16:00:00Z".to_owned(),
                    ),
                ]),
            })
            .is_err());
        drop(journal);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn verified_configuration_bound_completion_suppresses_due_schedule() {
        let input = snapshot();
        let path = std::env::temp_dir().join(format!(
            "follon-operations-schedule-{}-{}.ndjson",
            std::process::id(),
            "completion"
        ));
        let _ = fs::remove_file(&path);
        let mut journal = OperationalJournal::open(&path).unwrap();
        journal
            .append(JournalEntryInput {
                entry_id: "journal.schedule_complete.1".to_owned(),
                event_type: SCHEDULE_COMPLETION_EVENT_TYPE.to_owned(),
                occurred_at: "2026-08-10T16:05:00Z".to_owned(),
                actor: "operator.alice".to_owned(),
                details: BTreeMap::from([
                    ("schedule_id".to_owned(), "schedule.reconcile".to_owned()),
                    (
                        "configuration_hash".to_owned(),
                        input.configuration_content_hash.clone(),
                    ),
                    (
                        "parameter_set_fingerprint".to_owned(),
                        input.parameters.fingerprint().unwrap(),
                    ),
                    (
                        "scheduled_for".to_owned(),
                        "2026-08-10T16:00:00Z".to_owned(),
                    ),
                ]),
            })
            .unwrap();
        drop(journal);
        let records = OperationalJournal::read_verified_records(&path).unwrap();
        let projected = apply_schedule_completions(&input, &records).unwrap();
        let schedule = derive_schedule_statuses(&projected).unwrap().remove(0);
        assert_eq!(
            schedule.last_completed_at.as_deref(),
            Some("2026-08-10T16:05:00Z")
        );
        assert!(!schedule.due);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn completion_from_another_configuration_is_ignored_after_a_normal_rollover() {
        let input = snapshot();
        let record = JournalRecord {
            journal_schema_version: OPERATIONS_JOURNAL_SCHEMA_VERSION,
            sequence: 1,
            entry_id: "journal.schedule_complete.wrong_config".to_owned(),
            event_type: SCHEDULE_COMPLETION_EVENT_TYPE.to_owned(),
            occurred_at: "2026-08-10T16:05:00Z".to_owned(),
            actor: "operator.alice".to_owned(),
            details: BTreeMap::from([
                ("schedule_id".to_owned(), "schedule.reconcile".to_owned()),
                ("configuration_hash".to_owned(), "e".repeat(64)),
                (
                    "parameter_set_fingerprint".to_owned(),
                    input.parameters.fingerprint().unwrap(),
                ),
                (
                    "scheduled_for".to_owned(),
                    "2026-08-10T16:00:00Z".to_owned(),
                ),
            ]),
            prev_hash: EMPTY_JOURNAL_HASH.to_owned(),
            record_hash: "f".repeat(64),
        };
        let projected = apply_schedule_completions(&input, &[record]).unwrap();
        let schedule = derive_schedule_statuses(&projected).unwrap().remove(0);
        assert!(schedule.due);
        assert_eq!(schedule.last_completed_at, None);
    }

    #[test]
    fn unhealthy_journal_is_a_critical_dashboard_alert_and_changes_projection_identity() {
        let input = snapshot();
        let healthy = JournalInspection::empty();
        let unhealthy = JournalInspection::unhealthy("hash chain failed");
        let alerts = derive_alerts_with_journal(&input, &unhealthy).unwrap();
        assert!(alerts.iter().any(|alert| {
            alert.code == "operations_journal_unhealthy"
                && alert.severity == AlertSeverity::Critical
        }));
        assert_ne!(
            projection_fingerprint(&input, &healthy).unwrap(),
            projection_fingerprint(&input, &unhealthy).unwrap()
        );
        assert!(canonical_dashboard_json(&input, &unhealthy)
            .unwrap()
            .contains("operations_journal_unhealthy"));
    }
}
