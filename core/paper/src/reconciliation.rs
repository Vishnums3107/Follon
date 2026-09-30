//! Reconciliation issues, reports and incidents.

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
