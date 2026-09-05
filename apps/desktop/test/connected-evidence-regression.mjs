import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseChampionChallengerEvaluation,
  parseCapabilityExecutionPlanner,
  parseOperationsDiagnosisRunbook,
  parseModelEvaluationBenchmark,
  parseStrategyCapsuleManifest,
  parseMultiAssetExpansionPlan,
} from "../dist/evidence.js";
import { renderWorkspace } from "../dist/workspaces.js";

const testDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(testDir, "..", "..", "..");

// --- 1. Contract & Parser Tests ---

// A. ChampionChallengerEvaluation (RES-08)
const validChamp = {
  champion_challenger_schema_version: 1,
  evaluation_id: "eval.champ.trend-v1",
  champion_strategy_id: "strat.trend.v1",
  challenger_strategy_id: "strat.trend.v2",
  evaluation_window_start: "2026-06-01T00:00:00Z",
  evaluation_window_end: "2026-09-01T23:59:59Z",
  champion_return_bps: 840,
  challenger_return_bps: 1120,
  champion_max_drawdown_bps: 420,
  challenger_max_drawdown_bps: 380,
  information_ratio_diff_bps: 45,
  drift_detected: false,
  recommendation: "CONTINUE_SHADOW_MONITORING",
  created_at: "2026-09-04T12:00:00Z",
};

const parsedChamp = parseChampionChallengerEvaluation(JSON.stringify(validChamp));
assert.equal(parsedChamp.evaluation_id, "eval.champ.trend-v1");
assert.equal(parsedChamp.recommendation, "CONTINUE_SHADOW_MONITORING");
assert.equal(parsedChamp.drift_detected, false);

assert.throws(
  () => parseChampionChallengerEvaluation(JSON.stringify({ ...validChamp, recommendation: "INVALID_REC" })),
  /does not match the v1 evidence contract/,
  "Invalid recommendation must be rejected"
);

// B. CapabilityExecutionPlanner (EXEC-04)
const validPlanner = {
  planner_schema_version: 1,
  plan_id: "plan.exec.twap-01",
  parent_order_id: "ord.9b41",
  target_venue: "venue.nasdaq",
  algorithm: "TWAP_SLICED",
  max_volume_participation_pct: "15.0",
  passive_pegging_offset_bps: 10,
  schedule_slices: [
    {
      slice_sequence: 1,
      planned_release_time: "2026-09-04T14:30:00Z",
      allocated_quantity: "33.00",
      order_kind: "LIMIT_PASSIVE",
    },
    {
      slice_sequence: 2,
      planned_release_time: "2026-09-04T14:35:00Z",
      allocated_quantity: "33.00",
      order_kind: "LIMIT_PASSIVE",
    },
  ],
  supported_capabilities_verified: true,
  disposition: "VALIDATED_FOR_DISPATCH",
  created_at: "2026-09-04T14:25:00Z",
};

const parsedPlanner = parseCapabilityExecutionPlanner(JSON.stringify(validPlanner));
assert.equal(parsedPlanner.plan_id, "plan.exec.twap-01");
assert.equal(parsedPlanner.disposition, "VALIDATED_FOR_DISPATCH");
assert.equal(parsedPlanner.schedule_slices.length, 2);

assert.throws(
  () => parseCapabilityExecutionPlanner(JSON.stringify({ ...validPlanner, schedule_slices: [] })),
  /does not match the v1 evidence contract/,
  "Empty schedule slices must be rejected"
);

// C. OperationsDiagnosisRunbook (AI-05)
const validDiagnosis = {
  diagnosis_schema_version: 1,
  diagnosis_id: "diag.ops.feed-stale.01",
  incident_id: "inc.feed.001",
  failing_component: "US-Equities Quote Feed",
  root_cause_summary: "Quote feed latency exceeded 3s threshold due to gateway reconnect",
  cited_evidence_ids: ["ev.quote.stale.001", "heartbeat.feed.01"],
  proposed_runbook_steps: [
    {
      step_number: 1,
      action_name: "Restart Feed Receiver",
      target_service: "feed-receiver",
      command_template: "docker restart follon-feed-receiver",
      is_idempotent: true,
    },
  ],
  idempotency_certified: true,
  trading_path_isolated: true,
  approval_required: "OPERATOR_CONFIRMATION",
  created_at: "2026-09-04T15:00:00Z",
};

