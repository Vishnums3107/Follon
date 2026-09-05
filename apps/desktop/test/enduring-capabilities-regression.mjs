import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseDecisionReconstruction,
  parseCounterfactualScenario,
  parseDataRightsAndSemanticsReceipt,
  parseWorkspaceSnapshotManifest,
  parseAttentionBudget,
  parseAdversarialEvaluation,
  parseStrategyCapsuleManifest,
  parseRecoveryDrillResult,
  parseGatewayQualificationMatrix,
  parseCapitalAllocationProposal,
  parseCompatibilityMatrix,
} from "../dist/evidence.js";
import { renderWorkspace } from "../dist/workspaces.js";

const testDir = dirname(fileURLToPath(import.meta.url));

// --- 1. Schema & Parser Unit Tests (DUR-01 through DUR-12) ---

// DUR-01: DecisionReconstruction
const validRecon = {
  reconstruction_schema_version: 1,
  reconstruction_id: "recon.fill.9b41a2c",
  target_event_id: "evt.fill.9b41a2c",
  target_entity_type: "fill",
  causal_chain: [
    {
      node_id: "evt.bar.spy.001",
      event_type: "market.bar.v1",
      actor: "market-feed",
      event_time: "2026-09-01T14:30:00Z",
      available_at: "2026-09-01T14:30:00Z",
      content_hash: "a".repeat(64),
      summary: "Market bar SPY close 500.00",
    },
    {
      node_id: "evt.sig.spy.001",
      event_type: "signal.generated.v1",
      actor: "strategy-engine",
      event_time: "2026-09-01T14:30:01Z",
      available_at: "2026-09-01T14:30:01Z",
      causation_id: "evt.bar.spy.001",
      content_hash: "b".repeat(64),
      summary: "Trend signal buy 100 SPY",
    },
  ],
  edges: [
    {
      from_node_id: "evt.bar.spy.001",
      to_node_id: "evt.sig.spy.001",
      relation: "CAUSED_SIGNAL",
    },
  ],
  configuration_hash: "c".repeat(64),
  integrity_status: "VERIFIED",
  verified_at: "2026-09-01T14:30:05Z",
};

const parsedRecon = parseDecisionReconstruction(JSON.stringify(validRecon));
assert.equal(parsedRecon.reconstruction_id, "recon.fill.9b41a2c");
assert.equal(parsedRecon.integrity_status, "VERIFIED");
assert.equal(parsedRecon.causal_chain.length, 2);
assert.equal(parsedRecon.edges.length, 1);

assert.throws(
  () => parseDecisionReconstruction(JSON.stringify({ ...validRecon, integrity_status: "TAMPERED" })),
  /does not match the v1 evidence contract/,
  "Invalid integrity status must be rejected"
);

// DUR-02: CounterfactualScenario
const validScenario = {
  scenario_schema_version: 1,
  scenario_id: "cf.latency-shock.001",
  baseline_run_id: "run.baseline.100",
  seed: 42,
  interventions: [
    {
      intervention_type: "NETWORK_LATENCY_INJECTION",
      parameter_name: "gateway_rtt_ms",
      baseline_value: "5",
      counterfactual_value: "150",
    },
  ],
  delta_metrics: {
    fill_count_delta: -2,
    pnl_delta_usd: "-420.50000000",
    max_drawdown_delta_bps: 85,
    risk_rejection_count_delta: 3,
  },
  divergence_event_id: "evt.order.intent.045",
  created_at: "2026-09-01T16:00:00Z",
};

const parsedScenario = parseCounterfactualScenario(JSON.stringify(validScenario));
assert.equal(parsedScenario.scenario_id, "cf.latency-shock.001");
assert.equal(parsedScenario.interventions.length, 1);
assert.equal(parsedScenario.delta_metrics.max_drawdown_delta_bps, 85);

assert.throws(
  () => parseCounterfactualScenario(JSON.stringify({ ...validScenario, interventions: [] })),
  /does not match the v1 evidence contract/,
  "Empty interventions must be rejected"
);

// DUR-03: DataRightsAndSemanticsReceipt
const validRights = {
  receipt_schema_version: 1,
  receipt_id: "drsr.polygon.us-equity-l1",
  provider_id: "provider.polygon",
  dataset_id: "ds.us_equity.1m",
  license_tier: "COMMERCIAL_REPLAY",
  redistribution_permitted: false,
  corporate_action_policy: "RAW_SPLIT_AND_DIVIDEND_ADJUSTED",
  semantic_parity_score_bps: 9980,
  verified_at: "2026-09-01T08:00:00Z",
  expires_at: "2027-09-01T08:00:00Z",
};

