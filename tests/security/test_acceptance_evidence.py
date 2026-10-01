from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import mock

from acceptance_fixtures import (
    OTHER_SEED,
    RELEASE_ID,
    REVIEWER_ID,
    REVIEWER_KEY_ID,
    REVIEWER_SEED,
    SESSION_TYPES,
    Workspace,
    artifact_for,
    artifact_sha256,
    attributes_for,
    links_supported,
    make_chain,
    make_record,
    pkcs8,
    resign,
    reviewers_document,
    write_ledger,
)
from tools import acceptance_evidence, ed25519, generate_pipeline_evidence
from tools.acceptance_evidence import (
    APPEND_LOCK_NAME,
    ARTIFACT_SHARED,
    ARTIFACT_UNVERIFIED,
    CRITERIA_NOT_MET,
    CUSTOMER_KIND_CONFLICT,
    DISQUALIFIED,
    GATES,
    MIN_SESSION_SECONDS,
    OTHER_RELEASE,
    REVIEWER_REVOKED,
    SUBSCRIPTION_SHARED,
    ZERO_HASH,
    EvidenceError,
    append_record,
    artifact_is_retained,
    load_ledgers,
    load_trusted_reviewers,
    main,
    retain_artifact,
)

SECOND_REVIEWER_ID = "reviewer.three"
SECOND_KEY_ID = "reviewer.key.three.001"


def audit_of(records: list[dict[str, object]], **workspace_options: object) -> dict[str, object]:
    """Audits one ledger of `records` in a fresh workspace."""
    with tempfile.TemporaryDirectory() as directory:
        workspace = Workspace(directory, **workspace_options)
        workspace.write("paper", records)
        return workspace.audit()


class LedgerIntegrityTests(unittest.TestCase):
    """A malformed, mis-chained or mis-hashed record makes the whole ledger untrusted."""

    def refused(self, record: dict[str, object]) -> str:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_ledger(root / "paper.acceptance.ndjson", [record])
            with self.assertRaises(EvidenceError) as caught:
                load_ledgers(root)
            return str(caught.exception)

    def test_the_status_binds_every_ledger_file_it_counted(self) -> None:
        # Without this a status could not be tied to the ledger state it was
        # computed from, so a receipt could not show what it trusted (E6.3).
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            (workspace.ledgers / "partners").mkdir()
            paper = make_chain(
                ("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.2", {})
            )
            partner = make_chain(("evidence.partner.1", "partner.1", {"evidence_type": "design_partner"}))
            paper_path = workspace.write("paper", paper)
            partner_path = workspace.write("partners/partner", partner)

            report = workspace.audit()

            self.assertEqual(report["acceptance_status_schema_version"], 5)
            self.assertEqual(report["ledger_anchors"], [])
            self.assertEqual(report["release_id"], RELEASE_ID)
            self.assertEqual(
                report["trusted_reviewers_sha256"],
                hashlib.sha256(workspace.reviewers.read_bytes()).hexdigest(),
            )
            self.assertEqual(
                report["ledgers"],
                [
                    {
                        "path": "paper.acceptance.ndjson",
                        "sha256": hashlib.sha256(paper_path.read_bytes()).hexdigest(),
                        "records": 2,
                        "head": paper[-1]["record_hash"],
                    },
                    {
                        "path": "partners/partner.acceptance.ndjson",
                        "sha256": hashlib.sha256(partner_path.read_bytes()).hexdigest(),
                        "records": 1,
                        "head": partner[-1]["record_hash"],
                    },
                ],
            )
            self.assertEqual(report["verified_records"], 3)
            self.assertEqual(report["counted_records"], 3)

    def test_chain_and_unique_subject_counts_are_verified(self) -> None:
        report = audit_of(
            make_chain(("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.2", {}))
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 2)
        self.assertFalse(report["all_gates_eligible"])

    def test_one_subject_counts_once(self) -> None:
        report = audit_of(
            make_chain(("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.1", {}))
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
        self.assertEqual(report["counted_records"], 2)

    def test_tampering_fails_closed(self) -> None:
        record = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")
        record["notes"] = "tampered"
        self.assertIn("hash does not match", self.refused(record))

    def test_duplicate_evidence_ids_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_ledger(
                root / "paper.acceptance.ndjson",
                make_chain(("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.1", "session.paper.2", {})),
            )
            with self.assertRaisesRegex(EvidenceError, "duplicate evidence_id"):
                load_ledgers(root)

    def test_the_schema_version_must_be_the_integer_two(self) -> None:
        # JSON `true` equals 1 in Python and passed an equality check. Version 1
        # records, which nobody signed, are refused outright.
        for version in (True, 2.0, 1, 3):
            with self.subTest(version=version):
                record = make_record(
                    ZERO_HASH, "evidence.paper.1", "session.paper.1", acceptance_evidence_schema_version=version
                )
                self.assertIn("schema version", self.refused(record))

    def test_occurred_at_must_be_exactly_the_canonical_form(self) -> None:
        for value in (
            "2026-08-24 10:00:00Z",  # a space for the T, which fromisoformat accepted
            "2026-W35-1T10:00:00Z",  # an ISO week date, which fromisoformat accepted
            "2026-08-24T10:00:00+00:00",
            "2026-08-24T10:00:00.5Z",
            "2026-02-30T10:00:00Z",
            "٢٠٢٦-08-24T10:00:00Z",  # Arabic-Indic digits
        ):
            with self.subTest(value=value):
                record = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1", occurred_at=value)
                self.assertIn("occurred_at", self.refused(record))

    def test_a_record_that_does_not_follow_its_predecessor_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")
            # Chained to nothing, where it should follow `first`.
            second = make_record(ZERO_HASH, "evidence.paper.2", "session.paper.2")
            write_ledger(root / "paper.acceptance.ndjson", [first, second])
            with self.assertRaisesRegex(EvidenceError, "discontinuous"):
                load_ledgers(root)

    def test_observer_and_reviewer_must_differ(self) -> None:
        record = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1", observed_by=REVIEWER_ID)
        self.assertIn("distinct", self.refused(record))


class DisqualificationTests(unittest.TestCase):
    """A signed statement that a subject did not qualify disqualifies it in its gate."""

    def test_a_rejection_after_acceptance_disqualifies_the_subject(self) -> None:
        # Before E6.2 the later rejection only incremented a counter, and the
        # subject still counted toward the gate.
        report = audit_of(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {}),
                ("evidence.paper.2", "session.paper.1", {"outcome": "rejected"}),
            )
        )
        gate = report["gates"]["paper_session"]
        self.assertEqual((gate["observed"], gate["disqualified_subjects"], gate["rejected_records"]), (0, 1, 1))
        self.assertEqual(report["not_counted"], {DISQUALIFIED: ["evidence.paper.1"]})
        self.assertEqual(report["counted_records"], 0)

    def test_a_rejection_before_acceptance_disqualifies_the_subject(self) -> None:
        # No correction record exists, so a later acceptance cannot overturn it.
        report = audit_of(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {"outcome": "rejected"}),
                ("evidence.paper.2", "session.paper.1", {}),
            )
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertEqual(report["gates"]["paper_session"]["disqualified_subjects"], 1)

    def test_a_rejection_disqualifies_only_its_own_subject_in_its_own_gate(self) -> None:
        report = audit_of(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {}),
                ("evidence.paper.2", "session.paper.2", {"outcome": "rejected"}),
                ("evidence.partner.1", "session.paper.1", {"evidence_type": "design_partner", "outcome": "rejected"}),
            )
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
        self.assertEqual(report["gates"]["paper_session"]["disqualified_subjects"], 1)
        self.assertEqual(report["gates"]["design_partner"]["disqualified_subjects"], 1)
        self.assertEqual(report["gates"]["design_partner"]["observed"], 0)

    def test_a_rejection_disqualifies_regardless_of_its_artifact_and_release(self) -> None:
        report = audit_of(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {}),
                (
                    "evidence.paper.2",
                    "session.paper.1",
                    {"outcome": "rejected", "release_id": "release.other", "source_artifact_sha256": "b" * 64},
                ),
            )
        )
        gate = report["gates"]["paper_session"]
        self.assertEqual((gate["observed"], gate["disqualified_subjects"]), (0, 1))

    def test_a_failing_acceptance_disqualifies_its_subject_when_a_clean_one_follows(self) -> None:
        # The review's finding (4): the failing record only failed to count, and the
        # clean one after it counted the subject (E6.6b).
        failing = {**attributes_for("paper_session"), "unplanned_reconnects": 1}
        for order in ("failing first", "clean first"):
            specifications = [
                ("evidence.paper.1", "session.paper.1", {"attributes": failing}),
                ("evidence.paper.2", "session.paper.1", {}),
            ]
            if order == "clean first":
                specifications.reverse()
            with self.subTest(order):
                report = audit_of(make_chain(*specifications))
                gate = report["gates"]["paper_session"]
                self.assertEqual((gate["observed"], gate["disqualified_subjects"]), (0, 1))
                self.assertEqual(report["not_counted"][CRITERIA_NOT_MET], ["evidence.paper.1"])
                self.assertEqual(report["not_counted"][DISQUALIFIED], ["evidence.paper.1", "evidence.paper.2"])

    def test_a_failing_acceptance_disqualifies_in_every_release(self) -> None:
        failing = {**attributes_for("paper_session"), "reconciliation_discrepancies": 1}
        report = audit_of(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {"attributes": failing, "release_id": "release.other"}),
                ("evidence.paper.2", "session.paper.1", {}),
            )
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)

    def test_a_lone_rejection_is_a_disqualified_subject_and_no_counted_record(self) -> None:
        failing = {**attributes_for("paper_session"), "reconciliation_discrepancies": 2}
        report = audit_of(make_chain(("evidence.x.1", "session.x.1", {"outcome": "rejected", "attributes": failing})))
        gate = report["gates"]["paper_session"]
        self.assertEqual((gate["rejected_records"], gate["disqualified_subjects"]), (1, 1))
        self.assertEqual((report["verified_records"], report["counted_records"]), (1, 0))
        self.assertEqual(report["not_counted"], {})