const parsedDiagnosis = parseOperationsDiagnosisRunbook(JSON.stringify(validDiagnosis));
assert.equal(parsedDiagnosis.diagnosis_id, "diag.ops.feed-stale.01");
assert.equal(parsedDiagnosis.idempotency_certified, true);
assert.equal(parsedDiagnosis.trading_path_isolated, true);

assert.throws(
  () => parseOperationsDiagnosisRunbook(JSON.stringify({ ...validDiagnosis, cited_evidence_ids: [] })),
  /does not match the v1 evidence contract/,
  "Empty cited evidence IDs must be rejected"
);

// D. ModelEvaluationBenchmark (AI-06)
const validBenchmark = {
  benchmark_schema_version: 1,
  benchmark_id: "eval.model.gemini-pro",
  model_identifier: "gemini-1.5-pro",
  evaluation_dataset_id: "ds.eval.research-ops.v1",
  factuality_score_bps: 9850,
  citation_precision_bps: 9920,
  injection_resistance_score_bps: 9980,
  hallucination_rate_bps: 12,
  average_latency_ms: 480,
  token_cost_usd_per_million: "1.25",
  disposition: "QUALIFIED_FOR_ASSISTANCE",
  evaluated_at: "2026-09-04T16:00:00Z",
};

const parsedBenchmark = parseModelEvaluationBenchmark(JSON.stringify(validBenchmark));
assert.equal(parsedBenchmark.benchmark_id, "eval.model.gemini-pro");
assert.equal(parsedBenchmark.disposition, "QUALIFIED_FOR_ASSISTANCE");
assert.equal(parsedBenchmark.factuality_score_bps, 9850);

assert.throws(
  () => parseModelEvaluationBenchmark(JSON.stringify({ ...validBenchmark, factuality_score_bps: 15000 })),
  /does not match the v1 evidence contract/,
  "Out-of-range factuality score must be rejected"
);

// E. StrategyCapsuleManifest (ASSET-04)
const validCapsule = {
  capsule_schema_version: 1,
  capsule_id: "capsule.strat.trend-01",
  strategy_id: "strat.trend.v1",
  strategy_version: "2026-01-01.1",
  bundle_sha256: "1111111111111111111111111111111111111111111111111111111111111111",
  configuration_sha256: "2222222222222222222222222222222222222222222222222222222222222222",
  dependency_lockfile_sha256: "3333333333333333333333333333333333333333333333333333333333333333",
  runtime_target: "Python 3.11 / Rust Core v1",
  evaluation_receipt_id: "rcpt.eval.trend.001",
  replay_instruction_command: "follon-backtest --capsule var/capsule.strat.trend-01.tar.gz",
  export_disposition: "VERIFIED_PORTABLE",
  packaged_at: "2026-09-04T17:00:00Z",
};

const parsedCapsule = parseStrategyCapsuleManifest(JSON.stringify(validCapsule));
assert.equal(parsedCapsule.capsule_id, "capsule.strat.trend-01");
assert.equal(parsedCapsule.export_disposition, "VERIFIED_PORTABLE");

assert.throws(
  () => parseStrategyCapsuleManifest(JSON.stringify({ ...validCapsule, bundle_sha256: "invalid-hash" })),
  /does not match the v1 evidence contract/,
  "Invalid hash format must be rejected"
);

// F. MultiAssetExpansionPlan (PORT-02)
const validExpansion = {
  expansion_schema_version: 1,
  plan_id: "plan.asset.opt-roll.01",
  asset_class: "EQUITY_OPTION",
  underlying_universe: ["inst.us_equity.spy"],
  lifecycle_actions: [
    {
      action_id: "act.roll.spy.01",
      instrument_id: "inst.opt.spy.510c",
      action_kind: "OPTION_ROLL",
      target_date: "2026-09-18T20:00:00Z",
      contract_quantity: 10,
      estimated_cash_flow_usd: "480.00",
    },
  ],
  margin_requirement_usd: "15000.00",
  settlement_currency: "USD",
  reconciliation_clean: true,
  operational_verdict: "READY_FOR_LIFECYCLE_EXECUTION",
  created_at: "2026-09-04T18:00:00Z",
};