const parsedRights = parseDataRightsAndSemanticsReceipt(JSON.stringify(validRights));
assert.equal(parsedRights.receipt_id, "drsr.polygon.us-equity-l1");
assert.equal(parsedRights.license_tier, "COMMERCIAL_REPLAY");
assert.equal(parsedRights.semantic_parity_score_bps, 9980);

assert.throws(
  () => parseDataRightsAndSemanticsReceipt(JSON.stringify({ ...validRights, corporate_action_policy: "INVALID_POLICY" })),
  /does not match the v1 evidence contract/,
  "Invalid corporate action policy must be rejected"
);

// DUR-04: WorkspaceSnapshotManifest
const validManifest = {
  snapshot_schema_version: 1,
  manifest_id: "snapshot.2026-09-01.eod",
  as_of_time: "2026-09-01T20:00:00Z",
  created_at: "2026-09-01T20:01:00Z",
  content_hash: "d".repeat(64),
  retained_event_count: 5000,
  source_event_count: 5000,
  event_window: {
    window_kind: "full_day_session",
    first_event_time: "2026-09-01T13:30:00Z",
    last_event_time: "2026-09-01T20:00:00Z",
  },
  active_accounts: ["acct.paper.01", "acct.paper.02"],
  positions_fingerprint: "e".repeat(64),
  ledger_balance_fingerprint: "f".repeat(64),
  diagnostics: [],
};

const parsedManifest = parseWorkspaceSnapshotManifest(JSON.stringify(validManifest));
assert.equal(parsedManifest.manifest_id, "snapshot.2026-09-01.eod");
assert.equal(parsedManifest.retained_event_count, 5000);
assert.equal(parsedManifest.active_accounts.length, 2);

assert.throws(
  () => parseWorkspaceSnapshotManifest(JSON.stringify({ ...validManifest, content_hash: "not-a-hash" })),
  /does not match the v1 evidence contract/,
  "Invalid content hash must be rejected"
);

// DUR-05: AttentionBudget
const validBudget = {
  budget_schema_version: 1,
  budget_id: "attn.session.2026-09-01",
  session_date: "2026-09-01",
  cognitive_load_score_bps: 3500,
  interruptions_per_hour: 4.5,
  active_alarms_count: 1,
  suppressed_duplicates_count: 18,
  escalated_critical_tasks: [],
  budget_exhausted: false,
  calculated_at: "2026-09-01T17:00:00Z",
};

const parsedBudget = parseAttentionBudget(JSON.stringify(validBudget));
assert.equal(parsedBudget.budget_id, "attn.session.2026-09-01");
assert.equal(parsedBudget.cognitive_load_score_bps, 3500);
assert.equal(parsedBudget.suppressed_duplicates_count, 18);

assert.throws(
  () => parseAttentionBudget(JSON.stringify({ ...validBudget, cognitive_load_score_bps: 12000 })),
  /does not match the v1 evidence contract/,
  "Cognitive load > 10000 bps must be rejected"
);

// DUR-06: AdversarialEvaluation
const validAdvEval = {
  adversarial_schema_version: 1,
  evaluation_id: "adveval.strat.trend.v1",
  strategy_version: "strat.trend.v1.0.0",
  probes: [
    {
      probe_name: "LOOKAHEAD_LEAKAGE_PROBE",
      probe_description: "Audit shuffle test for lookahead leakage",
      passed: true,
      degradation_bps: 20,
      threshold_bps: 100,
    },
    {
      probe_name: "PRICE_JITTER_PROBE",
      probe_description: "Price noise perturbation audit",
      passed: true,
      degradation_bps: 45,
      threshold_bps: 150,
    },
    {
      probe_name: "TRANSACTION_COST_SHOCK",
      probe_description: "3x fee and slippage stress shock",
      passed: true,
      degradation_bps: 110,
      threshold_bps: 300,
    },
    {
      probe_name: "PARAMETER_CLIFF_PROBE",
      probe_description: "Parameter neighborhood cliff test",
      passed: true,
      degradation_bps: 60,
      threshold_bps: 200,
    },
    {
      probe_name: "REGIME_STRESS_PROBE",
      probe_description: "Historical crash regime replay",
      passed: true,
      degradation_bps: 140,
      threshold_bps: 400,
    },
  ],
  composite_robustness_score_bps: 10000,
  gate_passed: true,
  blocking_failure_reasons: [],
  evaluated_at: "2026-09-01T15:30:00Z",
};

