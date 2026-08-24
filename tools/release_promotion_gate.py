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


def acceptance_ready(status_path: Path, target_environment: str) -> str:
    status = json.loads(status_path.read_text(encoding="utf-8"))
    if status.get("acceptance_status_schema_version") != 1:
        raise PromotionError("acceptance status has an unsupported schema")
    if target_environment == "production" and status.get("all_gates_eligible") is not True:
        raise PromotionError("production promotion is blocked by open acceptance gates")
    return sha256_file(status_path)


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
    parser.add_argument("--acceptance-status", required=True, type=Path)
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
        acceptance_hash = acceptance_ready(
            arguments.acceptance_status.resolve(strict=True),
            arguments.target_environment,
        )
        promoted_at = datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")
        write_receipt(arguments.receipt, {
            "release_promotion_receipt_schema_version": 1,
            "source_environment": arguments.source_environment,
            "target_environment": arguments.target_environment,
            "manifest_sha256": sha256_file(arguments.manifest),
            "signature_sha256": sha256_file(arguments.signature),
            "trusted_key_sha256": sha256_file(arguments.trusted_key),
            "acceptance_status_sha256": acceptance_hash,
            "requester": arguments.requester,
            "approver": arguments.approver,
            "change_ticket": arguments.change_ticket,
            "promoted_at": promoted_at,
            "decision": "eligible",
        })
    except (PromotionError, OSError, json.JSONDecodeError) as error:
        print(f"release promotion gate failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
