"""The published schemas describe the acceptance records the audit tool reads (E6.4).

A schema and the tool that reads its documents can drift apart unnoticed, as the
version-2 PAPER schema once did (audit item 77). The key-set check needs nothing
installed. The full validation runs when `jsonschema` is.
"""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

from acceptance_fixtures import (
    REVIEWER_ID,
    REVIEWER_KEY_ID,
    REVIEWER_SEED,
    make_record,
    reviewers_document,
)
from tools.acceptance_evidence import BASE_KEYS, GATES, SCHEMA_VERSION, ZERO_HASH

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
        entry = schema["properties"]["reviewers"]["items"]
        self.assertEqual(set(entry["required"]), {"key_id", "reviewer_id", "public_key_hex"})
        self.assertIs(entry["additionalProperties"], False)

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
            "another version": {**valid, "trusted_reviewers_schema_version": 2},
            "an entry field too many": {**valid, "reviewers": [{**entry, "extra": 1}]},
            "a short key": {**valid, "reviewers": [{**entry, "public_key_hex": "abcd"}]},
        }.items():
            with self.subTest(refused=name):
                self.assertFalse(validator.is_valid(document))


if __name__ == "__main__":
    unittest.main()
