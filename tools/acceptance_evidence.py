#!/usr/bin/env python3
"""Validate tamper-evident external acceptance ledgers and report real gate counts."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
ZERO_HASH = "0" * 64
MAX_LEDGER_BYTES = 64 * 1024 * 1024
MAX_LINE_BYTES = 1024 * 1024
CANONICAL_ID = re.compile(r"^[a-z0-9._-]+$")
SHA256 = re.compile(r"^[a-f0-9]{64}$")
# ASCII digits only: `\d` also matches other scripts' digits.
CANONICAL_UTC = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z")
EVIDENCE_TARGETS = {
    "paper_session": 30,
    "live_session": 60,
    "design_partner": 5,
    "broker_options": 1,
    "paying_customer": 1,
}
REQUIRED_KEYS = {
    "acceptance_evidence_schema_version",
    "evidence_id",
    "evidence_type",
    "subject_id",
    "occurred_at",
    "observed_by",
    "reviewed_by",
    "source_artifact_sha256",
    "outcome",
    "notes",
    "prev_hash",
    "record_hash",
}


class EvidenceError(ValueError):
    """Raised when an external record cannot be trusted."""


def canonical_body(record: dict[str, Any]) -> bytes:
    body = {key: value for key, value in record.items() if key != "record_hash"}
    return json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("utf-8")


def record_hash(record: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_body(record)).hexdigest()


def validate_timestamp(value: object) -> None:
    # Exactly YYYY-MM-DDTHH:MM:SSZ. `fromisoformat` alone also accepted a
    # space for the `T` and ISO week dates, both the same length (E6.2).
    if not isinstance(value, str) or CANONICAL_UTC.fullmatch(value) is None:
        raise EvidenceError("occurred_at must use second-precision YYYY-MM-DDTHH:MM:SSZ")
    try:
        datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as error:
        raise EvidenceError("occurred_at is not a real UTC time") from error


def validate_record(record: object, expected_previous: str) -> dict[str, Any]:
    if not isinstance(record, dict) or set(record) != REQUIRED_KEYS:
        raise EvidenceError("record keys do not match the v1 evidence contract")
    version = record["acceptance_evidence_schema_version"]
    # `type` rather than `isinstance`: JSON `true` is a `bool`, which is an
    # `int` equal to 1, and passed an equality check (E6.2).
    if type(version) is not int or version != SCHEMA_VERSION:
        raise EvidenceError("unsupported evidence schema version")
    for key in ("evidence_id", "subject_id", "observed_by", "reviewed_by"):
        value = record[key]
        if not isinstance(value, str) or CANONICAL_ID.fullmatch(value) is None:
            raise EvidenceError(f"{key} is not a canonical ID")
    if record["observed_by"] == record["reviewed_by"]:
        raise EvidenceError("observed_by and reviewed_by must be distinct")
    if record["evidence_type"] not in EVIDENCE_TARGETS:
        raise EvidenceError("unknown evidence_type")
    if record["outcome"] not in {"accepted", "rejected"}:
        raise EvidenceError("outcome must be accepted or rejected")
    if not isinstance(record["notes"], str) or len(record["notes"]) > 1024 or "\n" in record["notes"]:
        raise EvidenceError("notes must be a concise single line")
    validate_timestamp(record["occurred_at"])
    for key in ("source_artifact_sha256", "prev_hash", "record_hash"):
        if not isinstance(record[key], str) or SHA256.fullmatch(record[key]) is None:
            raise EvidenceError(f"{key} is not lowercase SHA-256")
    if record["prev_hash"] != expected_previous:
        raise EvidenceError("evidence hash chain is discontinuous")
    if record_hash(record) != record["record_hash"]:
        raise EvidenceError("evidence record hash does not match canonical content")
    return record


def load_ledgers(root: Path) -> list[dict[str, Any]]:
    root = root.resolve(strict=True)
    if not root.is_dir():
        raise EvidenceError("evidence root must be a directory")
    records: list[dict[str, Any]] = []
    seen_ids: set[str] = set()
    paths = sorted(root.rglob("*.acceptance.ndjson"))
    for path in paths:
        if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_LEDGER_BYTES:
            raise EvidenceError(f"unsafe or oversized evidence ledger: {path.name}")
        previous = ZERO_HASH
        data = path.read_bytes()
        if data and not data.endswith(b"\n"):
            raise EvidenceError(f"ledger must end with a complete newline: {path.name}")
        for line_number, line in enumerate(data.splitlines(), start=1):
            if not line or len(line) > MAX_LINE_BYTES:
                raise EvidenceError(f"invalid evidence line {path.name}:{line_number}")
            try:
                candidate = json.loads(line)
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise EvidenceError(f"invalid JSON at {path.name}:{line_number}") from error
            record = validate_record(candidate, previous)
            if record["evidence_id"] in seen_ids:
                raise EvidenceError(f"duplicate evidence_id: {record['evidence_id']}")
            seen_ids.add(record["evidence_id"])
            previous = record["record_hash"]
            records.append(record)
    return records


def status(records: list[dict[str, Any]]) -> dict[str, Any]:
    accepted_subjects: dict[str, set[str]] = defaultdict(set)
    rejected_subjects: dict[str, set[str]] = defaultdict(set)
    rejected = defaultdict(int)
    for record in records:
        if record["outcome"] == "accepted":
            accepted_subjects[record["evidence_type"]].add(record["subject_id"])
        else:
            rejected_subjects[record["evidence_type"]].add(record["subject_id"])
            rejected[record["evidence_type"]] += 1
    gates = {}
    for evidence_type, required in EVIDENCE_TARGETS.items():
        # A rejection disqualifies its subject within its gate, whichever
        # record came first. The ledger has no correction record, so an
        # acceptance can neither outlive a later rejection nor overturn an
        # earlier one (E6.2).
        disqualified = accepted_subjects[evidence_type] & rejected_subjects[evidence_type]
        observed = len(accepted_subjects[evidence_type] - rejected_subjects[evidence_type])
        gates[evidence_type] = {
            "observed": observed,
            "required": required,
            "remaining": max(0, required - observed),
            "eligible": observed >= required,
            "rejected_records": rejected[evidence_type],
            "disqualified_subjects": len(disqualified),
        }
    return {
        "acceptance_status_schema_version": 1,
        "verified_records": len(records),
        "all_gates_eligible": all(gate["eligible"] for gate in gates.values()),
        "gates": gates,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence_root", type=Path)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args(argv)
    try:
        report = status(load_ledgers(arguments.evidence_root))
    except (EvidenceError, OSError) as error:
        print(f"acceptance evidence verification failed: {error}", file=sys.stderr)
        return 2
    encoded = json.dumps(report, sort_keys=True, separators=(",", ":")) + "\n"
    if arguments.output is None:
        sys.stdout.write(encoded)
    else:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        temporary = arguments.output.with_suffix(arguments.output.suffix + ".tmp")
        temporary.write_text(encoded, encoding="utf-8", newline="\n")
        temporary.replace(arguments.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
