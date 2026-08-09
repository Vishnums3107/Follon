export type EvidenceEvent = Readonly<{
  event_id: string;
  event_type: string;
  event_time: string;
  correlation_id: string;
  causation_id: string | null;
  actor: string;
  source: string;
  payload: Record<string, unknown>;
}>;

export type PaperDashboard = Readonly<{
  dashboard_schema_version: 1;
  environment: "PAPER";
  account_id: string;
  configuration_fingerprint: string;
  persistence_healthy: boolean;
  internal_cash: string;
  working_orders: number;
  unknown_orders: number;
  active_kill_switches: readonly string[];
  unexplained_incidents: number;
  last_reconciled_at: string | null;
  last_reconciliation_clean: boolean | null;
  clean_paper_days: number;
  required_paper_days: 30;
  promotion_eligible: boolean;
  positions: readonly PaperDashboardPosition[];
}>;

export type PaperDashboardPosition = Readonly<{
  instrument_id: string;
  quantity: string;
  average_cost: string;
  realized_pnl: string;
}>;

export type LiveMonitoringDashboard = Readonly<{
  dashboard_schema_version: 1;
  environment: "LIVE";
  mode: "SHADOW" | "CANARY";
  account_id: string;
  configuration_fingerprint: string;
  broker_connected: boolean;
  audit_healthy: boolean;
  audit_sequence: number;
  audit_head_hash: string;
  active_kill_switches: readonly string[];
  working_orders: number;
  unknown_orders: number;
  unresolved_incidents: number;
  last_reconciled_at: string | null;
  last_reconciliation_clean: boolean | null;
  clean_live_days: number;
  required_live_days: 60;
  promotion_eligible: boolean;
  internal_cash: string;
  positions: readonly PaperDashboardPosition[];
}>;

/** Parses and validates canonical NDJSON before it is shown as evidence. */
export function parseEvidenceLog(ndjson: string): EvidenceEvent[] {
  const eventIds = new Set<string>();
  const events: EvidenceEvent[] = [];
  for (const [index, line] of ndjson.split(/\r?\n/).filter(Boolean).entries()) {
    let candidate: unknown;
    try {
      candidate = JSON.parse(line);
    } catch {
      throw new Error(`Line ${index + 1} is not valid JSON.`);
    }
    if (!isEvidenceEvent(candidate)) {
      throw new Error(`Line ${index + 1} is not a valid Follon event envelope.`);
    }
    if (eventIds.has(candidate.event_id)) {
      throw new Error(`Line ${index + 1} repeats event ID ${candidate.event_id}.`);
    }
    if (candidate.causation_id !== null && !eventIds.has(candidate.causation_id)) {
      throw new Error(`Line ${index + 1} references an unseen causation event.`);
    }
    eventIds.add(candidate.event_id);
    events.push(candidate);
  }
  if (events.length === 0) {
    throw new Error("The selected event log is empty.");
  }
  return events;
}

/** Parses a server-owned paper-operations snapshot; it never creates an action. */
export function parsePaperDashboard(json: string): PaperDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Paper dashboard is not valid JSON.");
  }
  if (!isPaperDashboard(value)) {
    throw new Error("Paper dashboard does not match the v1 read-only contract.");
  }
  return value;
}

/** Parses a controlled-live monitoring projection; this format has no action fields. */
export function parseLiveMonitoringDashboard(json: string): LiveMonitoringDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Controlled-live monitoring dashboard is not valid JSON.");
  }
  if (!isLiveMonitoringDashboard(value)) {
    throw new Error("Controlled-live monitoring dashboard does not match the v1 read-only contract.");
  }
  return value;
}

const phaseByEventType: Readonly<Record<string, string>> = {
  "market.bar.v1": "Market bar",
  "intent.created.v1": "Strategy intent",
  "risk.decision.v1": "Risk decision",
  "order.state_changed.v1": "OMS transition",
  "execution.fill.v1": "Simulated fill",
  "portfolio.position_updated.v1": "Position update",
  "portfolio.pnl_updated.v1": "P&L update",
  "audit.trail.v1": "Audit trail",
};

