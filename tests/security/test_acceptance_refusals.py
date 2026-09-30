"""The acceptance audit refuses what it cannot trust, and never raises anything else (E6.6b).

A review probed the audit with records no reviewer's tooling would write: an evidence
type that is a list, an options list that mixes types, a 5,000-digit integer, a line
nested deeper than Python recurses. Each escaped as a traceback. These tests hold every
field to a refusal, and hold each format check and size limit where it binds, because
about half of the mutants the reviewer injected into those checks survived.
"""

from __future__ import annotations

import json
import tempfile
import unittest
from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path
from unittest import mock

from acceptance_fixtures import (
    REVIEWER_ID,
    REVIEWER_KEY_ID,
    REVIEWER_SEED,
    Workspace,
    attributes_for,
    make_chain,
    make_record,
    reviewers_document,
    write_ledger,
)
from tools import acceptance_evidence
from tools.acceptance_evidence import (
    BASE_KEYS,
    GATES,
    MAX_LEDGER_BYTES,
    MAX_LINE_BYTES,
    MAX_NOTES_CHARACTERS,
    MAX_REVIEWER_SET_BYTES,
    ZERO_HASH,
    EvidenceError,
    canonical_json,
    load_ledgers,
    load_trusted_reviewers,
    main,
    parse_ledger,
    record_hash,
)


def nested(depth: int) -> list[object]:
    value: list[object] = []
    for _ in range(depth):
        value = [value]
    return value


# Values of every JSON type, and the shapes that raised before: unhashable lists and
# objects, a list mixing types, a large integer, deep nesting and a non-finite number.
HOSTILE_VALUES: tuple[object, ...] = (
    None, True, False, 0, -1, 1.5, float("nan"), 10**400, "", "Not Canonical", "a\n",
    [], ["PAPER", 1], [[]], {}, {"a": [1]}, nested(64),
)

IDENTIFIER_KEYS = ("evidence_id", "subject_id", "observed_by", "reviewed_by", "release_id", "reviewer_key_id")


def line(record: dict[str, object]) -> bytes:
    return canonical_json(record) + b"\n"


def signed_record(**changes: object) -> dict[str, object]:
    """A paper session record with `changes`, signed and hashed as a reviewer's tooling would."""
    evidence_id = changes.pop("evidence_id", "evidence.x.1")
    subject_id = changes.pop("subject_id", "subject.x.1")
    return make_record(ZERO_HASH, evidence_id, subject_id, **changes)


def links_supported() -> bool:
    with tempfile.TemporaryDirectory() as directory:
        target = Path(directory) / "target"
        target.write_bytes(b"")
        try:
            (Path(directory) / "link").symlink_to(target)
        except (OSError, NotImplementedError):
            return False
        return True


class LedgerHarness(unittest.TestCase):
    """Parses ledger bytes in memory, and keeps a scratch root for what needs a file."""

    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def load(self, content: bytes) -> list[dict[str, object]]:
        return parse_ledger(content, "x.acceptance.ndjson")[0]

    def load_file(self, content: bytes) -> list[dict[str, object]]:
        (self.root / "x.acceptance.ndjson").write_bytes(content)
        return load_ledgers(self.root)

    def refusal(self, content: bytes) -> str:
        with self.assertRaises(EvidenceError) as caught:
            self.load(content)
        return str(caught.exception)

    def loads_or_refuses(self, content: bytes) -> None:
        # Anything but EvidenceError propagates and errors the test.
        try:
            self.load(content)
        except EvidenceError:
            pass


