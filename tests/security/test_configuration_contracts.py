"""Every checked-in configuration fixture conforms to its published schema.

A reader and its JSON Schema can drift apart unnoticed: nothing else loads a
schema next to the configuration it describes. The version-2 PAPER schema
stopped describing what `follon-paper-status` reads when items 61 and 70
added the tick and lot tables to the reader and to the version-1 schema only,
so both version-2 fixtures failed validation (audit item 77).
"""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SCHEMA_ROOT = REPOSITORY_ROOT / "contracts" / "json-schema"
FIXTURE_ROOT = REPOSITORY_ROOT / "tests" / "fixtures" / "config"

# Each configuration fixture a CLI or service reads, and the schema of the
# input contract that reader implements.
CONFIGURATION_CONTRACTS = {
    "backtest-v1.json": "v1/backtest-configuration.schema.json",
    "backtest-advanced-v1.json": "v1/backtest-configuration.schema.json",
    "backtest-probe-v1.json": "v1/backtest-configuration.schema.json",
    "paper-v1.json": "v1/paper-configuration.schema.json",
    "paper-v2.json": "v2/paper-configuration.schema.json",
    "paper-v2-portfolio-risk.json": "v2/paper-configuration.schema.json",
    "live-v1.json": "v1/live-configuration.schema.json",
    "live-v1-portfolio-risk.json": "v1/live-configuration.schema.json",
    "paper-command-route-v1.json": "v1/paper-command-route.schema.json",
    "paper-command-route-v1-bridge.json": "v1/paper-command-route.schema.json",
    "paper-command-route-v1-portfolio-risk.json": "v1/paper-command-route.schema.json",
    "operations-v1.json": "v1/operations-configuration.schema.json",
    "options-v1.json": "v1/options-configuration.schema.json",
    "commercial-data-inventory-v1.json": "v1/commercial-data-inventory.schema.json",
    "commercial-privacy-erasure-v1.json": "v1/commercial-privacy-request.schema.json",
    "commercial-provisioning-v1.json": "v1/commercial-provisioning.schema.json",
    "commercial-self-host-provisioning-v1.json": "v1/commercial-provisioning.schema.json",
    "commercial-subscription-v1.json": "v1/commercial-subscription.schema.json",
    "commercial-self-host-subscription-v1.json": "v1/commercial-subscription.schema.json",
}


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def resolve(schema: dict, node: dict) -> dict:
    """Follows a local `#/$defs/...` reference; other nodes are returned as is."""
    reference = node.get("$ref", "")
    if reference.startswith("#/$defs/"):
        return schema["$defs"][reference.removeprefix("#/$defs/")]
    return node


