import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseResearchHypothesis,
  parseExperimentLineage,
  parseResearchJob,
  parseAssistantEvidence,
} from "../dist/evidence.js";
import { renderWorkspace } from "../dist/workspaces.js";

const testDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(testDir, "..", "..", "..");

// --- 1. Contract & Parser Tests ---

// A. ResearchHypothesis (RES-01)
const validHypothesis = {
  hypothesis_schema_version: 1,
  hypothesis_id: "hyp.momentum-cross-v1",
  title: "Moving Average Trend Continuation on Volume Surge",
  mechanism: "Volume-weighted momentum breakout continuation past 20-day high with trailing stop",
  universe: ["inst.us_equity.spy", "inst.us_equity.qqq"],
  evaluation_horizon: {
    start_time: "2026-01-01T00:00:00Z",
    end_time: "2026-06-30T23:59:59Z",
    holding_period: "1 to 5 market bars",
  },
  assumptions: [
    "Continuous market bar feeds without unadjusted splits",
    "Slippage bounded to 5 bps under liquid market sessions",
  ],
  failure_criteria: [
    "Gross drawdown exceeds 1200 bps",
    "Realized annual Sharpe ratio falls below 0.50",
  ],
  frozen_evaluation_plan: {
    dataset_id: "ds.sp500.bars.v1",
    dataset_version: "2026-01-01.1",
    dataset_hash: "a".repeat(64),
    cost_model: "tier-1-maker-taker",
    slippage_bps: 5,
    fee_model: "fixed-exchange-per-share",
  },
  predecessor_id: null,
  status: "FROZEN",
  created_at: "2026-01-01T00:00:00Z",
  frozen_at: "2026-01-01T01:00:00Z",
};

const parsedHyp = parseResearchHypothesis(JSON.stringify(validHypothesis));
assert.equal(parsedHyp.hypothesis_id, "hyp.momentum-cross-v1");
assert.equal(parsedHyp.status, "FROZEN");
assert.equal(parsedHyp.frozen_evaluation_plan.slippage_bps, 5);

assert.throws(
  () => parseResearchHypothesis(JSON.stringify({ ...validHypothesis, status: "INVALID_STATUS" })),
  /does not match the v1 evidence contract/,
  "Invalid status must be rejected"
);

assert.throws(
  () => parseResearchHypothesis(JSON.stringify({ ...validHypothesis, universe: [] })),
  /does not match the v1 evidence contract/,
  "Empty universe must be rejected"
);

// B. ExperimentLineage (RES-04)
const validLineage = {
  lineage_schema_version: 1,
  lineage_id: "lin.trend-opt-001",
  hypothesis_id: "hyp.momentum-cross-v1",
  parent_run_ids: ["run.benchmark.001"],
  input_fingerprints: [{ name: "strategy-bundle", fingerprint: "b".repeat(64) }],
  output_fingerprints: [{ name: "events-ndjson", fingerprint: "c".repeat(64) }],
  candidate_trials: [
    {
      trial_id: "trial.001",
      specification_hash: "d".repeat(64),
      return_bps: "1420.00",
      max_drawdown_bps: "850.00",
      disposition: "BENCHMARK",
    },
    {
      trial_id: "trial.002",
      specification_hash: "e".repeat(64),
      return_bps: "-310.50",
      max_drawdown_bps: "1420.00",
      disposition: "REJECTED",
    },
    {
      trial_id: "trial.003",
      specification_hash: "f".repeat(64),
      return_bps: "1890.25",
      max_drawdown_bps: "710.00",
      disposition: "PROMOTED",
    },
  ],
  failed_candidates_count: 1,
  rejection_reasons: [
    { trial_id: "trial.002", reason: "Excessive turnover and fee drag from whipsaw signals" },
  ],
  created_at: "2026-01-02T00:00:00Z",
};

const parsedLineage = parseExperimentLineage(JSON.stringify(validLineage));
assert.equal(parsedLineage.candidate_trials.length, 3);
assert.equal(parsedLineage.rejection_reasons.length, 1);
assert.equal(parsedLineage.rejection_reasons[0].trial_id, "trial.002");

