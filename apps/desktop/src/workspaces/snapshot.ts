// Parsing and validating the server-owned workspace snapshot.

import { LiveMonitoringDashboard, OperationsDashboard, OptionsDashboard, PaperDashboard, parseLiveMonitoringDashboard, parseOperationsDashboard, parseOptionsDashboard, parsePaperDashboard } from "../evidence/index.js";
import { isRecord } from "./format.js";
import type { BacktestSummary, DatasetSummary, EventWindow, EvidenceArtifact, NotebookSummary, ProjectionDiagnostic, SnapshotDashboard, SnapshotRecord, WorkspaceEventWindow, WorkspaceSnapshot } from "./types.js";

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

export function paperDashboard(snapshot: WorkspaceSnapshot): PaperDashboard | undefined {
  return parseDashboard(snapshot.paper, parsePaperDashboard);
}

export function liveDashboard(snapshot: WorkspaceSnapshot): LiveMonitoringDashboard | undefined {
  return parseDashboard(snapshot.live, parseLiveMonitoringDashboard);
}

export function operationsDashboard(snapshot: WorkspaceSnapshot): OperationsDashboard | undefined {
  return parseDashboard(snapshot.operations, parseOperationsDashboard);
}

export function optionsDashboard(snapshot: WorkspaceSnapshot): OptionsDashboard | undefined {
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
    isRecord(value.report) && isRecord(value.specification) &&
    (value.advanced_account === undefined || value.advanced_account === null || isRecord(value.advanced_account));
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
