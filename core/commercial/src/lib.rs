//! Commercial, privacy, release, and self-hosting control primitives.
//!
//! This crate deliberately does not collect card data, call a payment provider,
//! host identity, or transmit customer information. It provides deterministic,
//! locally auditable boundaries for externally evidenced subscription state,
//! entitlement derivation, data-retention execution, release signing, and
//! self-hosted deployment readiness.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use follon_domain::{validate_canonical_id, validate_utc_timestamp};
use fs2::FileExt;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Schema version for append-only commercial-ledger records.
pub const COMMERCIAL_LEDGER_SCHEMA_VERSION: u32 = 1;
/// Schema version for retention and privacy deletion plans.
pub const RETENTION_PLAN_SCHEMA_VERSION: u32 = 1;
/// Schema version for signed-release manifests.
pub const RELEASE_MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Schema version for detached release signatures.
pub const RELEASE_SIGNATURE_SCHEMA_VERSION: u32 = 1;
/// Schema version for self-hosting configuration documents.
pub const SELF_HOST_CONFIG_SCHEMA_VERSION: u32 = 1;

const EMPTY_LEDGER_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const MAX_LEDGER_BYTES: u64 = 64 * 1024 * 1024;

/// Commercial policy, release, privacy, or persistence failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommercialError(pub String);

impl fmt::Display for CommercialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CommercialError {}

impl From<follon_domain::DomainError> for CommercialError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}

/// Commercial plan recorded in an externally evidenced subscription.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommercialPlan {
    /// A single professional workspace.
    Professional,
    /// A shared organization workspace.
    Organization,
    /// A customer-operated self-hosted installation.
    SelfHosted,
}

impl CommercialPlan {
    /// Parses the stable external representation.
    pub fn parse(value: &str) -> Result<Self, CommercialError> {
        match value {
            "PROFESSIONAL" => Ok(Self::Professional),
            "ORGANIZATION" => Ok(Self::Organization),
            "SELF_HOSTED" => Ok(Self::SelfHosted),
            _ => Err(CommercialError(
                "commercial plan must be PROFESSIONAL, ORGANIZATION, or SELF_HOSTED".to_owned(),
            )),
        }
    }

    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Professional => "PROFESSIONAL",
            Self::Organization => "ORGANIZATION",
            Self::SelfHosted => "SELF_HOSTED",
        }
    }

    /// Maximum named human members enabled by this plan.
    pub const fn maximum_members(self) -> u32 {
        match self {
            Self::Professional => 1,
            Self::Organization => 25,
            Self::SelfHosted => 100,
        }
    }

    /// Whether a customer may run an approved self-hosted deployment.
    pub const fn self_hosting_allowed(self) -> bool {
        matches!(self, Self::SelfHosted)
    }
}

/// Externally observed billing state. The repository never infers this from a card or invoice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BillingStatus {
    /// Verified paid subscription with full entitlement during its interval.
    Paid,
    /// Contractual grace period with read-only access only.
    Grace,
    /// No access because collection has failed or was suspended.
    Suspended,
    /// No access because subscription ended.
    Cancelled,
}

impl BillingStatus {
    /// Parses the stable external representation.
    pub fn parse(value: &str) -> Result<Self, CommercialError> {
        match value {
            "PAID" => Ok(Self::Paid),
            "GRACE" => Ok(Self::Grace),
            "SUSPENDED" => Ok(Self::Suspended),
            "CANCELLED" => Ok(Self::Cancelled),
            _ => Err(CommercialError(
                "billing status must be PAID, GRACE, SUSPENDED, or CANCELLED".to_owned(),
            )),
        }
    }

    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paid => "PAID",
            Self::Grace => "GRACE",
            Self::Suspended => "SUSPENDED",
            Self::Cancelled => "CANCELLED",
        }
    }
}

/// Derived access posture for a selected UTC instant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntitlementAccess {
    /// Plan features may be used.
    Full,
    /// Existing evidence may be read but mutable operation and export must stop.
    ReadOnly,
    /// Tenant access is disabled.
    Denied,
}

impl EntitlementAccess {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "FULL",
            Self::ReadOnly => "READ_ONLY",
            Self::Denied => "DENIED",
        }
    }
}

/// Immutable tenant/workspace provisioning record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantProvisioning {
    /// Canonical tenant identity.
    pub tenant_id: String,
    /// Canonical workspace identity.
    pub workspace_id: String,
    /// Provisioned plan.
    pub plan: CommercialPlan,
    /// Versioned retention policy identity.
    pub retention_policy_id: String,
    /// Whether this tenant is configured for customer-operated hosting.
    pub self_hosted: bool,
    /// Explicit UTC provisioning instant.
    pub provisioned_at: String,
}

impl TenantProvisioning {
    /// Validates immutable tenant provisioning facts.
    pub fn validate(&self) -> Result<(), CommercialError> {
        for (name, value) in [
            ("tenant_id", self.tenant_id.as_str()),
            ("workspace_id", self.workspace_id.as_str()),
            ("retention_policy_id", self.retention_policy_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_utc_timestamp("tenant provisioned_at", &self.provisioned_at)?;
        if self.self_hosted != self.plan.self_hosting_allowed() {
            return Err(CommercialError(
                "self_hosted must exactly match the selected commercial plan".to_owned(),
            ));
        }
        Ok(())
    }
}

/// An externally evidenced subscription observation used to derive access.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionObservation {
    /// Canonical subscription identity assigned by the billing system.
    pub subscription_id: String,
    /// Tenant receiving the entitlement.
    pub tenant_id: String,
    /// Contract plan selected by the billing system.
    pub plan: CommercialPlan,
    /// Externally observed billing state.
    pub status: BillingStatus,
    /// Inclusive UTC start of this observation's entitlement interval.
    pub effective_at: String,
    /// Exclusive UTC end of this observation's entitlement interval.
    pub expires_at: String,
    /// Canonical payment-provider integration identity (for example `stripe`).
    pub payment_provider: String,
    /// Pseudonymous provider customer/account reference; never a name or email address.
    pub external_customer_ref: String,
    /// SHA-256 of retained provider event/receipt evidence held outside this repository.
    pub payment_evidence_hash: String,
}

impl SubscriptionObservation {
    /// Validates a subscription record without calling a payment provider.
    pub fn validate(&self) -> Result<(), CommercialError> {
        for (name, value) in [
            ("subscription_id", self.subscription_id.as_str()),
            ("tenant_id", self.tenant_id.as_str()),
            ("payment_provider", self.payment_provider.as_str()),
            ("external_customer_ref", self.external_customer_ref.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_utc_timestamp("subscription effective_at", &self.effective_at)?;
        validate_utc_timestamp("subscription expires_at", &self.expires_at)?;
        if self.effective_at >= self.expires_at {
            return Err(CommercialError(
                "subscription effective_at must precede expires_at".to_owned(),
            ));
        }
        validate_sha256(
            "subscription payment_evidence_hash",
            &self.payment_evidence_hash,
        )
    }
}

/// Deterministic entitlement derived from the latest effective subscription record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantEntitlement {
    /// Tenant receiving the entitlement.
    pub tenant_id: String,
    /// Applied subscription identity, if one was effective.
    pub subscription_id: Option<String>,
    /// Applied plan, if one was effective.
    pub plan: Option<CommercialPlan>,
    /// Effective access posture.
    pub access: EntitlementAccess,
    /// Maximum members enabled under the applied plan.
    pub maximum_members: u32,
    /// Whether approved self-hosting is enabled.
    pub self_hosting_allowed: bool,
    /// Exact UTC instant selected by the caller.
    pub as_of: String,
    /// Fingerprint of the ledger cursor used for derivation.
    pub ledger_head_hash: String,
}

impl TenantEntitlement {
    /// Returns a portable canonical JSON representation.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"access\":{},\"as_of\":{},\"ledger_head_hash\":{},\"maximum_members\":{},\"plan\":{},\"self_hosting_allowed\":{},\"subscription_id\":{},\"tenant_id\":{}}}",
            json_string(self.access.as_str()),
            json_string(&self.as_of),
            json_string(&self.ledger_head_hash),
            self.maximum_members,
            optional_json_string(self.plan.map(CommercialPlan::as_str)),
            self.self_hosting_allowed,
            optional_json_string(self.subscription_id.as_deref()),
            json_string(&self.tenant_id),
        )
    }
}

/// One append-only commercial control record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommercialLedgerRecord {
    /// Ledger schema version.
    pub ledger_schema_version: u32,
    /// Monotonically increasing sequence number.
    pub sequence: u64,
    /// Idempotency identity selected by the caller.
    pub event_id: String,
    /// Typed event name.
    pub event_type: String,
    /// Canonical tenant identity affected by the event.
    pub tenant_id: String,
    /// Explicit UTC event occurrence time.
    pub occurred_at: String,
    /// Canonical actor or integration identity.
    pub actor: String,
    /// Typed event fields.
    pub details: BTreeMap<String, String>,
    /// SHA-256 predecessor record hash or zero genesis hash.
    pub prev_hash: String,
    /// SHA-256 of the canonical record body.
    pub record_hash: String,
}

impl CommercialLedgerRecord {
    /// Stable JSON line written to durable storage.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"actor\":{},\"details\":{},\"event_id\":{},\"event_type\":{},\"ledger_schema_version\":{},\"occurred_at\":{},\"prev_hash\":{},\"record_hash\":{},\"sequence\":{},\"tenant_id\":{}}}",
            json_string(&self.actor),
            canonical_details_json(&self.details),
            json_string(&self.event_id),
            json_string(&self.event_type),
            self.ledger_schema_version,
            json_string(&self.occurred_at),
            json_string(&self.prev_hash),
            json_string(&self.record_hash),
            self.sequence,
            json_string(&self.tenant_id),
        )
    }
}

/// Process-exclusive, fsynced, hash-chained commercial-control ledger.
pub struct CommercialLedger {
    path: PathBuf,
    file: File,
    records: Vec<CommercialLedgerRecord>,
    event_ids: BTreeSet<String>,
    head_hash: String,
}

