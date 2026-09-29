// Workspace snapshot and evidence-artifact types shared by every workspace.

import { FeatureDefinition, SystemStatus } from "../catalog.js";

export type EvidenceArtifact = Readonly<{
  name: string;
  bytes: number;
  modified_at: string;
  feature: string;
  kind: string;
  format: "ndjson" | "json" | "markdown" | "csv" | "text";
}>;

export type SnapshotRecord = Readonly<{
  artifact: string;
  feature?: string;
  category?: string;
  modified_at?: string;
  data: Readonly<Record<string, unknown>>;
}>;

export type EventWindow = Readonly<{
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

export type WorkspaceEventWindow = Readonly<{
  window_kind: "causal_prefix";
  source_event_count_lower_bound: number;
  retained_event_count: number;
  truncated: boolean;
}>;

export type ProjectionDiagnostic = Readonly<{
  artifact: string;
  code: string;
  detail: string;
}>;

export type DatasetSummary = Readonly<{
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

export type NotebookSummary = Readonly<{
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

export type BacktestSummary = Readonly<{
  artifact: string;
  modified_at: string;
  artifact_fingerprint: unknown;
  event_output_hash: unknown;
  performance: Readonly<Record<string, unknown>>;
  report: Readonly<Record<string, unknown>>;
  specification: Readonly<Record<string, unknown>>;
  specification_fingerprint: unknown;
  /** Complete advanced-account economics carried by a schema-3 artifact. */
  advanced_account?: Readonly<Record<string, unknown>> | null;
}>;

export type SnapshotDashboard = Readonly<{
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
