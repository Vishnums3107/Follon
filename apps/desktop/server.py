"""Loopback-only, read-only evidence dashboard server for Follon local development."""

from __future__ import annotations

import json
import base64
import binascii
import csv
import hmac
import mimetypes
import os
import re
import socket
from datetime import UTC, datetime
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from pathlib import PurePosixPath
from urllib.parse import parse_qs, unquote, urlsplit
from urllib.request import urlopen

STATIC_ROOT = Path(os.environ.get("FOLLON_DASHBOARD_STATIC_ROOT", "/srv/follon")).resolve()
EVIDENCE_ROOT = Path(os.environ.get("FOLLON_EVIDENCE_ROOT", "/var/follon")).resolve()
HOST = os.environ.get("FOLLON_DASHBOARD_HOST", "0.0.0.0")
PORT = int(os.environ.get("FOLLON_DASHBOARD_PORT", "8080"))
MODE = os.environ.get("FOLLON_DASHBOARD_MODE", "development")
AUTH_USERNAME = os.environ.get("FOLLON_DASHBOARD_USERNAME", "")
AUTH_PASSWORD_VALUE = os.environ.get("FOLLON_DASHBOARD_PASSWORD", "")
AUTH_PASSWORD_FILE = os.environ.get("FOLLON_DASHBOARD_PASSWORD_FILE", "")
POSTGRES_HOST = os.environ.get("FOLLON_POSTGRES_HOST", "postgres")
POSTGRES_PORT = int(os.environ.get("FOLLON_POSTGRES_INTERNAL_PORT", "5432"))
MINIO_HEALTH_URL = os.environ.get("FOLLON_MINIO_HEALTH_URL", "http://minio:9000/minio/health/live")
MAX_EVIDENCE_BYTES = 10 * 1024 * 1024
EVIDENCE_COMPONENT = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
STATIC_FILE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]*$")
CANONICAL_STORAGE_ID = re.compile(r"^[a-z0-9][a-z0-9._-]{0,127}$")
SHA256_HEX = re.compile(r"^[a-f0-9]{64}$")
EVIDENCE_SUFFIXES = {".ndjson", ".json", ".md", ".csv"}
IGNORED_EVIDENCE_DIRECTORIES = {
    ".git",
    ".mypy_cache",
    ".pytest_cache",
    "__pycache__",
    "node_modules",
    "site-packages",
    "target",
    "venv",
}
MAX_INDEXED_EVIDENCE = 2_000
MAX_WORKSPACE_RECORDS = 1_000
MAX_RECORDS_PER_ARTIFACT = 250


def load_auth_password() -> str:
    if AUTH_PASSWORD_VALUE and AUTH_PASSWORD_FILE:
        raise RuntimeError("set only one dashboard password source")
    if not AUTH_PASSWORD_FILE:
        return AUTH_PASSWORD_VALUE
    secret_path = Path(AUTH_PASSWORD_FILE)
    if not secret_path.is_file() or secret_path.stat().st_size > 4096:
        raise RuntimeError("dashboard password file is missing, invalid, or too large")
    password = secret_path.read_text(encoding="utf-8").rstrip("\r\n")
    if "\x00" in password:
        raise RuntimeError("dashboard password file contains invalid data")
    return password


AUTH_PASSWORD = load_auth_password()

if (AUTH_USERNAME == "") != (AUTH_PASSWORD == ""):
    raise RuntimeError("dashboard authentication requires both username and password")
if MODE == "production" and (not AUTH_USERNAME or len(AUTH_PASSWORD) < 16):
    raise RuntimeError("production dashboard mode requires a username and a password of at least 16 characters")