class NothingEscapesTests(LedgerHarness):
    def test_a_hostile_value_in_any_field_is_refused_and_never_raised(self) -> None:
        for evidence_type in GATES:
            base = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", evidence_type=evidence_type)
            for key in sorted(BASE_KEYS):
                for value in HOSTILE_VALUES:
                    with self.subTest(evidence_type=evidence_type, key=key, value=repr(value)[:40]):
                        forged = {**base, key: value}
                        if key != "record_hash":
                            forged["record_hash"] = record_hash(forged)
                        self.loads_or_refuses(line(forged))

    def test_a_hostile_attribute_value_is_refused_and_never_raised(self) -> None:
        for evidence_type in GATES:
            base = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", evidence_type=evidence_type)
            for key in sorted(attributes_for(evidence_type)):
                for value in HOSTILE_VALUES:
                    with self.subTest(evidence_type=evidence_type, attribute=key, value=repr(value)[:40]):
                        forged = {**base, "attributes": {**base["attributes"], key: value}}
                        forged["record_hash"] = record_hash(forged)
                        self.loads_or_refuses(line(forged))

    def test_the_reviewers_findings_are_refusals(self) -> None:
        record = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1")
        options = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", evidence_type="broker_options")
        customer = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", evidence_type="paying_customer")
        cases = {
            "an unhashable evidence type": {**record, "evidence_type": ["paper_session"]},
            "an unhashable outcome": {**record, "outcome": {"accepted": True}},
            "an options list that mixes types": {
                **options, "attributes": {"reconciled_environments": ["PAPER", 1]},
            },
            "an options list holding a list": {
                **options, "attributes": {"reconciled_environments": [["PAPER"]]},
            },
            "an unhashable customer kind": {
                **customer, "attributes": {**customer["attributes"], "customer_kind": ["professional"]},
            },
        }
        for name, forged in cases.items():
            with self.subTest(name):
                forged["record_hash"] = record_hash(forged)
                self.refusal(line(forged))

    def test_json_the_parser_raises_on_is_refused(self) -> None:
        for name, content in {
            "a 5,000-digit integer": b'{"a":' + b"1" * 5000 + b"}\n",
            "nesting deeper than Python recurses": b"[" * 100_000 + b"]" * 100_000 + b"\n",
            "bytes that are not UTF-8": b'{"a":"\xff"}\n',
            "two documents on one line": b'{"a":1}{"b":2}\n',
            "a truncated document": b'{"a":\n',
        }.items():
            with self.subTest(name):
                self.assertIn("invalid JSON at x.acceptance.ndjson:1", self.refusal(content))

    def test_a_value_that_is_not_a_record_is_refused(self) -> None:
        for content in (b"NaN\n", b"[]\n", b'"record"\n', b"null\n", b"{}\n"):
            with self.subTest(content=content):
                self.assertIn("record keys", self.refusal(content))

    def test_a_reviewer_set_the_parser_raises_on_is_refused(self) -> None:
        valid = reviewers_document((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED))
        entry = valid["reviewers"][0]
        documents: dict[str, bytes] = {
            "nesting deeper than Python recurses": b"[" * 100_000 + b"]" * 100_000,
            "a 5,000-digit version": b'{"trusted_reviewers_schema_version":' + b"1" * 5000 + b',"reviewers":[]}',
            "bytes that are not UTF-8": b'{"a":"\xff"}',
        }
        for field in ("key_id", "reviewer_id", "public_key_hex"):
            for index, value in enumerate(HOSTILE_VALUES):
                documents[f"{field} #{index}"] = json.dumps({**valid, "reviewers": [{**entry, field: value}]}).encode()
        for index, value in enumerate(HOSTILE_VALUES):
            documents[f"an entry that is value #{index}"] = json.dumps({**valid, "reviewers": [value]}).encode()
            if value != []:  # the one value that is an empty, valid set
                documents[f"reviewers that are value #{index}"] = json.dumps({**valid, "reviewers": value}).encode()
        for name, content in documents.items():
            with self.subTest(name):
                path = self.root / "reviewers.json"
                path.write_bytes(content)
                with self.assertRaises(EvidenceError):
                    load_trusted_reviewers(path)

    def test_the_command_line_refuses_rather_than_raises(self) -> None:
        (self.root / "workspace").mkdir()
        workspace = Workspace(str(self.root / "workspace"), retain=False)
        (workspace.ledgers / "x.acceptance.ndjson").write_bytes(b"[" * 100_000 + b"]" * 100_000 + b"\n")
        errors = StringIO()
        with redirect_stderr(errors):
            code = main([
                "audit", str(workspace.ledgers), "--trusted-reviewers", str(workspace.reviewers),
                "--artifact-root", str(workspace.artifacts), "--release-id", "release.unit.001",
            ])
        self.assertEqual(code, 2)
        self.assertIn("invalid JSON", errors.getvalue())
        template = self.root / "template.json"
        template.write_bytes(b"[" * 100_000 + b"]" * 100_000)
        key = self.root / "reviewer.pk8"
        key.write_bytes(bytes.fromhex("302e020100300506032b657004220420") + REVIEWER_SEED)
        artifact = self.root / "artifact.log"
        artifact.write_bytes(b"artifact")
        errors = StringIO()
        with redirect_stderr(errors):
            code = main([
                "append", str(workspace.ledgers / "y.acceptance.ndjson"), "--record", str(template),
                "--artifact", str(artifact), "--artifact-root", str(workspace.artifacts),
                "--reviewer-key", str(key), "--reviewer-key-id", REVIEWER_KEY_ID,
            ])
        self.assertEqual(code, 2)
        self.assertIn("cannot read the record template", errors.getvalue())


