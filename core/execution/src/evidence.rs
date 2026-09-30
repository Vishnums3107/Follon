//! Benchmark and plan evidence with canonical JSON and hashing.

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, Side};
use sha2::{Digest, Sha256};

use crate::*;

/// Frozen pre/post-trade benchmarks associated with an execution plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionBenchmarkEvidence {
    /// Unique benchmark identity.
    pub benchmark_id: String,
    /// Parent order identity.
    pub parent_order_id: String,
    /// Frozen arrival price at parent release.
    pub arrival_price: Decimal,
    /// Frozen algorithm target benchmark price.
    pub target_price: Decimal,
    /// Authoritative source of the benchmark mark.
    pub source: String,
}

impl ExecutionBenchmarkEvidence {
    /// Validates benchmark fields.
    pub fn validate(&self) -> Result<(), ExecutionError> {
        validate_canonical_id("benchmark_id", &self.benchmark_id)?;
        validate_canonical_id("parent_order_id", &self.parent_order_id)?;
        validate_canonical_id("source", &self.source)?;
        if self.arrival_price <= Decimal::ZERO || self.target_price <= Decimal::ZERO {
            return Err(ExecutionError(
                "benchmark prices must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Immutable, content-addressed evidence record for an execution plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPlanEvidence {
    /// Unique evidence identity.
    pub evidence_id: String,
    /// Parent order identity.
    pub parent_order_id: String,
    /// Account selected for the order.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Order economic side.
    pub side: Side,
    /// Total authorized parent quantity.
    pub parent_quantity: Decimal,
    /// Optional limit price.
    pub limit_price: Option<Decimal>,
    /// Execution algorithm label.
    pub algorithm: String,
    /// Scheduled child instructions.
    pub children: Vec<ChildInstruction>,
    /// Unallocated quantity if any.
    pub unallocated_quantity: Decimal,
    /// Source capability version if capability-routed.
    pub source_capability_version: Option<String>,
    /// Routing decisions if multi-venue routed.
    pub route_decisions: Vec<RouteDecision>,
    /// Optional frozen benchmarks.
    pub benchmarks: Option<ExecutionBenchmarkEvidence>,
    /// Content-addressed SHA-256 fingerprint.
    pub plan_sha256: String,
    /// Canonical RFC3339 creation timestamp.
    pub created_at: String,
}

impl ExecutionPlanEvidence {
    /// Creates and cryptographically fingerprints an execution plan evidence record.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        evidence_id: String,
        parent: &ParentOrder,
        plan: &ExecutionPlan,
        source_capability_version: Option<String>,
        route_decisions: Vec<RouteDecision>,
        benchmarks: Option<ExecutionBenchmarkEvidence>,
        created_at: String,
    ) -> Result<Self, ExecutionError> {
        validate_canonical_id("evidence_id", &evidence_id)?;
        parent.validate()?;
        plan.validate_against(parent)?;
        if let Some(ref version) = source_capability_version {
            validate_canonical_id("source_capability_version", version)?;
        }
        for decision in &route_decisions {
            validate_canonical_id("decision_id", &decision.decision_id)?;
            if decision.parent_order_id != parent.parent_order_id {
                return Err(ExecutionError(
                    "route decision parent_order_id mismatch".to_owned(),
                ));
            }
            validate_canonical_id("decision venue", &decision.venue)?;
            validate_canonical_id("decision capability_version", &decision.capability_version)?;
            if decision.allocated_quantity <= Decimal::ZERO
                || decision.all_in_price <= Decimal::ZERO
                || decision.fee_per_unit < Decimal::ZERO
            {
                return Err(ExecutionError(
                    "invalid route decision economics".to_owned(),
                ));
            }
        }
        if let Some(ref benchmark) = benchmarks {
            benchmark.validate()?;
            if benchmark.parent_order_id != parent.parent_order_id {
                return Err(ExecutionError(
                    "benchmark evidence parent_order_id mismatch".to_owned(),
                ));
            }
        }
        validate_utc_timestamp("created_at", &created_at)?;

        let unsigned_json = Self::build_canonical_json(
            &evidence_id,
            &parent.parent_order_id,
            &parent.account_id,
            &parent.instrument_id,
            parent.side,
            parent.quantity,
            parent.limit_price,
            &plan.algorithm,
            &plan.children,
            plan.unallocated_quantity,
            source_capability_version.as_deref(),
            &route_decisions,
            benchmarks.as_ref(),
            &created_at,
            None,
        );
        let plan_sha256 = format!("{:x}", Sha256::digest(unsigned_json.as_bytes()));

        Ok(Self {
            evidence_id,
            parent_order_id: parent.parent_order_id.clone(),
            account_id: parent.account_id.clone(),
            instrument_id: parent.instrument_id.clone(),
            side: parent.side,
            parent_quantity: parent.quantity,
            limit_price: parent.limit_price,
            algorithm: plan.algorithm.clone(),
            children: plan.children.clone(),
            unallocated_quantity: plan.unallocated_quantity,
            source_capability_version,
            route_decisions,
            benchmarks,
            plan_sha256,
            created_at,
        })
    }

    /// Verifies that `plan_sha256` matches the canonical hash of this evidence record.
    pub fn verify_fingerprint(&self) -> bool {
        let unsigned_json = Self::build_canonical_json(
            &self.evidence_id,
            &self.parent_order_id,
            &self.account_id,
            &self.instrument_id,
            self.side,
            self.parent_quantity,
            self.limit_price,
            &self.algorithm,
            &self.children,
            self.unallocated_quantity,
            self.source_capability_version.as_deref(),
            &self.route_decisions,
            self.benchmarks.as_ref(),
            &self.created_at,
            None,
        );
        let expected_sha256 = format!("{:x}", Sha256::digest(unsigned_json.as_bytes()));
        self.plan_sha256 == expected_sha256
    }

    /// Canonical JSON representation including the calculated `plan_sha256`.
    pub fn canonical_json(&self) -> String {
        Self::build_canonical_json(
            &self.evidence_id,
            &self.parent_order_id,
            &self.account_id,
            &self.instrument_id,
            self.side,
            self.parent_quantity,
            self.limit_price,
            &self.algorithm,
            &self.children,
            self.unallocated_quantity,
            self.source_capability_version.as_deref(),
            &self.route_decisions,
            self.benchmarks.as_ref(),
            &self.created_at,
            Some(&self.plan_sha256),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_canonical_json(
        evidence_id: &str,
        parent_order_id: &str,
        account_id: &str,
        instrument_id: &str,
        side: Side,
        parent_quantity: Decimal,
        limit_price: Option<Decimal>,
        algorithm: &str,
        children: &[ChildInstruction],
        unallocated_quantity: Decimal,
        source_capability_version: Option<&str>,
        route_decisions: &[RouteDecision],
        benchmarks: Option<&ExecutionBenchmarkEvidence>,
        created_at: &str,
        plan_sha256: Option<&str>,
    ) -> String {
        let children_json = children
            .iter()
            .map(|c| {
                format!(
                    "{{\"child_order_id\":{},\"kind\":{},\"limit_price\":{},\"quantity\":\"{}\",\"scheduled_after_seconds\":{},\"stop_price\":{},\"venue\":{}}}",
                    json_string(&c.child_order_id),
                    json_string(order_kind_label(c.kind)),
                    c.limit_price.map(|p| format!("\"{p}\"")).unwrap_or_else(|| "null".to_owned()),
                    c.quantity,
                    c.scheduled_after_seconds,
                    c.stop_price.map(|p| format!("\"{p}\"")).unwrap_or_else(|| "null".to_owned()),
                    c.venue.as_deref().map(json_string).unwrap_or_else(|| "null".to_owned()),
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        let routes_json = route_decisions
            .iter()
            .map(|r| {
                format!(
                    "{{\"allocated_quantity\":\"{}\",\"all_in_price\":\"{}\",\"capability_version\":{},\"decision_id\":{},\"fee_per_unit\":\"{}\",\"latency_rank\":{},\"parent_order_id\":{},\"venue\":{}}}",
                    r.allocated_quantity,
                    r.all_in_price,
                    json_string(&r.capability_version),
                    json_string(&r.decision_id),
                    r.fee_per_unit,
                    r.latency_rank,
                    json_string(&r.parent_order_id),
                    json_string(&r.venue),
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        let benchmarks_json = benchmarks
            .map(|b| {
                format!(
                    "{{\"arrival_price\":\"{}\",\"benchmark_id\":{},\"parent_order_id\":{},\"source\":{},\"target_price\":\"{}\"}}",
                    b.arrival_price,
                    json_string(&b.benchmark_id),
                    json_string(&b.parent_order_id),
                    json_string(&b.source),
                    b.target_price,
                )
            })
            .unwrap_or_else(|| "null".to_owned());

        format!(
            "{{\"account_id\":{},\"algorithm\":{},\"benchmarks\":{},\"children\":[{}],\"created_at\":{},\"evidence_id\":{},\"instrument_id\":{},\"limit_price\":{},\"parent_order_id\":{},\"parent_quantity\":\"{}\",\"plan_sha256\":{},\"route_decisions\":[{}],\"side\":{},\"source_capability_version\":{},\"unallocated_quantity\":\"{}\"}}",
            json_string(account_id),
            json_string(algorithm),
            benchmarks_json,
            children_json,
            json_string(created_at),
            json_string(evidence_id),
            json_string(instrument_id),
            limit_price.map(|p| format!("\"{p}\"")).unwrap_or_else(|| "null".to_owned()),
            json_string(parent_order_id),
            parent_quantity,
            plan_sha256.map(json_string).unwrap_or_else(|| "null".to_owned()),
            routes_json,
            json_string(side_label(side)),
            source_capability_version.map(json_string).unwrap_or_else(|| "null".to_owned()),
            unallocated_quantity,
        )
    }
}