const parsedAdvEval = parseAdversarialEvaluation(JSON.stringify(validAdvEval));
assert.equal(parsedAdvEval.evaluation_id, "adveval.strat.trend.v1");
assert.equal(parsedAdvEval.gate_passed, true);
assert.equal(parsedAdvEval.probes.length, 5);

assert.throws(
  () => parseAdversarialEvaluation(JSON.stringify({ ...validAdvEval, probes: validAdvEval.probes.slice(0, 3) })),
  /does not match the v1 evidence contract/,
  "Evaluations with fewer than 5 mandatory probes must be rejected"
);

// DUR-07: StrategyCapsuleManifest
const validCapsule = {
  capsule_schema_version: 1,
  capsule_id: "capsule.trend.v1",
  strategy_id: "strat.trend.v1",
  strategy_version: "v1.0.0",
  bundle_sha256: "1".repeat(64),
  configuration_sha256: "2".repeat(64),
  dependency_lockfile_sha256: "3".repeat(64),
  runtime_target: "follon-runtime-py312-v1",
  evaluation_receipt_id: "eval.golden.001",
  replay_instruction_command: "follon-cli replay --capsule capsule.trend.v1.tar.gz",
  export_disposition: "VERIFIED_PORTABLE",
  packaged_at: "2026-09-01T16:00:00Z",
};

const parsedCapsule = parseStrategyCapsuleManifest(JSON.stringify(validCapsule));
assert.equal(parsedCapsule.capsule_id, "capsule.trend.v1");
assert.equal(parsedCapsule.export_disposition, "VERIFIED_PORTABLE");

assert.throws(
  () => parseStrategyCapsuleManifest(JSON.stringify({ ...validCapsule, export_disposition: "UNVERIFIED" })),
  /does not match the v1 evidence contract/,
  "Invalid export disposition must be rejected"
);

// DUR-08: RecoveryDrillResult
const validDrill = {
  drill_schema_version: 1,
  drill_id: "drill.gameday.host-partition.01",
  scenario_name: "Split-brain host network partition recovery drill",
  injected_fault: "SPLIT_BRAIN_HOST_PARTITION",
  measured_rto_seconds: 12,
  target_rto_seconds: 30,
  measured_rpo_events_lost: 0,
  target_rpo_events_lost: 0,
  reconciliation_hash_matched: true,
  drill_passed: true,
  executed_at: "2026-09-01T04:00:00Z",
};

const parsedDrill = parseRecoveryDrillResult(JSON.stringify(validDrill));
assert.equal(parsedDrill.drill_id, "drill.gameday.host-partition.01");
assert.equal(parsedDrill.drill_passed, true);
assert.equal(parsedDrill.reconciliation_hash_matched, true);

assert.throws(
  () => parseRecoveryDrillResult(JSON.stringify({ ...validDrill, injected_fault: "UNKNOWN_FAULT" })),
  /does not match the v1 evidence contract/,
  "Unknown injected fault must be rejected"
);

// DUR-10: GatewayQualificationMatrix
const validGatewayMatrix = {
  matrix_schema_version: 1,
  matrix_id: "gqm.ibkr.paper-gateway-01",
  environment: "PAPER",
  gateway_id: "gw.ibkr.paper.primary",
  qualified_capabilities: [
    {
      capability_id: "cap.us_equity.market_and_limit",
      asset_class: "US_EQUITY",
      qualification_state: "CERTIFIED",
      measured_p99_latency_ms: 18,
      max_supported_slices: 100,
      reconciliation_accuracy_bps: 10000,
    },
  ],
  fencing_epoch: 14,
  evaluated_at: "2026-09-01T06:00:00Z",
  expires_at: "2026-10-01T06:00:00Z",
};

const parsedGatewayMatrix = parseGatewayQualificationMatrix(JSON.stringify(validGatewayMatrix));
assert.equal(parsedGatewayMatrix.matrix_id, "gqm.ibkr.paper-gateway-01");
assert.equal(parsedGatewayMatrix.qualified_capabilities.length, 1);
assert.equal(parsedGatewayMatrix.fencing_epoch, 14);

assert.throws(
  () => parseGatewayQualificationMatrix(JSON.stringify({ ...validGatewayMatrix, qualified_capabilities: [] })),
  /does not match the v1 evidence contract/,
  "Empty qualified capabilities must be rejected"
);

