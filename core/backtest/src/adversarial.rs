//! Adversarial research gate (DUR-06).
//!
//! Aggregates and certifies the results of the 5 standardized adversarial
//! stress probes (look-ahead leakage, noise fragility, cost vulnerability,
//! parameter cliff overfitting, and regime shifts) into a single composite
//! robustness score and a pass/fail gate decision.
//!
//! This module does **not** itself execute a backtest, inject noise/latency/
//! cost perturbations, time-shift any indicator, or otherwise run a stress
//! test — it only aggregates and certifies whatever `passed` /
//! `degradation_bps` figures its caller supplies. `run_adversarial` in
//! `apps/cli/src/backtest.rs` calls it two different ways: its `execute`
//! input mode actually drives the built-in deterministic strategy through 5
//! genuine perturbed replays — one per standardized probe (see
//! `execute_adversarial_probes`) — and computes `passed`/`degradation_bps`
//! from their real output; its `probes`-only mode instead passes through
//! operator-attested figures, assumed to already reflect a real stress test
//! executed by a separate tool or human operator. This gate cannot itself
//! distinguish the two — its guarantee is limited to deterministic,
//! tamper-evident aggregation and certification of whatever probe results it
//! is handed.

use sha2::{Digest, Sha256};

use crate::BacktestError;

/// Operator-attested result of a single stress probe, supplied by the caller.
/// This module does not compute or verify these values — it only aggregates
/// and certifies whatever numbers it is given.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdversarialProbeResult {
    /// Standardized probe identifier.
    pub probe_name: String,
    /// Detailed description of the probe test.
    pub probe_description: String,
    /// Whether the strategy satisfied the probe's tolerance threshold.
    pub passed: bool,
    /// Observed performance degradation in basis points.
    pub degradation_bps: i64,
    /// Maximum allowed degradation in basis points before failing.
    pub threshold_bps: i64,
}

/// Adversarial evaluation report matching `adversarial-evaluation.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdversarialEvaluation {
    /// Schema version (fixed at 1).
    pub adversarial_schema_version: u32,
    /// Unique evaluation identifier.
    pub evaluation_id: String,
    /// Strategy version tested.
    pub strategy_version: String,
    /// Results for the 5 standardized adversarial probes.
    pub probes: Vec<AdversarialProbeResult>,
    /// Overall composite robustness score in basis points (10000 = 100.00%).
    pub composite_robustness_score_bps: u32,
    /// Whether all mandatory probes passed and the strategy cleared the gate.
    pub gate_passed: bool,
    /// Explicit blocking failure reasons if the gate was failed.
    pub blocking_failure_reasons: Vec<String>,
    /// RFC3339 timestamp when evaluation completed.
    pub evaluated_at: String,
}

impl AdversarialEvaluation {
    /// Formats the evaluation report as canonical JSON matching the v1 schema.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{");
        json.push_str("\"adversarial_schema_version\":1,");
        json.push_str(&format!("\"evaluation_id\":\"{}\",", self.evaluation_id));
        json.push_str(&format!(
            "\"strategy_version\":\"{}\",",
            self.strategy_version
        ));

        // probes
        json.push_str("\"probes\":[");
        for (index, probe) in self.probes.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "{{\"probe_name\":\"{}\",\"probe_description\":\"{}\",\"passed\":{},\"degradation_bps\":{},\"threshold_bps\":{}}}",
                probe.probe_name, probe.probe_description, probe.passed, probe.degradation_bps, probe.threshold_bps
            ));
        }
        json.push_str("],");

        json.push_str(&format!(
            "\"composite_robustness_score_bps\":{},",
            self.composite_robustness_score_bps
        ));
        json.push_str(&format!("\"gate_passed\":{},", self.gate_passed));

        // blocking_failure_reasons
        json.push_str("\"blocking_failure_reasons\":[");
        for (index, reason) in self.blocking_failure_reasons.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&format!("\"{}\"", reason));
        }
        json.push_str("],");

        json.push_str(&format!("\"evaluated_at\":\"{}\"", self.evaluated_at));
        json.push('}');
        json
    }
}

/// Certifies operator-attested results for the 5 standardized adversarial
/// stress probes into a composite score and pass/fail gate. Does not execute
/// the probes itself — see the module-level documentation for details.
pub struct AdversarialResearchGate;