FEATURES: tuple[dict[str, object], ...] = (
    {
        "id": "market-data",
        "title": "Market data",
        "state": "implemented",
        "summary": "Strict trade ingestion, normalized bars, instruments, and exchange sessions.",
        "capabilities": ["Normalized trade importer", "Deterministic OHLCV bar builder", "Canonical ordering", "Duplicate and sequence rejection", "Instrument registry", "Trading calendar with explicit venue and instrument halts", "Corporate-action inputs", "Immutable Parquet datasets"],
        "boundary": "Historical inputs only; no licensed market-data redistribution or live feed is configured.",
        "gate": "Foundation verification passed locally.",
        "screens": ["Command Center", "Research Lab"],
        "source": "core/market-data + core/instrument + follon-build-bars",
        "documentation": "docs/03-capabilities/01-market-data-and-replay.md",
    },
    {
        "id": "replay",
        "title": "Replay",
        "state": "implemented",
        "summary": "Deterministic strategy-to-fill simulation with immutable canonical events.",
        "capabilities": ["Historical CSV and event-log replay", "Controllable replay clock", "Strategy intent generation", "Deterministic risk decisions", "OMS state transitions", "Persistent latency-aware working orders", "Deterministic partial fills", "Portfolio accounting", "Canonical causal audit trail"],
        "boundary": "Simulation only; the replay image contains no broker or credential interface.",
        "gate": "Foundation verification passed locally.",
        "screens": ["Command Center", "Replay and Incidents", "Portfolio"],
        "source": "core/control-plane + follon-replay",
        "documentation": "docs/06-delivery/02-first-vertical-slice.md",
    },
    {
        "id": "research",
        "title": "Research & backtests",
        "state": "implemented",
        "summary": "Reproducible backtests, Python strategy isolation, manifests, and experiment evidence.",
        "capabilities": ["Event-driven backtester", "Explicit spread, slippage, bar latency, and per-bar fill caps", "Python worker identity handshake", "Strategy bundle hashing", "Configuration and dataset fingerprinting", "Corporate actions", "Exact accounting", "Performance and equity metrics", "Immutable JSON and Markdown reports", "Completion manifests", "Experiment catalog", "DuckDB dataset catalog", "Versioned S3-compatible artifact storage"],
        "boundary": "Single-currency historical research; strategy workers cannot access adapters or credentials.",
        "gate": "Research engineering gate passed locally.",
        "screens": ["Research Lab", "Strategy Studio", "Backtest Explorer"],
        "source": "core/backtest + python/strategy-sdk + python/storage-adapter + follon-backtest",
        "documentation": "docs/03-capabilities/02-strategy-sdk-and-backtesting.md",
    },
    {
        "id": "paper",
        "title": "PAPER operations",
        "state": "gated",
        "summary": "Durable PAPER OMS, risk controls, reconciliation, recovery, and kill-switch evidence.",
        "capabilities": ["Durable PAPER OMS", "Out-of-order broker evidence handling", "Versioned pre-trade limits", "Cash reservation", "IBKR PAPER bridge boundary", "Order/fill/position/cash reconciliation", "Independent kill switches", "Restart and reconnect recovery", "Fault injection", "30-session evidence tracker"],
        "boundary": "Privileged controls stay in the operator CLI and are never exposed by this read-only dashboard.",
        "gate": "Observed operating evidence: 0 of 30 required clean PAPER sessions.",
        "screens": ["Execution Blotter", "Risk Cockpit", "Portfolio", "Journal"],
        "source": "core/paper + IBKR PAPER adapter + follon-paper-status",
        "documentation": "docs/03-capabilities/03-oms-and-execution.md",
    },
    {
        "id": "controlled-live",
        "title": "Controlled live",
        "state": "gated",
        "summary": "Shadow/canary safety, four-eyes approvals, audit, reconciliation, and recovery monitoring.",
        "capabilities": ["Explicit LIVE account boundary", "Managed-secret provider boundary", "Four-eyes activation", "Single-use intent approvals", "Shadow isolation", "Canary limits", "Independent kill switches", "Hash-chained irreversible-action audit", "Reconciliation and disaster recovery", "60-session evidence tracker"],
        "boundary": "No live broker endpoint, credential provider, or capital-bearing approval is configured.",
        "gate": "Observed operating evidence: 0 of 60 required controlled-live sessions.",
        "screens": ["Command Center", "Execution Blotter", "Risk Cockpit", "Journal"],
        "source": "core/live + managed-secret boundary + follon-live-status",
        "documentation": "docs/operations/02-controlled-live-runbook.md",
    },
    {
        "id": "operations",
        "title": "Operations workbench",
        "state": "gated",
        "summary": "Risk cockpit, attribution, alerts, schedules, journals, configuration, and reports.",
        "capabilities": ["Risk cockpit", "P&L attribution", "Stable alert projection", "Daily UTC scheduling", "Typed schedule completion", "Hash-chained operations journal", "Parameter/configuration diff", "Two-person risk-limit evidence", "Immutable dashboard and reports"],
        "boundary": "Deterministic projection only; no wall clock, background jobs, or order controls.",
        "gate": "Product adoption evidence: 0 of 5 unaided design partners.",
        "screens": ["Command Center", "Risk Cockpit", "Portfolio", "Replay and Incidents", "Journal"],
        "source": "core/operations + follon-operations",
        "documentation": "docs/06-delivery/09-months-12-14-status.md",
    },
    {
        "id": "options",
        "title": "Options evidence",
        "state": "gated",
        "summary": "European-options analytics, Greeks, expiry scenarios, and cross-environment reconciliation.",
        "capabilities": ["Frozen option chains", "Fixed-point European pricing", "Implied-volatility bisection", "Delta/Gamma/Vega/Theta/Rho", "Multi-leg expiry scenarios", "BACKTEST/PAPER/LIVE book reconciliation", "Source-export and run-identity verification", "Immutable analytics reports"],
        "boundary": "Evidence only; no broker order, exercise, assignment, or American-option path.",
        "gate": "Independent broker-backed reconciliation evidence: 0 sessions.",
        "screens": ["Research Lab", "Backtest Explorer", "Portfolio"],
        "source": "core/options + follon-options",
        "documentation": "docs/06-delivery/10-months-15-17-status.md",
    },
    {
        "id": "commercial",
        "title": "Commercial & deployment",
        "state": "gated",
        "summary": "Provisioning, entitlement, privacy, retention, signed releases, and self-host readiness evidence.",
        "capabilities": ["Tenant provisioning ledger", "Subscription observations", "Deterministic entitlements", "Privacy data inventory", "Hash-bound retention plans", "Confirmed single-file retention execution", "Release manifests", "Detached Ed25519 verification", "Trusted release keys", "Self-host readiness receipts"],
        "boundary": "No payment gateway, customer authentication, remote signer, or raw customer identity handling.",
        "gate": "Commercial evidence: 0 of 10 professionals and 0 of 3 organizations.",
        "screens": ["Administration", "Journal"],
        "source": "core/commercial + follon-admin + self-host compose",
        "documentation": "docs/06-delivery/11-months-18-20-status.md",
    },
)