impl CommercialLedger {
    /// Opens a ledger only after verifying its full hash chain.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CommercialError> {
        let path = path.as_ref().to_path_buf();
        reject_symlink_path("commercial ledger", &path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_error)?;
        FileExt::try_lock_exclusive(&file).map_err(|error| {
            CommercialError(format!(
                "commercial ledger is already open by another process: {error}"
            ))
        })?;
        let records = verify_locked_ledger(&mut file)?;
        file.seek(SeekFrom::End(0)).map_err(io_error)?;
        let head_hash = records
            .last()
            .map(|record| record.record_hash.clone())
            .unwrap_or_else(|| EMPTY_LEDGER_HASH.to_owned());
        let event_ids = records
            .iter()
            .map(|record| record.event_id.clone())
            .collect();
        Ok(Self {
            path,
            file,
            records,
            event_ids,
            head_hash,
        })
    }

    /// Reads and verifies a stable snapshot without opening a mutable ledger handle.
    pub fn read_verified(
        path: impl AsRef<Path>,
    ) -> Result<Vec<CommercialLedgerRecord>, CommercialError> {
        verify_ledger_file(path.as_ref())
    }

    /// Returns the durable local ledger path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns all verified records in sequence order.
    pub fn records(&self) -> &[CommercialLedgerRecord] {
        &self.records
    }

    /// Returns the current verified ledger head.
    pub fn head_hash(&self) -> &str {
        &self.head_hash
    }

    /// Durably records a tenant provisioning event.
    pub fn provision_tenant(
        &mut self,
        event_id: &str,
        actor: &str,
        provisioning: &TenantProvisioning,
    ) -> Result<&CommercialLedgerRecord, CommercialError> {
        provisioning.validate()?;
        let details = BTreeMap::from([
            ("plan".to_owned(), provisioning.plan.as_str().to_owned()),
            (
                "retention_policy_id".to_owned(),
                provisioning.retention_policy_id.clone(),
            ),
            (
                "self_hosted".to_owned(),
                provisioning.self_hosted.to_string(),
            ),
            ("workspace_id".to_owned(), provisioning.workspace_id.clone()),
        ]);
        self.append(
            event_id,
            "commercial.tenant_provisioned.v1",
            &provisioning.tenant_id,
            &provisioning.provisioned_at,
            actor,
            details,
        )
    }

    /// Durably records an externally evidenced subscription observation.
    pub fn record_subscription(
        &mut self,
        event_id: &str,
        actor: &str,
        observed_at: &str,
        subscription: &SubscriptionObservation,
    ) -> Result<&CommercialLedgerRecord, CommercialError> {
        subscription.validate()?;
        validate_utc_timestamp("subscription observed_at", observed_at)?;
        let details = BTreeMap::from([
            ("effective_at".to_owned(), subscription.effective_at.clone()),
            (
                "external_customer_ref".to_owned(),
                subscription.external_customer_ref.clone(),
            ),
            ("expires_at".to_owned(), subscription.expires_at.clone()),
            (
                "payment_evidence_hash".to_owned(),
                subscription.payment_evidence_hash.clone(),
            ),
            (
                "payment_provider".to_owned(),
                subscription.payment_provider.clone(),
            ),
            ("plan".to_owned(), subscription.plan.as_str().to_owned()),
            ("status".to_owned(), subscription.status.as_str().to_owned()),
            (
                "subscription_id".to_owned(),
                subscription.subscription_id.clone(),
            ),
        ]);
        self.append(
            event_id,
            "commercial.subscription_observed.v1",
            &subscription.tenant_id,
            observed_at,
            actor,
            details,
        )
    }

    fn append(
        &mut self,
        event_id: &str,
        event_type: &str,
        tenant_id: &str,
        occurred_at: &str,
        actor: &str,
        details: BTreeMap<String, String>,
    ) -> Result<&CommercialLedgerRecord, CommercialError> {
        validate_ledger_input(
            event_id,
            event_type,
            tenant_id,
            occurred_at,
            actor,
            &details,
        )?;
        if self.event_ids.contains(event_id) {
            return Err(CommercialError(
                "commercial ledger event_id is already recorded".to_owned(),
            ));
        }
        let sequence = self
            .records
            .len()
            .checked_add(1)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| CommercialError("commercial ledger sequence overflow".to_owned()))?;
        let prev_hash = self.head_hash.clone();
        let record_hash = sha256(&canonical_ledger_body(LedgerBody {
            sequence,
            event_id,
            event_type,
            tenant_id,
            occurred_at,
            actor,
            details: &details,
            prev_hash: &prev_hash,
        }));
        let record = CommercialLedgerRecord {
            ledger_schema_version: COMMERCIAL_LEDGER_SCHEMA_VERSION,
            sequence,
            event_id: event_id.to_owned(),
            event_type: event_type.to_owned(),
            tenant_id: tenant_id.to_owned(),
            occurred_at: occurred_at.to_owned(),
            actor: actor.to_owned(),
            details,
            prev_hash,
            record_hash,
        };
        let line = format!("{}\n", record.canonical_json());
        let current_size = self.file.metadata().map_err(io_error)?.len();
        let next_size = current_size
            .checked_add(u64::try_from(line.len()).map_err(|_| {
                CommercialError("commercial ledger line length overflow".to_owned())
            })?)
            .ok_or_else(|| CommercialError("commercial ledger size overflow".to_owned()))?;
        if next_size > MAX_LEDGER_BYTES {
            return Err(CommercialError(
                "commercial ledger exceeds its configured 64 MiB safety limit".to_owned(),
            ));
        }
        self.file.write_all(line.as_bytes()).map_err(io_error)?;
        self.file.sync_data().map_err(io_error)?;
        self.event_ids.insert(record.event_id.clone());
        self.head_hash = record.record_hash.clone();
        self.records.push(record);
        Ok(self.records.last().expect("record was pushed"))
    }
}

/// Derives a tenant's access posture from one verified ledger snapshot.
pub fn derive_entitlement(
    records: &[CommercialLedgerRecord],
    tenant_id: &str,
    as_of: &str,
) -> Result<TenantEntitlement, CommercialError> {
    validate_canonical_id("entitlement tenant_id", tenant_id)?;
    validate_utc_timestamp("entitlement as_of", as_of)?;
    verify_record_sequence(records)?;
    let provisionings = provisionings(records)?;
    let provisioning = provisionings
        .get(tenant_id)
        .ok_or_else(|| CommercialError("tenant has no durable provisioning record".to_owned()))?;
    let latest = subscriptions(records)?
        .into_iter()
        .filter(|subscription| {
            subscription.subscription.tenant_id == tenant_id
                && subscription.subscription.effective_at.as_str() <= as_of
        })
        .max_by(|left, right| {
            left.subscription
                .effective_at
                .cmp(&right.subscription.effective_at)
                .then_with(|| left.sequence.cmp(&right.sequence))
        });
    let head_hash = records
        .last()
        .map(|record| record.record_hash.clone())
        .unwrap_or_else(|| EMPTY_LEDGER_HASH.to_owned());
    let Some(subscription) = latest else {
        return Ok(TenantEntitlement {
            tenant_id: tenant_id.to_owned(),
            subscription_id: None,
            plan: None,
            access: EntitlementAccess::Denied,
            maximum_members: 0,
            self_hosting_allowed: false,
            as_of: as_of.to_owned(),
            ledger_head_hash: head_hash,
        });
    };
    let subscription = subscription.subscription;
    if subscription.plan != provisioning.plan {
        return Err(CommercialError(
            "subscription plan does not match immutable tenant provisioning".to_owned(),
        ));
    }
    let access = if as_of >= subscription.expires_at.as_str() {
        EntitlementAccess::Denied
    } else {
        match subscription.status {
            BillingStatus::Paid => EntitlementAccess::Full,
            BillingStatus::Grace => EntitlementAccess::ReadOnly,
            BillingStatus::Suspended | BillingStatus::Cancelled => EntitlementAccess::Denied,
        }
    };
    Ok(TenantEntitlement {
        tenant_id: tenant_id.to_owned(),
        subscription_id: Some(subscription.subscription_id),
        plan: Some(subscription.plan),
        access,
        maximum_members: if access == EntitlementAccess::Denied {
            0
        } else {
            subscription.plan.maximum_members()
        },
        self_hosting_allowed: access == EntitlementAccess::Full
            && subscription.plan.self_hosting_allowed(),
        as_of: as_of.to_owned(),
        ledger_head_hash: head_hash,
    })
}

/// Data classification used for retention and privacy decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataClassification {
    /// Customer-authored content that can be erased when no hold applies.
    CustomerContent,
    /// Operational metadata which may be subject to an explicit retention deadline.
    OperationalMetadata,
    /// Licensed or market data governed by an explicit retention deadline.
    MarketData,
    /// Immutable audit evidence; direct privacy erasure is never permitted here.
    AuditEvidence,
}

impl DataClassification {
    /// Parses the stable external representation.
    pub fn parse(value: &str) -> Result<Self, CommercialError> {
        match value {
            "CUSTOMER_CONTENT" => Ok(Self::CustomerContent),
            "OPERATIONAL_METADATA" => Ok(Self::OperationalMetadata),
            "MARKET_DATA" => Ok(Self::MarketData),
            "AUDIT_EVIDENCE" => Ok(Self::AuditEvidence),
            _ => Err(CommercialError(
                "data classification is unsupported".to_owned(),
            )),
        }
    }

    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CustomerContent => "CUSTOMER_CONTENT",
            Self::OperationalMetadata => "OPERATIONAL_METADATA",
            Self::MarketData => "MARKET_DATA",
            Self::AuditEvidence => "AUDIT_EVIDENCE",
        }
    }
}

/// One file-backed data asset governed by an explicit retention policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataAsset {
    /// Canonical asset identity.
    pub asset_id: String,
    /// Owning tenant.
    pub tenant_id: String,
    /// Root-relative file path. Symlinks, absolute paths, and traversal are refused.
    pub relative_path: String,
    /// Data classification.
    pub classification: DataClassification,
    /// Immutable UTC creation instant.
    pub created_at: String,
    /// Inclusive UTC end of retention. The asset becomes eligible at this instant.
    pub retain_until: String,
    /// Pseudonymous SHA-256 data-subject references. No names or emails are accepted.
    pub subject_hashes: Vec<String>,
    /// Legal/regulatory hold. Held assets are never planned for deletion.
    pub legal_hold: bool,
}

