from decimal import Decimal
from dataclasses import replace
import unittest

from follon_strategy_sdk import (
    BacktestProvenance,
    Bar,
    DatasetReference,
    OrderIntent,
    OrderType,
    Side,
    StrategyContext,
    TimeInForce,
)


class ModelContractTests(unittest.TestCase):
    def test_intent_serializes_decimal_as_a_string(self) -> None:
        intent = OrderIntent(
            intent_id="intent-test-001",
            account_id="acct-paper-001",
            strategy_id="strategy-test-001",
            instrument_id="inst.us_equity.spy",
            correlation_id="corr-test-001",
            side=Side.BUY,
            quantity=Decimal("1.25"),
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            rationale="test signal",
            created_at="2026-01-02T14:30:00Z",
            strategy_version="strategy-test-v1",
            configuration_version="cfg-test-v1",
            environment="SIMULATION",
        )
        self.assertEqual(intent.as_payload()["quantity"], "1.25")
        self.assertIsNone(intent.as_payload()["limit_price"])

    def test_strategy_context_rejects_noncanonical_id(self) -> None:
        with self.assertRaises(ValueError):
            StrategyContext(
                account_id="ACCOUNT",
                strategy_id="strategy-test-001",
                strategy_version="v1",
                configuration_version="cfg-v1",
                replay_time="2026-01-02T14:30:00Z",
            )
        with self.assertRaises(ValueError):
            StrategyContext(
                account_id="acct.paper.001",
                strategy_id="strategy-test-001",
                strategy_version="v1",
                configuration_version="cfg-v1",
                replay_time="2026-01-02T14:30:00+00:00",
            )

    def test_bar_requires_consistent_ohlc(self) -> None:
        with self.assertRaises(ValueError):
            Bar(
                instrument_id="inst.us_equity.spy",
                open=Decimal("100"),
                high=Decimal("99"),
                low=Decimal("98"),
                close=Decimal("100"),
                volume=Decimal("1"),
                interval_seconds=60,
                exchange_timezone="America/New_York",
            )
        with self.assertRaises(ValueError):
            Bar(
                instrument_id="inst.us_equity.spy",
                open=Decimal("0"),
                high=Decimal("1"),
                low=Decimal("0"),
                close=Decimal("1"),
                volume=Decimal("1"),
                interval_seconds=60,
                exchange_timezone="America/New_York",
            )

    def test_limit_price_follows_order_type(self) -> None:
        with self.assertRaises(ValueError):
            OrderIntent(
                intent_id="intent-test-001", account_id="acct-paper-001", strategy_id="strategy-test-001",
                instrument_id="inst.us_equity.spy", correlation_id="corr-test-001", side=Side.BUY,
                quantity=Decimal("1"), order_type=OrderType.LIMIT, time_in_force=TimeInForce.DAY,
                rationale="test", created_at="2026-01-02T14:30:00Z", strategy_version="v1",
                configuration_version="cfg-v1", environment="SIMULATION",
            )

    def test_provenance_is_stable_for_identical_versioned_inputs(self) -> None:
        dataset = DatasetReference(
            dataset_id="dataset.spy",
            dataset_version="v1",
            reference_data_version="ref-v1",
            universe_id="universe.spy",
            content_hash="b" * 64,
            starts_at="2026-01-02T14:30:00Z",
            ends_at="2026-01-02T14:31:00Z",
        )
        provenance = BacktestProvenance(
            strategy_bundle_hash="a" * 64,
            dataset=dataset,
            configuration_id="config.test",
            configuration_version="cfg-v1",
            configuration_hash="b" * 64,
            seed=7,
            engine_version="engine-v1",
            starts_at="2026-01-02T14:30:00Z",
            ends_at="2026-01-02T14:31:00Z",
        )
        self.assertEqual(provenance.fingerprint(), provenance.fingerprint())
        self.assertEqual(
            provenance.fingerprint(),
            "6c85e1e5453bcb9fedfe95787a14c73bdfbf5b51b35d058821098c00e8a084a3",
        )
        self.assertNotEqual(
            provenance.fingerprint(),
            replace(provenance, configuration_hash="c" * 64).fingerprint(),
        )
        with self.assertRaises(ValueError):
            replace(provenance, configuration_hash="A" * 64)


if __name__ == "__main__":
    unittest.main()
