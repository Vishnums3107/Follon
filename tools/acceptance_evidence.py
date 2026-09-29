#!/usr/bin/env python3
"""Verify tamper-evident external acceptance ledgers and report real gate counts.

Schema 2 (delivery state E6.4 and E6.5) makes a record something a reviewer signed,
about a release, backed by an artifact, against criteria fixed before it counts:

* A record carries the reviewer's Ed25519 signature. It counts only under a trusted
  reviewer key set, and only if the key belongs to the reviewer the record names.
* Its source artifact is re-hashed against a retained artifact root. An artifact that
  is missing or differs is a record that cannot be checked, so it does not count.
* It names the release and the environment it exercised, and counts only toward that
  release's gates.
* Its `attributes` state what the session, partner or customer actually was, and an
  acceptance counts only if they meet the criteria: what a "clean" session means, and
  how a reconnect, an unresolved `UNKNOWN` order or a discrepancy is treated.

A record that is malformed, mis-chained or mis-hashed makes the whole ledger
untrustworthy, so the audit fails. A record that is well formed but does not qualify
is listed with its reason and is never counted.

    acceptance_evidence.py audit LEDGER_ROOT --trusted-reviewers FILE --artifact-root DIR --release-id ID
    acceptance_evidence.py append LEDGER --record TEMPLATE --artifact FILE --artifact-root DIR
                                          --reviewer-key KEY.pk8 --reviewer-key-id ID
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

try:
    from tools import ed25519
except ImportError:  # run as a script: the tools directory itself is on sys.path
    import ed25519  # type: ignore[no-redef]

SCHEMA_VERSION = 2
STATUS_SCHEMA_VERSION = 3
SIGNATURE_DOMAIN = b"follon-acceptance-evidence-v2\n"
ZERO_HASH = "0" * 64
MAX_LEDGER_BYTES = 64 * 1024 * 1024
MAX_LINE_BYTES = 1024 * 1024
CANONICAL_ID = re.compile(r"^[a-z0-9._-]+$")
SHA256 = re.compile(r"^[a-f0-9]{64}$")
SIGNATURE = re.compile(r"^[a-f0-9]{128}$")
# ASCII digits only: `\d` also matches other scripts' digits.
CANONICAL_UTC = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z")

# The gates, each as the alternatives that satisfy it: a customer kind and how many
# distinct accepted subjects of that kind are needed. A gate is eligible when any
# alternative is met. The roadmap's commercial gate is ten paying professionals or
# three paying organisations (docs/06-delivery/03-roadmap-and-gates.md).
GATES: dict[str, tuple[tuple[str | None, int], ...]] = {
    "paper_session": ((None, 30),),
    "live_session": ((None, 60),),
    "design_partner": ((None, 5),),
    "broker_options": ((None, 1),),
    "paying_customer": (("professional", 10), ("organisation", 3)),
}
CUSTOMER_KINDS = ("professional", "organisation")

# A session runs in the environment its type names. The other types run in none.
SESSION_ENVIRONMENTS = {"paper_session": "PAPER", "live_session": "LIVE"}

# What a clean session is (E6.5), fixed before any session counts. A regular US equity
# session is 6.5 hours, so a shorter run has not seen one. A session with no order proves
# nothing about the order path. Every order must be resolved and reconciled at the close:
# no `UNKNOWN`, no discrepancy, no unexplained incident. A reconnect that nobody planned
# is a fault the session did not survive cleanly, whatever came after it, so it disqualifies
# the session. A planned reconnect drill is recorded but never disqualifies.
MIN_SESSION_SECONDS = 6 * 3600 + 30 * 60
BROKER_OPTIONS_ENVIRONMENTS = ("BACKTEST", "LIVE", "PAPER")

BASE_KEYS = {
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
    "release_id",
    "environment",
    "attributes",
    "reviewer_key_id",
    "reviewer_signature",
    "prev_hash",
    "record_hash",
}
SESSION_ATTRIBUTES = (
    "session_seconds",
    "orders_submitted",
    "reconciliations_completed",
    "unknown_orders_at_close",
    "reconciliation_discrepancies",
    "unexplained_incidents",
    "unplanned_reconnects",
    "planned_reconnect_drills",
)

# Why a well-formed record is not counted, in the order they are decided.
UNAUTHENTICATED = "UNAUTHENTICATED"
ARTIFACT_UNVERIFIED = "ARTIFACT_UNVERIFIED"
OTHER_RELEASE = "OTHER_RELEASE"
CRITERIA_NOT_MET = "CRITERIA_NOT_MET"


class EvidenceError(ValueError):
    """Raised when an external record cannot be trusted."""


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("utf-8")


def unsigned_body(record: dict[str, Any]) -> bytes:
    """What the reviewer signs: every field but the signature and the hash of both."""
    return canonical_json(
        {key: value for key, value in record.items() if key not in ("record_hash", "reviewer_signature")}
    )


def signature_message(record: dict[str, Any]) -> bytes:
    return SIGNATURE_DOMAIN + unsigned_body(record)


def record_hash(record: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_json({key: value for key, value in record.items() if key != "record_hash"})).hexdigest()


def sign_record(record: dict[str, Any], seed: bytes) -> str:
    return ed25519.sign(seed, signature_message(record)).hex()


def validate_timestamp(value: object) -> None:
    # Exactly YYYY-MM-DDTHH:MM:SSZ. `fromisoformat` alone also accepted a
    # space for the `T` and ISO week dates, both the same length (E6.2).
    if not isinstance(value, str) or CANONICAL_UTC.fullmatch(value) is None:
        raise EvidenceError("occurred_at must use second-precision YYYY-MM-DDTHH:MM:SSZ")
    try:
        datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as error:
        raise EvidenceError("occurred_at is not a real UTC time") from error


def is_count(value: object) -> bool:
    # `type` rather than `isinstance`: JSON `true` is a `bool`, which is an `int`.
    return type(value) is int and value >= 0


def validate_attributes(evidence_type: str, attributes: object) -> None:
    """Refuses attributes that are not exactly the shape the evidence type states."""
    if not isinstance(attributes, dict):
        raise EvidenceError("attributes must be an object")
    if evidence_type in SESSION_ENVIRONMENTS:
        expected = set(SESSION_ATTRIBUTES)
    elif evidence_type == "design_partner":
        expected = {"workflows_completed", "unaided"}
    elif evidence_type == "broker_options":
        expected = {"reconciled_environments"}
    else:
        expected = {"customer_kind", "subscription_id"}
    if set(attributes) != expected:
        raise EvidenceError(f"{evidence_type} attributes must be exactly {sorted(expected)}")
    if evidence_type in SESSION_ENVIRONMENTS:
        if not all(is_count(attributes[key]) for key in SESSION_ATTRIBUTES):
            raise EvidenceError("session attributes must be non-negative integers")
    elif evidence_type == "design_partner":
        if not is_count(attributes["workflows_completed"]) or type(attributes["unaided"]) is not bool:
            raise EvidenceError("design partner attributes are malformed")
    elif evidence_type == "broker_options":
        environments = attributes["reconciled_environments"]
        if (
            not isinstance(environments, list)
            or environments != sorted(set(environments))
            or not set(environments) <= set(BROKER_OPTIONS_ENVIRONMENTS)
        ):
            raise EvidenceError("reconciled_environments must be a sorted set of BACKTEST, LIVE and PAPER")
    else:
        subscription = attributes["subscription_id"]
        if attributes["customer_kind"] not in CUSTOMER_KINDS:
            raise EvidenceError("customer_kind must be professional or organisation")
        if not isinstance(subscription, str) or CANONICAL_ID.fullmatch(subscription) is None:
            raise EvidenceError("subscription_id is not a canonical ID")


def criteria_shortfalls(record: dict[str, Any]) -> list[str]:
    """What an accepted record's attributes fail to meet. Empty means it qualifies."""
    evidence_type = record["evidence_type"]
    attributes = record["attributes"]
    shortfalls: list[str] = []
    if evidence_type in SESSION_ENVIRONMENTS:
        if attributes["session_seconds"] < MIN_SESSION_SECONDS:
            shortfalls.append("the session was shorter than a regular trading session")
        if attributes["orders_submitted"] < 1:
            shortfalls.append("no order was submitted")
        if attributes["reconciliations_completed"] < 1:
            shortfalls.append("no reconciliation was completed")
        for key, message in (
            ("unknown_orders_at_close", "an order was still UNKNOWN at close"),
            ("reconciliation_discrepancies", "a reconciliation discrepancy was recorded"),
            ("unexplained_incidents", "an incident was left unexplained"),
            ("unplanned_reconnects", "the broker session reconnected without a plan"),
        ):
            if attributes[key] != 0:
                shortfalls.append(message)
    elif evidence_type == "design_partner":
        if attributes["workflows_completed"] < 1 or attributes["unaided"] is not True:
            shortfalls.append("no workflow was completed unaided")
    elif evidence_type == "broker_options":
        if attributes["reconciled_environments"] != list(BROKER_OPTIONS_ENVIRONMENTS):
            shortfalls.append("the export was not reconciled across BACKTEST, PAPER and LIVE")
    return shortfalls