def is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
    except ValueError:
        return False
    return True


def evidence_file(name: str) -> Path | None:
    normalized = name.replace("\\", "/")
    relative = PurePosixPath(normalized)
    if (
        relative.is_absolute()
        or not relative.parts
        or any(part in {"", ".", ".."} or not EVIDENCE_COMPONENT.fullmatch(part) for part in relative.parts)
        or Path(relative.name).suffix.lower() not in EVIDENCE_SUFFIXES
    ):
        return None
    try:
        candidate = EVIDENCE_ROOT.joinpath(*relative.parts).resolve(strict=True)
    except FileNotFoundError:
        return None
    if not candidate.is_file() or not is_within(candidate, EVIDENCE_ROOT):
        return None
    return candidate


def classify_artifact(path: Path) -> tuple[str, str]:
    """Return the owning feature and display kind without trusting file content as markup."""
    try:
        name = path.relative_to(EVIDENCE_ROOT).as_posix().lower()
    except ValueError:
        name = path.name.lower()
    try:
        with path.open("r", encoding="utf-8", errors="replace") as handle:
            sample = handle.read(32_768).lower()
    except OSError:
        sample = ""
    if "option_dashboard_schema_version" in sample or "option" in name:
        return "options", "Options analytics"
    if '"environment":"live"' in sample or "live" in name:
        return "controlled-live", "Controlled-live monitoring"
    if "paper" in name or ("promotion_eligible" in sample and '"environment":"paper"' in sample):
        return "paper", "PAPER operations"
    if "projection_fingerprint" in sample or "operations" in name:
        return "operations", "Operations workbench"
    if any(token in name for token in ("commercial", "provision", "subscription", "entitlement", "privacy", "retention", "release", "self-host", "trusted-key")):
        return "commercial", "Commercial or release evidence"
    if "backtest" in name or "experiment" in name or "completion_manifest" in sample:
        return "research", "Backtest or experiment"
    if path.suffix.lower() == ".csv" or any(token in name for token in ("bars", "trades", "market-data")):
        return "market-data", "Market-data artifact"
    if path.suffix.lower() == ".ndjson" or '"event_type"' in sample:
        return "replay", "Canonical event trail"
    return "research", "Structured evidence"


