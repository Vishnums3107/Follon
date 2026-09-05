import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  WORKSPACES,
  WORKSPACE_GROUPS,
  isWorkspaceId,
  getWorkspaceDefinition,
  decodeWorkspaceRoute,
  workspaceHash,
  workspacePath,
} from "../dist/routes.js";
import { renderWorkspace } from "../dist/workspaces.js";
import { CommandPalette } from "../dist/command-palette.js";

const testDir = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(testDir, "..");
const repoRoot = resolve(appDir, "..", "..");

// 1. Typed Route Registry verification
assert.equal(WORKSPACES.length, 12, "exactly 12 workspaces defined in typed registry");
assert.equal(WORKSPACE_GROUPS.length, 4, "four navigation groups (Monitor, Research, Operate, Govern)");
const groupLabels = WORKSPACE_GROUPS.map((g) => g.label);
assert.deepEqual(groupLabels, ["Monitor", "Research", "Operate", "Govern"]);

const expectedWorkspaces = [
  "command-center", "marketplace", "research-lab", "news-cockpit",
  "strategy-studio", "backtest-explorer", "execution-blotter", "risk-cockpit",
  "portfolio", "replay-incidents", "journal", "administration",
];

for (const id of expectedWorkspaces) {
  assert.ok(isWorkspaceId(id), `expected valid workspace ID: ${id}`);
  const def = getWorkspaceDefinition(id);
  assert.equal(def.id, id);
  assert.ok(def.title.length > 0);
  assert.ok(def.description.length > 0);
  assert.ok(def.features.length > 0);
}

assert.equal(isWorkspaceId("unknown-workspace"), false);
assert.equal(isWorkspaceId(""), false);
assert.equal(isWorkspaceId(null), false);

// Route encoding & decoding
assert.equal(workspaceHash("strategy-studio"), "#workspace/strategy-studio");
assert.equal(workspacePath("strategy-studio"), "/workspace/strategy-studio");
assert.equal(decodeWorkspaceRoute("#workspace/marketplace", "/"), "marketplace");
assert.equal(decodeWorkspaceRoute("", "/workspace/risk-cockpit"), "risk-cockpit");
assert.equal(decodeWorkspaceRoute("#invalid", "/"), "command-center");

// 2. Command Palette DOM and action verification
class MockElement {
  constructor(tag) {
    this.tag = tag;
    this.children = [];
    this.textContent = "";
    this.value = "";
    this.hidden = false;
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
globalThis.window = {
  addEventListener: () => {},
};

let navigatedWorkspace = null;
let loadedArtifact = null;
const palette = new CommandPalette({
  onOpenWorkspace: (id) => { navigatedWorkspace = id; },
  onOpenArtifact: (name) => { loadedArtifact = name; },
  onRefreshHealth: () => {},
  onRefreshEvidence: () => {},
  getArtifacts: () => [
    { name: "test-run.json", feature: "research", kind: "backtest", bytes: 2048, modified_at: "", format: "json" },
  ],
});

palette.open();
assert.equal(navigatedWorkspace, null);

// 3. Daily Operating Brief Invariants in Command Center
const mockSummary = new MockElement("div");
const mockCanvas = new MockElement("div");
const emptySnapshot = {
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

// Case A: Missing runtime status must prevent ALL CLEAR
renderWorkspace(mockSummary, mockCanvas, "command-center", emptySnapshot, {
  status: null,
  features: [],
  artifacts: [],
  workspaceFeatures: [],
  onOpenArtifact: () => {},
});

const briefPanel = mockCanvas.children.find((c) => c.id === "daily-brief");
assert.ok(briefPanel !== undefined, "Daily Operating Brief panel must exist");
const headerDiv = briefPanel.children[1];
const badge = headerDiv.children[0];
assert.ok(badge.textContent.includes("ATTENTION REQUIRED"), "Missing status must prevent all-clear summary");
assert.ok(badge.className.includes("f-badge--warn"));

// Case B: Healthy status and no unknown orders
const healthyContext = {
  status: {
    status: "healthy",
    services: {
      api: { status: "healthy", detail: "ok" },
      grpc: { status: "healthy", detail: "ok" },
    },
    capabilities: {},
  },
  features: [],
  artifacts: [],
  workspaceFeatures: [],
  onOpenArtifact: () => {},
};

const healthySnapshot = {
  ...emptySnapshot,
  paper: {
    artifact: "paper-dashboard.json",
    modified_at: "",
    data: {
      dashboard_schema_version: 2,
      environment: "PAPER",
      account_id: "acct.paper",
      configuration_fingerprint: "a".repeat(64),
      broker_connected: true,
      persistence_healthy: true,
      audit_sequence: 1,
      audit_head_hash: "a".repeat(64),
      internal_cash: "100000.00",
      working_orders: 0,
      unknown_orders: 0,
      active_kill_switches: [],
      unexplained_incidents: 0,
      last_reconciled_at: "2026-09-05T00:00:00Z",
      last_reconciliation_clean: true,
      clean_paper_days: 10,
      required_paper_days: 30,
      promotion_eligible: false,
      complete_auditability: true,
      positions: [],
    },
  },
};

renderWorkspace(mockSummary, mockCanvas, "command-center", healthySnapshot, healthyContext);
const healthyBrief = mockCanvas.children.find((c) => c.id === "daily-brief");
assert.ok(healthyBrief !== undefined);
const healthyBadge = healthyBrief.children[1].children[0];
assert.ok(healthyBadge.textContent.includes("NOMINAL"), "All healthy dependencies produce NOMINAL brief");
assert.ok(healthyBadge.className.includes("f-badge--good"));

// 4. Schema syntax and structure validation
const operatorTaskSchema = JSON.parse(
  await readFile(resolve(repoRoot, "contracts", "json-schema", "v1", "operator-task.schema.json"), "utf8")
);
assert.equal(operatorTaskSchema.title, "Follon Operator Task Contract v1");
assert.deepEqual(operatorTaskSchema.required, [
  "task_schema_version", "task_id", "cause", "severity", "environment",
  "account_id", "evidence_ids", "permitted_action", "state", "created_at",
  "updated_at", "history",
]);

const recoveryManifestSchema = JSON.parse(
  await readFile(resolve(repoRoot, "contracts", "json-schema", "v1", "recovery-manifest.schema.json"), "utf8")
);
assert.equal(recoveryManifestSchema.title, "Follon Recovery Manifest Contract v1");
assert.deepEqual(recoveryManifestSchema.required, [
  "recovery_manifest_schema_version", "manifest_id", "release_id",
  "generated_at", "schema_checksums", "configuration_hashes",
  "backup_checksums", "key_recovery_reference", "restore_procedure_ref",
  "last_drill",
]);

console.log("Workstation cockpit, typed routes, daily brief, and Increment 1 contract regressions passed");