def validate_record(record: object, expected_previous: str) -> dict[str, Any]:
    if not isinstance(record, dict) or set(record) != BASE_KEYS:
        raise EvidenceError("record keys do not match the v2 evidence contract")
    version = record["acceptance_evidence_schema_version"]
    # `type` rather than `isinstance`: JSON `true` is a `bool`, which is an
    # `int` equal to 1, and passed an equality check (E6.2).
    if type(version) is not int or version != SCHEMA_VERSION:
        raise EvidenceError("unsupported evidence schema version")
    for key in ("evidence_id", "subject_id", "observed_by", "reviewed_by", "release_id", "reviewer_key_id"):
        value = record[key]
        if not isinstance(value, str) or CANONICAL_ID.fullmatch(value) is None:
            raise EvidenceError(f"{key} is not a canonical ID")
    if record["observed_by"] == record["reviewed_by"]:
        raise EvidenceError("observed_by and reviewed_by must be distinct")
    evidence_type = record["evidence_type"]
    if evidence_type not in GATES:
        raise EvidenceError("unknown evidence_type")
    if record["outcome"] not in {"accepted", "rejected"}:
        raise EvidenceError("outcome must be accepted or rejected")
    if not isinstance(record["notes"], str) or len(record["notes"]) > 1024 or "\n" in record["notes"]:
        raise EvidenceError("notes must be a concise single line")
    if record["environment"] != SESSION_ENVIRONMENTS.get(evidence_type):
        raise EvidenceError("environment must be PAPER or LIVE for a session and null for any other evidence")
    validate_timestamp(record["occurred_at"])
    validate_attributes(evidence_type, record["attributes"])
    for key in ("source_artifact_sha256", "prev_hash", "record_hash"):
        if not isinstance(record[key], str) or SHA256.fullmatch(record[key]) is None:
            raise EvidenceError(f"{key} is not lowercase SHA-256")
    if not isinstance(record["reviewer_signature"], str) or SIGNATURE.fullmatch(record["reviewer_signature"]) is None:
        raise EvidenceError("reviewer_signature is not a lowercase Ed25519 signature")
    if record["prev_hash"] != expected_previous:
        raise EvidenceError("evidence hash chain is discontinuous")
    if record_hash(record) != record["record_hash"]:
        raise EvidenceError("evidence record hash does not match canonical content")
    return record


