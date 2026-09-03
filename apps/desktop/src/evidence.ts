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
  dashboard_schema_version: 2;
  environment: "PAPER";
  account_id: string;
  configuration_fingerprint: string;
  broker_connected: boolean;
  persistence_healthy: boolean;
  audit_sequence: number;
  audit_head_hash: string;
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
  complete_auditability: boolean;
  positions: readonly PaperDashboardPosition[];
}>;

export type PaperDashboardPosition = Readonly<{
  instrument_id: string;
  quantity: string;
  average_cost: string;
  realized_pnl: string;
}>;

export type LiveMonitoringDashboard = Readonly<{
  dashboard_schema_version: 2;
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
  complete_auditability: boolean;
  internal_cash: string;
  positions: readonly PaperDashboardPosition[];
}>;

export type OperationsDashboard = Readonly<{
  dashboard_schema_version: 1;
  environment: "SIMULATION" | "PAPER" | "LIVE";
  as_of: string;
  account_id: string;
  currency: string;
  starting_equity: string;
  configuration: OperationsConfiguration;
  reproducibility: ReproducibilityEvidence;
  risk: OperationsRisk;
  operational_health: OperationsHealth;
  attribution: OperationsAttribution;
  alerts: readonly OperationsAlert[];
  schedules: readonly OperationsSchedule[];
  journal: OperationsJournal;
  positions: readonly OperationsPosition[];
  projection_fingerprint: string;
}>;

export type OperationsConfiguration = Readonly<{
  configuration_content_hash: string;
  configuration_id: string;
  configuration_version: string;
  fingerprint: string;
  parameter_set_fingerprint: string;
}>;

export type ReproducibilityEvidence = Readonly<{
  strategy_id: string;
  strategy_version: string;
  strategy_bundle_hash: string;
  dataset_id: string;
  dataset_version: string;
  dataset_hash: string;
  replay_event_hash: string;
}>;

export type OperationsRisk = Readonly<{
  state: "NORMAL" | "WARNING" | "CRITICAL";
  cash: string;
  current_equity: string;
  effective_peak_equity: string;
  gross_exposure: string;
  largest_position_exposure: string;
  drawdown_bps: string;
  open_positions: number;
  limits: readonly OperationsRiskLimit[];
}>;

export type OperationsRiskLimit = Readonly<{
  limit_id: string;
  current: string;
  limit: string;
  breached: boolean;
}>;

export type OperationsHealth = Readonly<{
  audit_healthy: boolean;
  reconciliation_healthy: boolean;
  broker_connected: boolean;
  active_kill_switches: readonly string[];
  working_orders: number;
  unknown_orders: number;
  unresolved_incidents: number;
}>;

export type OperationsAttribution = Readonly<{
  net_pnl: string;
  totals: Readonly<Record<AttributionCategory, string>>;
  rows: readonly OperationsAttributionRow[];
}>;

type AttributionCategory = "REALIZED_PNL" | "UNREALIZED_PNL" | "FEES" | "DIVIDENDS" | "CORPORATE_ACTIONS";

export type OperationsAttributionRow = Readonly<{
  instrument_id: string;
  category: AttributionCategory;
  amount: string;
}>;

export type OperationsAlert = Readonly<{
  alert_id: string;
  severity: "WARNING" | "CRITICAL";
  code: string;
  subject: string;
  summary: string;
}>;

export type OperationsSchedule = Readonly<{
  schedule_id: string;
  purpose: string;
  time_utc: string;
  enabled: boolean;
  next_due_at: string;
  due: boolean;
  last_completed_at: string | null;
}>;

export type OperationsJournal = Readonly<{
  healthy: boolean;
  sequence: number;
  head_hash: string;
  failure_reason: string | null;
}>;

export type OperationsPosition = Readonly<{
  instrument_id: string;
  quantity: string;
  mark_price: string;
  average_cost: string;
  realized_pnl: string;
}>;

export type OptionsDashboard = Readonly<{
  option_dashboard_schema_version: 1;
  as_of: string;
  configuration_file_hash: string;
  model_version: "follon-european-black-scholes-fixed-v1";
  chain: OptionsChainEvidence;
  run_identity: OptionsRunIdentity;
  analytics: readonly OptionAnalyticsEvidence[];
  strategy: OptionsStrategyEvidence;
  reconciliation: OptionsReconciliationEvidence;
}>;

export type OptionsChainEvidence = Readonly<{
  chain_id: string;
  chain_snapshot_hash: string;
  currency: string;
  reference_version: string;
  underlying_instrument_id: string;
  underlying_mark: string;
}>;

