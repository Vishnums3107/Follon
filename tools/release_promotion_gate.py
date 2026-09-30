#!/usr/bin/env python3
"""Verify signed release and evidence inputs before controlled environment promotion."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

CANONICAL_ID = re.compile(r"^[a-z0-9._-]+$")
TRANSITIONS = {("development", "staging"), ("staging", "production")}


class PromotionError(RuntimeError):
    """Raised when a release is not eligible for promotion."""


@dataclass(frozen=True)
class VerifiedRelease:
    """Release identity and digests from the exact files given to the verifier."""

    release_id: str
    manifest_sha256: str
    signature_sha256: str
    trusted_key_sha256: str


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
) -> VerifiedRelease:
    """Verify snapshots from one read and bind the receipt to those same bytes.

    The operator-owned paths can change after `release-verify` exits. Passing them
    directly to the subprocess and later re-reading them let a different manifest's
    release ID and digest enter an eligible receipt (E6.6b finding 5).
    """
    manifest_bytes = manifest.read_bytes()
    signature_bytes = signature.read_bytes()
    trusted_key_bytes = trusted_key.read_bytes()
    with tempfile.TemporaryDirectory(prefix="follon-release-verify-") as directory:
        snapshot_root = Path(directory)
        snapshot_manifest = snapshot_root / "manifest.json"
        snapshot_signature = snapshot_root / "signature.json"
        snapshot_key = snapshot_root / "trusted-key.json"
        snapshot_manifest.write_bytes(manifest_bytes)
        snapshot_signature.write_bytes(signature_bytes)
        snapshot_key.write_bytes(trusted_key_bytes)
        command = [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "release-verify", str(snapshot_manifest), str(snapshot_signature), str(snapshot_key),
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
        if (
            snapshot_manifest.read_bytes() != manifest_bytes
            or snapshot_signature.read_bytes() != signature_bytes
            or snapshot_key.read_bytes() != trusted_key_bytes
        ):
            raise PromotionError("release verification inputs changed during verification")
    if result.returncode != 0:
        reason = result.stderr.strip().splitlines()[-1] if result.stderr.strip() else "verification failed"
        raise PromotionError(f"signed release verification failed: {reason[:512]}")
    return VerifiedRelease(
        release_id=release_id_of(manifest_bytes),
        manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
        signature_sha256=hashlib.sha256(signature_bytes).hexdigest(),
        trusted_key_sha256=hashlib.sha256(trusted_key_bytes).hexdigest(),
    )


def release_id_of(manifest: bytes) -> str:
    """The release a manifest describes: the only release whose evidence may count for it."""
    try:
        release_id = json.loads(manifest).get("release_id")
    except (ValueError, AttributeError) as error:
        raise PromotionError("the release manifest is unreadable") from error
    if not isinstance(release_id, str) or CANONICAL_ID.fullmatch(release_id) is None:
        raise PromotionError("the release manifest names no canonical release_id")
    return release_id


def acceptance_ready(
    repository_root: Path,
    ledger_root: Path,
    target_environment: str,
    *,
    trusted_reviewers: Path,
    artifact_root: Path,
    release_id: str,
) -> tuple[dict[str, object], bytes]:
    """Recomputes acceptance from the ledger root with the published tool.

    The gate reads no status document. It used to, and a caller-authored one
    that simply declared every gate eligible passed it for production (E6.3).
    It also counts only evidence a trusted reviewer signed, whose artifact is
    retained, and that exercised this release (E6.4). Returns the status and the
    exact bytes the tool emitted, which the receipt hashes.
    """
    result = subprocess.run(
        [
            sys.executable, str(repository_root / "tools" / "acceptance_evidence.py"), "audit",
            str(ledger_root),
            "--trusted-reviewers", str(trusted_reviewers),
            "--artifact-root", str(artifact_root),
            "--release-id", release_id,
        ],
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
    if (
        not isinstance(status, dict)
        or type(status.get("acceptance_status_schema_version")) is not int
        or status["acceptance_status_schema_version"] != 4
    ):
        raise PromotionError("acceptance tool returned an unsupported status")
    if status.get("release_id") != release_id:
        raise PromotionError("acceptance tool reported a different release")
    if (
        not isinstance(status.get("trusted_reviewers_sha256"), str)
        or re.fullmatch(r"[a-f0-9]{64}", status["trusted_reviewers_sha256"]) is None
        or not isinstance(status.get("ledgers"), list)
        or type(status.get("all_gates_eligible")) is not bool
    ):
        raise PromotionError("acceptance tool returned an incomplete status")
    if target_environment == "production" and status.get("all_gates_eligible") is not True:
        raise PromotionError("production promotion is blocked by open acceptance gates")
    return status, result.stdout


def promotion_receipt(
    *,
    source_environment: str,
    target_environment: str,
    release: VerifiedRelease,
    acceptance_status: dict[str, object],
    acceptance_status_bytes: bytes,
    requester: str,
    approver: str,
    change_ticket: str,
    promoted_at: str,
) -> dict[str, object]:
    """The eligibility receipt. Version 2 bound the recomputed acceptance status and
    every ledger file it counted (E6.3). Version 3 also names the release the evidence
    was counted for and the reviewer key set it was authenticated against (E6.4)."""
    if acceptance_status.get("release_id") != release.release_id:
        raise PromotionError("acceptance status does not belong to the verified release")
    return {
        "release_promotion_receipt_schema_version": 3,
        "release_id": release.release_id,
        "trusted_reviewers_sha256": acceptance_status["trusted_reviewers_sha256"],
        "source_environment": source_environment,
        "target_environment": target_environment,
        "manifest_sha256": release.manifest_sha256,
        "signature_sha256": release.signature_sha256,
        "trusted_key_sha256": release.trusted_key_sha256,
        "acceptance_status_sha256": hashlib.sha256(acceptance_status_bytes).hexdigest(),
        "acceptance_ledgers": acceptance_status["ledgers"],
        "requester": requester,
        "approver": approver,
        "change_ticket": change_ticket,
        "promoted_at": promoted_at,
        "decision": "eligible",
    }


def write_receipt(path: Path, value: object) -> None:
    """Publish one receipt atomically without following a staged link or replacing another."""
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    descriptor, temporary = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        try:
            # A hard link creates the final name only if absent, without a window
            # between an existence check and an overwriting replace. Both names
            # are in the same directory, so they remain on the same filesystem.
            os.link(temporary, path)
        except FileExistsError as error:
            raise PromotionError("promotion receipt already exists") from error
    finally:
        Path(temporary).unlink(missing_ok=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-environment", required=True)
    parser.add_argument("--target-environment", required=True)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--signature", required=True, type=Path)
    parser.add_argument("--trusted-key", required=True, type=Path)
    parser.add_argument("--artifacts-root", required=True, type=Path)
    parser.add_argument("--acceptance-ledger-root", required=True, type=Path)
    parser.add_argument("--acceptance-trusted-reviewers", required=True, type=Path)
    parser.add_argument("--acceptance-artifact-root", required=True, type=Path)
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
        release = verify_release(
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
            trusted_reviewers=arguments.acceptance_trusted_reviewers.resolve(strict=True),
            artifact_root=arguments.acceptance_artifact_root.resolve(strict=True),
            release_id=release.release_id,
        )
        promoted_at = datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")
        write_receipt(arguments.receipt, promotion_receipt(
            source_environment=arguments.source_environment,
            target_environment=arguments.target_environment,
            release=release,
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