class SignatureTests(unittest.TestCase):
    """Every record must be signed by a key the trusted set lists for its reviewer (E6.4, E6.6b)."""

    def assert_audit_fails_naming(self, records: list[dict[str, object]], *evidence_ids: str, **options) -> None:
        with self.assertRaisesRegex(EvidenceError, "no listed reviewer key signed") as caught:
            audit_of(records, **options)
        for evidence_id in evidence_ids:
            self.assertIn(evidence_id, str(caught.exception))

    def test_a_record_signed_by_a_listed_reviewer_counts(self) -> None:
        report = audit_of(make_chain(("evidence.paper.1", "session.paper.1", {})))
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
        self.assertEqual(report["not_counted"], {})

    def test_a_signature_from_another_key_fails_the_audit(self) -> None:
        records = make_chain(("evidence.paper.1", "session.paper.1", {"seed": OTHER_SEED}))
        self.assert_audit_fails_naming(records, "evidence.paper.1")

    def test_a_key_the_set_does_not_list_fails_the_audit(self) -> None:
        records = make_chain(("evidence.paper.1", "session.paper.1", {"reviewer_key_id": "reviewer.key.unknown"}))
        self.assert_audit_fails_naming(records, "evidence.paper.1")

    def test_an_empty_set_fails_a_ledger_that_holds_a_record_and_passes_one_that_holds_none(self) -> None:
        self.assert_audit_fails_naming(make_chain(("evidence.paper.1", "session.paper.1", {})), trust=False)
        report = audit_of([], trust=False)
        self.assertEqual((report["verified_records"], report["counted_records"]), (0, 0))

    def test_a_listed_key_cannot_sign_for_another_reviewer(self) -> None:
        # The key is listed for `reviewer.two`. A record naming another reviewer
        # is not that reviewer's, however valid the signature.
        records = make_chain(("evidence.paper.1", "session.paper.1", {"reviewed_by": SECOND_REVIEWER_ID}))
        self.assert_audit_fails_naming(records, "evidence.paper.1")

    def test_a_signed_field_edited_and_rehashed_fails_the_audit(self) -> None:
        # Rewriting the ledger is not enough: the chain and hashes can be redone,
        # the signature cannot.
        (record,) = make_chain(("evidence.paper.1", "session.paper.1", {}))
        self.assert_audit_fails_naming([resign(record, notes="Edited after review.")], "evidence.paper.1")

    def test_a_rejection_edited_into_an_acceptance_fails_the_audit(self) -> None:
        # The review's finding (2). Ignoring a record that no longer authenticates let
        # anyone who can write the ledger requalify a rejected session by editing it.
        # Failing only on an unsigned rejection would not do: the edit makes it an
        # acceptance, so every record must be signed.
        clean, rejection = make_chain(
            ("evidence.paper.1", "session.paper.1", {}),
            ("evidence.paper.2", "session.paper.1", {"outcome": "rejected"}),
        )
        self.assertEqual(audit_of([clean, rejection])["gates"]["paper_session"]["observed"], 0)
        edited = resign(rejection, outcome="accepted")
        self.assert_audit_fails_naming([clean, edited], "evidence.paper.2")

    def test_dropping_a_reviewer_from_the_set_fails_the_audit(self) -> None:
        # The runbook once said a reviewer who leaves is removed from the set. Removing
        # the reviewer who rejected a session made the rejection inert and requalified it.
        records = [
            make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1"),
        ]
        rejection = make_record(
            records[0]["record_hash"], "evidence.paper.2", "session.paper.1", outcome="rejected",
            seed=OTHER_SEED, reviewed_by=SECOND_REVIEWER_ID, reviewer_key_id=SECOND_KEY_ID,
        )
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", [*records, rejection])
            workspace.trust(
                (REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED), (SECOND_KEY_ID, SECOND_REVIEWER_ID, OTHER_SEED)
            )
            self.assertEqual(workspace.audit()["gates"]["paper_session"]["disqualified_subjects"], 1)
            workspace.trust((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED))
            with self.assertRaisesRegex(EvidenceError, "no listed reviewer key signed: evidence.paper.2"):
                workspace.audit()

    def test_a_signed_record_moved_elsewhere_in_the_chain_fails_the_audit(self) -> None:
        # The signature covers the previous hash, so a record cannot be lifted out
        # of the place the reviewer signed it and re-chained.
        _, second = make_chain(
            ("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.2", {})
        )
        other = make_record(ZERO_HASH, "evidence.paper.0", "session.paper.0")
        moved = resign(second, prev_hash=other["record_hash"])
        self.assert_audit_fails_naming([other, moved], "evidence.paper.2")

    def test_a_forged_rejection_fails_the_audit(self) -> None:
        # E6.4 ignored it, reasoning that it could only lower a count. It cannot now be
        # told from a real rejection whose reviewer was dropped, so it fails.
        records = make_chain(
            ("evidence.paper.1", "session.paper.1", {}),
            ("evidence.paper.2", "session.paper.1", {"outcome": "rejected", "seed": OTHER_SEED}),
        )
        self.assert_audit_fails_naming(records, "evidence.paper.2")

    def test_the_failure_names_the_first_five_unsigned_records_and_counts_the_rest(self) -> None:
        records = make_chain(
            *[(f"evidence.paper.{index}", f"session.paper.{index}", {"seed": OTHER_SEED}) for index in range(7)]
        )
        with self.assertRaises(EvidenceError) as caught:
            audit_of(records)
        self.assertEqual(
            str(caught.exception),
            "records that no listed reviewer key signed: evidence.paper.0, evidence.paper.1, "
            "evidence.paper.2, evidence.paper.3, evidence.paper.4 and 2 more",
        )

    def test_the_signature_covers_the_documented_message(self) -> None:
        # Built here from the documented recipe, not from the tool's own helper, so a
        # change to the domain string or the canonical form fails this test and not
        # only the wire format every reviewer's tooling depends on.
        (record,) = make_chain(("evidence.paper.1", "session.paper.1", {}))
        body = {key: value for key, value in record.items() if key not in ("record_hash", "reviewer_signature")}
        message = b"follon-acceptance-evidence-v2\n" + json.dumps(
            body, sort_keys=True, separators=(",", ":"), ensure_ascii=True
        ).encode("utf-8")
        independent = ed25519.sign(REVIEWER_SEED, message).hex()
        self.assertEqual(independent, record["reviewer_signature"])
        report = audit_of(make_chain(("evidence.paper.1", "session.paper.1", {"reviewer_signature": independent})))
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)