// DUR-11: CapitalAllocationProposal
const validProposal = {
  proposal_schema_version: 1,
  proposal_id: "cap-prop.2026-09-01.eod",
  total_equity_usd: "1000000.00000000",
  target_annual_volatility_bps: 1200,
  max_drawdown_limit_bps: 1500,
  allocations: [
    {
      strategy_id: "strat.trend.v1",
      recommended_capital_usd: "600000.00000000",
      risk_budget_share_bps: 6000,
      marginal_risk_contribution_bps: 720,
    },
    {
      strategy_id: "strat.mr.v1",
      recommended_capital_usd: "400000.00000000",
      risk_budget_share_bps: 4000,
      marginal_risk_contribution_bps: 480,
    },
  ],
  portfolio_diversification_ratio_bps: 14200,
  proposal_status: "RECOMMENDED",
  policy_version: "risk-policy-v2.1",
  proposed_at: "2026-09-01T21:00:00Z",
};

const parsedProposal = parseCapitalAllocationProposal(JSON.stringify(validProposal));
assert.equal(parsedProposal.proposal_id, "cap-prop.2026-09-01.eod");
assert.equal(parsedProposal.allocations.length, 2);
assert.equal(parsedProposal.proposal_status, "RECOMMENDED");

assert.throws(
  () => parseCapitalAllocationProposal(JSON.stringify({ ...validProposal, max_drawdown_limit_bps: 15000 })),
  /does not match the v1 evidence contract/,
  "Drawdown limit > 10000 bps must be rejected"
);

// DUR-12: CompatibilityMatrix
const validCompat = {
  compatibility_schema_version: 1,
  matrix_id: "compat.follon.engine-v1",
  engine_version: "follon-core-0.1.0",
  registered_schemas: [
    {
      schema_name: "market.bar.v1",
      current_version: 1,
      oldest_supported_version: 1,
      migration_status: "CURRENT",
    },
    {
      schema_name: "order_intent.v1",
      current_version: 2,
      oldest_supported_version: 1,
      migration_status: "AUTOMATIC_UPGRADE",
    },
  ],
  backward_compatibility_verified: true,
  golden_corpus_size: 450,
  verified_at: "2026-09-01T12:00:00Z",
};

const parsedCompat = parseCompatibilityMatrix(JSON.stringify(validCompat));
assert.equal(parsedCompat.matrix_id, "compat.follon.engine-v1");
assert.equal(parsedCompat.registered_schemas.length, 2);
assert.equal(parsedCompat.backward_compatibility_verified, true);

assert.throws(
  () => parseCompatibilityMatrix(JSON.stringify({ ...validCompat, registered_schemas: [{ ...validCompat.registered_schemas[0], migration_status: "INVALID_STATUS" }] })),
  /does not match the v1 evidence contract/,
  "Invalid migration status must be rejected"
);

// --- 2. Workspace DOM & Cockpit Integration Tests ---

class MockElement {
  constructor(tag) {
    this.tag = tag;
    this.children = [];
    this.textContent = "";
    this.value = "";
    this.hidden = false;
    this.disabled = false;
    this.events = {};
    this.style = {};
  }
  focus() {}
  get rows() { return this.children; }
  append(...items) { this.children.push(...items); }
  replaceChildren(...items) { this.children = items; }
  setAttribute(k, v) { this[k] = v; }
  addEventListener(k, cb) { this.events[k] = cb; }
  fire(k, ev = {}) { this.events[k]?.({ preventDefault() {}, ...ev }); }
  querySelector(sel) {
    return this.children.find((c) => c.tag === sel || c.id === sel.replace("#", "")) ??
      this.children.map((c) => c.querySelector?.(sel)).find(Boolean);
  }
}

globalThis.document = {
  createElement: (tag) => new MockElement(tag),
  body: new MockElement("body"),
  querySelector: () => null,
};
globalThis.Option = class extends MockElement {
  constructor(text, value) {
    super("option");
    this.textContent = text;
    this.value = value;
  }
};

function containsText(node, expected) {
  if (!node) return false;
  return node.textContent?.includes(expected) || node.children?.some((child) => containsText(child, expected));
}

