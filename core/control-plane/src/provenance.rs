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
    /// Formats the reconstruction as JSON matching the v1 schema.
    ///
    /// Serialized through `serde_json`, so every string is escaped. A value
    /// taken from a journal -- an actor or summary containing a quote, say --
    /// must never be able to produce an invalid or differently-shaped document.
    pub fn to_json(&self) -> String {
        let causal_chain: Vec<serde_json::Value> = self
            .causal_chain
            .iter()
            .map(|node| {
                serde_json::json!({
                    "node_id": node.node_id,
                    "event_type": node.event_type,
                    "actor": node.actor,
                    "event_time": node.event_time,
                    "available_at": node.available_at,
                    "causation_id": node.causation_id,
                    "content_hash": node.content_hash,
                    "summary": node.summary,
                })
            })
            .collect();
        let edges: Vec<serde_json::Value> = self
            .edges
            .iter()
            .map(|edge| {
                serde_json::json!({
                    "from_node_id": edge.from_node_id,
                    "to_node_id": edge.to_node_id,
                    "relation": edge.relation,
                })
            })
            .collect();
        serde_json::json!({
            "reconstruction_schema_version": 1,
            "reconstruction_id": self.reconstruction_id,
            "target_event_id": self.target_event_id,
            "target_entity_type": self.target_entity_type,
            "causal_chain": causal_chain,
            "edges": edges,
            "configuration_hash": self.configuration_hash,
            "integrity_status": self.integrity_status.as_str(),
            "verified_at": self.verified_at,
        })
        .to_string()
    }
}

/// The envelope metadata provenance needs, plus a hash of the exact record.
///
/// Reconstruction never reads a payload, so it does not need a full
/// [`EventEnvelope`]. This lets it work from a persisted journal line without
/// a payload decoder, while hashing the exact bytes that were persisted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenanceRecord {
    /// Globally unique event identifier.
    pub event_id: String,
    /// Canonical event type name.
    pub event_type: String,
    /// Subsystem or model actor that emitted the event.
    pub actor: String,
    /// Source event timestamp.
    pub event_time: String,
    /// Availability timestamp.
    pub receive_time: String,
    /// Direct parent event identifier if causally linked.
    pub causation_id: Option<String>,
    /// SHA-256 hex digest of the record's canonical JSON.
    pub content_hash: String,
}

impl ProvenanceRecord {
    /// Builds the record for an in-memory envelope.
    pub fn from_envelope(event: &EventEnvelope) -> Self {
        Self {
            event_id: event.event_id.clone(),
            event_type: event.event_type.clone(),
            actor: event.actor.clone(),
            event_time: event.event_time.clone(),
            receive_time: event.receive_time.clone(),
            causation_id: event.causation_id.clone(),
            content_hash: format!("{:x}", Sha256::digest(event.canonical_json().as_bytes())),
        }
    }

    /// Builds the record for one persisted journal line.
    ///
    /// The line must already be canonical -- compact, sorted-key JSON, the
    /// form [`EventEnvelope::canonical_json`] writes -- so that its hash is the
    /// same hash [`Self::from_envelope`] computes for the event it encodes. A
    /// line in any other form is refused rather than re-canonicalized: a
    /// provenance hash over bytes nobody persisted would bind nothing.
    pub fn from_canonical_line(line: &str) -> Result<Self, EngineError> {
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| EngineError(format!("journal record is not JSON: {error}")))?;
        if serde_json::to_string(&value)
            .map_err(|error| EngineError(format!("journal record cannot be re-encoded: {error}")))?
            != line
        {
            return Err(EngineError(
                "journal record is not canonical sorted-key JSON".to_owned(),
            ));
        }
        let text = |name: &str| -> Result<String, EngineError> {
            value
                .get(name)
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| EngineError(format!("journal record is missing {name}")))
        };
        let causation_id = match value.get("causation_id") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(id)) if !id.is_empty() => Some(id.clone()),
            Some(_) => {
                return Err(EngineError(
                    "journal record causation_id must be a string or null".to_owned(),
                ))
            }
        };
        Ok(Self {
            event_id: text("event_id")?,
            event_type: text("event_type")?,
            actor: text("actor")?,
            event_time: text("event_time")?,
            receive_time: text("receive_time")?,
            causation_id,
            content_hash: format!("{:x}", Sha256::digest(line.as_bytes())),
        })
    }
}

/// Builder for reconstructing decision provenance graphs from immutable event logs.
pub struct DecisionProvenanceGraphBuilder {
    events_by_id: HashMap<String, ProvenanceRecord>,
}

impl DecisionProvenanceGraphBuilder {
    /// Creates a builder indexing a slice of event envelopes.
    pub fn new(events: &[EventEnvelope]) -> Self {
        let mut events_by_id = HashMap::with_capacity(events.len());
        for event in events {
            events_by_id.insert(
                event.event_id.clone(),
                ProvenanceRecord::from_envelope(event),
            );
        }
        Self { events_by_id }
    }

