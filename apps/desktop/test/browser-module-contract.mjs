import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const appDirectory = resolve(testDirectory, "..");
const index = await readFile(resolve(appDirectory, "index.html"), "utf8");
const appShell = await readFile(resolve(appDirectory, "src", "app-shell.tsx"), "utf8");
const styles = await readFile(resolve(appDirectory, "styles.css"), "utf8");
const favicon = await readFile(resolve(appDirectory, "favicon.svg"), "utf8");

assert.match(index, /<script type="module" src="\/src\/react-main\.tsx"><\/script>/u);
assert.match(index, /<div id="root"><\/div>/u);
for (const id of ["workspace-detail", "workspace-summary", "workspace-canvas", "refresh-workspace"]) {
  assert.ok(appShell.includes(`id="${id}"`), `missing React workspace control ${id}`);
}
assert.doesNotMatch(appShell, /id="workspace-evidence"[^>]*\shidden(?:\s|>)/u);

for (const font of ["SF Pro Display", "SF Pro Text", "SF Compact", "SF Mono"]) {
  assert.ok(styles.includes(font), `missing requested system font mapping: ${font}`);
}
const colorTokens = [...`${styles}\n${favicon}`.matchAll(/#[0-9a-fA-F]{3,8}\b/gu)].map((match) => match[0].toLowerCase());
assert.ok(colorTokens.every((color) => color === "#000" || color === "#fff"), `non-monochrome color found: ${colorTokens.join(", ")}`);
assert.match(styles, /body\s*\{[^}]*background:\s*var\(--white\)/su);
assert.match(styles, /\.workspace-detail\s*\{[^}]*background:\s*var\(--black\)/su);

const visited = new Set();
async function verifyModule(relativePath) {
  if (visited.has(relativePath)) return;
  visited.add(relativePath);
  const source = await readFile(resolve(appDirectory, "dist", relativePath), "utf8");
  for (const match of source.matchAll(/from\s+["'](\.\/.+?)["']/gu)) {
    const specifier = match[1];
    assert.ok(specifier.endsWith(".js"), `${relativePath} has a browser-incompatible import: ${specifier}`);
    await verifyModule(specifier.slice(2));
  }
}

await verifyModule("main.js");
assert.deepEqual([...visited].sort(), ["catalog.js", "evidence.js", "main.js", "workspaces.js"]);
const main = await readFile(resolve(appDirectory, "src", "main.ts"), "utf8");
const workspaces = await readFile(resolve(appDirectory, "src", "workspaces.ts"), "utf8");
const workspaceContracts = new Map([
  ["command-center", ["System, broker, strategy, and risk status", "Environment readiness", "Attention queue"]],
  ["research-lab", ["Dataset inventory", "Notebook inventory", "Experiment catalogue"]],
  ["strategy-studio", ["Version and deployment identities", "Worker contract"]],
  ["backtest-explorer", ["Run comparison", "Trade evidence", "Regime and sensitivity dimensions"]],
  ["execution-blotter", ["Causal execution blotter", "Explainable risk decisions", "Broker lifecycle condition coverage"]],
  ["risk-cockpit", ["Exposure and loss control", "Versioned risk limits", "Alerts and reconciliation"]],
  ["portfolio", ["Internal positions", "P&L attribution", "Options scenario and book reconciliation"]],
  ["replay-incidents", ["Event distribution", "Causal replay timeline", "Incident and recovery state"]],
  ["journal", ["Chain integrity", "Unified append-only journal", "Details / annotation"]],
  ["administration", ["Commercial ledger", "Deployment and administrative controls", "Privileged-action boundary"]],
]);
for (const [workspaceId, signatures] of workspaceContracts) {
  assert.ok(main.includes(`id: "${workspaceId}"`), `missing primary navigation workspace ${workspaceId}`);
  assert.ok(workspaces.includes(`case "${workspaceId}"`), `missing workspace renderer ${workspaceId}`);
  for (const signature of signatures) {
    assert.ok(workspaces.includes(signature), `${workspaceId} is missing feature surface: ${signature}`);
  }
}
assert.match(main, /function decodeWorkspaceId\(value: string\)/u);
assert.match(main, /window\.location\.hostname === "tauri\.localhost"/u);
assert.match(main, /const apiOrigin = isTauriRuntime \? "http:\/\/127\.0\.0\.1:8080"/u);
assert.match(main, /function apiUrl\(path: string\)/u);
assert.doesNotMatch(main, /fetch\("\/api\/v1\//u);
assert.match(main, /window\.addEventListener\("hashchange"/u);
assert.match(main, /candidate\.origin === window\.location\.origin/u);
assert.match(main, /candidate\.protocol === expectedProtocol/u);
assert.match(workspaces, /Explainable risk decisions/u);
assert.match(workspaces, /function isDatasetSummary\(value: unknown\)/u);
assert.match(workspaces, /function isBacktestSummary\(value: unknown\)/u);
assert.match(workspaces, /Notebook inventory/u);
assert.match(workspaces, /never executes cells, JavaScript outputs, or embedded HTML/u);
assert.match(workspaces, /Trade evidence/u);
assert.match(workspaces, /execution\.fill\.v1/u);
assert.match(workspaces, /Regime and sensitivity dimensions/u);
assert.match(workspaces, /missing tags are reported explicitly rather than inferred/u);
assert.match(workspaces, /Decisions, annotations, and review evidence/u);
assert.match(workspaces, /Details \/ annotation/u);
console.log("Browser module graph / workspace shell contract passed");
