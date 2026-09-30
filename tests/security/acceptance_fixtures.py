"""Unit fixtures for the acceptance evidence tests.

Everything here is built in a temporary directory from a fixed test seed. None of it
is operational evidence, and the seed is a test value that must never be used as a key.

Each subject has an artifact and, as a customer, a subscription of its own, because one
artifact or one subscription backing two subjects counts for neither (E6.6b).
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

from tools import ed25519
from tools.acceptance_evidence import (
    MIN_SESSION_SECONDS,
    SCHEMA_VERSION,
    ZERO_HASH,
    audit,
    record_hash,
    sign_record,
)

RELEASE_ID = "release.unit.001"
REVIEWER_ID = "reviewer.two"
REVIEWER_KEY_ID = "reviewer.key.two.001"
REVIEWER_SEED = bytes(range(1, 33))
OTHER_SEED = bytes(range(101, 133))

SESSION_TYPES = ("paper_session", "live_session")


def artifact_for(subject_id: str) -> bytes:
    """The unit fixture artifact a reviewer examined for one subject."""
    return f"unit fixture artifact for {subject_id}: not operational evidence".encode("utf-8")


def artifact_sha256(subject_id: str) -> str:
    return hashlib.sha256(artifact_for(subject_id)).hexdigest()


def attributes_for(
    evidence_type: str, customer_kind: str = "professional", subject_id: str = "subject.unit"
) -> dict[str, object]:
    """The attributes of the smallest record of its type that meets the criteria."""
    if evidence_type in SESSION_TYPES:
        return {
            "session_seconds": MIN_SESSION_SECONDS,
            "orders_submitted": 3,
            "reconciliations_completed": 1,
            "unknown_orders_at_close": 0,
            "reconciliation_discrepancies": 0,
            "unexplained_incidents": 0,
            "unplanned_reconnects": 0,
            "planned_reconnect_drills": 1,
        }
    if evidence_type == "design_partner":
        return {"workflows_completed": 2, "unaided": True}
    if evidence_type == "broker_options":
        return {"reconciled_environments": ["BACKTEST", "LIVE", "PAPER"]}
    return {"customer_kind": customer_kind, "subscription_id": f"subscription.{subject_id}"}


def make_record(
    previous: str,
    evidence_id: str,
    subject_id: str,
    *,
    evidence_type: str = "paper_session",
    outcome: str = "accepted",
    seed: bytes = REVIEWER_SEED,
    attributes: dict[str, object] | None = None,
    customer_kind: str = "professional",
    **overrides: object,
) -> dict[str, object]:
    record: dict[str, object] = {
        "acceptance_evidence_schema_version": SCHEMA_VERSION,
        "evidence_id": evidence_id,
        "evidence_type": evidence_type,
        "subject_id": subject_id,
        "occurred_at": "2026-08-24T10:00:00Z",
        "observed_by": "operator.one",
        "reviewed_by": REVIEWER_ID,
        "source_artifact_sha256": artifact_sha256(subject_id),
        "outcome": outcome,
        "notes": "Independently reviewed unit fixture.",
        "release_id": RELEASE_ID,
        "environment": {"paper_session": "PAPER", "live_session": "LIVE"}.get(evidence_type),
        "attributes": attributes if attributes is not None else attributes_for(evidence_type, customer_kind, subject_id),
        "reviewer_key_id": REVIEWER_KEY_ID,
        "reviewer_signature": "0" * 128,
        "prev_hash": previous,
        "record_hash": ZERO_HASH,
    }
    record.update(overrides)
    if "reviewer_signature" not in overrides:
        record["reviewer_signature"] = sign_record(record, seed)
    record["record_hash"] = record_hash(record)
    return record


def resign(record: dict[str, object], **changes: object) -> dict[str, object]:
    """Changes fields of a signed record and recomputes only its hash, as a forger
    who can rewrite the ledger but does not hold the reviewer's key would."""
    forged = {**record, **changes}
    forged["record_hash"] = record_hash(forged)
    return forged


def make_chain(*specifications: tuple[str, str, dict[str, object]]) -> list[dict[str, object]]:
    """Chains records in order; each specification is (evidence_id, subject_id, keywords)."""
    records: list[dict[str, object]] = []
    previous = ZERO_HASH
    for evidence_id, subject_id, keywords in specifications:
        record = make_record(previous, evidence_id, subject_id, **keywords)
        records.append(record)
        previous = str(record["record_hash"])
    return records


def write_ledger(path: Path, records: list[dict[str, object]]) -> None:
    path.write_text(
        "".join(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n" for record in records),
        encoding="utf-8",
        newline="\n",
    )


def reviewers_document(*entries: tuple[str, ...]) -> dict[str, object]:
    """A version 2 trusted reviewer set from (key id, reviewer id, seed[, status]) entries.
    An entry without a status is active."""
    reviewers = []
    for entry in entries:
        key_id, reviewer_id, seed = entry[:3]
        status = entry[3] if len(entry) > 3 else "active"
        reviewers.append({
            "key_id": key_id,
            "reviewer_id": reviewer_id,
            "public_key_hex": ed25519.public_key(seed).hex(),
            "status": status,
        })
    return {"trusted_reviewers_schema_version": 2, "reviewers": reviewers}


def pkcs8(seed: bytes) -> bytes:
    """The PKCS#8 document `follon-admin release-keygen` writes for a seed."""
    return bytes.fromhex("302e020100300506032b657004220420") + seed


class Workspace:
    """A ledger root, a retained artifact root and a trusted reviewer set, in one directory.

    Writing a ledger retains the artifact of every subject it names, unless the workspace
    was made with `retain=False`.
    """

    def __init__(self, directory: str, *, trust: bool = True, retain: bool = True) -> None:
        root = Path(directory)
        self.ledgers = root / "ledgers"
        self.artifacts = root / "artifacts"
        self.reviewers = root / "reviewers.json"
        self.retain_artifacts = retain
        self.ledgers.mkdir()
        self.artifacts.mkdir()
        entries = [(REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED)] if trust else []
        self.trust(*entries)

    def trust(self, *entries: tuple[str, ...]) -> None:
        """Replaces the trusted reviewer set with `entries`, as `reviewers_document` takes them."""
        self.reviewers.write_text(json.dumps(reviewers_document(*entries)), encoding="utf-8")

    def retain(self, content: bytes) -> str:
        digest = hashlib.sha256(content).hexdigest()
        (self.artifacts / digest).write_bytes(content)
        return digest

    def artifact_path(self, subject_id: str) -> Path:
        return self.artifacts / artifact_sha256(subject_id)

    def write(self, name: str, records: list[dict[str, object]]) -> Path:
        path = self.ledgers / f"{name}.acceptance.ndjson"
        write_ledger(path, records)
        if self.retain_artifacts:
            for subject_id in {str(record["subject_id"]) for record in records}:
                self.retain(artifact_for(subject_id))
        return path

    def audit(self, release_id: str = RELEASE_ID) -> dict[str, object]:
        return audit(self.ledgers, self.reviewers, self.artifacts, release_id)