class FormatCheckTests(LedgerHarness):
    """Each check refuses its own field, so none can be removed while another hides it."""

    def assert_refused_for(self, record: dict[str, object], message: str) -> None:
        self.assertIn(message, self.refusal(line(record)))

    def test_every_identifier_must_be_canonical(self) -> None:
        for key in IDENTIFIER_KEYS:
            for value in ("Upper.case", "has space", "", "trailing\n", "café", 7):
                with self.subTest(key=key, value=value):
                    self.assert_refused_for(signed_record(**{key: value}), f"{key} is not a canonical ID")

    def test_notes_are_one_line_of_at_most_1024_characters(self) -> None:
        self.assertEqual(MAX_NOTES_CHARACTERS, 1024)
        longest = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", notes="n" * MAX_NOTES_CHARACTERS)
        self.assertEqual(len(self.load(line(longest))), 1)
        for value in ("n" * (MAX_NOTES_CHARACTERS + 1), "one\ntwo", 7, None):
            with self.subTest(value=repr(value)[:20]):
                record = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", notes=value)
                self.assert_refused_for(record, "notes must be a concise single line")

    def test_digests_are_lowercase_sha256(self) -> None:
        for key in ("source_artifact_sha256", "prev_hash"):
            for value in ("A" * 64, "a" * 63, "a" * 65, "g" * 64, 7):
                with self.subTest(key=key, value=value):
                    record = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", **{key: value})
                    self.assert_refused_for(record, f"{key} is not lowercase SHA-256")
        record = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1")
        for value in (str(record["record_hash"]).upper(), "a" * 63, "g" * 64, 7):
            with self.subTest(key="record_hash", value=value):
                self.assert_refused_for({**record, "record_hash": value}, "record_hash is not lowercase SHA-256")

    def test_the_signature_is_lowercase_ed25519(self) -> None:
        record = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1")
        for value in (str(record["reviewer_signature"]).upper(), "a" * 127, "a" * 129, "g" * 128, 7):
            with self.subTest(value=value):
                forged = {**record, "reviewer_signature": value}
                forged["record_hash"] = record_hash(forged)
                self.assert_refused_for(forged, "reviewer_signature is not a lowercase Ed25519 signature")

    def test_a_line_must_be_its_records_canonical_json(self) -> None:
        record = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", notes="café")
        canonical = canonical_json(record)
        self.assertEqual(len(self.load(canonical + b"\n")), 1)
        duplicate = b'{"outcome":"rejected",' + canonical[1:]
        self.assertEqual(json.loads(duplicate)["outcome"], "accepted")
        for name, content in {
            "spaced separators": json.dumps(record, sort_keys=True).encode() + b"\n",
            "unsorted keys": json.dumps(dict(reversed(list(record.items()))), separators=(",", ":")).encode() + b"\n",
            "a duplicate key whose first value is overruled": duplicate + b"\n",
            "a carriage return before the newline": canonical + b"\r\n",
            "unescaped non-ASCII": json.dumps(record, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
            + b"\n",
        }.items():
            with self.subTest(name):
                self.assertIn("is not its record's canonical JSON", self.refusal(content))


class SizeLimitTests(LedgerHarness):
    """Each limit binds at exactly its value, so an off-by-one is a failing test."""

    def test_the_limits_are_the_documented_ones(self) -> None:
        self.assertEqual(MAX_LEDGER_BYTES, 64 * 1024 * 1024)
        self.assertEqual(MAX_LINE_BYTES, 1024 * 1024)
        self.assertEqual(MAX_REVIEWER_SET_BYTES, 1024 * 1024)

    def test_a_line_may_be_exactly_the_line_limit(self) -> None:
        content = line(make_record(ZERO_HASH, "evidence.x.1", "subject.x.1"))
        with mock.patch.object(acceptance_evidence, "MAX_LINE_BYTES", len(content) - 1):
            self.assertEqual(len(self.load(content)), 1)
        with mock.patch.object(acceptance_evidence, "MAX_LINE_BYTES", len(content) - 2):
            self.assertIn("invalid evidence line x.acceptance.ndjson:1", self.refusal(content))

    def test_a_ledger_may_be_exactly_the_ledger_limit(self) -> None:
        content = b"".join(
            line(record)
            for record in make_chain(("evidence.x.1", "subject.x.1", {}), ("evidence.x.2", "subject.x.2", {}))
        )
        with mock.patch.object(acceptance_evidence, "MAX_LEDGER_BYTES", len(content)):
            self.assertEqual(len(self.load_file(content)), 2)
        with mock.patch.object(acceptance_evidence, "MAX_LEDGER_BYTES", len(content) - 1):
            with self.assertRaisesRegex(EvidenceError, "larger than"):
                self.load_file(content)

    def test_a_reviewer_set_may_be_exactly_its_limit(self) -> None:
        path = self.root / "reviewers.json"
        path.write_bytes(json.dumps(reviewers_document((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED))).encode())
        size = path.stat().st_size
        with mock.patch.object(acceptance_evidence, "MAX_REVIEWER_SET_BYTES", size):
            self.assertEqual(set(load_trusted_reviewers(path).keys), {REVIEWER_KEY_ID})
        with mock.patch.object(acceptance_evidence, "MAX_REVIEWER_SET_BYTES", size - 1):
            with self.assertRaisesRegex(EvidenceError, "larger than"):
                load_trusted_reviewers(path)


class LedgerRootTests(LedgerHarness):
    def test_only_the_exact_suffix_names_a_ledger(self) -> None:
        # A glob matched the suffix without regard to case on Windows and with it on Linux,
        # so one root counted different ledgers on the two. The stems differ from the
        # ledger's, because Windows would open `x.ACCEPTANCE.NDJSON` as the ledger itself.
        for name in ("y.ACCEPTANCE.NDJSON", "y.acceptance.ndjson.bak", "y.acceptance.ndjson.partial"):
            (self.root / name).write_bytes(b"not a ledger")
        records = self.load_file(line(make_record(ZERO_HASH, "evidence.x.1", "subject.x.1")))
        self.assertEqual([record["evidence_id"] for record in records], ["evidence.x.1"])

    def test_ledgers_are_bound_in_one_order_on_every_platform(self) -> None:
        # Ordered by relative path as a string, so an uppercase name sorts before a
        # lowercase one on every platform, where Windows paths compare without case.
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("a", make_chain(("evidence.a.1", "subject.a.1", {})))
            workspace.write("B", make_chain(("evidence.b.1", "subject.b.1", {})))
            report = workspace.audit()
        self.assertEqual([ledger["path"] for ledger in report["ledgers"]], ["B.acceptance.ndjson", "a.acceptance.ndjson"])

    @unittest.skipUnless(links_supported(), "cannot create a symbolic link here")
    def test_a_link_anywhere_under_the_root_is_refused(self) -> None:
        outside = self.root / "outside"
        outside.mkdir()
        outside_ledger = outside / "y.acceptance.ndjson"
        write_ledger(outside_ledger, [make_record(ZERO_HASH, "evidence.y.1", "subject.y.1")])
        for name, link_to in {
            "a linked directory": ("nested/linked", outside),
            "a linked ledger": ("linked.acceptance.ndjson", outside_ledger),
            "a linked file of any other name": ("notes.txt", outside_ledger),
        }.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "nested").mkdir()
                self.assertEqual(load_ledgers(root), [])
                link, target = link_to
                (root / link).symlink_to(target, target_is_directory=target.is_dir())
                with self.assertRaisesRegex(EvidenceError, f"the ledger root holds a link: {link}"):
                    load_ledgers(root)

    @unittest.skipUnless(links_supported(), "cannot create a symbolic link here")
    def test_a_linked_ledger_or_reviewer_set_is_refused_when_read_directly(self) -> None:
        target = self.root / "target.acceptance.ndjson"
        write_ledger(target, [make_record(ZERO_HASH, "evidence.x.1", "subject.x.1")])
        link = self.root / "link.acceptance.ndjson"
        link.symlink_to(target)
        with self.assertRaisesRegex(EvidenceError, "linked"):
            acceptance_evidence.read_ledger(link)
        reviewers = self.root / "reviewers.json"
        reviewers.write_text(json.dumps(reviewers_document()), encoding="utf-8")
        linked_reviewers = self.root / "linked-reviewers.json"
        linked_reviewers.symlink_to(reviewers)
        self.assertEqual(load_trusted_reviewers(reviewers).keys, {})
        with self.assertRaisesRegex(EvidenceError, "linked"):
            load_trusted_reviewers(linked_reviewers)


if __name__ == "__main__":
    unittest.main()