@dataclass(frozen=True)
class TrustedReviewers:
    """The reviewer key set that decides whose signature counts."""

    keys: dict[str, tuple[str, bytes]]  # key_id -> (reviewer_id, public key)
    sha256: str

    def authenticates(self, record: dict[str, Any]) -> bool:
        entry = self.keys.get(record["reviewer_key_id"])
        if entry is None or entry[0] != record["reviewed_by"]:
            return False
        return ed25519.verify(entry[1], signature_message(record), bytes.fromhex(record["reviewer_signature"]))


def load_trusted_reviewers(path: Path) -> TrustedReviewers:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_LINE_BYTES:
        raise EvidenceError("trusted reviewer file is missing, linked or too large")
    data = path.read_bytes()
    try:
        document = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvidenceError("trusted reviewer file is not JSON") from error
    if (
        not isinstance(document, dict)
        or set(document) != {"trusted_reviewers_schema_version", "reviewers"}
        or type(document["trusted_reviewers_schema_version"]) is not int
        or document["trusted_reviewers_schema_version"] != 1
        or not isinstance(document["reviewers"], list)
    ):
        raise EvidenceError("trusted reviewer file does not match its contract")
    keys: dict[str, tuple[str, bytes]] = {}
    for entry in document["reviewers"]:
        if not isinstance(entry, dict) or set(entry) != {"key_id", "reviewer_id", "public_key_hex"}:
            raise EvidenceError("a trusted reviewer entry does not match its contract")
        key_id, reviewer_id, public_hex = entry["key_id"], entry["reviewer_id"], entry["public_key_hex"]
        if (
            not isinstance(key_id, str)
            or CANONICAL_ID.fullmatch(key_id) is None
            or not isinstance(reviewer_id, str)
            or CANONICAL_ID.fullmatch(reviewer_id) is None
            or not isinstance(public_hex, str)
            or SHA256.fullmatch(public_hex) is None
        ):
            raise EvidenceError("a trusted reviewer entry is malformed")
        if key_id in keys:
            raise EvidenceError(f"duplicate trusted reviewer key: {key_id}")
        keys[key_id] = (reviewer_id, bytes.fromhex(public_hex))
    return TrustedReviewers(keys=keys, sha256=hashlib.sha256(data).hexdigest())


