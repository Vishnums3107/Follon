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
from http import HTTPStatus
from pathlib import Path
from threading import Thread
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.request import Request, urlopen


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
APP_SHELL_SOURCE_PATH = Path(__file__).resolve().parents[1] / "src" / "app-shell.tsx"
SPEC = importlib.util.spec_from_file_location("follon_dashboard_server", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("unable to load dashboard server")
server = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(server)


class DashboardServerContract(unittest.TestCase):
    def setUp(self) -> None:
        server.clear_authentication_failures("127.0.0.1")
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
        (EVIDENCE_ROOT / "research.ipynb").write_text(json.dumps({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {},
            "cells": [],
        }), encoding="utf-8")
        (EVIDENCE_ROOT / "ignored.txt").write_text("ignored", encoding="utf-8")

        artifacts = server.list_evidence()
        self.assertEqual({item["name"] for item in artifacts}, {
            "replay.ndjson", "operations-dashboard.json", "report.md", "research.ipynb"
        })
        notebook = next(item for item in artifacts if item["name"] == "research.ipynb")
        self.assertEqual(notebook["feature"], "research")
        self.assertEqual(notebook["kind"], "Research notebook")
        self.assertEqual(
            next(item for item in artifacts if item["name"] == "operations-dashboard.json")["feature"],
            "operations",
        )
        self.assertIsNone(server.evidence_file("../server.py"))
        self.assertIsNone(server.evidence_file("ignored.txt"))

    def test_tauri_read_only_origin_is_exactly_allow_listed(self) -> None:
        original_static_root = server.STATIC_ROOT
        server.STATIC_ROOT = MODULE_PATH.parent.resolve()
        httpd = server.ThreadingHTTPServer(("127.0.0.1", 0), server.DashboardHandler)
        thread = Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        endpoint = f"http://127.0.0.1:{httpd.server_port}/api/v1/health"
        try:
            allowed = Request(endpoint, headers={"Origin": "http://tauri.localhost"})
            with urlopen(allowed, timeout=2) as response:
                self.assertEqual(
                    response.headers["Access-Control-Allow-Origin"],
                    "http://tauri.localhost",
                )
                self.assertEqual(
                    response.headers["Cross-Origin-Resource-Policy"],
                    "cross-origin",
                )
            denied = Request(endpoint, headers={"Origin": "https://untrusted.example"})
            with urlopen(denied, timeout=2) as response:
                self.assertIsNone(response.headers["Access-Control-Allow-Origin"])
                self.assertEqual(
                    response.headers["Cross-Origin-Resource-Policy"],
                    "same-origin",
                )
        finally:
            httpd.shutdown()
            httpd.server_close()
            thread.join(timeout=2)
            server.STATIC_ROOT = original_static_root

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
        self.assertEqual(len(server.FEATURES), 13)
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

    def test_authentication_failures_are_rate_limited_per_direct_peer(self) -> None:
        client = "127.0.0.1"
        for second in range(server.AUTH_FAILURE_LIMIT):
            server.record_authentication_failure(client, now=100.0 + second)
        self.assertEqual(
            server.authentication_retry_after(client, now=104.0),
            56,
        )
        self.assertEqual(server.authentication_retry_after("127.0.0.2", now=104.0), 0)
        self.assertEqual(server.authentication_retry_after(client, now=160.0), 0)

    def test_successful_authentication_clears_failure_budget(self) -> None:
        original_username = server.AUTH_USERNAME
        original_password = server.AUTH_PASSWORD
        server.AUTH_USERNAME = "operator"
        server.AUTH_PASSWORD = "a-long-local-password"
        client = "127.0.0.1"
        server.record_authentication_failure(client, now=100.0)
        try:
            handler = object.__new__(server.DashboardHandler)
            handler.path = "/"
            handler.client_address = (client, 45678)
            handler.headers = Message()
            credentials = base64.b64encode(b"operator:a-long-local-password").decode("ascii")
            handler.headers["Authorization"] = f"Basic {credentials}"
            handler.write_static = lambda path: self.assertEqual(path, "/")
            with patch.object(server, "monotonic", return_value=101.0):
                handler.do_GET()
            self.assertEqual(server.authentication_retry_after(client, now=101.0), 0)
        finally:
            server.AUTH_USERNAME = original_username
            server.AUTH_PASSWORD = original_password
            server.clear_authentication_failures(client)

    def test_http_boundary_returns_401_then_429_and_accepts_valid_credentials(self) -> None:
        original_username = server.AUTH_USERNAME
        original_password = server.AUTH_PASSWORD
        original_static_root = server.STATIC_ROOT
        server.AUTH_USERNAME = "operator"
        server.AUTH_PASSWORD = "a-long-local-password"
        server.STATIC_ROOT = MODULE_PATH.parent.resolve()
        client = "127.0.0.1"
        httpd = server.ThreadingHTTPServer((client, 0), server.DashboardHandler)
        thread = Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        origin = f"http://{client}:{httpd.server_port}"
        try:
            with urlopen(f"{origin}/api/v1/health", timeout=2) as response:
                self.assertEqual(response.status, HTTPStatus.OK)
            for _ in range(server.AUTH_FAILURE_LIMIT):
                with self.assertRaises(HTTPError) as error:
                    urlopen(f"{origin}/api/v1/features", timeout=2)
                self.assertEqual(error.exception.code, HTTPStatus.UNAUTHORIZED)
            with self.assertRaises(HTTPError) as error:
                urlopen(f"{origin}/api/v1/features", timeout=2)
            self.assertEqual(error.exception.code, HTTPStatus.TOO_MANY_REQUESTS)
            self.assertGreaterEqual(int(error.exception.headers["Retry-After"]), 1)

            server.clear_authentication_failures(client)
            credentials = base64.b64encode(b"operator:a-long-local-password").decode("ascii")
            request = Request(
                f"{origin}/api/v1/features",
                headers={"Authorization": f"Basic {credentials}"},
            )
            with urlopen(request, timeout=2) as response:
                self.assertEqual(response.status, HTTPStatus.OK)
                self.assertEqual(len(json.load(response)), len(server.FEATURES))
            for path in ("/", "/workspace/command-center", "/styles.css"):
                request = Request(
                    f"{origin}{path}",
                    headers={"Authorization": f"Basic {credentials}"},
                )
                with urlopen(request, timeout=2) as response:
                    self.assertEqual(response.status, HTTPStatus.OK)
                    self.assertIn("default-src 'self'", response.headers["Content-Security-Policy"])
            request = Request(
                f"{origin}/not-a-dashboard-route",
                headers={"Authorization": f"Basic {credentials}"},
            )
            with self.assertRaises(HTTPError) as error:
                urlopen(request, timeout=2)
            self.assertEqual(error.exception.code, HTTPStatus.NOT_FOUND)
        finally:
            httpd.shutdown()
            httpd.server_close()
            thread.join(timeout=2)
            server.AUTH_USERNAME = original_username
            server.AUTH_PASSWORD = original_password
            server.STATIC_ROOT = original_static_root
            server.clear_authentication_failures(client)

    def test_production_mode_rejects_missing_or_short_credentials(self) -> None:
        for username, password in (("", ""), ("operator", "too-short")):
            with self.subTest(username=username, password=password), patch.dict(os.environ, {
                "FOLLON_DASHBOARD_MODE": "production",
                "FOLLON_DASHBOARD_USERNAME": username,
                "FOLLON_DASHBOARD_PASSWORD": password,
                "FOLLON_DASHBOARD_PASSWORD_FILE": "",
            }, clear=False):
                spec = importlib.util.spec_from_file_location("follon_dashboard_production_check", MODULE_PATH)
                self.assertIsNotNone(spec)
                self.assertIsNotNone(spec.loader)
                candidate = importlib.util.module_from_spec(spec)
                with self.assertRaisesRegex(RuntimeError, "production dashboard mode requires"):
                    spec.loader.exec_module(candidate)

    def test_security_headers_block_privileged_browser_features(self) -> None:
        handler = object.__new__(server.DashboardHandler)
        captured: dict[str, str] = {}
        handler.send_header = lambda key, value: captured.__setitem__(key, value)
        handler._headers_buffer = []
        handler.request_version = "HTTP/1.1"
        handler.flush_headers = lambda: None
        server.DashboardHandler.end_headers(handler)
        self.assertEqual(captured["X-Robots-Tag"], "noindex, noarchive")
        self.assertEqual(captured["Cross-Origin-Resource-Policy"], "same-origin")
        self.assertEqual(captured["Cross-Origin-Opener-Policy"], "same-origin")
        self.assertIn("payment=()", captured["Permissions-Policy"])
        self.assertIn("form-action 'none'", captured["Content-Security-Policy"])

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
        (EVIDENCE_ROOT / "tca.json").write_text(json.dumps({
            "as_of": "2026-08-30T21:30:00Z",
            "input_sha256": "a" * 64,
            "transaction_cost": {
                "transaction_cost_schema_version": 1,
                "reports": [{"analysis_id": "tca.spy.1"}],
                "buckets": [],
            },
        }), encoding="utf-8")
        (EVIDENCE_ROOT / "model-risk-register.json").write_text(json.dumps({
            "model_risk_register_schema_version": 1,
            "records": [],
        }), encoding="utf-8")
        (EVIDENCE_ROOT / "events.ndjson").write_text(json.dumps({
            "event_id": "evt-1",
            "event_type": "order.state_changed.v1",
            "schema_version": 1,
            "event_time": "2026-01-01T00:00:00Z",
            "receive_time": "2026-01-01T00:00:00Z",
            "account_id": "acct.paper",
            "strategy_id": None,
            "instrument_id": "inst.us_equity.spy",
            "correlation_id": "corr-1",
            "causation_id": None,
            "actor": "oms",
            "source": "simulator",
            "payload": {"new_state": "ACKNOWLEDGED"},
            "software_version": "follon-test",
            "configuration_version": "paper-v1",
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
        (EVIDENCE_ROOT / "research.ipynb").write_text(json.dumps({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {
                "kernelspec": {"display_name": "Python 3", "name": "python3"},
                "language_info": {"name": "python"},
            },
            "cells": [
                {"cell_type": "markdown", "metadata": {}, "source": ["# Research"]},
                {"cell_type": "code", "metadata": {}, "source": ["1 + 1"], "execution_count": 1, "outputs": [{"output_type": "execute_result"}]},
            ],
        }), encoding="utf-8")

        snapshot = server.workspace_snapshot()
        self.assertEqual(snapshot["workspace_schema_version"], 1)
        self.assertTrue(snapshot["read_only"])
        self.assertEqual(snapshot["counts"]["datasets"], 2)
        self.assertEqual(snapshot["counts"]["notebooks"], 1)
        self.assertEqual(snapshot["notebooks"][0]["artifact"], "research.ipynb")
        self.assertEqual(snapshot["notebooks"][0]["code_cells"], 1)
        self.assertEqual(snapshot["notebooks"][0]["markdown_cells"], 1)
        self.assertEqual(snapshot["notebooks"][0]["output_count"], 1)
        oversized_notebook = server.summarize_notebook(
            {"name": "oversized.ipynb", "modified_at": "2026-08-21T00:00:00Z", "bytes": 1},
            {"nbformat": 4, "cells": [{}] * (server.MAX_WORKSPACE_RECORDS + 1)},
        )
        self.assertIsNone(oversized_notebook)
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
        self.assertEqual(snapshot["execution_evidence"][0]["artifact"], "tca.json")
        artifacts = {item["name"]: item for item in server.list_evidence()}
        self.assertEqual(artifacts["tca.json"]["feature"], "execution-risk")
        self.assertEqual(artifacts["model-risk-register.json"]["feature"], "operations")

    def test_workspace_projection_validates_event_envelopes_and_discloses_windows(self) -> None:
        def event(
            event_id: str,
            event_time: str,
            causation_id: str | None = None,
            receive_time: str | None = None,
        ) -> dict[str, object]:
            return {
                "event_id": event_id,
                "event_type": "market.bar.v1",
                "schema_version": 1,
                "event_time": event_time,
                "receive_time": receive_time or event_time,
                "account_id": None,
                "strategy_id": None,
                "instrument_id": "inst.us_equity.spy",
                "correlation_id": "corr.test",
                "causation_id": causation_id,
                "actor": "replay",
                "source": "fixture",
                "payload": {"instrument_id": "inst.us_equity.spy"},
                "software_version": "follon-test",
                "configuration_version": "replay-fixture-v1",
            }

        causal = [
            event("evt.root", "2099-01-01T00:00:00Z"),
            event("evt.child", "2000-01-01T00:00:00Z", "evt.root"),
            {"event_id": "evt.invalid", "event_type": "market.bar.v1", "event_time": "2026-01-01T00:00:00Z", "payload": {}},
        ]
        (EVIDENCE_ROOT / "causal.ndjson").write_text(
            "\n".join(json.dumps(record) for record in causal) + "\n",
            encoding="utf-8",
        )
        (EVIDENCE_ROOT / "unseen.ndjson").write_text(
            json.dumps(event("evt.unseen", "2026-01-01T00:00:00Z", "evt.absent")) + "\n",
            encoding="utf-8",
        )
        (EVIDENCE_ROOT / "a-cross-child.ndjson").write_text(
            json.dumps(event("evt.cross-child", "2026-01-01T00:00:00Z", "evt.cross-parent")) + "\n",
            encoding="utf-8",
        )
        (EVIDENCE_ROOT / "z-cross-parent.ndjson").write_text(
            json.dumps(event("evt.cross-parent", "2026-01-01T00:00:01Z")) + "\n",
            encoding="utf-8",
        )
        availability = [
            event("evt.availability.market", "2026-01-01T00:00:01Z"),
            event("evt.availability.late", "2026-01-01T00:00:00Z", receive_time="2026-01-01T00:00:02Z"),
        ]
        (EVIDENCE_ROOT / "availability.ndjson").write_text(
            "\n".join(json.dumps(record) for record in availability) + "\n",
            encoding="utf-8",
        )
        ordering = [
            event("evt.whole-second", "2026-01-01T00:00:00Z"),
            event("evt.fractional-second", "2026-01-01T00:00:00.500Z"),
            event("evt.tie.first", "2026-01-01T00:00:02Z"),
            event("evt.tie.second", "2026-01-01T00:00:02Z"),
        ]
        (EVIDENCE_ROOT / "ordering.ndjson").write_text(
            "\n".join(json.dumps(record) for record in ordering) + "\n",
            encoding="utf-8",
        )
        cycle = [
            event("evt.cycle.one", "2026-01-01T00:00:03Z", "evt.cycle.two"),
            event("evt.cycle.two", "2026-01-01T00:00:04Z", "evt.cycle.one"),
            event("evt.cycle.descendant", "2026-01-01T00:00:05Z", "evt.cycle.one"),
        ]
        (EVIDENCE_ROOT / "cycle.ndjson").write_text(
            "\n".join(json.dumps(record) for record in cycle) + "\n",
            encoding="utf-8",
        )
        duplicate = event("evt.duplicate", "2026-01-01T00:00:00Z")
        duplicate_child = event("evt.duplicate-child", "2026-01-01T00:00:01Z", "evt.duplicate")
        (EVIDENCE_ROOT / "duplicate.ndjson").write_text(
            f"{json.dumps(duplicate)}\n{json.dumps(duplicate)}\n{json.dumps(duplicate_child)}\n",
            encoding="utf-8",
        )
        large = [event(f"evt.window.{index}", f"2026-02-01T00:{index % 60:02d}:00Z")
                 for index in range(server.MAX_RECORDS_PER_ARTIFACT + 1)]
        (EVIDENCE_ROOT / "window.ndjson").write_text(
            "\n".join(json.dumps(record) for record in large) + "\n",
            encoding="utf-8",
        )

        snapshot = server.workspace_snapshot()
        replay_ids = [item["data"]["event_id"] for item in snapshot["replay_events"]]
        self.assertLess(replay_ids.index("evt.root"), replay_ids.index("evt.child"))
        self.assertLess(replay_ids.index("evt.cross-parent"), replay_ids.index("evt.cross-child"))
        self.assertLess(replay_ids.index("evt.availability.market"), replay_ids.index("evt.availability.late"))
        self.assertLess(replay_ids.index("evt.whole-second"), replay_ids.index("evt.fractional-second"))
        self.assertLess(replay_ids.index("evt.tie.first"), replay_ids.index("evt.tie.second"))
        self.assertNotIn("evt.unseen", replay_ids)
        self.assertNotIn("evt.duplicate", replay_ids)
        self.assertNotIn("evt.duplicate-child", replay_ids)
        self.assertNotIn("evt.invalid", replay_ids)
        self.assertNotIn("evt.cycle.one", replay_ids)
        self.assertNotIn("evt.cycle.two", replay_ids)
        self.assertNotIn("evt.cycle.descendant", replay_ids)
        presentation_ids = [item["data"]["event_id"] for item in snapshot["events"]]
        self.assertLess(presentation_ids.index("evt.fractional-second"), presentation_ids.index("evt.whole-second"))
        self.assertLess(presentation_ids.index("evt.tie.first"), presentation_ids.index("evt.tie.second"))
        diagnostic_codes = {item["code"] for item in snapshot["projection_diagnostics"]}
        self.assertIn("INVALID_EVENT_ENVELOPE", diagnostic_codes)
        self.assertIn("UNRESOLVED_CAUSATION", diagnostic_codes)
        self.assertIn("DUPLICATE_EVENT_ID", diagnostic_codes)
        self.assertIn("CYCLIC_CAUSATION", diagnostic_codes)
        window = next(item for item in snapshot["event_windows"] if item["artifact"] == "window.ndjson")
        self.assertTrue(window["truncated"])
        self.assertEqual(window["window_kind"], "prefix")
        self.assertEqual(window["retained_record_count"], server.MAX_RECORDS_PER_ARTIFACT)
        self.assertEqual(window["source_record_count_lower_bound"], server.MAX_RECORDS_PER_ARTIFACT + 1)
        self.assertTrue(snapshot["event_window"]["truncated"])
        self.assertIn("source_event_count_lower_bound", snapshot["event_window"])
        strict_invalid_envelopes = [
            ({**event("evt.schema", "2026-01-01T00:00:00Z"), "schema_version": "1"}, "invalid schema_version"),
            ({key: value for key, value in event("evt.receive", "2026-01-01T00:00:00Z").items() if key != "receive_time"}, "missing required receive_time"),
            ({**event("evt.account", "2026-01-01T00:00:00Z"), "account_id": "acct/invalid"}, "invalid account_id"),
            ({**event("evt.extra", "2026-01-01T00:00:00Z"), "unversioned_extra": True}, "unexpected envelope field unversioned_extra"),
        ]
        for invalid, expected_error in strict_invalid_envelopes:
            self.assertEqual(server.event_envelope_error(invalid), expected_error)

    def test_workspace_projection_routes_each_advanced_discriminator_to_typed_evidence(self) -> None:
        expected_categories = set()
        for index, (schema_field, category, _) in enumerate(server.ADVANCED_EVIDENCE_SCHEMAS):
            expected_categories.add(category)
            (EVIDENCE_ROOT / f"advanced-{index}.json").write_text(
                json.dumps({schema_field: 1}),
                encoding="utf-8",
            )

        snapshot = server.workspace_snapshot()
        projected_categories = {item["category"] for item in snapshot["advanced_evidence"]}
        self.assertEqual(projected_categories, expected_categories)
        self.assertIn(
            "model_evaluation_benchmark",
            projected_categories,
            "model benchmarks must not be mistaken for generic execution evidence",
        )
        self.assertFalse(snapshot["execution_evidence"])

    def test_workspace_projection_caps_event_candidates_without_claiming_completeness(self) -> None:
        def event(event_id: str) -> dict[str, object]:
            return {
                "event_id": event_id,
                "event_type": "market.bar.v1",
                "schema_version": 1,
                "event_time": "2026-01-01T00:00:00Z",
                "receive_time": "2026-01-01T00:00:00Z",
                "account_id": None,
                "strategy_id": None,
                "instrument_id": "inst.us_equity.spy",
                "correlation_id": "corr.cap",
                "causation_id": None,
                "actor": "replay",
                "source": "fixture",
                "payload": {},
                "software_version": "follon-test",
                "configuration_version": "replay-fixture-v1",
            }

        original_cap = server.MAX_REPLAY_EVENT_CANDIDATES
        try:
            server.MAX_REPLAY_EVENT_CANDIDATES = 2
            (EVIDENCE_ROOT / "candidate-cap.ndjson").write_text(
                "\n".join(json.dumps(event(f"evt.cap.{index}")) for index in range(3)) + "\n",
                encoding="utf-8",
            )
            snapshot = server.workspace_snapshot()
        finally:
            server.MAX_REPLAY_EVENT_CANDIDATES = original_cap

        self.assertEqual(snapshot["counts"]["events"], 2)
        self.assertTrue(snapshot["event_window"]["truncated"])
        self.assertEqual(snapshot["event_window"]["source_event_count_lower_bound"], 3)
        self.assertIn(
            "EVENT_CANDIDATE_CAP",
            {item["code"] for item in snapshot["projection_diagnostics"]},
        )

    def test_canonical_replay_order_handles_a_long_causal_chain(self) -> None:
        chain_length = 10_000
        records = [
            {
                "artifact": "long-chain.ndjson",
                "data": {
                    "event_id": f"evt.chain.{index}",
                    "event_time": "2026-01-01T00:00:00Z",
                    "receive_time": "2026-01-01T00:00:00Z",
                    "causation_id": None if index == 0 else f"evt.chain.{index - 1}",
                },
                "_retained_index": index,
            }
            for index in range(chain_length)
        ]

        ordered = server.canonical_replay_events(list(reversed(records)))

        self.assertEqual(len(ordered), chain_length)
        self.assertEqual(ordered[0]["data"]["event_id"], "evt.chain.0")
        self.assertEqual(ordered[-1]["data"]["event_id"], f"evt.chain.{chain_length - 1}")

    def test_documented_primary_screens_are_integrated_in_static_shell(self) -> None:
        index_source = INDEX_PATH.read_text(encoding="utf-8")
        source = APP_SHELL_SOURCE_PATH.read_text(encoding="utf-8")
        screens = {
            "Command Center",
            "Research Lab",
            "News",
            "Strategies",
            "Marketplace",
            "Backtest",
            "Execution Blotter",
            "Risk Cockpit",
            "Portfolio",
            "Replay & Incidents",
            "Journal",
            "Administration",
        }
        for screen in screens:
            self.assertIn(screen, source)
        self.assertEqual(source.count('{ id: "'), len(screens))
        self.assertEqual(source.count('{ id: "command-center"'), 1)
        self.assertIn('id="workspace-detail"', source)
        self.assertIn('id="workspace-summary"', source)
        self.assertIn('id="workspace-canvas"', source)
        self.assertIn('id="refresh-workspace"', source)
        self.assertIn('id="workspace-evidence"', source)
        self.assertIn('id="coverage-summary"', source)
        self.assertIn('id="artifact-search"', source)
        self.assertIn('href="/favicon.svg"', index_source)
        self.assertIn('src="/src/react-main.tsx"', index_source)
        self.assertTrue((INDEX_PATH.parent / "favicon.svg").is_file())

        runtime_source = MAIN_SOURCE_PATH.read_text(encoding="utf-8")
        self.assertIn('const pathPrefix = "/workspace/"', runtime_source)
        self.assertIn('`#workspace/${encodeURIComponent(workspace.id)}`', runtime_source)
        self.assertIn('fetch(apiUrl("/api/v1/workspaces")', runtime_source)
        self.assertIn('from "./evidence.js"', runtime_source)
        self.assertIn('from "./catalog.js"', runtime_source)
        self.assertIn('from "./workspaces.js"', runtime_source)

        workspace_source = WORKSPACE_SOURCE_PATH.read_text(encoding="utf-8")
        for renderer in (
            "renderCommandCenter", "renderResearchLab", "renderStrategyStudio",
            "renderBacktestExplorer", "renderExecutionBlotter", "renderRiskCockpit",
            "renderPortfolio", "renderReplayAndIncidents", "renderJournal",
            "renderAdministration", "renderMarketplace", "renderNewsCockpit",
        ):
            self.assertIn(f"function {renderer}", workspace_source)

    def test_workspace_snapshot_filters_events_by_as_of(self) -> None:
        def envelope(event_id: str, time_str: str) -> dict[str, object]:
            return {
                "event_id": event_id,
                "event_type": "market.bar.v1",
                "schema_version": 1,
                "event_time": time_str,
                "receive_time": time_str,
                "account_id": None,
                "strategy_id": None,
                "instrument_id": "aapl.us",
                "correlation_id": "corr.1",
                "causation_id": None,
                "actor": "market_data",
                "source": "feed",
                "payload": {"open": "100.0"},
                "software_version": "0.1.0",
                "configuration_version": "cfg.1",
            }
        (EVIDENCE_ROOT / "events.ndjson").write_text(
            f"{json.dumps(envelope('evt.past', '2026-09-01T10:00:00Z'))}\n"
            f"{json.dumps(envelope('evt.future', '2026-09-01T12:00:00Z'))}\n",
            encoding="utf-8",
        )
        full_snap = server.workspace_snapshot()
        self.assertEqual(len(full_snap["events"]), 2)

        as_of_snap = server.workspace_snapshot(as_of="2026-09-01T10:30:00Z")
        self.assertEqual(as_of_snap["as_of"], "2026-09-01T10:30:00Z")
        self.assertEqual(len(as_of_snap["events"]), 1)
        self.assertEqual(as_of_snap["events"][0]["data"]["event_id"], "evt.past")

    def test_decision_reconstruction_endpoint(self) -> None:
        def envelope(event_id: str, time_str: str, causation: str | None = None) -> dict[str, object]:
            return {
                "event_id": event_id,
                "event_type": "execution.fill.v1" if causation else "intent.created.v1",
                "schema_version": 1,
                "event_time": time_str,
                "receive_time": time_str,
                "account_id": "acct.1",
                "strategy_id": "strat.1",
                "instrument_id": "aapl.us",
                "correlation_id": "corr.1",
                "causation_id": causation,
                "actor": "oms",
                "source": "paper",
                "payload": {"price": "100.0"},
                "software_version": "0.1.0",
                "configuration_version": "cfg.1",
            }
        (EVIDENCE_ROOT / "chain.ndjson").write_text(
            f"{json.dumps(envelope('evt.intent', '2026-09-01T10:00:00Z'))}\n"
            f"{json.dumps(envelope('evt.fill', '2026-09-01T10:00:01Z', 'evt.intent'))}\n",
            encoding="utf-8",
        )
        recon = server.build_decision_reconstruction("evt.fill")
        self.assertIsNotNone(recon)
        self.assertEqual(recon["reconstruction_schema_version"], 1)
        self.assertEqual(recon["target_event_id"], "evt.fill")
        self.assertEqual(recon["integrity_status"], "VERIFIED")
        self.assertEqual(len(recon["causal_chain"]), 2)
        self.assertEqual(recon["causal_chain"][0]["node_id"], "evt.intent")
        self.assertEqual(recon["causal_chain"][1]["node_id"], "evt.fill")
        self.assertEqual(len(recon["edges"]), 1)


if __name__ == "__main__":
    unittest.main()
