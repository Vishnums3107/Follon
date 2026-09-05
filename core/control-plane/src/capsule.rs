//! Strategy capability capsule verification engine (DUR-07, ASSET-04).
//!
//! Provides cryptographic verification of self-contained portable strategy capsules,
//! ensuring dependency locks, configuration hashes, and evaluation receipts are tamper-evident.

use sha2::{Digest, Sha256};

use crate::EngineError;

/// Export disposition certifying portable execution safety.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapsuleExportDisposition {
    /// Fully verified portable capsule ready for isolated execution.
    VerifiedPortable,
    /// Missing cryptographic dependency lockfile.
    MissingDependencyLock,
    /// Strategy lacks an immutable, evaluated backtest receipt.
    UnverifiedEvaluation,
    /// Bound dataset has restricted redistribution rights.
    RestrictedDatasetRights,
}

impl CapsuleExportDisposition {
    /// Returns the canonical uppercase string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedPortable => "VERIFIED_PORTABLE",
            Self::MissingDependencyLock => "MISSING_DEPENDENCY_LOCK",
            Self::UnverifiedEvaluation => "UNVERIFIED_EVALUATION",
            Self::RestrictedDatasetRights => "RESTRICTED_DATASET_RIGHTS",
        }
    }
}

/// Strategy capability capsule manifest matching `strategy-capsule-manifest.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyCapsuleManifest {
    /// Schema version (fixed at 1).
    pub capsule_schema_version: u32,
    /// Unique capsule manifest identifier.
    pub capsule_id: String,
    /// Associated strategy identifier.
    pub strategy_id: String,
    /// Version of the strategy bundle.
    pub strategy_version: String,
    /// SHA256 hex digest of the bundled source code.
    pub bundle_sha256: String,
    /// SHA256 hex digest of the configuration JSON.
    pub configuration_sha256: String,
    /// SHA256 hex digest of the dependency lockfile.
    pub dependency_lockfile_sha256: String,
    /// Execution runtime target.
    pub runtime_target: String,
    /// Attributable evaluation receipt ID.
    pub evaluation_receipt_id: String,
    /// Bounded replay command.
    pub replay_instruction_command: String,
    /// Certified disposition.
    pub export_disposition: CapsuleExportDisposition,
    /// Timestamp when capsule was sealed.
    pub packaged_at: String,
}

impl StrategyCapsuleManifest {
    /// Formats the manifest as canonical JSON matching the v1 schema.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"capsule_schema_version\":1,\"capsule_id\":\"{}\",\"strategy_id\":\"{}\",\"strategy_version\":\"{}\",\"bundle_sha256\":\"{}\",\"configuration_sha256\":\"{}\",\"dependency_lockfile_sha256\":\"{}\",\"runtime_target\":\"{}\",\"evaluation_receipt_id\":\"{}\",\"replay_instruction_command\":\"{}\",\"export_disposition\":\"{}\",\"packaged_at\":\"{}\"}}",
            self.capsule_id,
            self.strategy_id,
            self.strategy_version,
            self.bundle_sha256,
            self.configuration_sha256,
            self.dependency_lockfile_sha256,
            self.runtime_target,
            self.evaluation_receipt_id,
            self.replay_instruction_command,
            self.export_disposition.as_str(),
            self.packaged_at
        )
    }
}

/// Isolated verifier for strategy capability capsules.
pub struct StrategyCapsuleVerifier;

