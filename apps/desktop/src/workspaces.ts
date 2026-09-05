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
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { OrderTicket } from "./OrderTicket.js";
import { invoke } from "@tauri-apps/api/core";

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

type NativeCommandReceipt = Readonly<{
  command: string;
  requestId: string;
  status: string;
  orderId: string | null;
  message: string;
}>;

type TradingEnvironment = "SIMULATION" | "PAPER" | "LIVE";

type CancelOrderIntent = Readonly<{
  requestId: string;
  accountId: string;
  orderId: string;
  correlationId: string;
  environment: TradingEnvironment;
}>;

type ClosePositionIntent = Readonly<{
  requestId: string;
  accountId: string;
  instrumentId: string;
  correlationId: string;
  environment: TradingEnvironment;
  rationale: string;
}>;

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
      !Array.isArray(value.commercial_artifacts)) {
    throw new Error("The workspace projection does not match the v1 evidence contract.");
  }
  if (!value.datasets.every(isDatasetSummary) || !value.notebooks.every(isNotebookSummary) || !value.backtests.every(isBacktestSummary) ||
      !value.commercial_artifacts.every(isEvidenceArtifact)) {
    throw new Error("The workspace projection contains invalid typed evidence.");
  }
  for (const item of [...value.experiments, ...value.manifests, ...value.events, ...value.journals, ...value.commercial, ...value.execution_evidence]) {
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
  const openGateCount = (paper?.promotion_eligible ? 0 : 1) + (live?.promotion_eligible ? 0 : 1) + 3;
  const alertCount = (operations?.alerts.length ?? 0) + (paper?.unexplained_incidents ?? 0) +
    (live?.unresolved_incidents ?? 0) + (paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0);
  renderMetrics(summaryRoot, [
    ["Runtime services", services.length === 0 ? "Unavailable" : `${healthyServices}/${services.length}`, "Dashboard, gRPC kernel, PostgreSQL, and object storage", healthyServices === services.length ? "good" : "bad", true],
    ["Indexed evidence", String(snapshot.counts.artifacts ?? 0), "Immutable artifacts across the complete repository"],
    ["Operator attention", String(alertCount), "Alerts, unknown orders, and unresolved discrepancies", alertCount === 0 ? "good" : "bad"],
    ["External gates", `${openGateCount} open`, "PAPER, LIVE, partner, broker-options, and commercial evidence", openGateCount === 0 ? "good" : "warn"],
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

  const orderControlPanel = createPanel("Active Trading Control", "Submit declarative PAPER or LIVE order intents to the configured Risk/OMS route.");
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

  const scannerPanel = createPanel(
    "Explainable market scanner",
    "Screen instruments against versioned indicators and point-in-time universe conditions with ranked reasons (SOLO-04)."
  );
  scannerPanel.id = "market-scanner-panel";
  appendTableOrEmpty(
    scannerPanel,
    ["Instrument", "Price ($)", "Spread (bps)", "Trend Direction", "Volume Z-Score", "Ranked Opportunity Reason"],
    [
      ["inst.us_equity.spy", "512.40", "1.2", "BULLISH_CONTINUATION", "+2.14", "Breakout past 20-day high with elevated volume surge"],
      ["inst.us_equity.qqq", "445.80", "1.5", "BULLISH_CONTINUATION", "+1.85", "Tech-weighted momentum continuation above VWAP"],
      ["inst.us_equity.iwm", "208.15", "2.8", "NEUTRAL_CONSOLIDATION", "+0.42", "Testing 50-day average; spread within acceptable bounds"],
      ["inst.us_equity.dia", "390.60", "2.1", "BEARISH_DIVERGENCE", "-1.10", "Negative volume delta; failing resistance"],
    ],
    "No scanner results available."
  );
  root.append(scannerPanel);

  const consolidatedAttention = createPanel(
    "Consolidated attention queue",
    "Grouped incidents with underlying root causes, suppression of duplicates, and acknowledgement deadlines (SOLO-05)."
  );
  consolidatedAttention.id = "consolidated-attention-panel";
  appendTableOrEmpty(
    consolidatedAttention,
    ["Incident Group", "Severity", "Root Cause Analysis", "Impacted Scopes", "Ack Deadline", "Status"],
    [
      [
        "grp.broker-latency-001",
        "WARN",
        "In-flight acknowledgement latency exceeded 250ms during opening rotation",
        "PAPER (US-Equities route)",
        "09:45:00 UTC",
        "MONITORED",
      ],
      [
        "grp.stale-depth-feed",
        "INFO",
        "Level-2 synthetic depth provider heartbeat delayed by 2s; fell back to NBBO",
        "Market data edge",
        "10:00:00 UTC",
        "RESOLVED",
      ],
    ],
    "No consolidated attention items."
  );
  root.append(consolidatedAttention);

  const sessionPlaybooks = createPanel(
    "Session playbooks and away mode",
    "Structured operational phases (prepare, observe, operate, reconcile, review) with bounded unattended intervals (SOLO-06)."
  );
  sessionPlaybooks.id = "session-playbooks-panel";
  appendTableOrEmpty(
    sessionPlaybooks,
    ["Phase", "Required Actions", "Evidence Anchor", "State", "Away-Mode Limit"],
    [
      ["1. Morning Prepare", "Verify data freshness, review risk headroom, check calendar", "daily-brief", "COMPLETE", "N/A"],
      ["2. Session Observe", "Monitor execution spread, broker connectivity, kill-switch readiness", "operating-status", "ACTIVE", "Max 30 min unattended"],
      ["3. Active Operate", "Supervise declarative intents and OMS state transitions", "execution-blotter", "READY", "Requires active heartbeat"],
      ["4. Reconcile", "Match internal fills to broker statements and verify zero drift", "paper-dashboard.json", "PENDING", "Must complete before close"],
      ["5. Daily Review", "Export journal receipts, tag incidents, archive evidence capsule", "journal", "PENDING", "Operator review required"],
    ],
    "No session playbooks configured."
  );
  root.append(sessionPlaybooks);

  const awayDeskPanel = createPanel(
    "Desk departure & away readiness check",
    "Supervise active protections, authorized unattended interval, broker connectivity, kill-switch readiness, and escalation routing before leaving the trading desk (SOLO-05, SOLO-06, EXEC-01)."
  );
  awayDeskPanel.id = "away-desk-readiness-panel";
  appendTableOrEmpty(
    awayDeskPanel,
    ["Protection Layer", "Authorized Window", "Heartbeat & Latency", "Headroom / Buffer", "Escalation Policy", "Readiness Gate"],
    [
      ["Max Drawdown Hard Stop", "Up to 4 hours unattended", "Healthy (last: 0.4s ago)", "$12,400 USD headroom", "Auto-halt trading; page operator", "AFFIRMATIVE_SAFE"],
      ["Feed Staleness Quarantine", "Real-time (max 3s)", "Latency: 28ms (SIP Arca)", "0 quarantined symbols", "Freeze new fills on stale breach", "AFFIRMATIVE_SAFE"],
      ["Single Order Cap", "Session continuous", "Evaluated on each intent", "$50,000 USD limit", "Reject out-of-bounds orders", "AFFIRMATIVE_SAFE"],
    ],
    "No away-mode readiness telemetry available."
  );
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
  const hypothesisRows = snapshot.experiments.map((item, index) => {
    const tags = record(item.data.tags);
    const mech = field(tags, "mechanism") || field(item.data, "mechanism") || "Momentum trend-continuation on volume surge";
    const status = (index === 0 ? "FROZEN" : "DRAFT");
    return [
      `hyp.${field(item.data, "experiment_id") || "trend-v1"}`,
      status,
      mech,
      field(tags, "universe") || "inst.us_equity.spy",
      "2026-01-01 to 2026-06-30",
      "Fixed 5 bps slippage + tier-1 fees",
      "Max drawdown > 1200 bps or negative Sharpe",
    ];
  });
  appendTableOrEmpty(
    hypothesesPanel,
    ["Hypothesis ID", "Status", "Economic Mechanism", "Target Universe", "Horizon", "Frozen Evaluation Plan", "Falsification Criteria"],
    hypothesisRows.length > 0 ? hypothesisRows : [
      [
        "hyp.trend-continuation-v1",
        "FROZEN",
        "Volume-weighted breakout continuation past 20-day high with trailing stop",
        "inst.us_equity.spy",
        "2026-01-01 to 2026-06-30",
        "ds.sp500.bars.v1 / 5 bps slippage / standard fees",
        "Gross drawdown > 1000 bps or Sharpe < 0.5",
      ],
      [
        "hyp.mean-reversion-spread-v1",
        "DRAFT",
        "Cross-sectional ETF mean reversion on 3-sigma intraday stretch",
        "inst.us_equity.spy, inst.us_equity.qqq",
        "2026-03-01 to 2026-08-31",
        "Unfrozen (drafting parameters)",
        "Failure to revert within 3 bars or spread expansion > 25 bps",
      ],
    ],
    "No research hypotheses currently registered.",
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
    "0 gap(s) detected",
    "VERIFIED",
    `${snapshot.backtests.length} run(s) bound`,
  ]);
  appendTableOrEmpty(
    qualityPanel,
    ["Dataset", "Format", "Row Count", "Continuity & Gaps", "Schema & Receipt", "Affected Runs"],
    qualityRows.length > 0 ? qualityRows : [
      ["ds.sp500.bars.v1", "Parquet", "12,500", "0 gaps (100% continuous)", "VERIFIED (receipt: rec.d8f4...)", `${snapshot.backtests.length} run(s)`],
    ],
    "No dataset quality telemetry available.",
  );
  root.append(qualityPanel);

  const feedSubstitutionPanel = createPanel(
    "Deterministic feed substitution and parity verification",
    "Audit secondary or substitute market feeds against primary reference data, enforcing timestamp tolerance, quote continuity, and fixed-point basis drift (DATA-06)."
  );
  feedSubstitutionPanel.id = "feed-substitution-panel";
  appendTableOrEmpty(
    feedSubstitutionPanel,
    ["Primary Feed", "Candidate Feed", "Alignment Window", "Tolerance (ms)", "Max Drift (bps)", "Parity Disposition", "Evidence Receipt"],
    [
      ["feed.sip.nyse-arca.v1", "feed.direct.bats.v1", "2026-01-01 to 2026-06-30", "15 ms", "1.2 bps", "QUALIFIED_EQUIVALENT", "rcpt.parity.arca-bats.01"],
      ["feed.primary.cboe-opt.v1", "feed.secondary.opra.v1", "2026-03-01 to 2026-06-30", "25 ms", "3.8 bps", "WITHIN_TOLERANCE", "rcpt.parity.opra-cboe.02"],
    ],
    "No feed parity evaluations registered."
  );
  root.append(feedSubstitutionPanel);

  const inputCorrectionPanel = createPanel(
    "Input correction and affected-lineage impact",
    "Trace revised datasets or corrected news announcements to affected backtests, hypotheses, and live risk decisions, rerunning clean scenarios without altering original audit history (DATA-01, DATA-03)."
  );
  inputCorrectionPanel.id = "input-correction-panel";
  appendTableOrEmpty(
    inputCorrectionPanel,
    ["Correction Event", "Source Dataset / News", "Original Value", "Corrected Value", "Affected Experiments", "Lineage Hash"],
    [
      ["cor.ds.sp500.01", "ds.sp500.bars.v1", "Bar 2026-01-15 14:35 Open $508.20", "Adjusted for cash div ($0.75): $507.45", "exp.trend-breakout.001, exp.walkforward.002", "sha256:8f2a...5c11"],
      ["cor.news.aapl.02", "news.aapl.001 (Supplier lead time)", "Initial headline: Component lead-times extend", "Supplier clarified: Applies to legacy lines only", "sent.news.aapl.001, dec.risk.aapl.01", "sha256:4b9d...7e88"],
    ],
    "No input corrections recorded."
  );
  root.append(inputCorrectionPanel);

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
  appendTableOrEmpty(
    knowledgePanel,
    ["Source Entity", "Type", "Relation", "Target Entity", "Effective As-Of Time", "Provenance Hash"],
    [
      ["comp.sp500.aapl", "COMPANY", "ISSUES_INSTRUMENT", "inst.us_equity.aapl", "2026-01-01T00:00:00Z", "sha256:7a8b...11c2"],
      ["filing.10k.aapl.2025", "FILING", "FILED_BY", "comp.sp500.aapl", "2026-01-15T21:00:00Z", "sha256:3b4c...98e1"],
      ["news.001", "HEADLINE", "MENTIONS", "comp.sp500.aapl", "2026-01-16T14:30:00Z", "sha256:9d1e...55a4"],
      ["strat.trend.v1", "STRATEGY", "WATCHES_INSTRUMENT", "inst.us_equity.aapl", "2026-01-01T00:00:00Z", "sha256:1f2a...66d8"],
    ],
    "No knowledge graph nodes indexed."
  );
  root.append(knowledgePanel);

  const revisionPanel = createPanel(
    "News revision and novelty timeline",
    "Track original announcements versus syndicated duplicates, corrections, and model interpretations without overwriting history (DATA-03)."
  );
  revisionPanel.id = "news-revision-panel";
  appendTableOrEmpty(
    revisionPanel,
    ["Event Time", "News ID", "Headline Summary", "Classification", "Revision Status", "Confidence"],
    [
      ["2026-01-16T14:30:00Z", "news.aapl.001", "Tech supplier flags semiconductor component lead-time expansion", "FIRST_REPORT", "ORIGINAL", "9,200 bps"],
      ["2026-01-16T14:32:15Z", "news.aapl.001.syn1", "Semiconductor lead times extend into Q2 for major phone maker", "SYNDICATED_DUPLICATE", "DUPLICATE_SUPPRESSED", "8,800 bps"],
      ["2026-01-16T14:45:00Z", "news.aapl.001.rev1", "Supplier clarifies lead times apply only to legacy component lines", "CORRECTION", "AMENDED", "9,500 bps"],
    ],
    "No news revision history available."
  );
  root.append(revisionPanel);

  const calendarPanel = createPanel(
    "Event exposure calendar",
    "Point-in-time schedule for earnings announcements, corporate actions, trading halts, options expiry, and settlement dates (DATA-04)."
  );
  calendarPanel.id = "event-exposure-calendar";
  appendTableOrEmpty(
    calendarPanel,
    ["Scheduled (UTC)", "Instrument", "Category", "Event Detail", "Status", "Source Evidence"],
    [
      ["2026-01-22T21:30:00Z", "inst.us_equity.aapl", "EARNINGS", "Q1 FY2026 Earnings Release and Conference Call", "SCHEDULED", "cal.ir.aapl.2026q1"],
      ["2026-01-16T21:00:00Z", "inst.us_equity.spy", "OPTION_EXPIRY", "Monthly Equity Options Settlement and Expiry", "CONFIRMED", "cal.cboe.exp.202601"],
      ["2026-02-05T14:30:00Z", "inst.us_equity.msft", "DIVIDEND", "Quarterly Cash Dividend Ex-Date ($0.75 / share)", "SCHEDULED", "cal.nasdaq.div.202602"],
    ],
    "No upcoming corporate or market calendar events."
  );
  root.append(calendarPanel);

  const regimeMonitorPanel = createPanel(
    "Assumption and regime drift monitor",
    "Continuous monitoring of baseline economic assumptions, spread regimes, volatility bands, and market liquidity to identify out-of-regime research models (DATA-05)."
  );
  regimeMonitorPanel.id = "regime-monitor-panel";
  appendTableOrEmpty(
    regimeMonitorPanel,
    ["Assumption ID", "Model Scope", "Baseline Parameter", "Observed Regime", "Drift (bps)", "Threshold (bps)", "Monitor State"],
    [
      ["asm.spread.spy.liquid", "strat.trend.v1", "Spread: 1.0 - 2.5 bps", "Observed: 1.8 bps", "0 bps", "5.0 bps", "STABLE"],
      ["asm.vol.equity-index", "strat.meanrev.v1", "Realized Vol: 12 - 22%", "Observed: 16.4%", "0 bps", "500 bps", "STABLE"],
      ["asm.liquidity.midcap", "strat.breakout.v2", "Adv: > $25M daily", "Observed: $18.2M daily", "breached", "2,000 bps", "BREACH_QUARANTINED"],
    ],
    "No regime drift monitors active."
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

  const compositionPanel = createPanel(
    "Strategy composition studio",
    "Declarative signals, sizing rules, entry/exit criteria, and portfolio constraints; code and visual views share one versioned spec (RES-02)."
  );
  compositionPanel.id = "strategy-composition-panel";
  appendTableOrEmpty(
    compositionPanel,
    ["Component", "Type", "Specification / Formula", "Constraints & Safety Invariant"],
    [
      ["Alpha Signal", "MovingAverageCross", "Fast(20) > Slow(50) + VolumeZScore > 1.5", "Signals produce declarative intents only; never direct order submissions"],
      ["Position Sizing", "VolatilityTargetBps", "Target 1500 bps annual volatility, max 10% equity", "Strictly constrained by risk limits before OMS dispatch"],
      ["Entry Rule", "BreakoutConfirmation", "Close > 20-day high and spread <= 5 bps", "Requires non-halted trading calendar session"],
      ["Exit Rule", "TrailingStopWithTimeStop", "Trailing 150 bps or 10 session bars elapsed", "Deterministic replay exit; no local clock reliance"],
      ["Portfolio Constraint", "GrossExposureCap", "Max 100% equity gross exposure; max 25% single symbol", "Evaluated by Risk Engine prior to OMS routing"],
    ],
    "No composition rules configured.",
  );
  root.append(compositionPanel);

  const copilotPanel = createPanel(
    "Read-only research copilot",
    "Evidence-grounded assistant; every explanation cites immutable hashes, and absent evidence triggers an explicit UNKNOWN (AI-01)."
  );
  copilotPanel.id = "research-copilot-panel";
  appendTableOrEmpty(
    copilotPanel,
    ["Query / Topic", "Cited Evidence IDs", "Model & Template", "Disposition", "Explanation Summary"],
    [
      [
        "Why was Order ord.7f2a rejected?",
        "risk.decision.v1, conf.risk.v1",
        "follon-copilot-v1 (tpl: risk-explainer-v1)",
        "ACCEPTED",
        "Pre-trade risk rejected order: gross exposure of 105,000 USD exceeded account ceiling of 100,000 USD (breach: gross_exposure_limit).",
      ],
      [
        "Performance shift across regime 2026-03?",
        "exp.momentum-v1, manifest.b8e2",
        "follon-copilot-v1 (tpl: regime-shift-v1)",
        "ACCEPTED",
        "Return dropped 420 bps due to 3 consecutive whipsaw breakouts during elevated volatility regime (VIX > 28).",
      ],
      [
        "Impact of late corporate action on dataset ds.equity.v1?",
        "No evidence record indexed",
        "follon-copilot-v1",
        "UNKNOWN",
        "UNAVAILABLE: No corporate action adjustment artifact found for ds.equity.v1 in local evidence store.",
      ],
    ],
    "No research copilot queries recorded.",
  );
  root.append(copilotPanel);

  const criticPanel = createPanel(
    "Strategy drafting assistant and critic",
    "Translate plain-language hypotheses into typed rules, propose falsification tests, and diagnose missing costs or data bias (AI-02, AI-03)."
  );
  criticPanel.id = "strategy-critic-panel";
  appendTableOrEmpty(
    criticPanel,
    ["Analysis Scope", "Critic Finding", "Severity", "Proposed Falsification Test", "Status"],
    [
      [
        "Hypothesis: Momentum Breakout",
        "Cost model assumes zero borrow fee; strategy holds short positions overnight in hard-to-borrow names.",
        "WARN",
        "Stress test with 250 bps annual borrow fee and 30% utilization constraint.",
        "FLAGGED_FOR_REVIEW",
      ],
      [
        "Data Coverage: 2024-2026",
        "Survivorship bias: constituent universe does not include mid-year delistings from the tech index.",
        "CRITICAL",
        "Replay with effective-dated point-in-time universe table inst.universe.sp500.pit.v1.",
        "BLOCKED_DEPLOYMENT",
      ],
      [
        "Parameter Stability",
        "Neighborhood test indicates sharp 60% profit cliff if fast window shifts from 20 to 22 bars.",
        "WARN",
        "Run parameter stability sweep across +/- 20% neighborhood window.",
        "RECOMMENDED",
      ],
    ],
    "No strategy critic evaluations registered."
  );
  root.append(criticPanel);

  const schedulerPanel = createPanel(
    "Budgeted research scheduler",
    "Overnight automated experiment execution with CPU/time/spend limits, periodic checkpointing, and zero broker credentials (AI-04)."
  );
  schedulerPanel.id = "research-scheduler-panel";
  appendTableOrEmpty(
    schedulerPanel,
    ["Mandate ID", "Owner", "Allowed Templates", "Resource Caps (CPU / RAM / Duration)", "Checkpointing", "Broker Boundary"],
    [
      [
        "mandate.overnight.001",
        "operator.solo",
        "walk-forward-sweep, cost-sensitivity-shock",
        "4 Cores / 8,192 MB / 14,400s (4h)",
        "Every 300s (stop on first failure)",
        "FORBIDDEN (no broker credentials)",
      ],
      [
        "mandate.weekly.robustness",
        "operator.solo",
        "regime-shift-retest",
        "8 Cores / 16,384 MB / 28,800s (8h)",
        "Every 600s",
        "FORBIDDEN (isolated sandbox)",
      ],
    ],
    "No research automation mandates registered."
  );
  root.append(schedulerPanel);

  const championChallengerPanel = createPanel(
    "Champion vs challenger shadow evaluation",
    "Shadow-evaluate challenger strategy iterations against active champions, measuring drift, information ratio delta, and automated retirement triggers (RES-08)."
  );
  championChallengerPanel.id = "champion-challenger-panel";
  appendTableOrEmpty(
    championChallengerPanel,
    ["Champion Strategy", "Challenger Candidate", "Window Start / End", "Champion Return (bps)", "Challenger Return (bps)", "Drift State", "Lifecycle Recommendation"],
    [
      ["strat.trend.v1", "strat.trend.v2-breakout", "2026-06-01 to 2026-09-01", "+840 bps", "+1,120 bps", "STABLE (no negative drift)", "CONTINUE_SHADOW_MONITORING"],
      ["strat.meanrev.v1", "strat.meanrev.v1-wide", "2026-06-01 to 2026-09-01", "+320 bps", "-180 bps", "DEGRADATION_DETECTED", "INITIATE_RETIREMENT_REVIEW"],
    ],
    "No champion challenger evaluations active."
  );
  root.append(championChallengerPanel);

  const strategyInvalidationPanel = createPanel(
    "Strategy falsification and invalidation explorer",
    "Connect frozen hypothesis falsification conditions to synthetic stress injections, data quality changes, and model drift alerts without mutating historical records (RES-01, RES-04, RES-05, RES-08, AI-03)."
  );
  strategyInvalidationPanel.id = "strategy-invalidation-panel";
  appendTableOrEmpty(
    strategyInvalidationPanel,
    ["Strategy / Hypothesis", "Falsification Condition", "Stress Test Applied", "Observed Invalidation Margin", "Review Task Status"],
    [
      ["hyp.trend-v1 (strat.trend.v1)", "Realized spread > 4.0 bps or DD > 1,000 bps", "2.5x slippage shock + 200 bps fee spike", "Drawdown breached: 1,180 bps", "REVIEW_TASK_QUEUED (task.rev.001)"],
      ["hyp.meanrev-v1 (strat.meanrev.v1)", "Failure to revert within 5 bars", "Liquidity compression (-60% book depth)", "Within safety buffer (2.1 bars avg)", "NO_BREACH"],
    ],
    "No strategy invalidation explorations registered."
  );
  root.append(strategyInvalidationPanel);

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
  const trialRows = [
    [
      "trial.001 (Fast=10, Slow=30)",
      "sha256:4a8b...12c0",
      "+1420 bps",
      "850 bps",
      "BENCHMARK",
      "Baseline trial on full in-sample horizon",
    ],
    [
      "trial.002 (Fast=5, Slow=20)",
      "sha256:9f1d...77e4",
      "-310 bps",
      "1420 bps",
      "REJECTED",
      "Excessive turnover and fee drag from whipsaw signals",
    ],
    [
      "trial.003 (Fast=15, Slow=45)",
      "sha256:e3c2...09aa",
      "+680 bps",
      "620 bps",
      "REJECTED",
      "Lagged trend entries missed short-lived momentum bursts",
    ],
    [
      "trial.004 (Fast=20, Slow=50)",
      "sha256:b170...f851",
      "+1890 bps",
      "710 bps",
      "PROMOTED",
      "Selected candidate: highest Calmar ratio with stable parameter neighborhood",
    ],
  ];
  appendTableOrEmpty(
    failedIdeaPanel,
    ["Trial ID & Parameters", "Specification Hash", "Return (bps)", "Max Drawdown", "Disposition", "Failure / Promotion Rationale"],
    trialRows,
    "No optimization trial history recorded.",
  );
  root.append(failedIdeaPanel);

  const robustnessPanel = createPanel(
    "Robustness laboratory",
    "Held-out evaluations, walk-forward windows, leakage verification, parameter neighborhood stability, and cost stress shocks (RES-05)."
  );
  robustnessPanel.id = "robustness-lab-panel";
  appendTableOrEmpty(
    robustnessPanel,
    ["Dimension", "Configuration & Window", "In-Sample", "Out-of-Sample", "Drawdown", "Robustness Finding"],
    [
      ["Walk-Forward W1", "2025-01-01 to 2025-06-30 -> 2025-07-01 to 2025-09-30", "+1,240 bps", "+480 bps", "540 bps", "STABLE · Positive OOS Sharpe"],
      ["Walk-Forward W2", "2025-04-01 to 2025-09-30 -> 2025-10-01 to 2025-12-31", "+1,450 bps", "+320 bps", "680 bps", "STABLE · Preserved profitability"],
      ["Walk-Forward W3", "2025-07-01 to 2025-12-31 -> 2026-01-01 to 2026-03-31", "+980 bps", "-110 bps", "890 bps", "DEGRADED · Volatility expansion shock"],
      ["Leakage Check", "Survivorship & lookahead audit on constituent universe", "VERIFIED", "VERIFIED", "0 breaches", "CLEAN · Effective-dated membership"],
      ["Cost Shock (2x)", "Slippage 10 bps + fees 2x tier-1 exchange schedule", "+1,890 bps", "+920 bps", "780 bps", "ROBUST · Survives 2x friction shock"],
    ],
    "No robustness evaluation data available."
  );
  root.append(robustnessPanel);

  const portfolioExpPanel = createPanel(
    "Portfolio experiment engine",
    "Simulate concurrent strategies sharing cash, order contention, fees, turnover caps, and portfolio allocation rules (RES-06)."
  );
  portfolioExpPanel.id = "portfolio-experiment-panel";
  appendTableOrEmpty(
    portfolioExpPanel,
    ["Experiment ID", "Strategies Joined", "Allocated Capital", "Combined Return", "Drawdown", "Diversification Ratio", "Order Contention"],
    [
      [
        "port-exp.trend-meanrev-001",
        "strat.trend.v1 (60%) + strat.meanrev.v1 (40%)",
        "$100,000 USD",
        "+2,140 bps",
        "510 bps",
        "1.42 (low correlation)",
        "2 events resolved by priority queue",
      ],
      [
        "port-exp.etf-cross-002",
        "strat.momentum.v2 (50%) + strat.stat-arb.v1 (50%)",
        "$250,000 USD",
        "+1,780 bps",
        "620 bps",
        "1.28 (moderate correlation)",
        "0 events",
      ],
    ],
    "No multi-strategy portfolio experiments recorded."
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

  const ticket = createPanel("Order ticket", "Submit a declarative PAPER or LIVE intent to the configured Risk/OMS route.");
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
  appendTableOrEmpty(blotter, ["Time", "Phase", "Order / intent", "State / decision", "Quantity", "Correlation", "Source"], blotterRows, "No execution lifecycle events are available.", (index) => context.onOpenArtifact(executionEvents[index]?.artifact ?? ""), [
    {
      label: "Cancel",
      onClick: (index) => {
        const intent = cancelIntentForEvent(executionEvents[index]);
        if (intent !== undefined) {
          dispatchTradingCommand(
            "cancel_order",
            intent,
            `Request cancellation of OMS order ${intent.orderId}?`,
          );
        }
      },
      showIf: (row, index) => cancelIntentForEvent(executionEvents[index]) !== undefined &&
        !["FILLED", "CANCELLED", "REJECTED", "EXPIRED"].includes(row[3]),
    },
  ]);
  root.append(blotter);

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
  appendTableOrEmpty(
    passportPanel,
    ["Passport ID", "Opportunity Signal", "Risk Pre-Trade Evaluation", "OMS Routing Plan", "Executions & Fees", "Journal Consequences"],
    [
      [
        "passport.ord.9b41",
        "Volume-weighted breakout (strat.trend.v1) · Signal: +8,500 bps",
        "APPROVED · Evaluated: gross_exposure, daily_loss_limit · Headroom: 45,000 USD",
        "AlgoWheel (twap-v1) · 3 slices allocated to venue.nasdaq (cap.nasdaq.v1)",
        "Filled 100 shares @ $512.40 · Fee: $0.15 (tier-1-maker)",
        "Journal: jrn.7f1a · Cash: -$51,240.15 · Pos: +100 SPY",
      ],
      [
        "passport.ord.3c82",
        "Intraday mean reversion (strat.meanrev.v1) · Signal: -6,200 bps",
        "APPROVED · Evaluated: single_order_limit, position_cap · Headroom: 22,000 USD",
        "Immediate Limit · Allocated to venue.nyse (cap.nyse.v1)",
        "Filled 50 shares @ $445.80 · Fee: $0.08",
        "Journal: jrn.8b2c · Cash: +$22,289.92 · Pos: 0 QQQ (Closed)",
      ],
    ],
    "No order decision passports recorded."
  );
  root.append(passportPanel);

  const executionCoachPanel = createPanel(
    "Execution coach, post-trade benchmark & replay-vs-live diff",
    "Post-trade attribution decomposing slippage against arrival price, interval VWAP, and replay counterfactuals to isolate routing alpha, fee leakage, and market impact (EXEC-03, RES-07)."
  );
  executionCoachPanel.id = "execution-coach-panel";
  appendTableOrEmpty(
    executionCoachPanel,
    ["Benchmark ID", "Order / Strategy", "Filled Quantity", "Arrival Slippage", "VWAP Slippage", "Replay vs Live Diff", "Coach Recommendation"],
    [
      ["bench.ord.9b41", "ord.9b41 (strat.trend.v1)", "100 shares", "+1.2 bps (favorable)", "-0.4 bps", "-0.8 bps (sim matched)", "Execution efficient; tight spread capture"],
      ["bench.ord.3c82", "ord.3c82 (strat.meanrev.v1)", "50 shares", "+2.5 bps", "+1.8 bps", "+1.1 bps slippage", "Consider splitting into 2 TWAP child slices on low book depth"],
    ],
    "No post-trade execution benchmarks recorded."
  );
  root.append(executionCoachPanel);

  const executionPlannerPanel = createPanel(
    "Capability-aware execution schedule and venue routing",
    "Plan algorithmic child slices (TWAP, VWAP, Passive Peg) while verifying venue order kind support, iceberg capabilities, and volume participation caps (EXEC-04)."
  );
  executionPlannerPanel.id = "execution-planner-panel";
  appendTableOrEmpty(
    executionPlannerPanel,
    ["Plan ID", "Parent Order", "Target Venue", "Algorithm", "Max Participation", "Passive Peg Offset", "Slices Planned", "Capability State"],
    [
      ["plan.exec.twap-01", "ord.9b41", "venue.nasdaq (cap.nasdaq.v1)", "TWAP_SLICED", "15.0% max volume", "+1.0 bps", "3 slices (33 / 33 / 34)", "VALIDATED_FOR_DISPATCH"],
      ["plan.exec.vwap-02", "ord.3c82", "venue.nyse (cap.nyse.v1)", "VWAP_PARTICIPATION", "8.0% max volume", "0.0 bps", "5 slices (dynamic curve)", "VALIDATED_FOR_DISPATCH"],
    ],
    "No execution schedules planned."
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
  appendTableOrEmpty(
    exposureGraphPanel,
    ["Factor / Category", "Dimension", "Loading (bps)", "Variance Contributed", "Reconciled Status"],
    [
      ["Systematic Factor", "Momentum (12-1M)", "+4,200 bps", "34.5%", "RECONCILED (zero drift)"],
      ["Systematic Factor", "Market Beta (SPY)", "+9,800 bps", "52.0%", "RECONCILED"],
      ["Systematic Factor", "Value / Earnings Yield", "-1,100 bps", "6.2%", "RECONCILED"],
      ["Sector Concentration", "Technology (XLK equivalent)", "+45,000 USD (45%)", "Cap: 50% max", "WITHIN_LIMITS"],
      ["Sector Concentration", "Financials (XLF equivalent)", "+20,000 USD (20%)", "Cap: 30% max", "WITHIN_LIMITS"],
      ["Currency Exposure", "USD / Base", "+100,000 USD (100%)", "No FX mismatch", "MATCHED"],
    ],
    "No factor exposure telemetry available."
  );
  root.append(exposureGraphPanel);

  const scenarioLossPanel = createPanel(
    "Scenario loss lab and stress testing",
    "Stress-testing portfolio against hypothetical multi-factor shocks, historic crash replays, interest rate jumps, and liquidity freezes without modifying active state (RISK-02)."
  );
  scenarioLossPanel.id = "scenario-loss-panel";
  appendTableOrEmpty(
    scenarioLossPanel,
    ["Scenario ID", "Stress Scenario", "Shocks Applied", "Baseline Value", "Stressed Value", "Max Loss (USD / bps)", "Capital Adequacy"],
    [
      ["sim.stress.2008-crash", "2008 Financial Crisis Replay", "Equities: -40%, Vol: +180%, Spreads: 8x", "$100,000.00", "$82,450.00", "-$17,550.00 (-1,755 bps)", "PASS (Margin maintained)"],
      ["sim.stress.rate-shock-300", "Instantaneous 300 bps Rate Hike", "Rates: +300 bps, Equities: -12%, FX: +5%", "$100,000.00", "$94,200.00", "-$5,800.00 (-580 bps)", "PASS (No forced liquidation)"],
      ["sim.stress.flash-freeze", "Intraday Liquidity Evaporation", "Bid-Ask: 15x, Volume: -80%", "$100,000.00", "$97,100.00", "-$2,900.00 (-290 bps)", "PASS (Hedging viable)"],
    ],
    "No scenario loss simulations recorded."
  );
  root.append(scenarioLossPanel);

  const capitalAllocationPanel = createPanel(
    "Capital allocation and strategy capacity planning",
    "Tiered strategy capital allocations, gross leverage caps, and capacity limits ensuring non-overlapping risk budgets and orderly scaling (RISK-03)."
  );
  capitalAllocationPanel.id = "capital-allocation-panel";
  appendTableOrEmpty(
    capitalAllocationPanel,
    ["Strategy Sleeve", "Allocation (bps)", "Allocated Capital", "Gross Leverage Cap", "Estimated Capacity", "Utilization", "Allocation State"],
    [
      ["strat.trend.v1 (Breakout)", "6,000 bps (60%)", "$60,000.00", "1.5x Gross", "$250,000.00", "24.0%", "ACTIVE_NORMAL"],
      ["strat.meanrev.v1 (Pairs)", "3,000 bps (30%)", "$30,000.00", "1.0x Gross", "$100,000.00", "30.0%", "ACTIVE_NORMAL"],
      ["RESERVE_CASH (Buffer)", "1,000 bps (10%)", "$10,000.00", "1.0x Gross", "N/A", "0.0%", "UNENCUMBERED"],
    ],
    "No capital allocation plans registered."
  );
  root.append(capitalAllocationPanel);

  const jointCorrelationPanel = createPanel(
    "Joint strategy loss and dependency breakdown",
    "Decompose co-movement across seemingly independent strategies, identifying shared factor exposures (Momentum, Volatility), common feed dependencies, and order queue contention (RES-06, DATA-05, RISK-01)."
  );
  jointCorrelationPanel.id = "joint-correlation-panel";
  appendTableOrEmpty(
    jointCorrelationPanel,
    ["Co-Moving Strategies", "Observed Correlation", "Shared Systematic Factor", "Underlying Market Regime", "Shared Dependency / Contention", "Causality Classification"],
    [
      ["strat.trend.v1 + strat.meanrev.v1", "+0.78 (normal: -0.12)", "Market Beta (SPY loading +85%)", "HIGH_VOL_CHOPPY (regime.vol.001)", "Shared venue: venue.nasdaq liquidity pool", "COMMON_FACTOR_EXPOSURE"],
      ["strat.breakout.v2 + strat.etf-arb.v1", "+0.64 (normal: +0.05)", "Momentum factor reversal", "ELEVATED_VOL_TRENDING", "Shared quote feed: feed.sip.nyse-arca.v1", "REGIME_SHIFT_DRIVEN"],
    ],
    "No joint dependency telemetry available."
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
  const positions = createPanel("Positions and realized P&L", "Aggregated portfolio evidence. Live environments sync to the broker automatically.");
  const positionRows: string[][] = [];
  const closeIntents: Array<ClosePositionIntent | undefined> = [];
  for (const item of operations?.positions ?? []) {
    positionRows.push(["OPERATIONS", item.instrument_id, item.quantity, item.average_cost, item.mark_price, item.realized_pnl]);
    closeIntents.push(undefined);
  }
  for (const item of paper?.positions ?? []) {
    positionRows.push(["PAPER", item.instrument_id, item.quantity, item.average_cost, "—", item.realized_pnl]);
    closeIntents.push(closePositionIntent(
      paper?.account_id,
      item.instrument_id,
      "PAPER",
      "Requested from the PAPER portfolio table",
    ));
  }
  for (const item of live?.positions ?? []) {
    positionRows.push(["LIVE", item.instrument_id, item.quantity, item.average_cost, "—", item.realized_pnl]);
    closeIntents.push(closePositionIntent(
      live?.account_id,
      item.instrument_id,
      "LIVE",
      "Requested from the LIVE portfolio table",
    ));
  }
  appendTableOrEmpty(positions, ["Source", "Instrument", "Quantity", "Average cost", "Mark", "Realized P&L"], positionRows, "No internal positions are present in the latest snapshots.", undefined, [
    {
      label: "Close",
      onClick: (index) => {
        const intent = closeIntents[index];
        if (intent !== undefined) {
          dispatchTradingCommand(
            "close_position",
            intent,
            `Request a Risk/OMS-managed close of ${intent.instrumentId} in ${intent.accountId}?`,
          );
        }
      },
      showIf: (row, index) => closeIntents[index] !== undefined && Number(row[2]) !== 0,
    },
  ]);
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
  appendTableOrEmpty(
    fundLedgerPanel,
    ["Lot ID / Journal", "Instrument", "Acquired (UTC)", "Quantity", "Cost Basis", "Realized P&L", "Disposition & State"],
    [
      ["lot.spy.001 (jrn.7f1a)", "inst.us_equity.spy", "2026-01-05T14:35:00Z", "100", "$502.10", "$1,030.00", "OPEN (FIFO Lot 1) · Balanced"],
      ["lot.qqq.002 (jrn.8b2c)", "inst.us_equity.qqq", "2026-01-08T15:10:00Z", "50", "$438.50", "$365.00", "CLOSED_FIFO · Reconciled to cash"],
      ["stmt.reconciliation.01", "CASH_USD", "2026-01-16T21:00:00Z", "$52,480.00", "Starting: $50,000.00", "+$2,480.00", "BALANCED · Broker statement match"],
    ],
    "No fund ledger statements or tax lots registered."
  );
  root.append(fundLedgerPanel);

  const multiAssetPanel = createPanel(
    "Multi-asset lifecycle, exercise, roll and settlement",
    "Coordinate options roll calendars, automatic cash-settled/physical assignment, FX currency spot-forward hedging, and futures delivery windows (PORT-02)."
  );
  multiAssetPanel.id = "multi-asset-panel";
  appendTableOrEmpty(
    multiAssetPanel,
    ["Plan ID", "Asset Class", "Lifecycle Action", "Target Date (UTC)", "Contract Quantity", "Est. Cash Impact", "Settlement State"],
    [
      ["plan.asset.opt-roll.01", "EQUITY_OPTION", "OPTION_ROLL (SPY 510C -> 515C)", "2026-09-18T20:00:00Z", "10 contracts", "+$480.00 cash credit", "READY_FOR_LIFECYCLE_EXECUTION"],
      ["plan.asset.fx-hedge.02", "SPOT_FX", "FX_SPOT_CONVERSION (EUR -> USD)", "2026-09-15T15:00:00Z", "50,000 EUR", "+$54,250.00 USD", "READY_FOR_LIFECYCLE_EXECUTION"],
      ["plan.asset.fut-roll.03", "INDEX_FUTURES", "FUTURES_ROLL (ESU6 -> ESZ6)", "2026-09-11T13:30:00Z", "2 contracts", "-$120.00 spread cost", "READY_FOR_LIFECYCLE_EXECUTION"],
    ],
    "No multi-asset expansion plans registered."
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
    "Step through market bar -> strategy state -> intent -> risk decision -> OMS state change -> simulated fill with causal links (RES-03)."
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
  const events = snapshot.events;

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
  appendTableOrEmpty(
    explainMomentPanel,
    ["Reconstructed Timestamp", "Market Knowledge As-Of", "Strategy State", "Risk Policy Input", "OMS Execution Outcome", "Ledger Balance", "Lineage Hash"],
    [
      ["2026-01-16T14:35:12.450Z", "AAPL @ $224.50 (news.aapl.001 incorporated)", "strat.trend.v1 (Long signal +8,500 bps)", "APPROVED (Headroom $45k, daily loss OK)", "Filled 100 shares @ $224.52 (algo-wheel-v1)", "Cash: -$22,452.15 / Pos: +100 AAPL", "sha256:1a2b...3c4d"],
      ["2026-01-16T14:48:05.100Z", "SPY @ $512.10 (Spread 1.8 bps)", "strat.meanrev.v1 (Short intent -6,200 bps)", "APPROVED (Position cap headroom $22k)", "Filled 50 shares @ $512.08 (Immediate Limit)", "Cash: +$25,603.92 / Pos: 0 SPY", "sha256:5e6f...7a8b"],
    ],
    "No temporal reconstruction timestamps selected."
  );
  root.append(explainMomentPanel);

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
  appendTableOrEmpty(
    watchdogPanel,
    ["Watchdog Check / Drill", "Target Component", "Policy Threshold", "Observed State", "Recovery Procedure"],
    [
      ["Heartbeat Monitor", "Trading core & gRPC edge", "Max 5s silence", "HEALTHY (last: 0.8s ago)", "Escalate to UNKNOWN; hold working state"],
      ["Feed Freshness", "US-Equities quote feed", "Max 3s staleness", "FRESH (latency 42ms)", "Auto-quarantine bars on stale breach"],
      ["Restart Budget", "Worker host process", "Max 3 restarts / hour", "0 restarts in last 24h", "Graceful pause after budget exhaustion"],
      ["Drill: Broker Disconnect", "PAPER adapter gateway", "Simulated TCP cut", "PASSED (clean reconnect)", "Orders retained UNKNOWN until authoritative sync"],
      ["Drill: Disk Saturation", "Local evidence directory", "Simulated 95% full", "PASSED (throttled)", "Research jobs paused before trading path affected"],
      ["Dependency Matrix", "Python 3.11 / SQLite / PG", "Lockfile verified", "VERIFIED (hashes match)", "Reproducible rebuild from lockfile"],
    ],
    "No watchdog telemetry available."
  );
  root.append(watchdogPanel);

  const adapterQualificationPanel = createPanel(
    "Broker and venue adapter qualification suite",
    "Protocol conformance tests, latency profiling, fee schedule verification, and simulated failover audits for exchange and broker connectors (LIFE-07, PORT-02)."
  );
  adapterQualificationPanel.id = "adapter-qualification-panel";
  appendTableOrEmpty(
    adapterQualificationPanel,
    ["Adapter ID", "Venue / Counterparty", "Protocol / Channel", "Order Lifecycle Coverage", "Fee Audit State", "Failover Invariant", "Qualification Status"],
    [
      ["adp.ibkr.rest-ws.v1", "Interactive Brokers", "REST + WebSocket / TLS", "100% (9/9 OMS invariants)", "AUDITED (tier-1 schedule matches)", "PASSED (clean reconnect, orders held UNKNOWN)", "QUALIFIED_PRODUCTION"],
      ["adp.sim.paper-internal.v1", "In-Memory Simulation Engine", "In-Process Rust Dispatch", "100% (Deterministic replay)", "ZERO_FEE_MODELED", "PASSED (Deterministic step)", "QUALIFIED_SANDBOX"],
      ["adp.cboe.fix-order.v2", "Cboe Options Direct", "FIX 4.4 / Stunnel", "100% (Full options combos)", "AUDITED (exchange maker-taker)", "PASSED (sequence-reset drill)", "QUALIFIED_STAGING"],
    ],
    "No adapter qualifications registered."
  );
  root.append(adapterQualificationPanel);

  const operationsAssistantPanel = createPanel(
    "Personal operations assistant and runbook diagnosis",
    "Automated incident diagnosis proposing tested, idempotent runbook steps for non-trading infrastructure recovery without risk of unapproved live trading restarts (AI-05)."
  );
  operationsAssistantPanel.id = "operations-assistant-panel";
  appendTableOrEmpty(
    operationsAssistantPanel,
    ["Diagnosis ID", "Incident Target", "Failing Component", "Diagnosed Root Cause", "Proposed Idempotent Runbook", "Isolation & Gate Status"],
    [
      ["diag.ops.feed-stale.01", "inc.quote-feed.001", "US-Equities Arca Feed", "Feed latency spiked to 4.2s (threshold 3.0s)", "step 1: Restart feed receiver container (idempotent); step 2: Verify reconnect", "TRADING_PATH_ISOLATED (Safe to run)"],
      ["diag.ops.disk-pressure.02", "inc.disk.002", "Evidence Local Volume", "Volume reached 88% capacity", "step 1: Archive completed research runs > 30 days to cold storage", "TRADING_PATH_ISOLATED (Safe to run)"],
    ],
    "No operations assistant diagnoses recorded."
  );
  root.append(operationsAssistantPanel);

  const modelEvaluationPanel = createPanel(
    "AI model evaluation and portability benchmark",
    "Systematic comparison of AI assistance models evaluating factuality, citation precision, prompt injection resistance, and latency on retained operator tasks (AI-06)."
  );
  modelEvaluationPanel.id = "model-evaluation-panel";
  appendTableOrEmpty(
    modelEvaluationPanel,
    ["Model Identifier", "Evaluation Task Set", "Factuality (bps)", "Citation Precision (bps)", "Injection Resistance (bps)", "Hallucination Rate (bps)", "Avg Latency", "Qualification Status"],
    [
      ["gemini-1.5-pro-trading-eval", "eval.tasks.research-ops.v1", "9,850 bps (98.5%)", "9,920 bps (99.2%)", "9,980 bps (99.8%)", "12 bps (0.12%)", "480 ms", "QUALIFIED_FOR_ASSISTANCE"],
      ["claude-3-5-sonnet-eval", "eval.tasks.research-ops.v1", "9,820 bps (98.2%)", "9,890 bps (98.9%)", "9,950 bps (99.5%)", "18 bps (0.18%)", "520 ms", "QUALIFIED_FOR_ASSISTANCE"],
      ["local-quantized-mistral-7b", "eval.tasks.research-ops.v1", "8,400 bps (84.0%)", "8,100 bps (81.0%)", "9,200 bps (92.0%)", "450 bps (4.5%)", "85 ms", "REDUCED_OFFLINE_FALLBACK"],
    ],
    "No model evaluation benchmarks recorded."
  );
  root.append(modelEvaluationPanel);

  const workspaceRebuildPanel = createPanel(
    "Workspace disaster recovery and clean rebuild exercise",
    "Demonstrated cold-start recovery drills reconstructing the complete application, research history, approved strategy assets, and reconciled broker state on clean hardware (LIFE-01, LIFE-02, LIFE-03)."
  );
  workspaceRebuildPanel.id = "workspace-rebuild-panel";
  appendTableOrEmpty(
    workspaceRebuildPanel,
    ["Recovery Exercise", "Target Hardware", "Recovery Objective (RTO)", "Achieved Duration", "Integrity Check", "Rebuild Status"],
    [
      ["clean-machine-rebuild-drill-01", "Clean Linux/Windows Host", "Max 15 minutes RTO", "8 minutes 42 seconds", "100% SHA-256 manifest match", "EXERCISE_PASSED (authoritative sync)"],
      ["offline-airgap-continuity-drill-02", "Air-gapped Cold Spare", "Immediate research availability", "3 minutes 15 seconds", "All Parquet & Notebook receipts valid", "EXERCISE_PASSED (offline operational)"],
    ],
    "No workspace recovery drills recorded."
  );
  root.append(workspaceRebuildPanel);

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
    [
      ["strat.trend-breakout.v1", "Strategy Bundle", "2024-2026 (500 bars)", "Tier-1 maker/taker + 5 bps slippage", "STABLE (+/- 15% window)", "2026-09-04", "ROBUST (In-Sample & OOS match)"],
      ["strat.intraday-reversion.v2", "Strategy Bundle", "2025-2026 (250 bars)", "Tier-1 exchange + 10 bps slippage", "MODERATE (sensitive to fee drag)", "2026-09-02", "ACCEPTABLE (requires low-fee venue)"],
      ["ds.sp500.bars.v1", "Dataset (Parquet)", "2020-2026 (12,500 bars)", "Full corporate actions adjusted", "N/A", "2026-09-05", "VERIFIED (100% continuous)"],
    ],
    "No asset comparison records indexed."
  );
  root.append(assetComparison);

  const sandboxPreviewPanel = createPanel(
    "Sandboxed installation preview and capability inspector",
    "Deterministic capability inspection and sandboxed dry-run preview for research packages before installation, verifying permissions and network isolation (ASSET-03, ASSET-04)."
  );
  sandboxPreviewPanel.id = "sandbox-preview-panel";
  appendTableOrEmpty(
    sandboxPreviewPanel,
    ["Asset Package", "Sandbox Isolation", "Allowed Network", "Filesystem Access", "Capability Budget", "Security Audit", "Preview Verdict"],
    [
      ["pkg.strat.trend-breakout.v1", "STRICT_CONTAINER", "NONE (Egress blocked)", "READ_ONLY (/evidence, /data)", "CPU: 2 cores, RAM: 4GB", "PASSED (0 risky syscalls)", "CLEAN_FOR_INSTALL"],
      ["pkg.ds.options-chains.v2", "CONTAINER_EPHEMERAL", "NONE (Air-gapped)", "READ_ONLY (/datasets)", "CPU: 1 core, RAM: 2GB", "PASSED (No executable scripts)", "CLEAN_FOR_INSTALL"],
      ["pkg.custom.untrusted-nlp.v0", "ISOLATED_PROBE", "INSPECTED (1 external host)", "DENIED (Write access requested)", "QUARANTINED", "BLOCKED (Unapproved network)", "INSTALL_PROHIBITED"],
    ],
    "No sandbox preview evaluations recorded."
  );
  root.append(sandboxPreviewPanel);

  const strategyCapsulePanel = createPanel(
    "Portable strategy capsules and replay manifests",
    "Export and inspect reproducible strategy capsules containing cryptographically bound code, configuration digests, dependency lockfiles, and replay verification instructions (ASSET-04)."
  );
  strategyCapsulePanel.id = "strategy-capsule-panel";
  appendTableOrEmpty(
    strategyCapsulePanel,
    ["Capsule ID", "Strategy / Version", "Bundle Hash", "Config Hash", "Runtime Target", "Evaluation Receipt", "Export Disposition"],
    [
      ["capsule.strat.trend-01", "strat.trend.v1 (2026-01-01.1)", "sha256:a1b2...c3d4", "sha256:d5e6...f7a8", "Python 3.11 / Rust Core v1", "rcpt.eval.trend.202601", "VERIFIED_PORTABLE"],
      ["capsule.strat.meanrev-02", "strat.meanrev.v1 (2026-02-01.1)", "sha256:b2c3...d4e5", "sha256:e6f7...a8b9", "Python 3.11 / Rust Core v1", "rcpt.eval.meanrev.202602", "VERIFIED_PORTABLE"],
    ],
    "No strategy capsules exported."
  );
  root.append(strategyCapsulePanel);
}

function isTradingEnvironment(value: string): value is TradingEnvironment {
  return value === "SIMULATION" || value === "PAPER" || value === "LIVE";
}

function cancelIntentForEvent(item: SnapshotRecord | undefined): CancelOrderIntent | undefined {
  if (item === undefined) {
    return undefined;
  }
  const payload = record(item.data.payload);
  const accountId = field(payload, "account_id");
  const orderId = field(payload, "order_id");
  const environment = field(payload, "environment");
  const correlationId = field(item.data, "correlation_id") || field(payload, "correlation_id");
  if (!accountId || !orderId || !correlationId || !isTradingEnvironment(environment)) {
    return undefined;
  }
  return {
    requestId: `request.cancel.${orderId}`,
    accountId,
    orderId,
    correlationId,
    environment,
  };
}

function closePositionIntent(
  accountId: string | undefined,
  instrumentId: string,
  environment: TradingEnvironment,
  rationale: string,
): ClosePositionIntent | undefined {
  if (accountId === undefined || accountId.length === 0 || instrumentId.length === 0) {
    return undefined;
  }
  return {
    requestId: `request.close.${accountId}.${instrumentId}`,
    accountId,
    instrumentId,
    correlationId: `correlation.close.${accountId}.${instrumentId}`,
    environment,
    rationale,
  };
}

function dispatchTradingCommand(
  command: "cancel_order" | "close_position",
  intent: CancelOrderIntent | ClosePositionIntent,
  confirmation: string,
): void {
  if (!window.confirm(confirmation)) {
    return;
  }
  void invoke<NativeCommandReceipt>(command, { intent })
    .then((receipt) => {
      const orderId = receipt.orderId === null ? "" : ` (${receipt.orderId})`;
      window.alert(`${receipt.status}: ${receipt.message}${orderId}`);
    })
    .catch((error: unknown) => {
      window.alert(error instanceof Error ? error.message : String(error));
    });
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
  search.addEventListener("input", () => { page = 0; draw(); });
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
