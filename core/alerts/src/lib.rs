//! Alert definition and routing for critical system events.
//!
//! Provides a centralized mechanism to dispatch operational alerts to
//! external services like webhooks while preserving deterministic boundaries.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Alert severity level.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Severity {
    /// Informational message, normal operation.
    Info,
    /// Warning, operation continues but requires attention.
    Warning,
    /// Critical failure or invariant violation.
    Critical,
}

/// Category of the alert for routing and filtering.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AlertCategory {
    /// Risk limit or kill-switch activation.
    Risk,
    /// Component or service health failure.
    System,
    /// Market data staleness or connection drop.
    MarketData,
    /// Accounting or broker statement discrepancy.
    Reconciliation,
}

/// A structured operational alert.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    /// Unique identity for the alert type.
    pub alert_id: String,
    /// Canonical source component (e.g., "core/risk").
    pub source: String,
    /// Category of alert.
    pub category: AlertCategory,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable description.
    pub message: String,
    /// Optional context payload in JSON string format.
    pub context_json: Option<String>,
}

impl fmt::Display for Alert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{:?}] {} ({}): {}",
            self.severity, self.alert_id, self.source, self.message
        )
    }
}

/// Alert routing and dispatching error.
#[derive(Debug)]
pub struct AlertError(pub String);

impl fmt::Display for AlertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AlertError {}

/// Trait for dispatching alerts.
#[async_trait::async_trait]
pub trait AlertDispatcher: Send + Sync {
    /// Dispatches an alert asynchronously.
    async fn dispatch(&self, alert: &Alert) -> Result<(), AlertError>;
}

/// Webhook dispatcher for sending alerts as JSON POST requests.
pub struct WebhookDispatcher {
    webhook_url: String,
    client: Client,
}

impl WebhookDispatcher {
    /// Creates a new webhook dispatcher.
    pub fn new(webhook_url: String) -> Self {
        Self {
            webhook_url,
            client: Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl AlertDispatcher for WebhookDispatcher {
    async fn dispatch(&self, alert: &Alert) -> Result<(), AlertError> {
        self.client
            .post(&self.webhook_url)
            .json(alert)
            .send()
            .await
            .map_err(|e| AlertError(format!("failed to dispatch webhook: {}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_formatting() {
        let alert = Alert {
            alert_id: "RISK_LIMIT_BREACH".to_owned(),
            source: "core/risk".to_owned(),
            category: AlertCategory::Risk,
            severity: Severity::Critical,
            message: "Exposure exceeded".to_owned(),
            context_json: None,
        };
        assert_eq!(
            alert.to_string(),
            "[Critical] RISK_LIMIT_BREACH (core/risk): Exposure exceeded"
        );
    }
}
