"""Loopback-only, read-only evidence dashboard server for Follon local development."""

from __future__ import annotations

import json
import base64
import binascii
import csv
import hmac
import hashlib
import math
import mimetypes
import os
import re
import socket
from collections import deque
from datetime import UTC, datetime
from heapq import heappop, heappush
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from pathlib import PurePosixPath
from threading import Lock
from time import monotonic
from urllib.parse import parse_qs, unquote, urlsplit
from urllib.request import urlopen

_DEFAULT_STATIC = Path(__file__).resolve().parent / "web-dist"
_DEFAULT_EVIDENCE = Path(__file__).resolve().parents[2] / "var"
STATIC_ROOT = Path(
    os.environ.get(
        "FOLLON_DASHBOARD_STATIC_ROOT",
        str(_DEFAULT_STATIC if _DEFAULT_STATIC.is_dir() else "/srv/follon"),
    )
).resolve()
EVIDENCE_ROOT = Path(
    os.environ.get(
        "FOLLON_EVIDENCE_ROOT",
        str(_DEFAULT_EVIDENCE if _DEFAULT_EVIDENCE.is_dir() else "/var/follon"),
    )
).resolve()
HOST = os.environ.get("FOLLON_DASHBOARD_HOST", "0.0.0.0")
PORT = int(os.environ.get("FOLLON_DASHBOARD_PORT", "8080"))
MODE = os.environ.get("FOLLON_DASHBOARD_MODE", "development")
AUTH_USERNAME = os.environ.get("FOLLON_DASHBOARD_USERNAME", "")
AUTH_PASSWORD_VALUE = os.environ.get("FOLLON_DASHBOARD_PASSWORD", "")
AUTH_PASSWORD_FILE = os.environ.get("FOLLON_DASHBOARD_PASSWORD_FILE", "")
POSTGRES_HOST = os.environ.get("FOLLON_POSTGRES_HOST", "postgres")
POSTGRES_PORT = int(os.environ.get("FOLLON_POSTGRES_INTERNAL_PORT", "5432"))
MINIO_HEALTH_URL = os.environ.get("FOLLON_MINIO_HEALTH_URL", "http://minio:9000/minio/health/live")
TRADING_API_HOST = os.environ.get("FOLLON_TRADING_API_HOST", "trading-api")
TRADING_API_PORT = int(os.environ.get("FOLLON_TRADING_API_PORT", "50051"))
MAX_EVIDENCE_BYTES = 10 * 1024 * 1024
EVIDENCE_COMPONENT = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
STATIC_FILE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]*$")
CANONICAL_STORAGE_ID = re.compile(r"^[a-z0-9][a-z0-9._-]{0,127}$")
SHA256_HEX = re.compile(r"^[a-f0-9]{64}$")
EVIDENCE_SUFFIXES = {".ndjson", ".json", ".md", ".csv", ".ipynb"}
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
MAX_REPLAY_EVENT_CANDIDATES = 10_000
CANONICAL_EVENT_TYPE = re.compile(r"^([a-z]+\.)+[a-z_]+\.v[1-9][0-9]*$")
CANONICAL_EVENT_ID = re.compile(r"^[a-z0-9._-]+$")
CANONICAL_UTC_TIMESTAMP = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?Z$"
)
CANONICAL_EVENT_FIELDS = frozenset({
    "event_id",
    "event_type",
    "schema_version",
    "event_time",
    "receive_time",
    "account_id",
    "strategy_id",
    "instrument_id",
    "correlation_id",
    "causation_id",
    "actor",
    "source",
    "payload",
    "software_version",
    "configuration_version",
})
AUTH_FAILURE_LIMIT = 5
AUTH_FAILURE_WINDOW_SECONDS = 60.0
MAX_TRACKED_AUTH_CLIENTS = 10_000
_AUTH_FAILURES: dict[str, deque[float]] = {}
_AUTH_FAILURES_LOCK = Lock()

# Advanced evidence is intentionally projected only when the artifact declares
# one of the reviewed v1 schemas.  The browser validates the complete contract
# again before rendering it; this lightweight registry keeps the read-only
# projection bounded without treating arbitrary JSON as operational evidence.
ADVANCED_EVIDENCE_SCHEMAS: tuple[tuple[str, str, str], ...] = (
    ("hypothesis_schema_version", "research_hypothesis", "research"),
    ("lineage_schema_version", "experiment_lineage", "research"),
    ("job_schema_version", "research_job", "research"),
    ("assistant_evidence_schema_version", "assistant_evidence", "research"),
    ("evaluation_schema_version", "robustness_evaluation", "research"),
    ("portfolio_experiment_schema_version", "portfolio_experiment", "research"),
    ("knowledge_schema_version", "knowledge_snapshot", "news"),
    ("calendar_schema_version", "event_exposure_calendar", "news"),
    ("mandate_schema_version", "automation_mandate", "research"),
    ("passport_schema_version", "order_decision_passport", "execution-risk"),
    ("exposure_schema_version", "exposure_graph", "execution-risk"),
    ("ledger_schema_version", "fund_ledger_statement", "accounting"),
    ("policy_schema_version", "continuity_policy", "operations"),
    ("regime_schema_version", "assumption_regime_monitor", "research"),
    ("parity_schema_version", "feed_substitution_parity", "market-data"),
    ("coach_schema_version", "execution_coach_benchmark", "execution-risk"),
    ("simulation_schema_version", "scenario_loss_simulation", "execution-risk"),
    ("allocation_schema_version", "capital_allocation_plan", "execution-risk"),
    ("preview_schema_version", "sandbox_installation_preview", "research"),
    ("qualification_schema_version", "adapter_qualification", "platform"),
    ("champion_challenger_schema_version", "champion_challenger_evaluation", "research"),
    ("planner_schema_version", "capability_execution_planner", "execution-risk"),
    ("diagnosis_schema_version", "operations_diagnosis_runbook", "operations"),
    ("benchmark_schema_version", "model_evaluation_benchmark", "operations"),
    ("capsule_schema_version", "strategy_capsule_manifest", "research"),
    ("expansion_schema_version", "multi_asset_expansion_plan", "execution-risk"),
    ("reconstruction_schema_version", "decision_reconstruction", "execution-risk"),
    ("scenario_schema_version", "counterfactual_scenario", "research"),
    ("receipt_schema_version", "data_rights_and_semantics_receipt", "market-data"),
    ("snapshot_schema_version", "workspace_snapshot_manifest", "operations"),
    ("budget_schema_version", "attention_budget", "operations"),
    ("adversarial_schema_version", "adversarial_evaluation", "research"),
    ("drill_schema_version", "recovery_drill_result", "operations"),
    ("matrix_schema_version", "gateway_qualification_matrix", "platform"),
    ("proposal_schema_version", "capital_allocation_proposal", "execution-risk"),
    ("compatibility_schema_version", "compatibility_matrix", "platform"),
)


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