// C. ResearchJob
const validJob = {
  job_schema_version: 1,
  job_id: "job.eval.001",
  idempotency_key: "idem.eval.001",
  strategy_id: "strat.trend.v1",
  strategy_version: "1.0.0",
  dataset_id: "ds.sp500.bars.v1",
  dataset_version: "2026-01-01.1",
  frozen_specification_hash: "1".repeat(64),
  state_version: 1,
  state: "QUEUED",
  worker_lease: null,
  output_manifest_hash: null,
  failure_reason: null,
  created_at: "2026-01-02T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
};

const parsedJob = parseResearchJob(JSON.stringify(validJob));
assert.equal(parsedJob.job_id, "job.eval.001");
assert.equal(parsedJob.state, "QUEUED");

// D. AssistantEvidence (AI-01)
const validAssistant = {
  assistant_evidence_schema_version: 1,
  query_id: "query.exp.7f2a",
  model_version: "follon-copilot-v1",
  prompt_template_version: "risk-explainer-v1",
  retrieved_record_ids: ["risk.decision.v1", "conf.risk.v1"],
  generated_output: "Pre-trade risk rejected order: gross exposure of 105,000 USD exceeded account ceiling of 100,000 USD.",
  tool_attempts: [
    {
      tool_name: "query_evidence",
      arguments_hash: "2".repeat(64),
      status: "SUCCESS",
      evidence_id: "risk.decision.v1",
    },
  ],
  uncertainty_score_bps: 250,
  human_disposition: "ACCEPTED",
  created_at: "2026-01-02T00:00:00Z",
};

const parsedAssistant = parseAssistantEvidence(JSON.stringify(validAssistant));
assert.equal(parsedAssistant.query_id, "query.exp.7f2a");
assert.equal(parsedAssistant.human_disposition, "ACCEPTED");
assert.equal(parsedAssistant.uncertainty_score_bps, 250);

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

const mockSnapshot = {
  workspace_schema_version: 1,
  generated_at: "2026-09-05T00:00:00Z",
  read_only: true,
  counts: {},
  feature_artifact_counts: {},
  datasets: [
    { dataset_id: "ds.sp500.bars.v1", name: "sp500.bars.csv", storage_format: "Parquet", rows: 12500, columns: ["time", "open", "high", "low", "close"], modified_at: "2026-09-05T00:00:00Z" },
  ],
  notebooks: [],
  backtests: [
    {
      artifact: "backtest-run-001.json",
      specification: { strategy_version: "strat.trend.v1", dataset: { dataset_id: "ds.sp500.bars.v1" } },
      performance: { trade_count: "24", net_pnl: "1890.25", return_bps: "1890.25", max_drawdown_bps: "710.00" },
      report: {},
      artifact_fingerprint: "1".repeat(64),
    },
  ],
  experiments: [
    {
      artifact: "exp-001.json",
      data: {
        experiment_id: "trend-v1",
        run_id: "run-001",
        artifact_fingerprint: "2".repeat(64),
        event_output_hash: "3".repeat(64),
        tags: { mechanism: "Breakout trend following", universe: "inst.us_equity.spy" },
      },
    },
  ],
  manifests: [],
  events: [
    {
      artifact: "events.ndjson",
      data: {
        event_id: "evt.001",
        event_type: "market.bar.v1",
        event_time: "2026-01-01T09:30:00Z",
        actor: "feed",
        source: "market",
        correlation_id: "corr.001",
        causation_id: null,
        payload: { open: "450.00", close: "451.00" },
      },
    },
    {
      artifact: "events.ndjson",
      data: {
        event_id: "evt.002",
        event_type: "intent.created.v1",
        event_time: "2026-01-01T09:30:01Z",
        actor: "strat.trend.v1",
        source: "strategy",
        correlation_id: "corr.001",
        causation_id: "evt.001",
        payload: { order_type: "LIMIT", quantity: "100" },
      },
    },
    {
      artifact: "events.ndjson",
      data: {
        event_id: "evt.003",
        event_type: "risk.decision.v1",
        event_time: "2026-01-01T09:30:02Z",
        actor: "risk_engine",
        source: "risk",
        correlation_id: "corr.001",
        causation_id: "evt.002",
        payload: { approved: true },
      },
    },
    {
      artifact: "events.ndjson",
      data: {
        event_id: "evt.004",
        event_type: "order.state_changed.v1",
        event_time: "2026-01-01T09:30:03Z",
        actor: "oms",
        source: "oms",
        correlation_id: "corr.001",
        causation_id: "evt.003",
        payload: { status: "NEW" },
      },
    },
  ],
  journals: [],
  commercial: [],
  execution_evidence: [],
  paper: null,
  live: null,
  operations: null,
  options: null,
  commercial_artifacts: [],
};