const parsedExpansion = parseMultiAssetExpansionPlan(JSON.stringify(validExpansion));
assert.equal(parsedExpansion.plan_id, "plan.asset.opt-roll.01");
assert.equal(parsedExpansion.operational_verdict, "READY_FOR_LIFECYCLE_EXECUTION");
assert.equal(parsedExpansion.lifecycle_actions.length, 1);

assert.throws(
  () => parseMultiAssetExpansionPlan(JSON.stringify({ ...validExpansion, lifecycle_actions: [] })),
  /does not match the v1 evidence contract/,
  "Empty lifecycle actions must be rejected"
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
};

mockSnapshot.advanced_evidence = [
  { artifact: "champion.json", category: "champion_challenger_evaluation", data: validChamp },
  { artifact: "planner.json", category: "capability_execution_planner", data: validPlanner },
  { artifact: "diagnosis.json", category: "operations_diagnosis_runbook", data: validDiagnosis },
  { artifact: "model-evaluation.json", category: "model_evaluation_benchmark", data: validBenchmark },
  { artifact: "capsule.json", category: "strategy_capsule_manifest", data: validCapsule },
  { artifact: "expansion.json", category: "multi_asset_expansion_plan", data: validExpansion },
  {
    artifact: "invalid-champion.json",
    category: "champion_challenger_evaluation",
    data: { ...validChamp, recommendation: "NOT_A_VALID_RECOMMENDATION" },
  },
];

function containsText(node, expected) {
  return node.textContent?.includes(expected) || node.children?.some((child) => containsText(child, expected));
}

const mockContext = {
  status: null,
  features: [],
  artifacts: [],
  workspaceFeatures: [],
  onOpenArtifact: () => {},
};

// Test Command Center additions (Experience 5)
const cmdSummary = new MockElement("div");
const cmdCanvas = new MockElement("div");
renderWorkspace(cmdSummary, cmdCanvas, "command-center", mockSnapshot, mockContext);

const awayDeskPanel = cmdCanvas.children.find((c) => c.id === "away-desk-readiness-panel");
assert.ok(awayDeskPanel !== undefined, "Away desk readiness panel must exist in Command Center");

// Test Research Lab additions (Experience 4)
const resSummary = new MockElement("div");
const resCanvas = new MockElement("div");
renderWorkspace(resSummary, resCanvas, "research-lab", mockSnapshot, mockContext);

const inputCorPanel = resCanvas.children.find((c) => c.id === "input-correction-panel");
assert.ok(inputCorPanel !== undefined, "Input correction panel must exist in Research Lab");

// Test Strategy Studio additions (RES-08, Experience 2)
const stratSummary = new MockElement("div");
const stratCanvas = new MockElement("div");
renderWorkspace(stratSummary, stratCanvas, "strategy-studio", mockSnapshot, mockContext);

const champPanel = stratCanvas.children.find((c) => c.id === "champion-challenger-panel");
assert.ok(champPanel !== undefined, "Champion challenger panel must exist in Strategy Studio");
assert.ok(containsText(champPanel, validChamp.challenger_strategy_id), "Champion panel must render its typed artifact");
assert.ok(!containsText(champPanel, "NOT_A_VALID_RECOMMENDATION"), "Malformed advanced evidence must not render");

const invalidPanel = stratCanvas.children.find((c) => c.id === "strategy-invalidation-panel");
assert.ok(invalidPanel !== undefined, "Strategy invalidation explorer must exist in Strategy Studio");

// Test Execution Blotter additions (EXEC-04)
const execSummary = new MockElement("div");
const execCanvas = new MockElement("div");
renderWorkspace(execSummary, execCanvas, "execution-blotter", mockSnapshot, mockContext);

const execPlanPanel = execCanvas.children.find((c) => c.id === "execution-planner-panel");
assert.ok(execPlanPanel !== undefined, "Execution planner panel must exist in Execution Blotter");
assert.ok(containsText(execPlanPanel, validPlanner.plan_id), "Execution planner must render its typed artifact");