/** Renders server-owned evidence only; it never creates a trading transition. */
export function renderEvidence(root: HTMLElement, events: readonly EvidenceEvent[]): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Simulation evidence";
  root.append(heading);

  const environment = document.createElement("p");
  environment.textContent = "SIMULATION ? no broker connectivity";
  root.append(environment);

  if (events.length === 0) {
    const empty = document.createElement("p");
    empty.textContent = "Waiting for an immutable event trail.";
    root.append(empty);
    return;
  }

  const trail = document.createElement("ol");
  trail.setAttribute("aria-label", "Causal event trail");
  for (const event of events) {
    const item = document.createElement("li");
    const phase = document.createElement("strong");
    phase.textContent = phaseByEventType[event.event_type] ?? event.event_type;
    item.append(phase, document.createTextNode(` ? ${event.event_time}`));

    const details = document.createElement("pre");
    details.textContent = JSON.stringify(
      {
        event_id: event.event_id,
        correlation_id: event.correlation_id,
        causation_id: event.causation_id,
        actor: event.actor,
        source: event.source,
        payload: event.payload,
      },
      null,
      2,
    );
    item.append(details);
    trail.append(item);
  }
  root.append(trail);

  const current = latestPortfolio(events);
  if (current !== undefined) {
    const summary = document.createElement("p");
    summary.textContent = `Current simulated P&L: ${String(current.total_pnl ?? "unavailable")}`;
    root.append(summary);
  }
}

/** Renders paper risk and reconciliation state without offering trading controls. */
export function renderPaperDashboard(root: HTMLElement, dashboard: PaperDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Paper operations dashboard";
  root.append(heading);

  const environment = document.createElement("p");
  environment.textContent = `Environment: ${dashboard.environment}. This screen is read-only.`;
  root.append(environment);

  const summary = document.createElement("dl");
  const values: ReadonlyArray<readonly [string, string]> = [
    ["Account", dashboard.account_id],
    ["Configuration", dashboard.configuration_fingerprint],
    ["Durable journal", dashboard.persistence_healthy ? "Healthy" : "FAILED — operations halted"],
    ["Internal cash", dashboard.internal_cash],
    ["Working orders", String(dashboard.working_orders)],
    ["Unknown orders", String(dashboard.unknown_orders)],
    ["Unexplained discrepancies", String(dashboard.unexplained_incidents)],
    ["Last reconciliation", dashboard.last_reconciled_at ?? "Not yet reconciled"],
    ["Last reconciliation result", dashboard.last_reconciliation_clean === null
      ? "Not yet reconciled"
      : dashboard.last_reconciliation_clean ? "Clean" : "Discrepancy detected"],
    ["Paper-day gate", `${dashboard.clean_paper_days}/${dashboard.required_paper_days}`],
    ["Promotion status", dashboard.promotion_eligible ? "Evidence gate complete" : "Evidence gate incomplete"],
  ];
  for (const [label, value] of values) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value;
    summary.append(term, detail);
  }
  root.append(summary);

  const killSwitches = document.createElement("p");
  killSwitches.textContent = dashboard.active_kill_switches.length === 0
    ? "Kill switches: clear"
    : `Kill switches: ${dashboard.active_kill_switches.join(", ")}`;
  root.append(killSwitches);

  const positionsHeading = document.createElement("h2");
  positionsHeading.textContent = "Internal positions";
  root.append(positionsHeading);
  if (dashboard.positions.length === 0) {
    const empty = document.createElement("p");
    empty.textContent = "No internal paper positions.";
    root.append(empty);
    return;
  }
  const table = document.createElement("table");
  const header = document.createElement("tr");
  for (const label of ["Instrument", "Quantity", "Average cost", "Realized P&L"]) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = label;
    header.append(cell);
  }
  table.append(header);
  for (const position of dashboard.positions) {
    const row = document.createElement("tr");
    for (const value of [position.instrument_id, position.quantity, position.average_cost, position.realized_pnl]) {
      const cell = document.createElement("td");
      cell.textContent = value;
      row.append(cell);
    }
    table.append(row);
  }
  root.append(table);
}

