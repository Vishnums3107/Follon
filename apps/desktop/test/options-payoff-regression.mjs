import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { renderWorkspace } from "../dist/workspaces/index.js";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(testDirectory, "..", "..", "..");
const temporaryDirectory = mkdtempSync(join(tmpdir(), "follon-options-payoff-"));

class MockElement {
  constructor(tag) {
    this.tag = tag;
    this.children = [];
    this.textContent = "";
    this.style = {};
  }
  get rows() { return this.children; }
  append(...items) { this.children.push(...items); }
  replaceChildren(...items) { this.children = items; }
  setAttribute(k, v) { this[k] = v; }
  addEventListener() {}
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

function containsText(node, expected) {
  if (!node) return false;
  return node.textContent?.includes(expected) || node.children?.some((child) => containsText(child, expected));
}

try {
  const optionsDashboardPath = join(temporaryDirectory, "options-dashboard.json");
  execFileSync("cargo", [
    "run", "-q", "-p", "follon-cli", "--bin", "follon-options", "--",
    "analyze", "tests/fixtures/config/options-v1.json", optionsDashboardPath,
  ], { cwd: repositoryRoot, encoding: "utf8", stdio: "pipe" });

  const optionsDashboard = JSON.parse(readFileSync(optionsDashboardPath, "utf8"));

  const mockSnapshot = {
    workspace_schema_version: 1,
    generated_at: "2026-09-10T00:00:00Z",
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
    advanced_evidence: [],
    paper: null,
    live: null,
    operations: null,
    options: { artifact: "follon-options-dashboard.json", modified_at: "2026-09-10T00:00:00Z", data: optionsDashboard },
    commercial_artifacts: [],
  };
  const mockContext = { status: null, features: [], artifacts: [], workspaceFeatures: [], onOpenArtifact: () => {} };

  const labSummary = new MockElement("div");
  const labCanvas = new MockElement("div");
  renderWorkspace(labSummary, labCanvas, "research-lab", mockSnapshot, mockContext);

  // The payoff visual must be computed only from the real, retained frozen option-chain analytics.
  assert.ok(containsText(labCanvas, `K = ${Number(optionsDashboard.chain.underlying_mark) === 100 ? "100.00" : optionsDashboard.chain.underlying_mark}`) ||
    optionsDashboard.analytics.some((leg) => containsText(labCanvas, `K = ${Number(leg.strike).toFixed(2)}`)),
    "A real contract strike from the frozen chain must be rendered in the payoff visual");
  assert.ok(containsText(labCanvas, optionsDashboard.chain.underlying_instrument_id), "Real underlying instrument ID must be rendered");
  assert.ok(!containsText(labCanvas, "K = $500.00"), "Fabricated fixed strike must not be rendered");
  assert.ok(!containsText(labCanvas, "STRADDLE DELTA-NEUTRAL"), "Fabricated static badge text must not be rendered");

  // Absent option-chain evidence must render no payoff visual at all, never a fabricated placeholder.
  const emptySnapshot = { ...mockSnapshot, options: null };
  const emptyLabSummary = new MockElement("div");
  const emptyLabCanvas = new MockElement("div");
  renderWorkspace(emptyLabSummary, emptyLabCanvas, "research-lab", emptySnapshot, mockContext);
  assert.ok(!containsText(emptyLabCanvas, "Deterministic Options Payoff"), "No payoff visual may render without real chain analytics");

  console.log("Options payoff visualizer regression tests (real frozen chain analytics only) passed cleanly!");
} finally {
  rmSync(temporaryDirectory, { recursive: true, force: true });
}
