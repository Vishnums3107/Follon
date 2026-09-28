from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path

from tools.acceptance_evidence import EVIDENCE_TARGETS, ZERO_HASH, record_hash
from tools.release_promotion_gate import PromotionError, acceptance_ready, main, promotion_receipt, validate_approval

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def ledger_lines(evidence_type: str, count: int) -> str:
    """A valid chain of `count` accepted records for one gate. These are unit
    fixtures in a temporary directory, never operational evidence."""
    lines = []
    previous = ZERO_HASH
    for index in range(count):
        record: dict[str, object] = {
            "acceptance_evidence_schema_version": 1,
            "evidence_id": f"evidence.{evidence_type}.{index}",
            "evidence_type": evidence_type,
            "subject_id": f"subject.{evidence_type}.{index}",
            "occurred_at": "2026-08-24T10:00:00Z",
            "observed_by": "operator.one",
            "reviewed_by": "reviewer.two",
            "source_artifact_sha256": "a" * 64,
            "outcome": "accepted",
            "notes": "Unit fixture only.",
            "prev_hash": previous,
            "record_hash": "0" * 64,
        }
        record["record_hash"] = record_hash(record)
        previous = str(record["record_hash"])
        lines.append(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n")
    return "".join(lines)


def write_every_gate(root: Path, short_by: dict[str, int] | None = None) -> None:
    for evidence_type, required in EVIDENCE_TARGETS.items():
        count = required - (short_by or {}).get(evidence_type, 0)
        (root / f"{evidence_type}.acceptance.ndjson").write_text(
            ledger_lines(evidence_type, count), encoding="utf-8", newline="\n"
        )


class ReleasePromotionGateTests(unittest.TestCase):
    def test_only_ordered_transitions_with_distinct_approver_are_allowed(self) -> None:
        validate_approval("development", "staging", "user.release", "user.approver", "change.1")
        validate_approval("staging", "production", "user.release", "user.approver", "change.2")
        with self.assertRaises(PromotionError):
            validate_approval("development", "production", "user.release", "user.approver", "change.3")
        with self.assertRaises(PromotionError):
            validate_approval("staging", "production", "user.same", "user.same", "change.4")

    def test_production_is_refused_while_any_gate_is_open(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(PromotionError, "open acceptance gates"):
                acceptance_ready(REPOSITORY_ROOT, Path(directory), "production")

    def test_a_status_document_is_not_an_input(self) -> None:
        # A caller-authored status declaring every gate eligible passed the
        # old subcheck for production. The gate now counts ledgers itself, so
        # such a document, even inside the ledger root, changes nothing.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for version in (1, 2):
                (root / f"acceptance-status-v{version}.json").write_text(
                    json.dumps({"acceptance_status_schema_version": version, "all_gates_eligible": True}),
                    encoding="utf-8",
                )
            with self.assertRaisesRegex(PromotionError, "open acceptance gates"):
                acceptance_ready(REPOSITORY_ROOT, root, "production")
        # Every required argument is present, and the approval would fail
        # before anything runs, so only an unrecognized --acceptance-status
        # can make the parser exit here.
        arguments = [
            "--source-environment", "staging", "--target-environment", "production",
            "--manifest", "m.json", "--signature", "s.json", "--trusted-key", "k.json",
            "--artifacts-root", ".", "--acceptance-ledger-root", ".",
            "--requester", "user.same", "--approver", "user.same",
            "--change-ticket", "change.1", "--receipt", "r.json",
        ]
        with redirect_stderr(StringIO()):
            self.assertEqual(main(arguments), 2)
            with self.assertRaises(SystemExit):
                main([*arguments, "--acceptance-status", "status.json"])

    def test_production_is_eligible_only_when_the_ledgers_meet_every_gate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_every_gate(root)
            status, _ = acceptance_ready(REPOSITORY_ROOT, root, "production")
            self.assertTrue(status["all_gates_eligible"])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_every_gate(root, short_by={"paper_session": 1})
            with self.assertRaisesRegex(PromotionError, "open acceptance gates"):
                acceptance_ready(REPOSITORY_ROOT, root, "production")

    def test_a_tampered_ledger_blocks_promotion_everywhere(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_every_gate(root)
            ledger = root / "paper_session.acceptance.ndjson"
            ledger.write_text(ledger.read_text(encoding="utf-8").replace("Unit fixture only.", "Edited."),
                              encoding="utf-8", newline="\n")
            for environment in ("staging", "production"):
                with self.assertRaisesRegex(PromotionError, "failed verification"):
                    acceptance_ready(REPOSITORY_ROOT, root, environment)

    def test_staging_needs_verifiable_ledgers_but_not_eligibility(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            status, _ = acceptance_ready(REPOSITORY_ROOT, Path(directory), "staging")
            self.assertFalse(status["all_gates_eligible"])

    def test_the_receipt_binds_the_status_and_every_ledger_counted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_every_gate(root)
            status, encoded = acceptance_ready(REPOSITORY_ROOT, root, "production")
            for name in ("manifest", "signature", "trusted-key"):
                (root / f"{name}.json").write_text("{}", encoding="utf-8")
            receipt = promotion_receipt(
                source_environment="staging",
                target_environment="production",
                manifest=root / "manifest.json",
                signature=root / "signature.json",
                trusted_key=root / "trusted-key.json",
                acceptance_status=status,
                acceptance_status_bytes=encoded,
                requester="user.release",
                approver="user.approver",
                change_ticket="change.1",
                promoted_at="2026-09-28T10:00:00Z",
            )
            self.assertEqual(receipt["release_promotion_receipt_schema_version"], 2)
            self.assertEqual(receipt["acceptance_status_sha256"], hashlib.sha256(encoded).hexdigest())
            bound = {ledger["path"]: ledger["sha256"] for ledger in receipt["acceptance_ledgers"]}
            self.assertEqual(
                bound,
                {
                    path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                    for path in sorted(root.glob("*.acceptance.ndjson"))
                },
            )


if __name__ == "__main__":
    unittest.main()
