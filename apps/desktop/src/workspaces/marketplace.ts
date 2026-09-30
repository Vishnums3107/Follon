// Research asset marketplace workspace.

import { parseSandboxInstallationPreview, parseStrategyCapsuleManifest } from "../evidence/index.js";
import { appendAdvancedEvidenceRows } from "./advanced-evidence.js";
import { displayName, formatBytes, formatTime, shortHash } from "./format.js";
import { appendTableOrEmpty, createPanel, renderEmpty, renderMetrics } from "./panels.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

export function renderMarketplace(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const listings = context.artifacts.filter((artifact) => ["market-data", "research", "replay"].includes(artifact.feature));
  renderMetrics(summaryRoot, [
    ["Local assets", String(listings.length), "Available in the evidence index"],
    ["Datasets", String(snapshot.datasets.length), "Inputs with indexed metadata"],
    ["Backtest results", String(snapshot.backtests.length), "Inspect performance and provenance"],
    ["Catalogue mode", "Local", "Publishing, purchasing, and installation are not connected"],
  ]);
  const panel = createPanel("Research asset marketplace", "Discover datasets, strategy run evidence, and reports from this installation. An indexed asset is not an approved strategy bundle.");
  const toolbar = document.createElement("div");
  toolbar.className = "collection-toolbar";
  const search = document.createElement("input");
  search.type = "search";
  search.className = "f-input";
  search.placeholder = "Search assets by name, type, or feature";
  search.setAttribute("aria-label", "Search marketplace assets");
  const category = document.createElement("select");
  category.className = "f-select";
  category.setAttribute("aria-label", "Marketplace category");
  for (const [value, label] of [["all", "All categories"], ["market-data", "Market data"], ["research", "Research"], ["replay", "Replay"]]) {
    category.append(new Option(label, value));
  }
  const count = document.createElement("span");
  count.setAttribute("role", "status");
  toolbar.append(search, category, count);
  const results = document.createElement("div");
  results.className = "marketplace-grid";
  const more = document.createElement("button");
  more.type = "button";
  more.className = "f-btn";
  more.textContent = "Show more assets";
  let limit = 24;
  const draw = () => {
    const query = search.value.trim().toLowerCase();
    const matches = listings.filter((item) => (category.value === "all" || item.feature === category.value) &&
      `${item.name} ${item.kind} ${item.feature}`.toLowerCase().includes(query));
    results.replaceChildren();
    count.textContent = `${Math.min(limit, matches.length)} of ${matches.length} assets`;
    more.hidden = matches.length <= limit;
    if (matches.length === 0) {
      renderEmpty(results, listings.length ? "No matching assets" : "Your local catalogue is empty", listings.length
        ? "Change the search or category to see more assets."
        : "Publish research outputs under the configured evidence directory, then refresh this workspace. Browse Research Lab for dataset metadata and Backtest for completed runs.");
    }
    for (const item of matches.slice(0, limit)) {
      const card = document.createElement("article");
      card.className = "marketplace-card";
      const tag = document.createElement("span");
      tag.className = "workspace-badge";
      tag.textContent = displayName(item.feature);
      const title = document.createElement("h4");
      title.textContent = item.name;
      const detail = document.createElement("p");
      detail.textContent = `${item.kind} · ${formatBytes(item.bytes)} · ${formatTime(item.modified_at)}`;
      const open = document.createElement("button");
      open.type = "button";
      open.className = "f-btn";
      open.textContent = "Inspect asset";
      open.setAttribute("aria-label", `Inspect ${item.name}`);
      open.addEventListener("click", () => context.onOpenArtifact(item.name));
      card.append(tag, title, detail, open);
      results.append(card);
    }
  };
  search.addEventListener("input", () => { limit = 24; draw(); });
  category.addEventListener("change", () => { limit = 24; draw(); });
  more.addEventListener("click", () => { limit += 24; draw(); });
  panel.append(toolbar, results, more);
  draw();
  root.append(panel);

  const assetComparison = createPanel(
    "Evidence-based asset comparison",
    "Compare research assets across evaluation coverage, cost assumptions, parameter stability, and source freshness without fabricated ratings (ASSET-02)."
  );
  assetComparison.id = "asset-comparison-panel";
  appendTableOrEmpty(
    assetComparison,
    ["Asset Identity", "Kind", "Dataset Window", "Cost Model Assumption", "Parameter Stability", "Source Freshness", "Evaluation Disposition"],
    listings.map((asset) => [
      asset.name,
      asset.kind,
      "Not published by an asset-listing contract",
      "Not published by an asset-listing contract",
      "Not published by an asset-listing contract",
      formatTime(asset.modified_at),
      "Unassessed",
    ]),
    "No local assets are indexed.",
    (index) => context.onOpenArtifact(listings[index]?.name ?? "")
  );
  root.append(assetComparison);

  const sandboxPreviewPanel = createPanel(
    "Sandboxed installation preview and capability inspector",
    "Deterministic capability inspection and sandboxed dry-run preview for research packages before installation, verifying permissions and network isolation (ASSET-03, ASSET-04)."
  );
  sandboxPreviewPanel.id = "sandbox-preview-panel";
  appendAdvancedEvidenceRows(
    sandboxPreviewPanel,
    snapshot,
    context,
    "sandbox_installation_preview",
    parseSandboxInstallationPreview,
    ["Asset Package", "Sandbox Isolation", "Allowed Network", "Filesystem Access", "Capability Budget", "Security Audit", "Preview Verdict"],
    (preview) => [[
      `${preview.asset_id}@${preview.asset_version}`,
      preview.resource_caps.filesystem_isolated ? "Filesystem isolated" : "Filesystem not isolated",
      "Not published by this contract",
      preview.resource_caps.filesystem_isolated ? "Isolated" : "Unknown",
      `${preview.resource_caps.max_cpu_percent}% CPU / ${preview.resource_caps.max_memory_mb} MB`,
      `${preview.untrusted_capabilities_detected} untrusted capability/capabilities detected`,
      preview.disposition,
    ]],
    "No typed sandbox-installation preview is published."
  );
  root.append(sandboxPreviewPanel);

  const strategyCapsulePanel = createPanel(
    "Portable strategy capsules and replay manifests",
    "Export and inspect reproducible strategy capsules containing cryptographically bound code, configuration digests, dependency lockfiles, and replay verification instructions (ASSET-04)."
  );
  strategyCapsulePanel.id = "strategy-capsule-panel";
  appendAdvancedEvidenceRows(
    strategyCapsulePanel,
    snapshot,
    context,
    "strategy_capsule_manifest",
    parseStrategyCapsuleManifest,
    ["Capsule ID", "Strategy / Version", "Bundle Hash", "Config Hash", "Runtime Target", "Evaluation Receipt", "Export Disposition"],
    (capsule) => [[
      capsule.capsule_id,
      `${capsule.strategy_id}@${capsule.strategy_version}`,
      shortHash(capsule.bundle_sha256),
      shortHash(capsule.configuration_sha256),
      capsule.runtime_target,
      capsule.evaluation_receipt_id,
      capsule.export_disposition,
    ]],
    "No typed strategy capsule manifest is published."
  );
  root.append(strategyCapsulePanel);
}
