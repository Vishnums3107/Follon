//! Counterfactual safety laboratory (DUR-02).
//!
//! Simulates "what if" interventions on frozen historical runs without mutating
//! production state or reusing production order identities.

use sha2::{Digest, Sha256};

use crate::BacktestError;

/// Type of intervention applied to a counterfactual run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CounterfactualInterventionType {
    /// Adjusting pre-trade risk price collar thresholds.
    RiskCollarAdjustment,
    /// Simulating additional transport or exchange gateway latency.
    NetworkLatencyInjection,
    /// Simulating missing or corrupted market data bars.
    DataBarCorruption,
    /// Shocking historical volatility or price movement.
    VolatilityShock,
}

impl CounterfactualInterventionType {
    /// Returns the canonical uppercase representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RiskCollarAdjustment => "RISK_COLLAR_ADJUSTMENT",
            Self::NetworkLatencyInjection => "NETWORK_LATENCY_INJECTION",
            Self::DataBarCorruption => "DATA_BAR_CORRUPTION",
            Self::VolatilityShock => "VOLATILITY_SHOCK",
        }
    }
}

/// A specific parameter intervention applied in a counterfactual scenario.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CounterfactualIntervention {
    /// Category of intervention.
    pub intervention_type: CounterfactualInterventionType,
    /// Name of the affected parameter.
    pub parameter_name: String,
    /// Original baseline parameter value.
    pub baseline_value: String,
    /// Counterfactual perturbed parameter value.
    pub counterfactual_value: String,
}

/// Performance and execution deltas between counterfactual and baseline runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CounterfactualDeltaMetrics {
    /// Change in total fills count (counterfactual - baseline).
    pub fill_count_delta: i64,
    /// Change in total realized P&L formatted as signed USD.
    pub pnl_delta_usd: String,
    /// Change in maximum drawdown in basis points.
    pub max_drawdown_delta_bps: i64,
    /// Change in risk rejection events count.
    pub risk_rejection_count_delta: i64,
}

/// A counterfactual scenario report matching `counterfactual-scenario.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CounterfactualScenario {
    /// Schema version (fixed at 1).
    pub scenario_schema_version: u32,
    /// Unique scenario identifier.
    pub scenario_id: String,
    /// Baseline historical run identity.
    pub baseline_run_id: String,
    /// Randomization seed used for perturbation.
    pub seed: u64,
    /// Set of applied interventions.
    pub interventions: Vec<CounterfactualIntervention>,
    /// Resulting performance and risk deltas.
    pub delta_metrics: CounterfactualDeltaMetrics,
    /// First event where the counterfactual run diverged from baseline.
    pub divergence_event_id: String,
    /// RFC3339 timestamp when the scenario was generated.
    pub created_at: String,
}

impl CounterfactualScenario {
    /// Formats the scenario as canonical JSON matching the v1 schema.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{");
        json.push_str("\"scenario_schema_version\":1,");
        json.push_str(&format!("\"scenario_id\":\"{}\",", self.scenario_id));
        json.push_str(&format!("\"baseline_run_id\":\"{}\",", self.baseline_run_id));
        json.push_str(&format!("\"seed\":{},", self.seed));

        // interventions
        json.push_str("\"interventions\":[");
        for (index, intervention) in self.interventions.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "{{\"intervention_type\":\"{}\",\"parameter_name\":\"{}\",\"baseline_value\":\"{}\",\"counterfactual_value\":\"{}\"}}",
                intervention.intervention_type.as_str(),
                intervention.parameter_name,
                intervention.baseline_value,
                intervention.counterfactual_value
            ));
        }
        json.push_str("],");

        // delta_metrics
        json.push_str(&format!(
            "\"delta_metrics\":{{\"fill_count_delta\":{},\"pnl_delta_usd\":\"{}\",\"max_drawdown_delta_bps\":{},\"risk_rejection_count_delta\":{}}},",
            self.delta_metrics.fill_count_delta,
            self.delta_metrics.pnl_delta_usd,
            self.delta_metrics.max_drawdown_delta_bps,
            self.delta_metrics.risk_rejection_count_delta
        ));

        json.push_str(&format!("\"divergence_event_id\":\"{}\",", self.divergence_event_id));
        json.push_str(&format!("\"created_at\":\"{}\"", self.created_at));
        json.push('}');
        json
    }
}

/// Engine for evaluating counterfactual interventions against frozen baseline runs.
pub struct CounterfactualEngine;

impl CounterfactualEngine {
    /// Computes counterfactual divergence metrics comparing a baseline and an intervention.
    pub fn evaluate_scenario(
        baseline_run_id: &str,
        seed: u64,
        interventions: Vec<CounterfactualIntervention>,
        baseline_fills: i64,
        counterfactual_fills: i64,
        baseline_pnl_cents: i64,
        counterfactual_pnl_cents: i64,
        baseline_max_drawdown_bps: i64,
        counterfactual_max_drawdown_bps: i64,
        baseline_rejections: i64,
        counterfactual_rejections: i64,
        divergence_event_id: &str,
        created_at: &str,
    ) -> Result<CounterfactualScenario, BacktestError> {
        if interventions.is_empty() {
            return Err(BacktestError("counterfactual scenario requires at least one intervention".to_owned()));
        }

        let pnl_diff_cents = counterfactual_pnl_cents - baseline_pnl_cents;
        let pnl_delta_usd = format!("{:.2}", (pnl_diff_cents as f64) / 100.0);

        let delta_metrics = CounterfactualDeltaMetrics {
            fill_count_delta: counterfactual_fills - baseline_fills,
            pnl_delta_usd,
            max_drawdown_delta_bps: counterfactual_max_drawdown_bps - baseline_max_drawdown_bps,
            risk_rejection_count_delta: counterfactual_rejections - baseline_rejections,
        };

        let digest = format!("{:x}", Sha256::digest(format!("{}:{}:{}", baseline_run_id, seed, created_at).as_bytes()));
        let scenario_id = format!("cf.{}", &digest[..16]);

        Ok(CounterfactualScenario {
            scenario_schema_version: 1,
            scenario_id,
            baseline_run_id: baseline_run_id.to_owned(),
            seed,
            interventions,
            delta_metrics,
            divergence_event_id: divergence_event_id.to_owned(),
            created_at: created_at.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_counterfactual_risk_intervention() {
        let interventions = vec![CounterfactualIntervention {
            intervention_type: CounterfactualInterventionType::RiskCollarAdjustment,
            parameter_name: "price_collar_bps".to_owned(),
            baseline_value: "100".to_owned(),
            counterfactual_value: "50".to_owned(),
        }];

        let scenario = CounterfactualEngine::evaluate_scenario(
            "run.baseline.001",
            42,
            interventions,
            100,
            92,
            500_000,
            485_000,
            120,
            95,
            2,
            10,
            "evt.diverge.001",
            "2026-09-01T15:00:00Z",
        )
        .unwrap();

        assert_eq!(scenario.scenario_schema_version, 1);
        assert_eq!(scenario.delta_metrics.fill_count_delta, -8);
        assert_eq!(scenario.delta_metrics.pnl_delta_usd, "-150.00");
        assert_eq!(scenario.delta_metrics.max_drawdown_delta_bps, -25);
        assert_eq!(scenario.delta_metrics.risk_rejection_count_delta, 8);

        let json = scenario.to_json();
        assert!(json.contains("\"scenario_schema_version\":1"));
        assert!(json.contains("\"RISK_COLLAR_ADJUSTMENT\""));
    }
}