impl AdversarialResearchGate {
    /// Aggregates caller-supplied probe outcomes into a composite robustness
    /// score and a certified, tamper-evident gate report.
    ///
    /// This function performs no simulation, perturbation, or stress testing
    /// of its own: `probes` must already contain the outcomes of a real
    /// adversarial stress test that was run elsewhere (by another tool or by
    /// a human operator attesting to the results). The correctness of the
    /// resulting report depends entirely on the accuracy of the probe results
    /// it is given; this function only guarantees consistent, deterministic
    /// aggregation and a tamper-evident evaluation identifier.
    pub fn evaluate_probes(
        strategy_version: &str,
        probes: Vec<AdversarialProbeResult>,
        evaluated_at: &str,
    ) -> Result<AdversarialEvaluation, BacktestError> {
        if probes.len() < 5 {
            return Err(BacktestError(
                "adversarial evaluation requires all 5 standardized stress probes".to_owned(),
            ));
        }

        let mut passed_count = 0;
        let mut blocking_failure_reasons = Vec::new();

        for probe in &probes {
            if probe.passed {
                passed_count += 1;
            } else {
                blocking_failure_reasons.push(format!(
                    "{}: degradation of {} bps exceeded threshold of {} bps",
                    probe.probe_name, probe.degradation_bps, probe.threshold_bps
                ));
            }
        }

        let composite_robustness_score_bps =
            ((passed_count as u64 * 10_000) / probes.len() as u64) as u32;
        let gate_passed = blocking_failure_reasons.is_empty();

        let digest = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}", strategy_version, evaluated_at).as_bytes())
        );
        let evaluation_id = format!("adveval.{}", &digest[..16]);

        Ok(AdversarialEvaluation {
            adversarial_schema_version: 1,
            evaluation_id,
            strategy_version: strategy_version.to_owned(),
            probes,
            composite_robustness_score_bps,
            gate_passed,
            blocking_failure_reasons,
            evaluated_at: evaluated_at.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_5_probe_adversarial_suite_pass() {
        let probes = vec![
            AdversarialProbeResult {
                probe_name: "LOOKAHEAD_LEAKAGE_PROBE".to_owned(),
                probe_description: "Time-shifted indicator look-ahead test".to_owned(),
                passed: true,
                degradation_bps: 10,
                threshold_bps: 100,
            },
            AdversarialProbeResult {
                probe_name: "PRICE_JITTER_PROBE".to_owned(),
                probe_description: "Microstructure noise injection".to_owned(),
                passed: true,
                degradation_bps: 45,
                threshold_bps: 150,
            },
            AdversarialProbeResult {
                probe_name: "TRANSACTION_COST_SHOCK".to_owned(),
                probe_description: "2x slippage and fee inflation stress".to_owned(),
                passed: true,
                degradation_bps: 80,
                threshold_bps: 200,
            },
            AdversarialProbeResult {
                probe_name: "PARAMETER_CLIFF_PROBE".to_owned(),
                probe_description: "Parameter neighborhood stability test".to_owned(),
                passed: true,
                degradation_bps: 25,
                threshold_bps: 100,
            },
            AdversarialProbeResult {
                probe_name: "REGIME_STRESS_PROBE".to_owned(),
                probe_description: "Historical high volatility crash regime".to_owned(),
                passed: true,
                degradation_bps: 110,
                threshold_bps: 300,
            },
        ];

        let eval = AdversarialResearchGate::evaluate_probes(
            "strat.v1.0.0",
            probes,
            "2026-09-01T15:00:00Z",
        )
        .unwrap();
        assert_eq!(eval.adversarial_schema_version, 1);
        assert!(eval.gate_passed);
        assert_eq!(eval.composite_robustness_score_bps, 10_000);
        assert!(eval.blocking_failure_reasons.is_empty());

        let json = eval.to_json();
        assert!(json.contains("\"gate_passed\":true"));
        assert!(json.contains("\"composite_robustness_score_bps\":10000"));
    }

    #[test]
    fn fails_gate_when_cost_shock_exceeds_threshold() {
        let probes = vec![
            AdversarialProbeResult {
                probe_name: "LOOKAHEAD_LEAKAGE_PROBE".to_owned(),
                probe_description: "Time-shifted indicator look-ahead test".to_owned(),
                passed: true,
                degradation_bps: 10,
                threshold_bps: 100,
            },
            AdversarialProbeResult {
                probe_name: "PRICE_JITTER_PROBE".to_owned(),
                probe_description: "Microstructure noise injection".to_owned(),
                passed: true,
                degradation_bps: 45,
                threshold_bps: 150,
            },
            AdversarialProbeResult {
                probe_name: "TRANSACTION_COST_SHOCK".to_owned(),
                probe_description: "2x slippage and fee inflation stress".to_owned(),
                passed: false,
                degradation_bps: 450,
                threshold_bps: 200,
            },
            AdversarialProbeResult {
                probe_name: "PARAMETER_CLIFF_PROBE".to_owned(),
                probe_description: "Parameter neighborhood stability test".to_owned(),
                passed: true,
                degradation_bps: 25,
                threshold_bps: 100,
            },
            AdversarialProbeResult {
                probe_name: "REGIME_STRESS_PROBE".to_owned(),
                probe_description: "Historical high volatility crash regime".to_owned(),
                passed: true,
                degradation_bps: 110,
                threshold_bps: 300,
            },
        ];

        let eval = AdversarialResearchGate::evaluate_probes(
            "strat.v1.0.0",
            probes,
            "2026-09-01T15:00:00Z",
        )
        .unwrap();
        assert!(!eval.gate_passed);
        assert_eq!(eval.composite_robustness_score_bps, 8_000);
        assert_eq!(eval.blocking_failure_reasons.len(), 1);
        assert!(eval.blocking_failure_reasons[0].contains("TRANSACTION_COST_SHOCK"));
    }
}
