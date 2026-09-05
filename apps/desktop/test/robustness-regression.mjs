import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseRobustnessEvaluation,
  parsePortfolioExperiment,
  parseKnowledgeSnapshot,
  parseEventExposureCalendar,
  parseAutomationMandate,
} from "../dist/evidence.js";
import { renderWorkspace } from "../dist/workspaces.js";

const testDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(testDir, "..", "..", "..");

// --- 1. Contract & Parser Tests ---

// A. RobustnessEvaluation (RES-05)
const validRobustness = {
  evaluation_schema_version: 1,
  evaluation_id: "eval.trend-walkforward-001",
  strategy_version: "strat.trend.v1",
  hypothesis_id: "hyp.momentum-cross-v1",
  walk_forward_windows: [
    {
      window_id: "win.001",
      in_sample_start: "2025-01-01T00:00:00Z",
      in_sample_end: "2025-06-30T23:59:59Z",
      out_of_sample_start: "2025-07-01T00:00:00Z",
      out_of_sample_end: "2025-09-30T23:59:59Z",
      in_sample_return_bps: 1240,
      out_of_sample_return_bps: 480,
      max_drawdown_bps: 540,
    },
  ],
  leakage_checks: {
    survivorship_bias_verified: true,
    lookahead_bias_verified: true,
    corporate_action_adjusted: true,
    quarantine_violations: 0,
  },
  parameter_stability: {
    perturbation_percent: 15,
    neighborhood_variance_bps: 120,
    degradation_cliff_detected: false,
  },
  cost_shocks: [
    {
      slippage_multiplier: "2.0",
      fee_multiplier: "2.0",
      stressed_return_bps: 920,
    },
  ],
  uncertainty_score_bps: 180,
  disposition: "ROBUST",
  created_at: "2026-09-04T12:00:00Z",
};

const parsedRobust = parseRobustnessEvaluation(JSON.stringify(validRobustness));
assert.equal(parsedRobust.evaluation_id, "eval.trend-walkforward-001");
assert.equal(parsedRobust.disposition, "ROBUST");
assert.equal(parsedRobust.walk_forward_windows.length, 1);

assert.throws(
  () => parseRobustnessEvaluation(JSON.stringify({ ...validRobustness, disposition: "INVALID_DISP" })),
  /does not match the v1 evidence contract/,
  "Invalid disposition must be rejected"
);

assert.throws(
  () => parseRobustnessEvaluation(JSON.stringify({ ...validRobustness, walk_forward_windows: [] })),
  /does not match the v1 evidence contract/,
  "Empty walk forward windows must be rejected"
);

// B. PortfolioExperiment (RES-06)
const validPortfolio = {
  portfolio_experiment_schema_version: 1,
  experiment_id: "port-exp.dual-alpha-001",
  allocated_cash: "100000.00",
  currency: "USD",
  strategies: [
    {
      strategy_id: "strat.trend.v1",
      strategy_version: "2026-01-01.1",
      target_weight_bps: 6000,
      realized_pnl: "12400.00",
      max_drawdown_bps: 510,
    },
    {
      strategy_id: "strat.meanrev.v1",
      strategy_version: "2026-01-01.1",
      target_weight_bps: 4000,
      realized_pnl: "9000.00",
      max_drawdown_bps: 420,
    },
  ],
  joint_constraints: {
    max_gross_exposure_bps: 10000,
    max_single_instrument_bps: 2500,
    turnover_cap_daily_bps: 2000,
  },
  joint_performance: {
    combined_return_bps: 2140,
    combined_max_drawdown_bps: 510,
    diversification_ratio_bps: 1420,
    total_fee_drag: "350.50",
  },
  order_contention_events: 2,
  created_at: "2026-09-04T14:00:00Z",
};

const parsedPort = parsePortfolioExperiment(JSON.stringify(validPortfolio));
assert.equal(parsedPort.experiment_id, "port-exp.dual-alpha-001");
assert.equal(parsedPort.strategies.length, 2);
assert.equal(parsedPort.order_contention_events, 2);

assert.throws(
  () => parsePortfolioExperiment(JSON.stringify({ ...validPortfolio, strategies: [validPortfolio.strategies[0]] })),
  /does not match the v1 evidence contract/,
  "Fewer than 2 strategies must be rejected"
);

// C. KnowledgeSnapshot (DATA-02)
const validKnowledge = {
  knowledge_schema_version: 1,
  snapshot_id: "know.sp500.pit.001",
  as_of_time: "2026-01-16T15:00:00Z",
  entity_nodes: [
    {
      entity_id: "comp.sp500.aapl",
      entity_type: "COMPANY",
      name: "Apple Inc.",
      identifier: "AAPL",
    },
    {
      entity_id: "inst.us_equity.aapl",
      entity_type: "INSTRUMENT",
      name: "Apple Inc. Common Stock",
      identifier: "US0378331005",
    },
  ],
  relationships: [
    {
      source_entity_id: "comp.sp500.aapl",
      relation_type: "ISSUES_INSTRUMENT",
      target_entity_id: "inst.us_equity.aapl",
      effective_time: "2026-01-01T00:00:00Z",
      provenance_hash: "a".repeat(64),
    },
  ],
  source_lineage_hashes: ["b".repeat(64)],
  created_at: "2026-01-16T15:05:00Z",
};

const parsedKnowledge = parseKnowledgeSnapshot(JSON.stringify(validKnowledge));
assert.equal(parsedKnowledge.snapshot_id, "know.sp500.pit.001");
assert.equal(parsedKnowledge.entity_nodes[0].entity_type, "COMPANY");

