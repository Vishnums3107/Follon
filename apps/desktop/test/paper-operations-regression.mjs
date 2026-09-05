import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseOrderDecisionPassport,
  parseExposureGraph,
  parseFundLedgerStatement,
  parseContinuityPolicy,
} from "../dist/evidence.js";
import { renderWorkspace } from "../dist/workspaces.js";

const testDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(testDir, "..", "..", "..");

// --- 1. Contract & Parser Tests ---

// A. OrderDecisionPassport (EXEC-02)
const validPassport = {
  passport_schema_version: 1,
  passport_id: "passport.ord.9b41",
  intent_id: "intent.001",
  order_id: "ord.9b41",
  instrument_id: "inst.us_equity.spy",
  signal_attribution: {
    strategy_version: "strat.trend.v1",
    model_event_id: "evt.sig.001",
    opportunity_description: "Volume-weighted 20-day breakout continuation",
    signal_power_bps: 8500,
  },
  risk_evaluation: {
    policy_version: "pol.risk.v1",
    approved: true,
    evaluated_limits: ["gross_exposure", "daily_loss_limit"],
    headroom_remaining_bps: 4500,
  },
  routing_plan: {
    algorithm: "twap-v1",
    allocated_slices_count: 3,
    primary_venue: "venue.nasdaq",
    capability_version: "cap.nasdaq.v1",
  },
  executions: [
    {
      execution_id: "exec.001",
      venue: "venue.nasdaq",
      quantity: "100",
      price: "512.40",
      fee: "0.15",
      executed_at: "2026-09-04T14:30:00Z",
    },
  ],
  accounting_consequences: {
    journal_entry_id: "jrn.7f1a",
    realized_pnl: "0.00",
    cash_delta: "-51240.15",
    position_after: "100",
  },
  created_at: "2026-09-04T14:30:05Z",
};

const parsedPassport = parseOrderDecisionPassport(JSON.stringify(validPassport));
assert.equal(parsedPassport.passport_id, "passport.ord.9b41");
assert.equal(parsedPassport.routing_plan.algorithm, "twap-v1");
assert.equal(parsedPassport.executions[0].quantity, "100");
assert.equal(parsedPassport.accounting_consequences.cash_delta, "-51240.15");

assert.throws(
  () => parseOrderDecisionPassport(JSON.stringify({ ...validPassport, executions: [] })),
  /does not match the v1 evidence contract/,
  "Invalid passport without required executions structure must fail if malformed"
);

// B. ExposureGraph (RISK-01)
const validExposure = {
  exposure_schema_version: 1,
  graph_id: "exp-graph.paper.001",
  account_id: "acct.paper.001",
  as_of_time: "2026-09-04T15:00:00Z",
  gross_exposure: "100000.00",
  net_exposure: "25000.00",
  factors: [
    {
      factor_name: "Momentum (12-1M)",
      loading_bps: 4200,
      factor_variance_pct: "34.5%",
    },
    {
      factor_name: "Market Beta",
      loading_bps: 9800,
      factor_variance_pct: "52.0%",
    },
  ],
  sectors: [
    {
      sector_name: "Technology",
      exposure_usd: "45000.00",
      weight_bps: 4500,
    },
  ],
  top_concentrations: [
    {
      instrument_id: "inst.us_equity.spy",
      position_value: "51240.00",
      portfolio_pct: "51.2%",
    },
  ],
  unreconciled_discrepancy: false,
  created_at: "2026-09-04T15:00:05Z",
};

const parsedExp = parseExposureGraph(JSON.stringify(validExposure));
assert.equal(parsedExp.graph_id, "exp-graph.paper.001");
assert.equal(parsedExp.factors.length, 2);
assert.equal(parsedExp.unreconciled_discrepancy, false);

// C. FundLedgerStatement (PORT-01)
const validLedger = {
  ledger_schema_version: 1,
  statement_id: "stmt.reconciliation.001",
  account_id: "acct.paper.001",
  period_start: "2026-09-01T00:00:00Z",
  period_end: "2026-09-04T23:59:59Z",
  starting_cash: "50000.00",
  ending_cash: "52480.00",
  realized_pnl: "2480.00",
  unrealized_pnl: "1030.00",
  fee_totals: {
    exchange_fees: "14.20",
    brokerage_commissions: "0.00",
    borrow_financing: "0.00",
  },
  tax_lots: [
    {
      lot_id: "lot.spy.001",
      instrument_id: "inst.us_equity.spy",
      acquired_at: "2026-09-02T14:35:00Z",
      quantity: "100",
      cost_basis: "502.10",
      disposition: "OPEN",
    },
  ],
  balanced: true,
  created_at: "2026-09-05T00:00:00Z",
};

