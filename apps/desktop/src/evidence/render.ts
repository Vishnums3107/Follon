// DOM renderers for server-owned evidence; they never create a trading transition.

import type { EvidenceEvent } from "./core.js";
import type { LiveMonitoringDashboard, OperationsDashboard, OptionsDashboard, PaperDashboard } from "./operations.js";

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