def artifact_is_retained(artifact_root: Path, digest: str) -> bool:
    """Whether the retained artifact named by `digest` exists and still hashes to it."""
    path = artifact_root / digest
    if path.is_symlink() or not path.is_file():
        return False
    return hashlib.sha256(path.read_bytes()).hexdigest() == digest


@dataclass(frozen=True)
class Verdict:
    """A well-formed record and the reasons, if any, it does not count."""

    record: dict[str, Any]
    reasons: tuple[str, ...]


def judge(
    record: dict[str, Any], trusted: TrustedReviewers, artifact_root: Path, release_id: str
) -> Verdict:
    reasons: list[str] = []
    if not trusted.authenticates(record):
        reasons.append(UNAUTHENTICATED)
    if not artifact_is_retained(artifact_root, record["source_artifact_sha256"]):
        reasons.append(ARTIFACT_UNVERIFIED)
    if record["release_id"] != release_id:
        reasons.append(OTHER_RELEASE)
    if record["outcome"] == "accepted" and criteria_shortfalls(record):
        reasons.append(CRITERIA_NOT_MET)
    return Verdict(record, tuple(reasons))


def read_ledger(path: Path) -> tuple[list[dict[str, Any]], bytes, str]:
    """Reads one ledger, verifying every record's shape, chain and hash."""
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_LEDGER_BYTES:
        raise EvidenceError(f"unsafe or oversized evidence ledger: {path.name}")
    data = path.read_bytes()
    if data and not data.endswith(b"\n"):
        raise EvidenceError(f"ledger must end with a complete newline: {path.name}")
    previous = ZERO_HASH
    records: list[dict[str, Any]] = []
    for line_number, line in enumerate(data.splitlines(), start=1):
        if not line or len(line) > MAX_LINE_BYTES:
            raise EvidenceError(f"invalid evidence line {path.name}:{line_number}")
        try:
            candidate = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise EvidenceError(f"invalid JSON at {path.name}:{line_number}") from error
        record = validate_record(candidate, previous)
        previous = record["record_hash"]
        records.append(record)
    return records, data, previous