export type OptionsRunIdentity = Readonly<{
  strategy_bundle_hash: string;
  configuration_hash: string;
  dataset_hash: string;
  replay_event_hash: string;
  chain_snapshot_hash: string;
  model_version: "follon-european-black-scholes-fixed-v1";
}>;

export type OptionAnalyticsEvidence = Readonly<{
  option_id: string;
  expiration_at: string;
  right: "CALL" | "PUT";
  strike: string;
  bid: string;
  ask: string;
  market_premium: string;
  implied_volatility: string;
  model_price: string;
  delta: string;
  gamma: string;
  vega: string;
  theta: string;
  rho: string;
}>;

export type OptionsStrategyEvidence = Readonly<{
  strategy_id: string;
  strategy_version: string;
  scenarios: readonly OptionExpiryScenario[];
}>;

export type OptionExpiryScenario = Readonly<{
  underlying_price: string;
  total_pnl: string;
  legs: readonly OptionScenarioLeg[];
}>;

export type OptionScenarioLeg = Readonly<{
  leg_id: string;
  option_id: string;
  intrinsic_value: string;
  pnl: string;
}>;

export type OptionsReconciliationEvidence = Readonly<{
  backtest_book: OptionBookEvidence;
  clean: boolean;
  reconciled_at: string;
  paper_book: OptionBookEvidence;
  live_book: OptionBookEvidence;
  issues: readonly OptionReconciliationIssue[];
}>;

export type OptionBookEvidence = Readonly<{
  account_id: string;
  book_hash: string;
  environment: "BACKTEST" | "PAPER" | "LIVE";
  run_identity: OptionsRunIdentity;
  run_identity_hash: string;
  source_export_hash: string;
  source_export_id: string;
}>;

export type OptionReconciliationIssue = Readonly<{
  category: "IDENTITY_MISMATCH" | "CASH_MISMATCH" | "POSITION_MISMATCH";
  subject: string;
  expected: string;
  observed: string;
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

/** Parses a server-owned PAPER operations evidence snapshot. */
export function parsePaperDashboard(json: string): PaperDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Paper dashboard is not valid JSON.");
  }
  if (!isPaperDashboard(value)) {
    throw new Error("Paper dashboard does not match the v2 evidence contract.");
  }
  return value;
}

/** Parses a controlled-live monitoring evidence projection. */
export function parseLiveMonitoringDashboard(json: string): LiveMonitoringDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Controlled-live monitoring dashboard is not valid JSON.");
  }
  if (!isLiveMonitoringDashboard(value)) {
    throw new Error("Controlled-live monitoring dashboard does not match the v2 evidence contract.");
  }
  return value;
}