class RevocationTests(unittest.TestCase):
    """A key that may no longer be trusted is revoked, never removed (E6.6b)."""

    def audit_revoked(self, records: list[dict[str, object]]) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", records)
            workspace.trust((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED, "revoked"))
            return workspace.audit()

    def test_a_revoked_keys_acceptance_does_not_count(self) -> None:
        report = self.audit_revoked(make_chain(("evidence.paper.1", "session.paper.1", {})))
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertEqual(report["not_counted"], {REVIEWER_REVOKED: ["evidence.paper.1"]})

    def test_a_revoked_keys_rejection_still_disqualifies(self) -> None:
        # Revoking must not be a way to requalify a subject: a rejection only ever
        # lowers a count, so keeping it fails safe.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            clean = make_record(
                ZERO_HASH, "evidence.paper.1", "session.paper.1",
                seed=OTHER_SEED, reviewed_by=SECOND_REVIEWER_ID, reviewer_key_id=SECOND_KEY_ID,
            )
            rejection = make_record(clean["record_hash"], "evidence.paper.2", "session.paper.1", outcome="rejected")
            workspace.write("paper", [clean, rejection])
            workspace.trust(
                (REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED, "revoked"),
                (SECOND_KEY_ID, SECOND_REVIEWER_ID, OTHER_SEED),
            )
            gate = workspace.audit()["gates"]["paper_session"]
        self.assertEqual((gate["observed"], gate["disqualified_subjects"]), (0, 1))

    def test_a_revoked_key_still_verifies_what_it_signed(self) -> None:
        # Its records stay checkable, so editing one is still caught.
        (record,) = make_chain(("evidence.paper.1", "session.paper.1", {}))
        with self.assertRaisesRegex(EvidenceError, "no listed reviewer key signed"):
            self.audit_revoked([resign(record, notes="Edited after revocation.")])

    def test_another_reviewers_acceptance_still_counts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            first = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")
            second = make_record(
                first["record_hash"], "evidence.paper.2", "session.paper.2",
                seed=OTHER_SEED, reviewed_by=SECOND_REVIEWER_ID, reviewer_key_id=SECOND_KEY_ID,
            )
            workspace.write("paper", [first, second])
            workspace.trust(
                (REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED, "revoked"),
                (SECOND_KEY_ID, SECOND_REVIEWER_ID, OTHER_SEED, "active"),
            )
            report = workspace.audit()
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
        self.assertEqual(report["not_counted"], {REVIEWER_REVOKED: ["evidence.paper.1"]})


class ReviewerSetTests(unittest.TestCase):
    """The trusted reviewer set is strict, and enrols only honest public keys (E6.4, E6.6b)."""

    def load(self, document: object) -> object:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "reviewers.json"
            path.write_text(document if isinstance(document, str) else json.dumps(document), encoding="utf-8")
            return load_trusted_reviewers(path)

    def test_the_reviewer_file_is_strict(self) -> None:
        valid = reviewers_document((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED))
        entry = valid["reviewers"][0]
        malformed = {
            "not json": "{",
            "unknown top-level field": {**valid, "extra": 1},
            # With no entries, so that only the version check can refuse it.
            "unknown schema version": {"trusted_reviewers_schema_version": 3, "reviewers": []},
            "boolean schema version": {**valid, "trusted_reviewers_schema_version": True},
            "reviewers not a list": {**valid, "reviewers": {}},
            "unknown entry field": {**valid, "reviewers": [{**entry, "extra": 1}]},
            "version 2 without a status": {
                **valid, "reviewers": [{k: v for k, v in entry.items() if k != "status"}],
            },
            "version 1 with a status": {**valid, "trusted_reviewers_schema_version": 1},
            "an unknown status": {**valid, "reviewers": [{**entry, "status": "retired"}]},
            "non-canonical key id": {**valid, "reviewers": [{**entry, "key_id": "Key One"}]},
            "short public key": {**valid, "reviewers": [{**entry, "public_key_hex": "ab"}]},
            "duplicate key": {**valid, "reviewers": [entry, entry]},
        }
        for name, document in malformed.items():
            with self.subTest(name), self.assertRaises(EvidenceError):
                self.load(document)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "reviewers.json"
            path.write_text(json.dumps(valid), encoding="utf-8")
            loaded = load_trusted_reviewers(path)
            self.assertEqual(loaded.sha256, hashlib.sha256(path.read_bytes()).hexdigest())
            self.assertEqual(set(loaded.keys), {REVIEWER_KEY_ID})
            self.assertEqual(loaded.keys[REVIEWER_KEY_ID].status, "active")
            with self.assertRaises(EvidenceError):
                load_trusted_reviewers(Path(directory) / "missing.json")

    def test_a_version_1_set_is_read_with_every_key_active(self) -> None:
        # Version 1 had no status, and the pipeline wrote one before E6.6b.
        document = {
            "trusted_reviewers_schema_version": 1,
            "reviewers": [
                {"key_id": REVIEWER_KEY_ID, "reviewer_id": REVIEWER_ID, "public_key_hex": ed25519.public_key(REVIEWER_SEED).hex()}
            ],
        }
        self.assertEqual(self.load(document).keys[REVIEWER_KEY_ID].status, "active")
        self.assertEqual(self.load({"trusted_reviewers_schema_version": 1, "reviewers": []}).keys, {})

    def test_a_key_that_is_not_an_honest_public_key_is_refused(self) -> None:
        # The review's finding (1). Audit item 121 made such a key verify nothing; a set
        # that enrols one now fails, instead of seeming to trust someone it cannot.
        field = ed25519.FIELD
        honest = ed25519.public_key(REVIEWER_SEED)
        torsion = ed25519._decompress(
            bytes.fromhex("c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a")
        )
        refused = {
            "the all-zero placeholder": bytes(32),
            "the neutral element": int.to_bytes(1, 32, "little"),
            "the point of order two": int.to_bytes(field - 1, 32, "little"),
            "an honest key plus a torsion point": ed25519._compress(ed25519._add(ed25519._decompress(honest), torsion)),
            "a string that is no point": b"\xff" * 32,
        }
        valid = reviewers_document((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED))
        entry = valid["reviewers"][0]
        for name, key in refused.items():
            with self.subTest(name):
                document = {**valid, "reviewers": [{**entry, "public_key_hex": key.hex()}]}
                with self.assertRaisesRegex(EvidenceError, "not a valid Ed25519 public key"):
                    self.load(document)
        self.assertEqual(set(self.load(valid).keys), {REVIEWER_KEY_ID})

    def test_one_public_key_cannot_be_two_reviewers(self) -> None:
        document = reviewers_document(
            (REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED), (SECOND_KEY_ID, SECOND_REVIEWER_ID, REVIEWER_SEED)
        )
        with self.assertRaisesRegex(EvidenceError, "repeats the public key"):
            self.load(document)


