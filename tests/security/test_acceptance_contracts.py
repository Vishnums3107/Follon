"""The published schemas describe the acceptance records the audit tool reads (E6.4).

A schema and the tool that reads its documents can drift apart unnoticed, as the
version-2 PAPER schema once did (audit item 77). The key-set check needs nothing
installed. The full validation runs when `jsonschema` is.
"""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

from acceptance_fixtures import (
    REVIEWER_ID,
    REVIEWER_KEY_ID,
    REVIEWER_SEED,
    Workspace,
    make_chain,
    make_record,
    reviewers_document,
)
from tools.acceptance_evidence import (
    ANCHOR_KEYS,
    ANCHOR_LEDGER_KEYS,
    ANCHOR_SCHEMA_VERSION,
    BASE_KEYS,
    GATES,
    KEY_STATUSES,
    SCHEMA_VERSION,
    TRUSTED_REVIEWERS_SCHEMA_VERSIONS,
    ZERO_HASH,
)

SCHEMA_ROOT = Path(__file__).resolve().parents[2] / "contracts" / "json-schema" / "v2"


def load(name: str) -> dict:
    return json.loads((SCHEMA_ROOT / name).read_text(encoding="utf-8"))


class AcceptanceContractTests(unittest.TestCase):
    def test_the_record_schema_declares_exactly_the_keys_the_tool_requires(self) -> None:
        schema = load("external-acceptance-evidence.schema.json")
        self.assertEqual(set(schema["required"]), BASE_KEYS)
        self.assertEqual(set(schema["properties"]), BASE_KEYS)
        self.assertEqual(schema["properties"]["acceptance_evidence_schema_version"], {"const": SCHEMA_VERSION})
        self.assertIs(schema["additionalProperties"], False)
        self.assertEqual(set(schema["properties"]["evidence_type"]["enum"]), set(GATES))

    def test_the_reviewer_set_schema_declares_what_the_tool_reads(self) -> None:
        schema = load("trusted-reviewers.schema.json")
        self.assertEqual(set(schema["required"]), {"trusted_reviewers_schema_version", "reviewers"})
        self.assertEqual(
            schema["properties"]["trusted_reviewers_schema_version"], {"const": max(TRUSTED_REVIEWERS_SCHEMA_VERSIONS)}
        )
        entry = schema["properties"]["reviewers"]["items"]
        self.assertEqual(set(entry["required"]), {"key_id", "reviewer_id", "public_key_hex", "status"})
        self.assertEqual(set(entry["properties"]), set(entry["required"]))
        self.assertEqual(tuple(entry["properties"]["status"]["enum"]), KEY_STATUSES)
        self.assertIs(entry["additionalProperties"], False)

    def test_the_anchor_schema_declares_what_the_tool_reads(self) -> None:
        schema = load("acceptance-ledger-anchor.schema.json")
        self.assertEqual(set(schema["required"]), ANCHOR_KEYS)
        self.assertEqual(set(schema["properties"]), ANCHOR_KEYS)
        self.assertEqual(schema["properties"]["acceptance_ledger_anchor_schema_version"], {"const": ANCHOR_SCHEMA_VERSION})
        self.assertIs(schema["additionalProperties"], False)
        entry = schema["properties"]["ledgers"]["items"]
        self.assertEqual(set(entry["required"]), ANCHOR_LEDGER_KEYS)
        self.assertEqual(set(entry["properties"]), ANCHOR_LEDGER_KEYS)
        self.assertIs(entry["additionalProperties"], False)

    @unittest.skipUnless(importlib.util.find_spec("jsonschema"), "full validation needs jsonschema")
    def test_an_anchor_the_tool_writes_validates_and_a_malformed_one_does_not(self) -> None:
        import jsonschema

        schema = load("acceptance-ledger-anchor.schema.json")
        validator = jsonschema.validators.validator_for(schema)(schema)
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(directory)
            workspace.write("paper", make_chain(("evidence.x.1", "subject.x.1", {})))
            workspace.write("empty", [])
            anchor = json.loads(workspace.anchor().read_bytes())
        self.assertEqual([error.message for error in validator.iter_errors(anchor)], [])
        entry = anchor["ledgers"][1]
        for name, document in {
            "version 2": {**anchor, "acceptance_ledger_anchor_schema_version": 2},
            "an extra field": {**anchor, "extra": True},
            "a time with a space": {**anchor, "anchored_at": "2026-08-25 10:00:00Z"},
            "a path that is not a ledger": {**anchor, "ledgers": [{**entry, "path": "paper.json"}]},
            "a negative count": {**anchor, "ledgers": [{**entry, "records": -1}]},
            "a short head": {**anchor, "ledgers": [{**entry, "head": "ab"}]},
            "an entry field too many": {**anchor, "ledgers": [{**entry, "extra": 1}]},
        }.items():
            with self.subTest(refused=name):
                self.assertFalse(validator.is_valid(document))

    @unittest.skipUnless(importlib.util.find_spec("jsonschema"), "full validation needs jsonschema")
    def test_every_record_type_validates_and_a_malformed_one_does_not(self) -> None:
        import jsonschema

        schema = load("external-acceptance-evidence.schema.json")
        validator = jsonschema.validators.validator_for(schema)(schema)
        for evidence_type in GATES:
            with self.subTest(evidence_type=evidence_type):
                record = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1", evidence_type=evidence_type)
                self.assertEqual([error.message for error in validator.iter_errors(record)], [])
        session = make_record(ZERO_HASH, "evidence.x.1", "subject.x.1")
        partner = make_record(ZERO_HASH, "evidence.x.2", "subject.x.2", evidence_type="design_partner")
        refused = {
            "version 1": {**session, "acceptance_evidence_schema_version": 1},
            "an extra field": {**session, "extra": True},
            "a session in no environment": {**session, "environment": None},
            "a session in another environment": {**session, "environment": "STAGING"},
            "a partner in an environment": {**partner, "environment": "PAPER"},
            "a short signature": {**session, "reviewer_signature": "ab"},
            "session attributes missing a key": {
                **session,
                "attributes": {k: v for k, v in session["attributes"].items() if k != "orders_submitted"},
            },
            "a boolean where a count belongs": {**session, "attributes": {**session["attributes"], "orders_submitted": True}},
            "partner attributes that are session attributes": {**partner, "attributes": session["attributes"]},
            "a note with a newline": {**session, "notes": "two\nlines"},
        }
        for name, document in refused.items():
            with self.subTest(refused=name):
                self.assertFalse(validator.is_valid(document))

    @unittest.skipUnless(importlib.util.find_spec("jsonschema"), "full validation needs jsonschema")
    def test_a_reviewer_set_validates_and_a_malformed_one_does_not(self) -> None:
        import jsonschema

        schema = load("trusted-reviewers.schema.json")
        validator = jsonschema.validators.validator_for(schema)(schema)
        valid = reviewers_document((REVIEWER_KEY_ID, REVIEWER_ID, REVIEWER_SEED))
        self.assertTrue(validator.is_valid(valid))
        self.assertTrue(validator.is_valid(reviewers_document()))
        entry = valid["reviewers"][0]
        for name, document in {
            "an unknown field": {**valid, "extra": 1},
            "another version": {**valid, "trusted_reviewers_schema_version": 1},
            "an unknown status": {**valid, "reviewers": [{**entry, "status": "retired"}]},
            "no status": {**valid, "reviewers": [{k: v for k, v in entry.items() if k != "status"}]},
            "an entry field too many": {**valid, "reviewers": [{**entry, "extra": 1}]},
            "a short key": {**valid, "reviewers": [{**entry, "public_key_hex": "abcd"}]},
        }.items():
            with self.subTest(refused=name):
                self.assertFalse(validator.is_valid(document))


if __name__ == "__main__":
    unittest.main()