def _prune_auth_failures(failures: deque[float], now: float) -> None:
    cutoff = now - AUTH_FAILURE_WINDOW_SECONDS
    while failures and failures[0] <= cutoff:
        failures.popleft()


def authentication_retry_after(client: str, *, now: float | None = None) -> int:
    """Return a bounded Retry-After value for one direct peer, or zero when allowed."""
    current = monotonic() if now is None else now
    with _AUTH_FAILURES_LOCK:
        failures = _AUTH_FAILURES.get(client)
        if failures is None:
            return 0
        _prune_auth_failures(failures, current)
        if not failures:
            _AUTH_FAILURES.pop(client, None)
            return 0
        if len(failures) < AUTH_FAILURE_LIMIT:
            return 0
        return max(1, math.ceil(AUTH_FAILURE_WINDOW_SECONDS - (current - failures[0])))


def record_authentication_failure(client: str, *, now: float | None = None) -> None:
    """Record a failed dashboard login without trusting proxy-supplied identity headers."""
    current = monotonic() if now is None else now
    with _AUTH_FAILURES_LOCK:
        failures = _AUTH_FAILURES.get(client)
        if failures is None:
            if len(_AUTH_FAILURES) >= MAX_TRACKED_AUTH_CLIENTS:
                oldest_client = min(
                    _AUTH_FAILURES,
                    key=lambda tracked: _AUTH_FAILURES[tracked][-1],
                )
                _AUTH_FAILURES.pop(oldest_client, None)
            failures = deque()
            _AUTH_FAILURES[client] = failures
        _prune_auth_failures(failures, current)
        failures.append(current)


def clear_authentication_failures(client: str) -> None:
    with _AUTH_FAILURES_LOCK:
        _AUTH_FAILURES.pop(client, None)


