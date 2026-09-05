//! Gateway qualification matrix (DUR-10).
//!
//! Provides fine-grained qualification of broker gateways across order types,
//! execution capabilities, asset classes, and latency bounds.

use follon_domain::OrderIntent;
use serde::{Deserialize, Serialize};

/// Error returned when an order intent demands an uncertified gateway capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayQualificationError(pub String);

impl std::fmt::Display for GatewayQualificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for GatewayQualificationError {}

/// Qualification certification state for an individual capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QualificationState {
    /// Fully qualified and tested in the active environment.
    Certified,
    /// Provisionally approved with restricted size or monitoring.
    Provisional,
    /// Explicitly rejected or unverified.
    Rejected,
}

/// A certified capability within a gateway route.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct QualifiedCapability {
    /// Stable capability identifier (e.g., "order.limit", "order.bracket").
    pub capability_id: String,
    /// Certified asset class (e.g., "US_EQUITY", "EQUITY_OPTION").
    pub asset_class: String,
    /// Current qualification state.
    pub qualification_state: QualificationState,
    /// Measured 99th percentile roundtrip latency in milliseconds.
    pub measured_p99_latency_ms: u64,
    /// Maximum supported algorithm slices.
    pub max_supported_slices: u32,
    /// Historical reconciliation accuracy in basis points (10000 = 100.00%).
    pub reconciliation_accuracy_bps: u32,
}

/// Gateway qualification matrix matching `gateway-qualification-matrix.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct GatewayQualificationMatrix {
    /// Schema version (fixed at 1).
    pub matrix_schema_version: u32,
    /// Unique matrix identifier.
    pub matrix_id: String,
    /// Environment where qualification was measured.
    pub environment: String,
    /// Target gateway identifier.
    pub gateway_id: String,
    /// List of evaluated capabilities.
    pub qualified_capabilities: Vec<QualifiedCapability>,
    /// Single-writer fencing epoch.
    pub fencing_epoch: u64,
    /// RFC3339 timestamp of qualification.
    pub evaluated_at: String,
    /// RFC3339 timestamp when qualification expires.
    pub expires_at: String,
}

impl GatewayQualificationMatrix {
    /// Returns true if the requested capability is certified for the asset class.
    pub fn is_capability_qualified(&self, capability_id: &str, asset_class: &str) -> bool {
        self.qualified_capabilities.iter().any(|cap| {
            cap.capability_id == capability_id
                && cap.asset_class == asset_class
                && cap.qualification_state == QualificationState::Certified
        })
    }

    /// Validates that an order intent requests only certified capabilities.
    pub fn check_order_intent(
        &self,
        intent: &OrderIntent,
        asset_class: &str,
    ) -> Result<(), GatewayQualificationError> {
        let required_cap = match intent.order_type {
            follon_domain::OrderType::Market => "order.market",
            follon_domain::OrderType::Limit => "order.limit",
        };

        if !self.is_capability_qualified(required_cap, asset_class) {
            return Err(GatewayQualificationError(format!(
                "gateway {} is not certified for capability '{}' on asset class '{}'",
                self.gateway_id, required_cap, asset_class
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use follon_domain::{Decimal, Side, TimeInForce};

    fn sample_matrix() -> GatewayQualificationMatrix {
        GatewayQualificationMatrix {
            matrix_schema_version: 1,
            matrix_id: "gqm.ibkr.paper.v1".to_owned(),
            environment: "PAPER".to_owned(),
            gateway_id: "gateway.ibkr.paper".to_owned(),
            qualified_capabilities: vec![
                QualifiedCapability {
                    capability_id: "order.limit".to_owned(),
                    asset_class: "US_EQUITY".to_owned(),
                    qualification_state: QualificationState::Certified,
                    measured_p99_latency_ms: 12,
                    max_supported_slices: 10,
                    reconciliation_accuracy_bps: 10_000,
                },
                QualifiedCapability {
                    capability_id: "order.market".to_owned(),
                    asset_class: "US_EQUITY".to_owned(),
                    qualification_state: QualificationState::Certified,
                    measured_p99_latency_ms: 8,
                    max_supported_slices: 1,
                    reconciliation_accuracy_bps: 10_000,
                },
                QualifiedCapability {
                    capability_id: "order.bracket".to_owned(),
                    asset_class: "US_EQUITY".to_owned(),
                    qualification_state: QualificationState::Provisional,
                    measured_p99_latency_ms: 45,
                    max_supported_slices: 3,
                    reconciliation_accuracy_bps: 9_950,
                },
                QualifiedCapability {
                    capability_id: "order.limit".to_owned(),
                    asset_class: "EQUITY_OPTION".to_owned(),
                    qualification_state: QualificationState::Rejected,
                    measured_p99_latency_ms: 999,
                    max_supported_slices: 1,
                    reconciliation_accuracy_bps: 0,
                },
            ],
            fencing_epoch: 1,
            evaluated_at: "2026-09-01T12:00:00Z".to_owned(),
            expires_at: "2026-10-01T12:00:00Z".to_owned(),
        }
    }

    #[test]
    fn validates_certified_limit_order() {
        let matrix = sample_matrix();
        let intent = OrderIntent {
            intent_id: "intent.1".to_owned(),
            strategy_id: "strat.1".to_owned(),
            strategy_version: "1.0.0".to_owned(),
            account_id: "acct.paper".to_owned(),
            instrument_id: "AAPL".to_owned(),
            side: Side::Buy,
            order_type: follon_domain::OrderType::Limit,
            quantity: Decimal::from_integer(10).unwrap(),
            limit_price: Some(Decimal::from_integer(150).unwrap()),
            time_in_force: TimeInForce::Day,
            environment: "PAPER".to_owned(),
            created_at: "2026-09-01T14:30:00Z".to_owned(),
            correlation_id: "corr.1".to_owned(),
            rationale: "test".to_owned(),
            configuration_version: "cfg.1".to_owned(),
        };

        assert!(matrix.check_order_intent(&intent, "US_EQUITY").is_ok());
    }

    #[test]
    fn rejects_uncertified_option_route() {
        let matrix = sample_matrix();
        let intent = OrderIntent {
            intent_id: "intent.2".to_owned(),
            strategy_id: "strat.1".to_owned(),
            strategy_version: "1.0.0".to_owned(),
            account_id: "acct.paper".to_owned(),
            instrument_id: "AAPL260918C00150000".to_owned(),
            side: Side::Buy,
            order_type: follon_domain::OrderType::Limit,
            quantity: Decimal::from_integer(1).unwrap(),
            limit_price: Some(Decimal::from_integer(5).unwrap()),
            time_in_force: TimeInForce::Day,
            environment: "PAPER".to_owned(),
            created_at: "2026-09-01T14:30:00Z".to_owned(),
            correlation_id: "corr.2".to_owned(),
            rationale: "options test".to_owned(),
            configuration_version: "cfg.1".to_owned(),
        };

        let err = matrix.check_order_intent(&intent, "EQUITY_OPTION").unwrap_err();
        assert!(err.0.contains("not certified"));
    }
}
