#!/usr/bin/env python3
"""Verify tamper-evident external acceptance ledgers and report real gate counts.

Schema 2 (delivery state E6.4 and E6.5) makes a record something a reviewer signed,
about a release, backed by an artifact, against criteria fixed before it counts:

* A record carries the reviewer's Ed25519 signature, under a key the trusted reviewer
  set lists for the reviewer the record names. Every record must be signed so: one
  that is not makes the audit fail, because a rejection edited into an acceptance, or
  a rejection whose reviewer was dropped from the set, would otherwise stop counting
  and quietly requalify its subject (E6.6b). A key is never removed from the set. One
  that may no longer be trusted is marked revoked: its acceptances stop counting and
  its rejections still disqualify.
* Its source artifact is re-hashed against a retained artifact root. An artifact that
  is missing or differs is a record that cannot be checked, so it does not count. One
  artifact backs one subject, and one subscription one customer: an artifact or a
  subscription that accepted records cite for two subjects counts for neither.
* It names the release and the environment it exercised, and counts only toward that
  release's gates.
* Its `attributes` state what the session, partner or customer actually was, and an
  acceptance counts only if they meet the criteria: what a "clean" session means, and
  how a reconnect, an unresolved `UNKNOWN` order or a discrepancy is treated. An
  acceptance that fails them disqualifies its subject, as a rejection does.

A record that is malformed, mis-chained, mis-hashed or unsigned makes the whole ledger
untrustworthy, so the audit fails. A record that is well formed but does not qualify
is listed with its reason and is never counted. Whatever a ledger or a reviewer set
holds, the audit refuses it with `EvidenceError` rather than raising anything else.

    acceptance_evidence.py audit LEDGER_ROOT --trusted-reviewers FILE --artifact-root DIR --release-id ID
    acceptance_evidence.py append LEDGER --ledger-root DIR --trusted-reviewers FILE --record TEMPLATE
                                          --artifact FILE --artifact-root DIR
                                          --reviewer-key KEY.pk8 --reviewer-key-id ID
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import tempfile
from collections import defaultdict
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

try:
    from tools import ed25519
except ImportError:  # run as a script: the tools directory itself is on sys.path
    import ed25519  # type: ignore[no-redef]

SCHEMA_VERSION = 2
STATUS_SCHEMA_VERSION = 4
# Version 1 sets had no key status, and every key they list is read as active.
TRUSTED_REVIEWERS_SCHEMA_VERSIONS = (1, 2)
KEY_STATUSES = ("active", "revoked")
SIGNATURE_DOMAIN = b"follon-acceptance-evidence-v2\n"
ZERO_HASH = "0" * 64
LEDGER_SUFFIX = ".acceptance.ndjson"
MAX_LEDGER_BYTES = 64 * 1024 * 1024
MAX_LINE_BYTES = 1024 * 1024
MAX_REVIEWER_SET_BYTES = 1024 * 1024
MAX_ARTIFACT_BYTES = 256 * 1024 * 1024
APPEND_LOCK_NAME = ".append.lock"
MAX_NOTES_CHARACTERS = 1024
OUTCOMES = ("accepted", "rejected")
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

# Why an accepted record does not count, in the order they are decided. A record no
# listed key signed is not among them: it fails the audit.
REVIEWER_REVOKED = "REVIEWER_REVOKED"
ARTIFACT_UNVERIFIED = "ARTIFACT_UNVERIFIED"
OTHER_RELEASE = "OTHER_RELEASE"
CRITERIA_NOT_MET = "CRITERIA_NOT_MET"
DISQUALIFIED = "DISQUALIFIED"
ARTIFACT_SHARED = "ARTIFACT_SHARED"
SUBSCRIPTION_SHARED = "SUBSCRIPTION_SHARED"
CUSTOMER_KIND_CONFLICT = "CUSTOMER_KIND_CONFLICT"


class EvidenceError(ValueError):
    """Raised when an external record cannot be trusted."""


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("utf-8")


def parse_json(data: bytes, refusal: str) -> Any:
    """The JSON value `data` holds, or `EvidenceError(refusal)`.

    `json` raises more than `JSONDecodeError`: a digit string longer than Python's
    integer limit raises a plain `ValueError`, and deep nesting a `RecursionError`.
    Each of those escaped the audit as a traceback.
    """
    try:
        return json.loads(data)
    except (ValueError, RecursionError) as error:
        raise EvidenceError(refusal) from error


def read_regular_file(path: Path, limit: int, what: str) -> bytes:
    """The bytes of `path`, which must be a regular file and not a link, of at most `limit` bytes.

    It reads at most one byte past the limit, so a file that grows after it is opened
    is refused rather than read whole.
    """
    if path.is_symlink() or not path.is_file():
        raise EvidenceError(f"{what} is missing, linked or not a regular file")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise EvidenceError(f"{what} is larger than {limit} bytes")
    return data


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
        # Every item is checked to be a string first: sorting a list that mixes types,
        # or making a set of one that holds a list, raised `TypeError`.
        if (
            not isinstance(environments, list)
            or not all(isinstance(environment, str) for environment in environments)
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
    # A list or an object is unhashable, and looking one up in a dict or a set raised
    # `TypeError`. The type is checked before the dict lookup, and the outcomes are a
    # tuple, whose membership test compares rather than hashes.
    if not isinstance(evidence_type, str) or evidence_type not in GATES:
        raise EvidenceError("unknown evidence_type")
    if record["outcome"] not in OUTCOMES:
        raise EvidenceError("outcome must be accepted or rejected")
    notes = record["notes"]
    if not isinstance(notes, str) or len(notes) > MAX_NOTES_CHARACTERS or "\n" in notes:
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
class ReviewerKey:
    reviewer_id: str
    public_key: bytes
    status: str


@dataclass(frozen=True)
class TrustedReviewers:
    """The reviewer key set that decides whose signature counts."""

    keys: dict[str, ReviewerKey]  # by key id
    sha256: str

    def signer_status(self, record: dict[str, Any]) -> str | None:
        """The status of the listed key that signed `record`, or None when no listed key did:
        the key id is not listed, is bound to another reviewer, or did not make the signature."""
        key = self.keys.get(record["reviewer_key_id"])
        if key is None or key.reviewer_id != record["reviewed_by"]:
            return None
        signature = bytes.fromhex(record["reviewer_signature"])
        if not ed25519.verify(key.public_key, signature_message(record), signature):
            return None
        return key.status


def load_trusted_reviewers(path: Path) -> TrustedReviewers:
    """Reads a trusted reviewer set, refusing any key that is not an honest Ed25519 public key.

    A key of small order, the all-zero placeholder among them, lets anyone sign anything
    under RFC 8032 alone. Verification refuses one since audit item 121, and enrolling
    one is refused here, so a set that holds one fails loudly instead of trusting no one
    it seems to (E6.6b). One public key under two entries is refused too: it would let
    one key holder sign as two reviewers.
    """
    data = read_regular_file(path, MAX_REVIEWER_SET_BYTES, "the trusted reviewer file")
    document = parse_json(data, "trusted reviewer file is not JSON")
    if (
        not isinstance(document, dict)
        or set(document) != {"trusted_reviewers_schema_version", "reviewers"}
        or type(document["trusted_reviewers_schema_version"]) is not int
        or document["trusted_reviewers_schema_version"] not in TRUSTED_REVIEWERS_SCHEMA_VERSIONS
        or not isinstance(document["reviewers"], list)
    ):
        raise EvidenceError("trusted reviewer file does not match its contract")
    entry_keys = {"key_id", "reviewer_id", "public_key_hex"}
    if document["trusted_reviewers_schema_version"] == 2:
        entry_keys.add("status")
    keys: dict[str, ReviewerKey] = {}
    enrolled: set[bytes] = set()
    for entry in document["reviewers"]:
        if not isinstance(entry, dict) or set(entry) != entry_keys:
            raise EvidenceError("a trusted reviewer entry does not match its contract")
        key_id, reviewer_id, public_hex = entry["key_id"], entry["reviewer_id"], entry["public_key_hex"]
        status = entry.get("status", "active")
        if (
            not isinstance(key_id, str)
            or CANONICAL_ID.fullmatch(key_id) is None
            or not isinstance(reviewer_id, str)
            or CANONICAL_ID.fullmatch(reviewer_id) is None
            or not isinstance(public_hex, str)
            or SHA256.fullmatch(public_hex) is None
            or status not in KEY_STATUSES
        ):
            raise EvidenceError("a trusted reviewer entry is malformed")
        if key_id in keys:
            raise EvidenceError(f"duplicate trusted reviewer key: {key_id}")
        public_key = bytes.fromhex(public_hex)
        if not ed25519.is_valid_public_key(public_key):
            raise EvidenceError(f"trusted reviewer key {key_id} is not a valid Ed25519 public key")
        if public_key in enrolled:
            raise EvidenceError(f"trusted reviewer key {key_id} repeats the public key of another entry")
        enrolled.add(public_key)
        keys[key_id] = ReviewerKey(reviewer_id, public_key, status)
    return TrustedReviewers(keys=keys, sha256=hashlib.sha256(data).hexdigest())


def artifact_is_retained(artifact_root: Path, digest: str) -> bool:
    """Whether the retained artifact named by `digest` exists and still hashes to it."""
    path = artifact_root / digest
    if path.is_symlink() or not path.is_file():
        return False
    return hashlib.sha256(path.read_bytes()).hexdigest() == digest


@dataclass(frozen=True)
class Verdict:
    """A signed record and, for an acceptance, the reasons it does not count. A rejection
    never counts toward a gate, so it has no reasons: it disqualifies its subject."""

    record: dict[str, Any]
    reasons: tuple[str, ...]


def subject_of(record: dict[str, Any]) -> tuple[str, str]:
    """A subject is one session, partner, export or customer, within its evidence type."""
    return record["evidence_type"], record["subject_id"]


def is_disqualifying(record: dict[str, Any]) -> bool:
    """A rejection, or an acceptance whose own attributes fail the criteria.

    Either is a signed statement that the subject did not qualify, so it disqualifies the
    subject whatever else is recorded of it, in any release and whether or not its artifact
    is retained. Before E6.6b a failing acceptance only failed to count, and a clean one
    after it counted the subject.
    """
    return record["outcome"] == "rejected" or bool(criteria_shortfalls(record))


def require_signed(records: list[dict[str, Any]], trusted: TrustedReviewers) -> dict[str, str]:
    """The status of the listed key that signed each record, by evidence id. Refuses records
    that no listed key signed, naming the first five and counting the rest."""
    signers = {record["evidence_id"]: trusted.signer_status(record) for record in records}
    unsigned = [evidence_id for evidence_id, status in signers.items() if status is None]
    if unsigned:
        shown = ", ".join(unsigned[:5]) + (f" and {len(unsigned) - 5} more" if len(unsigned) > 5 else "")
        raise EvidenceError(f"records that no listed reviewer key signed: {shown}")
    return {evidence_id: status for evidence_id, status in signers.items() if status is not None}


def assess(
    records: list[dict[str, Any]], trusted: TrustedReviewers, artifact_root: Path, release_id: str
) -> list[Verdict]:
    """Judges every record of a ledger root. Refuses a root holding a record no listed key signed."""
    signers = require_signed(records, trusted)
    disqualified = {subject_of(record) for record in records if is_disqualifying(record)}
    # What each accepted record cites, so a citation shared between subjects is visible.
    # Only acceptances are read: a rejection lowers a count however it is backed.
    artifact_subjects: dict[str, set[tuple[str, str]]] = defaultdict(set)
    subscription_subjects: dict[str, set[str]] = defaultdict(set)
    customer_kinds: dict[str, set[str]] = defaultdict(set)
    accepted = [record for record in records if record["outcome"] == "accepted"]
    for record in accepted:
        artifact_subjects[record["source_artifact_sha256"]].add(subject_of(record))
        if record["evidence_type"] == "paying_customer":
            subscription_subjects[record["attributes"]["subscription_id"]].add(record["subject_id"])
            customer_kinds[record["subject_id"]].add(record["attributes"]["customer_kind"])
    verdicts = []
    for record in records:
        reasons: list[str] = []
        if record["outcome"] == "accepted":
            if signers[record["evidence_id"]] == "revoked":
                reasons.append(REVIEWER_REVOKED)
            if not artifact_is_retained(artifact_root, record["source_artifact_sha256"]):
                reasons.append(ARTIFACT_UNVERIFIED)
            if record["release_id"] != release_id:
                reasons.append(OTHER_RELEASE)
            if criteria_shortfalls(record):
                reasons.append(CRITERIA_NOT_MET)
            if subject_of(record) in disqualified:
                reasons.append(DISQUALIFIED)
            if len(artifact_subjects[record["source_artifact_sha256"]]) > 1:
                reasons.append(ARTIFACT_SHARED)
            if record["evidence_type"] == "paying_customer":
                if len(subscription_subjects[record["attributes"]["subscription_id"]]) > 1:
                    reasons.append(SUBSCRIPTION_SHARED)
                if len(customer_kinds[record["subject_id"]]) > 1:
                    reasons.append(CUSTOMER_KIND_CONFLICT)
        verdicts.append(Verdict(record, tuple(reasons)))
    return verdicts


def parse_ledger(data: bytes, name: str) -> tuple[list[dict[str, Any]], str]:
    """Verifies one ledger's bytes, every record's shape, chain and hash, and returns
    its records and its chain head. `name` only labels a refusal.

    Each line must be exactly its record's canonical JSON, the bytes `append` writes.
    Otherwise one record could be written many ways, and a line whose raw bytes say
    one thing could parse to another: JSON keeps the last of two duplicate keys.
    """
    if data and not data.endswith(b"\n"):
        raise EvidenceError(f"ledger must end with a complete newline: {name}")
    previous = ZERO_HASH
    records: list[dict[str, Any]] = []
    # Split on the newline alone. `bytes.splitlines` also splits on a carriage return.
    lines = data[:-1].split(b"\n") if data else []
    for line_number, line in enumerate(lines, start=1):
        if not line or len(line) > MAX_LINE_BYTES:
            raise EvidenceError(f"invalid evidence line {name}:{line_number}")
        candidate = parse_json(line, f"invalid JSON at {name}:{line_number}")
        record = validate_record(candidate, previous)
        if canonical_json(record) != line:
            raise EvidenceError(f"evidence line {name}:{line_number} is not its record's canonical JSON")
        previous = record["record_hash"]
        records.append(record)
    return records, previous


def read_ledger(path: Path) -> tuple[list[dict[str, Any]], bytes, str]:
    """Reads and verifies one ledger file: its records, its exact bytes and its chain head."""
    data = read_regular_file(path, MAX_LEDGER_BYTES, f"evidence ledger {path.name}")
    records, head = parse_ledger(data, path.name)
    return records, data, head


def ledger_paths(root: Path) -> list[Path]:
    """Every ledger file under `root`, ordered by its relative path the same way on every platform.

    A link or junction anywhere under the root is refused. Followed, a linked directory
    would count ledgers kept outside the root, and nothing a link points at can be
    held to the root's own custody. The suffix is matched exactly: a glob matched it
    case-insensitively on Windows and not on Linux.
    """
    found: list[Path] = []
    for directory, subdirectories, files in os.walk(root, followlinks=False):
        for name in (*subdirectories, *files):
            candidate = Path(directory) / name
            if candidate.is_symlink() or candidate.is_junction():
                raise EvidenceError(f"the ledger root holds a link: {candidate.relative_to(root).as_posix()}")
        found.extend(Path(directory) / name for name in files if name.endswith(LEDGER_SUFFIX))
    return sorted(found, key=lambda path: path.relative_to(root).as_posix())


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
    for path in ledger_paths(root):
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
    """Gate counts from the verdicts. Schema 4 (E6.6b): `counted_records` counts only
    acceptances that count, `disqualified_subjects` every subject a rejection or a failing
    acceptance disqualified, and `not_counted` lists acceptances only."""
    by_kind_of: dict[str, dict[str, set[str]]] = defaultdict(lambda: defaultdict(set))
    disqualified: dict[str, set[str]] = defaultdict(set)
    rejected = defaultdict(int)
    not_counted: dict[str, list[str]] = defaultdict(list)
    for verdict in verdicts:
        record = verdict.record
        evidence_type = record["evidence_type"]
        for reason in verdict.reasons:
            not_counted[reason].append(record["evidence_id"])
        # A disqualification holds within its gate whichever record came first. The
        # ledger has no correction record, so an acceptance can neither outlive a
        # later rejection nor overturn an earlier one (E6.2).
        if is_disqualifying(record):
            disqualified[evidence_type].add(record["subject_id"])
        if record["outcome"] == "rejected":
            rejected[evidence_type] += 1
        elif not verdict.reasons:
            by_kind_of[evidence_type][customer_kind(record) or ""].add(record["subject_id"])
    gates = {}
    for evidence_type, alternatives in GATES.items():
        by_kind = by_kind_of[evidence_type]
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
            "disqualified_subjects": len(disqualified[evidence_type]),
        }
    return {
        "acceptance_status_schema_version": STATUS_SCHEMA_VERSION,
        "release_id": release_id,
        "trusted_reviewers_sha256": trusted_reviewers_sha256,
        "verified_records": len(verdicts),
        "counted_records": sum(
            1 for verdict in verdicts if verdict.record["outcome"] == "accepted" and not verdict.reasons
        ),
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
    verdicts = assess(records, trusted, artifact_root, release_id)
    return status(verdicts, ledgers, release_id=release_id, trusted_reviewers_sha256=trusted.sha256)


def read_artifact(artifact: Path) -> bytes:
    """The bytes of a source artifact, which must be a regular file and not a link."""
    return read_regular_file(artifact, MAX_ARTIFACT_BYTES, "the source artifact")


def retain_artifact(content: bytes, artifact_root: Path) -> str:
    """Stores `content` in the content-addressed root under its SHA-256, which it returns.

    It is written to a temporary file of a fresh random name, created exclusively, and
    then renamed into place. A fixed temporary name let a link planted under it make
    retention write through the link and overwrite its target (E6.6b).
    """
    if artifact_root.is_symlink():
        raise EvidenceError("the artifact root must not be a link")
    digest = hashlib.sha256(content).hexdigest()
    artifact_root.mkdir(parents=True, exist_ok=True)
    target = artifact_root / digest
    if target.is_symlink():
        raise EvidenceError("the artifact root holds a link where an artifact belongs")
    if target.exists():
        if target.read_bytes() != content:
            raise EvidenceError("a different artifact already occupies this digest")
        return digest
    descriptor, temporary = tempfile.mkstemp(dir=artifact_root, prefix=f".{digest}.", suffix=".partial")
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, target)
    except BaseException:
        Path(temporary).unlink(missing_ok=True)
        raise
    return digest


@contextmanager
def exclusive_append(ledger_root: Path) -> Iterator[None]:
    """Holds a ledger root's append lock, a file created exclusively, for one append.

    Two appends that overlapped both chained to the same head, and the second broke the
    chain. The lock is a file so that it works alike on every platform, and an append
    that dies holding it leaves it visible rather than silently expired.
    """
    lock = ledger_root / APPEND_LOCK_NAME
    try:
        descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError as error:
        raise EvidenceError(
            f"another append holds the ledger root's lock, {lock}; remove it only once no append is running"
        ) from error
    os.close(descriptor)
    try:
        yield
    finally:
        lock.unlink()


TEMPLATE_KEYS = {
    "evidence_id", "evidence_type", "subject_id", "occurred_at", "observed_by", "reviewed_by",
    "outcome", "notes", "release_id", "environment", "attributes",
}


def append_record(
    ledger: Path,
    template: dict[str, Any],
    artifact: Path,
    *,
    ledger_root: Path,
    artifact_root: Path,
    trusted: TrustedReviewers,
    seed: bytes,
    reviewer_key_id: str,
) -> dict[str, Any]:
    """Signs and appends one record to a ledger under `ledger_root`, and retains its artifact.

    The record is validated whole before anything is written, and under the root's append
    lock (E6.6b):

    * the whole root is verified first, every record signed by a listed key included, so a
      record is never appended to a broken root;
    * its `evidence_id` must be new to the root, not only to its own ledger, or the next
      audit would refuse the root;
    * it must be signed by a key `trusted` lists as active for the reviewer it names, so a
      mistyped key id, another reviewer's key or a revoked one cannot append a record that
      every later audit would refuse;
    * the artifact is read once, and the bytes hashed into the record are the bytes retained.
    """
    if not ledger.name.endswith(LEDGER_SUFFIX):
        raise EvidenceError(f"a ledger file is named *{LEDGER_SUFFIX}")
    if set(template) != TEMPLATE_KEYS:
        raise EvidenceError(f"the record template must have exactly {sorted(TEMPLATE_KEYS)}")
    root = ledger_root.resolve(strict=True)
    if not root.is_dir():
        raise EvidenceError("the ledger root must be a directory")
    try:
        ledger_path = ledger.resolve().relative_to(root).as_posix()
    except ValueError as error:
        raise EvidenceError("the ledger must lie inside the ledger root") from error
    content = read_artifact(artifact)
    with exclusive_append(root):
        records, ledgers = load_ledger_files(root)
        require_signed(records, trusted)
        if any(record["evidence_id"] == template["evidence_id"] for record in records):
            raise EvidenceError("the ledger root already holds that evidence_id")
        previous = next((entry["head"] for entry in ledgers if entry["path"] == ledger_path), ZERO_HASH)
        record: dict[str, Any] = {
            **template,
            "acceptance_evidence_schema_version": SCHEMA_VERSION,
            "source_artifact_sha256": hashlib.sha256(content).hexdigest(),
            "reviewer_key_id": reviewer_key_id,
            "reviewer_signature": "0" * 128,
            "prev_hash": previous,
            "record_hash": ZERO_HASH,
        }
        record["reviewer_signature"] = sign_record(record, seed)
        record["record_hash"] = record_hash(record)
        validate_record(record, previous)
        if trusted.signer_status(record) != "active":
            raise EvidenceError(
                f"the signing key is not {reviewer_key_id}, listed as active for {record['reviewed_by']}"
            )
        retain_artifact(content, artifact_root)
        target = root / ledger_path
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("ab") as stream:
            stream.write(canonical_json(record) + b"\n")
            stream.flush()
            os.fsync(stream.fileno())
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
    append_parser.add_argument("--ledger-root", type=Path, required=True)
    append_parser.add_argument("--trusted-reviewers", type=Path, required=True)
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
            template_bytes = arguments.record.read_bytes()
        except OSError as error:
            raise EvidenceError(f"cannot read the record template: {error}") from error
        template = parse_json(template_bytes, "cannot read the record template: it is not JSON")
        if not isinstance(template, dict):
            raise EvidenceError("the record template must be an object")
        record = append_record(
            arguments.ledger,
            template,
            arguments.artifact,
            ledger_root=arguments.ledger_root,
            artifact_root=arguments.artifact_root,
            trusted=load_trusted_reviewers(arguments.trusted_reviewers),
            seed=seed,
            reviewer_key_id=arguments.reviewer_key_id,
        )
        print(f"appended {record['evidence_id']} ({record['record_hash']})")
        return 0
    except (EvidenceError, OSError) as error:
        print(f"acceptance evidence verification failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
