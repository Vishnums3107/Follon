from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import generate_pipeline_evidence
from tools.acceptance_evidence import ZERO_HASH, load_ledgers, record_hash, status


def make_record(
    previous: str,
    evidence_id: str,
    subject_id: str,
    *,
    evidence_type: str = "paper_session",
    outcome: str = "accepted",
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
    record["record_hash"] = record_hash(record)
    return record


def write_ledger(path: Path, records: list[dict[str, object]]) -> None:
    path.write_text(
        "".join(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n" for record in records),
        encoding="utf-8",
        newline="\n",
    )


class AcceptanceEvidenceTests(unittest.TestCase):
    def test_chain_and_unique_subject_counts_are_verified(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")
            second = make_record(str(first["record_hash"]), "evidence.paper.2", "session.paper.2")
            write_ledger(root / "paper.acceptance.ndjson", [first, second])
            report = status(load_ledgers(root))
            self.assertEqual(report["gates"]["paper_session"]["observed"], 2)
            self.assertFalse(report["all_gates_eligible"])

    def test_tampering_and_duplicate_ids_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            record = make_record(ZERO_HASH, "evidence.paper.1", "session.paper.1")
            record["notes"] = "tampered"
            (root / "paper.acceptance.ndjson").write_text(json.dumps(record) + "\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                load_ledgers(root)


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