class ConfigurationContractTests(unittest.TestCase):
    def test_each_fixture_declares_exactly_what_its_schema_allows(self) -> None:
        # Needs no third-party package, so CI's security job runs it too. It
        # checks the top level and every object property one level down,
        # which is where each configuration keeps its risk policy.
        for fixture_name, schema_name in CONFIGURATION_CONTRACTS.items():
            with self.subTest(fixture=fixture_name):
                schema = load(SCHEMA_ROOT / schema_name)
                fixture = load(FIXTURE_ROOT / fixture_name)
                levels = [("", schema, fixture)]
                for key, value in fixture.items():
                    declared = schema.get("properties", {}).get(key)
                    if isinstance(value, dict) and declared is not None:
                        levels.append((f"{key}.", resolve(schema, declared), value))
                for prefix, node, document in levels:
                    undeclared = set(document) - set(node.get("properties", {}))
                    if node.get("additionalProperties") is False:
                        self.assertEqual(sorted(prefix + key for key in undeclared), [])
                    missing = set(node.get("required", [])) - set(document)
                    self.assertEqual(sorted(prefix + key for key in missing), [])

    def test_version_2_paper_risk_extends_version_1(self) -> None:
        # `follon-paper-status` reads both versions through one risk document,
        # so version 2 may add to version 1's risk policy but never drop,
        # loosen or stop requiring any of it.
        version_1 = load(SCHEMA_ROOT / "v1" / "paper-configuration.schema.json")
        version_2 = load(SCHEMA_ROOT / "v2" / "paper-configuration.schema.json")
        risk_1 = resolve(version_1, version_1["properties"]["risk"])
        risk_2 = resolve(version_2, version_2["properties"]["risk"])
        self.assertEqual(sorted(set(risk_1["required"]) - set(risk_2["required"])), [])
        for name, definition in risk_1["properties"].items():
            with self.subTest(property=name):
                self.assertEqual(risk_2["properties"].get(name), definition)
        # Those definitions refer to shared ones by name, so the shared ones
        # must mean the same in both versions.
        for name, definition in version_1["$defs"].items():
            if name != "risk":
                with self.subTest(definition=name):
                    self.assertEqual(version_2["$defs"].get(name), definition)

    def test_the_route_reads_the_same_portfolio_risk_document_as_paper_status(self) -> None:
        # The gRPC route, the desktop gateway and `follon-paper-status` share one
        # `portfolio_risk` document (delivery state E7.5), so the two schemas that
        # publish it must not drift apart.
        route = load(SCHEMA_ROOT / "v1" / "paper-command-route.schema.json")
        paper = load(SCHEMA_ROOT / "v2" / "paper-configuration.schema.json")
        for name in ("portfolioRisk", "instrumentBucket", "marginRate"):
            with self.subTest(definition=name):
                self.assertEqual(route["$defs"].get(name), paper["$defs"].get(name))
        self.assertEqual(
            route["properties"]["portfolio_risk"]["$ref"], "#/$defs/portfolioRisk"
        )
        self.assertNotIn("portfolio_risk", route["required"])

    @unittest.skipUnless(
        importlib.util.find_spec("jsonschema"), "full validation needs jsonschema"
    )
    def test_each_fixture_validates_against_its_schema(self) -> None:
        import jsonschema

        for fixture_name, schema_name in CONFIGURATION_CONTRACTS.items():
            with self.subTest(fixture=fixture_name):
                schema = load(SCHEMA_ROOT / schema_name)
                validator = jsonschema.validators.validator_for(schema)(schema)
                fixture = load(FIXTURE_ROOT / fixture_name)
                errors = [error.message for error in validator.iter_errors(fixture)]
                self.assertEqual(errors, [])


    @unittest.skipUnless(
        importlib.util.find_spec("jsonschema"), "full validation needs jsonschema"
    )
    def test_the_route_schema_ties_the_bridge_section_to_its_adapter_kind(self) -> None:
        # The service refuses a bridge section on the model and requires one
        # on the bridge (delivery state E5.2a); the schema says the same.
        import jsonschema

        schema = load(SCHEMA_ROOT / "v1" / "paper-command-route.schema.json")
        validator = jsonschema.validators.validator_for(schema)(schema)
        model = load(FIXTURE_ROOT / "paper-command-route-v1.json")
        bridge = load(FIXTURE_ROOT / "paper-command-route-v1-bridge.json")
        section = bridge["ibkr_bridge"]
        refused = {
            "model with a bridge section": {**model, "ibkr_bridge": section},
            "bridge without a section": {**model, "adapter_kind": "IBKR_PAPER_BRIDGE"},
            "unknown adapter kind": {**model, "adapter_kind": "IBKR_LIVE"},
            "live port": {**bridge, "ibkr_bridge": {**section, "port": 7496}},
            "remote host": {**bridge, "ibkr_bridge": {**section, "host": "10.0.0.5"}},
            "client id": {**bridge, "ibkr_bridge": {**section, "client_id": 32}},
            "short timeout": {**bridge, "ibkr_bridge": {**section, "request_timeout_seconds": 2}},
            "long timeout": {**bridge, "ibkr_bridge": {**section, "request_timeout_seconds": 61}},
            "split account": {**bridge, "ibkr_bridge": {**section, "broker_account": "DU1\nDU2"}},
            "free-form arguments": {**bridge, "ibkr_bridge": {**section, "arguments": ["--port", "7496"]}},
        }
        for name, document in refused.items():
            with self.subTest(refused=name):
                self.assertFalse(validator.is_valid(document))
        for name, document in {"model": model, "bridge": bridge}.items():
            with self.subTest(accepted=name):
                self.assertTrue(validator.is_valid(document))

    @unittest.skipUnless(
        importlib.util.find_spec("jsonschema"), "full validation needs jsonschema"
    )
    def test_the_route_schema_refuses_a_malformed_portfolio_risk_block(self) -> None:
        import jsonschema

        schema = load(SCHEMA_ROOT / "v1" / "paper-command-route.schema.json")
        validator = jsonschema.validators.validator_for(schema)(schema)
        route = load(FIXTURE_ROOT / "paper-command-route-v1-portfolio-risk.json")
        block = route["portfolio_risk"]
        self.assertTrue(validator.is_valid(route))
        refused = {
            "unknown limit": {**route, "portfolio_risk": {**block, "max_gross_expsure": "1"}},
            "zero gross exposure": {**route, "portfolio_risk": {**block, "max_gross_exposure": "0"}},
            "missing concentration": {
                **route,
                "portfolio_risk": {k: v for k, v in block.items() if k != "max_concentration_bps"},
            },
            "text limit": {**route, "portfolio_risk": {**block, "max_daily_loss": "lots"}},
            "not an object": {**route, "portfolio_risk": "wide"},
        }
        for name, document in refused.items():
            with self.subTest(refused=name):
                self.assertFalse(validator.is_valid(document))


if __name__ == "__main__":
    unittest.main()