// D. EventExposureCalendar (DATA-04)
const validCalendar = {
  calendar_schema_version: 1,
  calendar_id: "cal.equities.2026q1",
  as_of_time: "2026-01-01T00:00:00Z",
  timezone: "America/New_York",
  scheduled_events: [
    {
      event_id: "ev.earn.aapl.2026q1",
      instrument_id: "inst.us_equity.aapl",
      category: "EARNINGS",
      scheduled_time: "2026-01-22T21:30:00Z",
      status: "SCHEDULED",
      source_evidence: "cal.ir.aapl.2026q1",
    },
  ],
  quarantined_events_count: 0,
  created_at: "2026-01-01T00:00:00Z",
};

const parsedCal = parseEventExposureCalendar(JSON.stringify(validCalendar));
assert.equal(parsedCal.calendar_id, "cal.equities.2026q1");
assert.equal(parsedCal.scheduled_events[0].category, "EARNINGS");

// E. AutomationMandate (AI-04)
const validMandate = {
  mandate_schema_version: 1,
  mandate_id: "mandate.nightly.001",
  owner: "operator.solo",
  allowed_tasks: ["walk-forward-sweep", "cost-sensitivity-shock"],
  resource_limits: {
    max_cpu_cores: 4,
    max_memory_mb: 8192,
    max_duration_seconds: 14400,
    max_storage_bytes: 104857600,
  },
  cancellation_policy: {
    stop_on_first_error: true,
    checkpoint_interval_seconds: 300,
  },
  broker_access_permitted: false,
  created_at: "2026-09-04T20:00:00Z",
  expires_at: "2026-09-05T08:00:00Z",
};

const parsedMandate = parseAutomationMandate(JSON.stringify(validMandate));
assert.equal(parsedMandate.mandate_id, "mandate.nightly.001");
assert.equal(parsedMandate.broker_access_permitted, false);

assert.throws(
  () => parseAutomationMandate(JSON.stringify({ ...validMandate, broker_access_permitted: true })),
  /does not match the v1 evidence contract/,
  "Broker access must be rejected in research automation mandate"
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

const mockContext = {
  status: null,
  features: [],
  artifacts: [],
  workspaceFeatures: [],
  onOpenArtifact: () => {},
};

// Test Backtest Explorer additions (RES-05, RES-06)
const backtestSummary = new MockElement("div");
const backtestCanvas = new MockElement("div");
renderWorkspace(backtestSummary, backtestCanvas, "backtest-explorer", mockSnapshot, mockContext);

const robPanel = backtestCanvas.children.find((c) => c.id === "robustness-lab-panel");
assert.ok(robPanel !== undefined, "Robustness laboratory panel must exist in Backtest Explorer");

const portPanel = backtestCanvas.children.find((c) => c.id === "portfolio-experiment-panel");
assert.ok(portPanel !== undefined, "Portfolio experiment engine panel must exist in Backtest Explorer");

// Test News Cockpit additions (DATA-02, DATA-03, DATA-04)
const newsSummary = new MockElement("div");
const newsCanvas = new MockElement("div");
renderWorkspace(newsSummary, newsCanvas, "news-cockpit", mockSnapshot, mockContext);

const knowPanel = newsCanvas.children.find((c) => c.id === "knowledge-graph-panel");
assert.ok(knowPanel !== undefined, "Knowledge graph panel must exist in News Cockpit");

const revPanel = newsCanvas.children.find((c) => c.id === "news-revision-panel");
assert.ok(revPanel !== undefined, "News revision panel must exist in News Cockpit");

const calPanel = newsCanvas.children.find((c) => c.id === "event-exposure-calendar");
assert.ok(calPanel !== undefined, "Event exposure calendar panel must exist in News Cockpit");

// Test Strategy Studio additions (AI-02, AI-03, AI-04)
const stratSummary = new MockElement("div");
const stratCanvas = new MockElement("div");
renderWorkspace(stratSummary, stratCanvas, "strategy-studio", mockSnapshot, mockContext);

const criticPanel = stratCanvas.children.find((c) => c.id === "strategy-critic-panel");
assert.ok(criticPanel !== undefined, "Strategy critic panel must exist in Strategy Studio");

const schedPanel = stratCanvas.children.find((c) => c.id === "research-scheduler-panel");
assert.ok(schedPanel !== undefined, "Research scheduler panel must exist in Strategy Studio");

// Test Marketplace additions (ASSET-02)
const marketSummary = new MockElement("div");
const marketCanvas = new MockElement("div");
renderWorkspace(marketSummary, marketCanvas, "marketplace", mockSnapshot, mockContext);

const assetCompPanel = marketCanvas.children.find((c) => c.id === "asset-comparison-panel");
assert.ok(assetCompPanel !== undefined, "Asset comparison panel must exist in Marketplace");

// --- 3. Schema File Existence and Syntax Assertions ---

const schemas = [
  "robustness-evaluation.schema.json",
  "portfolio-experiment.schema.json",
  "knowledge-snapshot.schema.json",
  "event-exposure-calendar.schema.json",
  "automation-mandate.schema.json",
];

for (const schemaName of schemas) {
  const schemaPath = resolve(repoRoot, "contracts", "json-schema", "v1", schemaName);
  const raw = await readFile(schemaPath, "utf8");
  const parsed = JSON.parse(raw);
  assert.ok(parsed.title.length > 0, `${schemaName} must have a title`);
  assert.ok(Array.isArray(parsed.required), `${schemaName} must specify required fields`);
  assert.equal(parsed.additionalProperties, false, `${schemaName} must disallow additional properties`);
}

console.log("Robustness laboratory, portfolio experiment, point-in-time knowledge graph, and scheduler regression tests passed cleanly");
