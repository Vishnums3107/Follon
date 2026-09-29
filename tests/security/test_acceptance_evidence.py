from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path

from acceptance_fixtures import (
    DEFAULT_ARTIFACT,
    DEFAULT_ARTIFACT_SHA256,
    OTHER_SEED,
    RELEASE_ID,
    REVIEWER_ID,
    REVIEWER_KEY_ID,
    REVIEWER_SEED,
    SESSION_TYPES,
    Workspace,
    attributes_for,
    make_chain,
    make_record,
    pkcs8,
    resign,
    reviewers_document,
    write_ledger,
)
from tools import ed25519, generate_pipeline_evidence
from tools.acceptance_evidence import (
    ARTIFACT_UNVERIFIED,
    CRITERIA_NOT_MET,
    GATES,
    MIN_SESSION_SECONDS,
    OTHER_RELEASE,
    UNAUTHENTICATED,
    ZERO_HASH,
    EvidenceError,
    append_record,
    artifact_is_retained,
    load_ledgers,
    load_trusted_reviewers,
    main,
    retain_artifact,
)


class LedgerIntegrityTests(unittest.TestCase):
    """A malformed, mis-chained or mis-hashed record makes the whole ledger untrusted."""

    def report_for(self, records: list[dict[str, object]]) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", records)
            return workspace.audit()

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
            partner_path = workspace.ledgers / "partners" / "partner.acceptance.ndjson"
            write_ledger(partner_path, partner)

            report = workspace.audit()

            self.assertEqual(report["acceptance_status_schema_version"], 3)
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
        report = self.report_for(
            make_chain(("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.2", {}))
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 2)
        self.assertFalse(report["all_gates_eligible"])

    def test_one_subject_counts_once(self) -> None:
        report = self.report_for(
            make_chain(("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.1", {}))
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)

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

    def test_a_rejection_after_acceptance_disqualifies_the_subject(self) -> None:
        # Before E6.2 the later rejection only incremented a counter, and the
        # subject still counted toward the gate.
        report = self.report_for(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {}),
                ("evidence.paper.2", "session.paper.1", {"outcome": "rejected"}),
            )
        )
        gate = report["gates"]["paper_session"]
        self.assertEqual(gate["observed"], 0)
        self.assertEqual(gate["disqualified_subjects"], 1)
        self.assertEqual(gate["rejected_records"], 1)

    def test_a_rejection_before_acceptance_disqualifies_the_subject(self) -> None:
        # No correction record exists, so a later acceptance cannot overturn it.
        report = self.report_for(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {"outcome": "rejected"}),
                ("evidence.paper.2", "session.paper.1", {}),
            )
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertEqual(report["gates"]["paper_session"]["disqualified_subjects"], 1)

    def test_a_rejection_disqualifies_only_its_own_subject_in_its_own_gate(self) -> None:
        report = self.report_for(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {}),
                ("evidence.paper.2", "session.paper.2", {"outcome": "rejected"}),
                ("evidence.partner.1", "session.paper.1", {"evidence_type": "design_partner", "outcome": "rejected"}),
            )
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
        self.assertEqual(report["gates"]["paper_session"]["disqualified_subjects"], 0)
        self.assertEqual(report["gates"]["design_partner"]["observed"], 0)

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


