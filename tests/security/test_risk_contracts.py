from __future__ import annotations

import json
import re
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SCHEMA_ROOT = REPOSITORY_ROOT / "contracts" / "json-schema" / "v1"
FIXTURE_ROOT = REPOSITORY_ROOT / "tests" / "fixtures" / "config"


class RiskConfigurationContractTests(unittest.TestCase):
    def test_price_collar_schema_matches_runtime_boundary(self) -> None:
        for prefix in ("backtest", "paper", "live"):
            with self.subTest(prefix=prefix):
                schema = json.loads(
                    (SCHEMA_ROOT / f"{prefix}-configuration.schema.json").read_text(
                        encoding="utf-8"
                    )
                )
                fixture = json.loads(
                    (FIXTURE_ROOT / f"{prefix}-v1.json").read_text(encoding="utf-8")
                )
                pattern = schema["$defs"]["basisPoints"]["pattern"]
                configured = fixture["risk"]["max_price_deviation_bps"]
                self.assertIsNotNone(re.fullmatch(pattern, configured))
                for rejected in ("-0.1", "10000", "10000.00000000", "01", "1.123456789"):
                    self.assertIsNone(re.fullmatch(pattern, rejected))
                for accepted in ("0", "0.00000001", "100", "9999.99999999"):
                    self.assertIsNotNone(re.fullmatch(pattern, accepted))

    def test_paper_command_route_fixture_matches_the_versioned_boundary(self) -> None:
        schema = json.loads(
            (SCHEMA_ROOT / "paper-command-route.schema.json").read_text(
                encoding="utf-8"
            )
        )
        fixture = json.loads(
            (FIXTURE_ROOT / "paper-command-route-v1.json").read_text(
                encoding="utf-8"
            )
        )

        self.assertEqual(
            fixture["schema_version"],
            schema["properties"]["schema_version"]["const"],
        )
        self.assertEqual(set(schema["required"]), set(fixture) - {"short_exposure"})
        self.assertEqual(
            fixture["adapter_kind"], schema["properties"]["adapter_kind"]["const"]
        )
        canonical = schema["$defs"]["canonicalId"]["pattern"]
        for field in ("account_id", "trading_calendar_id"):
            self.assertIsNotNone(re.fullmatch(canonical, fixture[field]))
        positive = schema["$defs"]["positiveDecimal"]["pattern"]
        for field in (
            "max_order_quantity",
            "max_order_notional",
            "max_position_quantity",
        ):
            self.assertIsNotNone(re.fullmatch(positive, fixture[field]))
        self.assertIsNotNone(
            re.fullmatch(
                positive, fixture["short_exposure"]["max_short_quantity"]
            )
        )


if __name__ == "__main__":
    unittest.main()
