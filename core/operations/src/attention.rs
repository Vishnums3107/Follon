//! Attention budget controller (DUR-05, SOLO-05).
//!
//! Protects the solo operator from cognitive overload and alert fatigue by
//! tracking interruptions, merging cascading alerts, suppressing duplicate alarms,
//! and enforcing strict non-suppression of critical risk and reconciliation events.

use serde::{Deserialize, Serialize};

use crate::AlertSeverity;

/// Attention budget calculation report matching `attention-budget.schema.json`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AttentionBudget {
    /// Schema version (fixed at 1).
    pub budget_schema_version: u32,
    /// Unique budget report identifier.
    pub budget_id: String,
    /// Calendar date of the operating session.
    pub session_date: String,
    /// Cognitive load score in basis points (10000 = 100.00% capacity exhausted).
    pub cognitive_load_score_bps: u32,
    /// Frequency of operator interruptions per hour.
    pub interruptions_per_hour: f64,
    /// Count of currently active, unacknowledged alarms.
    pub active_alarms_count: u32,
    /// Total count of cascading duplicate alerts suppressed.
    pub suppressed_duplicates_count: u32,
    /// Critical tasks escalated requiring urgent operator intervention.
    pub escalated_critical_tasks: Vec<String>,
    /// Whether the operator's cognitive attention budget is exhausted.
    pub budget_exhausted: bool,
    /// RFC3339 timestamp of calculation.
    pub calculated_at: String,
}

/// Controller managing operator attention, alert clustering, and cognitive load budgets.
pub struct AttentionBudgetController {
    max_interruptions_per_hour: f64,
}

impl AttentionBudgetController {
    /// Creates a controller with the specified hourly interruption tolerance threshold.
    pub fn new(max_interruptions_per_hour: f64) -> Self {
        Self {
            max_interruptions_per_hour,
        }
    }

    /// Evaluates current session alerts and computes an immutable attention budget.
    pub fn calculate_budget(
        &self,
        session_date: &str,
        session_duration_hours: f64,
        raw_alarm_count: u32,
        suppressed_count: u32,
        critical_task_ids: Vec<String>,
        calculated_at: &str,
    ) -> AttentionBudget {
        let active_alarms_count = raw_alarm_count.saturating_sub(suppressed_count);
        let safe_hours = if session_duration_hours <= 0.0 { 1.0 } else { session_duration_hours };
        let interruptions_per_hour = (active_alarms_count as f64) / safe_hours;

        // Cognitive load ratio
        let load_ratio = interruptions_per_hour / self.max_interruptions_per_hour.max(1.0);
        let cognitive_load_score_bps = ((load_ratio.min(1.0)) * 10_000.0) as u32;
        let budget_exhausted = interruptions_per_hour > self.max_interruptions_per_hour;

        let budget_id = format!("attn.{}.{}", session_date.replace('-', ""), active_alarms_count);

        AttentionBudget {
            budget_schema_version: 1,
            budget_id,
            session_date: session_date.to_owned(),
            cognitive_load_score_bps,
            interruptions_per_hour: (interruptions_per_hour * 100.0).round() / 100.0,
            active_alarms_count,
            suppressed_duplicates_count: suppressed_count,
            escalated_critical_tasks: critical_task_ids,
            budget_exhausted,
            calculated_at: calculated_at.to_owned(),
        }
    }

    /// Determines whether an alert may be suppressed to protect operator focus.
    ///
    /// CRITICAL invariant: Critical risk, reconciliation discrepancies, and kill-switch
    /// alarms can NEVER be suppressed.
    pub fn can_suppress_alert(&self, severity: AlertSeverity, is_duplicate: bool) -> bool {
        match severity {
            AlertSeverity::Critical => false,
            AlertSeverity::Warning => is_duplicate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_alarms_cannot_be_suppressed() {
        let controller = AttentionBudgetController::new(10.0);
        assert!(!controller.can_suppress_alert(AlertSeverity::Critical, true));
        assert!(controller.can_suppress_alert(AlertSeverity::Warning, true));
        assert!(!controller.can_suppress_alert(AlertSeverity::Warning, false));
    }

    #[test]
    fn calculates_healthy_attention_budget() {
        let controller = AttentionBudgetController::new(12.0);
        let budget = controller.calculate_budget(
            "2026-09-01",
            4.0,
            24,
            12,
            vec!["task.risk.collar".to_owned()],
            "2026-09-01T16:00:00Z",
        );

        assert_eq!(budget.budget_schema_version, 1);
        assert_eq!(budget.active_alarms_count, 12);
        assert_eq!(budget.suppressed_duplicates_count, 12);
        assert_eq!(budget.interruptions_per_hour, 3.0);
        assert_eq!(budget.cognitive_load_score_bps, 2500); // 3/12 = 25%
        assert!(!budget.budget_exhausted);
    }
}
