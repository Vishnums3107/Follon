#!/usr/bin/env python3
"""Verify signed release and evidence inputs before controlled environment promotion."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path

CANONICAL_ID = re.compile(r"^[a-z0-9._-]+$")
TRANSITIONS = {("development", "staging"), ("staging", "production")}


class PromotionError(RuntimeError):
    """Raised when a release is not eligible for promotion."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_approval(
    source_environment: str,
    target_environment: str,
    requester: str,
    approver: str,
    change_ticket: str,
) -> None:
    if (source_environment, target_environment) not in TRANSITIONS:
        raise PromotionError("unsupported release transition")
    for name, value in (("requester", requester), ("approver", approver), ("change_ticket", change_ticket)):
        if CANONICAL_ID.fullmatch(value) is None:
            raise PromotionError(f"{name} must be a canonical ID")
    if requester == approver:
        raise PromotionError("promotion requires a distinct approver")


def verify_release(
    repository_root: Path,
    manifest: Path,
    signature: Path,
    trusted_key: Path,
    artifact_root: Path,
) -> None:
    command = [
        "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
        "release-verify", str(manifest), str(signature), str(trusted_key),
        "--artifacts-root", str(artifact_root),
    ]
    result = subprocess.run(
        command,
        cwd=repository_root,
        shell=False,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        reason = result.stderr.strip().splitlines()[-1] if result.stderr.strip() else "verification failed"
        raise PromotionError(f"signed release verification failed: {reason[:512]}")


def acceptance_ready(repository_root: Path, ledger_root: Path, target_environment: str) -> tuple[dict[str, object], bytes]:
    """Recomputes acceptance from the ledger root with the published tool.

    The gate reads no status document. It used to, and a caller-authored one
    that simply declared every gate eligible passed it for production (E6.3).
    Returns the status and the exact bytes the tool emitted, which the
    receipt hashes.
    """
    result = subprocess.run(
        [sys.executable, str(repository_root / "tools" / "acceptance_evidence.py"), str(ledger_root)],
        cwd=repository_root,
        shell=False,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        stderr = result.stderr.decode("utf-8", errors="replace").strip()
        reason = stderr.splitlines()[-1] if stderr else "verification failed"
        raise PromotionError(f"acceptance ledgers failed verification: {reason[:512]}")
    try:
        status = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise PromotionError("acceptance tool returned malformed status") from error
    if not isinstance(status, dict) or status.get("acceptance_status_schema_version") != 2:
        raise PromotionError("acceptance tool returned an unsupported status")
    if target_environment == "production" and status.get("all_gates_eligible") is not True:
        raise PromotionError("production promotion is blocked by open acceptance gates")
    return status, result.stdout


def promotion_receipt(
    *,
    source_environment: str,
    target_environment: str,
    manifest: Path,
    signature: Path,
    trusted_key: Path,
    acceptance_status: dict[str, object],
    acceptance_status_bytes: bytes,
    requester: str,
    approver: str,
    change_ticket: str,
    promoted_at: str,
) -> dict[str, object]:
    """The eligibility receipt. Version 2 binds the recomputed acceptance
    status and every ledger file it counted (E6.3)."""
    return {
        "release_promotion_receipt_schema_version": 2,
        "source_environment": source_environment,
        "target_environment": target_environment,
        "manifest_sha256": sha256_file(manifest),
        "signature_sha256": sha256_file(signature),
        "trusted_key_sha256": sha256_file(trusted_key),
        "acceptance_status_sha256": hashlib.sha256(acceptance_status_bytes).hexdigest(),
        "acceptance_ledgers": acceptance_status["ledgers"],
        "requester": requester,
        "approver": approver,
        "change_ticket": change_ticket,
        "promoted_at": promoted_at,
        "decision": "eligible",
    }


def write_receipt(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        raise PromotionError("promotion receipt already exists")
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
    temporary.replace(path)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-environment", required=True)
    parser.add_argument("--target-environment", required=True)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--signature", required=True, type=Path)
    parser.add_argument("--trusted-key", required=True, type=Path)
    parser.add_argument("--artifacts-root", required=True, type=Path)
    parser.add_argument("--acceptance-ledger-root", required=True, type=Path)
    parser.add_argument("--requester", required=True)
    parser.add_argument("--approver", required=True)
    parser.add_argument("--change-ticket", required=True)
    parser.add_argument("--receipt", required=True, type=Path)
    arguments = parser.parse_args(argv)
    try:
        validate_approval(
            arguments.source_environment,
            arguments.target_environment,
            arguments.requester,
            arguments.approver,
            arguments.change_ticket,
        )
        repository_root = Path(__file__).resolve().parents[1]
        verify_release(
            repository_root,
            arguments.manifest.resolve(strict=True),
            arguments.signature.resolve(strict=True),
            arguments.trusted_key.resolve(strict=True),
            arguments.artifacts_root.resolve(strict=True),
        )
        acceptance_status, acceptance_status_bytes = acceptance_ready(
            repository_root,
            arguments.acceptance_ledger_root.resolve(strict=True),
            arguments.target_environment,
        )
        promoted_at = datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")
        write_receipt(arguments.receipt, promotion_receipt(
            source_environment=arguments.source_environment,
            target_environment=arguments.target_environment,
            manifest=arguments.manifest,
            signature=arguments.signature,
            trusted_key=arguments.trusted_key,
            acceptance_status=acceptance_status,
            acceptance_status_bytes=acceptance_status_bytes,
            requester=arguments.requester,
            approver=arguments.approver,
            change_ticket=arguments.change_ticket,
            promoted_at=promoted_at,
        ))
    except (PromotionError, OSError, json.JSONDecodeError) as error:
        print(f"release promotion gate failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
