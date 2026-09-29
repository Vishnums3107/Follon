// Replay and incidents workspace.

import { parseDecisionReconstruction, parseOrderDecisionPassport, parseRecoveryDrillResult } from "../evidence/index.js";
import { appendAdvancedEvidenceRows, typedAdvancedEvidence } from "./advanced-evidence.js";
import { field, reconciliationText, shortHash } from "./format.js";
import { appendDefinition, appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics, renderProjectionIntegrityPanel } from "./panels.js";
import { liveDashboard, paperDashboard } from "./snapshot.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";
import { renderCausalDagVisualizer } from "./visualizers.js";

export function renderReplayAndIncidents(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const replayEvents = snapshot.replay_events ?? [];
  const typeCounts = new Map<string, number>();
  for (const event of snapshot.events) {
    const type = field(event.data, "event_type");
    typeCounts.set(type, (typeCounts.get(type) ?? 0) + 1);
  }
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const unresolved = (paper?.unexplained_incidents ?? 0) + (live?.unresolved_incidents ?? 0);
  renderMetrics(summaryRoot, [
    ["Canonical events", String(replayEvents.length), "Causally ordered immutable envelopes at declared availability time"],
    ["Event types", String(typeCounts.size), "Market, intent, risk, OMS, fill, portfolio, and audit phases"],
    ["Journal records", String(snapshot.journals.length), "PAPER, LIVE, operations, and commercial chains"],
    ["Unresolved incidents", String(unresolved), "Latest PAPER and LIVE projections", unresolved === 0 ? "good" : "bad"],
  ]);
  const distribution = createPanel("Event distribution", "Counts by canonical event type across indexed replay outputs.");
  appendTableOrEmpty(distribution, ["Event type", "Count"], [...typeCounts.entries()].sort((a, b) => b[1] - a[1]).map(([type, count]) => [type, String(count)]), "No canonical events are available.");
  root.append(distribution);

  const projectionIntegrity = renderProjectionIntegrityPanel(snapshot, context);
  if (projectionIntegrity !== undefined) root.append(projectionIntegrity);

  const timeline = createPanel("Evidence timeline", "Newest-first presentation of validated envelopes; use the debugger for canonical causation-respecting order.");
  appendTableOrEmpty(timeline, ["Time", "Event", "Actor", "Correlation", "Caused by", "Instrument", "Artifact"], snapshot.events.map((item) => [
    field(item.data, "event_time"), field(item.data, "event_type"), field(item.data, "actor"), field(item.data, "correlation_id"),
    field(item.data, "causation_id"), field(item.data, "instrument_id"), item.artifact,
  ]), "No canonical replay timeline is available.", (index) => context.onOpenArtifact(snapshot.events[index]?.artifact ?? ""));
  root.append(timeline);

  const incidents = createPanel("Incident and recovery state", "UNKNOWN and reconciliation differences remain explicit until new evidence resolves them.");
  appendTableOrEmpty(incidents, ["Environment", "Unknown orders", "Incidents", "Reconciliation", "Audit sequence", "Audit head"], [
    ["PAPER", String(paper?.unknown_orders ?? 0), String(paper?.unexplained_incidents ?? 0), reconciliationText(paper?.last_reconciliation_clean, paper?.last_reconciled_at), String(paper?.audit_sequence ?? 0), shortHash(paper?.audit_head_hash ?? "")],
    ["LIVE", String(live?.unknown_orders ?? 0), String(live?.unresolved_incidents ?? 0), reconciliationText(live?.last_reconciliation_clean, live?.last_reconciled_at), String(live?.audit_sequence ?? 0), shortHash(live?.audit_head_hash ?? "")],
  ], "No incident state is available.");
  root.append(incidents);

  const debuggerPanel = createPanel(
    "Event-by-event debugger",
    "Step through the availability-time sequence: market bar -> strategy state -> intent -> risk decision -> OMS state change -> simulated fill with causal links. Source event time remains visible as evidence (RES-03)."
  );
  debuggerPanel.id = "event-debugger";

  const controls = document.createElement("div");
  controls.className = "debugger-controls";

  const prevBtn = document.createElement("button");
  prevBtn.type = "button";
  prevBtn.className = "f-btn f-btn--secondary";
  prevBtn.textContent = "◀ Step Back";

  const nextBtn = document.createElement("button");
  nextBtn.type = "button";
  nextBtn.className = "f-btn f-btn--primary";
  nextBtn.textContent = "Step Forward ▶";

  const statusText = document.createElement("span");
  statusText.className = "debugger-status";

  const detailsBox = document.createElement("div");
  detailsBox.className = "debugger-details";

  let eventCursor = 0;
  const events = replayEvents;

  const updateDebugger = () => {
    if (events.length === 0) {
      statusText.textContent = "No replay events available.";
      detailsBox.textContent = "Replay event log is empty.";
      prevBtn.disabled = true;
      nextBtn.disabled = true;
      return;
    }
    prevBtn.disabled = eventCursor <= 0;
    nextBtn.disabled = eventCursor >= events.length - 1;
    const current = events[eventCursor];
    const type = field(current.data, "event_type");
    const time = field(current.data, "event_time");
    const actor = field(current.data, "actor") || "kernel";
    const correlation = field(current.data, "correlation_id");
    const causation = field(current.data, "causation_id") || "root";
    statusText.textContent = `Event ${eventCursor + 1} of ${events.length} · ${time} · ${type}`;
    detailsBox.replaceChildren();
    appendDefinition(detailsBox, [
      ["Event Time (UTC)", time],
      ["Event Type / Phase", type],
      ["Actor / Source", `${actor} / ${field(current.data, "source") || "engine"}`],
      ["Event ID", field(current.data, "event_id")],
      ["Causation Link", causation],
      ["Correlation ID", correlation],
      ["Payload Summary", JSON.stringify(current.data.payload ?? {})],
    ]);
  };

  prevBtn.addEventListener("click", () => {
    if (eventCursor > 0) {
      eventCursor--;
      updateDebugger();
    }
  });
  nextBtn.addEventListener("click", () => {
    if (eventCursor < events.length - 1) {
      eventCursor++;
      updateDebugger();
    }
  });

  controls.append(prevBtn, nextBtn, statusText);
  debuggerPanel.append(controls, detailsBox);
  root.append(debuggerPanel);
  updateDebugger();

  const explainMomentPanel = createPanel(
    "Explain this moment (unified temporal reconstruction)",
    "Select any historical execution or alert timestamp to reconstruct exact market feed knowledge, strategy internal state, pre-trade risk decision, OMS child fills, and portfolio balance at that exact nanosecond (SOLO-01, RES-03, DATA-02, EXEC-02)."
  );
  explainMomentPanel.id = "explain-moment-panel";
  appendAdvancedEvidenceRows(
    explainMomentPanel,
    snapshot,
    context,
    "order_decision_passport",
    parseOrderDecisionPassport,
    ["Reconstructed Timestamp", "Market Knowledge As-Of", "Strategy State", "Risk Policy Input", "OMS Execution Outcome", "Ledger Balance", "Lineage Hash"],
    (passport) => [[
      passport.created_at,
      "Not published by this contract",
      `${passport.signal_attribution.strategy_version}; event ${passport.signal_attribution.model_event_id}`,
      `${passport.risk_evaluation.policy_version}; ${passport.risk_evaluation.approved ? "approved" : "rejected"}`,
      passport.executions.map((execution) => `${execution.quantity} @ ${execution.price}`).join(" | ") || "No execution recorded",
      `Cash ${passport.accounting_consequences.cash_delta}; position ${passport.accounting_consequences.position_after}`,
      passport.passport_id,
    ]],
    "No typed decision passport is published for temporal reconstruction."
  );
  appendAdvancedEvidenceRows(
    explainMomentPanel,
    snapshot,
    context,
    "decision_reconstruction",
    parseDecisionReconstruction,
    ["Reconstruction ID", "Target Event / Entity", "Integrity Status", "Causal Chain (Nodes)", "DAG Edges", "Config Hash", "Verified At"],
    (recon) => [[
      recon.reconstruction_id,
      `${recon.target_event_id} (${recon.target_entity_type})`,
      recon.integrity_status,
      recon.causal_chain.map((n) => `${n.node_id}: ${n.event_type} [${n.actor}] - ${n.summary}`).join(" | "),
      recon.edges.map((e) => `${e.from_node_id} -> ${e.to_node_id} (${e.relation})`).join(" | "),
      shortHash(recon.configuration_hash),
      recon.verified_at,
    ]],
    "No typed decision provenance reconstruction is published."
  );
  const decisionReconstruction = typedAdvancedEvidence(snapshot, "decision_reconstruction", parseDecisionReconstruction)[0]?.data;
  const causalDag = renderCausalDagVisualizer(decisionReconstruction);
  if (causalDag !== undefined) explainMomentPanel.append(causalDag);
  root.append(explainMomentPanel);

  const recoveryDrillPanel = createPanel(
    "Game-day disaster recovery and failover verification",
    "Empirical RTO, RPO, and ledger reconciliation proofs from automated recovery drills and fault injections (DUR-08)."
  );
  recoveryDrillPanel.id = "recovery-drill-panel";
  appendAdvancedEvidenceRows(
    recoveryDrillPanel,
    snapshot,
    context,
    "recovery_drill_result",
    parseRecoveryDrillResult,
    ["Drill ID", "Scenario", "Injected Fault", "Measured RTO / Target", "Measured RPO / Target", "Reconciliation Match", "Drill Passed", "Executed At"],
    (drill) => [[
      drill.drill_id,
      drill.scenario_name,
      drill.injected_fault,
      `${drill.measured_rto_seconds}s / ${drill.target_rto_seconds}s`,
      `${drill.measured_rpo_events_lost} events / ${drill.target_rpo_events_lost}`,
      drill.reconciliation_hash_matched ? "MATCHED" : "MISMATCH",
      drill.drill_passed ? "PASSED" : "FAILED",
      drill.executed_at,
    ]],
    "No typed disaster recovery drill result is published."
  );
  root.append(recoveryDrillPanel);

  root.append(renderFeatureEvidence(context, ["replay", "paper", "controlled-live", "operations"]));
}