impl DataAsset {
    /// Validates an inventory asset before it can influence deletion.
    pub fn validate(&self) -> Result<(), CommercialError> {
        for (name, value) in [
            ("data asset_id", self.asset_id.as_str()),
            ("data asset tenant_id", self.tenant_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_relative_path(&self.relative_path)?;
        validate_utc_timestamp("data asset created_at", &self.created_at)?;
        validate_utc_timestamp("data asset retain_until", &self.retain_until)?;
        if self.created_at > self.retain_until {
            return Err(CommercialError(
                "data asset retain_until cannot precede created_at".to_owned(),
            ));
        }
        let mut subject_hashes = BTreeSet::new();
        for hash in &self.subject_hashes {
            validate_sha256("data asset subject hash", hash)?;
            if !subject_hashes.insert(hash) {
                return Err(CommercialError(
                    "data asset subject hashes must be unique".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// Data-subject request type accepted by the local privacy workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivacyRequestKind {
    /// Produce a minimal data-location inventory; content is never printed by the core.
    Access,
    /// Plan erasure of eligible customer content.
    Erasure,
}

impl PrivacyRequestKind {
    /// Parses the stable external representation.
    pub fn parse(value: &str) -> Result<Self, CommercialError> {
        match value {
            "ACCESS" => Ok(Self::Access),
            "ERASURE" => Ok(Self::Erasure),
            _ => Err(CommercialError(
                "privacy request kind must be ACCESS or ERASURE".to_owned(),
            )),
        }
    }

    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Access => "ACCESS",
            Self::Erasure => "ERASURE",
        }
    }
}

/// Pseudonymous privacy request input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivacyRequest {
    /// Canonical request identity.
    pub request_id: String,
    /// Tenant scope.
    pub tenant_id: String,
    /// SHA-256 pseudonymous subject reference.
    pub subject_hash: String,
    /// Requested workflow.
    pub kind: PrivacyRequestKind,
    /// Explicit UTC request time.
    pub requested_at: String,
}

impl PrivacyRequest {
    /// Validates a request without accepting raw personally identifying information.
    pub fn validate(&self) -> Result<(), CommercialError> {
        validate_canonical_id("privacy request_id", &self.request_id)?;
        validate_canonical_id("privacy tenant_id", &self.tenant_id)?;
        validate_sha256("privacy subject_hash", &self.subject_hash)?;
        validate_utc_timestamp("privacy requested_at", &self.requested_at)?;
        Ok(())
    }
}

/// Reason why a candidate file is eligible for durable deletion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeletionReason {
    /// The asset's explicit retention deadline elapsed.
    RetentionExpired,
    /// A valid subject erasure request selected eligible customer content.
    PrivacyErasure,
}

impl DeletionReason {
    /// Parses the stable external representation.
    pub fn parse(value: &str) -> Result<Self, CommercialError> {
        match value {
            "RETENTION_EXPIRED" => Ok(Self::RetentionExpired),
            "PRIVACY_ERASURE" => Ok(Self::PrivacyErasure),
            _ => Err(CommercialError(
                "deletion reason must be RETENTION_EXPIRED or PRIVACY_ERASURE".to_owned(),
            )),
        }
    }

    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RetentionExpired => "RETENTION_EXPIRED",
            Self::PrivacyErasure => "PRIVACY_ERASURE",
        }
    }
}

/// A verified file candidate in a retention/deletion plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionCandidate {
    /// Canonical asset identity.
    pub asset_id: String,
    /// Root-relative path selected at plan time.
    pub relative_path: String,
    /// Classification retained as execution evidence.
    pub classification: DataClassification,
    /// Reason that selected the candidate.
    pub reason: DeletionReason,
    /// SHA-256 of exact file bytes at plan time.
    pub expected_hash: String,
    /// Exact file length at plan time.
    pub expected_bytes: u64,
}

/// An explicit, hash-bound plan. Execution deliberately deletes one named candidate at a time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionPlan {
    /// Plan schema version.
    pub retention_plan_schema_version: u32,
    /// Tenant scope.
    pub tenant_id: String,
    /// Optional privacy request identity which caused the plan.
    pub privacy_request_id: Option<String>,
    /// Explicit UTC plan time.
    pub as_of: String,
    /// Eligible files in stable asset-ID order.
    pub candidates: Vec<RetentionCandidate>,
    /// Asset IDs withheld due to a legal hold or immutable audit classification.
    pub withheld_asset_ids: Vec<String>,
}

impl RetentionPlan {
    /// Parses an operator plan only when its bytes use this crate's exact canonical form.
    pub fn parse_canonical(source: &str) -> Result<Self, CommercialError> {
        let document: RetentionPlanDocument = serde_json::from_str(source).map_err(|error| {
            CommercialError(format!("retention plan is not valid strict JSON: {error}"))
        })?;
        let plan = Self {
            retention_plan_schema_version: document.retention_plan_schema_version,
            tenant_id: document.tenant_id,
            privacy_request_id: document.privacy_request_id,
            as_of: document.as_of,
            candidates: document
                .candidates
                .into_iter()
                .map(|candidate| {
                    Ok(RetentionCandidate {
                        asset_id: candidate.asset_id,
                        relative_path: candidate.relative_path,
                        classification: DataClassification::parse(&candidate.classification)?,
                        reason: DeletionReason::parse(&candidate.reason)?,
                        expected_hash: candidate.expected_hash,
                        expected_bytes: candidate.expected_bytes,
                    })
                })
                .collect::<Result<Vec<_>, CommercialError>>()?,
            withheld_asset_ids: document.withheld_asset_ids,
        };
        plan.validate()?;
        if source != plan.canonical_json() {
            return Err(CommercialError(
                "retention plan must use canonical JSON serialization".to_owned(),
            ));
        }
        Ok(plan)
    }

    /// Validates plan shape and produces a stable SHA-256 confirmation token.
    pub fn fingerprint(&self) -> Result<String, CommercialError> {
        self.validate()?;
        Ok(sha256(&self.canonical_json()))
    }