const mockSnapshot = {
  workspace_schema_version: 1,
  generated_at: "2026-09-05T00:00:00Z",
  read_only: true,
  counts: {},
  feature_artifact_counts: {},
  datasets: [],
  notebooks: [],
  backtests: [],
  experiments: [],
  manifests: [],
  events: [],
  journals: [],
  commercial: [],
  execution_evidence: [],
  paper: null,
  live: null,
  operations: null,
  options: null,
  commercial_artifacts: [],
  advanced_evidence: [
    { artifact: "decision-recon.json", category: "decision_reconstruction", data: validRecon },
    { artifact: "counterfactual.json", category: "counterfactual_scenario", data: validScenario },
    { artifact: "data-rights.json", category: "data_rights_and_semantics_receipt", data: validRights },
    { artifact: "snapshot-manifest.json", category: "workspace_snapshot_manifest", data: validManifest },
    { artifact: "attention-budget.json", category: "attention_budget", data: validBudget },
    { artifact: "adversarial-eval.json", category: "adversarial_evaluation", data: validAdvEval },
    { artifact: "strategy-capsule.json", category: "strategy_capsule_manifest", data: validCapsule },
    { artifact: "recovery-drill.json", category: "recovery_drill_result", data: validDrill },
    { artifact: "gateway-matrix.json", category: "gateway_qualification_matrix", data: validGatewayMatrix },
    { artifact: "capital-proposal.json", category: "capital_allocation_proposal", data: validProposal },
    { artifact: "compat-matrix.json", category: "compatibility_matrix", data: validCompat },
  ],
};

const mockContext = {
  status: null,
  features: [],
  artifacts: [],
  workspaceFeatures: [],
  onOpenArtifact: () => {},
};

// 1. Replay & Incidents: Test #explain-moment-panel and #recovery-drill-panel
const replaySummary = new MockElement("div");
const replayCanvas = new MockElement("div");
renderWorkspace(replaySummary, replayCanvas, "replay-incidents", mockSnapshot, mockContext);

const explainPanel = replayCanvas.querySelector("#explain-moment-panel");
assert.ok(explainPanel, "#explain-moment-panel must be rendered in replay-incidents");
assert.ok(containsText(explainPanel, "recon.fill.9b41a2c"), "Reconstruction ID must be rendered");
assert.ok(containsText(explainPanel, "VERIFIED"), "Integrity status must be rendered");
assert.ok(containsText(explainPanel, "CAUSED_SIGNAL"), "Causal DAG relation must be rendered");

const recoveryDrillPanelReplay = replayCanvas.querySelector("#recovery-drill-panel");
assert.ok(recoveryDrillPanelReplay, "#recovery-drill-panel must be rendered in replay-incidents");
assert.ok(containsText(recoveryDrillPanelReplay, "drill.gameday.host-partition.01"), "Drill ID must be rendered");
assert.ok(containsText(recoveryDrillPanelReplay, "SPLIT_BRAIN_HOST_PARTITION"), "Injected fault must be rendered");
assert.ok(containsText(recoveryDrillPanelReplay, "12s / 30s"), "Measured RTO must be rendered");

// 2. Research Lab: Test #input-correction-panel and #counterfactual-panel
const researchSummary = new MockElement("div");
const researchCanvas = new MockElement("div");
renderWorkspace(researchSummary, researchCanvas, "research-lab", mockSnapshot, mockContext);

const inputCorrectionPanel = researchCanvas.querySelector("#input-correction-panel");
assert.ok(inputCorrectionPanel, "#input-correction-panel must be rendered in research-lab");
assert.ok(containsText(inputCorrectionPanel, "drsr.polygon.us-equity-l1"), "Receipt ID must be rendered");
assert.ok(containsText(inputCorrectionPanel, "COMMERCIAL_REPLAY"), "License tier must be rendered");
assert.ok(containsText(inputCorrectionPanel, "9980 bps"), "Semantic parity score must be rendered");

const counterfactualPanel = researchCanvas.querySelector("#counterfactual-panel");
assert.ok(counterfactualPanel, "#counterfactual-panel must be rendered in research-lab");
assert.ok(containsText(counterfactualPanel, "cf.latency-shock.001"), "Scenario ID must be rendered");
assert.ok(containsText(counterfactualPanel, "-420.50000000"), "PnL delta must be rendered");
assert.ok(containsText(counterfactualPanel, "85 bps"), "Drawdown delta must be rendered");

// 3. Strategy Studio: Test #strategy-invalidation-panel and #strategy-capsule-panel
const studioSummary = new MockElement("div");
const studioCanvas = new MockElement("div");
renderWorkspace(studioSummary, studioCanvas, "strategy-studio", mockSnapshot, mockContext);

