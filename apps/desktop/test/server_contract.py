"""Contract checks for the read-only dashboard artifact boundary."""

from __future__ import annotations

import base64
import importlib.util
import json
import os
import shutil
import tempfile
import unittest
from email.message import Message
from pathlib import Path


TEST_DIRECTORY = tempfile.TemporaryDirectory()
TEST_ROOT = Path(TEST_DIRECTORY.name)
STATIC_ROOT = TEST_ROOT / "static"
EVIDENCE_ROOT = TEST_ROOT / "evidence"
STATIC_ROOT.mkdir()
EVIDENCE_ROOT.mkdir()
os.environ["FOLLON_DASHBOARD_STATIC_ROOT"] = str(STATIC_ROOT)
os.environ["FOLLON_EVIDENCE_ROOT"] = str(EVIDENCE_ROOT)
os.environ["FOLLON_DASHBOARD_MODE"] = "development"

MODULE_PATH = Path(__file__).resolve().parents[1] / "server.py"
INDEX_PATH = Path(__file__).resolve().parents[1] / "index.html"
MAIN_SOURCE_PATH = Path(__file__).resolve().parents[1] / "src" / "main.ts"
WORKSPACE_SOURCE_PATH = Path(__file__).resolve().parents[1] / "src" / "workspaces.ts"
SPEC = importlib.util.spec_from_file_location("follon_dashboard_server", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("unable to load dashboard server")
server = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(server)


class DashboardServerContract(unittest.TestCase):
    def setUp(self) -> None:
        for path in EVIDENCE_ROOT.iterdir():
            if path.is_file() or path.is_symlink():
                path.unlink()
            elif path.is_dir():
                shutil.rmtree(path)

    def test_catalogs_supported_artifacts_and_rejects_traversal(self) -> None:
        (EVIDENCE_ROOT / "replay.ndjson").write_text(
            '{"event_type":"market.bar.v1"}\n', encoding="utf-8"
        )
        (EVIDENCE_ROOT / "operations-dashboard.json").write_text(
            '{"projection_fingerprint":"abc"}', encoding="utf-8"
        )
        (EVIDENCE_ROOT / "report.md").write_text("# Report", encoding="utf-8")
        (EVIDENCE_ROOT / "ignored.txt").write_text("ignored", encoding="utf-8")

        artifacts = server.list_evidence()
        self.assertEqual({item["name"] for item in artifacts}, {
            "replay.ndjson", "operations-dashboard.json", "report.md"
        })
        self.assertEqual(
            next(item for item in artifacts if item["name"] == "operations-dashboard.json")["feature"],
            "operations",
        )
        self.assertIsNone(server.evidence_file("../server.py"))
        self.assertIsNone(server.evidence_file("ignored.txt"))

    def test_recursively_indexes_supported_evidence_and_maps_all_features(self) -> None:
        nested = EVIDENCE_ROOT / "acceptance" / "run-a"
        nested.mkdir(parents=True)
        (nested / "artifact.json").write_text(
            '{"completion_manifest":true}', encoding="utf-8"
        )
        (nested / "paper.journal.ndjson").write_text(
            '{"environment":"PAPER"}\n', encoding="utf-8"
        )
        artifacts = server.list_evidence()
        names = {item["name"] for item in artifacts}
        self.assertIn("acceptance/run-a/artifact.json", names)
        self.assertIn("acceptance/run-a/paper.journal.ndjson", names)
        self.assertEqual(
            next(item for item in artifacts if item["name"].endswith("paper.journal.ndjson"))["feature"],
            "paper",
        )
        self.assertIsNotNone(server.evidence_file("acceptance/run-a/artifact.json"))
        self.assertIsNone(server.evidence_file("acceptance/../outside.json"))
        self.assertEqual(len(server.FEATURES), 8)
        self.assertTrue(all(feature["screens"] for feature in server.FEATURES))

    def test_symlink_escape_is_rejected(self) -> None:
        outside = TEST_ROOT / "outside.json"
        outside.write_text("{}", encoding="utf-8")
        link = EVIDENCE_ROOT / "linked.json"
        try:
            link.symlink_to(outside)
        except OSError:
            self.skipTest("symlink creation is not available to this user")
        self.assertIsNone(server.evidence_file(link.name))

    def test_basic_auth_uses_exact_credentials_and_handles_malformed_input(self) -> None:
        original_username = server.AUTH_USERNAME
        original_password = server.AUTH_PASSWORD
        server.AUTH_USERNAME = "operator"
        server.AUTH_PASSWORD = "a-long-local-password"
        try:
            handler = object.__new__(server.DashboardHandler)
            handler.headers = Message()
            self.assertFalse(handler.is_authorized())
            handler.headers["Authorization"] = "Basic not-valid-base64!"
            self.assertFalse(handler.is_authorized())
            del handler.headers["Authorization"]
            credentials = base64.b64encode(b"operator:a-long-local-password").decode("ascii")
            handler.headers["Authorization"] = f"Basic {credentials}"
            self.assertTrue(handler.is_authorized())
        finally:
            server.AUTH_USERNAME = original_username
            server.AUTH_PASSWORD = original_password

    def test_workspace_projection_integrates_typed_feature_evidence(self) -> None:
        (EVIDENCE_ROOT / "market-bars.csv").write_text(
            "event_time,instrument_id,close\n2026-01-01T00:00:00Z,inst.spy,100.0\n",
            encoding="utf-8",
        )
        (EVIDENCE_ROOT / "backtest.json").write_text(json.dumps({
            "artifact_schema_version": 2,
            "artifact_fingerprint": "a" * 64,
            "event_output_hash": "b" * 64,
            "performance": {"trade_count": 1, "net_pnl": "1.00000000"},
            "report": {"cash": "100.00000000"},
            "specification": {"strategy_bundle_hash": "c" * 64},
            "specification_fingerprint": "d" * 64,
        }), encoding="utf-8")
        (EVIDENCE_ROOT / "paper-dashboard.json").write_text(json.dumps({
            "dashboard_schema_version": 2,
            "environment": "PAPER",
            "account_id": "acct.paper",
        }), encoding="utf-8")
        (EVIDENCE_ROOT / "operations-dashboard.json").write_text(json.dumps({
            "dashboard_schema_version": 1,
            "projection_fingerprint": "e" * 64,
        }), encoding="utf-8")
        (EVIDENCE_ROOT / "options-dashboard.json").write_text(json.dumps({
            "option_dashboard_schema_version": 1,
            "analytics": [],
        }), encoding="utf-8")
        (EVIDENCE_ROOT / "events.ndjson").write_text(json.dumps({
            "event_id": "evt-1",
            "event_type": "order.state_changed.v1",
            "event_time": "2026-01-01T00:00:00Z",
            "payload": {"new_state": "ACKNOWLEDGED"},
        }) + "\n", encoding="utf-8")
        (EVIDENCE_ROOT / "spy.parquet.receipt.json").write_text(json.dumps({
            "storage_receipt_schema_version": 1,
            "dataset_id": "dataset.spy",
            "dataset_version": "v1",
            "parquet_sha256": "a" * 64,
            "source_sha256": "b" * 64,
            "row_count": 2,
            "parquet_file": "spy.parquet",
            "starts_at": "2026-01-02T14:31:00Z",
            "ends_at": "2026-01-02T14:32:00Z",
        }), encoding="utf-8")
        (EVIDENCE_ROOT / "commercial.ndjson").write_text(json.dumps({
            "ledger_schema_version": 1,
            "sequence": 1,
            "event_id": "commercial-1",
            "event_type": "commercial.tenant_provisioned.v1",
            "occurred_at": "2026-01-01T00:00:00Z",
            "record_hash": "f" * 64,
        }) + "\n", encoding="utf-8")

        snapshot = server.workspace_snapshot()
        self.assertEqual(snapshot["workspace_schema_version"], 1)
        self.assertTrue(snapshot["read_only"])
        self.assertEqual(snapshot["counts"]["datasets"], 2)
        parquet_dataset = next(item for item in snapshot["datasets"] if item["name"] == "spy.parquet.receipt.json")
        self.assertEqual(parquet_dataset["dataset_id"], "dataset.spy")
        self.assertEqual(parquet_dataset["dataset_version"], "v1")
        self.assertEqual(parquet_dataset["storage_format"], "Parquet")
        self.assertEqual(parquet_dataset["content_sha256"], "a" * 64)
        self.assertEqual(snapshot["counts"]["backtests"], 1)
        self.assertEqual(snapshot["counts"]["events"], 1)
        self.assertEqual(snapshot["counts"]["commercial_records"], 1)
        self.assertEqual(snapshot["paper"]["artifact"], "paper-dashboard.json")
        self.assertEqual(snapshot["operations"]["artifact"], "operations-dashboard.json")
        self.assertEqual(snapshot["options"]["artifact"], "options-dashboard.json")

    def test_documented_primary_screens_are_integrated_in_static_shell(self) -> None:
        source = INDEX_PATH.read_text(encoding="utf-8")
        screens = {
            "Command Center",
            "Research Lab",
            "Strategy Studio",
            "Backtest Explorer",
            "Execution Blotter",
            "Risk Cockpit",
            "Portfolio",
            "Replay &amp; Incidents",
            "Journal",
            "Administration",
        }
        for screen in screens:
            self.assertIn(screen, source)
        self.assertEqual(source.count("data-workspace="), len(screens))
        self.assertEqual(source.count('data-workspace="command-center"'), 1)
        self.assertIn('id="workspace-detail"', source)
        self.assertIn('id="workspace-summary"', source)
        self.assertIn('id="workspace-canvas"', source)
        self.assertIn('id="refresh-workspace"', source)
        self.assertIn('id="workspace-evidence"', source)
        self.assertIn('id="coverage-summary"', source)
        self.assertIn('id="artifact-search"', source)
        self.assertIn('href="/favicon.svg"', source)
        self.assertTrue((INDEX_PATH.parent / "favicon.svg").is_file())

        runtime_source = MAIN_SOURCE_PATH.read_text(encoding="utf-8")
        self.assertIn('const pathPrefix = "/workspace/"', runtime_source)
        self.assertIn('`/#workspace/${encodeURIComponent(workspace.id)}`', runtime_source)
        self.assertIn('fetch("/api/v1/workspaces"', runtime_source)
        self.assertIn('from "./evidence.js"', runtime_source)
        self.assertIn('from "./catalog.js"', runtime_source)
        self.assertIn('from "./workspaces.js"', runtime_source)

        workspace_source = WORKSPACE_SOURCE_PATH.read_text(encoding="utf-8")
        for renderer in (
            "renderCommandCenter", "renderResearchLab", "renderStrategyStudio",
            "renderBacktestExplorer", "renderExecutionBlotter", "renderRiskCockpit",
            "renderPortfolio", "renderReplayAndIncidents", "renderJournal",
            "renderAdministration",
        ):
            self.assertIn(f"function {renderer}", workspace_source)


if __name__ == "__main__":
    unittest.main()