    /// Portable canonical JSON evidence for operator review and confirmation.
    pub fn canonical_json(&self) -> String {
        let candidates = self
            .candidates
            .iter()
            .map(|candidate| {
                format!(
                    "{{\"asset_id\":{},\"classification\":{},\"expected_bytes\":{},\"expected_hash\":{},\"reason\":{},\"relative_path\":{}}}",
                    json_string(&candidate.asset_id),
                    json_string(candidate.classification.as_str()),
                    candidate.expected_bytes,
                    json_string(&candidate.expected_hash),
                    json_string(candidate.reason.as_str()),
                    json_string(&candidate.relative_path),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let withheld = self
            .withheld_asset_ids
            .iter()
            .map(|asset_id| json_string(asset_id))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"as_of\":{},\"candidates\":[{}],\"privacy_request_id\":{},\"retention_plan_schema_version\":{},\"tenant_id\":{},\"withheld_asset_ids\":[{}]}}",
            json_string(&self.as_of),
            candidates,
            optional_json_string(self.privacy_request_id.as_deref()),
            self.retention_plan_schema_version,
            json_string(&self.tenant_id),
            withheld,
        )
    }

    fn validate(&self) -> Result<(), CommercialError> {
        if self.retention_plan_schema_version != RETENTION_PLAN_SCHEMA_VERSION {
            return Err(CommercialError(
                "unsupported retention plan schema version".to_owned(),
            ));
        }
        validate_canonical_id("retention plan tenant_id", &self.tenant_id)?;
        if let Some(request_id) = &self.privacy_request_id {
            validate_canonical_id("retention plan privacy_request_id", request_id)?;
        }
        validate_utc_timestamp("retention plan as_of", &self.as_of)?;
        let mut candidate_ids = BTreeSet::new();
        let mut candidate_paths = BTreeSet::new();
        let mut last: Option<String> = None;
        for candidate in &self.candidates {
            validate_canonical_id("retention candidate asset_id", &candidate.asset_id)?;
            validate_relative_path(&candidate.relative_path)?;
            validate_sha256(
                "retention candidate expected_hash",
                &candidate.expected_hash,
            )?;
            if !candidate_ids.insert(&candidate.asset_id)
                || !candidate_paths.insert(&candidate.relative_path)
                || last
                    .as_ref()
                    .is_some_and(|previous| previous >= &candidate.asset_id)
            {
                return Err(CommercialError(
                    "retention candidates must have unique paths and strictly sorted asset IDs"
                        .to_owned(),
                ));
            }
            last = Some(candidate.asset_id.clone());
        }
        let mut withheld = BTreeSet::new();
        for asset_id in &self.withheld_asset_ids {
            validate_canonical_id("retention withheld asset_id", asset_id)?;
            if !withheld.insert(asset_id) || candidate_ids.contains(asset_id) {
                return Err(CommercialError(
                    "retention withheld asset IDs must be unique and not candidates".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionPlanDocument {
    retention_plan_schema_version: u32,
    tenant_id: String,
    privacy_request_id: Option<String>,
    as_of: String,
    candidates: Vec<RetentionCandidateDocument>,
    withheld_asset_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionCandidateDocument {
    asset_id: String,
    relative_path: String,
    classification: String,
    reason: String,
    expected_hash: String,
    expected_bytes: u64,
}

/// Durable evidence that one planned file was deleted after an exact confirmation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionExecutionReceipt {
    /// SHA-256 of the reviewed retention plan.
    pub retention_plan_hash: String,
    /// Deleted asset identity.
    pub asset_id: String,
    /// Exact plan-time file hash.
    pub deleted_file_hash: String,
    /// Explicit UTC execution instant.
    pub executed_at: String,
    /// Canonical operator/service actor.
    pub actor: String,
}

impl RetentionExecutionReceipt {
    /// Portable canonical JSON receipt.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"actor\":{},\"asset_id\":{},\"deleted_file_hash\":{},\"executed_at\":{},\"retention_plan_hash\":{}}}",
            json_string(&self.actor),
            json_string(&self.asset_id),
            json_string(&self.deleted_file_hash),
            json_string(&self.executed_at),
            json_string(&self.retention_plan_hash),
        )
    }
}

/// Plans deletion of non-held files whose explicit retention deadline elapsed.
pub fn plan_expired_retention(
    data_root: &Path,
    assets: &[DataAsset],
    tenant_id: &str,
    as_of: &str,
) -> Result<RetentionPlan, CommercialError> {
    validate_canonical_id("retention tenant_id", tenant_id)?;
    validate_utc_timestamp("retention as_of", as_of)?;
    let root = validated_data_root(data_root)?;
    let mut candidates = Vec::new();
    let mut withheld = Vec::new();
    for asset in assets.iter().filter(|asset| asset.tenant_id == tenant_id) {
        asset.validate()?;
        if asset.legal_hold {
            withheld.push(asset.asset_id.clone());
        } else if asset.retain_until.as_str() <= as_of {
            candidates.push(candidate_from_asset(
                &root,
                asset,
                DeletionReason::RetentionExpired,
            )?);
        }
    }
    candidates.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
    withheld.sort();
    let plan = RetentionPlan {
        retention_plan_schema_version: RETENTION_PLAN_SCHEMA_VERSION,
        tenant_id: tenant_id.to_owned(),
        privacy_request_id: None,
        as_of: as_of.to_owned(),
        candidates,
        withheld_asset_ids: withheld,
    };
    plan.validate()?;
    Ok(plan)
}

/// Plans erasure for eligible customer-content files matching a pseudonymous subject reference.
pub fn plan_privacy_erasure(
    data_root: &Path,
    assets: &[DataAsset],
    request: &PrivacyRequest,
    as_of: &str,
) -> Result<RetentionPlan, CommercialError> {
    request.validate()?;
    validate_utc_timestamp("privacy erasure as_of", as_of)?;
    if request.kind != PrivacyRequestKind::Erasure {
        return Err(CommercialError(
            "privacy erasure planning requires an ERASURE request".to_owned(),
        ));
    }
    if as_of < request.requested_at.as_str() {
        return Err(CommercialError(
            "privacy erasure as_of cannot precede the request".to_owned(),
        ));
    }
    let root = validated_data_root(data_root)?;
    let mut candidates = Vec::new();
    let mut withheld = Vec::new();
    for asset in assets.iter().filter(|asset| {
        asset.tenant_id == request.tenant_id
            && asset
                .subject_hashes
                .iter()
                .any(|hash| hash == &request.subject_hash)
    }) {
        asset.validate()?;
        if asset.legal_hold || asset.classification == DataClassification::AuditEvidence {
            withheld.push(asset.asset_id.clone());
        } else if asset.classification == DataClassification::CustomerContent {
            candidates.push(candidate_from_asset(
                &root,
                asset,
                DeletionReason::PrivacyErasure,
            )?);
        }
    }
    candidates.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
    withheld.sort();
    let plan = RetentionPlan {
        retention_plan_schema_version: RETENTION_PLAN_SCHEMA_VERSION,
        tenant_id: request.tenant_id.clone(),
        privacy_request_id: Some(request.request_id.clone()),
        as_of: as_of.to_owned(),
        candidates,
        withheld_asset_ids: withheld,
    };
    plan.validate()?;
    Ok(plan)
}

/// Deletes exactly one verified candidate after an explicit matching plan-hash confirmation.
///
/// The file is re-hashed immediately before removal; changed, missing, symlinked, or
/// out-of-root paths fail closed. Callers should persist the returned receipt as an immutable
/// artifact immediately after success.
pub fn execute_retention_candidate(
    data_root: &Path,
    plan: &RetentionPlan,
    asset_id: &str,
    confirmed_plan_hash: &str,
    executed_at: &str,
    actor: &str,
) -> Result<RetentionExecutionReceipt, CommercialError> {
    plan.validate()?;
    validate_canonical_id("retention execution asset_id", asset_id)?;
    validate_sha256("retention confirmed_plan_hash", confirmed_plan_hash)?;
    validate_utc_timestamp("retention executed_at", executed_at)?;
    validate_canonical_id("retention execution actor", actor)?;
    if executed_at < plan.as_of.as_str() {
        return Err(CommercialError(
            "retention execution cannot precede the reviewed plan".to_owned(),
        ));
    }
    let plan_hash = plan.fingerprint()?;
    if plan_hash != confirmed_plan_hash {
        return Err(CommercialError(
            "retention plan confirmation hash does not match the reviewed plan".to_owned(),
        ));
    }
    let candidate = plan
        .candidates
        .iter()
        .find(|candidate| candidate.asset_id == asset_id)
        .ok_or_else(|| CommercialError("asset is not an eligible plan candidate".to_owned()))?;
    let root = validated_data_root(data_root)?;
    let path = resolve_asset_path(&root, &candidate.relative_path)?;
    let metadata = fs::metadata(&path).map_err(io_error)?;
    if metadata.len() != candidate.expected_bytes || sha256_file(&path)? != candidate.expected_hash
    {
        return Err(CommercialError(
            "retention candidate changed after planning; refusing deletion".to_owned(),
        ));
    }
    fs::remove_file(&path).map_err(io_error)?;
    sync_parent(&path)?;
    Ok(RetentionExecutionReceipt {
        retention_plan_hash: plan_hash,
        asset_id: candidate.asset_id.clone(),
        deleted_file_hash: candidate.expected_hash.clone(),
        executed_at: executed_at.to_owned(),
        actor: actor.to_owned(),
    })
}

/// One artifact expected in a signed release.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseArtifact {
    /// Canonical artifact identity.
    pub artifact_id: String,
    /// Root-relative artifact path.
    pub relative_path: String,
    /// SHA-256 of exact artifact bytes.
    pub sha256: String,
    /// Exact byte length.
    pub bytes: u64,
}

/// Canonical signed-release manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseManifest {
    /// Release manifest schema version.
    pub release_manifest_schema_version: u32,
    /// Canonical release identity.
    pub release_id: String,
    /// Immutable display version; safe ASCII token, not used as a key.
    pub version: String,
    /// Explicit UTC creation time.
    pub created_at: String,
    /// Lowercase 40- or 64-hex source revision identity.
    pub source_revision: String,
    /// SHA-256 of the separately retained software bill of materials.
    pub sbom_sha256: String,
    /// Artifacts in strict artifact-ID order.
    pub artifacts: Vec<ReleaseArtifact>,
}

/// Builds a validated release manifest by measuring exact regular files below one artifact root.
///
/// The caller supplies stable artifact IDs and root-relative paths in the desired canonical order.
/// The returned manifest binds the measured byte length and SHA-256 for every artifact.
pub fn build_release_manifest(
    release_id: &str,
    version: &str,
    created_at: &str,
    source_revision: &str,
    sbom_sha256: &str,
    artifact_root: &Path,
    artifact_specs: &[(String, String)],
) -> Result<ReleaseManifest, CommercialError> {
    let root = validated_data_root(artifact_root)?;
    let artifacts = artifact_specs
        .iter()
        .map(|(artifact_id, relative_path)| {
            let path = resolve_asset_path(&root, relative_path)?;
            let metadata = fs::metadata(&path).map_err(io_error)?;
            Ok(ReleaseArtifact {
                artifact_id: artifact_id.clone(),
                relative_path: relative_path.clone(),
                sha256: sha256_file(&path)?,
                bytes: metadata.len(),
            })
        })
        .collect::<Result<Vec<_>, CommercialError>>()?;
    let manifest = ReleaseManifest {
        release_manifest_schema_version: RELEASE_MANIFEST_SCHEMA_VERSION,
        release_id: release_id.to_owned(),
        version: version.to_owned(),
        created_at: created_at.to_owned(),
        source_revision: source_revision.to_owned(),
        sbom_sha256: sbom_sha256.to_owned(),
        artifacts,
    };
    manifest.validate()?;
    Ok(manifest)
}

impl ReleaseManifest {
    /// Parses and requires exact canonical JSON bytes before signing or verification.
    pub fn parse_canonical(source: &str) -> Result<Self, CommercialError> {
        let document: ReleaseManifestDocument = serde_json::from_str(source).map_err(|error| {
            CommercialError(format!(
                "release manifest is not valid strict JSON: {error}"
            ))
        })?;
        let manifest = Self {
            release_manifest_schema_version: document.release_manifest_schema_version,
            release_id: document.release_id,
            version: document.version,
            created_at: document.created_at,
            source_revision: document.source_revision,
            sbom_sha256: document.sbom_sha256,
            artifacts: document
                .artifacts
                .into_iter()
                .map(|artifact| ReleaseArtifact {
                    artifact_id: artifact.artifact_id,
                    relative_path: artifact.relative_path,
                    sha256: artifact.sha256,
                    bytes: artifact.bytes,
                })
                .collect(),
        };
        manifest.validate()?;
        if source != manifest.canonical_json() {
            return Err(CommercialError(
                "release manifest must use its canonical JSON serialization".to_owned(),
            ));
        }
        Ok(manifest)
    }

    /// Validates manifest identity and artifact safety.
    pub fn validate(&self) -> Result<(), CommercialError> {
        if self.release_manifest_schema_version != RELEASE_MANIFEST_SCHEMA_VERSION {
            return Err(CommercialError(
                "unsupported release manifest schema version".to_owned(),
            ));
        }
        validate_canonical_id("release_id", &self.release_id)?;
        validate_utc_timestamp("release created_at", &self.created_at)?;
        if self.version.is_empty()
            || self.version.len() > 128
            || !self
                .version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
            || !(self.source_revision.len() == 40 || self.source_revision.len() == 64)
            || !self
                .source_revision
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte <= b'f'))
        {
            return Err(CommercialError(
                "release version or source_revision is invalid".to_owned(),
            ));
        }
        validate_sha256("release sbom_sha256", &self.sbom_sha256)?;
        if self.artifacts.is_empty() {
            return Err(CommercialError(
                "release manifest requires at least one artifact".to_owned(),
            ));
        }
        let mut artifact_ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut previous: Option<String> = None;
        for artifact in &self.artifacts {
            validate_canonical_id("release artifact_id", &artifact.artifact_id)?;
            validate_relative_path(&artifact.relative_path)?;
            validate_sha256("release artifact sha256", &artifact.sha256)?;
            if artifact.bytes == 0
                || !artifact_ids.insert(&artifact.artifact_id)
                || !paths.insert(&artifact.relative_path)
                || previous
                    .as_ref()
                    .is_some_and(|last| last >= &artifact.artifact_id)
            {
                return Err(CommercialError(
                    "release artifacts need positive bytes, unique paths, and strict artifact-ID order"
                        .to_owned(),
                ));
            }
            previous = Some(artifact.artifact_id.clone());
        }
        Ok(())
    }

    /// Stable exact manifest representation that is signed by this crate.
    pub fn canonical_json(&self) -> String {
        let artifacts = self
            .artifacts
            .iter()
            .map(|artifact| {
                format!(
                    "{{\"artifact_id\":{},\"bytes\":{},\"relative_path\":{},\"sha256\":{}}}",
                    json_string(&artifact.artifact_id),
                    artifact.bytes,
                    json_string(&artifact.relative_path),
                    json_string(&artifact.sha256),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"artifacts\":[{}],\"created_at\":{},\"release_id\":{},\"release_manifest_schema_version\":{},\"sbom_sha256\":{},\"source_revision\":{},\"version\":{}}}",
            artifacts,
            json_string(&self.created_at),
            json_string(&self.release_id),
            self.release_manifest_schema_version,
            json_string(&self.sbom_sha256),
            json_string(&self.source_revision),
            json_string(&self.version),
        )
    }

    /// SHA-256 of its exact canonical bytes.
    pub fn fingerprint(&self) -> Result<String, CommercialError> {
        self.validate()?;
        Ok(sha256(&self.canonical_json()))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseManifestDocument {
    release_manifest_schema_version: u32,
    release_id: String,
    version: String,
    created_at: String,
    source_revision: String,
    sbom_sha256: String,
    artifacts: Vec<ReleaseArtifactDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseArtifactDocument {
    artifact_id: String,
    relative_path: String,
    sha256: String,
    bytes: u64,
}

/// Detached Ed25519 signature for one exact canonical release manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseSignature {
    /// Signature schema version.
    pub release_signature_schema_version: u32,
    /// Bound release identity.
    pub release_id: String,
    /// SHA-256 of exact canonical manifest bytes.
    pub manifest_hash: String,
    /// Canonical trusted signing-key identity.
    pub key_id: String,
    /// Lowercase hex Ed25519 signature over exact manifest bytes.
    pub signature_hex: String,
    /// Explicit UTC signature time.
    pub signed_at: String,
}

impl ReleaseSignature {
    /// Parses and requires exact canonical JSON bytes.
    pub fn parse_canonical(source: &str) -> Result<Self, CommercialError> {
        let document: ReleaseSignatureDocument = serde_json::from_str(source).map_err(|error| {
            CommercialError(format!(
                "release signature is not valid strict JSON: {error}"
            ))
        })?;
        let signature = Self {
            release_signature_schema_version: document.release_signature_schema_version,
            release_id: document.release_id,
            manifest_hash: document.manifest_hash,
            key_id: document.key_id,
            signature_hex: document.signature_hex,
            signed_at: document.signed_at,
        };
        signature.validate()?;
        if source != signature.canonical_json() {
            return Err(CommercialError(
                "release signature must use its canonical JSON serialization".to_owned(),
            ));
        }
        Ok(signature)
    }

    /// Validates detached signature metadata.
    pub fn validate(&self) -> Result<(), CommercialError> {
        if self.release_signature_schema_version != RELEASE_SIGNATURE_SCHEMA_VERSION {
            return Err(CommercialError(
                "unsupported release signature schema version".to_owned(),
            ));
        }
        validate_canonical_id("release signature release_id", &self.release_id)?;
        validate_canonical_id("release signature key_id", &self.key_id)?;
        validate_sha256("release signature manifest_hash", &self.manifest_hash)?;
        validate_utc_timestamp("release signed_at", &self.signed_at)?;
        let bytes = decode_hex("release signature", &self.signature_hex)?;
        if bytes.len() != 64 {
            return Err(CommercialError(
                "release signature must be an Ed25519 64-byte lowercase hex value".to_owned(),
            ));
        }
        Ok(())
    }

    /// Stable detached-signature representation.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"key_id\":{},\"manifest_hash\":{},\"release_id\":{},\"release_signature_schema_version\":{},\"signature_hex\":{},\"signed_at\":{}}}",
            json_string(&self.key_id),
            json_string(&self.manifest_hash),
            json_string(&self.release_id),
            self.release_signature_schema_version,
            json_string(&self.signature_hex),
            json_string(&self.signed_at),
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseSignatureDocument {
    release_signature_schema_version: u32,
    release_id: String,
    manifest_hash: String,
    key_id: String,
    signature_hex: String,
    signed_at: String,
}

/// Trusted Ed25519 public signing key retained by a deployment owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedReleaseKey {
    /// Canonical key identity.
    pub key_id: String,
    /// Lowercase hex of the 32-byte Ed25519 public key.
    pub public_key_hex: String,
}

impl TrustedReleaseKey {
    /// Validates trusted key identity and length.
    pub fn validate(&self) -> Result<(), CommercialError> {
        validate_canonical_id("trusted release key_id", &self.key_id)?;
        let bytes = decode_hex("trusted release public key", &self.public_key_hex)?;
        if bytes.len() != 32 {
            return Err(CommercialError(
                "trusted release public key must be 32-byte lowercase hex".to_owned(),
            ));
        }
        Ok(())
    }

    /// Stable canonical JSON key file representation.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"key_id\":{},\"public_key_hex\":{}}}",
            json_string(&self.key_id),
            json_string(&self.public_key_hex),
        )
    }

    /// Parses an exact canonical trusted-key document.
    pub fn parse_canonical(source: &str) -> Result<Self, CommercialError> {
        let document: TrustedReleaseKeyDocument =
            serde_json::from_str(source).map_err(|error| {
                CommercialError(format!(
                    "trusted release key is not valid strict JSON: {error}"
                ))
            })?;
        let key = Self {
            key_id: document.key_id,
            public_key_hex: document.public_key_hex,
        };
        key.validate()?;
        if source != key.canonical_json() {
            return Err(CommercialError(
                "trusted release key must use its canonical JSON serialization".to_owned(),
            ));
        }
        Ok(key)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedReleaseKeyDocument {
    key_id: String,
    public_key_hex: String,
}

/// Creates an Ed25519 PKCS#8 private key and matching public-key metadata.
pub fn generate_release_keypair(
    key_id: &str,
) -> Result<(Vec<u8>, TrustedReleaseKey), CommercialError> {
    validate_canonical_id("release key_id", key_id)?;
    let random = SystemRandom::new();
    let document = Ed25519KeyPair::generate_pkcs8(&random)
        .map_err(|_| CommercialError("release Ed25519 key generation failed".to_owned()))?;
    let private_key = document.as_ref().to_vec();
    let key_pair = Ed25519KeyPair::from_pkcs8(&private_key)
        .map_err(|_| CommercialError("generated release key could not be loaded".to_owned()))?;
    let trusted_key = TrustedReleaseKey {
        key_id: key_id.to_owned(),
        public_key_hex: hex_encode(key_pair.public_key().as_ref()),
    };
    trusted_key.validate()?;
    Ok((private_key, trusted_key))
}

/// Signs exact canonical manifest bytes with an Ed25519 PKCS#8 private key.
pub fn sign_release_manifest(
    manifest_source: &str,
    private_key_pkcs8: &[u8],
    key_id: &str,
    signed_at: &str,
) -> Result<ReleaseSignature, CommercialError> {
    let manifest = ReleaseManifest::parse_canonical(manifest_source)?;
    validate_canonical_id("release signing key_id", key_id)?;
    validate_utc_timestamp("release signed_at", signed_at)?;
    let key_pair = Ed25519KeyPair::from_pkcs8(private_key_pkcs8).map_err(|_| {
        CommercialError("release private key is not valid Ed25519 PKCS#8".to_owned())
    })?;
    let signature = key_pair.sign(manifest_source.as_bytes());
    let result = ReleaseSignature {
        release_signature_schema_version: RELEASE_SIGNATURE_SCHEMA_VERSION,
        release_id: manifest.release_id,
        manifest_hash: sha256(manifest_source),
        key_id: key_id.to_owned(),
        signature_hex: hex_encode(signature.as_ref()),
        signed_at: signed_at.to_owned(),
    };
    result.validate()?;
    Ok(result)
}

/// Verifies a detached signature against an exact canonical manifest and trusted public key.
pub fn verify_release_signature(
    manifest_source: &str,
    signature: &ReleaseSignature,
    trusted_key: &TrustedReleaseKey,
) -> Result<ReleaseManifest, CommercialError> {
    let manifest = ReleaseManifest::parse_canonical(manifest_source)?;
    signature.validate()?;
    trusted_key.validate()?;
    if signature.release_id != manifest.release_id
        || signature.manifest_hash != sha256(manifest_source)
        || signature.key_id != trusted_key.key_id
    {
        return Err(CommercialError(
            "release signature does not bind this manifest and trusted key".to_owned(),
        ));
    }
    let public_key = decode_hex("trusted release public key", &trusted_key.public_key_hex)?;
    let signature_bytes = decode_hex("release signature", &signature.signature_hex)?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(manifest_source.as_bytes(), &signature_bytes)
        .map_err(|_| CommercialError("release signature verification failed".to_owned()))?;
    Ok(manifest)
}

/// Verifies every release artifact below one non-symlinked artifact root.
pub fn verify_release_artifacts(
    manifest: &ReleaseManifest,
    artifact_root: &Path,
) -> Result<(), CommercialError> {
    manifest.validate()?;
    let root = validated_data_root(artifact_root)?;
    for artifact in &manifest.artifacts {
        let path = resolve_asset_path(&root, &artifact.relative_path)?;
        let metadata = fs::metadata(&path).map_err(io_error)?;
        if metadata.len() != artifact.bytes || sha256_file(&path)? != artifact.sha256 {
            return Err(CommercialError(format!(
                "release artifact {} does not match its signed manifest",
                artifact.artifact_id
            )));
        }
    }
    Ok(())
}

/// Hardened self-hosted deployment configuration. It is intentionally local-only behind a
/// separately configured reverse proxy and managed secret provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelfHostConfig {
    /// Config schema version.
    pub self_host_config_schema_version: u32,
    /// Canonical self-hosted installation identity.
    pub instance_id: String,
    /// Tenant owned by the installation.
    pub tenant_id: String,
    /// Root-relative local storage directory inside the host deployment root.
    pub storage_relative_path: String,
    /// Root-relative commercial ledger location inside the host deployment root.
    pub ledger_relative_path: String,
    /// Canonical retention policy selected for hosted customer data.
    pub retention_policy_id: String,
    /// SHA-256 of the exact signed release manifest.
    pub release_manifest_hash: String,
    /// SHA-256 of the exact detached signature file.
    pub release_signature_hash: String,
    /// Trusted signing-key identity required by the instance.
    pub trusted_release_key_id: String,
    /// Credential boundary supported by this deployment.
    pub secret_provider_kind: String,
    /// Safe loopback bind address; a TLS reverse proxy terminates external traffic.
    pub bind_address: String,
}

impl SelfHostConfig {
    /// Parses and requires exact canonical configuration bytes.
    pub fn parse_canonical(source: &str) -> Result<Self, CommercialError> {
        let document: SelfHostConfigDocument = serde_json::from_str(source).map_err(|error| {
            CommercialError(format!(
                "self-host configuration is not valid strict JSON: {error}"
            ))
        })?;
        let config = Self {
            self_host_config_schema_version: document.self_host_config_schema_version,
            instance_id: document.instance_id,
            tenant_id: document.tenant_id,
            storage_relative_path: document.storage_relative_path,
            ledger_relative_path: document.ledger_relative_path,
            retention_policy_id: document.retention_policy_id,
            release_manifest_hash: document.release_manifest_hash,
            release_signature_hash: document.release_signature_hash,
            trusted_release_key_id: document.trusted_release_key_id,
            secret_provider_kind: document.secret_provider_kind,
            bind_address: document.bind_address,
        };
        config.validate()?;
        if source != config.canonical_json() {
            return Err(CommercialError(
                "self-host configuration must use canonical JSON serialization".to_owned(),
            ));
        }
        Ok(config)
    }

    /// Validates a minimal hardened self-hosting configuration.
    pub fn validate(&self) -> Result<(), CommercialError> {
        if self.self_host_config_schema_version != SELF_HOST_CONFIG_SCHEMA_VERSION {
            return Err(CommercialError(
                "unsupported self-host configuration schema version".to_owned(),
            ));
        }
        for (name, value) in [
            ("self-host instance_id", self.instance_id.as_str()),
            ("self-host tenant_id", self.tenant_id.as_str()),
            (
                "self-host retention_policy_id",
                self.retention_policy_id.as_str(),
            ),
            (
                "self-host trusted_release_key_id",
                self.trusted_release_key_id.as_str(),
            ),
            (
                "self-host secret_provider_kind",
                self.secret_provider_kind.as_str(),
            ),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_relative_path(&self.storage_relative_path)?;
        validate_relative_path(&self.ledger_relative_path)?;
        if self.storage_relative_path == self.ledger_relative_path {
            return Err(CommercialError(
                "self-host storage and ledger paths must be distinct".to_owned(),
            ));
        }
        validate_sha256(
            "self-host release_manifest_hash",
            &self.release_manifest_hash,
        )?;
        validate_sha256(
            "self-host release_signature_hash",
            &self.release_signature_hash,
        )?;
        if self.secret_provider_kind != "managed_command"
            || !matches!(self.bind_address.as_str(), "127.0.0.1" | "::1")
        {
            return Err(CommercialError(
                "self-host deployment requires managed_command secrets and a loopback bind address"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    /// Stable configuration bytes whose SHA-256 identifies the deployment policy.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"bind_address\":{},\"instance_id\":{},\"ledger_relative_path\":{},\"release_manifest_hash\":{},\"release_signature_hash\":{},\"retention_policy_id\":{},\"secret_provider_kind\":{},\"self_host_config_schema_version\":{},\"storage_relative_path\":{},\"tenant_id\":{},\"trusted_release_key_id\":{}}}",
            json_string(&self.bind_address),
            json_string(&self.instance_id),
            json_string(&self.ledger_relative_path),
            json_string(&self.release_manifest_hash),
            json_string(&self.release_signature_hash),
            json_string(&self.retention_policy_id),
            json_string(&self.secret_provider_kind),
            self.self_host_config_schema_version,
            json_string(&self.storage_relative_path),
            json_string(&self.tenant_id),
            json_string(&self.trusted_release_key_id),
        )
    }

    /// SHA-256 of exact canonical configuration bytes.
    pub fn fingerprint(&self) -> Result<String, CommercialError> {
        self.validate()?;
        Ok(sha256(&self.canonical_json()))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelfHostConfigDocument {
    self_host_config_schema_version: u32,
    instance_id: String,
    tenant_id: String,
    storage_relative_path: String,
    ledger_relative_path: String,
    retention_policy_id: String,
    release_manifest_hash: String,
    release_signature_hash: String,
    trusted_release_key_id: String,
    secret_provider_kind: String,
    bind_address: String,
}

/// Verifies the release/configuration binding which precedes entitlement-aware self-host readiness.
fn verify_release_bound_self_host_readiness(
    config: &SelfHostConfig,
    manifest_source: &str,
    signature_source: &str,
    trusted_key: &TrustedReleaseKey,
    artifact_root: &Path,
) -> Result<String, CommercialError> {
    config.validate()?;
    let signature = ReleaseSignature::parse_canonical(signature_source)?;
    let manifest = verify_release_signature(manifest_source, &signature, trusted_key)?;
    if config.release_manifest_hash != sha256(manifest_source)
        || config.release_signature_hash != sha256(signature_source)
        || config.trusted_release_key_id != trusted_key.key_id
    {
        return Err(CommercialError(
            "self-host configuration release pointers do not match verified release evidence"
                .to_owned(),
        ));
    }
    verify_release_artifacts(&manifest, artifact_root)?;
    Ok(format!(
        "{{\"configuration_fingerprint\":{},\"instance_id\":{},\"release_id\":{},\"self_host_readiness_schema_version\":1,\"state\":\"READY\",\"tenant_id\":{}}}",
        json_string(&config.fingerprint()?),
        json_string(&config.instance_id),
        json_string(&manifest.release_id),
        json_string(&config.tenant_id),
    ))
}

/// Verifies release readiness and requires a current full self-host entitlement from a verified
/// commercial ledger snapshot.
pub fn verify_entitled_self_host_readiness(
    config: &SelfHostConfig,
    manifest_source: &str,
    signature_source: &str,
    trusted_key: &TrustedReleaseKey,
    artifact_root: &Path,
    entitlement: &TenantEntitlement,
) -> Result<String, CommercialError> {
    if entitlement.tenant_id != config.tenant_id
        || entitlement.access != EntitlementAccess::Full
        || !entitlement.self_hosting_allowed
    {
        return Err(CommercialError(
            "self-host readiness requires a current full self-host entitlement for this tenant"
                .to_owned(),
        ));
    }
    verify_release_bound_self_host_readiness(
        config,
        manifest_source,
        signature_source,
        trusted_key,
        artifact_root,
    )?;
    let manifest = ReleaseManifest::parse_canonical(manifest_source)?;
    Ok(format!(
        "{{\"configuration_fingerprint\":{},\"entitlement_ledger_head_hash\":{},\"instance_id\":{},\"release_id\":{},\"self_host_readiness_schema_version\":1,\"self_hosting_allowed\":true,\"state\":\"READY\",\"tenant_id\":{}}}",
        json_string(&config.fingerprint()?),
        json_string(&entitlement.ledger_head_hash),
        json_string(&config.instance_id),
        json_string(&manifest.release_id),
        json_string(&config.tenant_id),
    ))
}

fn candidate_from_asset(
    root: &Path,
    asset: &DataAsset,
    reason: DeletionReason,
) -> Result<RetentionCandidate, CommercialError> {
    let path = resolve_asset_path(root, &asset.relative_path)?;
    let metadata = fs::metadata(&path).map_err(io_error)?;
    Ok(RetentionCandidate {
        asset_id: asset.asset_id.clone(),
        relative_path: asset.relative_path.clone(),
        classification: asset.classification,
        reason,
        expected_hash: sha256_file(&path)?,
        expected_bytes: metadata.len(),
    })
}

fn validated_data_root(path: &Path) -> Result<PathBuf, CommercialError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommercialError(
            "data/artifact root must be an existing non-symlink directory".to_owned(),
        ));
    }
    path.canonicalize().map_err(io_error)
}

fn resolve_asset_path(root: &Path, relative_path: &str) -> Result<PathBuf, CommercialError> {
    let relative = validate_relative_path(relative_path)?;
    let candidate = root.join(relative);
    let metadata = fs::symlink_metadata(&candidate).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CommercialError(
            "retention/release candidate must be an existing regular non-symlink file".to_owned(),
        ));
    }
    let resolved = candidate.canonicalize().map_err(io_error)?;
    if !resolved.starts_with(root) {
        return Err(CommercialError(
            "retention/release candidate resolves outside its configured root".to_owned(),
        ));
    }
    Ok(resolved)
}

fn validate_relative_path(value: &str) -> Result<PathBuf, CommercialError> {
    if value.is_empty() || value.len() > 4_096 || value.contains(['\0', '\\']) {
        return Err(CommercialError(
            "relative path is empty, too long, or contains a NUL".to_owned(),
        ));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CommercialError(
            "path must be a normal root-relative file path without traversal".to_owned(),
        ));
    }
    Ok(path.to_path_buf())
}

fn sha256_file(path: &Path) -> Result<String, CommercialError> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(not(windows))]
fn sync_parent(path: &Path) -> Result<(), CommercialError> {
    let parent = path
        .parent()
        .ok_or_else(|| CommercialError("retention candidate has no parent directory".to_owned()))?;
    File::open(parent)
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)
}

#[cfg(windows)]
fn sync_parent(path: &Path) -> Result<(), CommercialError> {
    if path.parent().is_none() {
        return Err(CommercialError(
            "retention candidate has no parent directory".to_owned(),
        ));
    }
    // Windows does not allow opening a directory through `std::fs::File`, so the
    // file removal is already committed by the successful `remove_file` call.
    Ok(())
}

fn provisionings(
    records: &[CommercialLedgerRecord],
) -> Result<BTreeMap<String, TenantProvisioning>, CommercialError> {
    let mut provisionings = BTreeMap::new();
    for record in records
        .iter()
        .filter(|record| record.event_type == "commercial.tenant_provisioned.v1")
    {
        let provisioning = TenantProvisioning {
            tenant_id: record.tenant_id.clone(),
            workspace_id: required_detail(record, "workspace_id")?.to_owned(),
            plan: CommercialPlan::parse(required_detail(record, "plan")?)?,
            retention_policy_id: required_detail(record, "retention_policy_id")?.to_owned(),
            self_hosted: parse_bool_detail(record, "self_hosted")?,
            provisioned_at: record.occurred_at.clone(),
        };
        provisioning.validate()?;
        if provisionings
            .insert(provisioning.tenant_id.clone(), provisioning)
            .is_some()
        {
            return Err(CommercialError(
                "tenant has multiple immutable provisioning records".to_owned(),
            ));
        }
    }
    Ok(provisionings)
}

struct RecordedSubscription {
    sequence: u64,
    subscription: SubscriptionObservation,
}

fn subscriptions(
    records: &[CommercialLedgerRecord],
) -> Result<Vec<RecordedSubscription>, CommercialError> {
    let mut subscriptions = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.event_type == "commercial.subscription_observed.v1")
    {
        let subscription = SubscriptionObservation {
            subscription_id: required_detail(record, "subscription_id")?.to_owned(),
            tenant_id: record.tenant_id.clone(),
            plan: CommercialPlan::parse(required_detail(record, "plan")?)?,
            status: BillingStatus::parse(required_detail(record, "status")?)?,
            effective_at: required_detail(record, "effective_at")?.to_owned(),
            expires_at: required_detail(record, "expires_at")?.to_owned(),
            payment_provider: required_detail(record, "payment_provider")?.to_owned(),
            external_customer_ref: required_detail(record, "external_customer_ref")?.to_owned(),
            payment_evidence_hash: required_detail(record, "payment_evidence_hash")?.to_owned(),
        };
        subscription.validate()?;
        subscriptions.push(RecordedSubscription {
            sequence: record.sequence,
            subscription,
        });
    }
    Ok(subscriptions)
}

fn required_detail<'a>(
    record: &'a CommercialLedgerRecord,
    key: &str,
) -> Result<&'a str, CommercialError> {
    record.details.get(key).map(String::as_str).ok_or_else(|| {
        CommercialError(format!(
            "commercial ledger {} is missing required detail {key}",
            record.event_id
        ))
    })
}

