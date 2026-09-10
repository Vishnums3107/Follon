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
  parseMarketScanner,
  parseNewsRevisionTimeline,
  parseStrategyCompositionSpec,
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

// SOLO-04: MarketScanner
const validScanner = {
  scanner_schema_version: 1,
  scanner_id: "scan.us-equity-momentum",
  universe_id: "univ.us-equity.liquid-top500",
  as_of_time: "2026-09-01T15:30:00Z",
  indicator_columns: [
    {
      column_id: "col.momentum-20d",
      name: "20-Day Momentum",
      timeframe: "1D",
      definition: "Rate of change over 20 daily close prices in basis points",
    },
    {
      column_id: "col.rsi-14",
      name: "14-Period RSI",
      timeframe: "1D",
      definition: "Wilder Relative Strength Index over 14 daily periods",
    },
  ],
  candidates: [
    {
      rank: 1,
      instrument_id: "inst.us_equity.spy",
      symbol: "SPY",
      close_price: "560.25000000",
      momentum_score_bps: 420,
      rsi_14: "62.45000000",
      matched_conditions: ["BREAKOUT_20D_HIGH", "RSI_BETWEEN_50_AND_70"],
      rationale: "Strong upward trend continuation above 20-day high with unoverbought RSI",
    },
    {
      rank: 2,
      instrument_id: "inst.us_equity.qqq",
      symbol: "QQQ",
      close_price: "485.10000000",
      momentum_score_bps: 385,
      rsi_14: "58.12000000",
      matched_conditions: ["VOLUME_SURGE_150PCT", "MOMENTUM_POSITIVE"],
      rationale: "High relative volume with positive 20-day momentum score",
    },
  ],
  quarantined_count: 0,
  created_at: "2026-09-01T15:30:05Z",
};

const parsedScanner = parseMarketScanner(JSON.stringify(validScanner));
assert.equal(parsedScanner.scanner_id, "scan.us-equity-momentum");
assert.equal(parsedScanner.candidates.length, 2);
assert.equal(parsedScanner.candidates[0].symbol, "SPY");

assert.throws(
  () => parseMarketScanner(JSON.stringify({ ...validScanner, scanner_schema_version: 2 })),
  /does not match the v1 evidence contract/,
  "Invalid scanner schema version must be rejected"
);

// DATA-03: NewsRevisionTimeline
const validNewsTimeline = {
  revision_timeline_schema_version: 1,
  timeline_id: "rev.timeline.earnings-001",
  target_event_id: "evt.news.001",
  chain: [
    {
      version_sequence: 1,
      received_at: "2026-09-01T11:00:00Z",
      source_id: "DOW_JONES",
      kind: "INITIAL_REPORT",
      headline: "Acme Corp reports Q3 EPS $1.20 vs $1.10 expected",
      entity_confidence_bps: 9500,
      content_hash: "a".repeat(64),
      supersedes_sequence: null,
    },
    {
      version_sequence: 2,
      received_at: "2026-09-01T11:05:00Z",
      source_id: "REUTERS",
      kind: "SYNDICATED_DUPLICATE",
      headline: "Acme Corp beats earnings estimates in third quarter",
      entity_confidence_bps: 9200,
      content_hash: "b".repeat(64),
      supersedes_sequence: 1,
    },
    {
      version_sequence: 3,
      received_at: "2026-09-01T11:15:00Z",
      source_id: "DOW_JONES",
      kind: "CORRECTION",
      headline: "CORRECTION: Acme Corp Q3 GAAP EPS was $1.15, adjusted $1.20",
      entity_confidence_bps: 9800,
      content_hash: "c".repeat(64),
      supersedes_sequence: 1,
    },
  ],
  conflict_detected: false,
  created_at: "2026-09-01T11:15:05Z",
};

const parsedNewsTimeline = parseNewsRevisionTimeline(JSON.stringify(validNewsTimeline));
assert.equal(parsedNewsTimeline.timeline_id, "rev.timeline.earnings-001");
assert.equal(parsedNewsTimeline.chain.length, 3);
assert.equal(parsedNewsTimeline.chain[2].kind, "CORRECTION");

assert.throws(
  () => parseNewsRevisionTimeline(JSON.stringify({ ...validNewsTimeline, chain: [{ ...validNewsTimeline.chain[0], kind: "INVALID_KIND" }] })),
  /does not match the v1 evidence contract/,
  "Invalid news revision kind must be rejected"
);

// RES-02: StrategyCompositionSpec
const validCompositionSpec = {
  composition_schema_version: 1,
  composition_id: "comp.strat.trend-v1",
  strategy_id: "strat.trend.v1",
  strategy_version: "1.0.0",
  signals: [
    {
      signal_id: "sig.ema-cross",
      indicator_ref: "ind.ema.12-26",
      condition: "FAST_EMA > SLOW_EMA",
      weight_bps: 6000,
    },
    {
      signal_id: "sig.vol-filter",
      indicator_ref: "ind.atr.14",
      condition: "ATR_14 > ATR_BASELINE",
      weight_bps: 4000,
    },
  ],
  sizing_rule: {
    sizing_type: "VOLATILITY_TARGETED",
    target_value: "1500_BPS_ANNUAL_VOL",
  },
  entry_criteria: [
    "SIGNAL_SUM_WEIGHT_BPS >= 5000",
    "MARKET_SESSION_NORMAL",
  ],
  exit_criteria: [
    "TRAILING_STOP_TRIGGERED",
    "FAST_EMA < SLOW_EMA",
  ],
  portfolio_constraints: {
    max_leverage_bps: 10000,
    max_single_position_bps: 2500,
    stop_loss_pct: "2.50000000",
  },
  code_hash: "d".repeat(64),
  visual_representation_hash: "e".repeat(64),
  created_at: "2026-09-01T09:00:00Z",
};