class ArtifactRetentionTests(unittest.TestCase):
    """A record's source artifact is re-hashed against the retained root (E6.4)."""

    def audit_with(self, prepare) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", make_chain(("evidence.paper.1", "session.paper.1", {})))
            prepare(workspace)
            return workspace.audit()

    def test_a_retained_artifact_lets_its_record_count(self) -> None:
        report = self.audit_with(lambda workspace: None)
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)

    def test_a_missing_artifact_means_the_record_cannot_be_checked(self) -> None:
        report = self.audit_with(lambda workspace: workspace.artifact_path("session.paper.1").unlink())
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertEqual(report["not_counted"][ARTIFACT_UNVERIFIED], ["evidence.paper.1"])

    def test_an_artifact_that_no_longer_matches_its_digest_does_not_count(self) -> None:
        report = self.audit_with(lambda workspace: workspace.artifact_path("session.paper.1").write_bytes(b"replaced"))
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertEqual(report["not_counted"][ARTIFACT_UNVERIFIED], ["evidence.paper.1"])

    def test_a_link_in_the_artifact_root_does_not_count(self) -> None:
        def replace_with_a_link(workspace: Workspace) -> None:
            elsewhere = workspace.artifacts.parent / "elsewhere"
            elsewhere.write_bytes(artifact_for("session.paper.1"))
            workspace.artifact_path("session.paper.1").unlink()
            try:
                workspace.artifact_path("session.paper.1").symlink_to(elsewhere)
            except (OSError, NotImplementedError):
                self.skipTest("cannot create a symbolic link here")

        report = self.audit_with(replace_with_a_link)
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)

    def test_retention_is_content_addressed_and_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            content = artifact_for("session.paper.1")
            digest = retain_artifact(content, root / "store")
            self.assertEqual(digest, artifact_sha256("session.paper.1"))
            self.assertTrue(artifact_is_retained(root / "store", digest))
            self.assertEqual(retain_artifact(content, root / "store"), digest)
            # Nothing but the artifact is left behind: no temporary file survives.
            self.assertEqual([path.name for path in (root / "store").iterdir()], [digest])
            (root / "store" / digest).write_bytes(b"different")
            with self.assertRaisesRegex(EvidenceError, "different artifact"):
                retain_artifact(content, root / "store")
            self.assertFalse(artifact_is_retained(root / "store", "c" * 64))

    def test_a_retention_that_fails_part_way_leaves_nothing_behind(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            store = Path(directory) / "store"
            with mock.patch.object(acceptance_evidence.os, "replace", side_effect=OSError("the disk is full")):
                with self.assertRaisesRegex(OSError, "the disk is full"):
                    retain_artifact(artifact_for("session.paper.1"), store)
            self.assertEqual(list(store.iterdir()), [])


class ExclusiveBackingTests(unittest.TestCase):
    """One artifact backs one subject, one subscription one customer, and a customer is one kind (E6.6b)."""

    def test_one_artifact_cannot_back_two_subjects(self) -> None:
        # The review's finding (3): thirty sessions could be one artifact.
        shared = artifact_sha256("session.paper.1")
        report = audit_of(
            make_chain(
                *[
                    (f"evidence.paper.{index}", f"session.paper.{index}", {"source_artifact_sha256": shared})
                    for index in range(1, 31)
                ]
            )
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertEqual(len(report["not_counted"][ARTIFACT_SHARED]), 30)

    def test_one_artifact_may_back_one_subject_many_times(self) -> None:
        report = audit_of(
            make_chain(("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.1", {}))
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
        self.assertNotIn(ARTIFACT_SHARED, report["not_counted"])

    def test_an_artifact_shared_across_gates_or_releases_counts_for_neither(self) -> None:
        shared = artifact_sha256("session.paper.1")
        for name, (subject_id, keywords) in {
            "another gate": ("subject.x.2", {"evidence_type": "design_partner", "source_artifact_sha256": shared}),
            "another release": ("subject.x.2", {"release_id": "release.other", "source_artifact_sha256": shared}),
            # One id in two gates names two subjects, so the artifact backs two.
            "the same id in another gate": ("session.paper.1", {"evidence_type": "design_partner"}),
        }.items():
            with self.subTest(name):
                report = audit_of(
                    make_chain(("evidence.paper.1", "session.paper.1", {}), ("evidence.x.2", subject_id, keywords))
                )
                self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
                self.assertIn("evidence.paper.1", report["not_counted"][ARTIFACT_SHARED])

    def test_a_rejection_citing_an_artifact_does_not_share_it(self) -> None:
        shared = artifact_sha256("session.paper.1")
        report = audit_of(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {}),
                ("evidence.paper.2", "session.paper.2", {"outcome": "rejected", "source_artifact_sha256": shared}),
            )
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)

    def test_one_subscription_cannot_back_two_customers(self) -> None:
        # Ten paying professionals could be one subscription.
        records = make_chain(
            *[
                (
                    f"evidence.customer.{index}",
                    f"customer.{index}",
                    {
                        "evidence_type": "paying_customer",
                        "attributes": {"customer_kind": "professional", "subscription_id": "subscription.one"},
                    },
                )
                for index in range(10)
            ]
        )
        report = audit_of(records)
        gate = report["gates"]["paying_customer"]
        self.assertEqual((gate["observed"], gate["eligible"]), (0, False))
        self.assertEqual(len(report["not_counted"][SUBSCRIPTION_SHARED]), 10)

    def test_a_customer_is_one_kind(self) -> None:
        records = make_chain(
            ("evidence.customer.1", "customer.1", {"evidence_type": "paying_customer", "customer_kind": "professional"}),
            ("evidence.customer.2", "customer.1", {"evidence_type": "paying_customer", "customer_kind": "organisation"}),
        )
        report = audit_of(records)
        self.assertEqual(report["gates"]["paying_customer"]["observed"], 0)
        self.assertEqual(report["not_counted"][CUSTOMER_KIND_CONFLICT], ["evidence.customer.1", "evidence.customer.2"])


class ReleaseBindingTests(unittest.TestCase):
    """Evidence counts only toward the release it exercised (E6.4)."""

    def test_another_releases_evidence_does_not_count(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write(
                "paper",
                make_chain(
                    ("evidence.paper.1", "session.paper.1", {}),
                    ("evidence.paper.2", "session.paper.2", {"release_id": "release.other"}),
                ),
            )
            own = workspace.audit()
            other = workspace.audit("release.other")
            self.assertEqual(own["gates"]["paper_session"]["observed"], 1)
            self.assertEqual(own["not_counted"][OTHER_RELEASE], ["evidence.paper.2"])
            self.assertEqual(other["gates"]["paper_session"]["observed"], 1)
            self.assertEqual(other["not_counted"][OTHER_RELEASE], ["evidence.paper.1"])
            self.assertEqual(other["release_id"], "release.other")

    def test_the_release_must_be_a_canonical_id(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            with self.assertRaisesRegex(EvidenceError, "release_id"):
                workspace.audit("Release One")

    def test_a_session_runs_in_its_own_environment_and_nothing_else_has_one(self) -> None:
        cases = {
            "paper session in LIVE": {"evidence_type": "paper_session", "environment": "LIVE"},
            "live session in PAPER": {"evidence_type": "live_session", "environment": "PAPER"},
            "session in no environment": {"evidence_type": "paper_session", "environment": None},
            "partner in PAPER": {"evidence_type": "design_partner", "environment": "PAPER"},
        }
        for name, keywords in cases.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as directory:
                write_ledger(
                    Path(directory) / "x.acceptance.ndjson",
                    [make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", **keywords)],
                )
                with self.assertRaisesRegex(EvidenceError, "environment"):
                    load_ledgers(Path(directory))


class SessionCriteriaTests(unittest.TestCase):
    """What a clean session is, fixed before any session counts (E6.5)."""

    def counted(self, evidence_type: str, **changes: object) -> bool:
        attributes = {**attributes_for(evidence_type, subject_id="subject.x.1"), **changes}
        report = audit_of(
            make_chain(("evidence.x.1", "subject.x.1", {"evidence_type": evidence_type, "attributes": attributes}))
        )
        if not report["counted_records"]:
            # A failing acceptance also disqualifies its own subject (E6.6b).
            self.assertEqual(report["not_counted"], {CRITERIA_NOT_MET: ["evidence.x.1"], DISQUALIFIED: ["evidence.x.1"]})
        return bool(report["counted_records"])

    def test_a_clean_session_lasts_at_least_a_regular_us_equity_session(self) -> None:
        # 6.5 hours. Pinned by value, because the tests below are written in terms of
        # the constant and would follow it wherever it went.
        self.assertEqual(MIN_SESSION_SECONDS, 23_400)

    def test_the_smallest_qualifying_session_counts(self) -> None:
        for evidence_type in SESSION_TYPES:
            with self.subTest(evidence_type):
                self.assertTrue(self.counted(evidence_type))

    def test_every_shortfall_disqualifies_an_accepted_session(self) -> None:
        shortfalls = {
            "shorter than a regular session": {"session_seconds": MIN_SESSION_SECONDS - 1},
            "no order": {"orders_submitted": 0},
            "no reconciliation": {"reconciliations_completed": 0},
            "an order still UNKNOWN": {"unknown_orders_at_close": 1},
            "a discrepancy": {"reconciliation_discrepancies": 1},
            "an unexplained incident": {"unexplained_incidents": 1},
            "an unplanned reconnect": {"unplanned_reconnects": 1},
        }
        for evidence_type in SESSION_TYPES:
            for name, change in shortfalls.items():
                with self.subTest(evidence_type=evidence_type, shortfall=name):
                    self.assertFalse(self.counted(evidence_type, **change))

    def test_planned_reconnect_drills_never_disqualify(self) -> None:
        self.assertTrue(self.counted("paper_session", planned_reconnect_drills=5))

    def test_the_other_gates_have_their_own_criteria(self) -> None:
        self.assertTrue(self.counted("design_partner"))
        self.assertFalse(self.counted("design_partner", unaided=False))
        self.assertFalse(self.counted("design_partner", workflows_completed=0))
        self.assertTrue(self.counted("broker_options"))
        self.assertFalse(self.counted("broker_options", reconciled_environments=["BACKTEST", "PAPER"]))

    def test_attribute_shapes_are_strict(self) -> None:
        good = attributes_for("paper_session")
        malformed = {
            "paper_session": [
                {**good, "extra": 1},
                {key: value for key, value in good.items() if key != "orders_submitted"},
                {**good, "orders_submitted": True},
                {**good, "orders_submitted": -1},
                {**good, "session_seconds": 1.5},
            ],
            "design_partner": [{"workflows_completed": 1, "unaided": "yes"}, {"workflows_completed": 1}],
            "broker_options": [
                {"reconciled_environments": ["PAPER", "LIVE"]},
                {"reconciled_environments": ["LIVE", "LIVE", "PAPER"]},
                {"reconciled_environments": ["MOON"]},
                {"reconciled_environments": "PAPER"},
            ],
            "paying_customer": [
                {"customer_kind": "person", "subscription_id": "subscription.unit.001"},
                {"customer_kind": "professional", "subscription_id": "Subscription One"},
            ],
        }
        for evidence_type, candidates in malformed.items():
            for candidate in candidates:
                with self.subTest(evidence_type=evidence_type, attributes=candidate):
                    with tempfile.TemporaryDirectory() as directory:
                        write_ledger(
                            Path(directory) / "x.acceptance.ndjson",
                            [make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", evidence_type=evidence_type, attributes=candidate)],
                        )
                        with self.assertRaises(EvidenceError):
                            load_ledgers(Path(directory))


class GateTargetTests(unittest.TestCase):
    def test_the_gates_are_the_ones_the_plan_requires(self) -> None:
        # 30 PAPER sessions, 60 controlled-LIVE sessions, five design partners, one
        # option-capable broker export, and ten paying professionals or three paying
        # organisations. Pinned by value: the gate tests are written in terms of GATES.
        self.assertEqual(
            GATES,
            {
                "paper_session": ((None, 30),),
                "live_session": ((None, 60),),
                "design_partner": ((None, 5),),
                "broker_options": ((None, 1),),
                "paying_customer": (("professional", 10), ("organisation", 3)),
            },
        )


class CustomerGateTests(unittest.TestCase):
    """The commercial gate is ten paying professionals or three paying organisations (E6.5)."""

    def gate(self, professionals: int, organisations: int) -> dict[str, object]:
        specifications = [
            (f"evidence.customer.p{index}", f"customer.p{index}",
             {"evidence_type": "paying_customer", "customer_kind": "professional"})
            for index in range(professionals)
        ] + [
            (f"evidence.customer.o{index}", f"customer.o{index}",
             {"evidence_type": "paying_customer", "customer_kind": "organisation"})
            for index in range(organisations)
        ]
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("customers", make_chain(*specifications))
            return workspace.audit()["gates"]["paying_customer"]

    def test_ten_professionals_or_three_organisations_open_the_gate(self) -> None:
        self.assertFalse(self.gate(9, 0)["eligible"])
        self.assertTrue(self.gate(10, 0)["eligible"])
        self.assertFalse(self.gate(0, 2)["eligible"])
        self.assertTrue(self.gate(0, 3)["eligible"])

    def test_kinds_are_not_pooled(self) -> None:
        # Nine professionals and two organisations is neither alternative.
        gate = self.gate(9, 2)
        self.assertFalse(gate["eligible"])
        self.assertEqual(gate["observed"], 11)
        self.assertEqual(
            {row["customer_kind"]: (row["observed"], row["required"]) for row in gate["alternatives"]},
            {"professional": (9, 10), "organisation": (2, 3)},
        )

    def test_the_nearest_alternative_is_reported(self) -> None:
        gate = self.gate(9, 1)
        self.assertEqual((gate["required"], gate["remaining"]), (10, 1))
        gate = self.gate(1, 2)
        self.assertEqual((gate["required"], gate["remaining"]), (3, 1))
        # A tie is reported against the smaller requirement.
        gate = self.gate(9, 2)
        self.assertEqual((gate["required"], gate["remaining"]), (3, 1))

    def test_a_rejected_customer_does_not_count_in_their_kind(self) -> None:
        specifications = [
            (f"evidence.customer.o{index}", f"customer.o{index}",
             {"evidence_type": "paying_customer", "customer_kind": "organisation"})
            for index in range(3)
        ] + [
            ("evidence.customer.reject", "customer.o0",
             {"evidence_type": "paying_customer", "customer_kind": "organisation", "outcome": "rejected"})
        ]
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("customers", make_chain(*specifications))
            gate = workspace.audit()["gates"]["paying_customer"]
        self.assertFalse(gate["eligible"])
        self.assertEqual(gate["disqualified_subjects"], 1)


class AppendWorkflowTests(unittest.TestCase):
    """A reviewer can sign and append a record, and the audit counts it (E6.4, E6.6b)."""

    def template(self, evidence_id: str = "evidence.paper.1", subject_id: str = "session.paper.1") -> dict:
        return {
            "evidence_id": evidence_id,
            "evidence_type": "paper_session",
            "subject_id": subject_id,
            "occurred_at": "2026-08-24T10:00:00Z",
            "observed_by": "operator.one",
            "reviewed_by": REVIEWER_ID,
            "outcome": "accepted",
            "notes": "Independently reviewed unit fixture.",
            "release_id": RELEASE_ID,
            "environment": "PAPER",
            "attributes": attributes_for("paper_session"),
        }

    def artifact(self, directory: str, subject_id: str = "session.paper.1") -> Path:
        path = Path(directory) / f"{subject_id}.log"
        path.write_bytes(artifact_for(subject_id))
        return path

    def append(
        self,
        workspace: Workspace,
        template: dict,
        artifact: Path,
        ledger: Path | None = None,
        seed: bytes = REVIEWER_SEED,
        signing_key: str = REVIEWER_KEY_ID,
    ) -> dict[str, object]:
        return append_record(
            ledger or workspace.ledgers / "paper.acceptance.ndjson",
            template,
            artifact,
            ledger_root=workspace.ledgers,
            artifact_root=workspace.artifacts,
            trusted=load_trusted_reviewers(workspace.reviewers),
            seed=seed,
            reviewer_key_id=signing_key,
        )

    def lock_of(self, workspace: Workspace) -> Path:
        return workspace.ledgers / APPEND_LOCK_NAME

    def test_an_appended_record_is_signed_chained_retained_and_counted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            first = self.append(workspace, self.template(), self.artifact(directory))
            second = self.append(
                workspace, self.template("evidence.paper.2", "session.paper.2"), self.artifact(directory, "session.paper.2")
            )

            self.assertEqual(first["prev_hash"], ZERO_HASH)
            self.assertEqual(second["prev_hash"], first["record_hash"])
            self.assertEqual(first["source_artifact_sha256"], artifact_sha256("session.paper.1"))
            self.assertFalse(self.lock_of(workspace).exists())
            report = workspace.audit()
            self.assertEqual(report["gates"]["paper_session"]["observed"], 2)
            self.assertEqual(report["not_counted"], {})

    def test_a_ledger_in_a_subdirectory_chains_on_its_own(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            self.append(workspace, self.template(), self.artifact(directory))
            nested = workspace.ledgers / "partners" / "paper.acceptance.ndjson"
            record = self.append(
                workspace, self.template("evidence.paper.2", "session.paper.2"),
                self.artifact(directory, "session.paper.2"), ledger=nested,
            )
            self.assertEqual(record["prev_hash"], ZERO_HASH)
            self.assertEqual(workspace.audit()["gates"]["paper_session"]["observed"], 2)

    def test_an_append_is_refused_before_anything_is_written(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            artifact = self.artifact(directory)
            ledger = workspace.ledgers / "paper.acceptance.ndjson"
            self.append(workspace, self.template(), artifact)
            before = ledger.read_bytes()
            retained = sorted(path.name for path in workspace.artifacts.iterdir())
            other = self.artifact(directory, "session.paper.9")

            def attempt(template, target=ledger, source=other, **keywords):
                return lambda: self.append(workspace, template, source, ledger=target, **keywords)

            outside = Path(directory) / "outside.acceptance.ndjson"
            refusals = {
                "a repeated evidence id": (attempt(self.template()), "already holds"),
                "an extra template field": (attempt({**self.template("evidence.paper.9"), "extra": 1}), "exactly"),
                "a missing template field": (
                    attempt({k: v for k, v in self.template("evidence.paper.9").items() if k != "notes"}),
                    "exactly",
                ),
                "attributes that miss the shape": (
                    attempt({**self.template("evidence.paper.9"), "attributes": {"extra": 1}}),
                    "attributes",
                ),
                "a wrong environment": (
                    attempt({**self.template("evidence.paper.9"), "environment": "LIVE"}),
                    "environment",
                ),
                "a file that is not a ledger": (
                    attempt(self.template("evidence.paper.9"), target=workspace.ledgers / "paper.ndjson"),
                    "acceptance.ndjson",
                ),
                "a ledger outside the root": (attempt(self.template("evidence.paper.9"), target=outside), "inside the ledger root"),
                "an artifact that is not a file": (
                    attempt(self.template("evidence.paper.9"), source=Path(directory)),
                    "regular file",
                ),
                # The review's finding (7): a mistyped key id appended a record every later
                # audit refuses, and a rejection under it would have been inert.
                "a key id the set does not list": (
                    attempt(self.template("evidence.paper.9"), signing_key="reviewer.key.two.002"),
                    "listed as active",
                ),
                "a seed that is not the listed key's": (
                    attempt(self.template("evidence.paper.9"), seed=OTHER_SEED),
                    "listed as active",
                ),
                "a key listed for another reviewer": (
                    attempt({**self.template("evidence.paper.9"), "reviewed_by": SECOND_REVIEWER_ID}),
                    "listed as active",
                ),
            }
            for name, (append, message) in refusals.items():
                with self.subTest(name):
                    with self.assertRaisesRegex(EvidenceError, message):
                        append()
                    self.assertEqual(ledger.read_bytes(), before)
                    self.assertEqual(sorted(path.name for path in workspace.artifacts.iterdir()), retained)
                    self.assertFalse(self.lock_of(workspace).exists())
            self.assertFalse((workspace.ledgers / "paper.ndjson").exists())
            self.assertFalse(outside.exists())

    def test_a_revoked_key_cannot_append(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            workspace.trust((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED, "revoked"))
            with self.assertRaisesRegex(EvidenceError, "listed as active"):
                self.append(workspace, self.template(), self.artifact(directory))
            self.assertFalse((workspace.ledgers / "paper.acceptance.ndjson").exists())

    def test_one_evidence_id_cannot_enter_two_ledgers(self) -> None:
        # The review's finding (7): the check read only the target ledger, and the next
        # audit refused the whole root for the duplicate.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            self.append(workspace, self.template(), self.artifact(directory))
            other_ledger = workspace.ledgers / "other.acceptance.ndjson"
            with self.assertRaisesRegex(EvidenceError, "the ledger root already holds that evidence_id"):
                self.append(
                    workspace, self.template(subject_id="session.paper.2"),
                    self.artifact(directory, "session.paper.2"), ledger=other_ledger,
                )
            self.assertFalse(other_ledger.exists())
            workspace.audit()

    def test_a_root_holding_an_unsigned_record_takes_no_append(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            forged = make_record(ZERO_HASH, "evidence.forged.1", "session.forged.1", seed=OTHER_SEED)
            write_ledger(workspace.ledgers / "forged.acceptance.ndjson", [forged])
            with self.assertRaisesRegex(EvidenceError, "no listed reviewer key signed: evidence.forged.1"):
                self.append(workspace, self.template(), self.artifact(directory))
            self.assertFalse((workspace.ledgers / "paper.acceptance.ndjson").exists())

    def test_an_append_refuses_while_another_holds_the_lock(self) -> None:
        # Two appends that overlapped both chained to one head, and the second broke the
        # chain (finding 7). The second now finds the lock and refuses.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            self.lock_of(workspace).write_bytes(b"")
            with self.assertRaisesRegex(EvidenceError, "another append holds the ledger root's lock"):
                self.append(workspace, self.template(), self.artifact(directory))
            self.assertFalse((workspace.ledgers / "paper.acceptance.ndjson").exists())
            self.assertTrue(self.lock_of(workspace).exists(), "a refused append must not take another's lock")
            self.lock_of(workspace).unlink()
            self.append(workspace, self.template(), self.artifact(directory))
            self.assertFalse(self.lock_of(workspace).exists())

    def test_the_lock_is_held_for_the_whole_append(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            seen: list[bool] = []
            original = acceptance_evidence.retain_artifact

            def retain_and_look(content: bytes, root: Path) -> str:
                seen.append(self.lock_of(workspace).exists())
                return original(content, root)

            with mock.patch.object(acceptance_evidence, "retain_artifact", retain_and_look):
                self.append(workspace, self.template(), self.artifact(directory))
            self.assertEqual(seen, [True])

    def test_the_bytes_retained_are_the_bytes_hashed(self) -> None:
        # The review's finding (7): the artifact was read once to hash and again to retain,
        # so one that grew in between was retained under a digest it did not have.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            growing = iter([artifact_for("session.paper.1"), artifact_for("session.paper.1") + b" and more"])
            with mock.patch.object(acceptance_evidence, "read_artifact", lambda path: next(growing)):
                record = self.append(workspace, self.template(), self.artifact(directory))
            self.assertTrue(artifact_is_retained(workspace.artifacts, str(record["source_artifact_sha256"])))
            self.assertEqual(workspace.audit()["gates"]["paper_session"]["observed"], 1)

    @unittest.skipUnless(links_supported(), "cannot create a symbolic link here")
    def test_a_link_planted_in_the_artifact_store_is_never_written_through(self) -> None:
        # The review's finding (7): retention wrote a fixed `.partial` name, so a link
        # planted there made it overwrite the link's target.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            victim = Path(directory) / "victim.txt"
            victim.write_bytes(b"must survive")
            digest = artifact_sha256("session.paper.1")
            (workspace.artifacts / f".{digest}.partial").symlink_to(victim)
            self.append(workspace, self.template(), self.artifact(directory))
            self.assertEqual(victim.read_bytes(), b"must survive")
            self.assertTrue(artifact_is_retained(workspace.artifacts, digest))
            (workspace.artifacts / digest).unlink()
            (workspace.artifacts / digest).symlink_to(victim)
            with self.assertRaisesRegex(EvidenceError, "holds a link where an artifact belongs"):
                retain_artifact(artifact_for("session.paper.1"), workspace.artifacts)
            self.assertEqual(victim.read_bytes(), b"must survive")

    @unittest.skipUnless(links_supported(), "cannot create a symbolic link here")
    def test_a_linked_artifact_root_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            elsewhere = Path(directory) / "elsewhere"
            elsewhere.mkdir()
            linked = Path(directory) / "linked-store"
            linked.symlink_to(elsewhere, target_is_directory=True)
            with self.assertRaisesRegex(EvidenceError, "must not be a link"):
                retain_artifact(b"content", linked)
            self.assertEqual(list(elsewhere.iterdir()), [])

    def test_a_tampered_ledger_refuses_further_appends(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            ledger = workspace.ledgers / "paper.acceptance.ndjson"
            self.append(workspace, self.template(), self.artifact(directory))
            ledger.write_text(ledger.read_text(encoding="utf-8").replace("Independently", "Casually"), encoding="utf-8")
            with self.assertRaisesRegex(EvidenceError, "hash does not match"):
                self.append(
                    workspace, self.template("evidence.paper.2", "session.paper.2"),
                    self.artifact(directory, "session.paper.2"),
                )
            self.assertFalse(self.lock_of(workspace).exists())

    def command(self, workspace: Workspace, ledger: Path, template: Path, artifact: Path, key: Path) -> list[str]:
        return [
            "append", str(ledger), "--ledger-root", str(workspace.ledgers),
            "--trusted-reviewers", str(workspace.reviewers), "--record", str(template),
            "--artifact", str(artifact), "--artifact-root", str(workspace.artifacts),
            "--reviewer-key", str(key), "--reviewer-key-id", REVIEWER_KEY_ID,
        ]

    def test_the_command_line_signs_appends_and_audits(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            root = Path(directory)
            artifact = self.artifact(directory)
            key = root / "reviewer.pk8"
            key.write_bytes(pkcs8(REVIEWER_SEED))
            template = root / "record.json"
            template.write_text(json.dumps(self.template()), encoding="utf-8")
            ledger = workspace.ledgers / "paper.acceptance.ndjson"

            output = StringIO()
            with redirect_stdout(output):
                code = main(self.command(workspace, ledger, template, artifact, key))
            self.assertEqual(code, 0)
            self.assertIn("appended evidence.paper.1", output.getvalue())

            report_path = root / "status.json"
            self.assertEqual(
                main([
                    "audit", str(workspace.ledgers), "--trusted-reviewers", str(workspace.reviewers),
                    "--artifact-root", str(workspace.artifacts), "--release-id", RELEASE_ID,
                    "--output", str(report_path),
                ]),
                0,
            )
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertEqual(report["gates"]["paper_session"]["observed"], 1)

            # A key that is not an Ed25519 PKCS#8 document is refused, and a failure exits 2.
            key.write_bytes(b"not a key")
            with redirect_stderr(StringIO()):
                self.assertEqual(main(self.command(workspace, ledger, template, artifact, key)), 2)

    def test_the_command_line_refuses_a_bad_key_or_template_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            root = Path(directory)
            artifact = self.artifact(directory)
            key = root / "reviewer.pk8"
            key.write_bytes(pkcs8(REVIEWER_SEED))
            template = root / "record.json"
            template.write_text(json.dumps(self.template()), encoding="utf-8")
            ledger = workspace.ledgers / "paper.acceptance.ndjson"

            def append(key_path: Path, template_path: Path) -> tuple[int, str]:
                errors = StringIO()
                with redirect_stderr(errors):
                    code = main(self.command(workspace, ledger, template_path, artifact, key_path))
                return code, errors.getvalue()

            not_json = root / "not-json.json"
            not_json.write_text("{", encoding="utf-8")
            not_an_object = root / "not-an-object.json"
            not_an_object.write_text("[]", encoding="utf-8")
            # Each refusal is held to its own message, because the command's outer handler
            # exits 2 for anything, so the exit code alone would not say which check fired.
            refusals = {
                "a key file that does not exist": (root / "absent.pk8", template, "must be a regular file"),
                "a directory for a key": (root, template, "must be a regular file"),
                "a template that does not exist": (key, root / "absent.json", "cannot read the record template"),
                "a template that is not JSON": (key, not_json, "cannot read the record template"),
                "a template that is not an object": (key, not_an_object, "template must be an object"),
            }
            link = root / "linked.pk8"
            try:
                link.symlink_to(key)
            except OSError:
                pass  # No link can be made here, so that one case is not held.
            else:
                refusals["a linked key"] = (link, template, "must be a regular file")
            for name, (key_path, template_path, message) in refusals.items():
                with self.subTest(refused=name):
                    code, errors = append(key_path, template_path)
                    self.assertEqual(code, 2)
                    self.assertIn(message, errors)
                    self.assertFalse(ledger.exists())
            # The same command with a sound key and template appends, so each refusal above
            # was about its input and not about the harness.
            self.assertEqual(append(key, template), (0, ""))
            self.assertTrue(ledger.exists())

    def test_the_command_line_requires_the_ledger_root_and_the_reviewer_set(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            full = self.command(workspace, workspace.ledgers / "x.acceptance.ndjson", Path("t"), Path("a"), Path("k"))
            for option in ("--ledger-root", "--trusted-reviewers"):
                index = full.index(option)
                with self.subTest(option), redirect_stderr(StringIO()), self.assertRaises(SystemExit):
                    main(full[:index] + full[index + 2:])


class PipelineAcceptanceRootTests(unittest.TestCase):
    """The evidence pipeline counts only the operational ledger root (E6.1)."""

    def publish(self, var_dir: Path) -> dict[str, object]:
        with redirect_stdout(StringIO()):
            target = generate_pipeline_evidence.publish_acceptance_status(var_dir, RELEASE_ID)
        return json.loads(target.read_text(encoding="utf-8"))

    def test_a_ledger_elsewhere_under_var_is_never_counted(self) -> None:
        # The shape the 2026-09-27 assessment left behind: a structurally
        # valid synthetic customer record retained with a report.
        with tempfile.TemporaryDirectory() as directory:
            var_dir = Path(directory)
            review = var_dir / "reports" / "project-assessment" / "review"
            review.mkdir(parents=True)
            synthetic = make_record(
                ZERO_HASH,
                "evidence.review.customer",
                "customer.synthetic",
                evidence_type="paying_customer",
            )
            write_ledger(review / "synthetic.acceptance.ndjson", [synthetic])

            report = self.publish(var_dir)

            self.assertEqual(report["verified_records"], 0)
            self.assertEqual(report["gates"]["paying_customer"]["observed"], 0)
            self.assertFalse(report["gates"]["paying_customer"]["eligible"])

    def test_a_ledger_nobody_listed_a_key_for_fails_the_pipeline(self) -> None:
        # The pipeline provisions an empty reviewer set, which lists no key, so a record
        # in the operational root cannot be authenticated and the audit fails (E6.6b).
        with tempfile.TemporaryDirectory() as directory:
            var_dir = Path(directory)
            root = var_dir / generate_pipeline_evidence.ACCEPTANCE_LEDGER_DIRECTORY
            root.mkdir()
            write_ledger(
                root / "paper.acceptance.ndjson",
                [make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")],
            )
            output = StringIO()
            with redirect_stdout(output), self.assertRaises(SystemExit):
                generate_pipeline_evidence.publish_acceptance_status(var_dir, RELEASE_ID)
            self.assertIn("no listed reviewer key signed: evidence.paper.1", output.getvalue())
            self.assertEqual(
                json.loads((var_dir / generate_pipeline_evidence.ACCEPTANCE_REVIEWERS_FILE).read_text(encoding="utf-8")),
                {"trusted_reviewers_schema_version": 2, "reviewers": []},
            )

    def test_an_operators_reviewer_set_and_artifacts_make_a_ledger_count(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            var_dir = Path(directory)
            root = var_dir / generate_pipeline_evidence.ACCEPTANCE_LEDGER_DIRECTORY
            store = var_dir / generate_pipeline_evidence.ACCEPTANCE_ARTIFACT_DIRECTORY
            root.mkdir()
            store.mkdir()
            reviewers = var_dir / generate_pipeline_evidence.ACCEPTANCE_REVIEWERS_FILE
            document = json.dumps(reviewers_document((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED)))
            reviewers.write_text(document, encoding="utf-8")
            (store / artifact_sha256("session.paper.1")).write_bytes(artifact_for("session.paper.1"))
            write_ledger(
                root / "paper.acceptance.ndjson",
                [make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")],
            )

            report = self.publish(var_dir)

            self.assertEqual(report["counted_records"], 1)
            self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
            # The operator's set is never overwritten by the pipeline.
            self.assertEqual(reviewers.read_text(encoding="utf-8"), document)

    def test_an_absent_operational_root_is_created_empty(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            var_dir = Path(directory)

            report = self.publish(var_dir)

            self.assertTrue((var_dir / generate_pipeline_evidence.ACCEPTANCE_LEDGER_DIRECTORY).is_dir())
            self.assertTrue((var_dir / generate_pipeline_evidence.ACCEPTANCE_ARTIFACT_DIRECTORY).is_dir())
            self.assertEqual(report["verified_records"], 0)
            self.assertFalse(report["all_gates_eligible"])


if __name__ == "__main__":
    unittest.main()