fn parse_bool_detail(record: &CommercialLedgerRecord, key: &str) -> Result<bool, CommercialError> {
    match required_detail(record, key)? {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(CommercialError(format!(
            "commercial ledger {} has invalid boolean detail {key}",
            record.event_id
        ))),
    }
}

fn verify_ledger_file(path: &Path) -> Result<Vec<CommercialLedgerRecord>, CommercialError> {
    reject_symlink_path("commercial ledger", path)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut file = File::open(path).map_err(io_error)?;
    FileExt::try_lock_shared(&file).map_err(|error| {
        CommercialError(format!(
            "commercial ledger cannot obtain a stable shared lock: {error}"
        ))
    })?;
    verify_locked_ledger(&mut file)
}

fn verify_locked_ledger(file: &mut File) -> Result<Vec<CommercialLedgerRecord>, CommercialError> {
    let metadata = file.metadata().map_err(io_error)?;
    if metadata.len() > MAX_LEDGER_BYTES {
        return Err(CommercialError(
            "commercial ledger exceeds its configured 64 MiB safety limit".to_owned(),
        ));
    }
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut source = String::new();
    file.read_to_string(&mut source).map_err(io_error)?;
    if source.is_empty() {
        return Ok(Vec::new());
    }
    if !source.ends_with('\n') {
        return Err(CommercialError(
            "commercial ledger must end with one complete newline-delimited record".to_owned(),
        ));
    }
    let mut records = Vec::new();
    let mut previous_hash = EMPTY_LEDGER_HASH.to_owned();
    let mut event_ids = BTreeSet::new();
    for (index, line) in source.lines().enumerate() {
        if line.is_empty() {
            return Err(CommercialError(format!(
                "commercial ledger contains an empty line at {}",
                index + 1
            )));
        }
        let document: CommercialLedgerRecordDocument =
            serde_json::from_str(line).map_err(|_| {
                CommercialError(format!(
                    "commercial ledger line {} is not valid JSON",
                    index + 1
                ))
            })?;
        let record = CommercialLedgerRecord {
            ledger_schema_version: document.ledger_schema_version,
            sequence: document.sequence,
            event_id: document.event_id,
            event_type: document.event_type,
            tenant_id: document.tenant_id,
            occurred_at: document.occurred_at,
            actor: document.actor,
            details: document.details,
            prev_hash: document.prev_hash,
            record_hash: document.record_hash,
        };
        validate_record(&record, index + 1, &previous_hash)?;
        if line != record.canonical_json() || !event_ids.insert(record.event_id.clone()) {
            return Err(CommercialError(format!(
                "commercial ledger line {} is non-canonical or duplicates an event ID",
                index + 1
            )));
        }
        previous_hash = record.record_hash.clone();
        records.push(record);
    }
    let _ = provisionings(&records)?;
    let _ = subscriptions(&records)?;
    Ok(records)
}

