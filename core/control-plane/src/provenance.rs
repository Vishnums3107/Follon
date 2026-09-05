//! Decision provenance graph and reconstruction engine (DUR-01).
//!
//! Enables verifiable traversal from any fill, order intent, risk rejection,
//! or position change back through its causal chain to market data, news,
//! signals, and configuration hashes.

use std::collections::{HashMap, HashSet};

use follon_domain::EventEnvelope;
use sha2::{Digest, Sha256};

use crate::EngineError;

/// Integrity status of a reconstructed decision DAG.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvenanceIntegrityStatus {
    /// Every node and causal ancestor exists with monotonic availability timestamps.
    Verified,
    /// One or more causation ancestors could not be resolved from available evidence.
    IncompleteChain,
    /// An availability timestamp preceded its logical source event time or parent availability time.
    TimestampAnomaly,
}

impl ProvenanceIntegrityStatus {
    /// Returns the canonical uppercase representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "VERIFIED",
            Self::IncompleteChain => "INCOMPLETE_CHAIN",
            Self::TimestampAnomaly => "TIMESTAMP_ANOMALY",
        }
    }
}

/// A single attributable causal node within a decision provenance graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalNode {
    /// Globally unique event identifier.
    pub node_id: String,
    /// Canonical event type name.
    pub event_type: String,
    /// Subsystem or model actor that emitted the event.
    pub actor: String,
    /// Source event timestamp in UTC RFC3339.
    pub event_time: String,
    /// Availability timestamp in UTC RFC3339.
    pub available_at: String,
    /// Direct parent event identifier if causally linked.
    pub causation_id: Option<String>,
    /// SHA256 hex digest of the canonical JSON envelope.
    pub content_hash: String,
    /// Safe human-readable summary of the node's payload.
    pub summary: String,
}

/// A directed causal edge connecting two nodes in a decision graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalEdge {
    /// Identifier of the parent or source node.
    pub from_node_id: String,
    /// Identifier of the child or derived node.
    pub to_node_id: String,
    /// Semantic relationship between the nodes.
    pub relation: String,
}

/// Reconstructed decision provenance graph matching `decision-reconstruction.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionReconstruction {
    /// Schema version (fixed at 1).
    pub reconstruction_schema_version: u32,
    /// Unique reconstruction report identity.
    pub reconstruction_id: String,
    /// Target event identifier being audited.
    pub target_event_id: String,
    /// High-level entity classification for the target.
    pub target_entity_type: String,
    /// Causally ordered sequence of nodes from root causes to target.
    pub causal_chain: Vec<CausalNode>,
    /// Directed edges describing causal dependencies.
    pub edges: Vec<CausalEdge>,
    /// Configuration hash bound during reconstruction.
    pub configuration_hash: String,
    /// Verification status.
    pub integrity_status: ProvenanceIntegrityStatus,
    /// RFC3339 timestamp when reconstruction was completed.
    pub verified_at: String,
}

impl DecisionReconstruction {
    /// Formats the reconstruction as canonical JSON matching the v1 schema.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{");
        json.push_str("\"reconstruction_schema_version\":1,");
        json.push_str(&format!("\"reconstruction_id\":\"{}\",", self.reconstruction_id));
        json.push_str(&format!("\"target_event_id\":\"{}\",", self.target_event_id));
        json.push_str(&format!("\"target_entity_type\":\"{}\",", self.target_entity_type));