def load_ledger_files(root: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Returns every well-formed record and, for each ledger file, what binds a
    status to exactly the bytes it counted: the file's path relative to the
    root, its SHA-256, its record count, and its chain head (E6.3)."""
    root = root.resolve(strict=True)
    if not root.is_dir():
        raise EvidenceError("evidence root must be a directory")
    records: list[dict[str, Any]] = []
    ledgers: list[dict[str, Any]] = []
    seen_ids: set[str] = set()
    for path in sorted(root.rglob("*.acceptance.ndjson")):
        ledger_records, data, head = read_ledger(path)
        for record in ledger_records:
            if record["evidence_id"] in seen_ids:
                raise EvidenceError(f"duplicate evidence_id: {record['evidence_id']}")
            seen_ids.add(record["evidence_id"])
        records.extend(ledger_records)
        ledgers.append({
            "path": path.relative_to(root).as_posix(),
            "sha256": hashlib.sha256(data).hexdigest(),
            "records": len(ledger_records),
            "head": head,
        })
    return records, ledgers


def load_ledgers(root: Path) -> list[dict[str, Any]]:
    return load_ledger_files(root)[0]


def customer_kind(record: dict[str, Any]) -> str | None:
    return record["attributes"]["customer_kind"] if record["evidence_type"] == "paying_customer" else None


def status(
    verdicts: list[Verdict],
    ledgers: list[dict[str, Any]],
    *,
    release_id: str,
    trusted_reviewers_sha256: str,
) -> dict[str, Any]:
    accepted_kinds: dict[str, dict[str, set[str]]] = defaultdict(lambda: defaultdict(set))
    rejected_subjects: dict[str, set[str]] = defaultdict(set)
    rejected = defaultdict(int)
    not_counted: dict[str, list[str]] = defaultdict(list)
    for verdict in verdicts:
        record = verdict.record
        evidence_type = record["evidence_type"]
        for reason in verdict.reasons:
            not_counted[reason].append(record["evidence_id"])
        if record["outcome"] == "rejected":
            # A rejection only ever lowers a count, so a forged one is the only
            # harm it can do: it counts only if its reviewer is trusted, and then
            # for its subject in any release, because a subject is one session.
            if UNAUTHENTICATED not in verdict.reasons:
                rejected_subjects[evidence_type].add(record["subject_id"])
                rejected[evidence_type] += 1
        elif not verdict.reasons:
            accepted_kinds[evidence_type][customer_kind(record) or ""].add(record["subject_id"])
    gates = {}
    for evidence_type, alternatives in GATES.items():
        # A rejection disqualifies its subject within its gate, whichever
        # record came first. The ledger has no correction record, so an
        # acceptance can neither outlive a later rejection nor overturn an
        # earlier one (E6.2).
        by_kind = {
            kind: subjects - rejected_subjects[evidence_type]
            for kind, subjects in accepted_kinds[evidence_type].items()
        }
        disqualified = {
            subject
            for subjects in accepted_kinds[evidence_type].values()
            for subject in subjects & rejected_subjects[evidence_type]
        }
        rows = []
        for kind, required in alternatives:
            observed = len(by_kind.get(kind or "", set()))
            rows.append({
                "customer_kind": kind,
                "observed": observed,
                "required": required,
                "remaining": max(0, required - observed),
                "eligible": observed >= required,
            })
        nearest = min(rows, key=lambda row: (row["remaining"], row["required"]))
        gates[evidence_type] = {
            "observed": sum(len(subjects) for subjects in by_kind.values()),
            "required": nearest["required"],
            "remaining": nearest["remaining"],
            "eligible": any(row["eligible"] for row in rows),
            "alternatives": rows,
            "rejected_records": rejected[evidence_type],
            "disqualified_subjects": len(disqualified),
        }
    return {
        "acceptance_status_schema_version": STATUS_SCHEMA_VERSION,
        "release_id": release_id,
        "trusted_reviewers_sha256": trusted_reviewers_sha256,
        "verified_records": len(verdicts),
        "counted_records": sum(1 for verdict in verdicts if not verdict.reasons),
        "not_counted": {reason: sorted(ids) for reason, ids in sorted(not_counted.items())},
        "all_gates_eligible": all(gate["eligible"] for gate in gates.values()),
        "gates": gates,
        "ledgers": ledgers,
    }


def audit(
    ledger_root: Path, trusted_reviewers: Path, artifact_root: Path, release_id: str
) -> dict[str, Any]:
    if CANONICAL_ID.fullmatch(release_id) is None:
        raise EvidenceError("release_id is not a canonical ID")
    trusted = load_trusted_reviewers(trusted_reviewers)
    artifact_root = artifact_root.resolve(strict=True)
    if not artifact_root.is_dir():
        raise EvidenceError("artifact root must be a directory")
    records, ledgers = load_ledger_files(ledger_root)
    verdicts = [judge(record, trusted, artifact_root, release_id) for record in records]
    return status(verdicts, ledgers, release_id=release_id, trusted_reviewers_sha256=trusted.sha256)


def artifact_digest(artifact: Path) -> str:
    if artifact.is_symlink() or not artifact.is_file():
        raise EvidenceError("the source artifact must be a regular file")
    return hashlib.sha256(artifact.read_bytes()).hexdigest()


def retain_artifact(artifact: Path, artifact_root: Path) -> str:
    """Stores `artifact` in the content-addressed root and returns its SHA-256."""
    digest = artifact_digest(artifact)
    content = artifact.read_bytes()
    artifact_root.mkdir(parents=True, exist_ok=True)
    target = artifact_root / digest
    if target.is_symlink():
        raise EvidenceError("the artifact root holds a link where an artifact belongs")
    if target.exists():
        if target.read_bytes() != content:
            raise EvidenceError("a different artifact already occupies this digest")
        return digest
    temporary = artifact_root / f".{digest}.partial"
    temporary.write_bytes(content)
    temporary.replace(target)
    return digest


TEMPLATE_KEYS = {
    "evidence_id", "evidence_type", "subject_id", "occurred_at", "observed_by", "reviewed_by",
    "outcome", "notes", "release_id", "environment", "attributes",
}


def append_record(
    ledger: Path,
    template: dict[str, Any],
    artifact: Path,
    artifact_root: Path,
    seed: bytes,
    reviewer_key_id: str,
) -> dict[str, Any]:
    """Signs and appends one record, and retains its artifact.

    The record is validated whole before anything is written, and the existing
    ledger is verified first, so a record is never appended to a broken chain.
    """
    if not ledger.name.endswith(".acceptance.ndjson"):
        raise EvidenceError("a ledger file is named *.acceptance.ndjson")
    if set(template) != TEMPLATE_KEYS:
        raise EvidenceError(f"the record template must have exactly {sorted(TEMPLATE_KEYS)}")
    previous = ZERO_HASH
    if ledger.exists():
        existing, _, previous = read_ledger(ledger)
        if any(record["evidence_id"] == template["evidence_id"] for record in existing):
            raise EvidenceError("this ledger already holds that evidence_id")
    elif ledger.is_symlink():
        raise EvidenceError("a ledger must not be a symbolic link")
    record: dict[str, Any] = {
        **template,
        "acceptance_evidence_schema_version": SCHEMA_VERSION,
        "source_artifact_sha256": artifact_digest(artifact),
        "reviewer_key_id": reviewer_key_id,
        "reviewer_signature": "0" * 128,
        "prev_hash": previous,
        "record_hash": ZERO_HASH,
    }
    record["reviewer_signature"] = sign_record(record, seed)
    record["record_hash"] = record_hash(record)
    validate_record(record, previous)
    retain_artifact(artifact, artifact_root)
    ledger.parent.mkdir(parents=True, exist_ok=True)
    with ledger.open("ab") as stream:
        stream.write(canonical_json(record) + b"\n")
    return record


def write_json(path: Path, value: object) -> None:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(encoded, encoding="utf-8", newline="\n")
    temporary.replace(path)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    audit_parser = commands.add_parser("audit", help="verify a ledger root and report gate counts")
    audit_parser.add_argument("ledger_root", type=Path)
    audit_parser.add_argument("--trusted-reviewers", type=Path, required=True)
    audit_parser.add_argument("--artifact-root", type=Path, required=True)
    audit_parser.add_argument("--release-id", required=True)
    audit_parser.add_argument("--output", type=Path)
    append_parser = commands.add_parser("append", help="sign and append one record")
    append_parser.add_argument("ledger", type=Path)
    append_parser.add_argument("--record", type=Path, required=True)
    append_parser.add_argument("--artifact", type=Path, required=True)
    append_parser.add_argument("--artifact-root", type=Path, required=True)
    append_parser.add_argument("--reviewer-key", type=Path, required=True)
    append_parser.add_argument("--reviewer-key-id", required=True)
    return parser


def read_reviewer_seed(path: Path) -> bytes:
    """The seed inside a reviewer's PKCS#8 signing key file, which must be a regular file."""
    if path.is_symlink() or not path.is_file():
        raise EvidenceError("the reviewer key must be a regular file")
    try:
        return ed25519.seed_from_pkcs8(path.read_bytes())
    except (ValueError, OSError) as error:
        raise EvidenceError(f"cannot read the reviewer key: {error}") from error


def main(argv: list[str] | None = None) -> int:
    arguments = build_parser().parse_args(argv)
    try:
        if arguments.command == "audit":
            report = audit(
                arguments.ledger_root, arguments.trusted_reviewers, arguments.artifact_root, arguments.release_id
            )
            encoded = json.dumps(report, sort_keys=True, separators=(",", ":")) + "\n"
            if arguments.output is None:
                sys.stdout.write(encoded)
            else:
                write_json(arguments.output, report)
            return 0
        seed = read_reviewer_seed(arguments.reviewer_key)
        try:
            template = json.loads(arguments.record.read_text(encoding="utf-8"))
        except (ValueError, OSError) as error:
            raise EvidenceError(f"cannot read the record template: {error}") from error
        if not isinstance(template, dict):
            raise EvidenceError("the record template must be an object")
        record = append_record(
            arguments.ledger, template, arguments.artifact, arguments.artifact_root, seed,
            arguments.reviewer_key_id,
        )
        print(f"appended {record['evidence_id']} ({record['record_hash']})")
        return 0
    except (EvidenceError, OSError) as error:
        print(f"acceptance evidence verification failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