const mockContext = {
  status: null,
  features: [],
  artifacts: [],
  workspaceFeatures: [],
  onOpenArtifact: () => {},
};

// Test Research Lab additions (RES-01, DATA-01)
const labSummary = new MockElement("div");
const labCanvas = new MockElement("div");
renderWorkspace(labSummary, labCanvas, "research-lab", mockSnapshot, mockContext);

const hypPanel = labCanvas.children.find((c) => c.id === "hypotheses-panel");
assert.ok(hypPanel !== undefined, "Hypothesis notebook panel must exist in Research Lab");

const qualityPanel = labCanvas.children.find((c) => c.id === "data-quality-console");
assert.ok(qualityPanel !== undefined, "Data quality console panel must exist in Research Lab");

// Test Strategy Studio additions (RES-02, AI-01)
const stratSummary = new MockElement("div");
const stratCanvas = new MockElement("div");
renderWorkspace(stratSummary, stratCanvas, "strategy-studio", mockSnapshot, mockContext);

const compPanel = stratCanvas.children.find((c) => c.id === "strategy-composition-panel");
assert.ok(compPanel !== undefined, "Strategy composition studio panel must exist in Strategy Studio");

const copilotPanel = stratCanvas.children.find((c) => c.id === "research-copilot-panel");
assert.ok(copilotPanel !== undefined, "Read-only research copilot panel must exist in Strategy Studio");

// Test Backtest Explorer additions (RES-04)
const backtestSummary = new MockElement("div");
const backtestCanvas = new MockElement("div");
renderWorkspace(backtestSummary, backtestCanvas, "backtest-explorer", mockSnapshot, mockContext);

const failedIdeaPanel = backtestCanvas.children.find((c) => c.id === "failed-idea-memory");
assert.ok(failedIdeaPanel !== undefined, "Failed-idea memory panel must exist in Backtest Explorer");

// Test Replay & Incidents additions (RES-03)
const replaySummary = new MockElement("div");
const replayCanvas = new MockElement("div");
renderWorkspace(replaySummary, replayCanvas, "replay-incidents", mockSnapshot, mockContext);

const debuggerPanel = replayCanvas.children.find((c) => c.id === "event-debugger");
assert.ok(debuggerPanel !== undefined, "Event-by-event debugger panel must exist in Replay & Incidents");

// Test debugger stepping logic
const controls = debuggerPanel.children[1];
assert.ok(controls !== undefined, "Debugger controls must exist");
const prevBtn = controls.children[0];
const nextBtn = controls.children[1];
const statusText = controls.children[2];

assert.equal(prevBtn.disabled, true, "Initial step cannot go backwards");
assert.equal(nextBtn.disabled, false, "Initial step can go forward");
assert.ok(statusText.textContent.includes("Event 1 of 4"), "Initial status must point to event 1");

// Step forward
nextBtn.fire("click");
assert.equal(prevBtn.disabled, false, "Can now step backward after stepping forward");
assert.ok(statusText.textContent.includes("Event 2 of 4"), "Status must advance to event 2");

// Step back
prevBtn.fire("click");
assert.equal(prevBtn.disabled, true);
assert.ok(statusText.textContent.includes("Event 1 of 4"));

// --- 3. Schema File Existence and Syntax Assertions ---

const schemas = [
  "research-hypothesis.schema.json",
  "experiment-lineage.schema.json",
  "research-job.schema.json",
  "assistant-evidence.schema.json",
];

for (const schemaName of schemas) {
  const schemaPath = resolve(repoRoot, "contracts", "json-schema", "v1", schemaName);
  const raw = await readFile(schemaPath, "utf8");
  const parsed = JSON.parse(raw);
  assert.ok(parsed.title.length > 0, `${schemaName} must have a title`);
  assert.ok(Array.isArray(parsed.required), `${schemaName} must specify required fields`);
  assert.equal(parsed.additionalProperties, false, `${schemaName} must disallow additional properties`);
}

console.log("Research contracts, schemas, workspace cockpits, and event debugger regression tests passed cleanly");
