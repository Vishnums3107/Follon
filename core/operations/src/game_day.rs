//! Recovery game-day compiler (DUR-08, LIFE-06).
//!
//! Turns simulated catastrophic outages into bounded, verifiable drills
//! measuring actual recovery time objective (RTO) and recovery point objective (RPO).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::OperationsError;

/// Categorized fault scenarios injected during disaster recovery drills.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InjectedFault {
    /// Sudden disk exhaustion triggering abrupt SIGKILL process loss.
    DiskPressureAbruptTermination,
    /// Upstream broker execution fill dropped or never acknowledged.
    DroppedBrokerExecutionAck,
    /// Persistence checkpoint corrupted or missing header block.
    CorruptPostgresCheckpoint,
    /// Network partition testing single-writer fencing epochs.
    SplitBrainHostPartition,
}

impl InjectedFault {
    /// Returns the canonical uppercase representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DiskPressureAbruptTermination => "DISK_PRESSURE_ABRUPT_TERMINATION",
            Self::DroppedBrokerExecutionAck => "DROPPED_BROKER_EXECUTION_ACK",
            Self::CorruptPostgresCheckpoint => "CORRUPT_POSTGRES_CHECKPOINT",
            Self::SplitBrainHostPartition => "SPLIT_BRAIN_HOST_PARTITION",
        }
    }
}

/// Recovery drill outcome record matching `recovery-drill-result.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RecoveryDrillResult {
    /// Schema version (fixed at 1).
    pub drill_schema_version: u32,
    /// Unique drill outcome identifier.
    pub drill_id: String,
    /// Human-readable scenario name.
    pub scenario_name: String,
    /// Injected failure condition.
    pub injected_fault: InjectedFault,
    /// Measured recovery duration in elapsed seconds.
    pub measured_rto_seconds: u64,
    /// Allowed RTO ceiling in seconds (e.g., 900s for 15-minute RTO).
    pub target_rto_seconds: u64,
    /// Number of durable events lost during restart.
    pub measured_rpo_events_lost: u64,
    /// Allowed RPO ceiling (strictly 0 for zero-data-loss kernels).
    pub target_rpo_events_lost: u64,
    /// Whether independent ledger/state hashes reconciled identically post-restore.
    pub reconciliation_hash_matched: bool,
    /// Overall drill pass status (met RTO, met RPO, hashes matched).
    pub drill_passed: bool,
    /// RFC3339 timestamp when the drill concluded.
    pub executed_at: String,
}

/// Compiler that plans and certifies disaster recovery drills.
pub struct GameDayCompiler;

impl GameDayCompiler {
    /// Compiles measured drill telemetry into an immutable `RecoveryDrillResult`.
    pub fn compile_drill(
        scenario_name: &str,
        injected_fault: InjectedFault,
        measured_rto_seconds: u64,
        target_rto_seconds: u64,
        measured_rpo_events_lost: u64,
        target_rpo_events_lost: u64,
        reconciliation_hash_matched: bool,
        executed_at: &str,
    ) -> Result<RecoveryDrillResult, OperationsError> {
        let drill_passed = measured_rto_seconds <= target_rto_seconds
            && measured_rpo_events_lost <= target_rpo_events_lost
            && reconciliation_hash_matched;

        let digest = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}:{}", scenario_name, injected_fault.as_str(), executed_at).as_bytes())
        );
        let drill_id = format!("drill.{}", &digest[..16]);

        Ok(RecoveryDrillResult {
            drill_schema_version: 1,
            drill_id,
            scenario_name: scenario_name.to_owned(),
            injected_fault,
            measured_rto_seconds,
            target_rto_seconds,
            measured_rpo_events_lost,
            target_rpo_events_lost,
            reconciliation_hash_matched,
            drill_passed,
            executed_at: executed_at.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_passing_disk_pressure_drill() {
        let result = GameDayCompiler::compile_drill(
            "Disk pressure kill drill",
            InjectedFault::DiskPressureAbruptTermination,
            120,
            900,
            0,
            0,
            true,
            "2026-09-01T17:00:00Z",
        )
        .unwrap();

        assert_eq!(result.drill_schema_version, 1);
        assert!(result.drill_passed);
        assert_eq!(result.measured_rto_seconds, 120);
        assert_eq!(result.measured_rpo_events_lost, 0);
        assert!(result.reconciliation_hash_matched);
    }

    #[test]
    fn fails_drill_when_reconciliation_hash_mismatches() {
        let result = GameDayCompiler::compile_drill(
            "Corrupt checkpoint recovery",
            InjectedFault::CorruptPostgresCheckpoint,
            300,
            900,
            0,
            0,
            false, // hash mismatch
            "2026-09-01T17:00:00Z",
        )
        .unwrap();

        assert!(!result.drill_passed);
    }
}