const invalidationPanel = studioCanvas.querySelector("#strategy-invalidation-panel");
assert.ok(invalidationPanel, "#strategy-invalidation-panel must be rendered in strategy-studio");
assert.ok(containsText(invalidationPanel, "adveval.strat.trend.v1"), "Evaluation ID must be rendered");
assert.ok(containsText(invalidationPanel, "5/5 probes"), "Probe pass count must be rendered");
assert.ok(containsText(invalidationPanel, "10000 bps"), "Composite robustness score must be rendered");

const capsulePanel = studioCanvas.querySelector("#strategy-capsule-panel");
assert.ok(capsulePanel, "#strategy-capsule-panel must be rendered in strategy-studio");
assert.ok(containsText(capsulePanel, "capsule.trend.v1"), "Capsule ID must be rendered");
assert.ok(containsText(capsulePanel, "VERIFIED_PORTABLE"), "Export disposition must be rendered");

// 4. Command Center: Test #away-desk-readiness-panel
const cmdSummary = new MockElement("div");
const cmdCanvas = new MockElement("div");
renderWorkspace(cmdSummary, cmdCanvas, "command-center", mockSnapshot, mockContext);

const awayDeskPanel = cmdCanvas.querySelector("#away-desk-readiness-panel");
assert.ok(awayDeskPanel, "#away-desk-readiness-panel must be rendered in command-center");
assert.ok(containsText(awayDeskPanel, "attn.session.2026-09-01"), "Attention budget ID must be rendered");
assert.ok(containsText(awayDeskPanel, "3500 bps"), "Cognitive load must be rendered");
assert.ok(containsText(awayDeskPanel, "18 suppressed"), "Suppressed duplicates count must be rendered");

// 5. Risk Cockpit: Test #joint-correlation-panel
const riskSummary = new MockElement("div");
const riskCanvas = new MockElement("div");
renderWorkspace(riskSummary, riskCanvas, "risk-cockpit", mockSnapshot, mockContext);

const jointCorrPanel = riskCanvas.querySelector("#joint-correlation-panel");
assert.ok(jointCorrPanel, "#joint-correlation-panel must be rendered in risk-cockpit");
assert.ok(containsText(jointCorrPanel, "cap-prop.2026-09-01.eod"), "Proposal ID must be rendered");
assert.ok(containsText(jointCorrPanel, "1200 bps"), "Target volatility must be rendered");
assert.ok(containsText(jointCorrPanel, "14200 bps"), "Diversification ratio must be rendered");
assert.ok(containsText(jointCorrPanel, "RECOMMENDED"), "Proposal status must be rendered");

// 6. Administration: Test #workspace-rebuild-panel, #gateway-matrix-panel, #compatibility-matrix-panel
const adminSummary = new MockElement("div");
const adminCanvas = new MockElement("div");
renderWorkspace(adminSummary, adminCanvas, "administration", mockSnapshot, mockContext);

const rebuildPanel = adminCanvas.querySelector("#workspace-rebuild-panel");
assert.ok(rebuildPanel, "#workspace-rebuild-panel must be rendered in administration");
assert.ok(containsText(rebuildPanel, "snapshot.2026-09-01.eod"), "Snapshot manifest ID must be rendered");
assert.ok(containsText(rebuildPanel, "5000 / 5000"), "Retained / source events must be rendered");
assert.ok(containsText(rebuildPanel, "drill.gameday.host-partition.01"), "Recovery drill ID must be rendered in rebuild panel");

const gatewayMatrixPanel = adminCanvas.querySelector("#gateway-matrix-panel");
assert.ok(gatewayMatrixPanel, "#gateway-matrix-panel must be rendered in administration");
assert.ok(containsText(gatewayMatrixPanel, "gqm.ibkr.paper-gateway-01"), "Gateway matrix ID must be rendered");
assert.ok(containsText(gatewayMatrixPanel, "fencing epoch 14") || containsText(gatewayMatrixPanel, "14"), "Fencing epoch must be rendered");
assert.ok(containsText(gatewayMatrixPanel, "CERTIFIED"), "Capability state must be rendered");

const compatMatrixPanel = adminCanvas.querySelector("#compatibility-matrix-panel");
assert.ok(compatMatrixPanel, "#compatibility-matrix-panel must be rendered in administration");
assert.ok(containsText(compatMatrixPanel, "compat.follon.engine-v1"), "Compatibility matrix ID must be rendered");
assert.ok(containsText(compatMatrixPanel, "VERIFIED"), "Backward compatibility verification must be rendered");
assert.ok(containsText(compatMatrixPanel, "450 fixtures"), "Golden corpus count must be rendered");

console.log("Enduring capabilities regression tests (DUR-01 through DUR-12) passed cleanly!");