def artifact_format(path: Path) -> str:
    return {
        ".ndjson": "ndjson",
        ".json": "json",
        ".md": "markdown",
        ".csv": "csv",
    }.get(path.suffix.lower(), "text")


def is_storage_receipt(payload: dict[str, object]) -> bool:
    """Recognize the strict, portable v1 dataset receipt projection."""
    required_strings = (
        "dataset_id",
        "dataset_version",
        "parquet_file",
        "parquet_sha256",
        "source_sha256",
        "starts_at",
        "ends_at",
    )
    if payload.get("storage_receipt_schema_version") != 1 or any(
        not isinstance(payload.get(field), str) for field in required_strings
    ):
        return False
    row_count = payload.get("row_count")
    if not isinstance(row_count, int) or isinstance(row_count, bool) or row_count <= 0:
        return False
    return bool(
        CANONICAL_STORAGE_ID.fullmatch(str(payload["dataset_id"]))
        and CANONICAL_STORAGE_ID.fullmatch(str(payload["dataset_version"]))
        and EVIDENCE_COMPONENT.fullmatch(str(payload["parquet_file"]))
        and str(payload["parquet_file"]).endswith(".parquet")
        and SHA256_HEX.fullmatch(str(payload["parquet_sha256"]))
        and SHA256_HEX.fullmatch(str(payload["source_sha256"]))
    )


def scan_evidence() -> tuple[list[dict[str, object]], dict[str, Path]]:
    """Index evidence once and retain already-validated paths for projections."""
    if not EVIDENCE_ROOT.is_dir():
        return [], {}
    files: list[dict[str, object]] = []
    paths: dict[str, Path] = {}
    for directory, child_directories, file_names in os.walk(EVIDENCE_ROOT, followlinks=False):
        child_directories[:] = sorted(
            child
            for child in child_directories
            if child not in IGNORED_EVIDENCE_DIRECTORIES
            and not child.startswith(".")
            and not (Path(directory) / child).is_symlink()
        )
        for file_name in sorted(file_names):
            relative_name = (Path(directory) / file_name).relative_to(EVIDENCE_ROOT).as_posix()
            resolved = evidence_file(relative_name)
            if resolved is None:
                continue
            stat = resolved.stat()
            if stat.st_size > MAX_EVIDENCE_BYTES:
                continue
            feature, kind = classify_artifact(resolved)
            files.append(
                {
                    "name": relative_name,
                    "bytes": stat.st_size,
                    "modified_at": datetime.fromtimestamp(stat.st_mtime, UTC).isoformat(),
                    "feature": feature,
                    "kind": kind,
                    "format": artifact_format(resolved),
                }
            )
            paths[relative_name] = resolved
            if len(files) >= MAX_INDEXED_EVIDENCE:
                return sorted(files, key=lambda item: str(item["modified_at"]), reverse=True), paths
    return sorted(files, key=lambda item: str(item["modified_at"]), reverse=True), paths


def list_evidence() -> list[dict[str, object]]:
    return scan_evidence()[0]


def read_json_artifact(path: Path) -> dict[str, object] | None:
    """Read one bounded JSON object; invalid and non-object evidence is ignored."""
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) else None


def read_ndjson_artifact(path: Path) -> list[dict[str, object]]:
    """Read a bounded prefix of object records without weakening artifact isolation."""
    records: list[dict[str, object]] = []
    try:
        with path.open("r", encoding="utf-8") as handle:
            for line in handle:
                if len(records) >= MAX_RECORDS_PER_ARTIFACT:
                    break
                if not line.strip():
                    continue
                try:
                    value = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(value, dict):
                    records.append(value)
    except (OSError, UnicodeDecodeError):
        return []
    return records


