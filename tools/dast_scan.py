#!/usr/bin/env python3
"""Repository-authored dynamic scan of a local Follon deployment (E3.4).

Starts the real evidence dashboard (production mode, Basic authentication)
and the real ``follon-trading-api`` (a durable PAPER route behind operator
authentication) on loopback, then probes both over the network:

- the dashboard for authentication bypass, credential rate limiting, method
  tampering, path traversal, security headers, version disclosure, CORS,
  oversized requests, and leaked tracebacks;
- the gRPC API for unauthenticated, malformed, forged, wrong-role, wrong-tenant
  and revoked sessions, password lockout, the TOTP second factor and its
  replay, malformed and oversized messages, and unknown methods;
- both binaries' startup refusals for unsafe configurations.

This is a scan the repository wrote about itself. It is not an independent
DAST product run or a penetration test, and it closes no external gate. It
writes ``dast-report.json`` and ``dast-report.md`` and exits non-zero when any
probe fails.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import hmac
import http.client
import json
import os
import secrets
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

import grpc

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
SERVICE = "/follon.trading.v1.TradingOperatingSystem/"
TENANT = "tenant.alpha"
EXE = ".exe" if os.name == "nt" else ""


# --- probe bookkeeping ------------------------------------------------------


@dataclass
class Probe:
    probe_id: str
    target: str
    category: str
    description: str
    expected: str
    observed: str = ""
    passed: bool = False

    def as_json(self) -> dict[str, object]:
        return {
            "category": self.category,
            "description": self.description,
            "expected": self.expected,
            "id": self.probe_id,
            "observed": self.observed,
            "result": "PASS" if self.passed else "FAIL",
            "target": self.target,
        }


@dataclass
class Scan:
    probes: list[Probe] = field(default_factory=list)

    def record(self, probe_id: str, target: str, category: str, description: str,
               expected: str, observed: str, passed: bool) -> None:
        self.probes.append(Probe(probe_id, target, category, description, expected, observed, passed))
        mark = "PASS" if passed else "FAIL"
        print(f"  [{mark}] {probe_id} {description} -> {observed}", flush=True)


# --- helpers ----------------------------------------------------------------


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def wait_for_port(port: int, process: subprocess.Popen[bytes], seconds: float = 60.0) -> None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"process exited early with {process.returncode}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return
        except OSError:
            time.sleep(0.2)
    raise RuntimeError(f"port {port} never opened")


def stop(process: subprocess.Popen[bytes] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)


def exits_nonzero(command: list[str], env: dict[str, str], reason: str,
                  seconds: float = 30.0) -> tuple[bool, str]:
    """Runs a server that must refuse to start, for exactly ``reason``.

    A clean start is a failure, and so is a refusal for some other reason,
    which would otherwise let an unrelated error pass as the control.
    """
    process = subprocess.Popen(command, cwd=REPOSITORY_ROOT, env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        _, stderr = process.communicate(timeout=seconds)
    except subprocess.TimeoutExpired:
        stop(process)
        return False, "still running (it started)"
    text = stderr.decode("utf-8", "replace")
    tail = text.strip().splitlines()[-1:] or [""]
    return process.returncode != 0 and reason in text, f"exit {process.returncode}: {tail[0][:160]}"


def target_directory() -> Path:
    return Path(os.environ.get("CARGO_TARGET_DIR", REPOSITORY_ROOT / "target")) / "debug"


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def totp(secret: bytes, at: float) -> str:
    counter = struct.pack(">Q", int(at) // 30)
    digest = hmac.new(secret, counter, hashlib.sha1).digest()
    offset = digest[19] & 0x0F
    binary = struct.unpack(">I", digest[offset:offset + 4])[0] & 0x7FFFFFFF
    return f"{binary % 1_000_000:06d}"


def base32_secret(uri: str) -> bytes:
    query = uri.split("?", 1)[1]
    value = next(part.removeprefix("secret=") for part in query.split("&") if part.startswith("secret="))
    return base64.b32decode(value + "=" * (-len(value) % 8))


# --- minimal protobuf encoding (field numbers from operating_system.proto) ---


def _varint(value: int) -> bytes:
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        out.append(byte | (0x80 if value else 0))
        if not value:
            return bytes(out)


def pb_string(number: int, value: str) -> bytes:
    data = value.encode("utf-8")
    return _varint((number << 3) | 2) + _varint(len(data)) + data


def pb_message(number: int, data: bytes) -> bytes:
    return _varint((number << 3) | 2) + _varint(len(data)) + data


def pb_varint(number: int, value: int) -> bytes:
    return _varint(number << 3) + _varint(value)


def pb_fields(data: bytes) -> dict[int, list[object]]:
    """Decodes the top-level fields of a response, enough to read strings."""
    fields: dict[int, list[object]] = {}
    index = 0
    while index < len(data):
        key, shift = 0, 0
        while True:
            byte = data[index]
            index += 1
            key |= (byte & 0x7F) << shift
            shift += 7
            if not byte & 0x80:
                break
        number, wire = key >> 3, key & 7
        if wire == 0:
            value, shift = 0, 0
            while True:
                byte = data[index]
                index += 1
                value |= (byte & 0x7F) << shift
                shift += 7
                if not byte & 0x80:
                    break
            fields.setdefault(number, []).append(value)
        elif wire == 2:
            length, shift = 0, 0
            while True:
                byte = data[index]
                index += 1
                length |= (byte & 0x7F) << shift
                shift += 7
                if not byte & 0x80:
                    break
            fields.setdefault(number, []).append(data[index:index + length])
            index += length
        else:
            raise ValueError(f"unsupported wire type {wire}")
    return fields


def combo_request(intent_id: str, tenant: str = TENANT) -> bytes:
    legs = [
        ("inst.us_option.spy.500c", 1, "3"),
        ("inst.us_option.spy.505c", 2, "1"),
    ]
    body = b"".join([
        pb_string(1, tenant), pb_string(2, intent_id), pb_string(3, "acct.dast.paper"),
        pb_string(4, "strategy.dast"), pb_string(5, f"corr-{intent_id}"), pb_string(6, "2"),
        pb_varint(7, 1), pb_string(8, "2"),
    ])
    for instrument, side, price in legs:
        body += pb_message(9, pb_string(1, instrument) + pb_varint(2, side) + pb_varint(3, 1) + pb_string(4, price))
    body += b"".join([
        pb_varint(10, 1), pb_string(11, "dynamic scan probe"), pb_string(12, "2026-01-02T14:30:00Z"),
        pb_string(13, "strategy.dast.v1"), pb_string(14, "config.dast.v1"), pb_string(15, "PAPER"),
        pb_string(16, "2026-01-02T14:30:02Z"),
    ])
    for instrument, _, price in legs:
        body += pb_message(17, pb_string(1, instrument) + pb_string(2, price) + pb_string(3, "2026-01-02T14:30:00Z"))
    return body


# --- the dashboard ------------------------------------------------------------


class Dashboard:
    def __init__(self, port: int, username: str, password: str) -> None:
        self.port = port
        self.good = "Basic " + base64.b64encode(f"{username}:{password}".encode()).decode()
        self.bad = "Basic " + base64.b64encode(f"{username}:{password}x".encode()).decode()

    def request(self, method: str, path: str, headers: dict[str, str] | None = None,
                body: bytes | None = None) -> tuple[int, dict[str, str], bytes]:
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=15)
        try:
            connection.putrequest(method, path, skip_host=False, skip_accept_encoding=True)
            for name, value in (headers or {}).items():
                connection.putheader(name, value)
            if body is not None:
                connection.putheader("Content-Length", str(len(body)))
            connection.endheaders(body)
            response = connection.getresponse()
            return response.status, {k.lower(): v for k, v in response.getheaders()}, response.read()
        except (OSError, http.client.HTTPException):
            # A dropped connection is itself a finding (the handler crashed),
            # so it is recorded as status 0, which every probe treats as a failure.
            return 0, {}, b""
        finally:
            connection.close()

    def authorized(self, method: str, path: str, **headers: str) -> tuple[int, dict[str, str], bytes]:
        return self.request(method, path, {"Authorization": self.good, **headers})

    def clear_failures(self) -> None:
        # One success clears the per-peer failure window, so negative probes
        # never trip the rate limit before the rate-limit probe itself.
        self.authorized("GET", "/api/v1/features")


def scan_dashboard(scan: Scan, dashboard: Dashboard, evidence_root: Path) -> None:
    target = "dashboard"
    status, _, body = dashboard.request("GET", "/api/v1/health")
    scan.record("H01", target, "availability", "health is served without credentials",
                "200", str(status), status == 200 and b"ok" in body)

    protected = ["/api/v1/status", "/api/v1/features", "/api/v1/workspaces", "/api/v1/evidence",
                 "/api/v1/evidence/probe.json", "/api/v1/reconstruction/evt.missing", "/"]
    for index, path in enumerate(protected, start=2):
        status, headers, _ = dashboard.request("GET", path)
        challenged = "basic" in headers.get("www-authenticate", "").lower()
        scan.record(f"H{index:02d}", target, "authentication", f"{path} refuses a request with no credentials",
                    "401 with a Basic challenge", f"{status}", status == 401 and challenged)
        dashboard.clear_failures()

    variants = {
        "wrong password": dashboard.bad,
        "wrong scheme": "Bearer " + "a" * 64,
        "malformed base64": "Basic !!!not-base64!!!",
        "no separator": "Basic " + base64.b64encode(b"dast-operator").decode(),
        "empty": "Basic ",
    }
    for offset, (name, header) in enumerate(variants.items()):
        status, _, _ = dashboard.request("GET", "/api/v1/status", {"Authorization": header})
        scan.record(f"H{10 + offset:02d}", target, "authentication", f"credentials with a {name} are refused",
                    "401", str(status), status == 401)
        dashboard.clear_failures()

    for offset, path in enumerate(["/api/v1/status", "/api/v1/features", "/api/v1/workspaces", "/api/v1/evidence"]):
        status, _, body = dashboard.authorized("GET", path)
        scan.record(f"H{20 + offset:02d}", target, "authentication", f"{path} serves the right credentials",
                    "200", str(status), status == 200 and b"Traceback" not in body)

    for offset, method in enumerate(["POST", "PUT", "DELETE", "PATCH", "TRACE", "CONNECT"]):
        status, _, body = dashboard.request(method, "/api/v1/status", {"Authorization": dashboard.good}, b"{}")
        scan.record(f"H{30 + offset:02d}", target, "method tampering", f"{method} is not accepted on an API path",
                    "405 or 501, never 2xx", str(status), status in {405, 501} and b"Traceback" not in body)

    sentinel = evidence_root.parent / "outside-sentinel.json"
    traversals = [
        "/api/v1/evidence/..%2foutside-sentinel.json",
        "/api/v1/evidence/../outside-sentinel.json",
        "/api/v1/evidence/%2e%2e/outside-sentinel.json",
        "/api/v1/evidence/..%5coutside-sentinel.json",
        "/api/v1/evidence/%2e%2e%2f%2e%2e%2fCargo.toml",
        "/..%2f..%2fCargo.toml",
        "/%2e%2e/%2e%2e/Cargo.toml",
        "/api/v1/evidence/%00probe.json",
    ]
    marker = sentinel.read_bytes()
    for offset, path in enumerate(traversals):
        status, _, body = dashboard.authorized("GET", path)
        leaked = marker in body or b"[workspace]" in body
        scan.record(f"H{40 + offset:02d}", target, "path traversal", f"{path} does not escape the evidence root",
                    "a 4xx and no file content from outside the root", str(status),
                    400 <= status < 500 and not leaked)

    required = {
        "content-security-policy": "frame-ancestors 'none'",
        "x-content-type-options": "nosniff",
        "x-frame-options": "DENY",
        "referrer-policy": "no-referrer",
        "cache-control": "no-store",
    }
    for label, path, credentials in (("authenticated", "/api/v1/status", True),
                                      ("refused", "/api/v1/status", False)):
        if credentials:
            _, headers, _ = dashboard.authorized("GET", path)
        else:
            _, headers, _ = dashboard.request("GET", path)
            dashboard.clear_failures()
        missing = [name for name, value in required.items() if value not in headers.get(name, "")]
        scan.record(f"H5{0 if credentials else 1}", target, "security headers",
                    f"a {label} response carries every security header",
                    "CSP frame-ancestors 'none', nosniff, DENY, no-referrer, no-store",
                    "all present" if not missing else f"missing {', '.join(missing)}", not missing)

    for offset, (label, path, credentials) in enumerate((("an API response", "/api/v1/status", True),
                                                        ("a refused request", "/api/v1/status", False),
                                                        ("an unsupported method", "/api/v1/status", None))):
        if credentials is None:
            _, headers, _ = dashboard.request("DELETE", path, {"Authorization": dashboard.good})
        elif credentials:
            _, headers, _ = dashboard.authorized("GET", path)
        else:
            _, headers, _ = dashboard.request("GET", path)
            dashboard.clear_failures()
        server = headers.get("server", "")
        disclosed = "python" in server.lower() or any(character.isdigit() for character in server)
        scan.record(f"H{52 + offset}", target, "information disclosure",
                    f"{label} does not disclose the server software version",
                    "no Python or version string in Server", server or "(no Server header)", not disclosed)

    _, headers, _ = dashboard.authorized("GET", "/api/v1/status", Origin="https://evil.example")
    scan.record("H60", target, "CORS", "an untrusted Origin is not granted cross-origin access",
                "no Access-Control-Allow-Origin", headers.get("access-control-allow-origin", "(absent)"),
                "access-control-allow-origin" not in headers)
    _, headers, _ = dashboard.authorized("GET", "/api/v1/status", Origin="null")
    scan.record("H61", target, "CORS", "the null Origin is not granted cross-origin access",
                "no Access-Control-Allow-Origin", headers.get("access-control-allow-origin", "(absent)"),
                "access-control-allow-origin" not in headers)

    status, _, body = dashboard.authorized("GET", "/api/v1/evidence/" + "a" * 70_000)
    scan.record("H70", target, "oversized request", "a 70 KB request line is refused",
                "414 or another 4xx", str(status), 400 <= status < 500 and b"Traceback" not in body)
    status, _, body = dashboard.authorized("GET", "/api/v1/workspaces?as_of=" + "%ff" * 200)
    scan.record("H71", target, "malformed input", "a malformed as_of parameter does not crash the server",
                "a non-5xx response with no traceback", str(status), 100 <= status < 500 and b"Traceback" not in body)
    status, headers, _ = dashboard.authorized("GET", "/api/v1/reconstruction/" + "%0d%0aSet-Cookie:%20x=1")
    # Text echoed inside a JSON body is inert; only a real response header is an injection.
    scan.record("H72", target, "header injection", "CRLF in a path is not reflected as a header",
                "no Set-Cookie response header", f"{status}, set-cookie={'set-cookie' in headers}",
                100 <= status < 500 and "set-cookie" not in headers)

    # Rate limiting last: it locks this peer out for the window.
    statuses = []
    for _ in range(6):
        status, headers, _ = dashboard.request("GET", "/api/v1/status", {"Authorization": dashboard.bad})
        statuses.append(status)
    scan.record("H80", target, "credential rate limiting", "repeated wrong passwords are rate limited",
                "401 five times, then 429 with Retry-After", ",".join(map(str, statuses)),
                statuses[:5] == [401] * 5 and statuses[5] == 429 and "retry-after" in headers)
    status, _, _ = dashboard.authorized("GET", "/api/v1/status")
    scan.record("H81", target, "credential rate limiting", "a locked-out peer cannot get in by then guessing right",
                "429", str(status), status == 429)
    status, _, _ = dashboard.request("GET", "/api/v1/health")
    scan.record("H99", target, "availability", "the dashboard is still healthy after every probe",
                "200", str(status), status == 200)


# --- the gRPC trading API -------------------------------------------------------


class Api:
    def __init__(self, port: int) -> None:
        self.channel = grpc.insecure_channel(f"127.0.0.1:{port}")
        grpc.channel_ready_future(self.channel).result(timeout=30)

    def call(self, method: str, body: bytes, token: str | None = None,
             header: str | None = None) -> tuple[grpc.StatusCode, bytes]:
        metadata = []
        if header is not None:
            metadata.append(("authorization", header))
        elif token is not None:
            metadata.append(("authorization", f"Bearer {token}"))
        stub = self.channel.unary_unary(SERVICE + method)
        try:
            return grpc.StatusCode.OK, stub(body, metadata=metadata, timeout=30)
        except grpc.RpcError as error:
            return error.code(), b""

    def login(self, email: str, password: str, secret: bytes) -> tuple[grpc.StatusCode, str, str]:
        code, body = self.call("BeginOperatorLogin", pb_string(1, TENANT) + pb_string(2, email) + pb_string(3, password))
        if code != grpc.StatusCode.OK:
            return code, "", ""
        challenge = pb_fields(body)[1][0].decode()
        otp = totp(secret, time.time())
        code, body = self.call("CompleteOperatorLogin", pb_string(1, challenge) + pb_string(2, otp))
        if code != grpc.StatusCode.OK:
            return code, "", otp
        return code, pb_fields(body)[1][0].decode(), otp


def scan_api(scan: Scan, api: Api, operators: dict[str, tuple[str, bytes]], password: str) -> None:
    target = "trading-api"
    code, _ = api.call("CheckHealth", b"")
    scan.record("G01", target, "availability", "health answers", "OK", code.name, code == grpc.StatusCode.OK)

    code, _ = api.call("SubmitPaperCombo", combo_request("intent.dast.none"))
    scan.record("G02", target, "authentication", "a write with no session is refused",
                "UNAUTHENTICATED", code.name, code == grpc.StatusCode.UNAUTHENTICATED)
    for offset, header in enumerate(["Basic ZGFzdDpkYXN0", "Bearer", "Bearer " + "z" * 64,
                                     "Bearer " + "a" * 63, "bearer " + "a" * 64]):
        code, _ = api.call("SubmitPaperCombo", combo_request("intent.dast.malformed"), header=header)
        scan.record(f"G{3 + offset:02d}", target, "authentication", f"a malformed authorization header is refused ({header[:12]}...)",
                    "UNAUTHENTICATED", code.name, code == grpc.StatusCode.UNAUTHENTICATED)
    code, _ = api.call("SubmitPaperCombo", combo_request("intent.dast.forged"), token=secrets.token_hex(32))
    scan.record("G10", target, "authentication", "a well-formed session nobody issued is refused",
                "PERMISSION_DENIED", code.name, code == grpc.StatusCode.PERMISSION_DENIED)

    email, secret = operators["user.trader"]
    code, _ = api.call("BeginOperatorLogin", pb_string(1, TENANT) + pb_string(2, email) + pb_string(3, password + "x"))
    scan.record("G11", target, "authentication", "a wrong password is refused",
                "UNAUTHENTICATED", code.name, code == grpc.StatusCode.UNAUTHENTICATED)
    code, body = api.call("BeginOperatorLogin", pb_string(1, TENANT) + pb_string(2, email) + pb_string(3, password))
    challenge = pb_fields(body)[1][0].decode() if code == grpc.StatusCode.OK else ""
    wrong = f"{(int(totp(secret, time.time())) + 1) % 1_000_000:06d}"
    code, _ = api.call("CompleteOperatorLogin", pb_string(1, challenge) + pb_string(2, wrong))
    scan.record("G12", target, "second factor", "a wrong TOTP code is refused",
                "UNAUTHENTICATED", code.name, code == grpc.StatusCode.UNAUTHENTICATED)

    code, trader, used_code = api.login(email, password, secret)
    scan.record("G13", target, "second factor", "password plus the right TOTP code issues a session",
                "OK", code.name, code == grpc.StatusCode.OK and len(trader) == 64)
    code, body = api.call("BeginOperatorLogin", pb_string(1, TENANT) + pb_string(2, email) + pb_string(3, password))
    challenge = pb_fields(body)[1][0].decode() if code == grpc.StatusCode.OK else ""
    code, _ = api.call("CompleteOperatorLogin", pb_string(1, challenge) + pb_string(2, used_code))
    scan.record("G14", target, "second factor", "a TOTP code cannot be replayed for a second session",
                "UNAUTHENTICATED", code.name, code == grpc.StatusCode.UNAUTHENTICATED)

    viewer_email, viewer_secret = operators["user.viewer"]
    code, viewer, _ = api.login(viewer_email, password, viewer_secret)
    code, _ = api.call("SubmitPaperCombo", combo_request("intent.dast.viewer"), token=viewer)
    scan.record("G15", target, "authorization", "a read-only operator cannot write",
                "PERMISSION_DENIED", code.name, code == grpc.StatusCode.PERMISSION_DENIED)
    code, _ = api.call("SubmitPaperCombo", combo_request("intent.dast.tenant", tenant="tenant.beta"), token=trader)
    scan.record("G16", target, "authorization", "a trader's session is refused for another tenant",
                "PERMISSION_DENIED", code.name, code == grpc.StatusCode.PERMISSION_DENIED)
    code, body = api.call("SubmitPaperCombo", combo_request("intent.dast.approved"), token=trader)
    fields = pb_fields(body) if code == grpc.StatusCode.OK else {}
    submitted_by = fields.get(7, [b""])[0].decode() if fields else ""
    approved = fields.get(2, [0])[0] == 1 if fields else False
    scan.record("G17", target, "authorization", "a trader's session writes and is recorded as the submitter",
                "OK, approved, submitted_by user.trader", f"{code.name}, approved={approved}, submitted_by={submitted_by}",
                code == grpc.StatusCode.OK and approved and submitted_by == "user.trader")

    code, body = api.call("RevokeOperatorSession", b"", token=trader)
    code, _ = api.call("SubmitPaperCombo", combo_request("intent.dast.revoked"), token=trader)
    scan.record("G18", target, "session", "a revoked session cannot write",
                "PERMISSION_DENIED", code.name, code == grpc.StatusCode.PERMISSION_DENIED)

    lockout_email, lockout_secret = operators["user.lockout"]
    for _ in range(5):
        api.call("BeginOperatorLogin", pb_string(1, TENANT) + pb_string(2, lockout_email) + pb_string(3, "Wrong-Guess-9-Password"))
    code, _, _ = api.login(lockout_email, password, lockout_secret)
    scan.record("G19", target, "credential lockout", "five wrong passwords lock the account even against the right one",
                "UNAUTHENTICATED", code.name, code == grpc.StatusCode.UNAUTHENTICATED)
    code, _ = api.call("BeginOperatorLogin", pb_string(1, TENANT) + pb_string(2, "nobody@example.com") + pb_string(3, password))
    scan.record("G20", target, "account enumeration", "an unknown operator fails exactly like a wrong password",
                "UNAUTHENTICATED", code.name, code == grpc.StatusCode.UNAUTHENTICATED)

    code, _ = api.call("SubmitPaperCombo", b"\xff\xff\xff\xff\x0f", token=secrets.token_hex(32))
    scan.record("G21", target, "malformed input", "an undecodable message is refused",
                "a non-OK status", code.name, code != grpc.StatusCode.OK)
    code, _ = api.call("SubmitPaperCombo", pb_string(11, "x" * (5 * 1024 * 1024)), token=secrets.token_hex(32))
    # tonic reports a message over its decoding limit as OUT_OF_RANGE; a
    # client-side limit would be RESOURCE_EXHAUSTED. Either is a refusal.
    scan.record("G22", target, "oversized input", "a 5 MiB message is refused",
                "OUT_OF_RANGE or RESOURCE_EXHAUSTED", code.name,
                code in {grpc.StatusCode.OUT_OF_RANGE, grpc.StatusCode.RESOURCE_EXHAUSTED})
    code, _ = api.call("GrantAdministrator", b"")
    scan.record("G23", target, "unknown method", "an undeclared method is not served",
                "UNIMPLEMENTED", code.name, code == grpc.StatusCode.UNIMPLEMENTED)
    code, _ = api.call("CheckHealth", b"")
    scan.record("G99", target, "availability", "the API is still healthy after every probe",
                "OK", code.name, code == grpc.StatusCode.OK)


# --- orchestration ----------------------------------------------------------------


def build_binaries() -> tuple[Path, Path]:
    for command in (["cargo", "build", "-q", "-p", "follon-trading-api"],
                    ["cargo", "build", "-q", "-p", "follon-cli", "--bin", "follon-admin"]):
        subprocess.run(command, cwd=REPOSITORY_ROOT, check=True)
    return target_directory() / f"follon-trading-api{EXE}", target_directory() / f"follon-admin{EXE}"


def provision(admin: Path, directory: Path, password_file: Path) -> dict[str, tuple[str, bytes]]:
    operators = {}
    for user, roles in (("user.trader", "trader"), ("user.viewer", "read_only"), ("user.lockout", "trader")):
        email = f"{user.removeprefix('user.')}@example.com"
        result = subprocess.run(
            [str(admin), "operator-add", "--directory", str(directory), "--tenant-id", TENANT,
             "--user-id", user, "--email", email, "--roles", roles, "--password-file", str(password_file)],
            cwd=REPOSITORY_ROOT, capture_output=True, text=True, check=True)
        uri = next(line.removeprefix("totp_uri=") for line in result.stdout.splitlines() if line.startswith("totp_uri="))
        operators[user] = (email, base32_secret(uri))
    return operators


def route_config(path: Path, journal: Path) -> None:
    path.write_text(json.dumps({
        "schema_version": 1, "account_id": "acct.dast.paper", "currency": "USD", "initial_cash": "100000",
        "risk_policy_version": "risk.dast.v1", "trading_calendar_id": "cal.us.options.dast",
        "max_order_quantity": "100", "max_order_notional": "50000", "max_price_deviation_bps": "100",
        "max_open_orders": 10, "max_position_quantity": "1000", "max_realized_loss": "10000",
        "max_market_data_age_seconds": 5, "max_order_rate": 20, "order_rate_window_seconds": 60,
        "instrument_tick_sizes": {"inst.us_equity.spy": "0.01", "inst.us_option.spy.500c": "0.01",
                                 "inst.us_option.spy.505c": "0.01"},
        "instrument_lot_sizes": {"inst.us_equity.spy": "1", "inst.us_option.spy.500c": "1",
                                "inst.us_option.spy.505c": "1"}, "short_exposure": {"max_short_quantity": "1000"},
        "kill_switch_version": "kills.dast.v1", "adapter_kind": "IBKR_PAPER_MODEL", "journal_path": str(journal),
    }), encoding="utf-8")


def base_env() -> dict[str, str]:
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("FOLLON_GRPC_", "FOLLON_TRADING_API_", "FOLLON_DASHBOARD_", "FOLLON_DATABASE_", "FOLLON_DEPLOYMENT_"))}
    return env


def write_report(output_dir: Path, scan: Scan, targets: list[dict[str, object]]) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    failed = [probe for probe in scan.probes if not probe.passed]
    report = {
        "dast_report_schema_version": 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "scanner": "tools/dast_scan.py",
        "independent": False,
        "statement": "A repository-authored dynamic scan of a local loopback deployment. It is not an independent DAST product run or a penetration test, and it closes no external gate.",
        "targets": targets,
        "summary": {"probes": len(scan.probes), "passed": len(scan.probes) - len(failed), "failed": len(failed)},
        "probes": [probe.as_json() for probe in scan.probes],
    }
    json_path = output_dir / "dast-report.json"
    json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    lines = [
        "# Follon dynamic scan (repository-authored)", "",
        report["statement"], "",
        f"- Generated: {report['generated_at']}",
        f"- Probes: {len(scan.probes)}, passed {len(scan.probes) - len(failed)}, failed {len(failed)}", "",
        "| ID | Target | Category | Probe | Expected | Observed | Result |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for probe in scan.probes:
        cells = [probe.probe_id, probe.target, probe.category, probe.description, probe.expected,
                 probe.observed, "PASS" if probe.passed else "FAIL"]
        lines.append("| " + " | ".join(cell.replace("|", "\\|") for cell in cells) + " |")
    (output_dir / "dast-report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output-dir", type=Path, default=REPOSITORY_ROOT / "var" / "dast")
    arguments = parser.parse_args()
    api_binary, admin = build_binaries()
    scan = Scan()
    workspace = Path(tempfile.mkdtemp(prefix="follon-dast-"))
    dashboard_process = api_process = None
    targets: list[dict[str, object]] = []
    try:
        # The dashboard serves a scratch evidence root with a sentinel just
        # outside it, so a traversal is detected by content, not by guesswork.
        evidence_root = workspace / "evidence"
        evidence_root.mkdir()
        (evidence_root / "probe.json").write_text('{"probe": true}\n', encoding="utf-8")
        (workspace / "outside-sentinel.json").write_text('{"sentinel": "' + secrets.token_hex(16) + '"}\n', encoding="utf-8")
        username, password = "dast-operator", secrets.token_urlsafe(24)
        dashboard_port = free_port()
        env = base_env() | {
            "FOLLON_DASHBOARD_MODE": "production", "FOLLON_DASHBOARD_HOST": "127.0.0.1",
            "FOLLON_DASHBOARD_PORT": str(dashboard_port), "FOLLON_DASHBOARD_USERNAME": username,
            "FOLLON_DASHBOARD_PASSWORD": password, "FOLLON_EVIDENCE_ROOT": str(evidence_root),
            "FOLLON_TRADING_API_HOST": "127.0.0.1", "FOLLON_TRADING_API_PORT": "1",
            "FOLLON_POSTGRES_HOST": "127.0.0.1", "FOLLON_POSTGRES_INTERNAL_PORT": "1",
            "FOLLON_MINIO_HEALTH_URL": "http://127.0.0.1:1/health",
        }
        dashboard_command = [sys.executable, "apps/desktop/server.py"]
        dashboard_process = subprocess.Popen(dashboard_command, cwd=REPOSITORY_ROOT, env=env,
                                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_for_port(dashboard_port, dashboard_process)
        print("[+] Scanning the dashboard", flush=True)
        scan_dashboard(scan, Dashboard(dashboard_port, username, password), evidence_root)

        operator_password = "Scan-" + secrets.token_urlsafe(18) + "-9a"
        password_file = workspace / "operator-password.txt"
        password_file.write_text(operator_password + "\n", encoding="utf-8")
        directory = workspace / "operators.json"
        operators = provision(admin, directory, password_file)
        route = workspace / "paper-route.json"
        route_config(route, workspace / "paper-journal.ndjson")
        api_port = free_port()
        api_env = base_env() | {
            "FOLLON_GRPC_BIND": f"127.0.0.1:{api_port}",
            "FOLLON_TRADING_API_PAPER_CONFIG": str(route),
            "FOLLON_TRADING_API_OPERATOR_DIRECTORY": str(directory),
        }
        api_process = subprocess.Popen([str(api_binary)], cwd=REPOSITORY_ROOT, env=api_env,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_for_port(api_port, api_process)
        print("[+] Scanning the trading API", flush=True)
        scan_api(scan, Api(api_port), operators, operator_password)

        print("[+] Probing unsafe startup configurations", flush=True)
        refused, observed = exits_nonzero(dashboard_command, env | {
            "FOLLON_DASHBOARD_PASSWORD": "short", "FOLLON_DASHBOARD_PORT": str(free_port())},
            "production dashboard mode requires")
        scan.record("C01", "dashboard", "configuration", "production mode refuses a short password",
                    "a non-zero exit", observed, refused)
        # Each startup probe gets its own route and journal, so a refusal can
        # never come from the running API still holding the first journal.
        isolated_route = workspace / "paper-route-startup.json"
        route_config(isolated_route, workspace / "paper-journal-startup.ndjson")
        no_directory = {key: value for key, value in api_env.items() if key != "FOLLON_TRADING_API_OPERATOR_DIRECTORY"}
        refused, observed = exits_nonzero([str(api_binary)], no_directory | {
            "FOLLON_GRPC_BIND": f"127.0.0.1:{free_port()}", "FOLLON_TRADING_API_PAPER_CONFIG": str(isolated_route)},
            "requires FOLLON_TRADING_API_OPERATOR_DIRECTORY")
        scan.record("C02", "trading-api", "configuration", "a PAPER route refuses to start without an operator directory",
                    "a non-zero exit", observed, refused)
        remote = {key: value for key, value in api_env.items() if key != "FOLLON_TRADING_API_PAPER_CONFIG"}
        refused, observed = exits_nonzero([str(api_binary)], remote | {"FOLLON_GRPC_BIND": f"0.0.0.0:{free_port()}"},
                                          "operator login off loopback requires server TLS")
        scan.record("C03", "trading-api", "configuration", "operator login refuses a plaintext non-loopback bind",
                    "a non-zero exit", observed, refused)

        targets = [
            {"name": "dashboard", "entry_point": "apps/desktop/server.py", "mode": "production",
             "bind": "127.0.0.1", "entry_point_sha256": sha256_file(REPOSITORY_ROOT / "apps/desktop/server.py")},
            {"name": "trading-api", "entry_point": api_binary.name, "bind": "127.0.0.1",
             "entry_point_sha256": sha256_file(api_binary), "paper_route": True, "operator_directory": True},
        ]
    except Exception as error:  # noqa: BLE001 - any harness error must fail the scan, not skip the report
        scan.record("X00", "scanner", "harness", "the scan ran to completion",
                    "no harness error", f"{type(error).__name__}: {error}"[:200], False)
    finally:
        stop(dashboard_process)
        stop(api_process)
        shutil.rmtree(workspace, ignore_errors=True)
    report = write_report(arguments.output_dir, scan, targets)
    failed = sum(not probe.passed for probe in scan.probes)
    print(f"\n[{'-' if failed else '+'}] {len(scan.probes)} probes, {failed} failed; report: {report}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