const parsedLedger = parseFundLedgerStatement(JSON.stringify(validLedger));
assert.equal(parsedLedger.statement_id, "stmt.reconciliation.001");
assert.equal(parsedLedger.balanced, true);
assert.equal(parsedLedger.tax_lots[0].disposition, "OPEN");

// D. ContinuityPolicy (SOLO-06, LIFE-04/05/06)
const validPolicy = {
  policy_schema_version: 1,
  policy_id: "cont-pol.standard.v1",
  unattended_interval_minutes: 30,
  heartbeat_interval_seconds: 5,
  max_restarts_per_hour: 3,
  away_mode_permitted: true,
  broker_disconnect_action: "RETAIN_UNKNOWN_AND_ESCALATE",
  feed_stale_threshold_seconds: 3,
  created_at: "2026-09-01T00:00:00Z",
};

const parsedPol = parseContinuityPolicy(JSON.stringify(validPolicy));
assert.equal(parsedPol.policy_id, "cont-pol.standard.v1");
assert.equal(parsedPol.unattended_interval_minutes, 30);
assert.equal(parsedPol.broker_disconnect_action, "RETAIN_UNKNOWN_AND_ESCALATE");

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

// Test Command Center additions (SOLO-04, SOLO-05, SOLO-06)
const ccSummary = new MockElement("div");
const ccCanvas = new MockElement("div");
renderWorkspace(ccSummary, ccCanvas, "command-center", mockSnapshot, mockContext);

const scannerPanel = ccCanvas.children.find((c) => c.id === "market-scanner-panel");
assert.ok(scannerPanel !== undefined, "Market scanner panel must exist in Command Center");

const attentionPanel = ccCanvas.children.find((c) => c.id === "consolidated-attention-panel");
assert.ok(attentionPanel !== undefined, "Consolidated attention panel must exist in Command Center");

const playbooksPanel = ccCanvas.children.find((c) => c.id === "session-playbooks-panel");
assert.ok(playbooksPanel !== undefined, "Session playbooks panel must exist in Command Center");

// Test Execution Blotter additions (EXEC-02)
const execSummary = new MockElement("div");
const execCanvas = new MockElement("div");
renderWorkspace(execSummary, execCanvas, "execution-blotter", mockSnapshot, mockContext);

const passportPanel = execCanvas.children.find((c) => c.id === "decision-passport-panel");
assert.ok(passportPanel !== undefined, "Decision passport panel must exist in Execution Blotter");

// Test Risk Cockpit additions (RISK-01)
const riskSummary = new MockElement("div");
const riskCanvas = new MockElement("div");
renderWorkspace(riskSummary, riskCanvas, "risk-cockpit", mockSnapshot, mockContext);

const exposureGraphPanel = riskCanvas.children.find((c) => c.id === "exposure-graph-panel");
assert.ok(exposureGraphPanel !== undefined, "Exposure graph panel must exist in Risk Cockpit");

// Test Portfolio additions (PORT-01)
const portSummary = new MockElement("div");
const portCanvas = new MockElement("div");
renderWorkspace(portSummary, portCanvas, "portfolio", mockSnapshot, mockContext);

const fundLedgerPanel = portCanvas.children.find((c) => c.id === "fund-ledger-panel");
assert.ok(fundLedgerPanel !== undefined, "Fund ledger panel must exist in Portfolio");

// Test Administration additions (LIFE-04/05/06/09/10/11)
const adminSummary = new MockElement("div");
const adminCanvas = new MockElement("div");
renderWorkspace(adminSummary, adminCanvas, "administration", mockSnapshot, mockContext);

const watchdogPanel = adminCanvas.children.find((c) => c.id === "watchdog-recovery-panel");
assert.ok(watchdogPanel !== undefined, "Watchdog recovery panel must exist in Administration");

// --- 3. Schema File Existence and Syntax Assertions ---

const schemas = [
  "order-decision-passport.schema.json",
  "exposure-graph.schema.json",
  "fund-ledger-statement.schema.json",
  "continuity-policy.schema.json",
];

for (const schemaName of schemas) {
  const schemaPath = resolve(repoRoot, "contracts", "json-schema", "v1", schemaName);
  const raw = await readFile(schemaPath, "utf8");
  const parsed = JSON.parse(raw);
  assert.ok(parsed.title.length > 0, `${schemaName} must have a title`);
  assert.ok(Array.isArray(parsed.required), `${schemaName} must specify required fields`);
  assert.equal(parsed.additionalProperties, false, `${schemaName} must disallow additional properties`);
}

console.log("PAPER operations, market scanner, decision passport, exposure graph, fund ledger, and watchdog recovery regression tests passed cleanly");