def summarize_csv(path: Path, metadata: dict[str, object]) -> dict[str, object] | None:
    """Return structural dataset metadata without returning the complete source file."""
    try:
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            reader = csv.reader(handle)
            header = next(reader, None)
            if header is None:
                return None
            row_count = sum(1 for _ in reader)
    except (OSError, UnicodeDecodeError, csv.Error):
        return None
    return {
        "name": metadata["name"],
        "modified_at": metadata["modified_at"],
        "bytes": metadata["bytes"],
        "columns": header[:32],
        "rows": row_count,
    }


def dashboard_candidate(
    candidates: list[tuple[str, str, dict[str, object]]],
    metadata: dict[str, object],
    payload: dict[str, object],
) -> None:
    candidates.append((str(metadata["modified_at"]), str(metadata["name"]), payload))


def latest_dashboard(candidates: list[tuple[str, str, dict[str, object]]]) -> dict[str, object] | None:
    if not candidates:
        return None
    modified_at, name, payload = max(candidates, key=lambda item: item[0])
    return {"artifact": name, "modified_at": modified_at, "data": payload}


def workspace_snapshot() -> dict[str, object]:
    """Build the bounded, read-only projection consumed by all operator workspaces."""
    artifacts, evidence_paths = scan_evidence()
    datasets: list[dict[str, object]] = []
    backtests: list[dict[str, object]] = []
    experiments: list[dict[str, object]] = []
    manifests: list[dict[str, object]] = []
    events: list[dict[str, object]] = []
    journals: list[dict[str, object]] = []
    commercial: list[dict[str, object]] = []
    paper_candidates: list[tuple[str, str, dict[str, object]]] = []
    live_candidates: list[tuple[str, str, dict[str, object]]] = []
    operations_candidates: list[tuple[str, str, dict[str, object]]] = []
    options_candidates: list[tuple[str, str, dict[str, object]]] = []

    for metadata in artifacts:
        name = str(metadata["name"])
        path = evidence_paths.get(name)
        if path is None:
            continue
        if metadata["format"] == "csv":
            summary = summarize_csv(path, metadata)
            if summary is not None:
                datasets.append(summary)
            continue
        if metadata["format"] == "json":
            payload = read_json_artifact(path)
            if payload is None:
                continue
            environment = payload.get("environment")
            if payload.get("dashboard_schema_version") == 2 and environment == "PAPER":
                dashboard_candidate(paper_candidates, metadata, payload)
            elif payload.get("dashboard_schema_version") == 2 and environment == "LIVE":
                dashboard_candidate(live_candidates, metadata, payload)
            elif payload.get("dashboard_schema_version") == 1 and "projection_fingerprint" in payload:
                dashboard_candidate(operations_candidates, metadata, payload)
            elif payload.get("option_dashboard_schema_version") == 1:
                dashboard_candidate(options_candidates, metadata, payload)
            elif is_storage_receipt(payload):
                datasets.append(
                    {
                        "name": name,
                        "modified_at": metadata["modified_at"],
                        "bytes": metadata["bytes"],
                        "columns": ["event_time", "instrument_id", "open", "high", "low", "close", "volume", "interval_seconds", "exchange_timezone"],
                        "rows": payload["row_count"],
                        "dataset_id": payload.get("dataset_id"),
                        "dataset_version": payload.get("dataset_version"),
                        "storage_format": "Parquet",
                        "content_sha256": payload.get("parquet_sha256"),
                    }
                )
            elif payload.get("artifact_schema_version") in {1, 2} and isinstance(payload.get("report"), dict):
                backtests.append(
                    {
                        "artifact": name,
                        "modified_at": metadata["modified_at"],
                        "artifact_fingerprint": payload.get("artifact_fingerprint"),
                        "event_output_hash": payload.get("event_output_hash"),
                        "performance": payload.get("performance", {}),
                        "report": payload.get("report", {}),
                        "specification": payload.get("specification", {}),
                        "specification_fingerprint": payload.get("specification_fingerprint"),
                    }
                )
            elif "manifest_schema_version" in payload:
                manifests.append({"artifact": name, "modified_at": metadata["modified_at"], "data": payload})
            continue
        if metadata["format"] != "ndjson":
            continue
        for record in read_ndjson_artifact(path):
            enriched = {"artifact": name, "feature": metadata["feature"], "data": record}
            if "experiment_id" in record and "run_id" in record:
                experiments.append(enriched)
            elif "ledger_schema_version" in record and str(record.get("event_type", "")).startswith("commercial."):
                commercial.append(enriched)
                journals.append({**enriched, "category": "commercial"})
            elif "event_id" in record and "event_type" in record and "event_time" in record:
                events.append(enriched)
            elif "sequence" in record and ("entry_hash" in record or "record_hash" in record):
                category = "live" if "live" in name.lower() else "paper" if "paper" in name.lower() else "operations"
                journals.append({**enriched, "category": category})

    events.sort(key=lambda item: str(item["data"].get("event_time", "")), reverse=True)
    journals.sort(
        key=lambda item: (
            str(item["data"].get("occurred_at", "")),
            int(item["data"].get("sequence", 0)) if isinstance(item["data"].get("sequence", 0), int) else 0,
        ),
        reverse=True,
    )
    backtests.sort(key=lambda item: str(item["modified_at"]), reverse=True)
    experiments.sort(key=lambda item: str(item["artifact"]), reverse=True)
    manifests.sort(key=lambda item: str(item["modified_at"]), reverse=True)

    feature_counts = {str(feature["id"]): 0 for feature in FEATURES}
    for artifact in artifacts:
        feature = str(artifact["feature"])
        feature_counts[feature] = feature_counts.get(feature, 0) + 1

    return {
        "workspace_schema_version": 1,
        "generated_at": datetime.now(UTC).isoformat(),
        "read_only": True,
        "counts": {
            "artifacts": len(artifacts),
            "datasets": len(datasets),
            "backtests": len(backtests),
            "experiments": len(experiments),
            "events": len(events),
            "journals": len(journals),
            "commercial_records": len(commercial),
        },
        "feature_artifact_counts": feature_counts,
        "datasets": datasets[:MAX_WORKSPACE_RECORDS],
        "backtests": backtests[:MAX_WORKSPACE_RECORDS],
        "experiments": experiments[:MAX_WORKSPACE_RECORDS],
        "manifests": manifests[:MAX_WORKSPACE_RECORDS],
        "events": events[:MAX_WORKSPACE_RECORDS],
        "journals": journals[:MAX_WORKSPACE_RECORDS],
        "commercial": commercial[:MAX_WORKSPACE_RECORDS],
        "paper": latest_dashboard(paper_candidates),
        "live": latest_dashboard(live_candidates),
        "operations": latest_dashboard(operations_candidates),
        "options": latest_dashboard(options_candidates),
        "commercial_artifacts": [artifact for artifact in artifacts if artifact["feature"] == "commercial"],
    }


