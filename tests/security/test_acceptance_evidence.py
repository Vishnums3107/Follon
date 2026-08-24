from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools.acceptance_evidence import ZERO_HASH, load_ledgers, record_hash, status


class AcceptanceEvidenceTests(unittest.TestCase):
    def record(self, previous: str, evidence_id: str, subject_id: str) -> dict[str, object]:
        record: dict[str, object] = {
            "acceptance_evidence_schema_version": 1,
            "evidence_id": evidence_id,
            "evidence_type": "paper_session",
            "subject_id": subject_id,
            "occurred_at": "2026-08-24T10:00:00Z",
            "observed_by": "operator.one",
            "reviewed_by": "reviewer.two",
            "source_artifact_sha256": "a" * 64,
            "outcome": "accepted",
            "notes": "Clean independently reviewed PAPER session.",
            "prev_hash": previous,
            "record_hash": "0" * 64,
        }
        record["record_hash"] = record_hash(record)
        return record

    def test_chain_and_unique_subject_counts_are_verified(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = self.record(ZERO_HASH, "evidence.paper.1", "session.paper.1")
            second = self.record(str(first["record_hash"]), "evidence.paper.2", "session.paper.2")
            ledger = root / "paper.acceptance.ndjson"
            ledger.write_text("\n".join(map(lambda value: json.dumps(value, sort_keys=True, separators=(",", ":")), [first, second])) + "\n", encoding="utf-8")
            report = status(load_ledgers(root))
            self.assertEqual(report["gates"]["paper_session"]["observed"], 2)
            self.assertFalse(report["all_gates_eligible"])

    def test_tampering_and_duplicate_ids_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            record = self.record(ZERO_HASH, "evidence.paper.1", "session.paper.1")
            record["notes"] = "tampered"
            (root / "paper.acceptance.ndjson").write_text(json.dumps(record) + "\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                load_ledgers(root)


if __name__ == "__main__":
    unittest.main()
