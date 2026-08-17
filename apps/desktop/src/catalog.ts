export type ServiceState = Readonly<{
  status: "healthy" | "degraded" | "unavailable";
  detail: string;
}>;

export type SystemStatus = Readonly<{
  dashboard_schema_version: 1;
  generated_at: string;
  mode: string;
  read_only: true;
  authentication: string;
  services: Readonly<Record<string, ServiceState>>;
  artifacts: Readonly<{ count: number; latest_at: string | null }>;
}>;

export type FeatureDefinition = Readonly<{
  id: string;
  title: string;
  state: "implemented" | "gated";
  summary: string;
  capabilities: readonly string[];
  boundary: string;
  gate: string;
  screens: readonly string[];
  source: string;
  documentation: string;
}>;

export function isSystemStatus(value: unknown): value is SystemStatus {
  if (!isRecord(value) || value.dashboard_schema_version !== 1 || value.read_only !== true) {
    return false;
  }
  return typeof value.generated_at === "string" && typeof value.mode === "string" &&
    typeof value.authentication === "string" && isRecord(value.services) && isRecord(value.artifacts) &&
    typeof value.artifacts.count === "number" &&
    (value.artifacts.latest_at === null || typeof value.artifacts.latest_at === "string");
}

export function isFeatureDefinitions(value: unknown): value is FeatureDefinition[] {
  return Array.isArray(value) && value.every((item) => isRecord(item) &&
    typeof item.id === "string" && typeof item.title === "string" &&
    (item.state === "implemented" || item.state === "gated") &&
    typeof item.summary === "string" && Array.isArray(item.capabilities) &&
    item.capabilities.every((capability) => typeof capability === "string") &&
    typeof item.boundary === "string" && typeof item.gate === "string" &&
    Array.isArray(item.screens) && item.screens.every((screen) => typeof screen === "string") &&
    typeof item.source === "string" && typeof item.documentation === "string");
}

export function renderCoverageSummary(
  root: HTMLElement,
  features: readonly FeatureDefinition[],
  artifactCounts: ReadonlyMap<string, number>,
): void {
  const screenCount = new Set(features.flatMap((feature) => feature.screens)).size;
  const capabilityCount = features.reduce((total, feature) => total + feature.capabilities.length, 0);
  const integratedCount = features.filter((feature) => (artifactCounts.get(feature.id) ?? 0) > 0).length;
  const values = [
    ["Feature areas", String(features.length), "Documented implementation domains"],
    ["Primary screens", String(screenCount), "Mapped to evidence workspaces"],
    ["Capabilities", String(capabilityCount), "Visible functions and controls"],
    ["With evidence", `${integratedCount}/${features.length}`, "Local artifacts currently indexed"],
  ] as const;
  root.replaceChildren();
  for (const [labelText, valueText, detailText] of values) {
    const card = document.createElement("article");
    card.className = "metric-card";
    const label = document.createElement("p");
    label.className = "metric-label";
    label.textContent = labelText;
    const value = document.createElement("p");
    value.className = "metric-value";
    value.textContent = valueText;
    const detail = document.createElement("p");
    detail.className = "metric-detail";
    detail.textContent = detailText;
    card.append(label, value, detail);
    root.append(card);
  }
}

export function renderSystemStatus(root: HTMLElement, status: SystemStatus): void {
  root.replaceChildren();
  const serviceOrder = ["dashboard", "postgres", "minio"];
  for (const serviceName of serviceOrder) {
    const service = status.services[serviceName];
    if (service === undefined) {
      continue;
    }
    const card = document.createElement("article");
    card.className = "metric-card";
    const label = document.createElement("p");
    label.className = "metric-label";
    label.textContent = displayName(serviceName);
    const value = document.createElement("p");
    value.className = `metric-value service-${service.status}`;
    value.textContent = service.status;
    const detail = document.createElement("p");
    detail.className = "metric-detail";
    detail.textContent = service.detail;
    card.append(label, value, detail);
    root.append(card);
  }

  const artifactCard = document.createElement("article");
  artifactCard.className = "metric-card";
  const artifactLabel = document.createElement("p");
  artifactLabel.className = "metric-label";
  artifactLabel.textContent = "Artifacts";
  const artifactValue = document.createElement("p");
  artifactValue.className = "metric-value";
  artifactValue.textContent = String(status.artifacts.count);
  const artifactDetail = document.createElement("p");
  artifactDetail.className = "metric-detail";
  artifactDetail.textContent = status.artifacts.latest_at === null
    ? "No evidence yet"
    : `Latest ${formatTimestamp(status.artifacts.latest_at)}`;
  artifactCard.append(artifactLabel, artifactValue, artifactDetail);
  root.append(artifactCard);
}

