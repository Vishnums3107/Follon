import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseAssumptionRegimeMonitor,
  parseFeedSubstitutionParity,
  parseExecutionCoachBenchmark,
  parseScenarioLossSimulation,
  parseCapitalAllocationPlan,
  parseSandboxInstallationPreview,
  parseAdapterQualification,
} from "../dist/evidence.js";
import { renderWorkspace } from "../dist/workspaces.js";

const testDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(testDir, "..", "..", "..");

// --- 1. Contract & Parser Tests ---

// A. AssumptionRegimeMonitor (DATA-05)
const validRegime = {
  regime_schema_version: 1,
  regime_id: "regime.spread-regime.001",
  as_of_time: "2026-09-04T12:00:00Z",
  lookback_bars: 200,
  current_regime: "LOW_VOL_TRENDING",
  indicators: {
    realized_vol_annual_bps: 1450,
    effective_spread_bps: 18,
    trend_strength_bps: 6500,
    cross_asset_correlation_bps: 4200,
  },
  impacted_strategy_assumptions: [
    {
      strategy_id: "strat.trend.v1",
      assumed_condition: "Effective spread < 25 bps",
      observed_condition: "Observed spread 18 bps",
      breach_status: "COMPATIBLE",
    },
  ],
  model_version: "regime-hdbscan-v1",
  created_at: "2026-09-04T12:05:00Z",
};

const parsedRegime = parseAssumptionRegimeMonitor(JSON.stringify(validRegime));
assert.equal(parsedRegime.regime_id, "regime.spread-regime.001");
assert.equal(parsedRegime.current_regime, "LOW_VOL_TRENDING");
assert.equal(parsedRegime.impacted_strategy_assumptions.length, 1);

assert.throws(
  () => parseAssumptionRegimeMonitor(JSON.stringify({ ...validRegime, current_regime: "INVALID_REGIME" })),
  /does not match the v1 evidence contract/,
  "Invalid regime must be rejected"
);

// B. FeedSubstitutionParity (DATA-06)
const validFeedParity = {
  parity_schema_version: 1,
  comparison_id: "feed-parity.arca-bats-001",
  primary_provider: "feed.sip.nyse-arca.v1",
  candidate_provider: "feed.direct.bats.v1",
  sample_start: "2026-01-01T00:00:00Z",
  sample_end: "2026-06-30T23:59:59Z",
  symbol_match_pct: "99.98",
  timestamp_variance_micros_p99: 15000,
  adjustment_parity_verified: true,
  parity_disposition: "QUALIFIED_FOR_SUBSTITUTION",
  created_at: "2026-09-04T14:00:00Z",
};

const parsedFeedParity = parseFeedSubstitutionParity(JSON.stringify(validFeedParity));
assert.equal(parsedFeedParity.comparison_id, "feed-parity.arca-bats-001");
assert.equal(parsedFeedParity.parity_disposition, "QUALIFIED_FOR_SUBSTITUTION");
assert.equal(parsedFeedParity.timestamp_variance_micros_p99, 15000);

assert.throws(
  () => parseFeedSubstitutionParity(JSON.stringify({ ...validFeedParity, parity_disposition: "UNKNOWN_DISP" })),
  /does not match the v1 evidence contract/,
  "Invalid feed parity disposition must be rejected"
);

// C. ExecutionCoachBenchmark (EXEC-03, RES-07)
const validCoach = {
  coach_schema_version: 1,
  analysis_id: "coach.analysis.001",
  order_id: "ord.9b41",
  instrument_id: "inst.us-equity.spy",
  arrival_price: "512.20",
  target_price: "512.25",
  realized_vwap: "512.40",
  pre_trade_estimated_cost_bps: 8,
  realized_shortfall_bps: 12,
  slippage_drag_bps: 4,
  market_impact_bps: 5,
  fee_drag_bps: 3,
  execution_grade: "OPTIMAL",
  created_at: "2026-09-04T16:00:00Z",
};

const parsedCoach = parseExecutionCoachBenchmark(JSON.stringify(validCoach));
assert.equal(parsedCoach.analysis_id, "coach.analysis.001");
assert.equal(parsedCoach.execution_grade, "OPTIMAL");
assert.equal(parsedCoach.realized_shortfall_bps, 12);

assert.throws(
  () => parseExecutionCoachBenchmark(JSON.stringify({ ...validCoach, execution_grade: "INVALID_GRADE" })),
  /does not match the v1 evidence contract/,
  "Invalid coach execution grade must be rejected"
);