def probe_tcp(host: str, port: int) -> dict[str, object]:
    try:
        with socket.create_connection((host, port), timeout=1.0):
            return {"status": "healthy", "detail": "Accepting connections"}
    except OSError:
        return {"status": "unavailable", "detail": "Connection unavailable"}


def probe_http(url: str) -> dict[str, object]:
    try:
        with urlopen(url, timeout=1.0) as response:  # noqa: S310 - fixed operator URL
            if 200 <= response.status < 300:
                return {"status": "healthy", "detail": f"HTTP {response.status}"}
            return {"status": "degraded", "detail": f"HTTP {response.status}"}
    except OSError:
        return {"status": "unavailable", "detail": "Health endpoint unavailable"}


def system_status() -> dict[str, object]:
    artifacts = list_evidence()
    return {
        "dashboard_schema_version": 1,
        "generated_at": datetime.now(UTC).isoformat(),
        "mode": MODE,
        "read_only": True,
        "authentication": "enabled" if AUTH_USERNAME else "loopback-development",
        "services": {
            "dashboard": {"status": "healthy", "detail": "Read-only API available"},
            "postgres": probe_tcp(POSTGRES_HOST, POSTGRES_PORT),
            "minio": probe_http(MINIO_HEALTH_URL),
        },
        "artifacts": {
            "count": len(artifacts),
            "latest_at": artifacts[0]["modified_at"] if artifacts else None,
        },
    }


