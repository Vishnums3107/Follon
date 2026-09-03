from decimal import Decimal
from io import StringIO
import json
from pathlib import Path
import unittest

from follon_strategy_sdk import (
    EventTaxonomy,
    NewsHeadlineEvent,
    OrderIntent,
    OrderType,
    Side,
    Strategy,
    StrategyBundle,
    StrategyMetric,
    TimeInForce,
    strategy_bundle_hash,
)
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


class NewsCallbackStrategy(Strategy):
    def __init__(self) -> None:
        self.headlines: list[str] = []

    def on_bar(self, context, bar):
        return None

    def on_news_headline(self, context, event: NewsHeadlineEvent):
        self.headlines.append(event.news_id)
        return None

    def on_news_sentiment(self, context, event):
        if event.taxonomy is not EventTaxonomy.EARNINGS_RELEASE or event.signal_power_bps < 7000:
            return None
        return OrderIntent(
            intent_id=f"intent-{event.event_id}",
            account_id=context.account_id,
            strategy_id=context.strategy_id,
            instrument_id=event.instrument_id,
            correlation_id=f"corr-{event.event_id}",
            side=Side.BUY,
            quantity=Decimal("1"),
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            rationale="news sentiment worker test",
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

    def test_worker_dispatches_headline_and_sentiment_frames(self) -> None:
        context = {
            "account_id": "acct-paper-001",
            "strategy_id": "strategy-worker-001",
            "strategy_version": "v1",
            "configuration_version": "cfg-v1",
            "replay_time": "2026-09-01T11:00:00Z",
            "environment": "SIMULATION",
        }
        headline = {
            "protocol_version": PROTOCOL_VERSION,
            "type": "news_headline",
            "context": context,
            "headline": {
                "news_id": "news.fixture.001",
                "source": "DOW_JONES",
                "headline": "Apple reports earnings beat",
                "raw_body_hash": "a" * 64,
                "sequence_number": 1,
                "event_time_ns": 1788260400000000000,
                "receive_time_ns": 1788260400000000001,
                "entity_tickers": ["aapl.us"],
            },
        }
        sentiment = {
            "protocol_version": PROTOCOL_VERSION,
            "type": "news_sentiment",
            "context": context,
            "sentiment": {
                "event_id": "sent.news.fixture.001.1",
                "causation_news_id": "news.fixture.001",
                "event_time_ns": 1788260400000000000,
                "instrument_id": "aapl.us",
                "taxonomy": "EARNINGS_RELEASE",
                "sentiment_polarity_bps": 9000,
                "confidence_bps": 9000,
                "novelty_score_bps": 10000,
                "surprise_magnitude_bps": 250,
            },
        }
        bundle = StrategyBundle(
            strategy_id="strategy-worker-001",
            strategy_version="v1",
            bundle_hash="a" * 64,
        )
        strategy = NewsCallbackStrategy()
        destination = StringIO()
        result = run_worker(
            strategy,
            bundle,
            StringIO(json.dumps(headline) + "\n" + json.dumps(sentiment) + "\n"),
            destination,
        )
        self.assertEqual(result, 0)
        ready, headline_output, sentiment_output = [
            json.loads(line) for line in destination.getvalue().splitlines()
        ]
        self.assertEqual(ready["type"], "ready")
        self.assertEqual(headline_output["type"], "strategy_output")
        self.assertIsNone(headline_output["intent"])
        self.assertEqual(strategy.headlines, ["news.fixture.001"])
        self.assertEqual(sentiment_output["intent"]["instrument_id"], "aapl.us")

    def test_worker_rejects_unknown_news_fields(self) -> None:
        frame = {
            "protocol_version": PROTOCOL_VERSION,
            "type": "news_sentiment",
            "context": {
                "account_id": "acct-paper-001",
                "strategy_id": "strategy-worker-001",
                "strategy_version": "v1",
                "configuration_version": "cfg-v1",
                "replay_time": "2026-09-01T11:00:00Z",
                "environment": "SIMULATION",
            },
            "sentiment": {"unexpected": True},
        }
        destination = StringIO()
        bundle = StrategyBundle(
            strategy_id="strategy-worker-001",
            strategy_version="v1",
            bundle_hash="a" * 64,
        )
        self.assertEqual(
            run_worker(NewsCallbackStrategy(), bundle, StringIO(json.dumps(frame) + "\n"), destination),
            2,
        )
        frames = [json.loads(line) for line in destination.getvalue().splitlines()]
        self.assertEqual(frames[-1]["code"], "INVALID_FRAME")

    def test_bundle_hash_is_stable_for_the_declared_sdk_source_tree(self) -> None:
        source_root = Path(__file__).parents[1] / "src" / "follon_strategy_sdk"
        self.assertEqual(strategy_bundle_hash(source_root), strategy_bundle_hash(source_root))

    def test_worker_round_trips_bounded_services_state_and_metrics(self) -> None:
        market_bar = {
            "instrument_id": "inst.us_equity.spy",
            "open": "100",
            "high": "101",
            "low": "99",
            "close": "100",
            "volume": "10",
            "interval_seconds": 60,
            "exchange_timezone": "America/New_York",
        }
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
            "bar": market_bar,
            "services": {
                "history": {
                    "as_of": "2026-01-02T14:30:00Z",
                    "records": [
                        {
                            "event_time": "2026-01-02T14:29:00Z",
                            "bar": market_bar,
                        }
                    ],
                },
                "portfolio": {
                    "as_of": "2026-01-02T14:30:00Z",
                    "positions": [
                        {
                            "instrument_id": "inst.us_equity.spy",
                            "quantity": "1",
                            "average_cost": "99",
                            "mark_price": "100",
                            "currency": "USD",
                        }
                    ],
                    "cash_by_currency": [{"currency": "USD", "amount": "1000"}],
                },
                "state": {"values": {"callback.count": 1}},
            },
        }
        destination = StringIO()
        bundle = StrategyBundle(
            strategy_id="strategy-worker-001",
            strategy_version="v1",
            bundle_hash="a" * 64,
        )
        self.assertEqual(
            run_worker(
                ServiceAwareStrategy(),
                bundle,
                StringIO(json.dumps(frame) + "\n"),
                destination,
            ),
            0,
        )
        _, output = [json.loads(line) for line in destination.getvalue().splitlines()]
        self.assertIsNone(output["intent"])
        self.assertEqual(output["state"]["values"], {"callback.count": 2})
        self.assertEqual(len(output["state"]["fingerprint"]), 64)
        self.assertEqual(output["metrics"][0]["value"], "2")

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


class ServiceAwareStrategy(Strategy):
    def on_bar(self, context, bar):
        services = context.services
        if services is None:
            raise ValueError("bounded services are required")
        history = services.history.bars(
            bar.instrument_id,
            starts_at="2026-01-02T14:29:00Z",
            ends_at=context.replay_time,
        )
        if len(history) != 1 or services.portfolio.position(bar.instrument_id) is None:
            raise ValueError("service snapshot is incomplete")
        count = services.state.get("callback.count", 0)
        services.state.set("callback.count", count + 1)
        services.metrics.emit(
            StrategyMetric(
                name="callback.count",
                value=Decimal(str(count + 1)),
                observed_at=context.replay_time,
            )
        )
        return None

        self.assertEqual(run_worker(EmitOneIntent(), bundle, source, destination), 2)
        frames = [json.loads(line) for line in destination.getvalue().splitlines()]
        self.assertEqual(frames[1]["code"], "INVALID_FRAME")


if __name__ == "__main__":
    unittest.main()