fn verify_record_sequence(records: &[CommercialLedgerRecord]) -> Result<(), CommercialError> {
    let mut previous_hash = EMPTY_LEDGER_HASH.to_owned();
    let mut event_ids = BTreeSet::new();
    for (index, record) in records.iter().enumerate() {
        validate_record(record, index + 1, &previous_hash)?;
        if !event_ids.insert(&record.event_id) {
            return Err(CommercialError(
                "commercial ledger contains duplicate event IDs".to_owned(),
            ));
        }
        previous_hash = record.record_hash.clone();
    }
    Ok(())
}

fn validate_record(
    record: &CommercialLedgerRecord,
    line_number: usize,
    previous_hash: &str,
) -> Result<(), CommercialError> {
    if record.ledger_schema_version != COMMERCIAL_LEDGER_SCHEMA_VERSION
        || record.sequence
            != u64::try_from(line_number)
                .map_err(|_| CommercialError("commercial ledger line number overflow".to_owned()))?
        || record.prev_hash != previous_hash
    {
        return Err(CommercialError(format!(
            "commercial ledger chain failed at line {line_number}"
        )));
    }
    validate_ledger_input(
        &record.event_id,
        &record.event_type,
        &record.tenant_id,
        &record.occurred_at,
        &record.actor,
        &record.details,
    )?;
    validate_sha256("commercial ledger prev_hash", &record.prev_hash)?;
    validate_sha256("commercial ledger record_hash", &record.record_hash)?;
    let expected_hash = sha256(&canonical_ledger_body(LedgerBody {
        sequence: record.sequence,
        event_id: &record.event_id,
        event_type: &record.event_type,
        tenant_id: &record.tenant_id,
        occurred_at: &record.occurred_at,
        actor: &record.actor,
        details: &record.details,
        prev_hash: &record.prev_hash,
    }));
    if record.record_hash != expected_hash {
        return Err(CommercialError(format!(
            "commercial ledger hash failed at line {line_number}"
        )));
    }
    Ok(())
}