FEATURES: tuple[dict[str, object], ...] = (
    {
        "id": "market-data",
        "title": "Market data",
        "state": "implemented",
        "summary": "Strict trade/quote ingestion, feed quality, complete instrument economics, and exchange sessions.",
        "capabilities": ["Normalized trades and source/receive-time quotes", "Deterministic OHLCV bar builder", "Canonical ordering", "Duplicate, out-of-order, gap, delay, and stale-feed detection", "Effective-dated instrument registry", "Option/future/settlement economics", "Trading calendar with explicit venue and instrument halts", "Corporate-action inputs", "Immutable Parquet datasets"],
        "boundary": "The normalized feed contract is implemented; no licensed production market-data redistribution or vendor connection is configured.",
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
        "summary": "Reproducible point-in-time backtests, bounded strategy services, manifests, and experiment evidence.",
        "capabilities": ["Event-driven backtester", "Explicit spread, slippage, bar latency, and per-bar fill caps", "Point-in-time universe and historical-data controls", "Long/short positions with borrow limits and recalls", "Borrow and cash-debit financing", "Multi-currency FX and portfolio-margin capital checks", "Delisting settlements and corporate actions", "Python history, indicators, portfolio, state, and metrics services", "Python worker identity handshake and bundle hashing", "Configuration and dataset fingerprinting", "Exact accounting", "Performance and equity metrics", "Immutable reports and completion manifests", "Experiment catalog", "DuckDB dataset catalog", "Versioned S3-compatible artifact storage"],
        "boundary": "The advanced account kernel is broker-neutral; strategy workers cannot access adapters or credentials, and production-scale performance evidence remains external.",
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
        "capabilities": ["Risk cockpit", "P&L attribution", "Stable alert projection", "Daily UTC scheduling", "Typed schedule completion", "Hash-chained operations journal", "Model-risk decision register", "Fault-injection game-day register", "Parameter/configuration diff", "Two-person risk-limit evidence", "Immutable dashboard and reports"],
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
        "summary": "European-options analytics, expiry lifecycle, scenarios, and cross-environment reconciliation.",
        "capabilities": ["Frozen option chains", "Fixed-point European pricing", "Implied-volatility bisection", "Delta/Gamma/Vega/Theta/Rho", "Multi-leg expiry scenarios", "Long exercise and short assignment settlement", "Physical and cash option settlement", "BACKTEST/PAPER/LIVE book reconciliation", "Source-export and run-identity verification", "Immutable analytics reports"],
        "boundary": "The lifecycle kernel is broker-neutral; no broker exercise instruction, American-option valuation, or accepted capital session is configured.",
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
    {
        "id": "execution-risk",
        "title": "Execution & portfolio risk",
        "state": "implemented",
        "summary": "Deterministic EMS planning, frozen-benchmark transaction-cost analysis, and portfolio-wide pre-trade risk exposed through versioned local contracts.",
        "capabilities": ["Immediate, exact TWAP, forecast VWAP, POV, and arrival-price scheduling over gRPC", "Strict cancel-before-replace passive plans over gRPC with post-only chase collars", "Fee/latency/price-aware smart venue routing", "Bracket, trailing-stop, and basket planning", "Atomic ratio-bound, net-price-protected option-combination plans over gRPC", "Frozen arrival/target implementation-shortfall and fee analysis", "Explicit local p99 risk-evaluator benchmark artifacts", "Gross, net, leverage, concentration, drawdown, and margin limits", "Sector, asset-class, currency, strategy, and instrument limits", "Greeks, self-trade, open-order, and order-rate controls"],
        "boundary": "Planning and risk decisions are broker-neutral; every capital-bearing submission still requires controlled-LIVE approval and its reviewed adapter.",
        "gate": "Focused Rust and gRPC contract tests pass locally; broker-backed acceptance remains external.",
        "screens": ["Execution Blotter", "Risk Cockpit", "Portfolio"],
        "source": "core/execution + core/risk + follon-trading-api",
        "documentation": "docs/06-delivery/14-master-plan-conformance-audit.md",
    },
    {
        "id": "accounting",
        "title": "Multi-currency accounting",
        "state": "implemented",
        "summary": "Exact balanced journals, tax lots, financing, FX conversion, and portfolio margin valuation.",
        "capabilities": ["Per-currency double-entry balancing", "Idempotent journal projection", "FIFO, LIFO, and highest-cost tax-lot disposal", "Cash-debit and short-borrow financing accrual", "Fresh direct and inverse FX rates", "Multi-currency cash and long/short position valuation", "Initial and maintenance margin", "Margin-call and excess-liquidity projection"],
        "boundary": "Missing or stale FX and absent asset-class margin policy fail closed; independent broker statement ingestion remains deployment evidence.",
        "gate": "Exact accounting and margin tests pass locally.",
        "screens": ["Portfolio", "Journal"],
        "source": "core/accounting + PostgreSQL journal schema + follon-trading-api",
        "documentation": "docs/06-delivery/14-master-plan-conformance-audit.md",
    },
    {
        "id": "identity",
        "title": "Customer IAM",
        "state": "implemented",
        "summary": "Tenant-isolated passwords, TOTP/recovery MFA, opaque sessions, revocation, and server-side RBAC.",
        "capabilities": ["Argon2id password hashing and authenticated password rotation", "TOTP MFA with bounded challenges", "Hashed one-time recovery codes", "Opaque hashed short-lived sessions", "Lockout and immediate security-version revocation", "Tenant-isolated authorization", "Organization admin, risk, trader, read-only, and auditor roles"],
        "boundary": "Production enrollment, out-of-band delivery, support operations, and external identity-provider acceptance require deployment ownership.",
        "gate": "IAM unit tests and PostgreSQL identity schema checks pass locally.",
        "screens": ["Administration"],
        "source": "core/identity + PostgreSQL RLS identity schema",
        "documentation": "docs/05-quality-security/02-security-architecture.md",
    },
    {
        "id": "platform",
        "title": "Deployable application platform",
        "state": "gated",
        "summary": "Transactional PostgreSQL, gRPC topology, React production bundle, and least-privilege Tauri desktop packaging.",
        "capabilities": ["Checksum-bound versioned PostgreSQL migrations", "Forced row-level tenant security", "Atomic event plus outbox commit", "Order, execution, position, broker, strategy, configuration, risk, audit, identity, billing, and journal projections", "Concurrent skip-locked delivery", "mTLS-capable gRPC service", "React and Vite browser client", "Tauri v2 host with a separate native IPC command boundary and no privileged web-origin commands"],
        "boundary": "Production certificates, secret custody, deployment promotion, monitoring, backup drills, and installer signing are operator-controlled.",
        "gate": "Local compilation passes; container and signed-installer runtime acceptance must be recorded on deployment infrastructure.",
        "screens": ["Command Center", "Administration"],
        "source": "adapters/persistence/postgres + services/trading-api + apps/desktop",
        "documentation": "docs/operations/08-dashboard-deployment-runbook.md",
    },
    {
        "id": "news",
        "title": "News & event intelligence",
        "state": "replay-paper",
        "summary": "Validated local-fixture headline ingress, deterministic integer-BPS sentiment vectors, replay-to-paper intent evidence, and pre-trade shock collars.",
        "capabilities": ["Schema-validated local NDJSON headline fixtures", "Deterministic taxonomy and integer-BPS baseline scoring", "Canonical replay ordering with source sequence and causal evidence", "Python headline and sentiment callbacks", "Pre-trade news slippage and spread-multiplier collars", "Read-only news-to-risk evidence projection"],
        "boundary": "No vendor feed, latency claim, credential, broker connection, or automated live execution is included. Every strategy intent remains subject to the risk kernel before simulation.",
        "gate": "Replay/local-fixture unit and integration evidence can be run locally; vendor licensing, data-quality validation, and paper-session operational evidence remain external gates.",
        "screens": ["News Cockpit", "Research Lab", "Risk Cockpit"],
        "source": "core/news + core/control-plane + python/strategy-sdk",
        "documentation": "docs/01-domain/08-news-event-driven-trading-architecture.md",
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
    if "headline" in sample or "sentiment" in sample or "news" in name:
        return "news", "News & sentiment intelligence"
    if path.suffix.lower() == ".ipynb":
        return "research", "Research notebook"
    if "transaction_cost_schema_version" in sample or "tca" in name:
        return "execution-risk", "Transaction-cost analysis"
    if "benchmark_schema_version" in sample and "p99_micros" in sample:
        return "execution-risk", "Risk latency benchmark"
    if (
        "model_risk_register_schema_version" in sample

        or "game_day_register_schema_version" in sample
        or "operations.model_risk_recorded.v1" in sample
        or "operations.game_day_recorded.v1" in sample
        or "model-risk" in name
        or "game-day" in name
    ):
        return "operations", "Operations governance evidence"
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
        ".ipynb": "json",
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
                return sorted(files, key=lambda item: str(item["name"])), paths
    return sorted(files, key=lambda item: str(item["name"])), paths


def list_evidence() -> list[dict[str, object]]:
    return sorted(
        scan_evidence()[0],
        key=lambda item: (str(item["modified_at"]), str(item["name"])),
        reverse=True,
    )


def read_json_artifact(path: Path) -> dict[str, object] | None:
    """Read one bounded JSON object; invalid and non-object evidence is ignored."""
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) else None


def read_ndjson_artifact(path: Path) -> tuple[list[dict[str, object]], bool, int]:
    """Read a bounded prefix and disclose whether an append-only artifact was cut.

    The server must never imply that a bounded sample is the complete event
    trail.  Prefix semantics keep any in-window causal parent available for a
    later child; the returned metadata lets the UI state that newer records may
    exist outside the projection.
    """
    records: list[dict[str, object]] = []
    source_record_count = 0
    truncated = False
    try:
        with path.open("r", encoding="utf-8") as handle:
            for line in handle:
                if not line.strip():
                    continue
                source_record_count += 1
                if len(records) >= MAX_RECORDS_PER_ARTIFACT:
                    truncated = True
                    break
                try:
                    value = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(value, dict):
                    records.append(value)
    except (OSError, UnicodeDecodeError):
        return [], False, 0
    return records, truncated, source_record_count


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


def summarize_notebook(metadata: dict[str, object], payload: dict[str, object]) -> dict[str, object] | None:
    """Return inert notebook metadata without executing cells or returning outputs."""
    nbformat = payload.get("nbformat")
    cells = payload.get("cells")
    if (
        not isinstance(nbformat, int)
        or isinstance(nbformat, bool)
        or nbformat < 1
        or not isinstance(cells, list)
        or len(cells) > MAX_WORKSPACE_RECORDS
    ):
        return None
    code_cells = 0
    markdown_cells = 0
    output_count = 0
    for cell in cells:
        if not isinstance(cell, dict):
            return None
        cell_type = cell.get("cell_type")
        if cell_type == "code":
            code_cells += 1
            outputs = cell.get("outputs", [])
            if not isinstance(outputs, list):
                return None
            output_count += len(outputs)
        elif cell_type == "markdown":
            markdown_cells += 1
    notebook_metadata = payload.get("metadata")
    metadata_record = notebook_metadata if isinstance(notebook_metadata, dict) else {}
    kernelspec = metadata_record.get("kernelspec")
    kernel_record = kernelspec if isinstance(kernelspec, dict) else {}
    language_info = metadata_record.get("language_info")
    language_record = language_info if isinstance(language_info, dict) else {}
    return {
        "artifact": metadata["name"],
        "modified_at": metadata["modified_at"],
        "bytes": metadata["bytes"],
        "nbformat": nbformat,
        "cell_count": len(cells),
        "code_cells": code_cells,
        "markdown_cells": markdown_cells,
        "output_count": output_count,
        "kernel": kernel_record.get("display_name") if isinstance(kernel_record.get("display_name"), str) else "",
        "language": language_record.get("name") if isinstance(language_record.get("name"), str) else "",
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


def advanced_evidence_kind(payload: dict[str, object]) -> tuple[str, str] | None:
    """Return a reviewed advanced-evidence category and feature, if declared."""
    for schema_field, category, feature in ADVANCED_EVIDENCE_SCHEMAS:
        if payload.get(schema_field) == 1:
            return category, feature
    return None


def event_envelope_error(payload: dict[str, object]) -> str | None:
    """Return the reason a candidate fails the v1 canonical event envelope."""
    unexpected_fields = sorted(set(payload).difference(CANONICAL_EVENT_FIELDS))
    if unexpected_fields:
        return f"unexpected envelope field {unexpected_fields[0]}"
    missing_fields = sorted(CANONICAL_EVENT_FIELDS.difference(payload))
    if missing_fields:
        return f"missing required {missing_fields[0]}"

    schema_version = payload["schema_version"]
    if isinstance(schema_version, bool) or not isinstance(schema_version, int) or schema_version < 1:
        return "invalid schema_version"

    for field in ("event_id", "correlation_id"):
        if not isinstance(payload[field], str) or not CANONICAL_EVENT_ID.fullmatch(payload[field]):
            return f"missing or invalid {field}"
    for field in ("account_id", "strategy_id", "instrument_id", "causation_id"):
        value = payload[field]
        if value is not None and (not isinstance(value, str) or not CANONICAL_EVENT_ID.fullmatch(value)):
            return f"invalid {field}"

    if not CANONICAL_EVENT_TYPE.fullmatch(str(payload["event_type"])):
        return "invalid event_type"
    for field in ("event_time", "receive_time"):
        value = payload[field]
        if not isinstance(value, str) or not CANONICAL_UTC_TIMESTAMP.fullmatch(value):
            return f"invalid {field}"
        try:
            datetime.fromisoformat(value.replace("Z", "+00:00"))
        except ValueError:
            return f"invalid {field}"
    for field in ("actor", "source", "software_version", "configuration_version"):
        if not isinstance(payload[field], str) or not payload[field]:
            return f"missing or invalid {field}"
    if not isinstance(payload["payload"], dict):
        return "payload must be an object"
    return None


def is_event_like(payload: dict[str, object]) -> bool:
    """Avoid quietly treating a partial envelope as a canonical event."""
    return any(
        field in payload
        for field in (
            "event_id",
            "event_type",
            "event_time",
            "receive_time",
            "correlation_id",
            "causation_id",
            "actor",
            "source",
            "software_version",
            "configuration_version",
        )
    )


def event_timestamp(record: dict[str, object], field: str) -> datetime:
    """Read a prevalidated canonical UTC timestamp without lexical ordering."""
    payload = record.get("data")
    event = payload if isinstance(payload, dict) else {}
    event_time = str(event.get(field, ""))
    return datetime.fromisoformat(event_time.replace("Z", "+00:00"))


def replay_event_key(record: dict[str, object]) -> tuple[datetime, int]:
    """Sort replay by availability (`receive_time`) then retained artifact/line order."""
    return (
        event_timestamp(record, "receive_time"),
        int(record.get("_retained_index", 0)),
    )


def presentation_event_key(record: dict[str, object]) -> tuple[float, int]:
    """Keep newest source-time evidence first without reversing equal-time append order."""
    event_time = event_timestamp(record, "event_time")
    retained_index = int(record.get("_retained_index", 0))
    return (-event_time.timestamp(), retained_index)


def cyclic_event_ids(events: list[dict[str, object]]) -> set[str]:
    """Find only true causation cycles without recursive depth limits."""
    events_by_id = {
        str(record["data"]["event_id"]): record
        for record in events
        if isinstance(record.get("data"), dict)
    }
    visited: set[str] = set()
    cycles: set[str] = set()
    for start_id in events_by_id:
        if start_id in visited:
            continue
        path: list[str] = []
        path_positions: dict[str, int] = {}
        current_id = start_id
        while current_id in events_by_id and current_id not in visited:
            if current_id in path_positions:
                cycles.update(path[path_positions[current_id]:])
                break
            path_positions[current_id] = len(path)
            path.append(current_id)
            current = events_by_id[current_id]["data"]
            causation_id = current.get("causation_id") if isinstance(current, dict) else None
            current_id = causation_id if isinstance(causation_id, str) else ""
        visited.update(path)
    return cycles


def canonical_replay_events(events: list[dict[str, object]]) -> list[dict[str, object]]:
    """Return a deterministic, causation-respecting order for replay inspection.

    The presentation timeline remains reverse chronological.  The debugger must
    never step into a child event before an indexed parent when the causation
    relation is available. The projection has already rejected malformed,
    duplicate, cyclic, and unresolved-causation input before it reaches this
    function.
    """
    indexed_events = {
        str(record["data"]["event_id"]): record
        for record in events
        if isinstance(record.get("data"), dict)
    }
    children_by_parent: dict[str, list[str]] = {}
    ready: list[tuple[tuple[datetime, int], str]] = []
    for event_id, record in indexed_events.items():
        payload = record["data"]
        causation_id = payload.get("causation_id") if isinstance(payload, dict) else None
        if isinstance(causation_id, str):
            children_by_parent.setdefault(causation_id, []).append(event_id)
        else:
            heappush(ready, (replay_event_key(record), event_id))
    ordered: list[dict[str, object]] = []
    while ready:
        _, event_id = heappop(ready)
        record = indexed_events[event_id]
        ordered.append(record)
        for child_id in children_by_parent.get(event_id, []):
            heappush(ready, (replay_event_key(indexed_events[child_id]), child_id))
    if len(ordered) != len(indexed_events):
        raise RuntimeError("causal replay projection retained a cycle after validation")
    return ordered


def workspace_snapshot(as_of: str | None = None) -> dict[str, object]:
    """Build the bounded, read-only projection consumed by all operator workspaces."""
    artifacts, evidence_paths = scan_evidence()
    datasets: list[dict[str, object]] = []
    notebooks: list[dict[str, object]] = []
    backtests: list[dict[str, object]] = []
    experiments: list[dict[str, object]] = []
    manifests: list[dict[str, object]] = []
    event_candidates: list[dict[str, object]] = []
    omitted_event_candidate_count = 0
    events: list[dict[str, object]] = []
    invalid_artifact_event_ids: set[tuple[str, str]] = set()
    projection_diagnostics: list[dict[str, object]] = []
    event_windows: list[dict[str, object]] = []
    journals: list[dict[str, object]] = []
    commercial: list[dict[str, object]] = []
    execution_evidence: list[dict[str, object]] = []
    advanced_evidence: list[dict[str, object]] = []
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
            if path.suffix.lower() == ".ipynb":
                notebook = summarize_notebook(metadata, payload)
                if notebook is not None:
                    notebooks.append(notebook)
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
            elif ("transaction_cost" in payload or "benchmark_schema_version" in payload) and advanced_evidence_kind(payload) is None:
                execution_evidence.append(
                    {"artifact": name, "modified_at": metadata["modified_at"], "data": payload}
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
            else:
                advanced = advanced_evidence_kind(payload)
                if advanced is not None:
                    category, feature = advanced
                    advanced_evidence.append(
                        {
                            "artifact": name,
                            "modified_at": metadata["modified_at"],
                            "feature": feature,
                            "category": category,
                            "data": payload,
                        }
                    )
            continue
        if metadata["format"] != "ndjson":
            continue
        records, truncated, source_record_count = read_ndjson_artifact(path)
        seen_artifact_event_ids: set[str] = set()
        window_events: list[dict[str, object]] = []
        saw_event_like = False
        for record in records:
            enriched = {"artifact": name, "feature": metadata["feature"], "data": record}
            if "ledger_schema_version" in record and str(record.get("event_type", "")).startswith("commercial."):
                commercial.append(enriched)
                journals.append({**enriched, "category": "commercial"})
            elif is_event_like(record):
                saw_event_like = True
                error = event_envelope_error(record)
                if error is not None:
                    projection_diagnostics.append(
                        {"artifact": name, "code": "INVALID_EVENT_ENVELOPE", "detail": error}
                    )
                    continue
                event_id = str(record["event_id"])
                if event_id in seen_artifact_event_ids:
                    invalid_artifact_event_ids.add((name, event_id))
                    projection_diagnostics.append(
                        {"artifact": name, "code": "DUPLICATE_EVENT_ID", "detail": f"duplicate event ID {event_id}"}
                    )
                    continue
                seen_artifact_event_ids.add(event_id)
                window_events.append(record)
                if len(event_candidates) >= MAX_REPLAY_EVENT_CANDIDATES:
                    omitted_event_candidate_count += 1
                    continue
                enriched["_retained_index"] = len(event_candidates)
                event_candidates.append(enriched)
            elif "experiment_id" in record and "run_id" in record:
                experiments.append(enriched)
            elif "sequence" in record and ("entry_hash" in record or "record_hash" in record):
                category = "live" if "live" in name.lower() else "paper" if "paper" in name.lower() else "operations"
                journals.append({**enriched, "category": category})

        if saw_event_like or window_events or truncated:
            first_event = window_events[0] if window_events else {}
            last_event = window_events[-1] if window_events else {}
            event_windows.append(
                {
                    "artifact": name,
                    "window_kind": "prefix",
                    "source_record_count_lower_bound": source_record_count,
                    "retained_record_count": len(records),
                    "retained_event_count": len(window_events),
                    "truncated": truncated,
                    "first_event_id": first_event.get("event_id"),
                    "first_event_time": first_event.get("event_time"),
                    "last_event_id": last_event.get("event_id"),
                    "last_event_time": last_event.get("event_time"),
                }
            )

    event_id_counts: dict[str, int] = {}
    for candidate in event_candidates:
        payload = candidate["data"]
        event_id = str(payload["event_id"])
        event_id_counts[event_id] = event_id_counts.get(event_id, 0) + 1
    surviving_candidates: list[dict[str, object]] = []
    for candidate in event_candidates:
        payload = candidate["data"]
        event_id = str(payload["event_id"])
        if (str(candidate["artifact"]), event_id) in invalid_artifact_event_ids:
            continue
        if event_id_counts[event_id] > 1:
            projection_diagnostics.append(
                {
                    "artifact": candidate["artifact"],
                    "code": "DUPLICATE_EVENT_ID",
                    "detail": f"event ID {event_id} appears in multiple artifacts",
                }
            )
            continue
        surviving_candidates.append(candidate)

    candidates_by_id = {
        str(candidate["data"]["event_id"]): candidate
        for candidate in surviving_candidates
    }
    children_by_parent: dict[str, list[str]] = {}
    rejected_event_ids: set[str] = set()
    for event_id, candidate in candidates_by_id.items():
        causation_id = candidate["data"].get("causation_id")
        if not isinstance(causation_id, str):
            continue
        if causation_id not in candidates_by_id:
            projection_diagnostics.append(
                {
                    "artifact": candidate["artifact"],
                    "code": "UNRESOLVED_CAUSATION",
                    "detail": f"causation ID {causation_id} was rejected or unavailable",
                }
            )
            rejected_event_ids.add(event_id)
            continue
        children_by_parent.setdefault(causation_id, []).append(event_id)
    for event_id in cyclic_event_ids(surviving_candidates):
        candidate = candidates_by_id[event_id]
        projection_diagnostics.append(
            {
                "artifact": candidate["artifact"],
                "code": "CYCLIC_CAUSATION",
                "detail": f"event ID {event_id} belongs to a causation cycle",
            }
        )
        rejected_event_ids.add(event_id)
    rejected_queue = deque(rejected_event_ids)
    while rejected_queue:
        rejected_parent_id = rejected_queue.popleft()
        for child_id in children_by_parent.get(rejected_parent_id, []):
            if child_id in rejected_event_ids:
                continue
            child = candidates_by_id[child_id]
            projection_diagnostics.append(
                {
                    "artifact": child["artifact"],
                    "code": "UNRESOLVED_CAUSATION",
                    "detail": f"causation ID {rejected_parent_id} was rejected or unavailable",
                }
            )
            rejected_event_ids.add(child_id)
            rejected_queue.append(child_id)
    events = [
        candidate
        for candidate in surviving_candidates
        if str(candidate["data"]["event_id"]) not in rejected_event_ids
    ]
    if omitted_event_candidate_count > 0:
        projection_diagnostics.append(
            {
                "artifact": "workspace_projection",
                "code": "EVENT_CANDIDATE_CAP",
                "detail": (
                    f"retained {MAX_REPLAY_EVENT_CANDIDATES} canonical event candidates and omitted "
                    f"at least {omitted_event_candidate_count} additional candidates"
                ),
            }
        )

    replay_events = canonical_replay_events(events)
    events.sort(key=presentation_event_key)
    replay_events = [
        {key: value for key, value in record.items() if key != "_retained_index"}
        for record in replay_events
    ]
    events = [
        {key: value for key, value in record.items() if key != "_retained_index"}
        for record in events
    ]
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
    execution_evidence.sort(key=lambda item: str(item["modified_at"]), reverse=True)
    advanced_evidence.sort(key=lambda item: (str(item["modified_at"]), str(item["artifact"])), reverse=True)
    event_windows.sort(key=lambda item: str(item["artifact"]))
    projection_diagnostics.sort(key=lambda item: (str(item["artifact"]), str(item["code"]), str(item["detail"])))

    feature_counts = {str(feature["id"]): 0 for feature in FEATURES}
    for artifact in artifacts:
        feature = str(artifact["feature"])
        feature_counts[feature] = feature_counts.get(feature, 0) + 1

    if as_of:
        events = [ev for ev in events if str(ev["data"].get("receive_time", ev["data"].get("event_time", ""))) <= as_of]
        replay_events = [ev for ev in replay_events if str(ev["data"].get("receive_time", ev["data"].get("event_time", ""))) <= as_of]
        journals = [j for j in journals if str(j["data"].get("occurred_at", "")) <= as_of]

    return {
        "workspace_schema_version": 1,
        "as_of": as_of,
        "generated_at": datetime.now(UTC).isoformat(),
        "read_only": True,
        "counts": {
            "artifacts": len(artifacts),
            "datasets": len(datasets),
            "notebooks": len(notebooks),
            "backtests": len(backtests),
            "experiments": len(experiments),
            "events": len(events),
            "journals": len(journals),
            "commercial_records": len(commercial),
        },
        "feature_artifact_counts": feature_counts,
        "datasets": datasets[:MAX_WORKSPACE_RECORDS],
        "notebooks": notebooks[:MAX_WORKSPACE_RECORDS],
        "backtests": backtests[:MAX_WORKSPACE_RECORDS],
        "experiments": experiments[:MAX_WORKSPACE_RECORDS],
        "manifests": manifests[:MAX_WORKSPACE_RECORDS],
        "events": events[:MAX_WORKSPACE_RECORDS],
        "journals": journals[:MAX_WORKSPACE_RECORDS],
        "commercial": commercial[:MAX_WORKSPACE_RECORDS],
        "execution_evidence": execution_evidence[:MAX_WORKSPACE_RECORDS],
        "advanced_evidence": advanced_evidence[:MAX_WORKSPACE_RECORDS],
        "replay_events": replay_events[:MAX_WORKSPACE_RECORDS],
        "event_windows": event_windows[:MAX_WORKSPACE_RECORDS],
        "event_window": {
            "window_kind": "causal_prefix",
            "source_event_count_lower_bound": len(event_candidates) + omitted_event_candidate_count,
            "retained_event_count": min(len(replay_events), MAX_WORKSPACE_RECORDS),
            "truncated": any(bool(window["truncated"]) for window in event_windows)
            or omitted_event_candidate_count > 0
            or len(replay_events) > MAX_WORKSPACE_RECORDS,
        },
        "projection_diagnostics": projection_diagnostics[:MAX_WORKSPACE_RECORDS],
        "paper": latest_dashboard(paper_candidates),
        "live": latest_dashboard(live_candidates),
        "operations": latest_dashboard(operations_candidates),
        "options": latest_dashboard(options_candidates),
        "commercial_artifacts": [artifact for artifact in artifacts if artifact["feature"] == "commercial"],
    }


def build_decision_reconstruction(target_event_id: str) -> dict[str, object] | None:
    """Build an attributable causal decision provenance graph starting from any event."""
    snapshot = workspace_snapshot()
    events_list = snapshot.get("events", [])
    events_by_id = {
        str(item["data"]["event_id"]): item["data"]
        for item in events_list
        if isinstance(item, dict) and "data" in item and isinstance(item["data"], dict) and "event_id" in item["data"]
    }
    if target_event_id not in events_by_id:
        return None

    target_event = events_by_id[target_event_id]
    ev_type = str(target_event.get("event_type", ""))
    entity_type = (
        "fill" if "fill" in ev_type
        else "order_intent" if "intent" in ev_type
        else "risk_rejection" if "risk" in ev_type
        else "position" if "position" in ev_type
        else "alert"
    )

    causal_chain: list[dict[str, object]] = []
    edges: list[dict[str, str]] = []
    current_id: str | None = target_event_id
    visited: set[str] = set()
    integrity_status = "VERIFIED"

    while current_id:
        if current_id in visited:
            integrity_status = "TIMESTAMP_ANOMALY"
            break
        visited.add(current_id)
        event = events_by_id.get(current_id)
        if not event:
            integrity_status = "INCOMPLETE_CHAIN"
            break

        event_time_str = str(event.get("event_time", datetime.now(UTC).isoformat()))
        receive_time_str = str(event.get("receive_time", event_time_str))
        if receive_time_str < event_time_str:
            integrity_status = "TIMESTAMP_ANOMALY"

        content_hash = hashlib.sha256(json.dumps(event, sort_keys=True).encode("utf-8")).hexdigest()
        causal_chain.append({
            "node_id": event.get("event_id", current_id),
            "event_type": event.get("event_type", "unknown"),
            "actor": event.get("actor", "system"),
            "event_time": event_time_str,
            "available_at": receive_time_str,
            "causation_id": event.get("causation_id"),
            "content_hash": content_hash,
            "summary": f"{event.get('event_type')}: {event.get('actor')}",
        })
        parent_id = event.get("causation_id")
        if parent_id and isinstance(parent_id, str):
            edges.append({
                "from_node_id": parent_id,
                "to_node_id": current_id,
                "relation": "caused",
            })
            current_id = parent_id
        else:
            current_id = None

    causal_chain.reverse()
    return {
        "reconstruction_schema_version": 1,
        "reconstruction_id": f"recon.{target_event_id.replace('event.', '')}",
        "target_event_id": target_event_id,
        "target_entity_type": entity_type,
        "causal_chain": causal_chain,
        "edges": edges,
        "configuration_hash": "cfg-hash-follon-v1",
        "integrity_status": integrity_status,
        "verified_at": datetime.now(UTC).isoformat(),
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
            "trading-api": probe_tcp(TRADING_API_HOST, TRADING_API_PORT),
        },
        "artifacts": {
            "count": len(artifacts),
            "latest_at": artifacts[0]["modified_at"] if artifacts else None,
        },
    }


class DashboardHandler(BaseHTTPRequestHandler):
    server_version = "FollonEvidenceDashboard/2.0"

    TAURI_READ_ONLY_ORIGINS = frozenset({
        "http://tauri.localhost",
        "https://tauri.localhost",
        "tauri://localhost",
    })

    def end_headers(self) -> None:
        request_origin = getattr(self, "headers", {}).get("Origin")
        tauri_origin = (
            request_origin
            if request_origin in self.TAURI_READ_ONLY_ORIGINS
            else None
        )
        if tauri_origin is not None:
            self.send_header("Access-Control-Allow-Origin", tauri_origin)
            self.send_header("Vary", "Origin")
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("X-Frame-Options", "DENY")
        self.send_header("X-Robots-Tag", "noindex, noarchive")
        self.send_header(
            "Cross-Origin-Resource-Policy",
            "cross-origin" if tauri_origin is not None else "same-origin",
        )
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Permissions-Policy", "camera=(), geolocation=(), microphone=(), payment=(), usb=()")
        self.send_header("Content-Security-Policy", "default-src 'self'; base-uri 'none'; form-action 'none'; object-src 'none'; frame-ancestors 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:")
        super().end_headers()

    def do_GET(self) -> None:  # noqa: N802
        request = urlsplit(self.path)
        path = unquote(request.path)
        if path == "/api/v1/health":
            self.write_json(HTTPStatus.OK, {"status": "ok"})
            return
        client = self.client_address[0]
        retry_after = authentication_retry_after(client)
        if retry_after:
            self.request_rate_limited(retry_after)
            return
        if not self.is_authorized():
            record_authentication_failure(client)
            self.request_authentication()
            return
        clear_authentication_failures(client)
        if path == "/api/v1/status":
            self.write_json(HTTPStatus.OK, system_status())
            return
        if path == "/api/v1/features":
            self.write_json(HTTPStatus.OK, FEATURES)
            return
        if path == "/api/v1/workspaces":
            query_params = parse_qs(request.query)
            as_of_param = query_params.get("as_of", [None])[0]
            self.write_json(HTTPStatus.OK, workspace_snapshot(as_of=as_of_param))
            return
        if path.startswith("/api/v1/reconstruction/"):
            target_id = path.removeprefix("/api/v1/reconstruction/")
            recon = build_decision_reconstruction(target_id)
            if recon is None:
                self.write_json(HTTPStatus.NOT_FOUND, {"error": f"Event {target_id} not found for reconstruction"})
            else:
                self.write_json(HTTPStatus.OK, recon)
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
        self.write_body(body)

    def request_rate_limited(self, retry_after: int) -> None:
        body = b"Too many authentication attempts."
        self.send_response(HTTPStatus.TOO_MANY_REQUESTS)
        self.send_header("Retry-After", str(retry_after))
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.write_body(body)

    def write_json(self, status: HTTPStatus, payload: object) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.write_body(body)

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
        self.write_body(body)

    def write_body(self, body: bytes) -> None:
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionAbortedError, ConnectionResetError):
            # A browser or desktop webview may close while a bounded response is
            # in flight. The request has no mutation to roll back, so suppress
            # the expected transport noise without hiding application errors.
            self.close_connection = True

    def log_message(self, format: str, *args: object) -> None:
        print(f"{self.address_string()} - {format % args}")


def main() -> None:
    with ThreadingHTTPServer((HOST, PORT), DashboardHandler) as server:
        print(f"Follon evidence dashboard listening on http://{HOST}:{PORT}")
        server.serve_forever()


if __name__ == "__main__":
    main()
