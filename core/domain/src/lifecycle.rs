//! Risk decisions, order lifecycle states, fills, positions, P&L and audit trails.

use crate::*;

/// Result of a versioned pre-trade risk policy evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskDecision {
    /// Decision identity.
    pub decision_id: String,
    /// Intent evaluated by this decision.
    pub intent_id: String,
    /// Whether the intent may create an executable order.
    pub approved: bool,
    /// Machine-readable rule outcomes.
    pub reason_codes: Vec<String>,
    /// Stable policy identity and version.
    pub policy_version: String,
    /// UTC replay-clock decision time.
    pub decided_at: String,
    /// Workflow correlation identity.
    pub correlation_id: String,
    /// Identity of the evaluating actor.
    pub actor: String,
    /// Deterministic limit values used during the decision.
    pub evaluated_limits: String,
}

/// Complete OMS lifecycle state set, including the safety `UNKNOWN` state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderState {
    /// Order was created from an approved intent.
    Created,
    /// Awaiting risk assessment.
    PendingRisk,
    /// Risk rejected the intent.
    RiskRejected,
    /// Risk approved the intent.
    Approved,
    /// Waiting for adapter or simulator submission.
    PendingSubmit,
    /// Submission was sent.
    Submitted,
    /// External system acknowledged submission.
    Acknowledged,
    /// A portion of the quantity executed.
    PartiallyFilled,
    /// All requested quantity executed.
    Filled,
    /// Cancellation is in progress.
    PendingCancel,
    /// A risk-preserving broker modification is in progress.
    PendingReplace,
    /// Cancellation completed.
    Cancelled,
    /// External system rejected the order.
    Rejected,
    /// Time in force elapsed.
    Expired,
    /// Submission outcome is ambiguous and needs reconciliation.
    Unknown,
}

impl OrderState {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::PendingRisk => "PENDING_RISK",
            Self::RiskRejected => "RISK_REJECTED",
            Self::Approved => "APPROVED",
            Self::PendingSubmit => "PENDING_SUBMIT",
            Self::Submitted => "SUBMITTED",
            Self::Acknowledged => "ACKNOWLEDGED",
            Self::PartiallyFilled => "PARTIALLY_FILLED",
            Self::Filled => "FILLED",
            Self::PendingCancel => "PENDING_CANCEL",
            Self::PendingReplace => "PENDING_REPLACE",
            Self::Cancelled => "CANCELLED",
            Self::Rejected => "REJECTED",
            Self::Expired => "EXPIRED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// A validated OMS state transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderStateChange {
    /// OMS order identity.
    pub order_id: String,
    /// State before the transition, if an order is newly created.
    pub previous_state: Option<OrderState>,
    /// New lifecycle state.
    pub new_state: OrderState,
    /// Reason for the state change.
    pub reason: String,
}

/// A normalized simulator or broker execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fill {
    /// Execution identity, unique per fill.
    pub execution_id: String,
    /// OMS order identity.
    pub order_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Executed direction.
    pub side: Side,
    /// Exact executed quantity.
    pub quantity: Decimal,
    /// Exact execution price.
    pub price: Decimal,
    /// Exact commission/fees in the position currency.
    pub fee: Decimal,
    /// UTC execution time.
    pub executed_at: String,
}

/// Rebuildable portfolio position projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionSnapshot {
    /// Account identity.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Signed position quantity.
    pub quantity: Decimal,
    /// Exact average cost.
    pub average_cost: Decimal,
    /// Exact realized P&L.
    pub realized_pnl: Decimal,
}

/// Rebuildable current P&L projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PnlSnapshot {
    /// Account identity.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Mark used for the valuation.
    pub mark_price: Decimal,
    /// Exact realized P&L.
    pub realized_pnl: Decimal,
    /// Exact unrealized P&L.
    pub unrealized_pnl: Decimal,
    /// Exact total P&L.
    pub total_pnl: Decimal,
}

/// Evidence linking a completed workflow to its primary events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditTrail {
    /// Workflow correlation identity.
    pub correlation_id: String,
    /// Stable ordered event IDs in the trail.
    pub event_ids: Vec<String>,
    /// Operator-readable statement of the completed transition.
    pub summary: String,
}
