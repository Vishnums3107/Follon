from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from acceptance_fixtures import ANCHORED_AT, RELEASE_ID, Workspace, links_supported, make_chain
from tools import release_promotion_gate
from tools.acceptance_evidence import GATES
from tools.release_promotion_gate import (
    PromotionError,
    VerifiedRelease,
    acceptance_ready,
    main,
    promotion_receipt,
    release_id_of,
    validate_approval,
    verify_release,
    write_receipt,
)

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def write_every_gate(workspace: Workspace, short_by: dict[str, int] | None = None, **keywords: object) -> None:
    """A valid chain of accepted records for every gate, just enough to open it. These
    are unit fixtures in a temporary directory, never operational evidence."""
    for evidence_type, alternatives in GATES.items():
        kind, required = alternatives[0]
        count = required - (short_by or {}).get(evidence_type, 0)
        workspace.write(
            evidence_type,
            make_chain(*[
                (
                    f"evidence.{evidence_type}.{index}",
                    f"subject.{evidence_type}.{index}",
                    {"evidence_type": evidence_type, "customer_kind": kind or "professional", **keywords},
                )
                for index in range(count)
            ]),
        )


def ready(workspace: Workspace, target: str, release_id: str = RELEASE_ID, anchors: list[Path] | None = None):
    """Runs the acceptance subcheck. Production is given an anchor of the root as it
    now stands unless `anchors` says otherwise, since it refuses to run without one."""
    if anchors is None:
        anchors = [workspace.anchor()] if target == "production" else []
    return acceptance_ready(
        REPOSITORY_ROOT,
        workspace.ledgers,
        target,
        trusted_reviewers=workspace.reviewers,
        artifact_root=workspace.artifacts,
        release_id=release_id,
        ledger_anchors=anchors,
    )