/** Renders audited controlled-live state without exposing credentials or order controls. */
export function renderLiveMonitoringDashboard(root: HTMLElement, dashboard: LiveMonitoringDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Controlled-live monitoring dashboard";
  root.append(heading);

  const boundary = document.createElement("p");
  boundary.textContent = `Environment: LIVE / ${dashboard.mode}. Monitoring only; no credential, approval, or order control is available here.`;
  root.append(boundary);

  const summary = document.createElement("dl");
  const values: ReadonlyArray<readonly [string, string]> = [
    ["Account", dashboard.account_id],
    ["Configuration", dashboard.configuration_fingerprint],
    ["Audit journal", dashboard.audit_healthy ? `Healthy (sequence ${dashboard.audit_sequence})` : "FAILED — controlled operations halted"],
    ["Audit head", dashboard.audit_head_hash],
    ["Broker session", dashboard.broker_connected ? "Connected" : "Disconnected / reconnect required"],
    ["Internal cash", dashboard.internal_cash],
    ["Working orders", String(dashboard.working_orders)],
    ["Unknown orders", String(dashboard.unknown_orders)],
    ["Unresolved discrepancies", String(dashboard.unresolved_incidents)],
    ["Last reconciliation", dashboard.last_reconciled_at ?? "Not yet reconciled"],
    ["Last reconciliation result", dashboard.last_reconciliation_clean === null
      ? "Not yet reconciled"
      : dashboard.last_reconciliation_clean ? "Clean" : "Discrepancy detected"],
    ["Controlled-live-day gate", `${dashboard.clean_live_days}/${dashboard.required_live_days}`],
    ["Promotion status", dashboard.promotion_eligible ? "Evidence gate complete" : "Evidence gate incomplete"],
  ];
  for (const [label, value] of values) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value;
    summary.append(term, detail);
  }
  root.append(summary);

  const killSwitches = document.createElement("p");
  killSwitches.textContent = dashboard.active_kill_switches.length === 0
    ? "Kill switches: clear"
    : `Kill switches: ${dashboard.active_kill_switches.join(", ")}`;
  root.append(killSwitches);

  const positionsHeading = document.createElement("h2");
  positionsHeading.textContent = "Internal positions";
  root.append(positionsHeading);
  if (dashboard.positions.length === 0) {
    const empty = document.createElement("p");
    empty.textContent = "No internal controlled-live positions.";
    root.append(empty);
    return;
  }
  const table = document.createElement("table");
  const header = document.createElement("tr");
  for (const label of ["Instrument", "Quantity", "Average cost", "Realized P&L"]) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = label;
    header.append(cell);
  }
  table.append(header);
  for (const position of dashboard.positions) {
    const row = document.createElement("tr");
    for (const value of [position.instrument_id, position.quantity, position.average_cost, position.realized_pnl]) {
      const cell = document.createElement("td");
      cell.textContent = value;
      row.append(cell);
    }
    table.append(row);
  }
  root.append(table);
}

function latestPortfolio(events: readonly EvidenceEvent[]): Record<string, unknown> | undefined {
  return [...events].reverse().find((event) => event.event_type === "portfolio.pnl_updated.v1")?.payload;
}

function isEvidenceEvent(value: unknown): value is EvidenceEvent {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.event_id === "string" &&
    typeof candidate.event_type === "string" &&
    /^([a-z]+\.)+[a-z_]+\.v[1-9][0-9]*$/.test(candidate.event_type) &&
    typeof candidate.event_time === "string" &&
    typeof candidate.correlation_id === "string" &&
    (candidate.causation_id === null || typeof candidate.causation_id === "string") &&
    typeof candidate.actor === "string" &&
    typeof candidate.source === "string" &&
    candidate.payload !== null &&
    typeof candidate.payload === "object" &&
    !Array.isArray(candidate.payload)
  );
}