export function renderFeatureCatalog(
  root: HTMLElement,
  features: readonly FeatureDefinition[],
  artifactCounts: ReadonlyMap<string, number>,
  onSelect: (featureId: string) => void,
): void {
  root.replaceChildren();
  for (const feature of features) {
    const card = document.createElement("article");
    card.className = "feature-card";
    const top = document.createElement("div");
    top.className = "feature-card-top";
    const title = document.createElement("h3");
    title.textContent = feature.title;
    const state = document.createElement("span");
    state.className = `feature-state feature-${feature.state}`;
    state.textContent = feature.state === "implemented" ? "Implemented" : "Gate open";
    top.append(title, state);

    const summary = document.createElement("p");
    summary.className = "feature-summary";
    summary.textContent = feature.summary;
    const capabilities = document.createElement("ul");
    capabilities.className = "capability-list";
    for (const capability of feature.capabilities) {
      const item = document.createElement("li");
      item.textContent = capability;
      capabilities.append(item);
    }
    const boundary = document.createElement("p");
    boundary.className = "feature-boundary";
    boundary.textContent = feature.boundary;
    const gate = document.createElement("p");
    gate.className = "feature-gate";
    gate.textContent = feature.gate;

    const screens = document.createElement("p");
    screens.className = "feature-screens";
    screens.textContent = `Views: ${feature.screens.join(" · ")}`;
    const source = document.createElement("p");
    source.className = "feature-source";
    source.textContent = `Integrated from ${feature.source}`;
    const documentation = document.createElement("p");
    documentation.className = "feature-documentation";
    documentation.textContent = feature.documentation;

    const view = document.createElement("button");
    view.className = "feature-button";
    view.type = "button";
    const count = artifactCounts.get(feature.id) ?? 0;
    view.textContent = count === 0 ? "No local artifacts" : `View ${count} artifact${count === 1 ? "" : "s"}`;
    view.disabled = count === 0;
    view.addEventListener("click", () => onSelect(feature.id));
    card.append(top, summary, capabilities, screens, source, documentation, boundary, gate, view);
    root.append(card);
  }
}

export function renderGenericArtifact(root: HTMLElement, contents: string, source: string): string {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = humanizeFileName(source);
  root.append(heading);

  if (source.toLowerCase().endsWith(".csv")) {
    const rows = parseCsv(contents);
    renderCsv(root, rows);
    return `Loaded ${Math.max(0, rows.length - 1)} data rows from ${source}.`;
  }
  if (source.toLowerCase().endsWith(".md")) {
    const description = document.createElement("p");
    description.textContent = "Immutable generated report";
    const report = document.createElement("pre");
    report.className = "document-preview";
    report.textContent = contents;
    root.append(description, report);
    return `Loaded generated report ${source}.`;
  }
  if (source.toLowerCase().endsWith(".ndjson")) {
    try {
      const records = contents.split(/\r?\n/).filter((line) => line.trim().length > 0)
        .map((line) => JSON.parse(line) as unknown);
      const description = document.createElement("p");
      description.textContent = "Structured append-only records displayed in source order.";
      root.append(description);
      appendJsonValue(root, records, 0);
      return `Loaded ${records.length} append-only record${records.length === 1 ? "" : "s"} from ${source}.`;
    } catch {
      // Fall through to the inert text preview for non-JSON line formats.
    }
  }

  try {
    const value: unknown = JSON.parse(contents);
    const description = document.createElement("p");
    description.textContent = "Validated as JSON and displayed without executable markup.";
    root.append(description);
    appendJsonValue(root, value, 0);
    return `Loaded structured evidence from ${source}.`;
  } catch {
    const preview = document.createElement("pre");
    preview.className = "document-preview";
    preview.textContent = contents;
    root.append(preview);
    return `Loaded text artifact ${source}.`;
  }
}

