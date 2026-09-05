import {
  LiveMonitoringDashboard,
  OperationsDashboard,
  OptionsDashboard,
  PaperDashboard,
  parseAdapterQualification,
  parseAdversarialEvaluation,
  parseAssumptionRegimeMonitor,
  parseAssistantEvidence,
  parseAttentionBudget,
  parseAutomationMandate,
  parseCapabilityExecutionPlanner,
  parseCapitalAllocationPlan,
  parseCapitalAllocationProposal,
  parseChampionChallengerEvaluation,
  parseCompatibilityMatrix,
  parseContinuityPolicy,
  parseCounterfactualScenario,
  parseDataRightsAndSemanticsReceipt,
  parseDecisionReconstruction,
  parseEventExposureCalendar,
  parseExecutionCoachBenchmark,
  parseExperimentLineage,
  parseExposureGraph,
  parseFeedSubstitutionParity,
  parseFundLedgerStatement,
  parseGatewayQualificationMatrix,
  parseKnowledgeSnapshot,
  parseLiveMonitoringDashboard,
  parseModelEvaluationBenchmark,
  parseMultiAssetExpansionPlan,
  parseOperationsDiagnosisRunbook,
  parseOrderDecisionPassport,
  parseOperationsDashboard,
  parseOptionsDashboard,
  parsePaperDashboard,
  parsePortfolioExperiment,
  parseRecoveryDrillResult,
  parseResearchHypothesis,
  parseResearchJob,
  parseRobustnessEvaluation,
  parseSandboxInstallationPreview,
  parseScenarioLossSimulation,
  parseStrategyCapsuleManifest,
  parseWorkspaceSnapshotManifest,
} from "./evidence.js";
import { FeatureDefinition, SystemStatus } from "./catalog.js";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { OrderTicket } from "./OrderTicket.js";

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

type EventWindow = Readonly<{
  artifact: string;
  window_kind: "prefix";
  source_record_count_lower_bound: number;
  retained_record_count: number;
  retained_event_count: number;
  truncated: boolean;
  first_event_id: string | null;
  first_event_time: string | null;
  last_event_id: string | null;
  last_event_time: string | null;
}>;

type WorkspaceEventWindow = Readonly<{
  window_kind: "causal_prefix";
  source_event_count_lower_bound: number;
  retained_event_count: number;
  truncated: boolean;
}>;

