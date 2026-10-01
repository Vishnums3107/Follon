"""A ledger anchor detects records deleted from a ledger's tail (E6.6b finding 8).

A hash chain verifies whatever prefix of it remains, so deleting the newest records of a
ledger left an audit that passed. Deleting a trailing rejection requalified its subject.
An anchor, kept outside the ledger root, records each ledger's record count and chain
head; an audit given one fails unless the root still holds what it recorded.
"""

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
    ANCHORED_AT,
    OTHER_SEED,
    RELEASE_ID,
    REVIEWER_ID,
    REVIEWER_KEY_ID,
    Workspace,
    links_supported,
    make_chain,
    write_ledger,
)
from tools import acceptance_evidence
from tools.acceptance_evidence import ZERO_HASH, EvidenceError, anchor_root, main


def chain_of(count: int, prefix: str = "paper", **keywords: object) -> list[dict[str, object]]:
    return make_chain(*[(f"evidence.{prefix}.{index}", f"session.{prefix}.{index}", keywords) for index in range(count)])


def drop_last_line(path: Path) -> None:
    path.write_bytes(b"".join(path.read_bytes().splitlines(keepends=True)[:-1]))


class AnchorWritingTests(unittest.TestCase):
    def test_an_anchor_records_every_ledgers_count_and_head(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            (workspace.ledgers / "partners").mkdir()
            paper = chain_of(2)
            partner = make_chain(("evidence.partner.1", "partner.1", {"evidence_type": "design_partner"}))
            workspace.write("paper", paper)
            workspace.write("partners/partner", partner)
            workspace.write("empty", [])

            anchor = workspace.anchor()

            data = anchor.read_bytes()
            document = json.loads(data)
            # The exact bytes `anchor` writes: canonical JSON and one newline.
            self.assertEqual(data, acceptance_evidence.canonical_json(document) + b"\n")
            self.assertEqual(
                document,
                {
                    "acceptance_ledger_anchor_schema_version": 1,
                    "anchored_at": ANCHORED_AT,
                    "trusted_reviewers_sha256": hashlib.sha256(workspace.reviewers.read_bytes()).hexdigest(),
                    "ledgers": [
                        {"path": "empty.acceptance.ndjson", "records": 0, "head": ZERO_HASH},
                        {"path": "paper.acceptance.ndjson", "records": 2, "head": paper[-1]["record_hash"]},
                        {
                            "path": "partners/partner.acceptance.ndjson",
                            "records": 1,
                            "head": partner[-1]["record_hash"],
                        },
                    ],
                },
            )

    def test_an_anchor_without_a_time_is_stamped_now_in_the_canonical_form(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            document = anchor_root(workspace.ledgers, workspace.reviewers, workspace.anchors / "now.json")
            acceptance_evidence.validate_timestamp(document["anchored_at"], "anchored_at")

    def test_an_anchor_is_never_kept_inside_the_ledger_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", chain_of(1))
            for inside in (workspace.ledgers / "anchor.json", workspace.ledgers / "nested" / "anchor.json", workspace.ledgers):
                with self.subTest(inside=inside.name), self.assertRaisesRegex(EvidenceError, "outside the ledger root"):
                    anchor_root(workspace.ledgers, workspace.reviewers, inside, anchored_at=ANCHORED_AT)
            self.assertEqual(sorted(path.name for path in workspace.ledgers.iterdir()), ["paper.acceptance.ndjson"])

    def test_an_anchor_never_overwrites_a_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            target = workspace.anchors / "anchor.json"
            target.write_bytes(b"an earlier anchor")
            with self.assertRaisesRegex(EvidenceError, "never overwritten"):
                anchor_root(workspace.ledgers, workspace.reviewers, target, anchored_at=ANCHORED_AT)
            self.assertEqual(target.read_bytes(), b"an earlier anchor")
            self.assertEqual([path.name for path in workspace.anchors.iterdir()], ["anchor.json"])

    def test_a_name_that_appears_during_publication_is_not_replaced(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            target = workspace.anchors / "anchor.json"

            def another_writer(source: str, destination: Path) -> None:
                target.write_bytes(b"another writer's anchor")
                raise FileExistsError(destination)

            with mock.patch.object(acceptance_evidence.os, "link", side_effect=another_writer):
                with self.assertRaisesRegex(EvidenceError, "never overwritten"):
                    anchor_root(workspace.ledgers, workspace.reviewers, target, anchored_at=ANCHORED_AT)
            self.assertEqual(target.read_bytes(), b"another writer's anchor")
            self.assertEqual([path.name for path in workspace.anchors.iterdir()], ["anchor.json"])

    def test_a_root_that_does_not_verify_is_never_anchored(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", chain_of(1, seed=OTHER_SEED))
            target = workspace.anchors / "anchor.json"
            with self.assertRaisesRegex(EvidenceError, "no listed reviewer key signed"):
                anchor_root(workspace.ledgers, workspace.reviewers, target, anchored_at=ANCHORED_AT)
            ledger = workspace.ledgers / "paper.acceptance.ndjson"
            workspace.write("paper", chain_of(1))
            ledger.write_bytes(ledger.read_bytes().replace(b"Independently", b"Dependently"))
            with self.assertRaisesRegex(EvidenceError, "hash does not match"):
                anchor_root(workspace.ledgers, workspace.reviewers, target, anchored_at=ANCHORED_AT)
            self.assertFalse(target.exists())


class AnchorCheckTests(unittest.TestCase):
    def test_an_anchor_of_the_root_as_it_stands_covers_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", chain_of(2))
            anchor = workspace.anchor()
            report = workspace.audit(anchors=(anchor,))
            self.assertEqual(
                report["ledger_anchors"],
                [{
                    "sha256": hashlib.sha256(anchor.read_bytes()).hexdigest(),
                    "anchored_at": ANCHORED_AT,
                    "ledgers": 1,
                    "covers_root": True,
                }],
            )
            self.assertEqual(report["gates"]["paper_session"]["observed"], 2)

    def test_a_deleted_tail_passes_alone_and_fails_against_its_anchor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            path = workspace.write("paper", chain_of(3))
            anchor = workspace.anchor()
            drop_last_line(path)
            # The gap: what is left is still a valid chain.
            self.assertEqual(workspace.audit()["gates"]["paper_session"]["observed"], 2)
            with self.assertRaisesRegex(EvidenceError, "holds 2 records where its anchor holds 3"):
                workspace.audit(anchors=(anchor,))

    def test_a_deleted_rejection_cannot_requalify_its_subject(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            path = workspace.write(
                "paper",
                make_chain(
                    ("evidence.paper.1", "session.paper.1", {}),
                    ("evidence.paper.2", "session.paper.1", {"outcome": "rejected"}),
                ),
            )
            anchor = workspace.anchor()
            self.assertEqual(workspace.audit(anchors=(anchor,))["gates"]["paper_session"]["observed"], 0)
            drop_last_line(path)
            self.assertEqual(workspace.audit()["gates"]["paper_session"]["observed"], 1)
            with self.assertRaisesRegex(EvidenceError, "deleted from its tail"):
                workspace.audit(anchors=(anchor,))

    def test_a_deleted_ledger_fails_against_its_anchor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", chain_of(1))
            partner = workspace.write("partner", make_chain(("evidence.p.1", "partner.1", {"evidence_type": "design_partner"})))
            anchor = workspace.anchor()
            partner.unlink()
            with self.assertRaisesRegex(EvidenceError, "anchored ledger partner.acceptance.ndjson is missing"):
                workspace.audit(anchors=(anchor,))

    def test_an_emptied_ledger_and_a_deleted_empty_one_fail_against_their_anchor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            path = workspace.write("paper", chain_of(1))
            empty = workspace.write("empty", [])
            anchor = workspace.anchor()
            path.write_bytes(b"")
            with self.assertRaisesRegex(EvidenceError, "holds 0 records where its anchor holds 1"):
                workspace.audit(anchors=(anchor,))
            workspace.write("paper", chain_of(1))
            empty.unlink()
            with self.assertRaisesRegex(EvidenceError, "anchored ledger empty.acceptance.ndjson is missing"):
                workspace.audit(anchors=(anchor,))

    def test_a_rewritten_prefix_fails_against_its_anchor(self) -> None:
        # Someone holding a listed key can sign a different chain of the same length.
        # Each record's hash covers its predecessor's, so the anchored head is gone.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            path = workspace.write("paper", chain_of(2))
            anchor = workspace.anchor()
            rewritten = chain_of(2, notes="A different review of the same sessions.")
            write_ledger(path, rewritten)
            self.assertEqual(workspace.audit()["verified_records"], 2)
            with self.assertRaisesRegex(EvidenceError, "no longer extends the chain head its anchor holds"):
                workspace.audit(anchors=(anchor,))
            # A rewrite that also appends cannot hide behind a longer ledger either.
            write_ledger(path, chain_of(3, notes="A different review of the same sessions."))
            with self.assertRaisesRegex(EvidenceError, "no longer extends"):
                workspace.audit(anchors=(anchor,))

    def test_records_appended_after_an_anchor_pass_but_the_anchor_no_longer_covers_the_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", chain_of(2))
            anchor = workspace.anchor()
            workspace.write("paper", chain_of(3))
            report = workspace.audit(anchors=(anchor,))
            self.assertEqual([entry["covers_root"] for entry in report["ledger_anchors"]], [False])
            self.assertEqual(report["gates"]["paper_session"]["observed"], 3)
            # A new ledger is outside the anchor too.
            workspace.write("paper", chain_of(2))
            workspace.write("partner", make_chain(("evidence.p.1", "partner.1", {"evidence_type": "design_partner"})))
            report = workspace.audit(anchors=(anchor,))
            self.assertEqual([entry["covers_root"] for entry in report["ledger_anchors"]], [False])
            # A later anchor covers both, and both are reported in the order given.
            later = workspace.anchor()
            report = workspace.audit(anchors=(anchor, later))
            self.assertEqual([entry["covers_root"] for entry in report["ledger_anchors"]], [False, True])

    def test_every_anchor_given_must_hold(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            path = workspace.write("paper", chain_of(3))
            full = workspace.anchor()
            drop_last_line(path)
            shorter = workspace.anchor()
            self.assertEqual(workspace.audit(anchors=(shorter,))["ledger_anchors"][0]["covers_root"], True)
            with self.assertRaisesRegex(EvidenceError, "deleted from its tail"):
                workspace.audit(anchors=(shorter, full))

    def test_an_anchor_inside_the_ledger_root_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", chain_of(1))
            inside = workspace.ledgers / "anchor.json"
            inside.write_bytes(workspace.anchor().read_bytes())
            with self.assertRaisesRegex(EvidenceError, "outside the ledger root"):
                workspace.audit(anchors=(inside,))

    @unittest.skipUnless(links_supported(), "cannot create a symbolic link here")
    def test_a_linked_anchor_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            anchor = workspace.anchor()
            link = workspace.anchors / "linked.json"
            link.symlink_to(anchor)
            with self.assertRaisesRegex(EvidenceError, "linked or not a regular file"):
                workspace.audit(anchors=(link,))


class AnchorContractTests(unittest.TestCase):
    """An anchor must be exactly the document `anchor` writes; anything else is refused."""

    def test_a_malformed_anchor_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", chain_of(2))
            workspace.write("partner", make_chain(("evidence.p.1", "partner.1", {"evidence_type": "design_partner"})))
            valid = json.loads(workspace.anchor().read_bytes())
            paper, partner = valid["ledgers"]
            cases: dict[str, object] = {
                "a list": [valid],
                "version 2": {**valid, "acceptance_ledger_anchor_schema_version": 2},
                "a boolean version": {**valid, "acceptance_ledger_anchor_schema_version": True},
                "an extra field": {**valid, "extra": 1},
                "a missing field": {key: value for key, value in valid.items() if key != "anchored_at"},
                "a time that is not canonical": {**valid, "anchored_at": "2026-08-25 10:00:00Z"},
                "a time that is not real": {**valid, "anchored_at": "2026-02-30T10:00:00Z"},
                "a malformed reviewer digest": {**valid, "trusted_reviewers_sha256": "A" * 64},
                "ledgers that are not a list": {**valid, "ledgers": {"paper": paper}},
                "an entry field too many": {**valid, "ledgers": [{**paper, "extra": 1}, partner]},
                "an entry field missing": {**valid, "ledgers": [{"path": paper["path"], "records": 2}, partner]},
                "a path that is not a ledger": {**valid, "ledgers": [{**paper, "path": "paper.json"}, partner]},
                "a negative count": {**valid, "ledgers": [{**paper, "records": -1}, partner]},
                "a boolean count": {**valid, "ledgers": [{**paper, "records": True}, partner]},
                "a malformed head": {**valid, "ledgers": [{**paper, "head": "not a hash"}, partner]},
                "records under the zero head": {**valid, "ledgers": [{**paper, "head": ZERO_HASH}, partner]},
                "no records under a real head": {**valid, "ledgers": [{**paper, "records": 0}, partner]},
                "one ledger twice": {**valid, "ledgers": [paper, paper, partner]},
                "ledgers out of order": {**valid, "ledgers": [partner, paper]},
            }
            target = workspace.anchors / "malformed.json"
            for name, document in cases.items():
                with self.subTest(refused=name):
                    target.write_text(json.dumps(document), encoding="utf-8")
                    with self.assertRaises(EvidenceError):
                        workspace.audit(anchors=(target,))
            for name, content in {"not JSON": b"{", "too large": b" " * (acceptance_evidence.MAX_ANCHOR_BYTES + 1)}.items():
                with self.subTest(refused=name):
                    target.write_bytes(content)
                    with self.assertRaises(EvidenceError):
                        workspace.audit(anchors=(target,))
            # The well-formed anchor, rewritten as the tool writes it, passes, so each
            # refusal above was about its change and not about the harness.
            target.write_text(json.dumps(valid), encoding="utf-8")
            self.assertTrue(workspace.audit(anchors=(target,))["ledger_anchors"][0]["covers_root"])

    def test_an_absent_anchor_is_an_error_not_a_pass(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            with self.assertRaisesRegex(EvidenceError, "missing, linked or not a regular file"):
                workspace.audit(anchors=(workspace.anchors / "absent.json",))


class AnchorCommandLineTests(unittest.TestCase):
    def test_the_command_line_anchors_and_audits_against_an_anchor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            path = workspace.write("paper", chain_of(2))
            anchor = workspace.anchors / "anchor.json"
            output = StringIO()
            with redirect_stdout(output):
                code = main(["anchor", str(workspace.ledgers), "--trusted-reviewers", str(workspace.reviewers), "--output", str(anchor)])
            self.assertEqual(code, 0)
            self.assertIn("anchored 1 ledgers, 2 records", output.getvalue())

            audit_command = [
                "audit", str(workspace.ledgers), "--trusted-reviewers", str(workspace.reviewers),
                "--artifact-root", str(workspace.artifacts), "--release-id", RELEASE_ID,
                "--anchor", str(anchor),
            ]
            output = StringIO()
            with redirect_stdout(output):
                self.assertEqual(main(audit_command), 0)
            self.assertTrue(json.loads(output.getvalue())["ledger_anchors"][0]["covers_root"])

            drop_last_line(path)
            errors = StringIO()
            with redirect_stderr(errors), redirect_stdout(StringIO()):
                self.assertEqual(main(audit_command), 2)
            self.assertIn("deleted from its tail", errors.getvalue())

            # A second anchor to the same name is refused, and exits 2.
            errors = StringIO()
            with redirect_stderr(errors):
                code = main(["anchor", str(workspace.ledgers), "--trusted-reviewers", str(workspace.reviewers), "--output", str(anchor)])
            self.assertEqual(code, 2)
            self.assertIn("never overwritten", errors.getvalue())

    def test_the_anchor_command_requires_its_reviewer_set_and_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            full = ["anchor", str(workspace.ledgers), "--trusted-reviewers", str(workspace.reviewers), "--output", "a.json"]
            for option in ("--trusted-reviewers", "--output"):
                index = full.index(option)
                with self.subTest(option), redirect_stderr(StringIO()), self.assertRaises(SystemExit):
                    main(full[:index] + full[index + 2:])


if __name__ == "__main__":
    unittest.main()
