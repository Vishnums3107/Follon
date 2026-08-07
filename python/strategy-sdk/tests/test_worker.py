from decimal import Decimal
from io import StringIO
import json
from pathlib import Path
import unittest

from follon_strategy_sdk import OrderIntent, OrderType, Side, Strategy, StrategyBundle, TimeInForce, strategy_bundle_hash
from follon_strategy_sdk.worker import PROTOCOL_VERSION, run_worker


class EmitOneIntent(Strategy):
    def on_bar(self, context, bar):
        return OrderIntent(
            intent_id="intent-worker-001",
            account_id=context.account_id,
            strategy_id=context.strategy_id,
            instrument_id=bar.instrument_id,
            correlation_id="corr-worker-001",
            side=Side.BUY,
            quantity=Decimal("1"),
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            rationale="worker test",
            created_at=context.replay_time,
            strategy_version=context.strategy_version,
            configuration_version=context.configuration_version,
            environment=context.environment,
        )


class WorkerProtocolTests(unittest.TestCase):
    def test_worker_announces_bundle_then_returns_canonical_intent(self) -> None:
        frame = {
            "protocol_version": PROTOCOL_VERSION,
            "type": "market_bar",
            "context": {
                "account_id": "acct-paper-001",
                "strategy_id": "strategy-worker-001",
                "strategy_version": "v1",
                "configuration_version": "cfg-v1",
                "replay_time": "2026-01-02T14:30:00Z",
                "environment": "SIMULATION",
            },
            "bar": {
                "instrument_id": "inst.us_equity.spy",
                "open": "100",
                "high": "101",
                "low": "99",
                "close": "100",
                "volume": "10",
                "interval_seconds": 60,
                "exchange_timezone": "America/New_York",
            },
        }
        source = StringIO(json.dumps(frame) + "\n")
        destination = StringIO()
        bundle = StrategyBundle(
            strategy_id="strategy-worker-001",
            strategy_version="v1",
            bundle_hash="a" * 64,
        )

        self.assertEqual(run_worker(EmitOneIntent(), bundle, source, destination), 0)
        ready, output = [json.loads(line) for line in destination.getvalue().splitlines()]
        self.assertEqual(ready["type"], "ready")
        self.assertEqual(ready["bundle_hash"], "a" * 64)
        self.assertEqual(output["type"], "strategy_output")
        self.assertEqual(output["intent"]["quantity"], "1")

    def test_bundle_hash_is_stable_for_the_declared_sdk_source_tree(self) -> None:
        source_root = Path(__file__).parents[1] / "src" / "follon_strategy_sdk"
        self.assertEqual(strategy_bundle_hash(source_root), strategy_bundle_hash(source_root))

    def test_worker_rejects_unknown_protocol_fields(self) -> None:
        frame = {
            "protocol_version": PROTOCOL_VERSION,
            "type": "market_bar",
            "unexpected": True,
            "context": {
                "account_id": "acct-paper-001",
                "strategy_id": "strategy-worker-001",
                "strategy_version": "v1",
                "configuration_version": "cfg-v1",
                "replay_time": "2026-01-02T14:30:00Z",
                "environment": "SIMULATION",
            },
            "bar": {
                "instrument_id": "inst.us_equity.spy",
                "open": "100",
                "high": "101",
                "low": "99",
                "close": "100",
                "volume": "10",
                "interval_seconds": 60,
                "exchange_timezone": "America/New_York",
            },
        }
        source = StringIO(json.dumps(frame) + "\n")
        destination = StringIO()
        bundle = StrategyBundle(
            strategy_id="strategy-worker-001",
            strategy_version="v1",
            bundle_hash="a" * 64,
        )

        self.assertEqual(run_worker(EmitOneIntent(), bundle, source, destination), 2)
        frames = [json.loads(line) for line in destination.getvalue().splitlines()]
        self.assertEqual(frames[1]["code"], "INVALID_FRAME")


if __name__ == "__main__":
    unittest.main()