function isPaperDashboard(value: unknown): value is PaperDashboard {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const expected = new Set([
    "dashboard_schema_version", "environment", "account_id", "configuration_fingerprint", "persistence_healthy", "internal_cash", "working_orders",
    "unknown_orders", "active_kill_switches", "unexplained_incidents", "last_reconciled_at", "last_reconciliation_clean", "clean_paper_days",
    "required_paper_days", "promotion_eligible", "positions",
  ]);
  if (Object.keys(candidate).length !== expected.size || Object.keys(candidate).some((key) => !expected.has(key))) {
    return false;
  }
  return (
    candidate.dashboard_schema_version === 1 &&
    candidate.environment === "PAPER" &&
    isCanonicalId(candidate.account_id) &&
    typeof candidate.configuration_fingerprint === "string" && /^[a-f0-9]{64}$/.test(candidate.configuration_fingerprint) &&
    typeof candidate.persistence_healthy === "boolean" &&
    isDecimal(candidate.internal_cash) &&
    isNonNegativeInteger(candidate.working_orders) &&
    isNonNegativeInteger(candidate.unknown_orders) &&
    Array.isArray(candidate.active_kill_switches) &&
    candidate.active_kill_switches.every((value) => typeof value === "string" && value.length > 0) &&
    new Set(candidate.active_kill_switches).size === candidate.active_kill_switches.length &&
    isNonNegativeInteger(candidate.unexplained_incidents) &&
    (candidate.last_reconciled_at === null || isUtcTimestamp(candidate.last_reconciled_at)) &&
    (candidate.last_reconciliation_clean === null || typeof candidate.last_reconciliation_clean === "boolean") &&
    (candidate.last_reconciled_at === null) === (candidate.last_reconciliation_clean === null) &&
    isNonNegativeInteger(candidate.clean_paper_days) &&
    candidate.required_paper_days === 30 &&
    typeof candidate.promotion_eligible === "boolean" &&
    Array.isArray(candidate.positions) &&
    candidate.positions.every(isPaperDashboardPosition)
  );
}

function isLiveMonitoringDashboard(value: unknown): value is LiveMonitoringDashboard {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const expected = new Set([
    "dashboard_schema_version", "environment", "mode", "account_id", "configuration_fingerprint", "broker_connected", "audit_healthy",
    "audit_sequence", "audit_head_hash", "active_kill_switches", "working_orders", "unknown_orders", "unresolved_incidents",
    "last_reconciled_at", "last_reconciliation_clean", "clean_live_days", "required_live_days", "promotion_eligible", "internal_cash", "positions",
  ]);
  if (Object.keys(candidate).length !== expected.size || Object.keys(candidate).some((key) => !expected.has(key))) {
    return false;
  }
  return (
    candidate.dashboard_schema_version === 1 &&
    candidate.environment === "LIVE" &&
    (candidate.mode === "SHADOW" || candidate.mode === "CANARY") &&
    isCanonicalId(candidate.account_id) &&
    isHash(candidate.configuration_fingerprint) &&
    typeof candidate.broker_connected === "boolean" &&
    typeof candidate.audit_healthy === "boolean" &&
    isPositiveInteger(candidate.audit_sequence) &&
    isHash(candidate.audit_head_hash) &&
    Array.isArray(candidate.active_kill_switches) &&
    candidate.active_kill_switches.every((entry) => typeof entry === "string" && entry.length > 0) &&
    new Set(candidate.active_kill_switches).size === candidate.active_kill_switches.length &&
    isNonNegativeInteger(candidate.working_orders) &&
    isNonNegativeInteger(candidate.unknown_orders) &&
    isNonNegativeInteger(candidate.unresolved_incidents) &&
    (candidate.last_reconciled_at === null || isUtcTimestamp(candidate.last_reconciled_at)) &&
    (candidate.last_reconciliation_clean === null || typeof candidate.last_reconciliation_clean === "boolean") &&
    (candidate.last_reconciled_at === null) === (candidate.last_reconciliation_clean === null) &&
    isNonNegativeInteger(candidate.clean_live_days) &&
    candidate.required_live_days === 60 &&
    typeof candidate.promotion_eligible === "boolean" &&
    isDecimal(candidate.internal_cash) &&
    Array.isArray(candidate.positions) &&
    candidate.positions.every(isPaperDashboardPosition)
  );
}

function isPaperDashboardPosition(value: unknown): value is PaperDashboardPosition {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    Object.keys(candidate).length === 4 &&
    isCanonicalId(candidate.instrument_id) &&
    isDecimal(candidate.quantity) &&
    isDecimal(candidate.average_cost) &&
    isDecimal(candidate.realized_pnl)
  );
}

function isCanonicalId(value: unknown): value is string {
  return typeof value === "string" && /^[a-z0-9._-]+$/.test(value);
}

function isDecimal(value: unknown): value is string {
  return typeof value === "string" && /^-?[0-9]+(?:\.[0-9]{1,8})?$/.test(value);
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isPositiveInteger(value: unknown): value is number {
  return isNonNegativeInteger(value) && value > 0;
}

function isHash(value: unknown): value is string {
  return typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
}

function isUtcTimestamp(value: unknown): value is string {
  return typeof value === "string" && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value);
}