        // causal_chain
        json.push_str("\"causal_chain\":[");
        for (index, node) in self.causal_chain.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            let causation_json = match &node.causation_id {
                Some(id) => format!("\"{}\"", id),
                None => "null".to_owned(),
            };
            json.push_str(&format!(
                "{{\"node_id\":\"{}\",\"event_type\":\"{}\",\"actor\":\"{}\",\"event_time\":\"{}\",\"available_at\":\"{}\",\"causation_id\":{},\"content_hash\":\"{}\",\"summary\":\"{}\"}}",
                node.node_id,
                node.event_type,
                node.actor,
                node.event_time,
                node.available_at,
                causation_json,
                node.content_hash,
                node.summary
            ));
        }
        json.push_str("],");

        // edges
        json.push_str("\"edges\":[");
        for (index, edge) in self.edges.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "{{\"from_node_id\":\"{}\",\"to_node_id\":\"{}\",\"relation\":\"{}\"}}",
                edge.from_node_id, edge.to_node_id, edge.relation
            ));
        }
        json.push_str("],");

        json.push_str(&format!("\"configuration_hash\":\"{}\",", self.configuration_hash));
        json.push_str(&format!("\"integrity_status\":\"{}\",", self.integrity_status.as_str()));
        json.push_str(&format!("\"verified_at\":\"{}\"", self.verified_at));
        json.push('}');
        json
    }
}

/// Builder for reconstructing decision provenance graphs from immutable event logs.
pub struct DecisionProvenanceGraphBuilder<'a> {
    events_by_id: HashMap<&'a str, &'a EventEnvelope>,
}

impl<'a> DecisionProvenanceGraphBuilder<'a> {
    /// Creates a builder indexing a slice of event envelopes.
    pub fn new(events: &'a [EventEnvelope]) -> Self {
        let mut events_by_id = HashMap::with_capacity(events.len());
        for event in events {
            events_by_id.insert(event.event_id.as_str(), event);
        }
        Self { events_by_id }
    }