const parsedCompositionSpec = parseStrategyCompositionSpec(JSON.stringify(validCompositionSpec));
assert.equal(parsedCompositionSpec.composition_id, "comp.strat.trend-v1");
assert.equal(parsedCompositionSpec.signals.length, 2);
assert.equal(parsedCompositionSpec.sizing_rule.sizing_type, "VOLATILITY_TARGETED");

assert.throws(
  () => parseStrategyCompositionSpec(JSON.stringify({ ...validCompositionSpec, sizing_rule: { sizing_type: "INVALID_SIZING", target_value: "100" } })),
  /does not match the v1 evidence contract/,
  "Invalid sizing type must be rejected"
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
  createElementNS: (_ns, tag) => new MockElement(tag),
  createTextNode: (text) => ({ textContent: text }),
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
    { artifact: "market-scanner.json", category: "market_scanner", data: validScanner },
    { artifact: "news-revision-timeline.json", category: "news_revision_timeline", data: validNewsTimeline },
    { artifact: "strategy-composition-spec.json", category: "strategy_composition_spec", data: validCompositionSpec },
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

// The causal-lineage DAG visual must be driven by the real reconstruction record, never fabricated content.
assert.ok(containsText(explainPanel, "market.bar.v1"), "Real causal-chain event type must be rendered in the DAG visual");
assert.ok(containsText(explainPanel, "Market bar SPY close 500.00"), "Real causal-chain summary must be rendered in the DAG visual");
assert.ok(containsText(explainPanel, "market-feed"), "Real causal-chain actor must be rendered in the DAG visual");
assert.ok(!containsText(explainPanel, "DATA.FEED"), "Fabricated placeholder node label must not be rendered");
assert.ok(!containsText(explainPanel, "T0 +0.0ms"), "Fabricated placeholder timing must not be rendered");
assert.ok(!containsText(explainPanel, "VERIFIED DETERMINISTIC CHAIN"), "Fabricated static badge text must not be rendered");

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

const strategyCompPanel = studioCanvas.querySelector("#strategy-composition-panel");
assert.ok(strategyCompPanel, "#strategy-composition-panel must be rendered in strategy-studio");
assert.ok(containsText(strategyCompPanel, "strat.trend.v1 (1.0.0)"), "Strategy ID and version must be rendered");
assert.ok(containsText(strategyCompPanel, "sig.ema-cross"), "Signal ID must be rendered");
assert.ok(containsText(strategyCompPanel, "VOLATILITY_TARGETED"), "Sizing rule must be rendered");

// 4. Command Center: Test #away-desk-readiness-panel and #market-scanner-panel
const cmdSummary = new MockElement("div");
const cmdCanvas = new MockElement("div");
renderWorkspace(cmdSummary, cmdCanvas, "command-center", mockSnapshot, mockContext);

const awayDeskPanel = cmdCanvas.querySelector("#away-desk-readiness-panel");
assert.ok(awayDeskPanel, "#away-desk-readiness-panel must be rendered in command-center");
assert.ok(containsText(awayDeskPanel, "attn.session.2026-09-01"), "Attention budget ID must be rendered");
assert.ok(containsText(awayDeskPanel, "3500 bps"), "Cognitive load must be rendered");
assert.ok(containsText(awayDeskPanel, "18 suppressed"), "Suppressed duplicates count must be rendered");

// The attention gauge visual must be driven by the real attention-budget record, never a fabricated default.
assert.ok(containsText(awayDeskPanel, "65.0%"), "Reserve capacity computed from the real load must be rendered");
assert.ok(containsText(awayDeskPanel, "4.5 / hr"), "Real interruption rate must be rendered in the gauge");
assert.ok(containsText(awayDeskPanel, "1 active / 18 suppressed"), "Real alarm counts must be rendered in the gauge caption");
assert.ok(!containsText(awayDeskPanel, "2.1 / hr"), "Fabricated interruption rate must not be rendered");

const marketScannerPanel = cmdCanvas.querySelector("#market-scanner-panel");
assert.ok(marketScannerPanel, "#market-scanner-panel must be rendered in command-center");
assert.ok(containsText(marketScannerPanel, "#1"), "Candidate rank must be rendered");
assert.ok(containsText(marketScannerPanel, "SPY"), "Candidate symbol must be rendered");
assert.ok(containsText(marketScannerPanel, "560.25000000"), "Close price must be rendered");
assert.ok(containsText(marketScannerPanel, "420 bps"), "Momentum score must be rendered");

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

// 7. News Cockpit: Test #news-revision-panel
const newsSummary = new MockElement("div");
const newsCanvas = new MockElement("div");
renderWorkspace(newsSummary, newsCanvas, "news-cockpit", mockSnapshot, mockContext);

const newsRevPanel = newsCanvas.querySelector("#news-revision-panel");
assert.ok(newsRevPanel, "#news-revision-panel must be rendered in news-cockpit");
assert.ok(containsText(newsRevPanel, "#1"), "Version sequence 1 must be rendered");
assert.ok(containsText(newsRevPanel, "INITIAL_REPORT"), "Initial report kind must be rendered");
assert.ok(containsText(newsRevPanel, "#3"), "Version sequence 3 must be rendered");
assert.ok(containsText(newsRevPanel, "CORRECTION"), "Correction kind must be rendered");
assert.ok(containsText(newsRevPanel, "Acme Corp"), "Headline text must be rendered");

console.log("Enduring capabilities regression tests (DUR-01 through DUR-12) passed cleanly!");