def audit_response(release_id: str = RELEASE_ID, **changes: object) -> SimpleNamespace:
    """A well-formed acceptance audit response, as the tool's process returns it."""
    status = {
        "acceptance_status_schema_version": 5,
        "release_id": release_id,
        "trusted_reviewers_sha256": "a" * 64,
        "all_gates_eligible": False,
        "ledgers": [],
        "ledger_anchors": [],
        **changes,
    }
    return SimpleNamespace(returncode=0, stdout=json.dumps(status).encode(), stderr=b"")


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
                ready(Workspace(directory), "production")

    def test_a_status_document_is_not_an_input(self) -> None:
        # A caller-authored status declaring every gate eligible passed the
        # old subcheck for production. The gate now counts ledgers itself, so
        # such a document, even inside the ledger root, changes nothing.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            for version in (1, 2, 3, 4):
                (workspace.ledgers / f"acceptance-status-v{version}.json").write_text(
                    json.dumps({"acceptance_status_schema_version": version, "all_gates_eligible": True}),
                    encoding="utf-8",
                )
            with self.assertRaisesRegex(PromotionError, "open acceptance gates"):
                ready(workspace, "production")
        # Every required argument is present, and the approval would fail
        # before anything runs, so only an unrecognized --acceptance-status
        # can make the parser exit here.
        arguments = [
            "--source-environment", "staging", "--target-environment", "production",
            "--manifest", "m.json", "--signature", "s.json", "--trusted-key", "k.json",
            "--artifacts-root", ".", "--acceptance-ledger-root", ".",
            "--acceptance-trusted-reviewers", "r.json", "--acceptance-artifact-root", ".",
            "--requester", "user.same", "--approver", "user.same",
            "--change-ticket", "change.1", "--receipt", "r.json",
        ]
        with redirect_stderr(StringIO()):
            self.assertEqual(main(arguments), 2)
            with self.assertRaises(SystemExit):
                main([*arguments, "--acceptance-status", "status.json"])

    def test_the_gate_cannot_be_run_without_a_reviewer_set_or_an_artifact_root(self) -> None:
        arguments = [
            "--source-environment", "staging", "--target-environment", "production",
            "--manifest", "m.json", "--signature", "s.json", "--trusted-key", "k.json",
            "--artifacts-root", ".", "--acceptance-ledger-root", ".",
            "--requester", "user.release", "--approver", "user.approver",
            "--change-ticket", "change.1", "--receipt", "r.json",
        ]
        with redirect_stderr(StringIO()):
            with self.assertRaises(SystemExit):
                main(arguments)
            with self.assertRaises(SystemExit):
                main([*arguments, "--acceptance-trusted-reviewers", "r.json"])
            with self.assertRaises(SystemExit):
                main([*arguments, "--acceptance-artifact-root", "."])

    def test_production_is_eligible_only_when_the_ledgers_meet_every_gate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace)
            status, _ = ready(workspace, "production")
            self.assertTrue(status["all_gates_eligible"])
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace, short_by={"paper_session": 1})
            with self.assertRaisesRegex(PromotionError, "open acceptance gates"):
                ready(workspace, "production")

    def test_evidence_no_listed_key_signed_fails_verification(self) -> None:
        # Every record must be signed by a listed key, so a set that lists none
        # fails the audit for staging too, not only production (E6.6b). The root
        # was anchored while its reviewer was still listed.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace)
            anchor = workspace.anchor()
            workspace.trust()
            for environment in ("staging", "production"):
                with self.assertRaisesRegex(PromotionError, "failed verification.*no listed reviewer key"):
                    ready(workspace, environment, anchors=[anchor])

    def test_evidence_without_its_retained_artifact_does_not_open_a_gate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, retain=False)
            write_every_gate(workspace)
            with self.assertRaisesRegex(PromotionError, "open acceptance gates"):
                ready(workspace, "production")

    def test_evidence_for_another_release_does_not_open_this_ones_gates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace)
            status, _ = ready(workspace, "production")
            self.assertTrue(status["all_gates_eligible"])
            with self.assertRaisesRegex(PromotionError, "open acceptance gates"):
                ready(workspace, "production", release_id="release.unit.002")

    def test_a_tampered_ledger_blocks_promotion_everywhere(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace)
            anchor = workspace.anchor()
            ledger = workspace.ledgers / "paper_session.acceptance.ndjson"
            ledger.write_text(
                ledger.read_text(encoding="utf-8").replace("Independently reviewed", "Edited"),
                encoding="utf-8",
                newline="\n",
            )
            for environment in ("staging", "production"):
                with self.assertRaisesRegex(PromotionError, "failed verification"):
                    ready(workspace, environment, anchors=[anchor])

    def test_production_needs_an_anchor_before_anything_is_audited(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace)
            with mock.patch.object(release_promotion_gate.subprocess, "run") as run:
                with self.assertRaisesRegex(PromotionError, "requires a ledger anchor kept outside"):
                    ready(workspace, "production", anchors=[])
            run.assert_not_called()
            # Staging is not held to it, and an anchor given there is still checked.
            self.assertEqual(ready(workspace, "staging")[0]["ledger_anchors"], [])
            status, _ = ready(workspace, "staging", anchors=[workspace.anchor()])
            self.assertEqual([anchor["covers_root"] for anchor in status["ledger_anchors"]], [True])

    def test_production_needs_an_anchor_that_covers_the_whole_root(self) -> None:
        # A record appended after the anchor is not protected by it: deleting that
        # record, a rejection perhaps, would leave the old anchor holding. So the
        # anchor a production promotion relies on must cover the root as it stands.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace)
            stale = workspace.anchor()
            workspace.write(
                "later", make_chain(("evidence.later.1", "subject.later.1", {"evidence_type": "design_partner"}))
            )
            with self.assertRaisesRegex(PromotionError, "covers every ledger and record"):
                ready(workspace, "production", anchors=[stale])
            status, _ = ready(workspace, "production", anchors=[stale, workspace.anchor()])
            self.assertTrue(status["all_gates_eligible"])
            self.assertEqual([anchor["covers_root"] for anchor in status["ledger_anchors"]], [False, True])

    def test_a_deleted_ledger_tail_blocks_promotion(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace)
            anchor = workspace.anchor()
            ledger = workspace.ledgers / "paper_session.acceptance.ndjson"
            ledger.write_bytes(b"".join(ledger.read_bytes().splitlines(keepends=True)[:-1]))
            with self.assertRaisesRegex(PromotionError, "failed verification.*deleted from its tail"):
                ready(workspace, "production", anchors=[anchor])

    def test_staging_needs_verifiable_ledgers_but_not_eligibility(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            status, _ = ready(Workspace(directory), "staging")
            self.assertFalse(status["all_gates_eligible"])

    def test_the_audit_response_must_name_the_requested_release(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            anchored = [{"sha256": "b" * 64, "covers_root": True}]
            response = audit_response("release.other", all_gates_eligible=True, ledger_anchors=anchored)
            with mock.patch.object(release_promotion_gate.subprocess, "run", return_value=response):
                with self.assertRaisesRegex(PromotionError, "different release"):
                    ready(workspace, "production")

    def test_incomplete_audit_responses_are_refused_at_the_gate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            valid = json.loads(audit_response().stdout)
            # The well-formed response passes, so each refusal below is about its change.
            with mock.patch.object(release_promotion_gate.subprocess, "run", return_value=audit_response()):
                self.assertEqual(ready(workspace, "staging")[0], valid)
            malformed = {
                "a boolean schema version": {"acceptance_status_schema_version": True},
                "schema 4": {"acceptance_status_schema_version": 4},
                "a missing reviewer digest": {"trusted_reviewers_sha256": None},
                "an invalid reviewer digest": {"trusted_reviewers_sha256": "not a digest"},
                "a missing ledger list": {"ledgers": None},
                "a nonboolean eligibility": {"all_gates_eligible": "false"},
                "a missing anchor list": {"ledger_anchors": None},
                "an anchor nobody passed": {"ledger_anchors": [{"sha256": "b" * 64, "covers_root": True}]},
            }
            for name, changes in malformed.items():
                response = audit_response(**changes)
                with self.subTest(name), mock.patch.object(release_promotion_gate.subprocess, "run", return_value=response):
                    with self.assertRaises(PromotionError):
                        ready(workspace, "staging")
            anchor = workspace.anchor()
            for name, entry in {
                "an anchor without a digest": {"covers_root": True},
                "an anchor with a malformed digest": {"sha256": "B" * 64, "covers_root": True},
                "an anchor whose coverage is not a boolean": {"sha256": "b" * 64, "covers_root": 1},
                "an anchor that is not an object": "b" * 64,
            }.items():
                response = audit_response(ledger_anchors=[entry])
                with self.subTest(name), mock.patch.object(release_promotion_gate.subprocess, "run", return_value=response):
                    with self.assertRaisesRegex(PromotionError, "incomplete status"):
                        ready(workspace, "staging", anchors=[anchor])

    def test_the_release_is_the_one_the_manifest_names(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "manifest.json"
            manifest.write_text(json.dumps({"release_id": "release.unit.007"}), encoding="utf-8")
            self.assertEqual(release_id_of(manifest.read_bytes()), "release.unit.007")
            for content in ("{", "[]", "{}", json.dumps({"release_id": "Release Seven"}), json.dumps({"release_id": 7})):
                with self.subTest(content=content):
                    manifest.write_text(content, encoding="utf-8")
                    with self.assertRaises(PromotionError):
                        release_id_of(manifest.read_bytes())

    def test_the_receipt_binds_the_status_release_reviewers_and_every_ledger_counted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            write_every_gate(workspace)
            status, encoded = ready(workspace, "production")
            root = Path(directory)
            for name in ("manifest", "signature", "trusted-key"):
                (root / f"{name}.json").write_text("{}", encoding="utf-8")
            receipt = promotion_receipt(
                source_environment="staging",
                target_environment="production",
                release=VerifiedRelease(
                    release_id=RELEASE_ID,
                    manifest_sha256=hashlib.sha256((root / "manifest.json").read_bytes()).hexdigest(),
                    signature_sha256=hashlib.sha256((root / "signature.json").read_bytes()).hexdigest(),
                    trusted_key_sha256=hashlib.sha256((root / "trusted-key.json").read_bytes()).hexdigest(),
                ),
                acceptance_status=status,
                acceptance_status_bytes=encoded,
                requester="user.release",
                approver="user.approver",
                change_ticket="change.1",
                promoted_at="2026-09-28T10:00:00Z",
            )
            self.assertEqual(receipt["release_promotion_receipt_schema_version"], 4)
            self.assertEqual(receipt["release_id"], RELEASE_ID)
            anchor = workspace.anchors / "anchor-1.json"
            self.assertEqual(
                receipt["acceptance_ledger_anchors"],
                [{
                    "sha256": hashlib.sha256(anchor.read_bytes()).hexdigest(),
                    "anchored_at": ANCHORED_AT,
                    "ledgers": len(GATES),
                    "covers_root": True,
                }],
            )
            self.assertEqual(
                receipt["trusted_reviewers_sha256"],
                hashlib.sha256(workspace.reviewers.read_bytes()).hexdigest(),
            )
            self.assertEqual(receipt["acceptance_status_sha256"], hashlib.sha256(encoded).hexdigest())
            for name, field in (
                ("manifest", "manifest_sha256"),
                ("signature", "signature_sha256"),
                ("trusted-key", "trusted_key_sha256"),
            ):
                self.assertEqual(
                    receipt[field], hashlib.sha256((root / f"{name}.json").read_bytes()).hexdigest()
                )
            bound = {ledger["path"]: ledger["sha256"] for ledger in receipt["acceptance_ledgers"]}
            self.assertEqual(
                bound,
                {
                    path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                    for path in sorted(workspace.ledgers.glob("*.acceptance.ndjson"))
                },
            )
            with self.assertRaisesRegex(PromotionError, "does not belong to the verified release"):
                promotion_receipt(
                    source_environment="staging",
                    target_environment="production",
                    release=VerifiedRelease("release.other", "a" * 64, "b" * 64, "c" * 64),
                    acceptance_status=status,
                    acceptance_status_bytes=encoded,
                    requester="user.release",
                    approver="user.approver",
                    change_ticket="change.1",
                    promoted_at="2026-09-28T10:00:00Z",
                )

    def test_a_swapped_manifest_cannot_change_the_verified_release_or_receipt(self) -> None:
        # Finding (5): verification once used the original path, then the gate read
        # that path twice more. Swapping it after verification changed the release
        # whose acceptance was checked and the digest the receipt claimed.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / "manifest.json"
            signature = root / "signature.json"
            key = root / "trusted-key.json"
            artifacts = root / "release-artifacts"
            ledgers = root / "ledgers"
            acceptance_artifacts = root / "acceptance-artifacts"
            reviewers = root / "reviewers.json"
            receipt_path = root / "receipt.json"
            for path in (artifacts, ledgers, acceptance_artifacts):
                path.mkdir()
            reviewers.write_text("{}", encoding="utf-8")
            anchor = root / "anchor.json"
            anchor.write_text("{}", encoding="utf-8")
            original_manifest = b'{"release_id":"release.original"}'
            original_signature = b"original signature"
            original_key = b"original key"
            manifest.write_bytes(original_manifest)
            signature.write_bytes(original_signature)
            key.write_bytes(original_key)
            calls: list[str] = []

            def run(command: list[str], **options: object) -> SimpleNamespace:
                if "release-verify" in command:
                    calls.append("verify")
                    offset = command.index("release-verify")
                    self.assertEqual(Path(command[offset + 1]).read_bytes(), original_manifest)
                    self.assertEqual(Path(command[offset + 2]).read_bytes(), original_signature)
                    self.assertEqual(Path(command[offset + 3]).read_bytes(), original_key)
                    self.assertNotEqual(Path(command[offset + 1]), manifest)
                    manifest.write_bytes(b'{"release_id":"release.swapped"}')
                    signature.write_bytes(b"swapped signature")
                    key.write_bytes(b"swapped key")
                    return SimpleNamespace(returncode=0, stderr="")
                calls.append("audit")
                self.assertIn("audit", command)
                self.assertEqual(command[command.index("--release-id") + 1], "release.original")
                self.assertEqual(command[command.index("--anchor") + 1], str(anchor.resolve()))
                return audit_response(
                    "release.original", ledger_anchors=[{"sha256": "b" * 64, "covers_root": True}]
                )

            arguments = [
                "--source-environment", "development", "--target-environment", "staging",
                "--manifest", str(manifest), "--signature", str(signature), "--trusted-key", str(key),
                "--artifacts-root", str(artifacts), "--acceptance-ledger-root", str(ledgers),
                "--acceptance-trusted-reviewers", str(reviewers),
                "--acceptance-artifact-root", str(acceptance_artifacts),
                "--acceptance-ledger-anchor", str(anchor),
                "--requester", "user.release", "--approver", "user.approver",
                "--change-ticket", "change.1", "--receipt", str(receipt_path),
            ]
            with mock.patch.object(release_promotion_gate.subprocess, "run", side_effect=run):
                self.assertEqual(main(arguments), 0)
            receipt = json.loads(receipt_path.read_bytes())
            self.assertEqual(calls, ["verify", "audit"])
            self.assertEqual(receipt["release_id"], "release.original")
            self.assertEqual(receipt["acceptance_ledger_anchors"], [{"sha256": "b" * 64, "covers_root": True}])
            self.assertEqual(receipt["manifest_sha256"], hashlib.sha256(original_manifest).hexdigest())
            self.assertEqual(receipt["signature_sha256"], hashlib.sha256(original_signature).hexdigest())
            self.assertEqual(receipt["trusted_key_sha256"], hashlib.sha256(original_key).hexdigest())

    def test_a_verification_snapshot_that_changes_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / "manifest.json"
            signature = root / "signature.json"
            key = root / "trusted-key.json"
            manifest.write_bytes(b'{"release_id":"release.original"}')
            signature.write_bytes(b"signature")
            key.write_bytes(b"key")

            def change_snapshot(command: list[str], **options: object) -> SimpleNamespace:
                offset = command.index("release-verify")
                Path(command[offset + 1]).write_bytes(b'{"release_id":"release.changed"}')
                return SimpleNamespace(returncode=0, stderr="")

            with mock.patch.object(release_promotion_gate.subprocess, "run", side_effect=change_snapshot):
                with self.assertRaisesRegex(PromotionError, "inputs changed during verification"):
                    verify_release(REPOSITORY_ROOT, manifest, signature, key, root)

    def test_receipt_creation_never_replaces_an_existing_name(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt = root / "promotion.json"
            write_receipt(receipt, {"decision": "eligible"})
            self.assertEqual(json.loads(receipt.read_bytes()), {"decision": "eligible"})
            with self.assertRaisesRegex(PromotionError, "already exists"):
                write_receipt(receipt, {"decision": "changed"})
            self.assertEqual(json.loads(receipt.read_bytes()), {"decision": "eligible"})
            self.assertEqual([path.name for path in root.iterdir()], [receipt.name])

    def test_receipt_creation_refuses_a_name_that_appears_during_publication(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt = root / "promotion.json"

            def another_writer(source: str, target: Path) -> None:
                receipt.write_bytes(b"another writer's receipt")
                raise FileExistsError(receipt)

            with mock.patch.object(release_promotion_gate.os, "link", side_effect=another_writer):
                with self.assertRaisesRegex(PromotionError, "already exists"):
                    write_receipt(receipt, {"decision": "eligible"})
            self.assertEqual(receipt.read_bytes(), b"another writer's receipt")
            self.assertEqual([path.name for path in root.iterdir()], [receipt.name])

    @unittest.skipUnless(links_supported(), "cannot create a symbolic link here")
    def test_receipt_creation_does_not_write_through_a_staged_link(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            victim = root / "victim.txt"
            victim.write_bytes(b"must survive")
            receipt = root / "promotion.json"
            (root / "promotion.json.tmp").symlink_to(victim)
            write_receipt(receipt, {"decision": "eligible"})
            self.assertEqual(victim.read_bytes(), b"must survive")
            self.assertEqual(json.loads(receipt.read_bytes()), {"decision": "eligible"})


if __name__ == "__main__":
    unittest.main()