    /// Reconstructs the complete causal graph for a chosen target event.
    pub fn reconstruct(
        &self,
        target_event_id: &str,
        configuration_hash: &str,
        verified_at: &str,
    ) -> Result<DecisionReconstruction, EngineError> {
        let target_event = self
            .events_by_id
            .get(target_event_id)
            .ok_or_else(|| EngineError(format!("target event {} not found in store", target_event_id)))?;

        let target_entity_type = match target_event.event_type.as_str() {
            "execution.fill.v1" => "fill",
            "intent.created.v1" => "order_intent",
            "risk.decision.v1" => "risk_rejection",
            "position.updated.v1" => "position",
            _ => "alert",
        }
        .to_owned();

        let mut visited_ids = HashSet::new();
        let mut chain_nodes = Vec::new();
        let mut edges = Vec::new();
        let mut current_id = Some(target_event.event_id.as_str());
        let mut integrity_status = ProvenanceIntegrityStatus::Verified;

        // Traverse causation backward
        while let Some(node_id) = current_id {
            if !visited_ids.insert(node_id) {
                // Cycle detected in causation trail
                integrity_status = ProvenanceIntegrityStatus::TimestampAnomaly;
                break;
            }

            match self.events_by_id.get(node_id) {
                Some(event) => {
                    let canonical = event.canonical_json();
                    let hash = format!("{:x}", Sha256::digest(canonical.as_bytes()));

                    // Timestamp sanity: receive_time must not precede event_time
                    if event.receive_time < event.event_time {
                        integrity_status = ProvenanceIntegrityStatus::TimestampAnomaly;
                    }

                    let summary = format!("{}: {}", event.event_type, event.actor);
                    chain_nodes.push(CausalNode {
                        node_id: event.event_id.clone(),
                        event_type: event.event_type.clone(),
                        actor: event.actor.clone(),
                        event_time: event.event_time.clone(),
                        available_at: event.receive_time.clone(),
                        causation_id: event.causation_id.clone(),
                        content_hash: hash,
                        summary,
                    });

                    if let Some(parent_id) = &event.causation_id {
                        edges.push(CausalEdge {
                            from_node_id: parent_id.clone(),
                            to_node_id: event.event_id.clone(),
                            relation: "caused".to_owned(),
                        });
                        current_id = Some(parent_id.as_str());
                    } else {
                        current_id = None;
                    }
                }
                None => {
                    // Parent causation_id not found in historical events
                    integrity_status = ProvenanceIntegrityStatus::IncompleteChain;
                    current_id = None;
                }
            }
        }

        // Reverse so that chain nodes are topologically sorted from root cause to target
        chain_nodes.reverse();

        // Check timestamp monotonicity along the reversed chain
        for window in chain_nodes.windows(2) {
            let parent = &window[0];
            let child = &window[1];
            if child.available_at < parent.available_at {
                integrity_status = ProvenanceIntegrityStatus::TimestampAnomaly;
            }
        }

        let reconstruction_id = format!("recon.{}", target_event.event_id.replace("event.", ""));

        Ok(DecisionReconstruction {
            reconstruction_schema_version: 1,
            reconstruction_id,
            target_event_id: target_event_id.to_owned(),
            target_entity_type,
            causal_chain: chain_nodes,
            edges,
            configuration_hash: configuration_hash.to_owned(),
            integrity_status,
            verified_at: verified_at.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use follon_domain::{Bar, EventPayload};

    fn dummy_bar_envelope(id: &str, time: &str, causation: Option<&str>) -> EventEnvelope {
        EventEnvelope {
            event_id: id.to_owned(),
            event_type: "market.bar.v1".to_owned(),
            schema_version: 1,
            event_time: time.to_owned(),
            receive_time: time.to_owned(),
            account_id: None,
            strategy_id: None,
            instrument_id: Some("AAPL".to_owned()),
            correlation_id: "corr.1".to_owned(),
            causation_id: causation.map(str::to_owned),
            actor: "market_data".to_owned(),
            source: "feed".to_owned(),
            payload: EventPayload::MarketBar(Bar {
                instrument_id: "AAPL".to_owned(),
                open: follon_domain::Decimal::from_integer(100).unwrap(),
                high: follon_domain::Decimal::from_integer(105).unwrap(),
                low: follon_domain::Decimal::from_integer(99).unwrap(),
                close: follon_domain::Decimal::from_integer(104).unwrap(),
                volume: follon_domain::Decimal::from_integer(1000).unwrap(),
                interval_seconds: 60,
                exchange_timezone: "America/New_York".to_owned(),
            }),
            software_version: "0.1.0".to_owned(),
            configuration_version: "cfg.1".to_owned(),
        }
    }

    #[test]
    fn reconstructs_linear_provenance_chain_with_verification() {
        let root = dummy_bar_envelope("evt.1", "2026-09-01T10:00:00Z", None);
        let mid = dummy_bar_envelope("evt.2", "2026-09-01T10:00:01Z", Some("evt.1"));
        let target = dummy_bar_envelope("evt.3", "2026-09-01T10:00:02Z", Some("evt.2"));

        let events = vec![root, mid, target];
        let builder = DecisionProvenanceGraphBuilder::new(&events);
        let recon = builder
            .reconstruct("evt.3", "cfg_hash_abc", "2026-09-01T10:05:00Z")
            .unwrap();

        assert_eq!(recon.reconstruction_schema_version, 1);
        assert_eq!(recon.target_event_id, "evt.3");
        assert_eq!(recon.integrity_status, ProvenanceIntegrityStatus::Verified);
        assert_eq!(recon.causal_chain.len(), 3);
        assert_eq!(recon.causal_chain[0].node_id, "evt.1");
        assert_eq!(recon.causal_chain[1].node_id, "evt.2");
        assert_eq!(recon.causal_chain[2].node_id, "evt.3");
        assert_eq!(recon.edges.len(), 2);

        let json = recon.to_json();
        assert!(json.contains("\"integrity_status\":\"VERIFIED\""));
        assert!(json.contains("\"target_event_id\":\"evt.3\""));
    }

    #[test]
    fn detects_incomplete_causation_chain() {
        let target = dummy_bar_envelope("evt.2", "2026-09-01T10:00:01Z", Some("evt.missing"));
        let events = vec![target];
        let builder = DecisionProvenanceGraphBuilder::new(&events);
        let recon = builder
            .reconstruct("evt.2", "cfg_hash_abc", "2026-09-01T10:05:00Z")
            .unwrap();

        assert_eq!(recon.integrity_status, ProvenanceIntegrityStatus::IncompleteChain);
    }
}
