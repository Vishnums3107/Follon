// Shared panel, table and metric builders.

import { formatBytes, formatTime } from "./format.js";
import type { EvidenceArtifact, WorkspaceContext, WorkspaceSnapshot } from "./types.js";

type Metric = readonly [label: string, value: string, detail: string, state?: "good" | "warn" | "bad", isSignature?: boolean];

const tableFilterValues = new Map<string, string>();

export function renderFeatureEvidence(context: WorkspaceContext, featureIds: readonly string[]): HTMLElement {
  const artifacts = context.artifacts.filter((item) => featureIds.includes(item.feature));
  return renderArtifactPanel("Workspace evidence", artifacts.slice(0, 30), context.onOpenArtifact);
}

export function renderArtifactPanel(title: string, artifacts: readonly EvidenceArtifact[], onOpen: (name: string) => void): HTMLElement {
  const panel = createPanel(title, "Open an immutable artifact in the evidence inspector.");
  const list = document.createElement("div");
  list.className = "workspace-artifact-grid";
  if (artifacts.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = "No matching local evidence is currently indexed.";
    list.append(empty);
  }
  for (const artifact of artifacts) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "artifact-link-card";
    const name = document.createElement("strong");
    name.textContent = artifact.name;
    const meta = document.createElement("span");
    meta.textContent = `${artifact.kind} · ${formatBytes(artifact.bytes)} · ${formatTime(artifact.modified_at)}`;
    button.append(name, meta);
    button.addEventListener("click", () => onOpen(artifact.name));
    list.append(button);
  }
  panel.append(list);
  return panel;
}

export function renderMetrics(root: HTMLElement, metrics: readonly Metric[]): void {
  root.replaceChildren();
  for (const [label, value, detail, state, isSignature] of metrics) {
    const card = document.createElement("article");
    card.className = `workspace-metric${state === undefined ? "" : ` metric-${state}`}${isSignature ? " f-card--signature" : ""}`;
    const labelElement = document.createElement("p");
    labelElement.className = "metric-label";
    labelElement.textContent = label;
    const valueElement = document.createElement("p");
    valueElement.className = "metric-value";
    valueElement.textContent = value;
    const detailElement = document.createElement("p");
    detailElement.className = "metric-detail";
    detailElement.textContent = detail;
    card.append(labelElement, valueElement, detailElement);
    root.append(card);
  }
}

export function createPanel(titleText: string, descriptionText: string): HTMLElement {
  const panel = document.createElement("section");
  panel.className = "workspace-panel";
  const heading = document.createElement("div");
  heading.className = "workspace-panel-heading";
  const title = document.createElement("h3");
  title.textContent = titleText;
  const description = document.createElement("p");
  description.textContent = descriptionText;
  heading.append(title, description);
  panel.append(heading);
  return panel;
}

export function appendDefinition(parent: HTMLElement, values: ReadonlyArray<readonly [string, string]>): void {
  const list = document.createElement("dl");
  list.className = "workspace-definition";
  for (const [label, value] of values) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value || "—";
    list.append(term, detail);
  }
  parent.append(list);
}

