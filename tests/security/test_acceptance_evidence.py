from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import generate_pipeline_evidence
from tools.acceptance_evidence import ZERO_HASH, EvidenceError, load_ledgers, record_hash, status


def make_record(
    previous: str,
    evidence_id: str,
    subject_id: str,
    *,
    evidence_type: str = "paper_session",
    outcome: str = "accepted",
    **overrides: object,
) -> dict[str, object]:
    record: dict[str, object] = {
        "acceptance_evidence_schema_version": 1,
        "evidence_id": evidence_id,
        "evidence_type": evidence_type,
        "subject_id": subject_id,
        "occurred_at": "2026-08-24T10:00:00Z",
        "observed_by": "operator.one",
        "reviewed_by": "reviewer.two",
        "source_artifact_sha256": "a" * 64,
        "outcome": outcome,
        "notes": "Clean independently reviewed PAPER session.",
        "prev_hash": previous,
        "record_hash": "0" * 64,
    }
    record.update(overrides)
    record["record_hash"] = record_hash(record)
    return record


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


class AcceptanceEvidenceTests(unittest.TestCase):
    def report_for(self, records: list[dict[str, object]]) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_ledger(root / "paper.acceptance.ndjson", records)
            return status(load_ledgers(root))

    def refused(self, record: dict[str, object]) -> str:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_ledger(root / "paper.acceptance.ndjson", [record])
            with self.assertRaises(EvidenceError) as caught:
                load_ledgers(root)
            return str(caught.exception)

    def test_chain_and_unique_subject_counts_are_verified(self) -> None:
        report = self.report_for(
            make_chain(("evidence.paper.1", "session.paper.1", {}), ("evidence.paper.2", "session.paper.2", {}))
        )
        self.assertEqual(report["gates"]["paper_session"]["observed"], 2)
        self.assertFalse(report["all_gates_eligible"])

    def test_tampering_fails_closed(self) -> None:
        record = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")
        record["notes"] = "tampered"
        self.assertIn("hash does not match", self.refused(record))

    def test_duplicate_evidence_ids_fail_closed(self) -> None:
        # The test above was once named for duplicates too, but only ever
        # tampered with a record.
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

    def test_the_schema_version_must_be_the_integer_one(self) -> None:
        # JSON `true` equals 1 in Python and passed an equality check.
        record = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1", acceptance_evidence_schema_version=True)
        self.assertIn("schema version", self.refused(record))
        record = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1", acceptance_evidence_schema_version=1.0)
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


class PipelineAcceptanceRootTests(unittest.TestCase):
    """The evidence pipeline counts only the operational ledger root (E6.1)."""

    def publish(self, var_dir: Path) -> dict[str, object]:
        target = generate_pipeline_evidence.publish_acceptance_status(var_dir)
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

    def test_a_ledger_under_the_operational_root_is_counted(self) -> None:
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
            self.assertEqual(report["gates"]["paper_session"]["observed"], 1)

    def test_an_absent_operational_root_is_created_empty(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            var_dir = Path(directory)

            report = self.publish(var_dir)

            self.assertTrue((var_dir / generate_pipeline_evidence.ACCEPTANCE_LEDGER_DIRECTORY).is_dir())
            self.assertEqual(report["verified_records"], 0)
            self.assertFalse(report["all_gates_eligible"])


if __name__ == "__main__":
    unittest.main()