    /// Creates a builder from persisted journal records.
    ///
    /// A repeated event identity is refused: in an append-only journal it
    /// means two different records claim to be the same event, and choosing
    /// either one would make the reconstruction depend on which was read last.
    pub fn from_records(records: Vec<ProvenanceRecord>) -> Result<Self, EngineError> {
        let mut events_by_id = HashMap::with_capacity(records.len());
        for record in records {
            let event_id = record.event_id.clone();
            if events_by_id.insert(event_id.clone(), record).is_some() {
                return Err(EngineError(format!(
                    "journal contains event {event_id} more than once"
                )));
            }
        }
        Ok(Self { events_by_id })
    }

    /// Reconstructs the complete causal graph for a chosen target event.
    pub fn reconstruct(
        &self,
        target_event_id: &str,
        configuration_hash: &str,
        verified_at: &str,
    ) -> Result<DecisionReconstruction, EngineError> {
        let target_event = self.events_by_id.get(target_event_id).ok_or_else(|| {
            EngineError(format!(
                "target event {} not found in store",
                target_event_id
            ))
        })?;

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
                        content_hash: event.content_hash.clone(),
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

    fn chain() -> Vec<EventEnvelope> {
        vec![
            dummy_bar_envelope("evt.1", "2026-09-01T10:00:00Z", None),
            dummy_bar_envelope("evt.2", "2026-09-01T10:00:01Z", Some("evt.1")),
            dummy_bar_envelope("evt.3", "2026-09-01T10:00:02Z", Some("evt.2")),
        ]
    }

    #[test]
    fn a_persisted_line_hashes_exactly_like_its_envelope() {
        for event in chain() {
            let from_line = ProvenanceRecord::from_canonical_line(&event.canonical_json()).unwrap();
            assert_eq!(from_line, ProvenanceRecord::from_envelope(&event));
        }
    }

    #[test]
    fn reconstruction_from_journal_lines_matches_reconstruction_from_envelopes() {
        let events = chain();
        let records = events
            .iter()
            .map(|event| ProvenanceRecord::from_canonical_line(&event.canonical_json()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let from_lines = DecisionProvenanceGraphBuilder::from_records(records)
            .unwrap()
            .reconstruct("evt.3", "cfg_hash_abc", "2026-09-01T10:05:00Z")
            .unwrap();
        let from_envelopes = DecisionProvenanceGraphBuilder::new(&events)
            .reconstruct("evt.3", "cfg_hash_abc", "2026-09-01T10:05:00Z")
            .unwrap();
        assert_eq!(from_lines, from_envelopes);
    }

    #[test]
    fn a_non_canonical_line_is_refused_rather_than_rehashed() {
        let line = chain()[0].canonical_json();
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        let pretty = serde_json::to_string_pretty(&value).unwrap();
        assert!(ProvenanceRecord::from_canonical_line(&pretty).is_err());
        let reordered = line.replacen("{\"account_id\":null,", "{", 1).replacen(
            "\"strategy_id\":null}",
            "\"strategy_id\":null,\"account_id\":null}",
            1,
        );
        assert_ne!(reordered, line);
        assert!(ProvenanceRecord::from_canonical_line(&reordered).is_err());
    }

    #[test]
    fn a_repeated_event_identity_is_refused() {
        let events = chain();
        let mut records: Vec<_> = events.iter().map(ProvenanceRecord::from_envelope).collect();
        let mut forged = records[1].clone();
        forged.actor = "someone_else".to_owned();
        records.push(forged);
        assert!(DecisionProvenanceGraphBuilder::from_records(records).is_err());
    }

    #[test]
    fn journal_strings_are_escaped_in_the_published_document() {
        let mut event = dummy_bar_envelope("evt.1", "2026-09-01T10:00:00Z", None);
        event.actor = "desk \"alpha\" \\ ops".to_owned();
        let events = vec![event];
        let json = DecisionProvenanceGraphBuilder::new(&events)
            .reconstruct("evt.1", "cfg_hash_abc", "2026-09-01T10:05:00Z")
            .unwrap()
            .to_json();
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("the document must remain valid JSON");
        assert_eq!(parsed["causal_chain"][0]["actor"], "desk \"alpha\" \\ ops");
        assert_eq!(
            parsed["causal_chain"][0]["causation_id"],
            serde_json::Value::Null
        );
        assert_eq!(parsed.as_object().unwrap().len(), 9);
    }

    #[test]
    fn detects_incomplete_causation_chain() {
        let target = dummy_bar_envelope("evt.2", "2026-09-01T10:00:01Z", Some("evt.missing"));
        let events = vec![target];
        let builder = DecisionProvenanceGraphBuilder::new(&events);
        let recon = builder
            .reconstruct("evt.2", "cfg_hash_abc", "2026-09-01T10:05:00Z")
            .unwrap();

        assert_eq!(
            recon.integrity_status,
            ProvenanceIntegrityStatus::IncompleteChain
        );
    }
}
