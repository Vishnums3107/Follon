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

from acceptance_fixtures import RELEASE_ID, Workspace, links_supported, make_chain
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


def ready(workspace: Workspace, target: str, release_id: str = RELEASE_ID):
    return acceptance_ready(
        REPOSITORY_ROOT,
        workspace.ledgers,
        target,
        trusted_reviewers=workspace.reviewers,
        artifact_root=workspace.artifacts,
        release_id=release_id,
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
        # fails the audit for staging too, not only production (E6.6b).
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory, trust=False)
            write_every_gate(workspace)
            for environment in ("staging", "production"):
                with self.assertRaisesRegex(PromotionError, "failed verification.*no listed reviewer key"):
                    ready(workspace, environment)

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
            ledger = workspace.ledgers / "paper_session.acceptance.ndjson"
            ledger.write_text(
                ledger.read_text(encoding="utf-8").replace("Independently reviewed", "Edited"),
                encoding="utf-8",
                newline="\n",
            )
            for environment in ("staging", "production"):
                with self.assertRaisesRegex(PromotionError, "failed verification"):
                    ready(workspace, environment)

    def test_staging_needs_verifiable_ledgers_but_not_eligibility(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            status, _ = ready(Workspace(directory), "staging")
            self.assertFalse(status["all_gates_eligible"])

    def test_the_audit_response_must_name_the_requested_release(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            wrong = {
                "acceptance_status_schema_version": 4,
                "release_id": "release.other",
                "trusted_reviewers_sha256": "a" * 64,
                "all_gates_eligible": True,
                "ledgers": [],
            }
            response = SimpleNamespace(returncode=0, stdout=json.dumps(wrong).encode(), stderr=b"")
            with mock.patch.object(release_promotion_gate.subprocess, "run", return_value=response):
                with self.assertRaisesRegex(PromotionError, "different release"):
                    ready(workspace, "production")

    def test_incomplete_audit_responses_are_refused_at_the_gate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            valid = {
                "acceptance_status_schema_version": 4,
                "release_id": RELEASE_ID,
                "trusted_reviewers_sha256": "a" * 64,
                "all_gates_eligible": False,
                "ledgers": [],
            }
            malformed = {
                "a boolean schema version": {**valid, "acceptance_status_schema_version": True},
                "a missing reviewer digest": {key: value for key, value in valid.items() if key != "trusted_reviewers_sha256"},
                "an invalid reviewer digest": {**valid, "trusted_reviewers_sha256": "not a digest"},
                "a missing ledger list": {key: value for key, value in valid.items() if key != "ledgers"},
                "a nonboolean eligibility": {**valid, "all_gates_eligible": "false"},
            }
            for name, status in malformed.items():
                response = SimpleNamespace(returncode=0, stdout=json.dumps(status).encode(), stderr=b"")
                with self.subTest(name), mock.patch.object(release_promotion_gate.subprocess, "run", return_value=response):
                    with self.assertRaises(PromotionError):
                        ready(workspace, "staging")

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
            self.assertEqual(receipt["release_promotion_receipt_schema_version"], 3)
            self.assertEqual(receipt["release_id"], RELEASE_ID)
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
                status = {
                    "acceptance_status_schema_version": 4,
                    "release_id": "release.original",
                    "trusted_reviewers_sha256": "a" * 64,
                    "all_gates_eligible": False,
                    "ledgers": [],
                }
                return SimpleNamespace(returncode=0, stdout=json.dumps(status).encode(), stderr=b"")

            arguments = [
                "--source-environment", "development", "--target-environment", "staging",
                "--manifest", str(manifest), "--signature", str(signature), "--trusted-key", str(key),
                "--artifacts-root", str(artifacts), "--acceptance-ledger-root", str(ledgers),
                "--acceptance-trusted-reviewers", str(reviewers),
                "--acceptance-artifact-root", str(acceptance_artifacts),
                "--requester", "user.release", "--approver", "user.approver",
                "--change-ticket", "change.1", "--receipt", str(receipt_path),
            ]
            with mock.patch.object(release_promotion_gate.subprocess, "run", side_effect=run):
                self.assertEqual(main(arguments), 0)
            receipt = json.loads(receipt_path.read_bytes())
            self.assertEqual(calls, ["verify", "audit"])
            self.assertEqual(receipt["release_id"], "release.original")
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