export function appendTableOrEmpty(
  parent: HTMLElement,
  headers: readonly string[],
  rows: readonly (readonly string[])[],
  emptyText: string,
  onRow?: (index: number) => void,
  actions?: {
    label: string;
    onClick: (rowIndex: number) => void;
    showIf?: (row: readonly string[], rowIndex: number) => boolean;
  }[],
): void {
  if (rows.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = emptyText;
    parent.append(empty);
    return;
  }
  const scroll = document.createElement("div");
  scroll.className = "table-scroll f-table-container";
  const table = document.createElement("table");
  table.className = "f-table";
  const heading = document.createElement("thead");
  const headerRow = document.createElement("tr");
  for (const header of headers) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = header;
    headerRow.append(cell);
  }
  if (actions) {
    for (const action of actions) {
      const cell = document.createElement("th");
      cell.scope = "col";
      cell.textContent = action.label;
      headerRow.append(cell);
    }
  }
  heading.append(headerRow);
  table.append(heading);
  const body = document.createElement("tbody");
  rows.forEach((values, index) => {
    const row = document.createElement("tr");
    if (onRow !== undefined) {
      row.className = "clickable-row";
      row.tabIndex = 0;
      row.addEventListener("click", () => onRow(index));
      row.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onRow(index);
        }
      });
    }
    values.forEach((value, cellIndex) => {
      const cell = document.createElement("td");
      cell.setAttribute("data-label", headers[cellIndex] || "");
      cell.textContent = value || "—";
      row.append(cell);
    });
    if (actions) {
      actions.forEach(action => {
        const cell = document.createElement("td");
        if (!action.showIf || action.showIf(values, index)) {
          const btn = document.createElement("button");
          btn.className = "f-btn f-btn--sm";
          btn.textContent = action.label;
          btn.onclick = (e) => {
            e.stopPropagation();
            action.onClick(index);
          };
          cell.append(btn);
        }
        row.append(cell);
      });
    }
  body.append(row);
  });
  table.append(body);
  scroll.append(table);
  const toolbar = document.createElement("div");
  toolbar.className = "collection-toolbar";
  const search = document.createElement("input");
  search.type = "search";
  search.className = "f-input";
  search.placeholder = "Filter these records…";
  search.setAttribute("aria-label", `Filter ${parent.querySelector("h3")?.textContent ?? "table"} records`);
  const filterKey = `${parent.id || parent.querySelector("h3")?.textContent || "table"}:${headers.join("|")}`;
  search.value = tableFilterValues.get(filterKey) ?? "";
  const status = document.createElement("span");
  status.setAttribute("role", "status");
  const previous = document.createElement("button");
  const next = document.createElement("button");
  previous.type = next.type = "button";
  previous.className = next.className = "f-btn";
  previous.textContent = "Previous";
  next.textContent = "Next";
  const domRows = Array.from(body.rows);
  let page = 0;
  const pageSize = 20;
  const draw = () => {
    const query = search.value.trim().toLowerCase();
    const matches = domRows.filter((_, index) => rows[index].some((value) => value.toLowerCase().includes(query)));
    const pages = Math.max(1, Math.ceil(matches.length / pageSize));
    page = Math.min(page, pages - 1);
    for (const row of domRows) row.hidden = true;
    for (const row of matches.slice(page * pageSize, (page + 1) * pageSize)) row.hidden = false;
    status.textContent = `${matches.length} of ${rows.length} records · Page ${page + 1} of ${pages}`;
    previous.disabled = page === 0;
    next.disabled = page >= pages - 1;
  };
  search.addEventListener("input", () => {
    tableFilterValues.set(filterKey, search.value);
    page = 0;
    draw();
  });
  previous.addEventListener("click", () => { page--; draw(); });
  next.addEventListener("click", () => { page++; draw(); });
  toolbar.append(search, status, previous, next);
  parent.append(toolbar);
  parent.append(scroll);
  draw();
}

export function renderEmpty(parent: HTMLElement, titleText: string, detailText: string): void {
  const title = document.createElement("h3");
  title.textContent = titleText;
  const detail = document.createElement("p");
  detail.className = "empty-state";
  detail.textContent = detailText;
  parent.append(title, detail);
}

export function renderProjectionIntegrityPanel(
  snapshot: WorkspaceSnapshot,
  context: WorkspaceContext,
): HTMLElement | undefined {
  const truncatedArtifacts = (snapshot.event_windows ?? []).filter((window) => window.truncated);
  const diagnostics = snapshot.projection_diagnostics ?? [];
  const globallyTruncated = snapshot.event_window?.truncated ?? false;
  if (!globallyTruncated && truncatedArtifacts.length === 0 && diagnostics.length === 0) return undefined;

  const panel = createPanel(
    "Evidence projection integrity",
    "Rejected envelopes and bounded windows are disclosed separately. An incomplete window is not a complete or current audit trail.",
  );
  if (globallyTruncated) {
    const summary = document.createElement("p");
    summary.className = "empty-state";
    summary.textContent = `The causal replay projection retains ${snapshot.event_window?.retained_event_count ?? 0} events from a source window containing at least ${snapshot.event_window?.source_event_count_lower_bound ?? 0} validated envelopes.`;
    panel.append(summary);
  }
  appendTableOrEmpty(
    panel,
    ["Artifact", "Window", "Retained records", "Retained events", "First event", "Last event"],
    truncatedArtifacts.map((window) => [
      window.artifact,
      `${window.window_kind}; source records >= ${window.source_record_count_lower_bound}`,
      String(window.retained_record_count),
      String(window.retained_event_count),
      `${window.first_event_time ?? "Unknown"} / ${window.first_event_id ?? "Unknown"}`,
      `${window.last_event_time ?? "Unknown"} / ${window.last_event_id ?? "Unknown"}`,
    ]),
    diagnostics.length === 0 ? "No incomplete artifact windows are present." : "See rejected-record diagnostics below.",
    (index) => context.onOpenArtifact(truncatedArtifacts[index]?.artifact ?? ""),
  );
  if (diagnostics.length > 0) {
    appendTableOrEmpty(
      panel,
      ["Artifact", "Rejection", "Detail"],
      diagnostics.map((diagnostic) => [diagnostic.artifact, diagnostic.code, diagnostic.detail]),
      "No rejected records are present.",
      (index) => context.onOpenArtifact(diagnostics[index]?.artifact ?? ""),
    );
  }
  return panel;
}