fn validate_ledger_input(
    event_id: &str,
    event_type: &str,
    tenant_id: &str,
    occurred_at: &str,
    actor: &str,
    details: &BTreeMap<String, String>,
) -> Result<(), CommercialError> {
    for (name, value) in [
        ("commercial event_id", event_id),
        ("commercial tenant_id", tenant_id),
        ("commercial actor", actor),
    ] {
        validate_canonical_id(name, value)?;
    }
    validate_utc_timestamp("commercial occurred_at", occurred_at)?;
    match event_type {
        "commercial.tenant_provisioned.v1" => {
            if details.len() != 4
                || !details.keys().map(String::as_str).eq([
                    "plan",
                    "retention_policy_id",
                    "self_hosted",
                    "workspace_id",
                ])
            {
                return Err(CommercialError(
                    "tenant provisioning has an invalid detail set".to_owned(),
                ));
            }
        }
        "commercial.subscription_observed.v1" => {
            if details.len() != 8
                || !details.keys().map(String::as_str).eq([
                    "effective_at",
                    "expires_at",
                    "external_customer_ref",
                    "payment_evidence_hash",
                    "payment_provider",
                    "plan",
                    "status",
                    "subscription_id",
                ])
            {
                return Err(CommercialError(
                    "subscription observation has an invalid detail set".to_owned(),
                ));
            }
        }
        _ => {
            return Err(CommercialError(
                "commercial event_type is not a supported typed v1 event".to_owned(),
            ));
        }
    }
    for (key, value) in details {
        validate_canonical_id("commercial detail key", key)?;
        if value.is_empty()
            || value.len() > 4_096
            || value.contains(['\r', '\n'])
            || key.contains("secret")
            || key.contains("token")
            || key.contains("password")
            || key.contains("credential")
        {
            return Err(CommercialError(
                "commercial ledger details must be concise, one-line, and non-secret".to_owned(),
            ));
        }
    }
    Ok(())
}

struct LedgerBody<'a> {
    sequence: u64,
    event_id: &'a str,
    event_type: &'a str,
    tenant_id: &'a str,
    occurred_at: &'a str,
    actor: &'a str,
    details: &'a BTreeMap<String, String>,
    prev_hash: &'a str,
}

