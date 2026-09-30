// Typed advanced-evidence rows shared by the workspaces.

import { appendTableOrEmpty } from "./panels.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

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
export function typedAdvancedEvidence<T extends object>(
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
export function appendAdvancedEvidenceRows<T extends object>(
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
export function appendUnavailableEvidence(panel: HTMLElement, detail: string): void {
  const empty = document.createElement("p");
  empty.className = "empty-state";
  empty.textContent = detail;
  panel.append(empty);
}
