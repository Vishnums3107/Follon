// Record access and text formatting helpers.

import { OperationsDashboard } from "../evidence/index.js";
import type { WorkspaceSnapshot } from "./types.js";

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

export function reconciliationText(clean: boolean | null | undefined, at: string | null | undefined): string {
  if (clean === undefined || clean === null) return "Not yet reconciled";
  return `${clean ? "Clean" : "Discrepancy"}${at === undefined || at === null ? "" : ` at ${formatTime(at)}`}`;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function record(value: unknown): Readonly<Record<string, unknown>> {
  return isRecord(value) ? value : {};
}

export function field(value: Readonly<Record<string, unknown>>, name: string): string {
  return text(value[name]);
}

export function text(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return String(value);
  return "[structured]";
}

export function shortHash(value: string): string {
  if (value.length <= 18) return value;
  return `${value.slice(0, 10)}…${value.slice(-6)}`;
}

export function displayName(value: string): string {
  return value.replaceAll("_", " ").replaceAll("-", " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function formatTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString();
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