fn canonical_ledger_body(body: LedgerBody<'_>) -> String {
    format!(
        "actor={}\ndetails={}\nevent_id={}\nevent_type={}\nledger_schema_version={}\noccurred_at={}\nprev_hash={}\nsequence={}\ntenant_id={}\n",
        body.actor,
        canonical_details_json(body.details),
        body.event_id,
        body.event_type,
        COMMERCIAL_LEDGER_SCHEMA_VERSION,
        body.occurred_at,
        body.prev_hash,
        body.sequence,
        body.tenant_id,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommercialLedgerRecordDocument {
    ledger_schema_version: u32,
    sequence: u64,
    event_id: String,
    event_type: String,
    tenant_id: String,
    occurred_at: String,
    actor: String,
    details: BTreeMap<String, String>,
    prev_hash: String,
    record_hash: String,
}

fn reject_symlink_path(label: &str, path: &Path) -> Result<(), CommercialError> {
    if path.exists()
        && fs::symlink_metadata(path)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
    {
        return Err(CommercialError(format!(
            "{label} path must not be a symbolic link"
        )));
    }
    Ok(())
}

fn validate_sha256(name: &str, value: &str) -> Result<(), CommercialError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte <= b'f'))
    {
        return Err(CommercialError(format!(
            "{name} must be lowercase SHA-256 hex"
        )));
    }
    Ok(())
}

fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn canonical_details_json(details: &BTreeMap<String, String>) -> String {
    format!(
        "{{{}}}",
        details
            .iter()
            .map(|(key, value)| format!("{}:{}", json_string(key), json_string(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("strings are serializable")
}

fn optional_json_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_owned())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(label: &str, value: &str) -> Result<Vec<u8>, CommercialError> {
    if value.len() % 2 != 0
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte <= b'f'))
    {
        return Err(CommercialError(format!(
            "{label} must be lowercase hexadecimal"
        )));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| CommercialError(format!("{label} must be lowercase hexadecimal")))
        })
        .collect()
}

fn io_error(error: std::io::Error) -> CommercialError {
    CommercialError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "follon-commercial-{}-{}-{}",
            std::process::id(),
            label,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn provisioning() -> TenantProvisioning {
        TenantProvisioning {
            tenant_id: "tenant.acme".to_owned(),
            workspace_id: "workspace.acme".to_owned(),
            plan: CommercialPlan::Professional,
            retention_policy_id: "retention.standard.1".to_owned(),
            self_hosted: false,
            provisioned_at: "2026-08-12T09:00:00Z".to_owned(),
        }
    }

    fn subscription(status: BillingStatus) -> SubscriptionObservation {
        SubscriptionObservation {
            subscription_id: "subscription.acme.001".to_owned(),
            tenant_id: "tenant.acme".to_owned(),
            plan: CommercialPlan::Professional,
            status,
            effective_at: "2026-08-12T09:00:00Z".to_owned(),
            expires_at: "2026-09-12T09:00:00Z".to_owned(),
            payment_provider: "stripe".to_owned(),
            external_customer_ref: "customer.acme.001".to_owned(),
            payment_evidence_hash: "a".repeat(64),
        }
    }

    #[test]
    fn paid_subscription_derives_a_durable_entitlement() {
        let directory = temp_directory("ledger");
        let path = directory.join("commercial.ndjson");
        let mut ledger = CommercialLedger::open(&path).unwrap();
        ledger
            .provision_tenant("event.provision.001", "operator.alice", &provisioning())
            .unwrap();
        ledger
            .record_subscription(
                "event.subscription.001",
                "billing.stripe",
                "2026-08-12T09:01:00Z",
                &subscription(BillingStatus::Paid),
            )
            .unwrap();
        let first =
            derive_entitlement(ledger.records(), "tenant.acme", "2026-08-12T10:00:00Z").unwrap();
        let second =
            derive_entitlement(ledger.records(), "tenant.acme", "2026-08-12T10:00:00Z").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.access, EntitlementAccess::Full);
        assert_eq!(first.maximum_members, 1);
        drop(ledger);
        assert_eq!(CommercialLedger::read_verified(&path).unwrap().len(), 2);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn later_observation_for_the_same_provider_subscription_supersedes_by_sequence() {
        let directory = temp_directory("renewal");
        let path = directory.join("commercial.ndjson");
        let mut ledger = CommercialLedger::open(&path).unwrap();
        ledger
            .provision_tenant("event.provision.001", "operator.alice", &provisioning())
            .unwrap();
        ledger
            .record_subscription(
                "event.subscription.001",
                "billing.stripe",
                "2026-08-12T09:01:00Z",
                &subscription(BillingStatus::Paid),
            )
            .unwrap();
        let mut grace = subscription(BillingStatus::Grace);
        grace.effective_at = "2026-08-20T09:00:00Z".to_owned();
        grace.expires_at = "2026-09-20T09:00:00Z".to_owned();
        ledger
            .record_subscription(
                "event.subscription.002",
                "billing.stripe",
                "2026-08-20T09:01:00Z",
                &grace,
            )
            .unwrap();
        assert_eq!(
            derive_entitlement(ledger.records(), "tenant.acme", "2026-08-21T10:00:00Z",)
                .unwrap()
                .access,
            EntitlementAccess::ReadOnly
        );
        drop(ledger);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn commercial_ledger_rejects_tampering_and_plan_mismatch() {
        let directory = temp_directory("tamper");
        let path = directory.join("commercial.ndjson");
        let mut ledger = CommercialLedger::open(&path).unwrap();
        ledger
            .provision_tenant("event.provision.001", "operator.alice", &provisioning())
            .unwrap();
        let mut mismatched = subscription(BillingStatus::Paid);
        mismatched.plan = CommercialPlan::Organization;
        ledger
            .record_subscription(
                "event.subscription.001",
                "billing.stripe",
                "2026-08-12T09:01:00Z",
                &mismatched,
            )
            .unwrap();
        assert!(
            derive_entitlement(ledger.records(), "tenant.acme", "2026-08-12T10:00:00Z",).is_err()
        );
        drop(ledger);
        let source = fs::read_to_string(&path).unwrap();
        fs::write(&path, source.replace("operator.alice", "operator.mallory")).unwrap();
        assert!(CommercialLedger::read_verified(&path).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    fn asset(
        asset_id: &str,
        relative_path: &str,
        classification: DataClassification,
        legal_hold: bool,
    ) -> DataAsset {
        DataAsset {
            asset_id: asset_id.to_owned(),
            tenant_id: "tenant.acme".to_owned(),
            relative_path: relative_path.to_owned(),
            classification,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            retain_until: "2026-08-01T00:00:00Z".to_owned(),
            subject_hashes: vec!["b".repeat(64)],
            legal_hold,
        }
    }

    #[test]
    fn retention_execution_requires_exact_reviewed_file_and_respects_hold() {
        let directory = temp_directory("retention");
        fs::write(directory.join("delete.txt"), "delete me").unwrap();
        fs::write(directory.join("hold.txt"), "do not delete").unwrap();
        let assets = vec![
            asset(
                "asset.delete",
                "delete.txt",
                DataClassification::OperationalMetadata,
                false,
            ),
            asset(
                "asset.hold",
                "hold.txt",
                DataClassification::OperationalMetadata,
                true,
            ),
        ];
        let plan =
            plan_expired_retention(&directory, &assets, "tenant.acme", "2026-08-12T10:00:00Z")
                .unwrap();
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.withheld_asset_ids, vec!["asset.hold"]);
        let receipt = execute_retention_candidate(
            &directory,
            &plan,
            "asset.delete",
            &plan.fingerprint().unwrap(),
            "2026-08-12T10:01:00Z",
            "operator.alice",
        )
        .unwrap();
        assert_eq!(receipt.asset_id, "asset.delete");
        assert!(!directory.join("delete.txt").exists());
        assert!(directory.join("hold.txt").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn privacy_erasure_never_selects_audit_evidence() {
        let directory = temp_directory("privacy");
        fs::write(directory.join("customer.txt"), "customer content").unwrap();
        fs::write(directory.join("audit.txt"), "audit evidence").unwrap();
        let request = PrivacyRequest {
            request_id: "privacy.request.001".to_owned(),
            tenant_id: "tenant.acme".to_owned(),
            subject_hash: "b".repeat(64),
            kind: PrivacyRequestKind::Erasure,
            requested_at: "2026-08-12T09:00:00Z".to_owned(),
        };
        let plan = plan_privacy_erasure(
            &directory,
            &[
                asset(
                    "asset.customer",
                    "customer.txt",
                    DataClassification::CustomerContent,
                    false,
                ),
                asset(
                    "asset.audit",
                    "audit.txt",
                    DataClassification::AuditEvidence,
                    false,
                ),
            ],
            &request,
            "2026-08-12T10:00:00Z",
        )
        .unwrap();
        assert_eq!(plan.candidates[0].asset_id, "asset.customer");
        assert_eq!(plan.withheld_asset_ids, vec!["asset.audit"]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn signed_release_and_self_host_readiness_fail_closed() {
        let directory = temp_directory("release");
        fs::write(directory.join("follon-replay"), "binary evidence").unwrap();
        let artifact_hash = sha256_file(&directory.join("follon-replay")).unwrap();
        let manifest = ReleaseManifest {
            release_manifest_schema_version: RELEASE_MANIFEST_SCHEMA_VERSION,
            release_id: "release.0.1.0".to_owned(),
            version: "0.1.0".to_owned(),
            created_at: "2026-08-12T09:00:00Z".to_owned(),
            source_revision: "a".repeat(40),
            sbom_sha256: "c".repeat(64),
            artifacts: vec![ReleaseArtifact {
                artifact_id: "follon.replay".to_owned(),
                relative_path: "follon-replay".to_owned(),
                sha256: artifact_hash,
                bytes: 15,
            }],
        };
        let source = manifest.canonical_json();
        let (mut private_key, trusted_key) = generate_release_keypair("release.key.001").unwrap();
        let signature = sign_release_manifest(
            &source,
            &private_key,
            "release.key.001",
            "2026-08-12T09:01:00Z",
        )
        .unwrap();
        private_key.fill(0);
        assert_eq!(
            verify_release_signature(&source, &signature, &trusted_key)
                .unwrap()
                .release_id,
            "release.0.1.0"
        );
        assert!(verify_release_artifacts(&manifest, &directory).is_ok());
        fs::write(directory.join("follon-replay"), "tampered").unwrap();
        assert!(verify_release_artifacts(&manifest, &directory).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