// D. ScenarioLossSimulation (RISK-02)
const validScenario = {
  simulation_schema_version: 1,
  simulation_id: "loss-sim.stress-2008-crash",
  account_id: "acct.paper.01",
  scenario_name: "2008 Financial Crisis Replay",
  shock_assumptions: {
    equity_shock_pct: "-40.0",
    volatility_multiplier: "2.8",
    spread_expansion_multiplier: "4.0",
    financing_rate_shock_bps: 150,
  },
  estimated_loss_usd: "17550.00",
  estimated_loss_bps: 1755,
  liquidity_haircut_usd: "2400.00",
  stressed_margin_utilization_pct: "68.5",
  capital_adequate: true,
  created_at: "2026-09-04T18:00:00Z",
};

const parsedScenario = parseScenarioLossSimulation(JSON.stringify(validScenario));
assert.equal(parsedScenario.simulation_id, "loss-sim.stress-2008-crash");
assert.equal(parsedScenario.capital_adequate, true);
assert.equal(parsedScenario.estimated_loss_bps, 1755);

assert.throws(
  () => parseScenarioLossSimulation(JSON.stringify({ ...validScenario, capital_adequate: "not_a_boolean" })),
  /does not match the v1 evidence contract/,
  "Invalid capital adequate type must be rejected"
);

// E. CapitalAllocationPlan (RISK-03)
const validAllocation = {
  allocation_schema_version: 1,
  plan_id: "alloc-plan.q3-001",
  total_capital_usd: "100000.00",
  cash_reserve_bps: 1000,
  allocations: [
    {
      strategy_id: "strat.trend.v1",
      allocated_capital_usd: "60000.00",
      target_weight_bps: 6000,
      expected_sharpe: "1.45",
    },
    {
      strategy_id: "strat.meanrev.v1",
      allocated_capital_usd: "30000.00",
      target_weight_bps: 3000,
      expected_sharpe: "1.20",
    },
  ],
  risk_policy_version: "risk-policy-2026-v1",
  approved_by_policy: true,
  created_at: "2026-09-04T19:00:00Z",
};

const parsedAllocation = parseCapitalAllocationPlan(JSON.stringify(validAllocation));
assert.equal(parsedAllocation.plan_id, "alloc-plan.q3-001");
assert.equal(parsedAllocation.allocations.length, 2);
assert.equal(parsedAllocation.allocations[0].target_weight_bps, 6000);

assert.throws(
  () => parseCapitalAllocationPlan(JSON.stringify({ ...validAllocation, allocations: [] })),
  /does not match the v1 evidence contract/,
  "Empty allocations list must be rejected"
);

// F. SandboxInstallationPreview (ASSET-03, ASSET-04)
const validSandbox = {
  preview_schema_version: 1,
  preview_id: "preview.pkg-trend-001",
  asset_id: "pkg.strat.trend-breakout.v1",
  asset_version: "1.0.0",
  manifest_hash: "4a5b6c7d8e9f0123456789abcdef0123456789abcdef0123456789abcdef0123",
  declared_permissions: ["READ_MARKET_DATA", "EMIT_ORDER_INTENT"],
  resource_caps: {
    max_memory_mb: 4096,
    max_cpu_percent: 50,
    filesystem_isolated: true,
  },
  untrusted_capabilities_detected: 0,
  rollback_snapshot_id: "snap.preinstall.001",
  disposition: "QUALIFIED_FOR_ISOLATED_INSTALL",
  created_at: "2026-09-04T20:00:00Z",
};

const parsedSandbox = parseSandboxInstallationPreview(JSON.stringify(validSandbox));
assert.equal(parsedSandbox.preview_id, "preview.pkg-trend-001");
assert.equal(parsedSandbox.disposition, "QUALIFIED_FOR_ISOLATED_INSTALL");

assert.throws(
  () => parseSandboxInstallationPreview(JSON.stringify({ ...validSandbox, disposition: "NO_ISOLATION" })),
  /does not match the v1 evidence contract/,
  "Disallowed disposition must be rejected"
);

// G. AdapterQualification (LIFE-07, PORT-02)
const validAdapter = {
  qualification_schema_version: 1,
  qualification_id: "qual.ibkr.rest-ws.001",
  venue: "venue.interactive-brokers",
  asset_class: "US_EQUITY",
  adapter_version: "ibkr-adapter-v2.1",
  supported_capabilities: ["LIMIT_ORDER", "MARKET_ORDER", "CANCEL_REPLACE", "ORDER_STATUS_POLL"],
  single_writer_fenced: true,
  reconciliation_pass_rate_pct: "99.99",
  operational_gate_status: "QUALIFIED",
  created_at: "2026-09-04T21:00:00Z",
  expires_at: "2027-09-04T21:00:00Z",
};

