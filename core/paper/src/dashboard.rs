//! Promotion status and the PAPER dashboard read model.

use serde::Serialize;

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