function appendJsonValue(parent: HTMLElement, value: unknown, depth: number): void {
  if (depth > 6) {
    const truncated = document.createElement("p");
    truncated.textContent = "Nested value omitted beyond display depth.";
    parent.append(truncated);
    return;
  }
  if (Array.isArray(value)) {
    if (value.every(isFlatRecord) && value.length > 0) {
      appendRecordTable(parent, value);
      return;
    }
    const list = document.createElement("ol");
    list.className = "json-list";
    for (const item of value) {
      const entry = document.createElement("li");
      if (isScalar(item)) {
        entry.textContent = scalarText(item);
      } else {
        appendJsonValue(entry, item, depth + 1);
      }
      list.append(entry);
    }
    parent.append(list);
    return;
  }
  if (isRecord(value)) {
    const definition = document.createElement("dl");
    const nested: Array<readonly [string, unknown]> = [];
    for (const [key, item] of Object.entries(value)) {
      if (isScalar(item)) {
        const term = document.createElement("dt");
        term.textContent = displayName(key);
        const detail = document.createElement("dd");
        detail.textContent = scalarText(item);
        definition.append(term, detail);
      } else {
        nested.push([key, item]);
      }
    }
    if (definition.childElementCount > 0) {
      parent.append(definition);
    }
    for (const [key, item] of nested) {
      const details = document.createElement("details");
      details.open = depth < 1;
      const summary = document.createElement("summary");
      summary.textContent = displayName(key);
      const content = document.createElement("div");
      content.className = "nested-evidence";
      appendJsonValue(content, item, depth + 1);
      details.append(summary, content);
      parent.append(details);
    }
    return;
  }
  const scalar = document.createElement("p");
  scalar.textContent = scalarText(value);
  parent.append(scalar);
}

function appendRecordTable(parent: HTMLElement, rows: ReadonlyArray<Record<string, unknown>>): void {
  const headers = [...new Set(rows.flatMap((row) => Object.keys(row)))].slice(0, 12);
  const table = document.createElement("table");
  const heading = document.createElement("tr");
  for (const header of headers) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = displayName(header);
    heading.append(cell);
  }
  table.append(heading);
  for (const row of rows.slice(0, 500)) {
    const tableRow = document.createElement("tr");
    for (const header of headers) {
      const cell = document.createElement("td");
      const value = row[header];
      cell.textContent = isScalar(value) ? scalarText(value) : "[structured]";
      tableRow.append(cell);
    }
    table.append(tableRow);
  }
  parent.append(table);
}

function renderCsv(root: HTMLElement, rows: readonly (readonly string[])[]): void {
  if (rows.length === 0) {
    const empty = document.createElement("p");
    empty.textContent = "The CSV artifact is empty.";
    root.append(empty);
    return;
  }
  const table = document.createElement("table");
  for (const [rowIndex, values] of rows.slice(0, 501).entries()) {
    const row = document.createElement("tr");
    for (const value of values) {
      const cell = document.createElement(rowIndex === 0 ? "th" : "td");
      if (cell instanceof HTMLTableCellElement && rowIndex === 0) {
        cell.scope = "col";
      }
      cell.textContent = value;
      row.append(cell);
    }
    table.append(row);
  }
  root.append(table);
}

function parseCsv(contents: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let value = "";
  let quoted = false;
  for (let index = 0; index < contents.length; index += 1) {
    const character = contents[index];
    if (character === '"') {
      if (quoted && contents[index + 1] === '"') {
        value += '"';
        index += 1;
      } else {
        quoted = !quoted;
      }
    } else if (character === "," && !quoted) {
      row.push(value);
      value = "";
    } else if ((character === "\n" || character === "\r") && !quoted) {
      if (character === "\r" && contents[index + 1] === "\n") {
        index += 1;
      }
      row.push(value);
      if (row.some((cell) => cell.length > 0)) {
        rows.push(row);
      }
      row = [];
      value = "";
    } else {
      value += character;
    }
  }
  row.push(value);
  if (row.some((cell) => cell.length > 0)) {
    rows.push(row);
  }
  return rows;
}

function isFlatRecord(value: unknown): value is Record<string, unknown> {
  return isRecord(value) && Object.values(value).every(isScalar);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isScalar(value: unknown): value is string | number | boolean | null | undefined {
  return value === null || value === undefined || ["string", "number", "boolean"].includes(typeof value);
}

function scalarText(value: unknown): string {
  if (value === null) {
    return "Not set";
  }
  if (value === undefined) {
    return "—";
  }
  return String(value);
}

function displayName(value: string): string {
  return value.replaceAll("_", " ").replaceAll("-", " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function humanizeFileName(value: string): string {
  return displayName(value.replace(/\.(?:ndjson|json|md|csv)$/i, ""));
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString();
}