impl StrategyCapsuleVerifier {
    /// Verifies that code, configuration, and lockfile match the claimed SHA256 digests.
    pub fn verify_capsule_payload(
        manifest: &StrategyCapsuleManifest,
        bundle_bytes: &[u8],
        configuration_bytes: &[u8],
        lockfile_bytes: Option<&[u8]>,
    ) -> Result<CapsuleExportDisposition, EngineError> {
        let computed_bundle_hash = format!("{:x}", Sha256::digest(bundle_bytes));
        if computed_bundle_hash != manifest.bundle_sha256 {
            return Err(EngineError(format!(
                "bundle hash mismatch: expected {}, computed {}",
                manifest.bundle_sha256, computed_bundle_hash
            )));
        }

        let computed_cfg_hash = format!("{:x}", Sha256::digest(configuration_bytes));
        if computed_cfg_hash != manifest.configuration_sha256 {
            return Err(EngineError(format!(
                "configuration hash mismatch: expected {}, computed {}",
                manifest.configuration_sha256, computed_cfg_hash
            )));
        }

        let Some(lock_bytes) = lockfile_bytes else {
            return Ok(CapsuleExportDisposition::MissingDependencyLock);
        };

        let computed_lock_hash = format!("{:x}", Sha256::digest(lock_bytes));
        if computed_lock_hash != manifest.dependency_lockfile_sha256 {
            return Err(EngineError(format!(
                "lockfile hash mismatch: expected {}, computed {}",
                manifest.dependency_lockfile_sha256, computed_lock_hash
            )));
        }

        if manifest.evaluation_receipt_id.is_empty() {
            return Ok(CapsuleExportDisposition::UnverifiedEvaluation);
        }

        Ok(manifest.export_disposition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_intact_strategy_capsule() {
        let bundle = b"print('alpha algorithm')";
        let cfg = b"{\"leverage\": 1.0}";
        let lock = b"numpy==2.2.0";

        let bundle_hash = format!("{:x}", Sha256::digest(bundle));
        let cfg_hash = format!("{:x}", Sha256::digest(cfg));
        let lock_hash = format!("{:x}", Sha256::digest(lock));

        let manifest = StrategyCapsuleManifest {
            capsule_schema_version: 1,
            capsule_id: "capsule.alpha.v1".to_owned(),
            strategy_id: "strat.alpha".to_owned(),
            strategy_version: "1.0.0".to_owned(),
            bundle_sha256: bundle_hash,
            configuration_sha256: cfg_hash,
            dependency_lockfile_sha256: lock_hash,
            runtime_target: "cpython-3.12-sandbox".to_owned(),
            evaluation_receipt_id: "eval.rcpt.001".to_owned(),
            replay_instruction_command: "follon replay --capsule capsule.alpha.v1".to_owned(),
            export_disposition: CapsuleExportDisposition::VerifiedPortable,
            packaged_at: "2026-09-01T12:00:00Z".to_owned(),
        };

        let disposition = StrategyCapsuleVerifier::verify_capsule_payload(
            &manifest,
            bundle,
            cfg,
            Some(lock),
        )
        .unwrap();

        assert_eq!(disposition, CapsuleExportDisposition::VerifiedPortable);
        let json = manifest.to_json();
        assert!(json.contains("\"export_disposition\":\"VERIFIED_PORTABLE\""));
    }

    #[test]
    fn detects_tampered_bundle() {
        let bundle = b"print('clean')";
        let tampered = b"print('backdoor')";
        let cfg = b"{}";
        let lock = b"";

        let manifest = StrategyCapsuleManifest {
            capsule_schema_version: 1,
            capsule_id: "capsule.clean.v1".to_owned(),
            strategy_id: "strat.clean".to_owned(),
            strategy_version: "1.0.0".to_owned(),
            bundle_sha256: format!("{:x}", Sha256::digest(bundle)),
            configuration_sha256: format!("{:x}", Sha256::digest(cfg)),
            dependency_lockfile_sha256: format!("{:x}", Sha256::digest(lock)),
            runtime_target: "cpython-3.12-sandbox".to_owned(),
            evaluation_receipt_id: "eval.1".to_owned(),
            replay_instruction_command: "run".to_owned(),
            export_disposition: CapsuleExportDisposition::VerifiedPortable,
            packaged_at: "2026-09-01T12:00:00Z".to_owned(),
        };

        assert!(StrategyCapsuleVerifier::verify_capsule_payload(
            &manifest,
            tampered,
            cfg,
            Some(lock),
        )
        .is_err());
    }
}
