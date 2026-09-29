// Journal workspace.

import { displayName, field, record, shortHash, text } from "./format.js";
import { appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics } from "./panels.js";
import { liveDashboard, operationsDashboard, paperDashboard } from "./snapshot.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

export function renderJournal(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
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

function keyValueText(value: unknown): string {
  return Object.entries(record(value))
    .map(([key, item]) => `${displayName(key)}: ${text(item)}`)
    .join(" | ");
}