// Test Risk Cockpit additions (Experience 3)
const riskSummary = new MockElement("div");
const riskCanvas = new MockElement("div");
renderWorkspace(riskSummary, riskCanvas, "risk-cockpit", mockSnapshot, mockContext);

const jointCorrPanel = riskCanvas.children.find((c) => c.id === "joint-correlation-panel");
assert.ok(jointCorrPanel !== undefined, "Joint correlation panel must exist in Risk Cockpit");

// Test Portfolio additions (PORT-02)
const portSummary = new MockElement("div");
const portCanvas = new MockElement("div");
renderWorkspace(portSummary, portCanvas, "portfolio", mockSnapshot, mockContext);

const multiAssetPanel = portCanvas.children.find((c) => c.id === "multi-asset-panel");
assert.ok(multiAssetPanel !== undefined, "Multi-asset lifecycle panel must exist in Portfolio");
assert.ok(containsText(multiAssetPanel, validExpansion.plan_id), "Multi-asset panel must render its typed artifact");

// Test Replay & Incidents additions (Experience 1)
const repSummary = new MockElement("div");
const repCanvas = new MockElement("div");
renderWorkspace(repSummary, repCanvas, "replay-incidents", mockSnapshot, mockContext);

const explainPanel = repCanvas.children.find((c) => c.id === "explain-moment-panel");
assert.ok(explainPanel !== undefined, "Explain this moment panel must exist in Replay & Incidents");

// Test Marketplace additions (ASSET-04)
const marketSummary = new MockElement("div");
const marketCanvas = new MockElement("div");
renderWorkspace(marketSummary, marketCanvas, "marketplace", mockSnapshot, mockContext);

const capsulePanel = marketCanvas.children.find((c) => c.id === "strategy-capsule-panel");
assert.ok(capsulePanel !== undefined, "Strategy capsule panel must exist in Marketplace");
assert.ok(containsText(capsulePanel, validCapsule.capsule_id), "Capsule panel must render its typed artifact");

// Test Administration additions (AI-05, AI-06, Experience 6)
const adminSummary = new MockElement("div");
const adminCanvas = new MockElement("div");
renderWorkspace(adminSummary, adminCanvas, "administration", mockSnapshot, mockContext);

const opsAssPanel = adminCanvas.children.find((c) => c.id === "operations-assistant-panel");
assert.ok(opsAssPanel !== undefined, "Operations assistant panel must exist in Administration");
assert.ok(containsText(opsAssPanel, validDiagnosis.diagnosis_id), "Operations panel must render its typed artifact");

const modelEvalPanel = adminCanvas.children.find((c) => c.id === "model-evaluation-panel");
assert.ok(modelEvalPanel !== undefined, "Model evaluation benchmark panel must exist in Administration");
assert.ok(containsText(modelEvalPanel, validBenchmark.model_identifier), "Model evaluation panel must render its typed artifact");

const rebuildPanel = adminCanvas.children.find((c) => c.id === "workspace-rebuild-panel");
assert.ok(rebuildPanel !== undefined, "Workspace rebuild exercise panel must exist in Administration");

// --- 3. Schema File Existence and Syntax Assertions ---

const schemas = [
  "champion-challenger-evaluation.schema.json",
  "capability-execution-planner.schema.json",
  "operations-diagnosis-runbook.schema.json",
  "model-evaluation-benchmark.schema.json",
  "strategy-capsule-manifest.schema.json",
  "multi-asset-expansion-plan.schema.json",
];

for (const schemaName of schemas) {
  const schemaPath = resolve(repoRoot, "contracts", "json-schema", "v1", schemaName);
  const raw = await readFile(schemaPath, "utf8");
  const parsed = JSON.parse(raw);
  assert.ok(parsed.title.length > 0, `${schemaName} must have a title`);
  assert.ok(Array.isArray(parsed.required), `${schemaName} must specify required fields`);
  assert.equal(parsed.additionalProperties, false, `${schemaName} must disallow additional properties`);
}

console.log("Connected evidence & advanced capabilities regression tests passed cleanly");