const parsedAdapter = parseAdapterQualification(JSON.stringify(validAdapter));
assert.equal(parsedAdapter.qualification_id, "qual.ibkr.rest-ws.001");
assert.equal(parsedAdapter.operational_gate_status, "QUALIFIED");
assert.equal(parsedAdapter.single_writer_fenced, true);

assert.throws(
  () => parseAdapterQualification(JSON.stringify({ ...validAdapter, operational_gate_status: "NOT_QUALIFIED" })),
  /does not match the v1 evidence contract/,
  "Invalid adapter operational gate status must be rejected"
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

// Test Research Lab additions (DATA-06)
const researchSummary = new MockElement("div");
const researchCanvas = new MockElement("div");
renderWorkspace(researchSummary, researchCanvas, "research-lab", mockSnapshot, mockContext);

const feedSubPanel = researchCanvas.children.find((c) => c.id === "feed-substitution-panel");
assert.ok(feedSubPanel !== undefined, "Feed substitution panel must exist in Research Lab");

// Test News Cockpit additions (DATA-05)
const newsSummary = new MockElement("div");
const newsCanvas = new MockElement("div");
renderWorkspace(newsSummary, newsCanvas, "news-cockpit", mockSnapshot, mockContext);

const regimePanel = newsCanvas.children.find((c) => c.id === "regime-monitor-panel");
assert.ok(regimePanel !== undefined, "Regime monitor panel must exist in News Cockpit");

// Test Execution Blotter additions (EXEC-03, RES-07)
const execSummary = new MockElement("div");
const execCanvas = new MockElement("div");
renderWorkspace(execSummary, execCanvas, "execution-blotter", mockSnapshot, mockContext);

const coachPanel = execCanvas.children.find((c) => c.id === "execution-coach-panel");
assert.ok(coachPanel !== undefined, "Execution coach benchmark panel must exist in Execution Blotter");

// Test Risk Cockpit additions (RISK-02, RISK-03)
const riskSummary = new MockElement("div");
const riskCanvas = new MockElement("div");
renderWorkspace(riskSummary, riskCanvas, "risk-cockpit", mockSnapshot, mockContext);

const scenarioLossPanel = riskCanvas.children.find((c) => c.id === "scenario-loss-panel");
assert.ok(scenarioLossPanel !== undefined, "Scenario loss panel must exist in Risk Cockpit");

const capitalAllocPanel = riskCanvas.children.find((c) => c.id === "capital-allocation-panel");
assert.ok(capitalAllocPanel !== undefined, "Capital allocation panel must exist in Risk Cockpit");

// Test Marketplace additions (ASSET-03, ASSET-04)
const marketSummary = new MockElement("div");
const marketCanvas = new MockElement("div");
renderWorkspace(marketSummary, marketCanvas, "marketplace", mockSnapshot, mockContext);

const sandboxPrevPanel = marketCanvas.children.find((c) => c.id === "sandbox-preview-panel");
assert.ok(sandboxPrevPanel !== undefined, "Sandbox preview panel must exist in Marketplace");

// Test Administration additions (LIFE-07, PORT-02)
const adminSummary = new MockElement("div");
const adminCanvas = new MockElement("div");
renderWorkspace(adminSummary, adminCanvas, "administration", mockSnapshot, mockContext);

const adapterQualPanel = adminCanvas.children.find((c) => c.id === "adapter-qualification-panel");
assert.ok(adapterQualPanel !== undefined, "Adapter qualification panel must exist in Administration");

// --- 3. Schema File Existence and Syntax Assertions ---

const schemas = [
  "assumption-regime-monitor.schema.json",
  "feed-substitution-parity.schema.json",
  "execution-coach-benchmark.schema.json",
  "scenario-loss-simulation.schema.json",
  "capital-allocation-plan.schema.json",
  "sandbox-installation-preview.schema.json",
  "adapter-qualification.schema.json",
];

for (const schemaName of schemas) {
  const schemaPath = resolve(repoRoot, "contracts", "json-schema", "v1", schemaName);
  const raw = await readFile(schemaPath, "utf8");
  const parsed = JSON.parse(raw);
  assert.ok(parsed.title.length > 0, `${schemaName} must have a title`);
  assert.ok(Array.isArray(parsed.required), `${schemaName} must specify required fields`);
  assert.equal(parsed.additionalProperties, false, `${schemaName} must disallow additional properties`);
}

console.log("Measured depth & multi-asset expansion regression tests (Increments 5 & 6) passed cleanly");