class ReviewerAuthenticationTests(unittest.TestCase):
    """A record counts only if a trusted reviewer signed it (E6.4)."""

    def audit(self, records, **workspace_options) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, **workspace_options)
            workspace.write("paper", records)
            return workspace.audit()

    def assert_not_counted(self, report: dict[str, object], reason: str, evidence_id: str) -> None:
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertIn(evidence_id, report["not_counted"][reason])

    def test_a_record_signed_by_a_trusted_reviewer_counts(self) -> None:
        report = self.audit(make_chain(("evidence.paper.1", "session.paper.1", {})))
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
        self.assertEqual(report["not_counted"], {})

    def test_a_signature_from_another_key_does_not_count(self) -> None:
        records = make_chain(("evidence.paper.1", "session.paper.1", {"seed": OTHER_SEED}))
        self.assert_not_counted(self.audit(records), UNAUTHENTICATED, "evidence.paper.1")

    def test_a_key_the_set_does_not_hold_does_not_count(self) -> None:
        records = make_chain(("evidence.paper.1", "session.paper.1", {"reviewer_key_id": "reviewer.key.unknown"}))
        self.assert_not_counted(self.audit(records), UNAUTHENTICATED, "evidence.paper.1")

    def test_an_empty_set_trusts_no_one(self) -> None:
        records = make_chain(("evidence.paper.1", "session.paper.1", {}))
        self.assert_not_counted(self.audit(records, trust=False), UNAUTHENTICATED, "evidence.paper.1")

    def test_a_trusted_key_cannot_sign_for_another_reviewer(self) -> None:
        # The key is trusted for `reviewer.two`. A record naming another reviewer
        # is not that reviewer's, however valid the signature.
        records = make_chain(("evidence.paper.1", "session.paper.1", {"reviewed_by": "reviewer.three"}))
        self.assert_not_counted(self.audit(records), UNAUTHENTICATED, "evidence.paper.1")

    def test_a_signed_field_edited_and_rehashed_does_not_count(self) -> None:
        # Rewriting the ledger is not enough: the chain and hashes can be redone,
        # the signature cannot.
        (record,) = make_chain(("evidence.paper.1", "session.paper.1", {}))
        forged = resign(record, notes="Edited after review.")
        self.assert_not_counted(self.audit([forged]), UNAUTHENTICATED, "evidence.paper.1")

    def test_a_signed_record_moved_elsewhere_in_the_chain_does_not_authenticate(self) -> None:
        # The signature covers the previous hash, so a record cannot be lifted out
        # of the place the reviewer signed it and re-chained.
        _, second = make_chain(
            ("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.2", {})
        )
        # `second` was signed to follow another record. Lifted out and re-chained
        # after this one, the chain and hashes are valid, but the signature is not.
        other = make_record(ZERO_HASH, "evidence.paper.0", "session.paper.0")
        moved = resign(second, prev_hash=other["record_hash"])
        report = self.audit([other, moved])
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)
        self.assertEqual(report["not_counted"][UNAUTHENTICATED], ["evidence.paper.2"])

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
        report = self.audit(make_chain(("evidence.paper.1", "session.paper.1", {"reviewer_signature": independent})))
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)

    def test_a_forged_rejection_does_not_disqualify_a_subject(self) -> None:
        # A rejection only lowers a count, so an untrusted one could only sabotage.
        report = self.audit(
            make_chain(
                ("evidence.paper.1", "session.paper.1", {}),
                ("evidence.paper.2", "session.paper.1", {"outcome": "rejected", "seed": OTHER_SEED}),
            )
        )
        gate = report["gates"]["paper_session"]
        self.assertEqual((gate["observed"], gate["disqualified_subjects"], gate["rejected_records"]), (1, 0, 0))

    def test_a_trusted_rejection_disqualifies_regardless_of_artifact_and_release(self) -> None:
        report = self.audit(
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

    def test_the_reviewer_file_is_strict(self) -> None:
        valid = reviewers_document((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED))
        entry = valid["reviewers"][0]
        malformed = {
            "not json": "{",
            "unknown top-level field": {**valid, "extra": 1},
            "wrong schema version": {**valid, "trusted_reviewers_schema_version": 2},
            "boolean schema version": {**valid, "trusted_reviewers_schema_version": True},
            "reviewers not a list": {**valid, "reviewers": {}},
            "unknown entry field": {**valid, "reviewers": [{**entry, "extra": 1}]},
            "non-canonical key id": {**valid, "reviewers": [{**entry, "key_id": "Key One"}]},
            "short public key": {**valid, "reviewers": [{**entry, "public_key_hex": "ab"}]},
            "duplicate key": {**valid, "reviewers": [entry, entry]},
        }
        for name, document in malformed.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "reviewers.json"
                path.write_text(document if isinstance(document, str) else json.dumps(document), encoding="utf-8")
                with self.assertRaises(EvidenceError):
                    load_trusted_reviewers(path)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "reviewers.json"
            path.write_text(json.dumps(valid), encoding="utf-8")
            loaded = load_trusted_reviewers(path)
            self.assertEqual(loaded.sha256, hashlib.sha256(path.read_bytes()).hexdigest())
            self.assertEqual(set(loaded.keys), {REVIEWER_KEY_ID})
            with self.assertRaises(EvidenceError):
                load_trusted_reviewers(Path(directory) / "missing.json")


class ArtifactRetentionTests(unittest.TestCase):
    """A record's source artifact is re-hashed against the retained root (E6.4)."""

    def audit_with(self, prepare) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            prepare(workspace)
            workspace.write("paper", make_chain(("evidence.paper.1", "session.paper.1", {})))
            return workspace.audit()

    def test_a_retained_artifact_lets_its_record_count(self) -> None:
        report = self.audit_with(lambda workspace: None)
        self.assertEqual(report["gates"]["paper_session"]["observed"], 1)

    def test_a_missing_artifact_means_the_record_cannot_be_checked(self) -> None:
        report = self.audit_with(lambda workspace: (workspace.artifacts / DEFAULT_ARTIFACT_SHA256).unlink())
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertEqual(report["not_counted"][ARTIFACT_UNVERIFIED], ["evidence.paper.1"])

    def test_an_artifact_that_no_longer_matches_its_digest_does_not_count(self) -> None:
        report = self.audit_with(
            lambda workspace: (workspace.artifacts / DEFAULT_ARTIFACT_SHA256).write_bytes(b"replaced")
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
        self.assertEqual(report["not_counted"][ARTIFACT_UNVERIFIED], ["evidence.paper.1"])

    def test_a_link_in_the_artifact_root_does_not_count(self) -> None:
        def replace_with_a_link(workspace: Workspace) -> None:
            elsewhere = workspace.artifacts.parent / "elsewhere"
            elsewhere.write_bytes(DEFAULT_ARTIFACT)
            (workspace.artifacts / DEFAULT_ARTIFACT_SHA256).unlink()
            try:
                (workspace.artifacts / DEFAULT_ARTIFACT_SHA256).symlink_to(elsewhere)
            except (OSError, NotImplementedError):
                self.skipTest("cannot create a symbolic link here")

        report = self.audit_with(replace_with_a_link)
        self.assertEqual(report["gates"]["paper_session"]["observed"], 0)

    def test_retention_is_content_addressed_and_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "session.log"
            source.write_bytes(DEFAULT_ARTIFACT)
            digest = retain_artifact(source, root / "store")
            self.assertEqual(digest, DEFAULT_ARTIFACT_SHA256)
            self.assertTrue(artifact_is_retained(root / "store", digest))
            self.assertEqual(retain_artifact(source, root / "store"), digest)
            (root / "store" / digest).write_bytes(b"different")
            with self.assertRaisesRegex(EvidenceError, "different artifact"):
                retain_artifact(source, root / "store")
            with self.assertRaisesRegex(EvidenceError, "regular file"):
                retain_artifact(root, root / "store")
            self.assertFalse(artifact_is_retained(root / "store", "c" * 64))


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
        attributes = {**attributes_for(evidence_type), **changes}
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write(
                "x", make_chain(("evidence.x.1", "subject.x.1", {"evidence_type": evidence_type, "attributes": attributes}))
            )
            report = workspace.audit()
            if not report["counted_records"]:
                self.assertEqual(report["not_counted"], {CRITERIA_NOT_MET: ["evidence.x.1"]})
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

    def test_a_rejection_may_record_why_a_session_failed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            failing = {**attributes_for("paper_session"), "reconciliation_discrepancies": 2}
            workspace.write(
                "x",
                make_chain(
                    ("evidence.x.1", "session.x.1", {"outcome": "rejected", "attributes": failing}),
                ),
            )
            report = workspace.audit()
            gate = report["gates"]["paper_session"]
            self.assertEqual((gate["rejected_records"], gate["disqualified_subjects"]), (1, 0))
            self.assertEqual(report["not_counted"], {})

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
    """A reviewer can sign and append a record, and the audit counts it (E6.4)."""

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

    def test_an_appended_record_is_signed_chained_retained_and_counted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            artifact = Path(directory) / "session.log"
            artifact.write_bytes(DEFAULT_ARTIFACT)
            ledger = workspace.ledgers / "paper.acceptance.ndjson"

            first = append_record(ledger, self.template(), artifact, workspace.artifacts, REVIEWER_SEED, REVIEWER_KEY_ID)
            second = append_record(
                ledger, self.template("evidence.paper.2", "session.paper.2"), artifact, workspace.artifacts,
                REVIEWER_SEED, REVIEWER_KEY_ID,
            )

            self.assertEqual(first["prev_hash"], ZERO_HASH)
            self.assertEqual(second["prev_hash"], first["record_hash"])
            self.assertEqual(first["source_artifact_sha256"], DEFAULT_ARTIFACT_SHA256)
            report = workspace.audit()
            self.assertEqual(report["gates"]["paper_session"]["observed"], 2)
            self.assertEqual(report["not_counted"], {})

    def test_an_append_is_refused_before_anything_is_written(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            artifact = Path(directory) / "session.log"
            artifact.write_bytes(DEFAULT_ARTIFACT)
            ledger = workspace.ledgers / "paper.acceptance.ndjson"
            append_record(ledger, self.template(), artifact, workspace.artifacts, REVIEWER_SEED, REVIEWER_KEY_ID)
            before = ledger.read_bytes()

            def append(template, target=ledger, source=artifact):
                return append_record(target, template, source, workspace.artifacts, REVIEWER_SEED, REVIEWER_KEY_ID)

            refusals = {
                "a repeated evidence id": (lambda: append(self.template()), "already holds"),
                "an extra template field": (lambda: append({**self.template(), "extra": 1}), "exactly"),
                "a missing template field": (
                    lambda: append({k: v for k, v in self.template("evidence.paper.9").items() if k != "notes"}),
                    "exactly",
                ),
                "attributes that miss the shape": (
                    lambda: append({**self.template("evidence.paper.9"), "attributes": {"extra": 1}}),
                    "attributes",
                ),
                "a wrong environment": (
                    lambda: append({**self.template("evidence.paper.9"), "environment": "LIVE"}),
                    "environment",
                ),
                "a file that is not a ledger": (
                    lambda: append(self.template("evidence.paper.9"), target=workspace.ledgers / "paper.ndjson"),
                    "acceptance.ndjson",
                ),
                "an artifact that is not a file": (
                    lambda: append(self.template("evidence.paper.9"), source=Path(directory)),
                    "regular file",
                ),
            }
            for name, (attempt, message) in refusals.items():
                with self.subTest(name), self.assertRaisesRegex(EvidenceError, message):
                    attempt()
            self.assertEqual(ledger.read_bytes(), before)
            self.assertFalse((workspace.ledgers / "paper.ndjson").exists())

    def test_a_tampered_ledger_refuses_further_appends(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            artifact = Path(directory) / "session.log"
            artifact.write_bytes(DEFAULT_ARTIFACT)
            ledger = workspace.ledgers / "paper.acceptance.ndjson"
            append_record(ledger, self.template(), artifact, workspace.artifacts, REVIEWER_SEED, REVIEWER_KEY_ID)
            ledger.write_text(ledger.read_text(encoding="utf-8").replace("Independently", "Casually"), encoding="utf-8")
            with self.assertRaisesRegex(EvidenceError, "hash does not match"):
                append_record(
                    ledger, self.template("evidence.paper.2", "session.paper.2"), artifact,
                    workspace.artifacts, REVIEWER_SEED, REVIEWER_KEY_ID,
                )

    def test_the_command_line_signs_appends_and_audits(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            root = Path(directory)
            artifact = root / "session.log"
            artifact.write_bytes(DEFAULT_ARTIFACT)
            key = root / "reviewer.pk8"
            key.write_bytes(pkcs8(REVIEWER_SEED))
            template = root / "record.json"
            template.write_text(json.dumps(self.template()), encoding="utf-8")
            ledger = workspace.ledgers / "paper.acceptance.ndjson"

            output = StringIO()
            with redirect_stdout(output):
                code = main([
                    "append", str(ledger), "--record", str(template), "--artifact", str(artifact),
                    "--artifact-root", str(workspace.artifacts), "--reviewer-key", str(key),
                    "--reviewer-key-id", REVIEWER_KEY_ID,
                ])
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
                self.assertEqual(
                    main([
                        "append", str(ledger), "--record", str(template), "--artifact", str(artifact),
                        "--artifact-root", str(workspace.artifacts), "--reviewer-key", str(key),
                        "--reviewer-key-id", REVIEWER_KEY_ID,
                    ]),
                    2,
                )

    def test_the_command_line_refuses_a_bad_key_or_template_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            root = Path(directory)
            artifact = root / "session.log"
            artifact.write_bytes(DEFAULT_ARTIFACT)
            key = root / "reviewer.pk8"
            key.write_bytes(pkcs8(REVIEWER_SEED))
            template = root / "record.json"
            template.write_text(json.dumps(self.template()), encoding="utf-8")
            ledger = workspace.ledgers / "paper.acceptance.ndjson"

            def append(key_path: Path, template_path: Path) -> tuple[int, str]:
                errors = StringIO()
                with redirect_stderr(errors):
                    code = main([
                        "append", str(ledger), "--record", str(template_path), "--artifact", str(artifact),
                        "--artifact-root", str(workspace.artifacts), "--reviewer-key", str(key_path),
                        "--reviewer-key-id", REVIEWER_KEY_ID,
                    ])
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


class PipelineAcceptanceRootTests(unittest.TestCase):
    """The evidence pipeline counts only the operational ledger root (E6.1)."""

    def publish(self, var_dir: Path) -> dict[str, object]:
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

    def test_a_ledger_nobody_trusts_verifies_but_counts_nothing(self) -> None:
        # The pipeline provisions an empty reviewer set, so no signature counts
        # until an operator supplies their own (E6.4).
        with tempfile.TemporaryDirectory() as directory:
            var_dir = Path(directory)
            root = var_dir / generate_pipeline_evidence.ACCEPTANCE_LEDGER_DIRECTORY
            root.mkdir()
            write_ledger(
                root / "paper.acceptance.ndjson",
                [make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")],
            )

            report = self.publish(var_dir)

            self.assertEqual(report["verified_records"], 1)
            self.assertEqual(report["counted_records"], 0)
            self.assertEqual(report["gates"]["paper_session"]["observed"], 0)
            self.assertIn("evidence.paper.1", report["not_counted"][UNAUTHENTICATED])

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
            (store / DEFAULT_ARTIFACT_SHA256).write_bytes(DEFAULT_ARTIFACT)
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
