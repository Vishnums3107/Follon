import {
  LiveMonitoringDashboard,
  OperationsDashboard,
  OptionsDashboard,
  PaperDashboard,
  parseLiveMonitoringDashboard,
  parseOperationsDashboard,
  parseOptionsDashboard,
  parsePaperDashboard,
} from "./evidence.js";
import { FeatureDefinition, SystemStatus } from "./catalog.js";

export type EvidenceArtifact = Readonly<{
  name: string;
  bytes: number;
  modified_at: string;
  feature: string;
  kind: string;
  format: "ndjson" | "json" | "markdown" | "csv" | "text";
}>;

type SnapshotRecord = Readonly<{
  artifact: string;
  feature?: string;
  category?: string;
  modified_at?: string;
  data: Readonly<Record<string, unknown>>;
}>;

type DatasetSummary = Readonly<{
  name: string;
  modified_at: string;
  bytes: number;
  columns: readonly string[];
  rows: number;
  dataset_id?: string;
  dataset_version?: string;
  storage_format?: string;
  content_sha256?: string;
}>;

type BacktestSummary = Readonly<{
  artifact: string;
  modified_at: string;
  artifact_fingerprint: unknown;
  event_output_hash: unknown;
  performance: Readonly<Record<string, unknown>>;
  report: Readonly<Record<string, unknown>>;
  specification: Readonly<Record<string, unknown>>;
  specification_fingerprint: unknown;
}>;

type SnapshotDashboard = Readonly<{
  artifact: string;
  modified_at: string;
  data: Readonly<Record<string, unknown>>;
}>;

export type WorkspaceSnapshot = Readonly<{
  workspace_schema_version: 1;
  generated_at: string;
  read_only: true;
  counts: Readonly<Record<string, number>>;
  feature_artifact_counts: Readonly<Record<string, number>>;
  datasets: readonly DatasetSummary[];
  backtests: readonly BacktestSummary[];
  experiments: readonly SnapshotRecord[];
  manifests: readonly SnapshotRecord[];
  events: readonly SnapshotRecord[];
  journals: readonly SnapshotRecord[];
  commercial: readonly SnapshotRecord[];
  paper: SnapshotDashboard | null;
  live: SnapshotDashboard | null;
  operations: SnapshotDashboard | null;
  options: SnapshotDashboard | null;
  commercial_artifacts: readonly EvidenceArtifact[];
}>;

export type WorkspaceContext = Readonly<{
  status: SystemStatus | null;
  features: readonly FeatureDefinition[];
  artifacts: readonly EvidenceArtifact[];
  workspaceFeatures: readonly string[];
  onOpenArtifact: (name: string) => void;
}>;

type Metric = readonly [label: string, value: string, detail: string, state?: "good" | "warn" | "bad"];

const OMS_LIFECYCLE_COVERAGE: ReadonlyArray<readonly [string, string, string]> = [
  ["Fill before acknowledgement", "Handled", "Execution evidence is authoritative and idempotent"],
  ["Fill while cancellation pending", "Handled", "Cumulative quantity advances without losing cancel intent"],
  ["Partial fill then terminal outcome", "Handled", "Filled quantity is preserved for cancel, reject, or expiry"],
  ["Cancel rejection", "Handled", "Explicit broker evidence restores the valid working state"],
  ["Replace lifecycle", "Handled", "Requested, replaced, and rejected outcomes are auditable"],
  ["Broker order versions", "Handled", "Every modified broker version retains lineage"],
  ["Late terminal messages", "Handled", "Safe no-op unless authoritative execution changes quantity"],
  ["UNKNOWN resolution", "Handled", "A new durable resolution step preserves prior history"],
  ["Terminal status vs cumulative fill", "Handled", "Non-filled terminal states cannot claim full quantity"],
];

export function parseWorkspaceSnapshot(value: unknown): WorkspaceSnapshot {
  if (!isRecord(value) || value.workspace_schema_version !== 1 || value.read_only !== true ||
      typeof value.generated_at !== "string" || !isNumberRecord(value.counts) ||
      !isNumberRecord(value.feature_artifact_counts) || !Array.isArray(value.datasets) ||
      !Array.isArray(value.backtests) || !Array.isArray(value.experiments) ||
      !Array.isArray(value.manifests) || !Array.isArray(value.events) ||
      !Array.isArray(value.journals) || !Array.isArray(value.commercial) ||
      !Array.isArray(value.commercial_artifacts)) {
    throw new Error("The workspace projection does not match the v1 read-only contract.");
  }
  for (const item of [...value.experiments, ...value.manifests, ...value.events, ...value.journals, ...value.commercial]) {
    if (!isSnapshotRecord(item)) {
      throw new Error("The workspace projection contains an invalid record.");
    }
  }
  for (const dashboard of [value.paper, value.live, value.operations, value.options]) {
    if (dashboard !== null && !isSnapshotDashboard(dashboard)) {
      throw new Error("The workspace projection contains an invalid dashboard snapshot.");
    }
  }
  return value as WorkspaceSnapshot;
}

