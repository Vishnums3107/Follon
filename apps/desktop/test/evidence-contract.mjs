import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { parseEvidenceLog, parseOperationsDashboard, parseOptionsDashboard } from "../dist/evidence.js";
import { parseWorkspaceSnapshot } from "../dist/workspaces.js";

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

  const canonicalEvent = {
    event_id: "evt.fixture.001",
    event_type: "market.bar.v1",
    schema_version: 1,
    event_time: "2026-08-21T00:00:00Z",
    receive_time: "2026-08-21T00:00:00Z",
    account_id: null,
    strategy_id: null,
    instrument_id: "inst.us_equity.spy",
    correlation_id: "corr.fixture.001",
    causation_id: null,
    actor: "fixture",
    source: "test",
    payload: { close: "100.00000000" },
    software_version: "follon-test",
    configuration_version: "fixture-v1",
  };
  assert.doesNotThrow(() => parseEvidenceLog(`${JSON.stringify(canonicalEvent)}\n`));
  assert.throws(() => parseEvidenceLog(`${JSON.stringify({ ...canonicalEvent, schema_version: "1" })}\n`));
  assert.throws(() => parseEvidenceLog(`${JSON.stringify({ ...canonicalEvent, unversioned_extra: true })}\n`));

  const validWorkspace = {
    workspace_schema_version: 1,
    generated_at: "2026-08-21T00:00:00Z",
    read_only: true,
    counts: { artifacts: 1, datasets: 1, notebooks: 1, backtests: 1, experiments: 0, events: 0, journals: 0, commercial_records: 0 },
    feature_artifact_counts: { "market-data": 1, replay: 0, research: 0, paper: 0, "controlled-live": 0, operations: 0, options: 0, commercial: 0, "execution-risk": 0, accounting: 0, identity: 0, platform: 0, news: 0 },
    datasets: [{ name: "bars.csv", modified_at: "2026-08-21T00:00:00Z", bytes: 12, columns: ["close"], rows: 1 }],
    notebooks: [{ artifact: "research.ipynb", modified_at: "2026-08-21T00:00:00Z", bytes: 100, nbformat: 4, cell_count: 2, code_cells: 1, markdown_cells: 1, output_count: 0, kernel: "Python 3", language: "python" }],
    backtests: [{ artifact: "run.json", modified_at: "2026-08-21T00:00:00Z", artifact_fingerprint: "a".repeat(64), event_output_hash: "b".repeat(64), performance: {}, report: {}, specification: {}, specification_fingerprint: "c".repeat(64) }],
    experiments: [], manifests: [], events: [], journals: [], commercial: [], execution_evidence: [], paper: null, live: null, operations: null, options: null,
    commercial_artifacts: [],
  };
  assert.doesNotThrow(() => parseWorkspaceSnapshot(validWorkspace));
  const invalidDataset = structuredClone(validWorkspace);
  invalidDataset.datasets[0].rows = -1;
  assert.throws(() => parseWorkspaceSnapshot(invalidDataset));
  const invalidBacktest = structuredClone(validWorkspace);
  invalidBacktest.backtests[0].artifact_fingerprint = null;
  assert.throws(() => parseWorkspaceSnapshot(invalidBacktest));
  const invalidNotebook = structuredClone(validWorkspace);
  invalidNotebook.notebooks[0].code_cells = 3;
  assert.throws(() => parseWorkspaceSnapshot(invalidNotebook));

  const realProjection = JSON.parse(execFileSync("python", ["-c",
    "import json,runpy; print(json.dumps(runpy.run_path('apps/desktop/server.py')['workspace_snapshot'](), separators=(',', ':')))",
  ], {
    cwd: repositoryRoot,
    env: {
      ...process.env,
      FOLLON_DASHBOARD_STATIC_ROOT: resolve(repositoryRoot, "apps", "desktop"),
      FOLLON_EVIDENCE_ROOT: resolve(repositoryRoot, "var"),
      FOLLON_DASHBOARD_MODE: "development",
      FOLLON_DASHBOARD_USERNAME: "",
      FOLLON_DASHBOARD_PASSWORD: "",
      FOLLON_DASHBOARD_PASSWORD_FILE: "",
    },
    encoding: "utf8",
    stdio: "pipe",
  }));
  const parsedRealProjection = parseWorkspaceSnapshot(realProjection);
  assert.equal(parsedRealProjection.read_only, true);
  assert.deepEqual(Object.keys(parsedRealProjection.feature_artifact_counts).sort(), [
    "accounting", "commercial", "controlled-live", "execution-risk", "identity", "market-data", "news", "operations", "options", "paper", "platform", "replay", "research",
  ]);

  const newsEvidencePath = join(temporaryDirectory, "news-replay.ndjson");
  writeFileSync(newsEvidencePath, [
    JSON.stringify({
      event_id: "evt-news-000001", event_type: "news.headline.v1", event_time: "2026-09-01T11:00:00Z",
      correlation_id: "corr-news-000001", causation_id: null, actor: "news-ingest", source: "fixture",
      schema_version: 1, receive_time: "2026-09-01T11:00:00Z", account_id: null, strategy_id: null,
      instrument_id: "inst.us_equity.spy", software_version: "follon-test", configuration_version: "news-fixture-v1", payload: {
        news_id: "news.fixture.001", source: "DOW_JONES", headline: "Apple reports earnings beat",
        raw_body_hash: "a".repeat(64), sequence_number: 1, event_time_ns: 1788260400000000000,
        receive_time_ns: 1788260400000000001, entity_tickers: ["inst.us_equity.spy"],
      },
    }),
    JSON.stringify({
      event_id: "evt-news-000002", event_type: "news.sentiment.v1", event_time: "2026-09-01T11:00:00Z",
      correlation_id: "corr-news-000001", causation_id: "evt-news-000001", actor: "news-classifier", source: "fixture",
      schema_version: 1, receive_time: "2026-09-01T11:00:00Z", account_id: null, strategy_id: null,
      instrument_id: "inst.us_equity.spy", software_version: "follon-test", configuration_version: "news-fixture-v1", payload: {
        event_id: "sent.news.fixture.001.1", causation_news_id: "news.fixture.001", event_time_ns: 1788260400000000000,
        instrument_id: "inst.us_equity.spy", taxonomy: "EARNINGS_RELEASE", sentiment_polarity_bps: 9000,
        confidence_bps: 9000, novelty_score_bps: 10000, surprise_magnitude_bps: 250,
      },
    }),
  ].join("\n") + "\n", "utf8");
  const newsProjection = JSON.parse(execFileSync("python", ["-c",
    "import json,runpy; print(json.dumps(runpy.run_path('apps/desktop/server.py')['workspace_snapshot'](), separators=(',', ':')))",
  ], {
    cwd: repositoryRoot,
    env: {
      ...process.env,
      FOLLON_DASHBOARD_STATIC_ROOT: resolve(repositoryRoot, "apps", "desktop"),
      FOLLON_EVIDENCE_ROOT: temporaryDirectory,
      FOLLON_DASHBOARD_MODE: "development",
      FOLLON_DASHBOARD_USERNAME: "",
      FOLLON_DASHBOARD_PASSWORD: "",
      FOLLON_DASHBOARD_PASSWORD_FILE: "",
    },
    encoding: "utf8",
    stdio: "pipe",
  }));
  const parsedNewsProjection = parseWorkspaceSnapshot(newsProjection);
  assert.equal(parsedNewsProjection.feature_artifact_counts.news, 1);
  assert.deepEqual(
    parsedNewsProjection.events.map((event) => event.data.event_type).sort(),
    ["news.headline.v1", "news.sentiment.v1"],
  );

  process.stdout.write("CLI dashboard / desktop evidence-contract test passed\n");
} finally {
  rmSync(temporaryDirectory, { recursive: true, force: true });
}