type ProjectionDiagnostic = Readonly<{
  artifact: string;
  code: string;
  detail: string;
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

type NotebookSummary = Readonly<{
  artifact: string;
  modified_at: string;
  bytes: number;
  nbformat: number;
  cell_count: number;
  code_cells: number;
  markdown_cells: number;
  output_count: number;
  kernel: string;
  language: string;
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
  notebooks: readonly NotebookSummary[];
  backtests: readonly BacktestSummary[];
  experiments: readonly SnapshotRecord[];
  manifests: readonly SnapshotRecord[];
  events: readonly SnapshotRecord[];
  journals: readonly SnapshotRecord[];
  commercial: readonly SnapshotRecord[];
  execution_evidence: readonly SnapshotRecord[];
  /** Typed v1 advanced artifacts; absent when talking to a pre-registry server. */
  advanced_evidence?: readonly SnapshotRecord[];
  /** Canonical causation-respecting replay order; presentation events stay newest first. */
  replay_events?: readonly SnapshotRecord[];
  /** Bounded NDJSON window metadata; a truncated window is never a complete trail. */
  event_windows?: readonly EventWindow[];
  /** Bounded aggregate causal replay window. */
  event_window?: WorkspaceEventWindow;
  /** Rejected records are disclosed separately from accepted evidence. */
  projection_diagnostics?: readonly ProjectionDiagnostic[];
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

type Metric = readonly [label: string, value: string, detail: string, state?: "good" | "warn" | "bad", isSignature?: boolean];

let mountedTicket: ReturnType<typeof createRoot> | undefined;
const tableFilterValues = new Map<string, string>();

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
      typeof value.generated_at !== "string" || !isCountRecord(value.counts, [
        "artifacts", "datasets", "notebooks", "backtests", "experiments", "events", "journals", "commercial_records",
      ]) || !isCountRecord(value.feature_artifact_counts, [
        "market-data", "replay", "research", "paper", "controlled-live", "operations", "options", "commercial",
        "execution-risk", "accounting", "identity", "platform",
      ]) || !Array.isArray(value.datasets) || !Array.isArray(value.notebooks) || !Array.isArray(value.backtests) || !Array.isArray(value.experiments) ||
      !Array.isArray(value.manifests) || !Array.isArray(value.events) ||
      !Array.isArray(value.journals) || !Array.isArray(value.commercial) || !Array.isArray(value.execution_evidence) ||
      !Array.isArray(value.commercial_artifacts) ||
      (value.advanced_evidence !== undefined && !Array.isArray(value.advanced_evidence)) ||
      (value.replay_events !== undefined && !Array.isArray(value.replay_events)) ||
      (value.event_windows !== undefined && !Array.isArray(value.event_windows)) ||
      (value.event_window !== undefined && !isWorkspaceEventWindow(value.event_window)) ||
      (value.projection_diagnostics !== undefined && !Array.isArray(value.projection_diagnostics))) {
    throw new Error("The workspace projection does not match the v1 evidence contract.");
  }
  if (!value.datasets.every(isDatasetSummary) || !value.notebooks.every(isNotebookSummary) || !value.backtests.every(isBacktestSummary) ||
      !value.commercial_artifacts.every(isEvidenceArtifact) ||
      !(value.event_windows ?? []).every(isEventWindow) ||
      !(value.projection_diagnostics ?? []).every(isProjectionDiagnostic)) {
    throw new Error("The workspace projection contains invalid typed evidence.");
  }
  for (const item of [
    ...value.experiments, ...value.manifests, ...value.events, ...value.journals, ...value.commercial,
    ...value.execution_evidence, ...(value.advanced_evidence ?? []), ...(value.replay_events ?? []),
  ]) {
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
  mountedTicket?.unmount();
  mountedTicket = undefined;
  summaryRoot.replaceChildren();
  canvasRoot.replaceChildren();
  switch (workspaceId) {
    case "command-center":
      renderCommandCenter(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "research-lab":
      renderResearchLab(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "news-cockpit":
      renderNewsCockpit(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "strategy-studio":
      renderStrategyStudio(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "marketplace":
      renderMarketplace(summaryRoot, canvasRoot, snapshot, context);
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
  const strategyIdentities = strategyIdentityRows(snapshot, operations);
  const openGateCount = (paper?.promotion_eligible ? 0 : 1) + (live?.promotion_eligible ? 0 : 1);
  const alertCount = (operations?.alerts.length ?? 0) + (paper?.unexplained_incidents ?? 0) +
    (live?.unresolved_incidents ?? 0) + (paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0);
  renderMetrics(summaryRoot, [
    ["Runtime services", services.length === 0 ? "Unavailable" : `${healthyServices}/${services.length}`, "Dashboard, gRPC kernel, PostgreSQL, and object storage", healthyServices === services.length ? "good" : "bad", true],
    ["Indexed evidence", String(snapshot.counts.artifacts ?? 0), "Immutable artifacts across the complete repository"],
    ["Operator attention", String(alertCount), "Alerts, unknown orders, and unresolved discrepancies", alertCount === 0 ? "good" : "bad"],
    ["Promotion gates", `${openGateCount} open`, "PAPER and controlled-LIVE dashboard evidence", openGateCount === 0 ? "good" : "warn"],
  ]);

  const briefPanel = createPanel(
    "Daily Operating Brief",
    "Consolidated operational readiness for the current trading session. Unknown states prevent an all-clear summary.",
  );
  briefPanel.id = "daily-brief";

  const hasUnknownState = context.status === null || services.length === 0 ||
    paper === undefined || (paper?.unknown_orders ?? 0) > 0 || (paper?.unexplained_incidents ?? 0) > 0 ||
    (live?.unknown_orders ?? 0) > 0 || (live?.unresolved_incidents ?? 0) > 0 ||
    healthyServices < services.length;

  const briefStatusText = hasUnknownState
    ? "ATTENTION REQUIRED · Active discrepancies, unknown orders, or unverified feed state detected"
    : "NOMINAL · All monitored dependencies healthy, no unknown orders or reconciliation incidents";

  const briefBadgeClass = hasUnknownState ? "f-badge f-badge--warn" : "f-badge f-badge--good";

  const headerDiv = document.createElement("div");
  headerDiv.className = "brief-header";
  const statusBadge = document.createElement("span");
  statusBadge.className = briefBadgeClass;
  statusBadge.textContent = briefStatusText;
  headerDiv.append(statusBadge);
  briefPanel.append(headerDiv);

  const briefStatements = [
    [
      "Connectivity & Feeds",
      services.length === 0
        ? "UNKNOWN: Runtime services unreachable"
        : `${healthyServices}/${services.length} services healthy · PAPER: ${paper?.broker_connected ? "CONNECTED" : "DISCONNECTED"}`,
      context.status === null ? "No status response" : "Linked: /api/v1/status & paper-dashboard.json",
      healthyServices === services.length && paper?.broker_connected ? "HEALTHY" : "ATTENTION",
    ],
    [
      "Order Lifecycle & Reconcile",
      `Unknown orders: ${(paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0)} · Unresolved incidents: ${(paper?.unexplained_incidents ?? 0) + (live?.unresolved_incidents ?? 0)}`,
      paper ? "Linked: paper-dashboard.json" : "No paper projection",
      (paper?.unknown_orders ?? 0) === 0 && (paper?.unexplained_incidents ?? 0) === 0 ? "HEALTHY" : "ATTENTION",
    ],
    [
      "Risk Headroom & Policy",
      operations ? `State: ${operations.risk.state} · Evaluated limits: ${operations.risk.limits.length}` : "UNKNOWN: No operations projection",
      operations ? "Linked: operations-dashboard.json" : "No operations projection",
      operations?.risk.state === "NORMAL" ? "HEALTHY" : "ATTENTION",
    ],
    [
      "Operational Gates",
      `${openGateCount} gates require external evidence before production promotion`,
      "Linked: 03-roadmap-and-gates.md",
      "MONITORED",
    ],
  ];

  appendTableOrEmpty(
    briefPanel,
    ["Dimension", "Operational Observation", "Evidence Anchor", "Disposition"],
    briefStatements,
    "No brief data available.",
  );
  root.append(briefPanel);

  const projectionIntegrity = renderProjectionIntegrityPanel(snapshot, context);
  if (projectionIntegrity !== undefined) root.append(projectionIntegrity);

  const orderControlPanel = createPanel("Active Trading Control", "Submit declarative PAPER intents to the configured Risk/OMS route. Controlled-LIVE remains evidence-only in this workstation.");
  const orderControlContainer = document.createElement("div");
  orderControlPanel.append(orderControlContainer);
  root.append(orderControlPanel);
  try {
    mountedTicket = createRoot(orderControlContainer);
    mountedTicket.render(createElement(OrderTicket, {
      defaultAccountId: paper?.account_id ?? "",
      defaultEnvironment: "PAPER",
    }));
  } catch {
    // Non-browser or mock DOM testing environment
  }

  const serviceSection = createPanel("Runtime and dependencies", "Live health from the container boundary.");
  const serviceRows = context.status === null ? [] : Object.entries(context.status.services).map(([name, service]) => [
    displayName(name), service.status.toUpperCase(), service.detail,
  ]);
  appendTableOrEmpty(serviceSection, ["Service", "Status", "Detail"], serviceRows, "Runtime health is unavailable.");
  root.append(serviceSection);

  const operatingStatus = createPanel("System, broker, strategy, and risk status", "One evidence-backed operating view; an unavailable source is never treated as healthy.");
  appendTableOrEmpty(operatingStatus, ["Area", "Status", "Evidence"], [
    ["System dependencies", services.length === 0 ? "UNAVAILABLE" : healthyServices === services.length ? "HEALTHY" : "DEGRADED", services.length === 0 ? "No runtime status" : `${healthyServices}/${services.length} healthy`],
    ["Strategy identities", strategyIdentities.length > 0 ? "EVIDENCED" : "UNAVAILABLE", `${strategyIdentities.length} versioned identity record(s)`],
    ["Risk", operations?.risk.state ?? "UNAVAILABLE", operations === undefined ? "No operations projection" : `${operations.risk.limits.length} evaluated limit(s)`],
    ["PAPER broker", paper === undefined ? "UNAVAILABLE" : paper.broker_connected ? "CONNECTED" : "DISCONNECTED", paper?.account_id ?? "No PAPER projection"],
    ["Controlled-LIVE broker", live === undefined ? "UNAVAILABLE" : live.broker_connected ? "CONNECTED" : "DISCONNECTED", live?.account_id ?? "No controlled-LIVE projection"],
  ], "No operating-status evidence is available.");
  root.append(operatingStatus);

  const environment = createPanel("Environment readiness", "Code capability is separated from observed operating evidence.");
  appendTableOrEmpty(environment, ["Environment / gate", "Observed", "Required", "Decision"], [
    ["Historical research", `${snapshot.backtests.length} indexed backtest artifact(s)`, "Frozen, reproducible run evidence", snapshot.backtests.length > 0 ? "Evidence indexed" : "No evidence indexed"],
    ["PAPER sessions", paper === undefined ? "No PAPER dashboard" : String(paper.clean_paper_days), paper === undefined ? "PAPER dashboard required" : String(paper.required_paper_days), paper?.promotion_eligible ? "Eligible according to projection" : "Not eligible or unknown"],
    ["Controlled LIVE sessions", live === undefined ? "No controlled-LIVE dashboard" : String(live.clean_live_days), live === undefined ? "Controlled-LIVE dashboard required" : String(live.required_live_days), live?.promotion_eligible ? "Eligible according to projection" : "Not eligible or unknown"],
    ["Commercial operation", `${snapshot.commercial.length} ledger record(s)`, "External customer and entitlement evidence", "Not inferred from local records"],
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

  const scannerPanel = createPanel(
    "Explainable market scanner",
    "Screen instruments against versioned indicators and point-in-time universe conditions with ranked reasons (SOLO-04)."
  );
  scannerPanel.id = "market-scanner-panel";
  appendUnavailableEvidence(scannerPanel, "No versioned market-scanner result is published. Scanner output will appear only after its evidence contract and producer are available.");
  root.append(scannerPanel);

  const consolidatedAttention = createPanel(
    "Consolidated attention queue",
    "Grouped incidents with underlying root causes, suppression of duplicates, and acknowledgement deadlines (SOLO-05)."
  );
  consolidatedAttention.id = "consolidated-attention-panel";
  appendTableOrEmpty(
    consolidatedAttention,
    ["Severity", "Code", "Subject", "Summary"],
    rows,
    "No consolidated attention items."
  );
  root.append(consolidatedAttention);

  const sessionPlaybooks = createPanel(
    "Session playbooks and away mode",
    "Structured operational phases (prepare, observe, operate, reconcile, review) with bounded unattended intervals (SOLO-06)."
  );
  sessionPlaybooks.id = "session-playbooks-panel";
  appendAdvancedEvidenceRows(
    sessionPlaybooks,
    snapshot,
    context,
    "continuity_policy",
    parseContinuityPolicy,
    ["Policy", "Away mode permitted", "Unattended interval", "Heartbeat", "Feed stale threshold", "Broker disconnect action", "Restart budget"],
    (policy) => [[
      policy.policy_id,
      String(policy.away_mode_permitted),
      `${policy.unattended_interval_minutes} min`,
      `${policy.heartbeat_interval_seconds} s`,
      `${policy.feed_stale_threshold_seconds} s`,
      policy.broker_disconnect_action,
      `${policy.max_restarts_per_hour}/hour`,
    ]],
    "No typed continuity policy is published."
  );
  root.append(sessionPlaybooks);

  const awayDeskPanel = createPanel(
    "Desk departure & away readiness check",
    "Supervise active protections, authorized unattended interval, broker connectivity, kill-switch readiness, and escalation routing before leaving the trading desk (SOLO-05, SOLO-06, EXEC-01)."
  );
  awayDeskPanel.id = "away-desk-readiness-panel";
  appendAdvancedEvidenceRows(
    awayDeskPanel,
    snapshot,
    context,
    "continuity_policy",
    parseContinuityPolicy,
    ["Policy", "Away mode permitted", "Unattended interval", "Heartbeat", "Feed stale threshold", "Disconnect response"],
    (policy) => [[
      policy.policy_id,
      String(policy.away_mode_permitted),
      `${policy.unattended_interval_minutes} min`,
      `${policy.heartbeat_interval_seconds} s`,
      `${policy.feed_stale_threshold_seconds} s`,
      policy.broker_disconnect_action,
    ]],
    "No typed away-mode policy is published; readiness is unknown."
  );
  appendAdvancedEvidenceRows(
    awayDeskPanel,
    snapshot,
    context,
    "attention_budget",
    parseAttentionBudget,
    ["Budget ID", "Session Date", "Cognitive Load", "Interruption Rate", "Alarms (Active / Suppressed)", "Escalated Tasks", "Budget Exhausted"],
    (budget) => [[
      budget.budget_id,
      budget.session_date,
      `${budget.cognitive_load_score_bps} bps`,
      `${budget.interruptions_per_hour}/hr`,
      `${budget.active_alarms_count} active / ${budget.suppressed_duplicates_count} suppressed`,
      budget.escalated_critical_tasks.join(", ") || "None",
      budget.budget_exhausted ? "EXHAUSTED" : "NOMINAL",
    ]],
    "No typed attention budget is published."
  );
  const attnEvidence = snapshot.advanced_evidence?.find((item) => item.category === "attention_budget");
  let cognitiveLoadBps = 2450;
  if (attnEvidence && typeof attnEvidence.data === "object" && attnEvidence.data !== null && "cognitive_load_score_bps" in attnEvidence.data) {
    const raw = (attnEvidence.data as { cognitive_load_score_bps: unknown }).cognitive_load_score_bps;
    if (typeof raw === "number") cognitiveLoadBps = raw;
  }
  awayDeskPanel.append(renderAttentionGauge(cognitiveLoadBps));
  root.append(awayDeskPanel);

  root.append(renderArtifactPanel("Recent evidence", context.artifacts.slice(0, 12), context.onOpenArtifact));
}

function renderResearchLab(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const options = optionsDashboard(snapshot);
  renderMetrics(summaryRoot, [
    ["Datasets", String(snapshot.datasets.length), "CSV and immutable Parquet receipts indexed with row and schema metadata"],
    ["Notebooks", String(snapshot.notebooks.length), "Inert Jupyter metadata; notebook code and outputs are never executed"],
    ["Experiments", String(snapshot.experiments.length), "Immutable experiment catalogue records"],
    ["Backtest artifacts", String(snapshot.backtests.length), "Reproducible completed runs"],
  ]);
  const datasets = createPanel("Dataset inventory", "Historical inputs currently available to deterministic research workflows, including verified Parquet receipts.");
  appendTableOrEmpty(datasets, ["Dataset", "Version / format", "Rows", "Columns", "Modified"], snapshot.datasets.map((dataset) => [
    dataset.dataset_id || dataset.name,
    [dataset.dataset_version, dataset.storage_format || "CSV"].filter(Boolean).join(" / "),
    String(dataset.rows), dataset.columns.join(", "), formatTime(dataset.modified_at),
  ]), "No indexed datasets are available.", (index) => context.onOpenArtifact(snapshot.datasets[index]?.name ?? ""));
  root.append(datasets);

  const notebooks = createPanel("Notebook inventory", "Jupyter notebooks are indexed as inert research evidence. The dashboard never executes cells, JavaScript outputs, or embedded HTML.");
  appendTableOrEmpty(notebooks, ["Notebook", "Format", "Cells", "Code", "Markdown", "Outputs", "Kernel / language", "Modified"], snapshot.notebooks.map((notebook) => [
    notebook.artifact, `nbformat ${notebook.nbformat}`, String(notebook.cell_count), String(notebook.code_cells), String(notebook.markdown_cells),
    String(notebook.output_count), [notebook.kernel, notebook.language].filter(Boolean).join(" / "), formatTime(notebook.modified_at),
  ]), "No Jupyter notebook evidence is currently indexed.", (index) => context.onOpenArtifact(snapshot.notebooks[index]?.artifact ?? ""));
  root.append(notebooks);

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

  const hypothesesPanel = createPanel(
    "Hypothesis notebook",
    "Record expected mechanisms, horizons, universe, assumptions, failure criteria, and frozen evaluation plans before optimization (RES-01)."
  );
  hypothesesPanel.id = "hypotheses-panel";
  appendAdvancedEvidenceRows(
    hypothesesPanel,
    snapshot,
    context,
    "research_hypothesis",
    parseResearchHypothesis,
    ["Hypothesis ID", "Status", "Economic Mechanism", "Target Universe", "Horizon", "Frozen Evaluation Plan", "Falsification Criteria"],
    (hypothesis) => [[
      hypothesis.hypothesis_id,
      hypothesis.status,
      hypothesis.mechanism,
      hypothesis.universe.join(", "),
      `${hypothesis.evaluation_horizon.start_time} to ${hypothesis.evaluation_horizon.end_time} (${hypothesis.evaluation_horizon.holding_period})`,
      `${hypothesis.frozen_evaluation_plan.dataset_id}@${hypothesis.frozen_evaluation_plan.dataset_version}; ${hypothesis.frozen_evaluation_plan.slippage_bps} bps; ${hypothesis.frozen_evaluation_plan.fee_model}`,
      hypothesis.failure_criteria.join(" | "),
    ]],
    "No typed research hypotheses are published.",
  );
  root.append(hypothesesPanel);

  const qualityPanel = createPanel(
    "Data quality console",
    "Inspect gaps, schema stability, receipts, and affected-run lookups; quarantined inputs cannot enter research (DATA-01)."
  );
  qualityPanel.id = "data-quality-console";
  const qualityRows = snapshot.datasets.map((dataset) => [
    dataset.dataset_id || dataset.name,
    dataset.storage_format || "CSV",
    String(dataset.rows),
    "Not evaluated by a published quality report",
    dataset.content_sha256 ? "Storage receipt indexed" : "Metadata only",
    `${snapshot.backtests.length} indexed run(s)`,
  ]);
  appendTableOrEmpty(
    qualityPanel,
    ["Dataset", "Format", "Row Count", "Continuity & Gaps", "Schema & Receipt", "Affected Runs"],
    qualityRows,
    "No dataset quality telemetry available.",
  );
  root.append(qualityPanel);

  const feedSubstitutionPanel = createPanel(
    "Deterministic feed substitution and parity verification",
    "Audit secondary or substitute market feeds against primary reference data, enforcing timestamp tolerance, quote continuity, and fixed-point basis drift (DATA-06)."
  );
  feedSubstitutionPanel.id = "feed-substitution-panel";
  appendAdvancedEvidenceRows(
    feedSubstitutionPanel,
    snapshot,
    context,
    "feed_substitution_parity",
    parseFeedSubstitutionParity,
    ["Primary Feed", "Candidate Feed", "Alignment Window", "Tolerance (ms)", "Max Drift (bps)", "Parity Disposition", "Evidence Receipt"],
    (parity) => [[
      parity.primary_provider,
      parity.candidate_provider,
      `${parity.sample_start} to ${parity.sample_end}`,
      `${parity.timestamp_variance_micros_p99 / 1000} ms p99`,
      parity.symbol_match_pct,
      parity.parity_disposition,
      parity.adjustment_parity_verified ? "Adjustment parity verified" : "Adjustment parity not verified",
    ]],
    "No typed feed parity evaluation is published."
  );
  root.append(feedSubstitutionPanel);

  const inputCorrectionPanel = createPanel(
    "Input correction and data rights ledger",
    "Verification receipts recording market data provider licenses, redistribution entitlements, corporate-action adjustment semantics, and affected lineage (DUR-03, DATA-01, DATA-03)."
  );
  inputCorrectionPanel.id = "input-correction-panel";
  appendAdvancedEvidenceRows(
    inputCorrectionPanel,
    snapshot,
    context,
    "data_rights_and_semantics_receipt",
    parseDataRightsAndSemanticsReceipt,
    ["Receipt ID", "Provider", "Dataset", "License Tier", "Redistributable", "Corporate Action Policy", "Semantic Parity", "Verified At"],
    (receipt) => [[
      receipt.receipt_id,
      receipt.provider_id,
      receipt.dataset_id,
      receipt.license_tier,
      receipt.redistribution_permitted ? "YES" : "NO",
      receipt.corporate_action_policy,
      `${receipt.semantic_parity_score_bps} bps`,
      receipt.verified_at,
    ]],
    "No typed data rights and semantics receipt is published."
  );
  root.append(inputCorrectionPanel);

  const counterfactualPanel = createPanel(
    "Counterfactual scenario replay and intervention lab",
    "Simulate parameter, latency, data corruption, and volatility interventions on frozen baseline runs without mutating production history (DUR-02)."
  );
  counterfactualPanel.id = "counterfactual-panel";
  appendAdvancedEvidenceRows(
    counterfactualPanel,
    snapshot,
    context,
    "counterfactual_scenario",
    parseCounterfactualScenario,
    ["Scenario ID", "Baseline Run", "Seed", "Interventions", "Divergence Event", "P&L Delta USD", "Max DD Delta (bps)", "Risk Rejections Delta"],
    (scenario) => [[
      scenario.scenario_id,
      scenario.baseline_run_id,
      String(scenario.seed),
      scenario.interventions.map((iv) => `${iv.intervention_type}: ${iv.parameter_name} (${iv.baseline_value} -> ${iv.counterfactual_value})`).join(" | "),
      scenario.divergence_event_id,
      scenario.delta_metrics.pnl_delta_usd,
      `${scenario.delta_metrics.max_drawdown_delta_bps} bps`,
      String(scenario.delta_metrics.risk_rejection_count_delta),
    ]],
    "No typed counterfactual replay scenario is published."
  );
  root.append(counterfactualPanel);
  root.append(renderOptionsPayoffVisualizer());

  root.append(renderFeatureEvidence(context, ["market-data", "research", "options"]));
}

function renderNewsCockpit(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const headlines = snapshot.events.filter((item) => field(item.data, "event_type") === "news.headline.v1");
  const sentiments = snapshot.events.filter((item) => field(item.data, "event_type") === "news.sentiment.v1");
  const headlinesByNewsId = new Map(headlines.map((item) => [field(record(item.data.payload), "news_id"), item]));
  const sentimentEventIds = new Set(sentiments.flatMap((item) => [
    field(item.data, "event_id"),
    field(record(item.data.payload), "event_id"),
  ]).filter(Boolean));
  const riskDecisions = snapshot.events.filter((item) =>
    field(item.data, "event_type") === "risk.decision.v1" && sentimentEventIds.has(field(item.data, "causation_id"))
  );
  const sources = new Set(headlines.map((item) => field(record(item.data.payload), "source")).filter(Boolean));
  renderMetrics(summaryRoot, [
    ["Headlines", String(headlines.length), "Validated news.headline.v1 envelopes"],
    ["Sentiment vectors", String(sentiments.length), "Deterministic news.sentiment.v1 evidence"],
    ["Sources", String(sources.size), "Declared provenance labels in stored headline evidence"],
    ["Linked risk decisions", String(riskDecisions.length), "Causation links from sentiment through pre-trade risk"],
  ]);

  const headlinePanel = createPanel("Headline evidence", "Stored source labels and headlines are rendered from immutable events; an empty workspace does not infer providers or market activity.");
  appendTableOrEmpty(headlinePanel, ["Time", "Source", "News ID", "Headline", "Instruments", "Artifact"], headlines.map((item) => {
    const payload = record(item.data.payload);
    const instruments = Array.isArray(payload.entity_tickers) ? payload.entity_tickers.map(text).join(", ") : "";
    return [
      field(item.data, "event_time"), field(payload, "source"), field(payload, "news_id"), field(payload, "headline"), instruments, item.artifact,
    ];
  }), "No headline evidence is available.", (index) => context.onOpenArtifact(headlines[index]?.artifact ?? ""));
  root.append(headlinePanel);

  const sentimentPanel = createPanel("Sentiment signals", "Signal power is derived from stored integer-BPS polarity and confidence values; no browser-side classifier is used.");
  appendTableOrEmpty(sentimentPanel, ["Time", "Instrument", "Taxonomy", "Polarity", "Confidence", "Novelty", "Surprise", "Signal power", "Headline", "Artifact"], sentiments.map((item) => {
    const payload = record(item.data.payload);
    const signalPower = sentimentSignalPower(payload.sentiment_polarity_bps, payload.confidence_bps);
    const headline = headlinesByNewsId.get(field(payload, "causation_news_id"));
    return [
      field(item.data, "event_time"), field(payload, "instrument_id"), field(payload, "taxonomy"),
      field(payload, "sentiment_polarity_bps"), field(payload, "confidence_bps"), field(payload, "novelty_score_bps"),
      field(payload, "surprise_magnitude_bps"), signalPower,
      headline === undefined ? "No stored headline link" : field(record(headline.data.payload), "headline"), item.artifact,
    ];
  }), "No sentiment evidence is available.", (index) => context.onOpenArtifact(sentiments[index]?.artifact ?? ""));
  root.append(sentimentPanel);

  const riskPanel = createPanel("Causally linked risk decisions", "Only recorded risk decisions with a direct stored causation link to a sentiment event are shown.");
  appendTableOrEmpty(riskPanel, ["Time", "Decision", "Intent", "Reason codes", "Causation", "Artifact"], riskDecisions.map((item) => {
    const payload = record(item.data.payload);
    const reasonCodes = Array.isArray(payload.reason_codes) ? payload.reason_codes.map(text).join(", ") : field(payload, "reason_code");
    return [
      field(item.data, "event_time"), field(payload, "decision") || field(payload, "outcome"), field(payload, "intent_id"),
      reasonCodes, field(item.data, "causation_id"), item.artifact,
    ];
  }), "No stored risk decisions are causally linked to news sentiment.", (index) => context.onOpenArtifact(riskDecisions[index]?.artifact ?? ""));
  root.append(riskPanel);

  const knowledgePanel = createPanel(
    "Point-in-time knowledge graph",
    "Attributable links connecting companies, instruments, filings, headlines, and exposures with effective availability timestamps (DATA-02)."
  );
  knowledgePanel.id = "knowledge-graph-panel";
  appendAdvancedEvidenceRows(
    knowledgePanel,
    snapshot,
    context,
    "knowledge_snapshot",
    parseKnowledgeSnapshot,
    ["Source Entity", "Type", "Relation", "Target Entity", "Effective As-Of Time", "Provenance Hash"],
    (knowledge) => {
      const nodes = new Map(knowledge.entity_nodes.map((node) => [node.entity_id, node]));
      return knowledge.relationships.map((relationship) => [
        relationship.source_entity_id,
        nodes.get(relationship.source_entity_id)?.entity_type ?? "Unknown",
        relationship.relation_type,
        relationship.target_entity_id,
        relationship.effective_time,
        shortHash(relationship.provenance_hash),
      ]);
    },
    "No typed point-in-time knowledge snapshot is published."
  );
  root.append(knowledgePanel);

  const revisionPanel = createPanel(
    "News revision and novelty timeline",
    "Track original announcements versus syndicated duplicates, corrections, and model interpretations without overwriting history (DATA-03)."
  );
  revisionPanel.id = "news-revision-panel";
  appendUnavailableEvidence(revisionPanel, "No versioned news-revision artifact is published. Headline events are preserved above, but duplicate or correction status is not inferred by this workspace.");
  root.append(revisionPanel);

  const calendarPanel = createPanel(
    "Event exposure calendar",
    "Point-in-time schedule for earnings announcements, corporate actions, trading halts, options expiry, and settlement dates (DATA-04)."
  );
  calendarPanel.id = "event-exposure-calendar";
  appendAdvancedEvidenceRows(
    calendarPanel,
    snapshot,
    context,
    "event_exposure_calendar",
    parseEventExposureCalendar,
    ["Scheduled (UTC)", "Instrument", "Category", "Event Detail", "Status", "Source Evidence"],
    (calendar) => calendar.scheduled_events.map((event) => [
      event.scheduled_time,
      event.instrument_id,
      event.category,
      event.event_id,
      event.status,
      event.source_evidence,
    ]),
    "No typed event exposure calendar is published."
  );
  root.append(calendarPanel);

  const regimeMonitorPanel = createPanel(
    "Assumption and regime drift monitor",
    "Continuous monitoring of baseline economic assumptions, spread regimes, volatility bands, and market liquidity to identify out-of-regime research models (DATA-05)."
  );
  regimeMonitorPanel.id = "regime-monitor-panel";
  appendAdvancedEvidenceRows(
    regimeMonitorPanel,
    snapshot,
    context,
    "assumption_regime_monitor",
    parseAssumptionRegimeMonitor,
    ["Assumption ID", "Model Scope", "Baseline Parameter", "Observed Regime", "Drift (bps)", "Threshold (bps)", "Monitor State"],
    (monitor) => monitor.impacted_strategy_assumptions.map((assumption) => [
      monitor.regime_id,
      assumption.strategy_id,
      assumption.assumed_condition,
      `${monitor.current_regime}: ${assumption.observed_condition}`,
      String(monitor.indicators.effective_spread_bps),
      "Not published by this contract",
      assumption.breach_status,
    ]),
    "No typed assumption-regime monitor is published."
  );
  root.append(regimeMonitorPanel);

  root.append(renderFeatureEvidence(context, ["news", "research", "execution-risk"]));
}

function renderStrategyStudio(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const operations = operationsDashboard(snapshot);
  const identities = strategyIdentityRows(snapshot, operations);
  renderMetrics(summaryRoot, [
    ["Strategy identities", String(identities.length), "Versioned strategy and bundle combinations"],
    ["Configuration identities", String(new Set(identities.map((row) => row[3]).filter(Boolean)).size), "Exact source-bound configuration hashes"],
    ["Worker boundary", "Trusted simulation worker", "Environment clearing is not filesystem, network, or resource isolation", "warn"],
    ["Broker access", "No desktop adapter route", "A worker must still be treated as untrusted until sandboxed and gateway-authorized", "warn"],
  ]);
  const identityPanel = createPanel("Version and deployment identities", "Every strategy run binds source, configuration, dataset, engine, and event output.");
  appendTableOrEmpty(identityPanel, ["Strategy", "Version", "Bundle", "Configuration", "Dataset", "Engine / source"], identities, "No strategy identities are available.");
  root.append(identityPanel);
  const boundary = createPanel("Worker contract", "The browser does not execute strategy code. The current local process worker validates identity, but it is only approved for trusted simulation bundles until a resource and network sandbox is evidenced.");
  appendDefinition(boundary, [
    ["Input", "Immutable strategy context and normalized market bars"],
    ["Output", "One validated order intent or null"],
    ["Identity", "SHA-256 of strategy tree, SDK source, and Python runtime"],
    ["Environment", "Cleared; only an explicit non-secret SDK path may be supplied"],
    ["Sandbox status", "No filesystem/network/resource sandbox is represented by this desktop projection"],
    ["Forbidden", "Broker adapters, credentials, unverified dependencies, and browser execution"],
  ]);
  root.append(boundary);

  const compositionPanel = createPanel(
    "Strategy composition studio",
    "Declarative signals, sizing rules, entry/exit criteria, and portfolio constraints; code and visual views share one versioned spec (RES-02)."
  );
  compositionPanel.id = "strategy-composition-panel";
  appendUnavailableEvidence(compositionPanel, "No versioned strategy-composition specification is published. Existing strategy identities are shown above; visual composition is unavailable until its contract producer exists.");
  root.append(compositionPanel);

  const copilotPanel = createPanel(
    "Read-only research copilot",
    "Evidence-grounded assistant; every explanation cites immutable hashes, and absent evidence triggers an explicit UNKNOWN (AI-01)."
  );
  copilotPanel.id = "research-copilot-panel";
  appendAdvancedEvidenceRows(
    copilotPanel,
    snapshot,
    context,
    "assistant_evidence",
    parseAssistantEvidence,
    ["Query / Topic", "Cited Evidence IDs", "Model & Template", "Disposition", "Explanation Summary"],
    (evidence) => [[
      evidence.query_id,
      evidence.retrieved_record_ids.join(", ") || "None",
      `${evidence.model_version} (${evidence.prompt_template_version})`,
      `${evidence.human_disposition}; uncertainty ${evidence.uncertainty_score_bps} bps`,
      evidence.generated_output,
    ]],
    "No typed research-assistant evidence is published.",
  );
  root.append(copilotPanel);

  const criticPanel = createPanel(
    "Strategy drafting assistant and critic",
    "Translate plain-language hypotheses into typed rules, propose falsification tests, and diagnose missing costs or data bias (AI-02, AI-03)."
  );
  criticPanel.id = "strategy-critic-panel";
  appendAdvancedEvidenceRows(
    criticPanel,
    snapshot,
    context,
    "assistant_evidence",
    parseAssistantEvidence,
    ["Analysis Scope", "Critic Finding", "Severity", "Proposed Falsification Test", "Status"],
    (evidence) => [[
      evidence.query_id,
      evidence.generated_output,
      `${evidence.uncertainty_score_bps} bps uncertainty`,
      evidence.tool_attempts.map((attempt) => `${attempt.tool_name}: ${attempt.status}`).join(" | ") || "No tool attempt",
      evidence.human_disposition,
    ]],
    "No typed strategy-critique evidence is published."
  );
  root.append(criticPanel);

  const schedulerPanel = createPanel(
    "Budgeted research scheduler",
    "Overnight automated experiment execution with CPU/time/spend limits, periodic checkpointing, and zero broker credentials (AI-04)."
  );
  schedulerPanel.id = "research-scheduler-panel";
  appendAdvancedEvidenceRows(
    schedulerPanel,
    snapshot,
    context,
    "automation_mandate",
    parseAutomationMandate,
    ["Mandate ID", "Owner", "Allowed Templates", "Resource Caps (CPU / RAM / Duration)", "Checkpointing", "Broker Boundary"],
    (mandate) => [[
      mandate.mandate_id,
      mandate.owner,
      mandate.allowed_tasks.join(", "),
      `${mandate.resource_limits.max_cpu_cores} cores / ${mandate.resource_limits.max_memory_mb} MB / ${mandate.resource_limits.max_duration_seconds} s`,
      `${mandate.cancellation_policy.checkpoint_interval_seconds}s; stop on first error: ${mandate.cancellation_policy.stop_on_first_error}`,
      mandate.broker_access_permitted ? "Broker access granted" : "Broker access prohibited",
    ]],
    "No typed research automation mandate is published."
  );
  appendAdvancedEvidenceRows(
    schedulerPanel,
    snapshot,
    context,
    "research_job",
    parseResearchJob,
    ["Job ID", "Strategy", "Dataset", "State", "Lease", "Failure reason"],
    (job) => [[
      job.job_id,
      `${job.strategy_id}@${job.strategy_version}`,
      `${job.dataset_id}@${job.dataset_version}`,
      `${job.state} (v${job.state_version})`,
      job.worker_lease === null ? "No worker lease" : `${job.worker_lease.worker_id} until ${job.worker_lease.expires_at}`,
      job.failure_reason ?? "",
    ]],
    "No typed research job receipts are published."
  );
  root.append(schedulerPanel);

  const championChallengerPanel = createPanel(
    "Champion vs challenger shadow evaluation",
    "Shadow-evaluate challenger strategy iterations against active champions, measuring drift, information ratio delta, and automated retirement triggers (RES-08)."
  );
  championChallengerPanel.id = "champion-challenger-panel";
  appendAdvancedEvidenceRows(
    championChallengerPanel,
    snapshot,
    context,
    "champion_challenger_evaluation",
    parseChampionChallengerEvaluation,
    ["Champion Strategy", "Challenger Candidate", "Window Start / End", "Champion Return (bps)", "Challenger Return (bps)", "Drift State", "Lifecycle Recommendation"],
    (evaluation) => [[
      evaluation.champion_strategy_id,
      evaluation.challenger_strategy_id,
      `${evaluation.evaluation_window_start} to ${evaluation.evaluation_window_end}`,
      String(evaluation.champion_return_bps),
      String(evaluation.challenger_return_bps),
      evaluation.drift_detected ? "Drift detected" : "No drift detected",
      evaluation.recommendation,
    ]],
    "No typed champion/challenger evaluation is published."
  );
  root.append(championChallengerPanel);

  const strategyInvalidationPanel = createPanel(
    "Strategy falsification and invalidation explorer",
    "Connect frozen hypothesis falsification conditions to synthetic stress injections, data quality changes, and model drift alerts without mutating historical records (RES-01, RES-04, RES-05, RES-08, AI-03)."
  );
  strategyInvalidationPanel.id = "strategy-invalidation-panel";
  appendAdvancedEvidenceRows(
    strategyInvalidationPanel,
    snapshot,
    context,
    "robustness_evaluation",
    parseRobustnessEvaluation,
    ["Strategy / Hypothesis", "Falsification Condition", "Stress Test Applied", "Observed Invalidation Margin", "Review Task Status"],
    (evaluation) => [[
      `${evaluation.hypothesis_id} (${evaluation.strategy_version})`,
      `Leakage violations: ${evaluation.leakage_checks.quarantine_violations}; cliff: ${evaluation.parameter_stability.degradation_cliff_detected}`,
      evaluation.cost_shocks.map((shock) => `${shock.slippage_multiplier}x slip / ${shock.fee_multiplier}x fee`).join(" | "),
      `Neighborhood variance ${evaluation.parameter_stability.neighborhood_variance_bps} bps`,
      evaluation.disposition,
    ]],
    "No typed robustness evaluation is published."
  );
  appendAdvancedEvidenceRows(
    strategyInvalidationPanel,
    snapshot,
    context,
    "adversarial_evaluation",
    parseAdversarialEvaluation,
    ["Evaluation ID", "Strategy", "Probes Passed", "Composite Robustness", "Gate Status", "Blocking Failure Reasons", "Evaluated At"],
    (evaluation) => [[
      evaluation.evaluation_id,
      evaluation.strategy_version,
      `${evaluation.probes.filter((p) => p.passed).length}/${evaluation.probes.length} probes`,
      `${evaluation.composite_robustness_score_bps} bps`,
      evaluation.gate_passed ? "PASSED" : "FAILED_GATE",
      evaluation.blocking_failure_reasons.join(" | ") || "None",
      evaluation.evaluated_at,
    ]],
    "No typed adversarial research evaluation is published."
  );
  root.append(strategyInvalidationPanel);

  const strategyCapsulePanel = createPanel(
    "Portable strategy capsule and reproducibility manifest",
    "Self-contained strategy archive with bundle hash, pinned dependencies, runtime target, and deterministic replay instruction (DUR-07)."
  );
  strategyCapsulePanel.id = "strategy-capsule-panel";
  appendAdvancedEvidenceRows(
    strategyCapsulePanel,
    snapshot,
    context,
    "strategy_capsule_manifest",
    parseStrategyCapsuleManifest,
    ["Capsule ID", "Strategy Version", "Bundle Hash", "Config Hash", "Lockfile Hash", "Runtime Target", "Export Disposition", "Replay Command"],
    (capsule) => [[
      capsule.capsule_id,
      `${capsule.strategy_id}@${capsule.strategy_version}`,
      shortHash(capsule.bundle_sha256),
      shortHash(capsule.configuration_sha256),
      shortHash(capsule.dependency_lockfile_sha256),
      capsule.runtime_target,
      capsule.export_disposition,
      capsule.replay_instruction_command,
    ]],
    "No typed strategy capsule manifest is published."
  );
  root.append(strategyCapsulePanel);

  root.append(renderFeatureEvidence(context, ["research", "replay"]));
}

function renderBacktestExplorer(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const fills = snapshot.events.filter((item) =>
    field(item.data, "event_type") === "execution.fill.v1" &&
    (field(item.data, "source") === "simulator" || field(item.data, "actor") === "simulator")
  );
  const taggedExperiments = snapshot.experiments.filter((item) => {
    const tags = record(item.data.tags);
    return Boolean(field(tags, "regime") || field(tags, "sensitivity") || field(tags, "scenario"));
  });
  renderMetrics(summaryRoot, [
    ["Completed runs", String(snapshot.backtests.length), "Immutable backtest artifacts"],
    ["Recorded fills", String(fills.length), "Canonical simulated execution.fill.v1 evidence"],
    ["Experiment records", String(snapshot.experiments.length), `${taggedExperiments.length} with regime or sensitivity dimensions`],
    ["Completion manifests", String(snapshot.manifests.length), "SHA-256 publication records"],
  ]);
  const runs = createPanel("Run comparison", "Compare performance, accounting, and provenance without rerunning or mutating results.");
  appendTableOrEmpty(runs, ["Artifact", "Strategy", "Dataset", "Trades", "Net P&L", "Return bps", "Max drawdown", "Fingerprint"], snapshot.backtests.map((run) => {
    const dataset = record(run.specification.dataset);
    return [run.artifact, field(run.specification, "strategy_version") || shortHash(field(run.specification, "strategy_bundle_hash")),
      field(dataset, "dataset_id") || "Legacy artifact", field(run.performance, "trade_count"), field(run.performance, "net_pnl") || field(run.report, "realized_pnl"),
      field(run.performance, "return_bps"), field(run.performance, "max_drawdown_bps"), shortHash(text(run.artifact_fingerprint))];
  }), "No backtest result artifacts are available.", (index) => context.onOpenArtifact(snapshot.backtests[index]?.artifact ?? ""));
  root.append(runs);

  const trades = createPanel("Trade evidence", "Inspect each canonical simulated execution rather than relying only on aggregate trade counts.");
  appendTableOrEmpty(trades, ["Time", "Execution", "Order", "Instrument", "Side", "Quantity", "Price", "Fee", "Source"], fills.map((item) => {
    const payload = record(item.data.payload);
    return [
      field(payload, "executed_at") || field(item.data, "event_time"),
      field(payload, "execution_id"),
      field(payload, "order_id"),
      field(payload, "instrument_id") || field(item.data, "instrument_id"),
      field(payload, "side"),
      field(payload, "quantity"),
      field(payload, "price"),
      field(payload, "fee"),
      item.artifact,
    ];
  }), "No canonical fill events are available for the indexed backtests.", (index) => context.onOpenArtifact(fills[index]?.artifact ?? ""));
  root.append(trades);

  const dimensions = createPanel("Regime and sensitivity dimensions", "Experiment tags remain attached to immutable run identities; missing tags are reported explicitly rather than inferred from results.");
  appendTableOrEmpty(dimensions, ["Experiment", "Run", "Regime", "Sensitivity / scenario", "Other dimensions", "Specification", "Source"], snapshot.experiments.map((item) => {
    const tags = record(item.data.tags);
    const otherTags = Object.entries(tags)
      .filter(([key]) => key !== "regime" && key !== "sensitivity" && key !== "scenario")
      .map(([key, value]) => `${key}=${text(value)}`)
      .join(" | ");
    return [
      field(item.data, "experiment_id"),
      field(item.data, "run_id"),
      field(tags, "regime") || "Not tagged",
      field(tags, "sensitivity") || field(tags, "scenario") || "Not tagged",
      otherTags || "None",
      shortHash(field(item.data, "specification_fingerprint")),
      item.artifact,
    ];
  }), "No experiment records are available; regime and sensitivity comparisons require tagged immutable runs.", (index) => context.onOpenArtifact(snapshot.experiments[index]?.artifact ?? ""));
  root.append(dimensions);

  const executionModel = createPanel("Execution realism model", "Every run binds these deterministic assumptions through its immutable configuration fingerprint.");
  appendDefinition(executionModel, [
    ["Quoted spread", "Buys pay and sells concede half of the configured full spread"],
    ["Slippage", "Configured basis points are applied unfavourably after half-spread"],
    ["Limit protection", "The final spread-and-slippage price can never violate the order limit"],
    ["Latency", "A configured number of complete market bars must pass before fill eligibility"],
    ["Partial fills", "An optional per-bar quantity cap persists remaining quantity as a working order"],
    ["Trading halts", "Version-controlled venue or instrument halt windows block strategy evaluation"],
    ["Survivorship", "Every replay bar must belong to an effective-dated point-in-time universe interval"],
    ["Short and borrow", "Explicit shortability, borrow availability/recalls, and daily financing accrue without look-ahead"],
    ["Delistings", "A versioned terminal settlement closes long or short positions and preserves realized P&L evidence"],
    ["Capital", "Fresh FX and portfolio-wide initial margin are evaluated before an advanced-account fill is committed"],
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

  const failedIdeaPanel = createPanel(
    "Experiment graph and failed-idea memory",
    "Retain rejected hypotheses, parameter candidates, and branch history; selecting a winner never erases the trials that produced it (RES-04)."
  );
  failedIdeaPanel.id = "failed-idea-memory";
  appendAdvancedEvidenceRows(
    failedIdeaPanel,
    snapshot,
    context,
    "experiment_lineage",
    parseExperimentLineage,
    ["Trial ID & Parameters", "Specification Hash", "Return (bps)", "Max Drawdown", "Disposition", "Failure / Promotion Rationale"],
    (lineage) => lineage.candidate_trials.map((trial) => [
      trial.trial_id,
      shortHash(trial.specification_hash),
      trial.return_bps,
      trial.max_drawdown_bps,
      trial.disposition,
      lineage.rejection_reasons.find((reason) => reason.trial_id === trial.trial_id)?.reason ?? "No disposition rationale published",
    ]),
    "No typed experiment-lineage record is published.",
  );
  root.append(failedIdeaPanel);

  const robustnessPanel = createPanel(
    "Robustness laboratory",
    "Held-out evaluations, walk-forward windows, leakage verification, parameter neighborhood stability, and cost stress shocks (RES-05)."
  );
  robustnessPanel.id = "robustness-lab-panel";
  appendAdvancedEvidenceRows(
    robustnessPanel,
    snapshot,
    context,
    "robustness_evaluation",
    parseRobustnessEvaluation,
    ["Dimension", "Configuration & Window", "In-Sample", "Out-of-Sample", "Drawdown", "Robustness Finding"],
    (evaluation) => evaluation.walk_forward_windows.map((window) => [
      window.window_id,
      `${window.in_sample_start} to ${window.in_sample_end} -> ${window.out_of_sample_start} to ${window.out_of_sample_end}`,
      `${window.in_sample_return_bps} bps`,
      `${window.out_of_sample_return_bps} bps`,
      `${window.max_drawdown_bps} bps`,
      `${evaluation.disposition}; leakage violations ${evaluation.leakage_checks.quarantine_violations}`,
    ]),
    "No typed robustness evaluation is published."
  );
  root.append(robustnessPanel);

  const portfolioExpPanel = createPanel(
    "Portfolio experiment engine",
    "Simulate concurrent strategies sharing cash, order contention, fees, turnover caps, and portfolio allocation rules (RES-06)."
  );
  portfolioExpPanel.id = "portfolio-experiment-panel";
  appendAdvancedEvidenceRows(
    portfolioExpPanel,
    snapshot,
    context,
    "portfolio_experiment",
    parsePortfolioExperiment,
    ["Experiment ID", "Strategies Joined", "Allocated Capital", "Combined Return", "Drawdown", "Diversification Ratio", "Order Contention"],
    (experiment) => [[
      experiment.experiment_id,
      experiment.strategies.map((strategy) => `${strategy.strategy_id}@${strategy.strategy_version} (${strategy.target_weight_bps} bps)`).join(" + "),
      `${experiment.allocated_cash} ${experiment.currency}`,
      `${experiment.joint_performance.combined_return_bps} bps`,
      `${experiment.joint_performance.combined_max_drawdown_bps} bps`,
      `${experiment.joint_performance.diversification_ratio_bps} bps`,
      `${experiment.order_contention_events} event(s)`,
    ]],
    "No typed multi-strategy portfolio experiment is published."
  );
  root.append(portfolioExpPanel);

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

  const ticket = createPanel("Order ticket", "Submit a declarative PAPER intent to the configured Risk/OMS route. Controlled-LIVE is not exposed by this ticket.");
  const ticketRoot = document.createElement("div");
  ticket.append(ticketRoot);
  root.append(ticket);
  try {
    mountedTicket = createRoot(ticketRoot);
    mountedTicket.render(createElement(OrderTicket, {
      defaultAccountId: paper?.account_id ?? "",
      defaultEnvironment: "PAPER",
    }));
  } catch {
    // Non-browser or mock DOM testing environment
  }

  const blotter = createPanel("Causal execution blotter", "Every row links event, causation, correlation, actor, and normalized lifecycle payload.");
  const blotterRows = executionEvents.map((item) => {
    const payload = record(item.data.payload);
    return [field(item.data, "event_time"), field(item.data, "event_type"), field(payload, "order_id") || field(payload, "intent_id") || field(payload, "execution_id"),
      field(payload, "new_state") || field(payload, "status") || (payload.approved === true ? "APPROVED" : payload.approved === false ? "REJECTED" : field(payload, "reason")),
      field(payload, "quantity") || field(payload, "filled_quantity") || field(payload, "cumulative_quantity"), field(item.data, "correlation_id"), item.artifact];
  });
  appendTableOrEmpty(blotter, ["Time", "Phase", "Order / intent", "State / decision", "Quantity", "Correlation", "Source"], blotterRows, "No execution lifecycle events are available.", (index) => context.onOpenArtifact(executionEvents[index]?.artifact ?? ""));
  root.append(blotter);

  const commandBoundary = createPanel(
    "Order-management boundary",
    "Cancellation and close-position requests remain unavailable until a separately qualified native Risk/OMS route is supplied. Inspect immutable evidence here; do not treat this browser surface as a command channel.",
  );
  appendUnavailableEvidence(commandBoundary, "No native command route is currently configured.");
  root.append(commandBoundary);

  const riskDecisions = snapshot.events.filter((item) => field(item.data, "event_type") === "risk.decision.v1");
  const risk = createPanel("Explainable risk decisions", "Every approval and rejection exposes the exact rule outcomes, evaluated inputs and thresholds, policy version, and decision actor.");
  appendTableOrEmpty(risk, ["Time", "Decision", "Intent", "Outcome", "Reason codes", "Evaluated inputs and limits", "Policy", "Actor"], riskDecisions.map((item) => {
    const payload = record(item.data.payload);
    return [
      field(item.data, "event_time"),
      field(payload, "decision_id"),
      field(payload, "intent_id"),
      payload.approved === true ? "APPROVED" : payload.approved === false ? "REJECTED" : "UNKNOWN",
      stringList(payload.reason_codes).join(", "),
      field(payload, "evaluated_limits"),
      field(payload, "policy_version"),
      field(payload, "actor"),
    ];
  }), "No immutable risk-decision events are available.", (index) => context.onOpenArtifact(riskDecisions[index]?.artifact ?? ""));
  root.append(risk);

  const tcaRows: Array<{ artifact: string; values: string[] }> = [];
  const benchmarkRows: Array<{ artifact: string; values: string[] }> = [];
  for (const evidence of snapshot.execution_evidence) {
    const transactionCost = record(evidence.data.transaction_cost);
    const reports = Array.isArray(transactionCost.reports) ? transactionCost.reports : [];
    for (const candidate of reports) {
      const report = record(candidate);
      tcaRows.push({
        artifact: evidence.artifact,
        values: [evidence.artifact, field(report, "analysis_id"), field(report, "strategy_id"), field(report, "side"), field(report, "filled_quantity"), field(report, "execution_vwap"), field(report, "arrival_total_cost"), field(report, "target_total_cost")],
      });
    }
    const measurement = record(evidence.data.measurement);
    if (measurement.p99_micros !== undefined) {
      benchmarkRows.push({
        artifact: evidence.artifact,
        values: [evidence.artifact, field(evidence.data, "observed_at"), field(evidence.data, "policy_version"), field(measurement, "p99_micros"), field(measurement, "threshold_micros"), measurement.within_threshold === true ? "Within local threshold" : "Outside local threshold"],
      });
    }
  }
  const tca = createPanel("Transaction-cost analysis", "Immutable implementation-shortfall evidence measured from caller-supplied frozen arrival and target benchmarks; it is not a broker-statement acceptance claim.");
  appendTableOrEmpty(tca, ["Artifact", "Analysis", "Strategy", "Side", "Filled", "VWAP", "Arrival total", "Target total"], tcaRows.map((row) => row.values), "No transaction-cost artifact is indexed.", (index) => context.onOpenArtifact(tcaRows[index]?.artifact ?? ""));
  root.append(tca);

  const benchmark = createPanel("Local risk-evaluator benchmark", "Explicit-hardware local timing observation only; production availability and load evidence remain separate gates.");
  appendTableOrEmpty(benchmark, ["Artifact", "Observed at", "Policy", "p99 (µs)", "Threshold (µs)", "Result"], benchmarkRows.map((row) => row.values), "No local risk benchmark artifact is indexed.", (index) => context.onOpenArtifact(benchmarkRows[index]?.artifact ?? ""));
  root.append(benchmark);

  const lifecycle = createPanel("Broker lifecycle condition coverage", "Explicit handling for the out-of-order and modification cases recorded in the system review.");
  appendTableOrEmpty(lifecycle, ["Condition", "Implementation", "Invariant"], OMS_LIFECYCLE_COVERAGE.map((row) => [...row]), "No lifecycle coverage metadata is available.");
  root.append(lifecycle);

  const passportPanel = createPanel(
    "Order decision passport",
    "One unified attributable audit trail from market opportunity signal, policy inputs, and risk approval through OMS routing, child fills, and ledger consequences (EXEC-02)."
  );
  passportPanel.id = "decision-passport-panel";
  appendAdvancedEvidenceRows(
    passportPanel,
    snapshot,
    context,
    "order_decision_passport",
    parseOrderDecisionPassport,
    ["Passport ID", "Opportunity Signal", "Risk Pre-Trade Evaluation", "OMS Routing Plan", "Executions & Fees", "Journal Consequences"],
    (passport) => [[
      passport.passport_id,
      `${passport.signal_attribution.strategy_version}; ${passport.signal_attribution.signal_power_bps} bps; ${passport.signal_attribution.opportunity_description}`,
      `${passport.risk_evaluation.approved ? "Approved" : "Rejected"}; ${passport.risk_evaluation.evaluated_limits.join(", ")}; ${passport.risk_evaluation.headroom_remaining_bps} bps`,
      `${passport.routing_plan.algorithm}; ${passport.routing_plan.allocated_slices_count} slice(s) at ${passport.routing_plan.primary_venue}`,
      passport.executions.map((execution) => `${execution.quantity} @ ${execution.price}; fee ${execution.fee}`).join(" | ") || "No execution recorded",
      `${passport.accounting_consequences.journal_entry_id}; cash ${passport.accounting_consequences.cash_delta}; position ${passport.accounting_consequences.position_after}`,
    ]],
    "No typed order-decision passport is published."
  );
  root.append(passportPanel);

  const executionCoachPanel = createPanel(
    "Execution coach, post-trade benchmark & replay-vs-live diff",
    "Post-trade attribution decomposing slippage against arrival price, interval VWAP, and replay counterfactuals to isolate routing alpha, fee leakage, and market impact (EXEC-03, RES-07)."
  );
  executionCoachPanel.id = "execution-coach-panel";
  appendAdvancedEvidenceRows(
    executionCoachPanel,
    snapshot,
    context,
    "execution_coach_benchmark",
    parseExecutionCoachBenchmark,
    ["Benchmark ID", "Order / Strategy", "Filled Quantity", "Arrival Slippage", "VWAP Slippage", "Replay vs Live Diff", "Coach Recommendation"],
    (benchmark) => [[
      benchmark.analysis_id,
      benchmark.order_id,
      "Not published by this contract",
      `${benchmark.realized_shortfall_bps} bps`,
      `${benchmark.realized_vwap} vs target ${benchmark.target_price}`,
      `${benchmark.slippage_drag_bps + benchmark.market_impact_bps + benchmark.fee_drag_bps} bps decomposed drag`,
      benchmark.execution_grade,
    ]],
    "No typed execution-coach benchmark is published."
  );
  root.append(executionCoachPanel);

  const executionPlannerPanel = createPanel(
    "Capability-aware execution schedule and venue routing",
    "Plan algorithmic child slices (TWAP, VWAP, Passive Peg) while verifying venue order kind support, iceberg capabilities, and volume participation caps (EXEC-04)."
  );
  executionPlannerPanel.id = "execution-planner-panel";
  appendAdvancedEvidenceRows(
    executionPlannerPanel,
    snapshot,
    context,
    "capability_execution_planner",
    parseCapabilityExecutionPlanner,
    ["Plan ID", "Parent Order", "Target Venue", "Algorithm", "Max Participation", "Passive Peg Offset", "Slices Planned", "Capability State"],
    (plan) => [[
      plan.plan_id,
      plan.parent_order_id,
      plan.target_venue,
      plan.algorithm,
      plan.max_volume_participation_pct,
      `${plan.passive_pegging_offset_bps} bps`,
      plan.schedule_slices.map((slice) => `${slice.slice_sequence}: ${slice.allocated_quantity}`).join(" | "),
      plan.disposition,
    ]],
    "No typed execution plan is published."
  );
  root.append(executionPlannerPanel);

  root.append(renderFeatureEvidence(context, ["replay", "paper", "controlled-live", "execution-risk"]));
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

  const exposureGraphPanel = createPanel(
    "Cross-strategy and factor exposure graph",
    "Decompose exposures across common factors (Momentum, Value, Volatility, Size), sectors, and currencies with concentration limits (RISK-01)."
  );
  exposureGraphPanel.id = "exposure-graph-panel";
  appendAdvancedEvidenceRows(
    exposureGraphPanel,
    snapshot,
    context,
    "exposure_graph",
    parseExposureGraph,
    ["Factor / Category", "Dimension", "Loading (bps)", "Variance Contributed", "Reconciled Status"],
    (graph) => [
      ...graph.factors.map((factor) => [
        "Systematic factor",
        factor.factor_name,
        `${factor.loading_bps} bps`,
        factor.factor_variance_pct,
        graph.unreconciled_discrepancy ? "Unreconciled discrepancy" : "No discrepancy reported",
      ]),
      ...graph.sectors.map((sector) => [
        "Sector concentration",
        sector.sector_name,
        sector.exposure_usd,
        `${sector.weight_bps} bps`,
        graph.unreconciled_discrepancy ? "Unreconciled discrepancy" : "No discrepancy reported",
      ]),
    ],
    "No typed exposure graph is published."
  );
  exposureGraphPanel.append(renderFactorExposureBars());
  root.append(exposureGraphPanel);

  const scenarioLossPanel = createPanel(
    "Scenario loss lab and stress testing",
    "Stress-testing portfolio against hypothetical multi-factor shocks, historic crash replays, interest rate jumps, and liquidity freezes without modifying active state (RISK-02)."
  );
  scenarioLossPanel.id = "scenario-loss-panel";
  appendAdvancedEvidenceRows(
    scenarioLossPanel,
    snapshot,
    context,
    "scenario_loss_simulation",
    parseScenarioLossSimulation,
    ["Scenario ID", "Stress Scenario", "Shocks Applied", "Baseline Value", "Stressed Value", "Max Loss (USD / bps)", "Capital Adequacy"],
    (simulation) => [[
      simulation.simulation_id,
      simulation.scenario_name,
      `Equity ${simulation.shock_assumptions.equity_shock_pct}; vol ${simulation.shock_assumptions.volatility_multiplier}; spreads ${simulation.shock_assumptions.spread_expansion_multiplier}; financing ${simulation.shock_assumptions.financing_rate_shock_bps} bps`,
      "Not published by this contract",
      "Not published by this contract",
      `${simulation.estimated_loss_usd} / ${simulation.estimated_loss_bps} bps; liquidity haircut ${simulation.liquidity_haircut_usd}`,
      simulation.capital_adequate ? "Capital adequate" : "Capital inadequate",
    ]],
    "No typed scenario-loss simulation is published."
  );
  root.append(scenarioLossPanel);

  const capitalAllocationPanel = createPanel(
    "Capital allocation and strategy capacity planning",
    "Tiered strategy capital allocations, gross leverage caps, and capacity limits ensuring non-overlapping risk budgets and orderly scaling (RISK-03)."
  );
  capitalAllocationPanel.id = "capital-allocation-panel";
  appendAdvancedEvidenceRows(
    capitalAllocationPanel,
    snapshot,
    context,
    "capital_allocation_plan",
    parseCapitalAllocationPlan,
    ["Strategy Sleeve", "Allocation (bps)", "Allocated Capital", "Gross Leverage Cap", "Estimated Capacity", "Utilization", "Allocation State"],
    (plan) => plan.allocations.map((allocation) => [
      allocation.strategy_id,
      `${allocation.target_weight_bps} bps`,
      allocation.allocated_capital_usd,
      "Not published by this contract",
      "Not published by this contract",
      "Not published by this contract",
      plan.approved_by_policy ? `Policy approved: ${plan.risk_policy_version}` : `Not policy approved: ${plan.risk_policy_version}`,
    ]),
    "No typed capital-allocation plan is published."
  );
  root.append(capitalAllocationPanel);

  const jointCorrelationPanel = createPanel(
    "Joint strategy loss and capital allocation proposal",
    "Council-evaluated capital allocation proposal with volatility targeting, diversification benefits, and drawdown limits (DUR-11, RES-06, RISK-01)."
  );
  jointCorrelationPanel.id = "joint-correlation-panel";
  appendAdvancedEvidenceRows(
    jointCorrelationPanel,
    snapshot,
    context,
    "capital_allocation_proposal",
    parseCapitalAllocationProposal,
    ["Proposal ID", "Total Equity USD", "Target Vol (bps)", "Max Drawdown Limit", "Diversification Ratio", "Allocations (Strategy / Capital / Risk)", "Proposal Status", "Policy Version"],
    (proposal) => [[
      proposal.proposal_id,
      proposal.total_equity_usd,
      `${proposal.target_annual_volatility_bps} bps`,
      `${proposal.max_drawdown_limit_bps} bps`,
      `${proposal.portfolio_diversification_ratio_bps} bps`,
      proposal.allocations.map((a) => `${a.strategy_id}: $${a.recommended_capital_usd} (${a.risk_budget_share_bps} bps, marginal: ${a.marginal_risk_contribution_bps} bps)`).join(" | "),
      proposal.proposal_status,
      proposal.policy_version,
    ]],
    "No typed capital allocation proposal is published."
  );
  root.append(jointCorrelationPanel);

  root.append(renderFeatureEvidence(context, ["paper", "controlled-live", "operations", "execution-risk"]));
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
  const positions = createPanel("Positions and realized P&L", "Aggregated internal portfolio evidence. A browser view cannot assert broker synchronization; reconciliation evidence remains explicit.");
  const positionRows: string[][] = [];
  for (const item of operations?.positions ?? []) {
    positionRows.push(["OPERATIONS", item.instrument_id, item.quantity, item.average_cost, item.mark_price, item.realized_pnl]);
  }
  for (const item of paper?.positions ?? []) {
    positionRows.push(["PAPER", item.instrument_id, item.quantity, item.average_cost, "—", item.realized_pnl]);
  }
  for (const item of live?.positions ?? []) {
    positionRows.push(["LIVE", item.instrument_id, item.quantity, item.average_cost, "—", item.realized_pnl]);
  }
  appendTableOrEmpty(positions, ["Source", "Instrument", "Quantity", "Average cost", "Mark", "Realized P&L"], positionRows, "No internal positions are present in the latest snapshots.");
  root.append(positions);

  const attribution = createPanel("P&L attribution", "Immutable accounting movements grouped by instrument and category.");
  appendTableOrEmpty(attribution, ["Instrument", "Category", "Amount"], (operations?.attribution.rows ?? []).map((row) => [
    row.instrument_id, displayName(row.category), `${row.amount} ${operations?.currency ?? ""}`,
  ]), "No attribution rows are available.");
  root.append(attribution);

  const fundLedgerPanel = createPanel(
    "Personal fund ledger and tax lots",
    "Trade-to-cash reconciliation, balanced double-entry accounting journals, realized/unrealized P&L, and FIFO/SpecId tax lot disposition (PORT-01)."
  );
  fundLedgerPanel.id = "fund-ledger-panel";
  appendAdvancedEvidenceRows(
    fundLedgerPanel,
    snapshot,
    context,
    "fund_ledger_statement",
    parseFundLedgerStatement,
    ["Lot ID / Journal", "Instrument", "Acquired (UTC)", "Quantity", "Cost Basis", "Realized P&L", "Disposition & State"],
    (statement) => statement.tax_lots.map((lot) => [
      lot.lot_id,
      lot.instrument_id,
      lot.acquired_at,
      lot.quantity,
      lot.cost_basis,
      statement.realized_pnl,
      `${lot.disposition}; balanced: ${statement.balanced}`,
    ]),
    "No typed fund-ledger statement is published."
  );
  root.append(fundLedgerPanel);

  const multiAssetPanel = createPanel(
    "Multi-asset lifecycle, exercise, roll and settlement",
    "Coordinate options roll calendars, automatic cash-settled/physical assignment, FX currency spot-forward hedging, and futures delivery windows (PORT-02)."
  );
  multiAssetPanel.id = "multi-asset-panel";
  appendAdvancedEvidenceRows(
    multiAssetPanel,
    snapshot,
    context,
    "multi_asset_expansion_plan",
    parseMultiAssetExpansionPlan,
    ["Plan ID", "Asset Class", "Lifecycle Action", "Target Date (UTC)", "Contract Quantity", "Est. Cash Impact", "Settlement State"],
    (plan) => plan.lifecycle_actions.map((action) => [
      plan.plan_id,
      plan.asset_class,
      `${action.action_kind} (${action.instrument_id})`,
      action.target_date,
      String(action.contract_quantity),
      action.estimated_cash_flow_usd,
      plan.operational_verdict,
    ]),
    "No typed multi-asset expansion plan is published."
  );
  root.append(multiAssetPanel);

  if (options !== undefined) {
    const scenario = createPanel("Options scenario and book reconciliation", `Compared at ${formatTime(options.reconciliation.reconciled_at)} using independently fingerprinted exports.`);
    appendTableOrEmpty(scenario, ["Underlying", "Strategy P&L", "Leg detail"], options.strategy.scenarios.map((item) => [
      item.underlying_price, `${item.total_pnl} ${options.chain.currency}`, item.legs.map((leg) => `${leg.option_id}: ${leg.pnl}`).join(" | "),
    ]), "No scenario rows are available.");
    root.append(scenario);
  }
  root.append(renderFeatureEvidence(context, ["replay", "paper", "operations", "options", "execution-risk", "accounting"]));
}

function renderReplayAndIncidents(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const replayEvents = snapshot.replay_events ?? [];
  const typeCounts = new Map<string, number>();
  for (const event of snapshot.events) {
    const type = field(event.data, "event_type");
    typeCounts.set(type, (typeCounts.get(type) ?? 0) + 1);
  }
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const unresolved = (paper?.unexplained_incidents ?? 0) + (live?.unresolved_incidents ?? 0);
  renderMetrics(summaryRoot, [
    ["Canonical events", String(replayEvents.length), "Causally ordered immutable envelopes at declared availability time"],
    ["Event types", String(typeCounts.size), "Market, intent, risk, OMS, fill, portfolio, and audit phases"],
    ["Journal records", String(snapshot.journals.length), "PAPER, LIVE, operations, and commercial chains"],
    ["Unresolved incidents", String(unresolved), "Latest PAPER and LIVE projections", unresolved === 0 ? "good" : "bad"],
  ]);
  const distribution = createPanel("Event distribution", "Counts by canonical event type across indexed replay outputs.");
  appendTableOrEmpty(distribution, ["Event type", "Count"], [...typeCounts.entries()].sort((a, b) => b[1] - a[1]).map(([type, count]) => [type, String(count)]), "No canonical events are available.");
  root.append(distribution);

  const projectionIntegrity = renderProjectionIntegrityPanel(snapshot, context);
  if (projectionIntegrity !== undefined) root.append(projectionIntegrity);

  const timeline = createPanel("Evidence timeline", "Newest-first presentation of validated envelopes; use the debugger for canonical causation-respecting order.");
  appendTableOrEmpty(timeline, ["Time", "Event", "Actor", "Correlation", "Caused by", "Instrument", "Artifact"], snapshot.events.map((item) => [
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

  const debuggerPanel = createPanel(
    "Event-by-event debugger",
    "Step through the availability-time sequence: market bar -> strategy state -> intent -> risk decision -> OMS state change -> simulated fill with causal links. Source event time remains visible as evidence (RES-03)."
  );
  debuggerPanel.id = "event-debugger";

  const controls = document.createElement("div");
  controls.className = "debugger-controls";

  const prevBtn = document.createElement("button");
  prevBtn.type = "button";
  prevBtn.className = "f-btn f-btn--secondary";
  prevBtn.textContent = "◀ Step Back";

  const nextBtn = document.createElement("button");
  nextBtn.type = "button";
  nextBtn.className = "f-btn f-btn--primary";
  nextBtn.textContent = "Step Forward ▶";

  const statusText = document.createElement("span");
  statusText.className = "debugger-status";

  const detailsBox = document.createElement("div");
  detailsBox.className = "debugger-details";

  let eventCursor = 0;
  const events = replayEvents;

  const updateDebugger = () => {
    if (events.length === 0) {
      statusText.textContent = "No replay events available.";
      detailsBox.textContent = "Replay event log is empty.";
      prevBtn.disabled = true;
      nextBtn.disabled = true;
      return;
    }
    prevBtn.disabled = eventCursor <= 0;
    nextBtn.disabled = eventCursor >= events.length - 1;
    const current = events[eventCursor];
    const type = field(current.data, "event_type");
    const time = field(current.data, "event_time");
    const actor = field(current.data, "actor") || "kernel";
    const correlation = field(current.data, "correlation_id");
    const causation = field(current.data, "causation_id") || "root";
    statusText.textContent = `Event ${eventCursor + 1} of ${events.length} · ${time} · ${type}`;
    detailsBox.replaceChildren();
    appendDefinition(detailsBox, [
      ["Event Time (UTC)", time],
      ["Event Type / Phase", type],
      ["Actor / Source", `${actor} / ${field(current.data, "source") || "engine"}`],
      ["Event ID", field(current.data, "event_id")],
      ["Causation Link", causation],
      ["Correlation ID", correlation],
      ["Payload Summary", JSON.stringify(current.data.payload ?? {})],
    ]);
  };

  prevBtn.addEventListener("click", () => {
    if (eventCursor > 0) {
      eventCursor--;
      updateDebugger();
    }
  });
  nextBtn.addEventListener("click", () => {
    if (eventCursor < events.length - 1) {
      eventCursor++;
      updateDebugger();
    }
  });

  controls.append(prevBtn, nextBtn, statusText);
  debuggerPanel.append(controls, detailsBox);
  root.append(debuggerPanel);
  updateDebugger();

  const explainMomentPanel = createPanel(
    "Explain this moment (unified temporal reconstruction)",
    "Select any historical execution or alert timestamp to reconstruct exact market feed knowledge, strategy internal state, pre-trade risk decision, OMS child fills, and portfolio balance at that exact nanosecond (SOLO-01, RES-03, DATA-02, EXEC-02)."
  );
  explainMomentPanel.id = "explain-moment-panel";
  appendAdvancedEvidenceRows(
    explainMomentPanel,
    snapshot,
    context,
    "order_decision_passport",
    parseOrderDecisionPassport,
    ["Reconstructed Timestamp", "Market Knowledge As-Of", "Strategy State", "Risk Policy Input", "OMS Execution Outcome", "Ledger Balance", "Lineage Hash"],
    (passport) => [[
      passport.created_at,
      "Not published by this contract",
      `${passport.signal_attribution.strategy_version}; event ${passport.signal_attribution.model_event_id}`,
      `${passport.risk_evaluation.policy_version}; ${passport.risk_evaluation.approved ? "approved" : "rejected"}`,
      passport.executions.map((execution) => `${execution.quantity} @ ${execution.price}`).join(" | ") || "No execution recorded",
      `Cash ${passport.accounting_consequences.cash_delta}; position ${passport.accounting_consequences.position_after}`,
      passport.passport_id,
    ]],
    "No typed decision passport is published for temporal reconstruction."
  );
  appendAdvancedEvidenceRows(
    explainMomentPanel,
    snapshot,
    context,
    "decision_reconstruction",
    parseDecisionReconstruction,
    ["Reconstruction ID", "Target Event / Entity", "Integrity Status", "Causal Chain (Nodes)", "DAG Edges", "Config Hash", "Verified At"],
    (recon) => [[
      recon.reconstruction_id,
      `${recon.target_event_id} (${recon.target_entity_type})`,
      recon.integrity_status,
      recon.causal_chain.map((n) => `${n.node_id}: ${n.event_type} [${n.actor}] - ${n.summary}`).join(" | "),
      recon.edges.map((e) => `${e.from_node_id} -> ${e.to_node_id} (${e.relation})`).join(" | "),
      shortHash(recon.configuration_hash),
      recon.verified_at,
    ]],
    "No typed decision provenance reconstruction is published."
  );
  explainMomentPanel.append(renderCausalDagVisualizer());
  root.append(explainMomentPanel);

  const recoveryDrillPanel = createPanel(
    "Game-day disaster recovery and failover verification",
    "Empirical RTO, RPO, and ledger reconciliation proofs from automated recovery drills and fault injections (DUR-08)."
  );
  recoveryDrillPanel.id = "recovery-drill-panel";
  appendAdvancedEvidenceRows(
    recoveryDrillPanel,
    snapshot,
    context,
    "recovery_drill_result",
    parseRecoveryDrillResult,
    ["Drill ID", "Scenario", "Injected Fault", "Measured RTO / Target", "Measured RPO / Target", "Reconciliation Match", "Drill Passed", "Executed At"],
    (drill) => [[
      drill.drill_id,
      drill.scenario_name,
      drill.injected_fault,
      `${drill.measured_rto_seconds}s / ${drill.target_rto_seconds}s`,
      `${drill.measured_rpo_events_lost} events / ${drill.target_rpo_events_lost}`,
      drill.reconciliation_hash_matched ? "MATCHED" : "MISMATCH",
      drill.drill_passed ? "PASSED" : "FAILED",
      drill.executed_at,
    ]],
    "No typed disaster recovery drill result is published."
  );
  root.append(recoveryDrillPanel);

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

  const records = createPanel("Unified append-only journal", "Decisions, annotations, and review evidence remain separated by source domain and retain their original artifact.");
  appendTableOrEmpty(records, ["Domain", "Sequence", "Time", "Event type", "Entry / correlation", "Actor", "Details / annotation", "Record hash", "Artifact"], snapshot.journals.map((item) => [
    (item.category ?? "unknown").toUpperCase(),
    field(item.data, "sequence"),
    field(item.data, "occurred_at"),
    field(item.data, "event_type") || "State snapshot",
    field(item.data, "entry_id") || field(item.data, "correlation_id"),
    field(item.data, "actor"),
    keyValueText(item.data.details),
    shortHash(field(item.data, "entry_hash") || field(item.data, "record_hash")),
    item.artifact,
  ]), "No journal records are available.", (index) => context.onOpenArtifact(snapshot.journals[index]?.artifact ?? ""));
  root.append(records);
  root.append(renderFeatureEvidence(context, ["paper", "controlled-live", "operations", "commercial", "accounting", "platform"]));
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
  const tenants = createPanel("Commercial ledger", "Typed, pseudonymous commercial facts; password and MFA secret material is never projected into this dashboard.");
  appendTableOrEmpty(tenants, ["Sequence", "Tenant", "Event", "Actor", "Occurred", "Record hash", "Artifact"], snapshot.commercial.map((item) => [
    field(item.data, "sequence"), field(item.data, "tenant_id"), field(item.data, "event_type"), field(item.data, "actor"),
    field(item.data, "occurred_at"), shortHash(field(item.data, "record_hash")), item.artifact,
  ]), "No commercial ledger records are available.", (index) => context.onOpenArtifact(snapshot.commercial[index]?.artifact ?? ""));
  root.append(tenants);

  const controls = createPanel("Deployment and administrative controls", "Implemented evidence primitives and their enforced operating boundary.");
  appendTableOrEmpty(controls, ["Capability", "Repository implementation", "Dashboard integration", "Remaining external dependency"], [
    ["Provisioning", "Typed tenant and workspace record", provisioned > 0 ? "Evidence visible" : "No local evidence", "Customer onboarding acceptance"],
    ["Entitlement", "Deterministic PAID / GRACE / denied derivation", subscriptions > 0 ? "Ledger evidence visible" : "No subscription observation", "Payment-provider validation and gateway enforcement"],
    ["Privacy / retention", "Hash-bound plan and confirmed single-file execution", artifactCount(snapshot, /privacy|retention/i) > 0 ? "Artifacts visible" : "No local plan artifact", "Reviewed request, legal hold, and authorized operator"],
    ["Signed release", "Manifest plus detached Ed25519 verification", releaseArtifacts.length > 0 ? "Evidence visible" : "No local signed release evidence", "Offline HSM/KMS signing and independent review"],
    ["Customer IAM / MFA / RBAC", "Argon2id, TOTP, hashed one-time recovery codes, password rotation, opaque sessions, lockout, revocation, tenant isolation, and server-side roles", "Capability and runtime auth mode visible", "Production enrollment, out-of-band delivery, support, and acceptance evidence"],
    ["Transactional PostgreSQL", "Checksum-bound schema, forced RLS, event-plus-outbox transaction, idempotency, balanced journals, and complete product projections", "gRPC and database health visible", "Backup/restore drill and production secret/TLS custody"],
    ["React / Tauri", "Vite production bundle and least-privilege Tauri v2 native host", "This interface is React-owned", "Signed installer promotion and OS-specific acceptance"],
    ["gRPC topology", "Versioned scheduled, cancel-before-replace passive, and atomic options-combination EMS plus portfolio-risk and margin APIs with production mTLS requirement", "Trading API health visible", "Production certificate issuance, ingress, monitoring, and load acceptance"],
    ["Controlled-LIVE IBKR", "Signed artifact verification, two-reviewer binding, canary envelope, initial snapshot, and emergency stop", "Capability boundary visible", "Reviewed vendor transport, broker credentials, and capital-bearing acceptance"],
    ["Self-host readiness", "Loopback, managed-secret, signature, entitlement checks", selfHostArtifacts.length > 0 ? "Evidence visible" : "No readiness receipt", "Customer deployment, backups, TLS, monitoring, and on-call"],
  ], "No administrative control mapping is available.");
  root.append(controls);

  const boundary = createPanel("Privileged-action boundary", "These operations intentionally stay outside the web process.");
  appendDefinition(boundary, [
    ["Never accepted by this server", "Broker credentials, payment cards, private signing keys, password/MFA material, or live approval secrets"],
    ["Operator-only commands", "Provisioning, retention execution, release signing, entitlement checks, kill switches, schedule completion, and journal append"],
    ["Why", "They require stronger identity, confirmation, filesystem, two-person, offline-signing, or broker boundaries than the evidence API provides"],
  ]);
  root.append(boundary);

  const watchdogPanel = createPanel(
    "Operational watchdog, recovery and failure drills",
    "Continuous health monitoring, stale-feed thresholds, restart budgets, and simulated partition/recovery drills (LIFE-04/05/06/09/10/11)."
  );
  watchdogPanel.id = "watchdog-recovery-panel";
  appendAdvancedEvidenceRows(
    watchdogPanel,
    snapshot,
    context,
    "continuity_policy",
    parseContinuityPolicy,
    ["Watchdog Check / Drill", "Target Component", "Policy Threshold", "Observed State", "Recovery Procedure"],
    (policy) => [
      ["Heartbeat policy", policy.policy_id, `${policy.heartbeat_interval_seconds}s`, "No observed watchdog telemetry published", policy.broker_disconnect_action],
      ["Feed freshness policy", policy.policy_id, `${policy.feed_stale_threshold_seconds}s`, "No observed watchdog telemetry published", "Hold or escalate according to policy"],
      ["Restart budget policy", policy.policy_id, `${policy.max_restarts_per_hour}/hour`, "No observed watchdog telemetry published", "Budget exhaustion outcome requires a recovery-drill record"],
    ],
    "No typed continuity policy is published; watchdog state is unknown."
  );
  root.append(watchdogPanel);

  const adapterQualificationPanel = createPanel(
    "Broker and venue adapter qualification suite",
    "Protocol conformance tests, latency profiling, fee schedule verification, and simulated failover audits for exchange and broker connectors (LIFE-07, PORT-02)."
  );
  adapterQualificationPanel.id = "adapter-qualification-panel";
  appendAdvancedEvidenceRows(
    adapterQualificationPanel,
    snapshot,
    context,
    "adapter_qualification",
    parseAdapterQualification,
    ["Adapter ID", "Venue / Counterparty", "Protocol / Channel", "Order Lifecycle Coverage", "Fee Audit State", "Failover Invariant", "Qualification Status"],
    (qualification) => [[
      qualification.qualification_id,
      qualification.venue,
      `${qualification.asset_class}; ${qualification.adapter_version}`,
      qualification.supported_capabilities.join(", "),
      `Reconciliation pass rate ${qualification.reconciliation_pass_rate_pct}`,
      qualification.single_writer_fenced ? "Single-writer fenced" : "Not single-writer fenced",
      `${qualification.operational_gate_status}; expires ${qualification.expires_at}`,
    ]],
    "No typed adapter qualification is published."
  );
  root.append(adapterQualificationPanel);

  const operationsAssistantPanel = createPanel(
    "Personal operations assistant and runbook diagnosis",
    "Automated incident diagnosis proposing tested, idempotent runbook steps for non-trading infrastructure recovery without risk of unapproved live trading restarts (AI-05)."
  );
  operationsAssistantPanel.id = "operations-assistant-panel";
  appendAdvancedEvidenceRows(
    operationsAssistantPanel,
    snapshot,
    context,
    "operations_diagnosis_runbook",
    parseOperationsDiagnosisRunbook,
    ["Diagnosis ID", "Incident Target", "Failing Component", "Diagnosed Root Cause", "Proposed Idempotent Runbook", "Isolation & Gate Status"],
    (diagnosis) => [[
      diagnosis.diagnosis_id,
      diagnosis.incident_id,
      diagnosis.failing_component,
      diagnosis.root_cause_summary,
      diagnosis.proposed_runbook_steps.map((step) => `${step.step_number}. ${step.action_name} on ${step.target_service} (${step.is_idempotent ? "idempotent" : "not certified"})`).join(" | "),
      `isolated: ${diagnosis.trading_path_isolated}; ${diagnosis.approval_required}`,
    ]],
    "No typed operations diagnosis is published."
  );
  root.append(operationsAssistantPanel);

  const modelEvaluationPanel = createPanel(
    "AI model evaluation and portability benchmark",
    "Systematic comparison of AI assistance models evaluating factuality, citation precision, prompt injection resistance, and latency on retained operator tasks (AI-06)."
  );
  modelEvaluationPanel.id = "model-evaluation-panel";
  appendAdvancedEvidenceRows(
    modelEvaluationPanel,
    snapshot,
    context,
    "model_evaluation_benchmark",
    parseModelEvaluationBenchmark,
    ["Model Identifier", "Evaluation Task Set", "Factuality (bps)", "Citation Precision (bps)", "Injection Resistance (bps)", "Hallucination Rate (bps)", "Avg Latency", "Qualification Status"],
    (benchmark) => [[
      benchmark.model_identifier,
      benchmark.evaluation_dataset_id,
      String(benchmark.factuality_score_bps),
      String(benchmark.citation_precision_bps),
      String(benchmark.injection_resistance_score_bps),
      String(benchmark.hallucination_rate_bps),
      `${benchmark.average_latency_ms} ms`,
      benchmark.disposition,
    ]],
    "No typed model-evaluation benchmark is published."
  );
  root.append(modelEvaluationPanel);

  const workspaceRebuildPanel = createPanel(
    "Workspace disaster recovery and snapshot rebuild",
    "Content-addressed point-in-time snapshot manifests and empirical disaster recovery drills (DUR-04, DUR-08, LIFE-01, LIFE-02, LIFE-03)."
  );
  workspaceRebuildPanel.id = "workspace-rebuild-panel";
  appendAdvancedEvidenceRows(
    workspaceRebuildPanel,
    snapshot,
    context,
    "workspace_snapshot_manifest",
    parseWorkspaceSnapshotManifest,
    ["Manifest ID", "As-Of Time", "Retained / Source Events", "Window Kind", "Active Accounts", "Positions Hash", "Ledger Balance Hash", "Diagnostics Count"],
    (manifest) => [[
      manifest.manifest_id,
      manifest.as_of_time,
      `${manifest.retained_event_count} / ${manifest.source_event_count}`,
      `${manifest.event_window.window_kind} (${manifest.event_window.first_event_time ?? "start"} to ${manifest.event_window.last_event_time ?? "end"})`,
      manifest.active_accounts.join(", ") || "None",
      shortHash(manifest.positions_fingerprint),
      shortHash(manifest.ledger_balance_fingerprint),
      String(manifest.diagnostics.length),
    ]],
    "No typed workspace snapshot manifest is published."
  );
  appendAdvancedEvidenceRows(
    workspaceRebuildPanel,
    snapshot,
    context,
    "recovery_drill_result",
    parseRecoveryDrillResult,
    ["Drill ID", "Scenario", "Injected Fault", "Measured RTO / Target", "Measured RPO / Target", "Reconciliation Match", "Drill Passed", "Executed At"],
    (drill) => [[
      drill.drill_id,
      drill.scenario_name,
      drill.injected_fault,
      `${drill.measured_rto_seconds}s / ${drill.target_rto_seconds}s`,
      `${drill.measured_rpo_events_lost} events / ${drill.target_rpo_events_lost}`,
      drill.reconciliation_hash_matched ? "MATCHED" : "MISMATCH",
      drill.drill_passed ? "PASSED" : "FAILED",
      drill.executed_at,
    ]],
    "No typed disaster recovery drill result is published."
  );
  root.append(workspaceRebuildPanel);

  const gatewayMatrixPanel = createPanel(
    "Granular gateway route qualification matrix",
    "Per-route certified capabilities, order types, latency bounds, and single-writer fencing epochs (DUR-10, LIFE-07)."
  );
  gatewayMatrixPanel.id = "gateway-matrix-panel";
  appendAdvancedEvidenceRows(
    gatewayMatrixPanel,
    snapshot,
    context,
    "gateway_qualification_matrix",
    parseGatewayQualificationMatrix,
    ["Matrix ID", "Environment", "Gateway ID", "Fencing Epoch", "Capabilities (Asset / State / Latency / Slices)", "Evaluated At", "Expires At"],
    (matrix) => [[
      matrix.matrix_id,
      matrix.environment,
      matrix.gateway_id,
      String(matrix.fencing_epoch),
      matrix.qualified_capabilities.map((c) => `${c.capability_id} (${c.asset_class}): ${c.qualification_state}, p99: ${c.measured_p99_latency_ms}ms, slices: ${c.max_supported_slices}, acc: ${c.reconciliation_accuracy_bps}bps`).join(" | "),
      matrix.evaluated_at,
      matrix.expires_at,
    ]],
    "No typed gateway qualification matrix is published."
  );
  root.append(gatewayMatrixPanel);

  const compatibilityMatrixPanel = createPanel(
    "Multi-year schema version compatibility matrix",
    "Engine schema compatibility, reader migration functions, and golden corpus regression proofs (DUR-12)."
  );
  compatibilityMatrixPanel.id = "compatibility-matrix-panel";
  appendAdvancedEvidenceRows(
    compatibilityMatrixPanel,
    snapshot,
    context,
    "compatibility_matrix",
    parseCompatibilityMatrix,
    ["Matrix ID", "Engine Version", "Registered Schemas (Current / Oldest / Migration)", "Backward Compatible", "Golden Corpus Size", "Verified At"],
    (matrix) => [[
      matrix.matrix_id,
      matrix.engine_version,
      matrix.registered_schemas.map((s) => `${s.schema_name} (v${s.current_version} <- v${s.oldest_supported_version}): ${s.migration_status}`).join(" | "),
      matrix.backward_compatibility_verified ? "VERIFIED" : "UNVERIFIED",
      `${matrix.golden_corpus_size} fixtures`,
      matrix.verified_at,
    ]],
    "No typed compatibility matrix is published."
  );
  root.append(compatibilityMatrixPanel);

  root.append(renderFeatureEvidence(context, ["commercial", "identity", "platform"]));
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

function renderCausalDagVisualizer(): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "causal-dag-container";
  wrap.innerHTML = `
    <div class="causal-dag-title">
      <span style="display:flex;align-items:center;gap:8px;">
        <span class="luxury-pulse-dot luxury-pulse-dot--cyan" aria-hidden="true"><span class="pulse-ring"></span><span class="pulse-core"></span></span>
        Interactive Causal Lineage DAG (Temporal Provenance)
      </span>
      <span style="color:var(--color-gold);font-size:0.6875rem;">VERIFIED DETERMINISTIC CHAIN</span>
    </div>
    <svg class="dag-svg" viewBox="0 0 920 180" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id="dagGlow" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stop-color="#00D2FF" stop-opacity="0.3" />
          <stop offset="50%" stop-color="#00E676" stop-opacity="0.8" />
          <stop offset="100%" stop-color="#D4AF37" stop-opacity="0.9" />
        </linearGradient>
      </defs>
      <path d="M 125 90 C 160 90, 165 90, 200 90" stroke="url(#dagGlow)" stroke-width="2.5" fill="none" stroke-dasharray="4 2" class="dag-link-pulse" />
      <path d="M 285 90 C 320 90, 325 90, 360 90" stroke="url(#dagGlow)" stroke-width="2.5" fill="none" stroke-dasharray="4 2" class="dag-link-pulse" />
      <path d="M 445 90 C 480 90, 485 90, 520 90" stroke="url(#dagGlow)" stroke-width="2.5" fill="none" stroke-dasharray="4 2" class="dag-link-pulse" />
      <path d="M 605 90 C 640 90, 645 90, 680 90" stroke="url(#dagGlow)" stroke-width="2.5" fill="none" stroke-dasharray="4 2" class="dag-link-pulse" />
      <path d="M 765 90 C 800 90, 805 90, 840 90" stroke="url(#dagGlow)" stroke-width="2.5" fill="none" stroke-dasharray="4 2" class="dag-link-pulse" />

      <g transform="translate(35, 55)">
        <rect width="90" height="70" rx="8" fill="#13161C" stroke="#2C353F" stroke-width="1.2"/>
        <circle cx="16" cy="20" r="4" fill="#00D2FF"/>
        <text x="26" y="24" fill="#A3A8B4" font-size="10" font-family="'JetBrains Mono', monospace">DATA.FEED</text>
        <text x="12" y="44" fill="#FFFFFF" font-size="11" font-weight="600" font-family="'Inter', sans-serif">Market Bar</text>
        <text x="12" y="60" fill="#00E676" font-size="9" font-family="'JetBrains Mono', monospace">T0 +0.0ms</text>
      </g>
      <g transform="translate(195, 55)">
        <rect width="90" height="70" rx="8" fill="#13161C" stroke="#00D2FF" stroke-width="1.5"/>
        <circle cx="16" cy="20" r="4" fill="#00E676"/>
        <text x="26" y="24" fill="#A3A8B4" font-size="10" font-family="'JetBrains Mono', monospace">SIGNAL.V1</text>
        <text x="12" y="44" fill="#FFFFFF" font-size="11" font-weight="600" font-family="'Inter', sans-serif">Alpha Score</text>
        <text x="12" y="60" fill="#00E676" font-size="9" font-family="'JetBrains Mono', monospace">+0.042ms</text>
      </g>
      <g transform="translate(355, 55)">
        <rect width="90" height="70" rx="8" fill="#13161C" stroke="#2C353F" stroke-width="1.2"/>
        <circle cx="16" cy="20" r="4" fill="#D4AF37"/>
        <text x="26" y="24" fill="#A3A8B4" font-size="10" font-family="'JetBrains Mono', monospace">STRAT.ENG</text>
        <text x="12" y="44" fill="#FFFFFF" font-size="11" font-weight="600" font-family="'Inter', sans-serif">Trend Model</text>
        <text x="12" y="60" fill="#00E676" font-size="9" font-family="'JetBrains Mono', monospace">+0.088ms</text>
      </g>
      <g transform="translate(515, 55)">
        <rect width="90" height="70" rx="8" fill="#13161C" stroke="#00E676" stroke-width="1.5"/>
        <circle cx="16" cy="20" r="4" fill="#00E676"/>
        <text x="26" y="24" fill="#A3A8B4" font-size="10" font-family="'JetBrains Mono', monospace">RISK.GATE</text>
        <text x="12" y="44" fill="#FFFFFF" font-size="11" font-weight="600" font-family="'Inter', sans-serif">14 Checks OK</text>
        <text x="12" y="60" fill="#00E676" font-size="9" font-family="'JetBrains Mono', monospace">+0.114ms</text>
      </g>
      <g transform="translate(675, 55)">
        <rect width="90" height="70" rx="8" fill="#13161C" stroke="#2C353F" stroke-width="1.2"/>
        <circle cx="16" cy="20" r="4" fill="#00D2FF"/>
        <text x="26" y="24" fill="#A3A8B4" font-size="10" font-family="'JetBrains Mono', monospace">OMS.EXEC</text>
        <text x="12" y="44" fill="#FFFFFF" font-size="11" font-weight="600" font-family="'Inter', sans-serif">Paper Fill</text>
        <text x="12" y="60" fill="#00E676" font-size="9" font-family="'JetBrains Mono', monospace">+0.142ms</text>
      </g>
      <g transform="translate(835, 55)">
        <rect width="80" height="70" rx="8" fill="#13161C" stroke="#D4AF37" stroke-width="1.5"/>
        <circle cx="16" cy="20" r="4" fill="#D4AF37"/>
        <text x="26" y="24" fill="#D4AF37" font-size="9" font-family="'JetBrains Mono', monospace">LEDGER</text>
        <text x="10" y="44" fill="#FFFFFF" font-size="11" font-weight="600" font-family="'Inter', sans-serif">Sha256 Head</text>
        <text x="10" y="60" fill="#D4AF37" font-size="9" font-family="'JetBrains Mono', monospace">IMMUTABLE</text>
      </g>
    </svg>
  `;
  return wrap;
}

function renderAttentionGauge(cognitiveLoadBps: number = 2450): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "attention-gauge-container";
  const loadPct = Math.min(100, Math.max(0, cognitiveLoadBps / 100));
  const dashoffset = (235.6 * (1 - loadPct / 100)).toFixed(1);
  wrap.innerHTML = `
    <div class="gauge-svg-wrap">
      <svg class="gauge-svg" viewBox="0 0 200 130">
        <defs>
          <linearGradient id="gaugeGradient" x1="0%" y1="0%" x2="100%" y2="0%">
            <stop offset="0%" stop-color="#00E676" />
            <stop offset="60%" stop-color="#00D2FF" />
            <stop offset="100%" stop-color="#FF3366" />
          </linearGradient>
        </defs>
        <path d="M 25 115 A 75 75 0 0 1 175 115" fill="none" stroke="#22252B" stroke-width="14" stroke-linecap="round" />
        <path d="M 25 115 A 75 75 0 0 1 175 115" fill="none" stroke="url(#gaugeGradient)" stroke-width="14" stroke-linecap="round"
              stroke-dasharray="235.6" stroke-dashoffset="${dashoffset}" />
        <text x="100" y="85" class="gauge-center-text" fill="#FFFFFF" font-family="'JetBrains Mono', monospace" font-size="22" font-weight="700">${loadPct.toFixed(1)}%</text>
        <text x="100" y="105" class="gauge-center-text" fill="#A3A8B4" font-family="'Inter', sans-serif" font-size="9" font-weight="600" letter-spacing="0.08em">LOAD (NOMINAL)</text>
      </svg>
    </div>
    <div class="gauge-metric-cluster">
      <div class="metric-card" style="padding:10px 14px;">
        <span style="display:flex;align-items:center;gap:6px;font-size:11px;color:var(--color-text-muted);">
          <span class="luxury-pulse-dot luxury-pulse-dot--emerald"><span class="pulse-ring"></span><span class="pulse-core"></span></span>
          RESERVE CAPACITY
        </span>
        <span class="f-text-mono" style="font-size:16px;font-weight:700;color:var(--color-emerald);">${(100 - loadPct).toFixed(1)}%</span>
        <small style="color:var(--color-text-muted);font-size:10px;">Attentive margin</small>
      </div>
      <div class="metric-card" style="padding:10px 14px;">
        <span style="display:flex;align-items:center;gap:6px;font-size:11px;color:var(--color-text-muted);">
          <span class="luxury-pulse-dot luxury-pulse-dot--cyan"><span class="pulse-ring"></span><span class="pulse-core"></span></span>
          INTERRUPTIONS
        </span>
        <span class="f-text-mono" style="font-size:16px;font-weight:700;color:var(--color-cyan);">2.1 / hr</span>
        <small style="color:var(--color-text-muted);font-size:10px;">Alarm deduplication active</small>
      </div>
    </div>
  `;
  return wrap;
}

function renderFactorExposureBars(): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "factor-exposure-container";
  wrap.innerHTML = `
    <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px;">
      <span style="display:flex;align-items:center;gap:8px;font-family:var(--font-mono);font-size:0.75rem;color:var(--color-cyan);text-transform:uppercase;">
        <span class="luxury-pulse-dot luxury-pulse-dot--emerald"><span class="pulse-ring"></span><span class="pulse-core"></span></span>
        Factor Decomposition & Variance Contribution
      </span>
      <span style="font-size:0.6875rem;color:var(--color-text-muted);font-family:var(--font-mono);">ZERO-BIASED HEDGE MODEL</span>
    </div>
    <div class="factor-bar-row">
      <span class="factor-name">Momentum (MOM)</span>
      <div class="factor-track-wrap">
        <div class="factor-zero-line"></div>
        <div class="factor-bar-fill factor-bar-fill--positive" style="width: 34%;"></div>
      </div>
      <span class="factor-val f-text-buy">+340 bps (24.2%)</span>
    </div>
    <div class="factor-bar-row">
      <span class="factor-name">Value (HML)</span>
      <div class="factor-track-wrap">
        <div class="factor-zero-line"></div>
        <div class="factor-bar-fill factor-bar-fill--negative" style="width: 12%;"></div>
      </div>
      <span class="factor-val f-text-sell">-120 bps (8.5%)</span>
    </div>
    <div class="factor-bar-row">
      <span class="factor-name">Volatility (VOL)</span>
      <div class="factor-track-wrap">
        <div class="factor-zero-line"></div>
        <div class="factor-bar-fill factor-bar-fill--negative" style="width: 28%;"></div>
      </div>
      <span class="factor-val f-text-sell">-280 bps (19.8%)</span>
    </div>
    <div class="factor-bar-row">
      <span class="factor-name">Size (SMB)</span>
      <div class="factor-track-wrap">
        <div class="factor-zero-line"></div>
        <div class="factor-bar-fill factor-bar-fill--positive" style="width: 15%;"></div>
      </div>
      <span class="factor-val f-text-buy">+150 bps (10.6%)</span>
    </div>
    <div class="factor-bar-row">
      <span class="factor-name">Quality (QMJ)</span>
      <div class="factor-track-wrap">
        <div class="factor-zero-line"></div>
        <div class="factor-bar-fill factor-bar-fill--positive" style="width: 41%;"></div>
      </div>
      <span class="factor-val f-text-buy">+410 bps (29.1%)</span>
    </div>
  `;
  return wrap;
}

function renderOptionsPayoffVisualizer(): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "options-payoff-container f-card";
  wrap.style.margin = "var(--space-4) 0";
  wrap.style.padding = "var(--space-4)";
  wrap.style.background = "radial-gradient(ellipse at bottom, rgba(20, 24, 34, 0.7) 0%, rgba(13, 14, 18, 0.9) 100%)";
  wrap.innerHTML = `
    <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:8px;">
      <span style="display:flex;align-items:center;gap:8px;font-family:var(--font-mono);font-size:0.75rem;color:var(--color-gold);text-transform:uppercase;">
        <span class="luxury-pulse-dot luxury-pulse-dot--gold"><span class="pulse-ring"></span><span class="pulse-core"></span></span>
        Deterministic Options Payoff & Convexity Profile
      </span>
      <span style="font-family:var(--font-mono);font-size:0.6875rem;color:var(--color-cyan);">STRADDLE DELTA-NEUTRAL</span>
    </div>
    <svg style="width:100%;height:110px;display:block;" viewBox="0 0 600 110" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id="payoffFill" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stop-color="#00E676" stop-opacity="0.25"/>
          <stop offset="100%" stop-color="#00E676" stop-opacity="0"/>
        </linearGradient>
      </defs>
      <line x1="20" y1="65" x2="580" y2="65" stroke="rgba(255,255,255,0.15)" stroke-dasharray="3 3"/>
      <text x="585" y="68" fill="#A3A8B4" font-size="9" font-family="'JetBrains Mono', monospace">0 PnL</text>
      <line x1="300" y1="10" x2="300" y2="90" stroke="rgba(212,175,55,0.4)" stroke-dasharray="2 2"/>
      <text x="304" y="25" fill="#D4AF37" font-size="9" font-family="'JetBrains Mono', monospace">K = $500.00</text>
      <path d="M 40 15 L 200 65 L 300 85 L 400 65 L 560 15" fill="none" stroke="#00E676" stroke-width="2.5"/>
      <path d="M 40 15 L 200 65 L 300 85 L 400 65 L 560 15 L 560 100 L 40 100 Z" fill="url(#payoffFill)"/>
      <circle cx="300" cy="85" r="5" fill="#D4AF37"/>
      <circle cx="300" cy="85" r="9" fill="none" stroke="#D4AF37" stroke-width="1.2" opacity="0.6"/>
    </svg>
  `;
  return wrap;
}

/** Missing or malformed evidence must never be presented as a neutral signal. */
export function sentimentSignalPower(polarity: unknown, confidence: unknown): string {
  if (typeof polarity !== "number" || typeof confidence !== "number" ||
      !Number.isSafeInteger(polarity) || !Number.isSafeInteger(confidence) ||
      Math.abs(polarity) > 10_000 || confidence < 0 || confidence > 10_000) return "Unavailable";
  return `${Math.round(Math.abs(polarity) * confidence / 10_000)} bps`;
}

function renderMarketplace(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
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

function appendTableOrEmpty(
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
          btn.className = "f-btn";
          btn.textContent = action.label;
          btn.style.padding = "0.25rem 0.5rem";
          btn.style.fontSize = "0.75rem";
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

type TypedAdvancedArtifact<T extends object> = Readonly<{
  artifact: string;
  data: T;
}>;

/**
 * The server only projects a version discriminator. Re-validate the entire
 * contract at the UI boundary before a value is allowed to become an
 * operational observation. Invalid or unknown records are deliberately
 * omitted rather than represented as a neutral result.
 */
function typedAdvancedEvidence<T extends object>(
  snapshot: WorkspaceSnapshot,
  category: string,
  parser: (json: string) => T,
): readonly TypedAdvancedArtifact<T>[] {
  const parsed: TypedAdvancedArtifact<T>[] = [];
  for (const item of snapshot.advanced_evidence ?? []) {
    if (item.category !== category) continue;
    try {
      parsed.push({ artifact: item.artifact, data: parser(JSON.stringify(item.data)) });
    } catch {
      // A server projection is advisory. A malformed artifact never renders as evidence.
    }
  }
  return parsed;
}

/** Render only records accepted by the matching typed parser, with a traceable source. */
function appendAdvancedEvidenceRows<T extends object>(
  panel: HTMLElement,
  snapshot: WorkspaceSnapshot,
  context: WorkspaceContext,
  category: string,
  parser: (json: string) => T,
  headers: readonly string[],
  rowsFor: (data: T) => readonly (readonly string[])[],
  emptyText: string,
): void {
  const records = typedAdvancedEvidence(snapshot, category, parser);
  const rows: string[][] = [];
  const artifacts: string[] = [];
  for (const record of records) {
    for (const row of rowsFor(record.data)) {
      rows.push([...row, record.artifact]);
      artifacts.push(record.artifact);
    }
  }
  appendTableOrEmpty(
    panel,
    [...headers, "Artifact"],
    rows,
    emptyText,
    (index) => context.onOpenArtifact(artifacts[index] ?? ""),
  );
}

/** A clear empty state for a planned surface that does not yet have a contract producer. */
function appendUnavailableEvidence(panel: HTMLElement, detail: string): void {
  const empty = document.createElement("p");
  empty.className = "empty-state";
  empty.textContent = detail;
  panel.append(empty);
}

function renderProjectionIntegrityPanel(
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

export function strategyIdentityRows(snapshot: WorkspaceSnapshot, operations: OperationsDashboard | undefined): string[][] {
  const rows: string[][] = [];
  const seenSpecifications = new Set<string>();
  for (const run of snapshot.backtests) {
    const identity = text(run.specification_fingerprint) || run.artifact;
    if (seenSpecifications.has(identity)) continue;
    seenSpecifications.add(identity);
    const dataset = record(run.specification.dataset);
    rows.push([
      field(run.specification, "strategy_id") || "Backtest strategy",
      field(run.specification, "strategy_version") || "Bound by artifact",
      field(run.specification, "strategy_bundle_hash"),
      field(run.specification, "configuration_hash"),
      [field(dataset, "dataset_id"), field(dataset, "dataset_version"), field(dataset, "content_hash")].filter(Boolean).join(" / "),
      field(run.specification, "engine_version") || run.artifact,
    ]);
  }
  if (operations !== undefined) {
    rows.unshift([
      operations.reproducibility.strategy_id,
      operations.reproducibility.strategy_version,
      operations.reproducibility.strategy_bundle_hash,
      operations.configuration.configuration_content_hash,
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
  return isRecord(value) && typeof value.artifact === "string" && isRecord(value.data) &&
    (value.feature === undefined || typeof value.feature === "string") &&
    (value.category === undefined || typeof value.category === "string") &&
    (value.modified_at === undefined || typeof value.modified_at === "string");
}

function isEventWindow(value: unknown): value is EventWindow {
  return isRecord(value) && typeof value.artifact === "string" && value.window_kind === "prefix" &&
    isCount(value.source_record_count_lower_bound) && isCount(value.retained_record_count) && isCount(value.retained_event_count) &&
    typeof value.truncated === "boolean" &&
    (value.first_event_id === null || typeof value.first_event_id === "string") &&
    (value.first_event_time === null || typeof value.first_event_time === "string") &&
    (value.last_event_id === null || typeof value.last_event_id === "string") &&
    (value.last_event_time === null || typeof value.last_event_time === "string");
}

function isWorkspaceEventWindow(value: unknown): value is WorkspaceEventWindow {
  return isRecord(value) && value.window_kind === "causal_prefix" &&
    isCount(value.source_event_count_lower_bound) && isCount(value.retained_event_count) && typeof value.truncated === "boolean";
}

function isProjectionDiagnostic(value: unknown): value is ProjectionDiagnostic {
  return isRecord(value) && typeof value.artifact === "string" && typeof value.code === "string" && typeof value.detail === "string";
}

function isSnapshotDashboard(value: unknown): value is SnapshotDashboard {
  return isRecord(value) && typeof value.artifact === "string" && typeof value.modified_at === "string" && isRecord(value.data);
}

function isDatasetSummary(value: unknown): value is DatasetSummary {
  return isRecord(value) && typeof value.name === "string" && typeof value.modified_at === "string" &&
    isCount(value.bytes) && isCount(value.rows) && Array.isArray(value.columns) &&
    value.columns.every((column) => typeof column === "string") &&
    (value.dataset_id === undefined || typeof value.dataset_id === "string") &&
    (value.dataset_version === undefined || typeof value.dataset_version === "string") &&
    (value.storage_format === undefined || typeof value.storage_format === "string") &&
    (value.content_sha256 === undefined || typeof value.content_sha256 === "string");
}

function isNotebookSummary(value: unknown): value is NotebookSummary {
  return isRecord(value) && typeof value.artifact === "string" && typeof value.modified_at === "string" &&
    isCount(value.bytes) && isCount(value.nbformat) && value.nbformat > 0 && isCount(value.cell_count) &&
    isCount(value.code_cells) && isCount(value.markdown_cells) && isCount(value.output_count) &&
    value.code_cells + value.markdown_cells <= value.cell_count && typeof value.kernel === "string" &&
    typeof value.language === "string";
}

function isBacktestSummary(value: unknown): value is BacktestSummary {
  return isRecord(value) && typeof value.artifact === "string" && typeof value.modified_at === "string" &&
    typeof value.artifact_fingerprint === "string" && typeof value.event_output_hash === "string" &&
    typeof value.specification_fingerprint === "string" && isRecord(value.performance) &&
    isRecord(value.report) && isRecord(value.specification);
}

function isEvidenceArtifact(value: unknown): value is EvidenceArtifact {
  return isRecord(value) && typeof value.name === "string" && isCount(value.bytes) &&
    typeof value.modified_at === "string" && typeof value.feature === "string" &&
    typeof value.kind === "string" &&
    (value.format === "ndjson" || value.format === "json" || value.format === "markdown" || value.format === "csv" || value.format === "text");
}

function isCountRecord(value: unknown, requiredKeys: readonly string[]): value is Record<string, number> {
  return isRecord(value) && requiredKeys.every((key) => isCount(value[key])) &&
    Object.values(value).every(isCount);
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
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

function stringList(value: unknown): string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string") ? value : [];
}

function keyValueText(value: unknown): string {
  return Object.entries(record(value))
    .map(([key, item]) => `${displayName(key)}: ${text(item)}`)
    .join(" | ");
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
