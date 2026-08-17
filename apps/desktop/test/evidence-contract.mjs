import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { parseOperationsDashboard, parseOptionsDashboard } from "../dist/evidence.js";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(testDirectory, "..", "..", "..");
const temporaryDirectory = mkdtempSync(join(tmpdir(), "follon-evidence-contract-"));

function runCargo(cargoArgs) {
  execFileSync("cargo", cargoArgs, {
    cwd: repositoryRoot,
    encoding: "utf8",
    stdio: "pipe",
  });
}

try {
  const optionsDashboardPath = join(temporaryDirectory, "options-dashboard.json");
  const operationsDashboardPath = join(temporaryDirectory, "operations-dashboard.json");
  const journalPath = join(temporaryDirectory, "operations.journal.ndjson");

  runCargo([
    "run", "-q", "-p", "follon-cli", "--bin", "follon-options", "--",
    "analyze", "tests/fixtures/config/options-v1.json", optionsDashboardPath,
  ]);
  runCargo([
    "run", "-q", "-p", "follon-cli", "--bin", "follon-operations", "--",
    "dashboard", "tests/fixtures/config/operations-v1.json", operationsDashboardPath,
    "--as-of", "2026-08-10T16:30:00Z", "--journal", journalPath,
  ]);

  const optionsDashboard = parseOptionsDashboard(readFileSync(optionsDashboardPath, "utf8"));
  const operationsDashboard = parseOperationsDashboard(readFileSync(operationsDashboardPath, "utf8"));

  assert.equal(optionsDashboard.reconciliation.clean, true);
  assert.equal(operationsDashboard.journal.healthy, true);

  const prematurelyReconciled = structuredClone(optionsDashboard);
  prematurelyReconciled.reconciliation.reconciled_at = "2026-08-10T16:29:59Z";
  assert.throws(() => parseOptionsDashboard(JSON.stringify(prematurelyReconciled)));

  const mismatchedCleanIdentity = structuredClone(optionsDashboard);
  mismatchedCleanIdentity.reconciliation.paper_book.run_identity_hash = "f".repeat(64);
  assert.throws(() => parseOptionsDashboard(JSON.stringify(mismatchedCleanIdentity)));

  const impossibleJournal = structuredClone(operationsDashboard);
  impossibleJournal.journal.failure_reason = "verification failed";
  assert.throws(() => parseOperationsDashboard(JSON.stringify(impossibleJournal)));

  process.stdout.write("CLI dashboard / desktop evidence-contract test passed\n");
} finally {
  rmSync(temporaryDirectory, { recursive: true, force: true });
}