class DashboardHandler(BaseHTTPRequestHandler):
    server_version = "FollonEvidenceDashboard/2.0"

    def end_headers(self) -> None:
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("X-Frame-Options", "DENY")
        self.send_header("Content-Security-Policy", "default-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:")
        super().end_headers()

    def do_GET(self) -> None:  # noqa: N802
        request = urlsplit(self.path)
        path = unquote(request.path)
        if path == "/api/v1/health":
            self.write_json(HTTPStatus.OK, {"status": "ok"})
            return
        if not self.is_authorized():
            self.request_authentication()
            return
        if path == "/api/v1/status":
            self.write_json(HTTPStatus.OK, system_status())
            return
        if path == "/api/v1/features":
            self.write_json(HTTPStatus.OK, FEATURES)
            return
        if path == "/api/v1/workspaces":
            self.write_json(HTTPStatus.OK, workspace_snapshot())
            return
        if path == "/api/v1/evidence":
            self.write_json(HTTPStatus.OK, list_evidence())
            return
        if path.startswith("/api/v1/evidence/"):
            download = parse_qs(request.query).get("download") == ["1"]
            self.write_evidence(path.removeprefix("/api/v1/evidence/"), download=download)
            return
        if path == "/favicon.ico":
            self.write_static("/favicon.svg")
            return
        if path in {"/evidence", "/workspaces"} or path.startswith("/workspace/"):
            self.write_static("/")
            return
        self.write_static(path)

    def is_authorized(self) -> bool:
        if not AUTH_USERNAME:
            return True
        header = self.headers.get("Authorization", "")
        if not header.startswith("Basic "):
            return False
        try:
            decoded = base64.b64decode(header.removeprefix("Basic "), validate=True).decode("utf-8")
        except (binascii.Error, UnicodeDecodeError):
            return False
        username, separator, password = decoded.partition(":")
        return bool(separator) and hmac.compare_digest(username, AUTH_USERNAME) and hmac.compare_digest(password, AUTH_PASSWORD)

    def request_authentication(self) -> None:
        body = b"Dashboard authentication required."
        self.send_response(HTTPStatus.UNAUTHORIZED)
        self.send_header("WWW-Authenticate", 'Basic realm="Follon dashboard", charset="UTF-8"')
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def write_json(self, status: HTTPStatus, payload: object) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def write_evidence(self, name: str, *, download: bool = False) -> None:
        evidence = evidence_file(name)
        if evidence is None:
            self.send_error(HTTPStatus.NOT_FOUND, "Evidence file not found")
            return
        if evidence.stat().st_size > MAX_EVIDENCE_BYTES:
            self.send_error(HTTPStatus.REQUEST_ENTITY_TOO_LARGE, "Evidence file exceeds the 10 MiB dashboard limit")
            return
        mime_type = {
            ".ndjson": "application/x-ndjson",
            ".json": "application/json",
            ".md": "text/markdown",
            ".csv": "text/csv",
        }.get(evidence.suffix.lower(), "text/plain")
        self.write_file(evidence, mime_type, download_name=evidence.name if download else None)

    def write_static(self, request_path: str) -> None:
        relative_path = "index.html" if request_path in {"", "/"} else request_path.lstrip("/")
        if not STATIC_FILE_NAME.fullmatch(relative_path):
            self.send_error(HTTPStatus.NOT_FOUND, "File not found")
            return
        candidate = (STATIC_ROOT / relative_path).resolve()
        if not candidate.is_file() or not is_within(candidate, STATIC_ROOT):
            self.send_error(HTTPStatus.NOT_FOUND, "File not found")
            return
        self.write_file(candidate, mimetypes.guess_type(candidate.name)[0] or "application/octet-stream")

    def write_file(self, path: Path, mime_type: str, *, download_name: str | None = None) -> None:
        body = path.read_bytes()
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", f"{mime_type}; charset=utf-8")
        if download_name is not None:
            self.send_header("Content-Disposition", f'attachment; filename="{download_name}"')
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        print(f"{self.address_string()} - {format % args}")


def main() -> None:
    with ThreadingHTTPServer((HOST, PORT), DashboardHandler) as server:
        print(f"Follon evidence dashboard listening on http://{HOST}:{PORT}")
        server.serve_forever()


if __name__ == "__main__":
    main()