export function renderWorkspace(
  summaryRoot: HTMLElement,
  canvasRoot: HTMLElement,
  workspaceId: string,
  snapshot: WorkspaceSnapshot,
  context: WorkspaceContext,
): void {
  summaryRoot.replaceChildren();
  canvasRoot.replaceChildren();
  switch (workspaceId) {
    case "command-center":
      renderCommandCenter(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "research-lab":
      renderResearchLab(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "strategy-studio":
      renderStrategyStudio(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "backtest-explorer":
      renderBacktestExplorer(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "execution-blotter":
      renderExecutionBlotter(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "risk-cockpit":
      renderRiskCockpit(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "portfolio":
      renderPortfolio(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "replay-incidents":
      renderReplayAndIncidents(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "journal":
      renderJournal(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "administration":
      renderAdministration(summaryRoot, canvasRoot, snapshot, context);
      break;
    default:
      renderEmpty(canvasRoot, "Unknown workspace", "Choose a workspace from the navigation.");
  }
}

function renderCommandCenter(
  summaryRoot: HTMLElement,
  root: HTMLElement,
  snapshot: WorkspaceSnapshot,
  context: WorkspaceContext,
): void {
  const services = context.status === null ? [] : Object.values(context.status.services);
  const healthyServices = services.filter((service) => service.status === "healthy").length;
  const operations = operationsDashboard(snapshot);
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const alertCount = (operations?.alerts.length ?? 0) + (paper?.unexplained_incidents ?? 0) +
    (live?.unresolved_incidents ?? 0) + (paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0);
  renderMetrics(summaryRoot, [
    ["Runtime services", services.length === 0 ? "Unavailable" : `${healthyServices}/${services.length}`, "Dashboard, PostgreSQL, and object storage", healthyServices === services.length ? "good" : "bad"],
    ["Indexed evidence", String(snapshot.counts.artifacts ?? 0), "Immutable artifacts across the complete repository"],
    ["Operator attention", String(alertCount), "Alerts, unknown orders, and unresolved discrepancies", alertCount === 0 ? "good" : "bad"],
    ["External gates", "5 open", "PAPER, LIVE, partner, broker-options, and commercial evidence", "warn"],
  ]);

  const serviceSection = createPanel("Runtime and dependencies", "Live health from the container boundary.");
  const serviceRows = context.status === null ? [] : Object.entries(context.status.services).map(([name, service]) => [
    displayName(name), service.status.toUpperCase(), service.detail,
  ]);
  appendTableOrEmpty(serviceSection, ["Service", "Status", "Detail"], serviceRows, "Runtime health is unavailable.");
  root.append(serviceSection);

  const environment = createPanel("Environment readiness", "Code capability is separated from observed operating evidence.");
  appendTableOrEmpty(environment, ["Environment / gate", "Observed", "Required", "Decision"], [
    ["Historical research", "Local verification passed", "Deterministic repeatability", "Available"],
    ["PAPER sessions", String(paper?.clean_paper_days ?? 0), String(paper?.required_paper_days ?? 30), paper?.promotion_eligible ? "Complete" : "Blocked"],
    ["Controlled LIVE sessions", String(live?.clean_live_days ?? 0), String(live?.required_live_days ?? 60), live?.promotion_eligible ? "Complete" : "Blocked"],
    ["Design partners", "0", "5 unaided workflows", "External evidence required"],
    ["Broker-backed options", "0", "Independent clean session", "External evidence required"],
    ["Paying customers", "0", "10 professionals or 3 organizations", "External evidence required"],
  ], "No gate data is available.");
  root.append(environment);

  const attention = createPanel("Attention queue", "Conditions requiring operator investigation; no browser mutation is offered.");
  const rows: string[][] = [];
  for (const alert of operations?.alerts ?? []) {
    rows.push([alert.severity, alert.code, alert.subject, alert.summary]);
  }
  if ((paper?.unknown_orders ?? 0) > 0) rows.push(["CRITICAL", "PAPER_UNKNOWN", paper?.account_id ?? "PAPER", `${paper?.unknown_orders} unknown order(s)`]);
  if ((paper?.unexplained_incidents ?? 0) > 0) rows.push(["CRITICAL", "PAPER_RECONCILIATION", paper?.account_id ?? "PAPER", `${paper?.unexplained_incidents} unexplained incident(s)`]);
  if ((live?.unknown_orders ?? 0) > 0) rows.push(["CRITICAL", "LIVE_UNKNOWN", live?.account_id ?? "LIVE", `${live?.unknown_orders} unknown order(s)`]);
  if ((live?.unresolved_incidents ?? 0) > 0) rows.push(["CRITICAL", "LIVE_RECONCILIATION", live?.account_id ?? "LIVE", `${live?.unresolved_incidents} unresolved incident(s)`]);
  appendTableOrEmpty(attention, ["Severity", "Code", "Subject", "Summary"], rows, "No active evidence-backed alerts.");
  root.append(attention);

  root.append(renderArtifactPanel("Recent evidence", context.artifacts.slice(0, 12), context.onOpenArtifact));
}

function renderResearchLab(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const options = optionsDashboard(snapshot);
  renderMetrics(summaryRoot, [
    ["Datasets", String(snapshot.datasets.length), "CSV and immutable Parquet receipts indexed with row and schema metadata"],
    ["Experiments", String(snapshot.experiments.length), "Immutable experiment catalogue records"],
    ["Backtest artifacts", String(snapshot.backtests.length), "Reproducible completed runs"],
    ["Option contracts", String(options?.analytics.length ?? 0), "Frozen European-option analytics"],
  ]);
  const datasets = createPanel("Dataset inventory", "Historical inputs currently available to deterministic research workflows, including verified Parquet receipts.");
  appendTableOrEmpty(datasets, ["Dataset", "Version / format", "Rows", "Columns", "Modified"], snapshot.datasets.map((dataset) => [
    dataset.dataset_id || dataset.name,
    [dataset.dataset_version, dataset.storage_format || "CSV"].filter(Boolean).join(" / "),
    String(dataset.rows), dataset.columns.join(", "), formatTime(dataset.modified_at),
  ]), "No indexed datasets are available.", (index) => context.onOpenArtifact(snapshot.datasets[index]?.name ?? ""));
  root.append(datasets);

  const experiments = createPanel("Experiment catalogue", "Content-addressed runs and linked output identities.");
  appendTableOrEmpty(experiments, ["Experiment", "Run", "Artifact fingerprint", "Event output", "Source"], snapshot.experiments.map((record) => [
    field(record.data, "experiment_id"), field(record.data, "run_id"), shortHash(field(record.data, "artifact_fingerprint")),
    shortHash(field(record.data, "event_output_hash")), record.artifact,
  ]), "No experiment catalogue records are available.", (index) => context.onOpenArtifact(snapshot.experiments[index]?.artifact ?? ""));
  root.append(experiments);

  if (options !== undefined) {
    const chain = createPanel("Frozen option chain", `${options.chain.underlying_instrument_id} at ${options.chain.underlying_mark} ${options.chain.currency}; ${options.model_version}.`);
    appendTableOrEmpty(chain, ["Contract", "Right", "Strike", "Bid", "Ask", "IV", "Delta", "Gamma", "Theta"], options.analytics.map((item) => [
      item.option_id, item.right, item.strike, item.bid, item.ask, item.implied_volatility, item.delta, item.gamma, item.theta,
    ]), "No option analytics are available.");
    root.append(chain);
  }
  root.append(renderFeatureEvidence(context, ["market-data", "research", "options"]));
}

function renderStrategyStudio(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const operations = operationsDashboard(snapshot);
  const identities = strategyIdentityRows(snapshot, operations);
  renderMetrics(summaryRoot, [
    ["Strategy identities", String(identities.length), "Versioned strategy and bundle combinations"],
    ["Configuration identities", String(new Set(identities.map((row) => row[3])).size), "Exact source-bound configuration hashes"],
    ["Worker boundary", "Isolated", "Cleared environment and verified bundle handshake", "good"],
    ["Broker access", "None", "Strategy SDK has no adapter or credential interface", "good"],
  ]);
  const identityPanel = createPanel("Version and deployment identities", "Every strategy run binds source, configuration, dataset, engine, and event output.");
  appendTableOrEmpty(identityPanel, ["Strategy", "Version", "Bundle", "Configuration", "Dataset", "Engine / source"], identities, "No strategy identities are available.");
  root.append(identityPanel);

  const boundary = createPanel("Worker contract", "The browser does not execute strategy code. The local worker protocol validates identity before the first normalized bar.");
  appendDefinition(boundary, [
    ["Input", "Immutable strategy context and normalized market bars"],
    ["Output", "One validated order intent or null"],
    ["Identity", "SHA-256 of strategy tree, SDK source, and Python runtime"],
    ["Environment", "Cleared; only an explicit non-secret SDK path may be supplied"],
    ["Forbidden", "Broker adapters, credentials, unverified dependencies, and browser execution"],
  ]);
  root.append(boundary);
  root.append(renderFeatureEvidence(context, ["research", "replay"]));
}

function renderBacktestExplorer(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const completed = snapshot.backtests.filter((run) => Object.keys(run.performance).length > 0);
  const fingerprints = new Set(snapshot.backtests.map((run) => text(run.artifact_fingerprint)).filter(Boolean));
  renderMetrics(summaryRoot, [
    ["Completed runs", String(snapshot.backtests.length), "Immutable backtest artifacts"],
    ["Unique outputs", String(fingerprints.size), "Content-addressed artifact fingerprints"],
    ["Completion manifests", String(snapshot.manifests.length), "SHA-256 publication records"],
    ["Metric-complete", String(completed.length), "Runs with performance and accounting summaries"],
  ]);
  const runs = createPanel("Run comparison", "Compare performance, accounting, and provenance without rerunning or mutating results.");
  appendTableOrEmpty(runs, ["Artifact", "Strategy", "Dataset", "Trades", "Net P&L", "Return bps", "Max drawdown", "Fingerprint"], snapshot.backtests.slice(0, 50).map((run) => {
    const dataset = record(run.specification.dataset);
    return [run.artifact, field(run.specification, "strategy_version") || shortHash(field(run.specification, "strategy_bundle_hash")),
      field(dataset, "dataset_id") || "Legacy artifact", field(run.performance, "trade_count"), field(run.performance, "net_pnl") || field(run.report, "realized_pnl"),
      field(run.performance, "return_bps"), field(run.performance, "max_drawdown_bps"), shortHash(text(run.artifact_fingerprint))];
  }), "No backtest result artifacts are available.", (index) => context.onOpenArtifact(snapshot.backtests[index]?.artifact ?? ""));
  root.append(runs);

  const executionModel = createPanel("Execution realism model", "Every run binds these deterministic assumptions through its immutable configuration fingerprint.");
  appendDefinition(executionModel, [
    ["Quoted spread", "Buys pay and sells concede half of the configured full spread"],
    ["Slippage", "Configured basis points are applied unfavourably after half-spread"],
    ["Limit protection", "The final spread-and-slippage price can never violate the order limit"],
    ["Latency", "A configured number of complete market bars must pass before fill eligibility"],
    ["Partial fills", "An optional per-bar quantity cap persists remaining quantity as a working order"],
    ["Trading halts", "Version-controlled venue or instrument halt windows block strategy evaluation"],
  ]);
  root.append(executionModel);

  const manifests = createPanel("Publication manifests", "A manifest binds the artifact, events, report, configuration, and specification hashes.");
  appendTableOrEmpty(manifests, ["Manifest", "Artifact hash", "Events hash", "Report hash", "Configuration"], snapshot.manifests.map((item) => [
    item.artifact, shortHash(field(item.data, "artifact_sha256")), shortHash(field(item.data, "events_sha256")),
    shortHash(field(item.data, "report_sha256")), shortHash(field(item.data, "configuration_hash")),
  ]), "No completion manifests are available.", (index) => context.onOpenArtifact(snapshot.manifests[index]?.artifact ?? ""));
  root.append(manifests);

  const options = optionsDashboard(snapshot);
  if (options !== undefined) {
    const scenarios = createPanel("Options expiry scenarios", `${options.strategy.strategy_id} / ${options.strategy.strategy_version}; deterministic European expiry payoff.`);
    appendTableOrEmpty(scenarios, ["Underlying", "Total P&L", "Legs"], options.strategy.scenarios.map((scenario) => [
      scenario.underlying_price, scenario.total_pnl, scenario.legs.map((leg) => `${leg.leg_id}: ${leg.pnl}`).join(" | "),
    ]), "No option scenarios are available.");
    root.append(scenarios);
  }
  root.append(renderFeatureEvidence(context, ["research", "replay", "options"]));
}

function renderExecutionBlotter(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const executionEvents = snapshot.events.filter((item) => isExecutionEvent(field(item.data, "event_type")));
  const orders = executionEvents.filter((item) => field(item.data, "event_type").includes("order"));
  const fills = executionEvents.filter((item) => /fill|execution/i.test(field(item.data, "event_type")));
  renderMetrics(summaryRoot, [
    ["Lifecycle events", String(executionEvents.length), "Intent, risk, OMS, fill, cancel, replace, and terminal evidence"],
    ["Order transitions", String(orders.length), "Canonical order state changes"],
    ["Executions", String(fills.length), "Idempotent execution evidence"],
    ["Unknown orders", String((paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0)), "PAPER and controlled-LIVE unresolved state", (paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0) === 0 ? "good" : "bad"],
  ]);
  const environment = createPanel("Execution environments", "Simulation, PAPER, and controlled-LIVE remain visibly distinct.");
  appendTableOrEmpty(environment, ["Environment", "Account", "Broker", "Working", "Unknown", "Audit", "Gate"], [
    ["SIMULATION", "Fixture/backtest accounts", "No broker", "Evidence trail", "0", "Canonical events", "Research available"],
    ["PAPER", paper?.account_id ?? "No snapshot", paper === undefined ? "Unavailable" : paper.broker_connected ? "Connected" : "Disconnected", String(paper?.working_orders ?? 0), String(paper?.unknown_orders ?? 0), paper?.complete_auditability ? "Complete" : "Incomplete", `${paper?.clean_paper_days ?? 0}/${paper?.required_paper_days ?? 30}`],
    ["LIVE / SHADOW-CANARY", live?.account_id ?? "No snapshot", live === undefined ? "Unavailable" : live.broker_connected ? "Connected" : "Disconnected", String(live?.working_orders ?? 0), String(live?.unknown_orders ?? 0), live?.complete_auditability ? "Complete" : "Incomplete", `${live?.clean_live_days ?? 0}/${live?.required_live_days ?? 60}`],
  ], "No execution environment snapshots are available.");
  root.append(environment);

  const blotter = createPanel("Causal execution blotter", "Every row links event, causation, correlation, actor, and normalized lifecycle payload.");
  appendTableOrEmpty(blotter, ["Time", "Phase", "Order / intent", "State / decision", "Quantity", "Correlation", "Source"], executionEvents.slice(0, 300).map((item) => {
    const payload = record(item.data.payload);
    return [field(item.data, "event_time"), field(item.data, "event_type"), field(payload, "order_id") || field(payload, "intent_id") || field(payload, "execution_id"),
      field(payload, "new_state") || field(payload, "status") || (payload.approved === true ? "APPROVED" : payload.approved === false ? "REJECTED" : field(payload, "reason")),
      field(payload, "quantity") || field(payload, "filled_quantity") || field(payload, "cumulative_quantity"), field(item.data, "correlation_id"), item.artifact];
  }), "No execution lifecycle events are available.", (index) => context.onOpenArtifact(executionEvents[index]?.artifact ?? ""));
  root.append(blotter);

  const lifecycle = createPanel("Broker lifecycle condition coverage", "Explicit handling for the out-of-order and modification cases recorded in the system review.");
  appendTableOrEmpty(lifecycle, ["Condition", "Implementation", "Invariant"], OMS_LIFECYCLE_COVERAGE.map((row) => [...row]), "No lifecycle coverage metadata is available.");
  root.append(lifecycle);
  root.append(renderFeatureEvidence(context, ["replay", "paper", "controlled-live"]));
}

function renderRiskCockpit(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const operations = operationsDashboard(snapshot);
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const limits = operations?.risk.limits ?? [];
  const breaches = limits.filter((limit) => limit.breached).length;
  const killSwitches = [...(operations?.operational_health.active_kill_switches ?? []), ...(paper?.active_kill_switches ?? []), ...(live?.active_kill_switches ?? [])];
  renderMetrics(summaryRoot, [
    ["Risk state", operations?.risk.state ?? "Unavailable", "Deterministic workbench projection", operations?.risk.state === "NORMAL" ? "good" : operations === undefined ? "warn" : "bad"],
    ["Current equity", operations === undefined ? "Unavailable" : `${operations.risk.current_equity} ${operations.currency}`, "Marked internal equity"],
    ["Limit breaches", String(breaches), `${limits.length} configured cockpit limits`, breaches === 0 ? "good" : "bad"],
    ["Active kill switches", String(killSwitches.length), killSwitches.length === 0 ? "All monitored scopes clear" : killSwitches.join(", "), killSwitches.length === 0 ? "good" : "bad"],
  ]);
  const exposure = createPanel("Exposure and loss control", "Fixed-point values from the selected immutable operations projection.");
  if (operations === undefined) {
    renderEmpty(exposure, "Operations projection unavailable", "Generate an operations dashboard artifact to populate risk controls.");
  } else {
    appendDefinition(exposure, [
      ["Cash", `${operations.risk.cash} ${operations.currency}`], ["Gross exposure", `${operations.risk.gross_exposure} ${operations.currency}`],
      ["Largest position", `${operations.risk.largest_position_exposure} ${operations.currency}`], ["Drawdown", `${operations.risk.drawdown_bps} bps`],
      ["Peak equity", `${operations.risk.effective_peak_equity} ${operations.currency}`], ["Open positions", String(operations.risk.open_positions)],
    ]);
  }
  root.append(exposure);

  const limitPanel = createPanel("Versioned risk limits", "Current values are shown beside their exact evaluated limits.");
  appendTableOrEmpty(limitPanel, ["Limit", "Current", "Threshold", "Status"], limits.map((limit) => [
    displayName(limit.limit_id), limit.current, limit.limit, limit.breached ? "BREACHED" : "Within limit",
  ]), "No risk-limit projection is available.");
  root.append(limitPanel);

  const alerts = createPanel("Alerts and reconciliation", "Audit, broker, schedule, kill-switch, and reconciliation conditions.");
  const alertRows = (operations?.alerts ?? []).map((alert) => [alert.severity, alert.code, alert.subject, alert.summary]);
  alertRows.push(["PAPER", "RECONCILIATION", paper?.account_id ?? "No snapshot", reconciliationText(paper?.last_reconciliation_clean, paper?.last_reconciled_at)]);
  alertRows.push(["LIVE", "RECONCILIATION", live?.account_id ?? "No snapshot", reconciliationText(live?.last_reconciliation_clean, live?.last_reconciled_at)]);
  appendTableOrEmpty(alerts, ["Scope", "Code", "Subject", "State"], alertRows, "No alerts or reconciliation records are available.");
  root.append(alerts);
  root.append(renderFeatureEvidence(context, ["paper", "controlled-live", "operations"]));
}

function renderPortfolio(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const operations = operationsDashboard(snapshot);
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const options = optionsDashboard(snapshot);
  const positionCount = (operations?.positions.length ?? 0) + (paper?.positions.length ?? 0) + (live?.positions.length ?? 0);
  renderMetrics(summaryRoot, [
    ["Visible positions", String(positionCount), "Operations, PAPER, and controlled-LIVE internal ledgers"],
    ["Net attribution", operations === undefined ? "Unavailable" : `${operations.attribution.net_pnl} ${operations.currency}`, "Realized, unrealized, fee, dividend, and action movements"],
    ["Options reconciliation", options === undefined ? "Unavailable" : options.reconciliation.clean ? "Clean" : `${options.reconciliation.issues.length} issue(s)`, "BACKTEST / PAPER / LIVE declared books", options?.reconciliation.clean ? "good" : "warn"],
    ["Scenario points", String(options?.strategy.scenarios.length ?? 0), "Deterministic multi-leg expiry outcomes"],
  ]);
  const positions = createPanel("Internal positions", "Independent portfolio truth; reconciliation differences are not overwritten.");
  const positionRows: string[][] = [];
  for (const item of operations?.positions ?? []) positionRows.push(["OPERATIONS", item.instrument_id, item.quantity, item.average_cost, item.mark_price, item.realized_pnl]);
  for (const item of paper?.positions ?? []) positionRows.push(["PAPER", item.instrument_id, item.quantity, item.average_cost, "—", item.realized_pnl]);
  for (const item of live?.positions ?? []) positionRows.push(["LIVE", item.instrument_id, item.quantity, item.average_cost, "—", item.realized_pnl]);
  appendTableOrEmpty(positions, ["Source", "Instrument", "Quantity", "Average cost", "Mark", "Realized P&L"], positionRows, "No internal positions are present in the latest snapshots.");
  root.append(positions);

  const attribution = createPanel("P&L attribution", "Immutable accounting movements grouped by instrument and category.");
  appendTableOrEmpty(attribution, ["Instrument", "Category", "Amount"], (operations?.attribution.rows ?? []).map((row) => [
    row.instrument_id, displayName(row.category), `${row.amount} ${operations?.currency ?? ""}`,
  ]), "No attribution rows are available.");
  root.append(attribution);

  if (options !== undefined) {
    const scenario = createPanel("Options scenario and book reconciliation", `Compared at ${formatTime(options.reconciliation.reconciled_at)} using independently fingerprinted exports.`);
    appendTableOrEmpty(scenario, ["Underlying", "Strategy P&L", "Leg detail"], options.strategy.scenarios.map((item) => [
      item.underlying_price, `${item.total_pnl} ${options.chain.currency}`, item.legs.map((leg) => `${leg.option_id}: ${leg.pnl}`).join(" | "),
    ]), "No scenario rows are available.");
    root.append(scenario);
  }
  root.append(renderFeatureEvidence(context, ["replay", "paper", "operations", "options"]));
}

function renderReplayAndIncidents(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const typeCounts = new Map<string, number>();
  for (const event of snapshot.events) {
    const type = field(event.data, "event_type");
    typeCounts.set(type, (typeCounts.get(type) ?? 0) + 1);
  }
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const unresolved = (paper?.unexplained_incidents ?? 0) + (live?.unresolved_incidents ?? 0);
  renderMetrics(summaryRoot, [
    ["Canonical events", String(snapshot.events.length), "Causally ordered immutable envelopes"],
    ["Event types", String(typeCounts.size), "Market, intent, risk, OMS, fill, portfolio, and audit phases"],
    ["Journal records", String(snapshot.journals.length), "PAPER, LIVE, operations, and commercial chains"],
    ["Unresolved incidents", String(unresolved), "Latest PAPER and LIVE projections", unresolved === 0 ? "good" : "bad"],
  ]);
  const distribution = createPanel("Event distribution", "Counts by canonical event type across indexed replay outputs.");
  appendTableOrEmpty(distribution, ["Event type", "Count"], [...typeCounts.entries()].sort((a, b) => b[1] - a[1]).map(([type, count]) => [type, String(count)]), "No canonical events are available.");
  root.append(distribution);

  const timeline = createPanel("Causal replay timeline", "Correlation and causation links reconstruct strategy, risk, order, fill, and portfolio effects.");
  appendTableOrEmpty(timeline, ["Time", "Event", "Actor", "Correlation", "Caused by", "Instrument", "Artifact"], snapshot.events.slice(0, 500).map((item) => [
    field(item.data, "event_time"), field(item.data, "event_type"), field(item.data, "actor"), field(item.data, "correlation_id"),
    field(item.data, "causation_id"), field(item.data, "instrument_id"), item.artifact,
  ]), "No canonical replay timeline is available.", (index) => context.onOpenArtifact(snapshot.events[index]?.artifact ?? ""));
  root.append(timeline);

  const incidents = createPanel("Incident and recovery state", "UNKNOWN and reconciliation differences remain explicit until new evidence resolves them.");
  appendTableOrEmpty(incidents, ["Environment", "Unknown orders", "Incidents", "Reconciliation", "Audit sequence", "Audit head"], [
    ["PAPER", String(paper?.unknown_orders ?? 0), String(paper?.unexplained_incidents ?? 0), reconciliationText(paper?.last_reconciliation_clean, paper?.last_reconciled_at), String(paper?.audit_sequence ?? 0), shortHash(paper?.audit_head_hash ?? "")],
    ["LIVE", String(live?.unknown_orders ?? 0), String(live?.unresolved_incidents ?? 0), reconciliationText(live?.last_reconciliation_clean, live?.last_reconciled_at), String(live?.audit_sequence ?? 0), shortHash(live?.audit_head_hash ?? "")],
  ], "No incident state is available.");
  root.append(incidents);
  root.append(renderFeatureEvidence(context, ["replay", "paper", "controlled-live", "operations"]));
}

function renderJournal(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const categories = new Set(snapshot.journals.map((item) => item.category ?? "unknown"));
  const operations = operationsDashboard(snapshot);
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  renderMetrics(summaryRoot, [
    ["Journal records", String(snapshot.journals.length), "Bounded records from every integrated ledger"],
    ["Journal domains", String(categories.size), "PAPER, LIVE, operations, and commercial"],
    ["Operations cursor", String(operations?.journal.sequence ?? 0), operations?.journal.healthy ? "Verified" : "Unavailable or failed", operations?.journal.healthy ? "good" : "warn"],
    ["Trading audit heads", String((paper === undefined ? 0 : 1) + (live === undefined ? 0 : 1)), "PAPER and controlled-LIVE hash-chain snapshots"],
  ]);
  const integrity = createPanel("Chain integrity", "Head hashes and sequence cursors are displayed without permitting history edits.");
  appendTableOrEmpty(integrity, ["Journal", "Health", "Sequence", "Head", "Source"], [
    ["PAPER", paper?.persistence_healthy ? "Healthy" : "Unavailable / failed", String(paper?.audit_sequence ?? 0), shortHash(paper?.audit_head_hash ?? ""), snapshot.paper?.artifact ?? "—"],
    ["Controlled LIVE", live?.audit_healthy ? "Healthy" : "Unavailable / failed", String(live?.audit_sequence ?? 0), shortHash(live?.audit_head_hash ?? ""), snapshot.live?.artifact ?? "—"],
    ["Operations", operations?.journal.healthy ? "Healthy" : "Unavailable / failed", String(operations?.journal.sequence ?? 0), shortHash(operations?.journal.head_hash ?? ""), snapshot.operations?.artifact ?? "—"],
  ], "No integrity heads are available.");
  root.append(integrity);

  const records = createPanel("Unified append-only journal", "Entries remain separated by source domain and retain their original artifact.");
  appendTableOrEmpty(records, ["Domain", "Sequence", "Time", "Event type", "Actor", "Record hash", "Artifact"], snapshot.journals.slice(0, 500).map((item) => [
    (item.category ?? "unknown").toUpperCase(), field(item.data, "sequence"), field(item.data, "occurred_at"), field(item.data, "event_type") || "State snapshot",
    field(item.data, "actor"), shortHash(field(item.data, "entry_hash") || field(item.data, "record_hash")), item.artifact,
  ]), "No journal records are available.", (index) => context.onOpenArtifact(snapshot.journals[index]?.artifact ?? ""));
  root.append(records);
  root.append(renderFeatureEvidence(context, ["paper", "controlled-live", "operations", "commercial"]));
}

function renderAdministration(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const provisioned = snapshot.commercial.filter((item) => field(item.data, "event_type") === "commercial.tenant_provisioned.v1").length;
  const subscriptions = snapshot.commercial.filter((item) => field(item.data, "event_type").includes("subscription")).length;
  const releaseArtifacts = snapshot.commercial_artifacts.filter((item) => /release|signature|trusted-key/i.test(item.name));
  const selfHostArtifacts = snapshot.commercial_artifacts.filter((item) => /self-host|readiness/i.test(item.name));
  renderMetrics(summaryRoot, [
    ["Provisioning records", String(provisioned), "Pseudonymous tenant evidence"],
    ["Subscription observations", String(subscriptions), "External payment facts recorded without card data"],
    ["Release evidence", String(releaseArtifacts.length), "Manifest, signature, and trusted-key artifacts", releaseArtifacts.length > 0 ? "good" : "warn"],
    ["Self-host readiness", String(selfHostArtifacts.length), "Verified entitlement and signed release evidence", selfHostArtifacts.length > 0 ? "good" : "warn"],
  ]);
  const tenants = createPanel("Commercial ledger", "Typed, pseudonymous commercial facts; this dashboard does not process payments or identity.");
  appendTableOrEmpty(tenants, ["Sequence", "Tenant", "Event", "Actor", "Occurred", "Record hash", "Artifact"], snapshot.commercial.map((item) => [
    field(item.data, "sequence"), field(item.data, "tenant_id"), field(item.data, "event_type"), field(item.data, "actor"),
    field(item.data, "occurred_at"), shortHash(field(item.data, "record_hash")), item.artifact,
  ]), "No commercial ledger records are available.", (index) => context.onOpenArtifact(snapshot.commercial[index]?.artifact ?? ""));
  root.append(tenants);

  const controls = createPanel("Deployment and administrative controls", "Implemented evidence primitives and their enforced operating boundary.");
  appendTableOrEmpty(controls, ["Capability", "Repository implementation", "Dashboard integration", "Remaining external dependency"], [
    ["Provisioning", "Typed tenant and workspace record", provisioned > 0 ? "Evidence visible" : "No local evidence", "Customer identity and authorization gateway"],
    ["Entitlement", "Deterministic PAID / GRACE / denied derivation", subscriptions > 0 ? "Ledger evidence visible" : "No subscription observation", "Payment-provider validation and gateway enforcement"],
    ["Privacy / retention", "Hash-bound plan and confirmed single-file execution", artifactCount(snapshot, /privacy|retention/i) > 0 ? "Artifacts visible" : "No local plan artifact", "Reviewed request, legal hold, and authorized operator"],
    ["Signed release", "Manifest plus detached Ed25519 verification", releaseArtifacts.length > 0 ? "Evidence visible" : "No local signed release evidence", "Offline HSM/KMS signing and independent review"],
    ["Self-host readiness", "Loopback, managed-secret, signature, entitlement checks", selfHostArtifacts.length > 0 ? "Evidence visible" : "No readiness receipt", "Customer deployment, backups, TLS, and monitoring"],
    ["Authentication / RBAC", "Basic operator gate for protected deployment", "Runtime auth mode visible", "Customer identity, roles, MFA, and revocation service"],
  ], "No administrative control mapping is available.");
  root.append(controls);

  const boundary = createPanel("Privileged-action boundary", "These operations intentionally stay outside the web process.");
  appendDefinition(boundary, [
    ["Never accepted by this server", "Broker credentials, payment cards, private signing keys, raw customer identity, or live approval secrets"],
    ["Operator-only commands", "Provisioning, retention execution, release signing, entitlement checks, kill switches, schedule completion, and journal append"],
    ["Why", "They require stronger identity, confirmation, filesystem, two-person, offline-signing, or broker boundaries than this local read-only dashboard provides"],
  ]);
  root.append(boundary);
  root.append(renderFeatureEvidence(context, ["commercial"]));
}

function renderFeatureEvidence(context: WorkspaceContext, featureIds: readonly string[]): HTMLElement {
  const artifacts = context.artifacts.filter((item) => featureIds.includes(item.feature));
  return renderArtifactPanel("Workspace evidence", artifacts.slice(0, 30), context.onOpenArtifact);
}

function renderArtifactPanel(title: string, artifacts: readonly EvidenceArtifact[], onOpen: (name: string) => void): HTMLElement {
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

function renderMetrics(root: HTMLElement, metrics: readonly Metric[]): void {
  root.replaceChildren();
  for (const [label, value, detail, state] of metrics) {
    const card = document.createElement("article");
    card.className = `workspace-metric${state === undefined ? "" : ` metric-${state}`}`;
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

function createPanel(titleText: string, descriptionText: string): HTMLElement {
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

function appendDefinition(parent: HTMLElement, values: ReadonlyArray<readonly [string, string]>): void {
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

function appendTableOrEmpty(
  parent: HTMLElement,
  headers: readonly string[],
  rows: readonly (readonly string[])[],
  emptyText: string,
  onRow?: (index: number) => void,
): void {
  if (rows.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = emptyText;
    parent.append(empty);
    return;
  }
  const scroll = document.createElement("div");
  scroll.className = "table-scroll";
  const table = document.createElement("table");
  const heading = document.createElement("thead");
  const headerRow = document.createElement("tr");
  for (const header of headers) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = header;
    headerRow.append(cell);
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
    for (const value of values) {
      const cell = document.createElement("td");
      cell.textContent = value || "—";
      row.append(cell);
    }
    body.append(row);
  });
  table.append(body);
  scroll.append(table);
  parent.append(scroll);
}

function renderEmpty(parent: HTMLElement, titleText: string, detailText: string): void {
  const title = document.createElement("h3");
  title.textContent = titleText;
  const detail = document.createElement("p");
  detail.className = "empty-state";
  detail.textContent = detailText;
  parent.append(title, detail);
}

function paperDashboard(snapshot: WorkspaceSnapshot): PaperDashboard | undefined {
  return parseDashboard(snapshot.paper, parsePaperDashboard);
}

function liveDashboard(snapshot: WorkspaceSnapshot): LiveMonitoringDashboard | undefined {
  return parseDashboard(snapshot.live, parseLiveMonitoringDashboard);
}

function operationsDashboard(snapshot: WorkspaceSnapshot): OperationsDashboard | undefined {
  return parseDashboard(snapshot.operations, parseOperationsDashboard);
}

function optionsDashboard(snapshot: WorkspaceSnapshot): OptionsDashboard | undefined {
  return parseDashboard(snapshot.options, parseOptionsDashboard);
}

function parseDashboard<T>(snapshot: SnapshotDashboard | null, parser: (json: string) => T): T | undefined {
  if (snapshot === null) return undefined;
  try {
    return parser(JSON.stringify(snapshot.data));
  } catch {
    return undefined;
  }
}

function strategyIdentityRows(snapshot: WorkspaceSnapshot, operations: OperationsDashboard | undefined): string[][] {
  const rows: string[][] = [];
  for (const run of snapshot.backtests) {
    const dataset = record(run.specification.dataset);
    rows.push([
      field(run.specification, "strategy_id") || "Backtest strategy",
      field(run.specification, "strategy_version") || "Bound by artifact",
      shortHash(field(run.specification, "strategy_bundle_hash")),
      shortHash(field(run.specification, "configuration_hash")),
      field(dataset, "dataset_id") || shortHash(field(dataset, "content_hash")),
      field(run.specification, "engine_version") || run.artifact,
    ]);
  }
  if (operations !== undefined) {
    rows.unshift([
      operations.reproducibility.strategy_id,
      operations.reproducibility.strategy_version,
      shortHash(operations.reproducibility.strategy_bundle_hash),
      shortHash(operations.configuration.configuration_content_hash),
      `${operations.reproducibility.dataset_id} / ${operations.reproducibility.dataset_version}`,
      "Operations projection",
    ]);
  }
  return rows;
}

function isExecutionEvent(eventType: string): boolean {
  return /intent|risk\.decision|order|execution|fill|cancel|replace|reject|expir/i.test(eventType);
}

function reconciliationText(clean: boolean | null | undefined, at: string | null | undefined): string {
  if (clean === undefined || clean === null) return "Not yet reconciled";
  return `${clean ? "Clean" : "Discrepancy"}${at === undefined || at === null ? "" : ` at ${formatTime(at)}`}`;
}

function artifactCount(snapshot: WorkspaceSnapshot, pattern: RegExp): number {
  return snapshot.commercial_artifacts.filter((artifact) => pattern.test(artifact.name)).length;
}

function isSnapshotRecord(value: unknown): value is SnapshotRecord {
  return isRecord(value) && typeof value.artifact === "string" && isRecord(value.data);
}

function isSnapshotDashboard(value: unknown): value is SnapshotDashboard {
  return isRecord(value) && typeof value.artifact === "string" && typeof value.modified_at === "string" && isRecord(value.data);
}

function isNumberRecord(value: unknown): value is Record<string, number> {
  return isRecord(value) && Object.values(value).every((item) => typeof item === "number" && Number.isFinite(item));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function record(value: unknown): Readonly<Record<string, unknown>> {
  return isRecord(value) ? value : {};
}

function field(value: Readonly<Record<string, unknown>>, name: string): string {
  return text(value[name]);
}

function text(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return String(value);
  return "[structured]";
}

function shortHash(value: string): string {
  if (value.length <= 18) return value;
  return `${value.slice(0, 10)}…${value.slice(-6)}`;
}

function displayName(value: string): string {
  return value.replaceAll("_", " ").replaceAll("-", " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function formatTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString();
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