/** Parses the versioned operator-workbench evidence dashboard. */
export function parseOperationsDashboard(json: string): OperationsDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Operations dashboard is not valid JSON.");
  }
  if (!isOperationsDashboard(value)) {
    throw new Error("Operations dashboard does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a frozen option-chain analytics and cross-environment evidence snapshot. */
export function parseOptionsDashboard(json: string): OptionsDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Options dashboard is not valid JSON.");
  }
  if (!isOptionsDashboard(value)) {
    throw new Error("Options dashboard does not match the v1 deterministic European-options contract.");
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
  environment.textContent = "SIMULATION — no broker connectivity";
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
    item.append(phase, document.createTextNode(` — ${event.event_time}`));

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

/** Renders PAPER risk and reconciliation evidence. */
export function renderPaperDashboard(root: HTMLElement, dashboard: PaperDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Paper operations dashboard";
  root.append(heading);

  const environment = document.createElement("p");
  environment.textContent = `Environment: ${dashboard.environment}. Use the Order Ticket for active requests.`;
  root.append(environment);

  const summary = document.createElement("dl");
  const values: ReadonlyArray<readonly [string, string]> = [
    ["Account", dashboard.account_id],
    ["Configuration", dashboard.configuration_fingerprint],
    ["Broker session", dashboard.broker_connected ? "Connected" : "Disconnected / reconnect required"],
    ["Audit sequence", String(dashboard.audit_sequence)],
    ["Audit head", dashboard.audit_head_hash],
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
    ["Complete auditability", dashboard.complete_auditability ? "Yes" : "No"],
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
    ["Complete auditability", dashboard.complete_auditability ? "Yes" : "No"],
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

/** Renders risk, attribution, schedules, replay identities, and journal evidence. */
export function renderOperationsDashboard(root: HTMLElement, dashboard: OperationsDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Operations workbench";
  root.append(heading);

  const boundary = document.createElement("p");
  boundary.textContent = `Environment: ${dashboard.environment}. This is an evidence view; use the active controls in Command Center or Execution Blotter to submit a request.`;
  root.append(boundary);

  const summary = document.createElement("dl");
  appendDefinitionList(summary, [
    ["As of", dashboard.as_of],
    ["Account", dashboard.account_id],
    ["Risk state", dashboard.risk.state],
    ["Current equity", `${dashboard.risk.current_equity} ${dashboard.currency}`],
    ["Gross exposure", `${dashboard.risk.gross_exposure} ${dashboard.currency}`],
    ["Drawdown", `${dashboard.risk.drawdown_bps} bps`],
    ["Open positions", String(dashboard.risk.open_positions)],
    ["Audit health", dashboard.operational_health.audit_healthy ? "Healthy" : "FAILED"],
    ["Reconciliation", dashboard.operational_health.reconciliation_healthy ? "Clean" : "Discrepancy detected"],
    ["Broker session", dashboard.operational_health.broker_connected ? "Connected" : "Disconnected"],
    ["Operations journal", dashboard.journal.healthy ? `Healthy (sequence ${dashboard.journal.sequence})` : "FAILED"],
  ]);
  root.append(summary);

  const alertsHeading = document.createElement("h2");
  alertsHeading.textContent = "Alerts";
  root.append(alertsHeading);
  if (dashboard.alerts.length === 0) {
    const clear = document.createElement("p");
    clear.textContent = "No active deterministic alerts.";
    root.append(clear);
  } else {
    const alerts = document.createElement("ul");
    for (const alert of dashboard.alerts) {
      const item = document.createElement("li");
      item.textContent = `${alert.severity}: ${alert.summary} (${alert.code} / ${alert.subject})`;
      alerts.append(item);
    }
    root.append(alerts);
  }

  const riskHeading = document.createElement("h2");
  riskHeading.textContent = "Risk limits";
  root.append(riskHeading);
  root.append(tableFromRows(
    ["Limit", "Current", "Limit", "Status"],
    dashboard.risk.limits.map((limit) => [
      limit.limit_id,
      limit.current,
      limit.limit,
      limit.breached ? "BREACHED" : "Within limit",
    ]),
  ));

  const attributionHeading = document.createElement("h2");
  attributionHeading.textContent = `Attribution — net ${dashboard.attribution.net_pnl} ${dashboard.currency}`;
  root.append(attributionHeading);
  root.append(tableFromRows(
    ["Instrument", "Category", "Amount"],
    dashboard.attribution.rows.map((row) => [row.instrument_id, row.category, `${row.amount} ${dashboard.currency}`]),
  ));

  const scheduleHeading = document.createElement("h2");
  scheduleHeading.textContent = "Schedule";
  root.append(scheduleHeading);
  root.append(tableFromRows(
    ["Schedule", "Next due", "State", "Purpose"],
    dashboard.schedules.map((schedule) => [
      schedule.schedule_id,
      schedule.next_due_at,
      !schedule.enabled ? "Disabled" : schedule.due ? "Due" : "Scheduled",
      schedule.purpose,
    ]),
  ));

  const replayHeading = document.createElement("h2");
  replayHeading.textContent = "Replay and configuration evidence";
  root.append(replayHeading);
  const replay = document.createElement("dl");
  appendDefinitionList(replay, [
    ["Configuration", `${dashboard.configuration.configuration_id} / ${dashboard.configuration.configuration_version}`],
    ["Configuration bytes", dashboard.configuration.configuration_content_hash],
    ["Parameter revision", dashboard.configuration.parameter_set_fingerprint],
    ["Strategy", `${dashboard.reproducibility.strategy_id} / ${dashboard.reproducibility.strategy_version}`],
    ["Strategy bundle", dashboard.reproducibility.strategy_bundle_hash],
    ["Dataset", `${dashboard.reproducibility.dataset_id} / ${dashboard.reproducibility.dataset_version}`],
    ["Dataset hash", dashboard.reproducibility.dataset_hash],
    ["Replay event hash", dashboard.reproducibility.replay_event_hash],
    ["Journal head", dashboard.journal.head_hash],
    ["Projection fingerprint", dashboard.projection_fingerprint],
  ]);
  root.append(replay);

  if (dashboard.journal.failure_reason !== null) {
    const failure = document.createElement("p");
    failure.textContent = `Journal verification failure: ${dashboard.journal.failure_reason}`;
    root.append(failure);
  }
}

/** Renders a frozen options chain and reconciliation evidence projection. */
export function renderOptionsDashboard(root: HTMLElement, dashboard: OptionsDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Options chain and scenario evidence";
  root.append(heading);

  const boundary = document.createElement("p");
  boundary.textContent = "Deterministic European-option analytics. Use the active trading controls for supported order-entry requests.";
  root.append(boundary);

  const summary = document.createElement("dl");
  appendDefinitionList(summary, [
    ["As of", dashboard.as_of],
    ["Chain", dashboard.chain.chain_id],
    ["Underlying", `${dashboard.chain.underlying_instrument_id} at ${dashboard.chain.underlying_mark} ${dashboard.chain.currency}`],
    ["Reference version", dashboard.chain.reference_version],
    ["Pricing model", dashboard.model_version],
    ["Reconciliation", dashboard.reconciliation.clean ? "CLEAN across BACKTEST, PAPER, and LIVE" : "DIFFERENCES FOUND"],
    ["Chain snapshot hash", dashboard.chain.chain_snapshot_hash],
  ]);
  root.append(summary);

  const reconciliationHeading = document.createElement("h2");
  reconciliationHeading.textContent = "Cross-environment reconciliation";
  root.append(reconciliationHeading);
  root.append(tableFromRows(
    ["Environment", "Account", "Source export", "Source hash", "Run identity", "Book hash"],
    [dashboard.reconciliation.backtest_book, dashboard.reconciliation.paper_book, dashboard.reconciliation.live_book].map((book) => [
      book.environment,
      book.account_id,
      book.source_export_id,
      book.source_export_hash,
      book.run_identity_hash,
      book.book_hash,
    ]),
  ));
  if (dashboard.reconciliation.issues.length === 0) {
    const clean = document.createElement("p");
    clean.textContent = "The bound BACKTEST, PAPER, and LIVE books agree on the compared economics and run identity. Their source exports remain independently fingerprinted.";
    root.append(clean);
  } else {
    root.append(tableFromRows(
      ["Category", "Subject", "Expected", "Observed"],
      dashboard.reconciliation.issues.map((issue) => [issue.category, issue.subject, issue.expected, issue.observed]),
    ));
  }

  const analyticsHeading = document.createElement("h2");
  analyticsHeading.textContent = "Implied volatility and Greeks";
  root.append(analyticsHeading);
  root.append(tableFromRows(
    ["Contract", "Right", "Strike", "Mid", "Implied vol", "Delta", "Gamma", "Vega", "Theta", "Rho"],
    dashboard.analytics.map((option) => [
      option.option_id,
      option.right,
      option.strike,
      option.market_premium,
      option.implied_volatility,
      option.delta,
      option.gamma,
      option.vega,
      option.theta,
      option.rho,
    ]),
  ));

  const scenariosHeading = document.createElement("h2");
  scenariosHeading.textContent = `Expiry scenarios — ${dashboard.strategy.strategy_id} / ${dashboard.strategy.strategy_version}`;
  root.append(scenariosHeading);
  root.append(tableFromRows(
    ["Underlying price", "Strategy P&L"],
    dashboard.strategy.scenarios.map((scenario) => [
      scenario.underlying_price,
      `${scenario.total_pnl} ${dashboard.chain.currency}`,
    ]),
  ));

  const provenanceHeading = document.createElement("h2");
  provenanceHeading.textContent = "Reproducibility evidence";
  root.append(provenanceHeading);
  const provenance = document.createElement("dl");
  appendDefinitionList(provenance, [
    ["Configuration file", dashboard.configuration_file_hash],
    ["Bound configuration", dashboard.run_identity.configuration_hash],
    ["Strategy bundle", dashboard.run_identity.strategy_bundle_hash],
    ["Dataset", dashboard.run_identity.dataset_hash],
    ["Replay event output", dashboard.run_identity.replay_event_hash],
    ["Reconciled at", dashboard.reconciliation.reconciled_at],
  ]);
  root.append(provenance);
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

function appendDefinitionList(target: HTMLDListElement, values: ReadonlyArray<readonly [string, string]>): void {
  for (const [label, value] of values) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value;
    target.append(term, detail);
  }
}

function tableFromRows(headers: readonly string[], rows: readonly (readonly string[])[]): HTMLTableElement {
  const table = document.createElement("table");
  const header = document.createElement("tr");
  for (const label of headers) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = label;
    header.append(cell);
  }
  table.append(header);
  for (const values of rows) {
    const row = document.createElement("tr");
    for (const value of values) {
      const cell = document.createElement("td");
      cell.textContent = value;
      row.append(cell);
    }
    table.append(row);
  }
  return table;
}

function isPaperDashboard(value: unknown): value is PaperDashboard {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const expected = new Set([
    "dashboard_schema_version", "environment", "account_id", "configuration_fingerprint", "broker_connected", "persistence_healthy", "audit_sequence", "audit_head_hash", "internal_cash", "working_orders",
    "unknown_orders", "active_kill_switches", "unexplained_incidents", "last_reconciled_at", "last_reconciliation_clean", "clean_paper_days",
    "required_paper_days", "promotion_eligible", "complete_auditability", "positions",
  ]);
  if (Object.keys(candidate).length !== expected.size || Object.keys(candidate).some((key) => !expected.has(key))) {
    return false;
  }
  return (
    candidate.dashboard_schema_version === 2 &&
    candidate.environment === "PAPER" &&
    isCanonicalId(candidate.account_id) &&
    typeof candidate.configuration_fingerprint === "string" && /^[a-f0-9]{64}$/.test(candidate.configuration_fingerprint) &&
    typeof candidate.broker_connected === "boolean" &&
    typeof candidate.persistence_healthy === "boolean" &&
    isNonNegativeInteger(candidate.audit_sequence) &&
    isHash(candidate.audit_head_hash) &&
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
    typeof candidate.complete_auditability === "boolean" &&
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
    "last_reconciled_at", "last_reconciliation_clean", "clean_live_days", "required_live_days", "promotion_eligible", "complete_auditability", "internal_cash", "positions",
  ]);
  if (Object.keys(candidate).length !== expected.size || Object.keys(candidate).some((key) => !expected.has(key))) {
    return false;
  }
  return (
    candidate.dashboard_schema_version === 2 &&
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
    typeof candidate.complete_auditability === "boolean" &&
    isDecimal(candidate.internal_cash) &&
    Array.isArray(candidate.positions) &&
    candidate.positions.every(isPaperDashboardPosition)
  );
}

function isOperationsDashboard(value: unknown): value is OperationsDashboard {
  if (!hasExactKeys(value, [
    "account_id", "alerts", "as_of", "attribution", "configuration", "currency", "dashboard_schema_version", "environment", "journal", "operational_health", "positions", "projection_fingerprint", "reproducibility", "risk", "schedules", "starting_equity",
  ])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    candidate.dashboard_schema_version === 1 &&
    (candidate.environment === "SIMULATION" || candidate.environment === "PAPER" || candidate.environment === "LIVE") &&
    isUtcTimestamp(candidate.as_of) &&
    isCanonicalId(candidate.account_id) &&
    typeof candidate.currency === "string" && /^[A-Z]{3}$/.test(candidate.currency) &&
    isDecimal(candidate.starting_equity) &&
    isOperationsConfiguration(candidate.configuration) &&
    isReproducibilityEvidence(candidate.reproducibility) &&
    isOperationsRisk(candidate.risk) &&
    isOperationsHealth(candidate.operational_health) &&
    isOperationsAttribution(candidate.attribution) &&
    Array.isArray(candidate.alerts) && candidate.alerts.every(isOperationsAlert) &&
    Array.isArray(candidate.schedules) && candidate.schedules.every(isOperationsSchedule) &&
    Array.isArray(candidate.positions) && candidate.positions.every(isOperationsPosition) &&
    isOperationsJournal(candidate.journal) && isHash(candidate.projection_fingerprint)
  );
}

function isOperationsConfiguration(value: unknown): value is OperationsConfiguration {
  if (!hasExactKeys(value, ["configuration_content_hash", "configuration_id", "configuration_version", "fingerprint", "parameter_set_fingerprint"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isHash(candidate.configuration_content_hash) && isCanonicalId(candidate.configuration_id) &&
    typeof candidate.configuration_version === "string" && candidate.configuration_version.length > 0 &&
    isHash(candidate.fingerprint) && isHash(candidate.parameter_set_fingerprint);
}

function isReproducibilityEvidence(value: unknown): value is ReproducibilityEvidence {
  if (!hasExactKeys(value, ["dataset_hash", "dataset_id", "dataset_version", "replay_event_hash", "strategy_bundle_hash", "strategy_id", "strategy_version"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isHash(candidate.dataset_hash) && isCanonicalId(candidate.dataset_id) &&
    typeof candidate.dataset_version === "string" && candidate.dataset_version.length > 0 &&
    isHash(candidate.replay_event_hash) && isHash(candidate.strategy_bundle_hash) &&
    isCanonicalId(candidate.strategy_id) && typeof candidate.strategy_version === "string" && candidate.strategy_version.length > 0;
}

function isOperationsRisk(value: unknown): value is OperationsRisk {
  if (!hasExactKeys(value, ["cash", "current_equity", "drawdown_bps", "effective_peak_equity", "gross_exposure", "largest_position_exposure", "limits", "open_positions", "state"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const expectedLimitIds = new Set(["gross_exposure", "single_instrument_exposure", "drawdown_bps", "working_orders", "unknown_orders", "unresolved_incidents"]);
  return (
    (candidate.state === "NORMAL" || candidate.state === "WARNING" || candidate.state === "CRITICAL") &&
    isDecimal(candidate.cash) && isDecimal(candidate.current_equity) && isDecimal(candidate.effective_peak_equity) &&
    isDecimal(candidate.gross_exposure) && isDecimal(candidate.largest_position_exposure) && isDecimal(candidate.drawdown_bps) &&
    isNonNegativeInteger(candidate.open_positions) && Array.isArray(candidate.limits) &&
    candidate.limits.length === expectedLimitIds.size && candidate.limits.every(isOperationsRiskLimit) &&
    new Set(candidate.limits.map((limit) => limit.limit_id)).size === expectedLimitIds.size &&
    candidate.limits.every((limit) => expectedLimitIds.has(limit.limit_id))
  );
}

function isOperationsRiskLimit(value: unknown): value is OperationsRiskLimit {
  if (!hasExactKeys(value, ["breached", "current", "limit", "limit_id"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.breached === "boolean" && isDecimal(candidate.current) && isDecimal(candidate.limit) && isCanonicalId(candidate.limit_id);
}

function isOperationsHealth(value: unknown): value is OperationsHealth {
  if (!hasExactKeys(value, ["active_kill_switches", "audit_healthy", "broker_connected", "reconciliation_healthy", "unknown_orders", "unresolved_incidents", "working_orders"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.audit_healthy === "boolean" && typeof candidate.reconciliation_healthy === "boolean" &&
    typeof candidate.broker_connected === "boolean" && Array.isArray(candidate.active_kill_switches) &&
    candidate.active_kill_switches.every((scope) => typeof scope === "string" && scope.length > 0) &&
    new Set(candidate.active_kill_switches).size === candidate.active_kill_switches.length &&
    isNonNegativeInteger(candidate.working_orders) && isNonNegativeInteger(candidate.unknown_orders) && isNonNegativeInteger(candidate.unresolved_incidents);
}

function isOperationsAttribution(value: unknown): value is OperationsAttribution {
  if (!hasExactKeys(value, ["net_pnl", "rows", "totals"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.net_pnl) && Array.isArray(candidate.rows) && candidate.rows.every(isOperationsAttributionRow) && isAttributionTotals(candidate.totals);
}

function isAttributionTotals(value: unknown): value is Readonly<Record<AttributionCategory, string>> {
  const categories: AttributionCategory[] = ["REALIZED_PNL", "UNREALIZED_PNL", "FEES", "DIVIDENDS", "CORPORATE_ACTIONS"];
  return hasExactKeys(value, categories) && categories.every((category) => isDecimal((value as Record<string, unknown>)[category]));
}

function isOperationsAttributionRow(value: unknown): value is OperationsAttributionRow {
  if (!hasExactKeys(value, ["amount", "category", "instrument_id"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.amount) && isCanonicalId(candidate.instrument_id) && isAttributionCategory(candidate.category);
}

function isAttributionCategory(value: unknown): value is AttributionCategory {
  return value === "REALIZED_PNL" || value === "UNREALIZED_PNL" || value === "FEES" || value === "DIVIDENDS" || value === "CORPORATE_ACTIONS";
}

function isOperationsAlert(value: unknown): value is OperationsAlert {
  if (!hasExactKeys(value, ["alert_id", "code", "severity", "subject", "summary"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isHash(candidate.alert_id) && isCanonicalId(candidate.code) &&
    (candidate.severity === "WARNING" || candidate.severity === "CRITICAL") &&
    typeof candidate.subject === "string" && candidate.subject.length > 0 &&
    typeof candidate.summary === "string" && candidate.summary.length > 0;
}

function isOperationsSchedule(value: unknown): value is OperationsSchedule {
  if (!hasExactKeys(value, ["due", "enabled", "last_completed_at", "next_due_at", "purpose", "schedule_id", "time_utc"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.due === "boolean" && typeof candidate.enabled === "boolean" &&
    (candidate.last_completed_at === null || isUtcTimestamp(candidate.last_completed_at)) &&
    isUtcTimestamp(candidate.next_due_at) && typeof candidate.purpose === "string" && candidate.purpose.length > 0 &&
    isCanonicalId(candidate.schedule_id) && typeof candidate.time_utc === "string" && /^([01][0-9]|2[0-3]):[0-5][0-9]$/.test(candidate.time_utc);
}

function isOperationsJournal(value: unknown): value is OperationsJournal {
  if (!hasExactKeys(value, ["failure_reason", "head_hash", "healthy", "sequence"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.healthy !== "boolean" || !isNonNegativeInteger(candidate.sequence) || !isHash(candidate.head_hash)) {
    return false;
  }
  return candidate.healthy
    ? candidate.failure_reason === null
    : typeof candidate.failure_reason === "string" && candidate.failure_reason.length > 0 &&
      candidate.head_hash === "0000000000000000000000000000000000000000000000000000000000000000";
}

function isOperationsPosition(value: unknown): value is OperationsPosition {
  if (!hasExactKeys(value, ["average_cost", "instrument_id", "mark_price", "quantity", "realized_pnl"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isCanonicalId(candidate.instrument_id) && isDecimal(candidate.quantity) && isDecimal(candidate.mark_price) &&
    isDecimal(candidate.average_cost) && isDecimal(candidate.realized_pnl);
}

function isOptionsDashboard(value: unknown): value is OptionsDashboard {
  if (!hasExactKeys(value, ["analytics", "as_of", "chain", "configuration_file_hash", "model_version", "option_dashboard_schema_version", "reconciliation", "run_identity", "strategy"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return candidate.option_dashboard_schema_version === 1 && isUtcTimestamp(candidate.as_of) && isHash(candidate.configuration_file_hash) &&
    candidate.model_version === "follon-european-black-scholes-fixed-v1" && isOptionsChainEvidence(candidate.chain) &&
    isOptionsRunIdentity(candidate.run_identity) && Array.isArray(candidate.analytics) && candidate.analytics.length > 0 &&
    candidate.analytics.every(isOptionAnalyticsEvidence) && new Set(candidate.analytics.map((option) => option.option_id)).size === candidate.analytics.length &&
    candidate.run_identity.chain_snapshot_hash === candidate.chain.chain_snapshot_hash &&
    candidate.run_identity.model_version === candidate.model_version &&
    isOptionsStrategyEvidence(candidate.strategy) && isOptionsReconciliationEvidence(
      candidate.reconciliation,
      candidate.as_of,
      candidate.chain.chain_snapshot_hash,
      candidate.model_version,
      candidate.run_identity,
    ) &&
    (candidate.reconciliation.clean === (candidate.reconciliation.issues.length === 0));
}

function isOptionsChainEvidence(value: unknown): value is OptionsChainEvidence {
  if (!hasExactKeys(value, ["chain_id", "chain_snapshot_hash", "currency", "reference_version", "underlying_instrument_id", "underlying_mark"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isCanonicalId(candidate.chain_id) && isHash(candidate.chain_snapshot_hash) && typeof candidate.currency === "string" &&
    /^[A-Z]{3}$/.test(candidate.currency) && isCanonicalId(candidate.reference_version) &&
    isCanonicalId(candidate.underlying_instrument_id) && isDecimal(candidate.underlying_mark);
}

function isOptionsRunIdentity(value: unknown): value is OptionsRunIdentity {
  if (!hasExactKeys(value, ["chain_snapshot_hash", "configuration_hash", "dataset_hash", "model_version", "replay_event_hash", "strategy_bundle_hash"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isHash(candidate.chain_snapshot_hash) && isHash(candidate.configuration_hash) && isHash(candidate.dataset_hash) &&
    candidate.model_version === "follon-european-black-scholes-fixed-v1" && isHash(candidate.replay_event_hash) && isHash(candidate.strategy_bundle_hash);
}

function isOptionAnalyticsEvidence(value: unknown): value is OptionAnalyticsEvidence {
  if (!hasExactKeys(value, ["ask", "bid", "delta", "expiration_at", "gamma", "implied_volatility", "market_premium", "model_price", "option_id", "rho", "right", "strike", "theta", "vega"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.ask) && isDecimal(candidate.bid) && isDecimal(candidate.delta) && isUtcTimestamp(candidate.expiration_at) &&
    isDecimal(candidate.gamma) && isDecimal(candidate.implied_volatility) && isDecimal(candidate.market_premium) && isDecimal(candidate.model_price) &&
    isCanonicalId(candidate.option_id) && isDecimal(candidate.rho) && (candidate.right === "CALL" || candidate.right === "PUT") &&
    isDecimal(candidate.strike) && isDecimal(candidate.theta) && isDecimal(candidate.vega);
}

function isOptionsStrategyEvidence(value: unknown): value is OptionsStrategyEvidence {
  if (!hasExactKeys(value, ["scenarios", "strategy_id", "strategy_version"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isCanonicalId(candidate.strategy_id) && typeof candidate.strategy_version === "string" && candidate.strategy_version.length > 0 &&
    Array.isArray(candidate.scenarios) && candidate.scenarios.length > 0 && candidate.scenarios.every(isOptionExpiryScenario);
}

function isOptionExpiryScenario(value: unknown): value is OptionExpiryScenario {
  if (!hasExactKeys(value, ["legs", "total_pnl", "underlying_price"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.total_pnl) && isDecimal(candidate.underlying_price) && Array.isArray(candidate.legs) && candidate.legs.length > 0 && candidate.legs.every(isOptionScenarioLeg);
}

function isOptionScenarioLeg(value: unknown): value is OptionScenarioLeg {
  if (!hasExactKeys(value, ["intrinsic_value", "leg_id", "option_id", "pnl"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.intrinsic_value) && isCanonicalId(candidate.leg_id) && isCanonicalId(candidate.option_id) && isDecimal(candidate.pnl);
}

function isOptionsReconciliationEvidence(
  value: unknown,
  asOf: string,
  chainSnapshotHash: string,
  modelVersion: OptionsDashboard["model_version"],
  referenceIdentity: OptionsRunIdentity,
): value is OptionsReconciliationEvidence {
  if (!hasExactKeys(value, ["backtest_book", "clean", "issues", "live_book", "paper_book", "reconciled_at"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isOptionBookEvidence(candidate.backtest_book, "BACKTEST", chainSnapshotHash, modelVersion) && typeof candidate.clean === "boolean" &&
    isOptionBookEvidence(candidate.paper_book, "PAPER", chainSnapshotHash, modelVersion) &&
    isOptionBookEvidence(candidate.live_book, "LIVE", chainSnapshotHash, modelVersion) &&
    isUtcTimestamp(candidate.reconciled_at) && candidate.reconciled_at >= asOf &&
    sameRunIdentity(candidate.backtest_book.run_identity, referenceIdentity) &&
    (!candidate.clean || (
      candidate.backtest_book.run_identity_hash === candidate.paper_book.run_identity_hash &&
      candidate.paper_book.run_identity_hash === candidate.live_book.run_identity_hash
    )) &&
    Array.isArray(candidate.issues) && candidate.issues.every(isOptionReconciliationIssue);
}

function isOptionBookEvidence(
  value: unknown,
  expectedEnvironment: OptionBookEvidence["environment"],
  chainSnapshotHash: string,
  modelVersion: OptionsDashboard["model_version"],
): value is OptionBookEvidence {
  if (!hasExactKeys(value, ["account_id", "book_hash", "environment", "run_identity", "run_identity_hash", "source_export_hash", "source_export_id"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isCanonicalId(candidate.account_id) && isHash(candidate.book_hash) && candidate.environment === expectedEnvironment &&
    isOptionsRunIdentity(candidate.run_identity) && candidate.run_identity.chain_snapshot_hash === chainSnapshotHash &&
    candidate.run_identity.model_version === modelVersion && isHash(candidate.run_identity_hash) &&
    isHash(candidate.source_export_hash) && isCanonicalId(candidate.source_export_id);
}

function sameRunIdentity(left: OptionsRunIdentity, right: OptionsRunIdentity): boolean {
  return left.chain_snapshot_hash === right.chain_snapshot_hash &&
    left.configuration_hash === right.configuration_hash &&
    left.dataset_hash === right.dataset_hash &&
    left.model_version === right.model_version &&
    left.replay_event_hash === right.replay_event_hash &&
    left.strategy_bundle_hash === right.strategy_bundle_hash;
}

function isOptionReconciliationIssue(value: unknown): value is OptionReconciliationIssue {
  if (!hasExactKeys(value, ["category", "expected", "observed", "subject"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (candidate.category === "IDENTITY_MISMATCH" || candidate.category === "CASH_MISMATCH" || candidate.category === "POSITION_MISMATCH") &&
    typeof candidate.subject === "string" && candidate.subject.length > 0 && typeof candidate.expected === "string" && typeof candidate.observed === "string";
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

function hasExactKeys(value: unknown, expected: readonly string[]): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const keys = Object.keys(value);
  return keys.length === expected.length && keys.every((key) => expected.includes(key));
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
