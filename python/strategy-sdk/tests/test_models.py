from decimal import Decimal
from dataclasses import replace
import unittest

from follon_strategy_sdk import (
    BacktestProvenance,
    Bar,
    DatasetReference,
    EventTaxonomy,
    NewsHeadlineEvent,
    NewsSource,
    OrderIntent,
    OrderType,
    SentimentVectorEvent,
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

    def test_limit_price_follows_order_type(self) -> None:
        with self.assertRaises(ValueError):
            OrderIntent(
                intent_id="intent-test-001", account_id="acct-paper-001", strategy_id="strategy-test-001",
                instrument_id="inst.us_equity.spy", correlation_id="corr-test-001", side=Side.BUY,
                quantity=Decimal("1"), order_type=OrderType.LIMIT, time_in_force=TimeInForce.DAY,
                rationale="test", created_at="2026-01-02T14:30:00Z", strategy_version="v1",
                configuration_version="cfg-v1", environment="SIMULATION",
            )

    def test_news_headline_and_sentiment_vector_contract(self) -> None:
        headline = NewsHeadlineEvent(
            news_id="news.dj.001",
            source=NewsSource.DOW_JONES,
            headline="Apple Earnings Beat",
            raw_body_hash="a" * 64,
            sequence_number=1,
            event_time_ns=1000,
            receive_time_ns=1050,
            entity_tickers=("aapl.us",),
        )
        self.assertEqual(headline.news_id, "news.dj.001")

        sentiment = SentimentVectorEvent(
            event_id="sent.001",
            causation_news_id="news.dj.001",
            event_time_ns=1000,
            instrument_id="aapl.us",
            taxonomy=EventTaxonomy.EARNINGS_RELEASE,
            sentiment_polarity_bps=8000,   # +0.8000
            confidence_bps=9000,           # 90.00%
            novelty_score_bps=10000,       # 100.00%
            surprise_magnitude_bps=200,
        )
        self.assertEqual(sentiment.signal_power_bps, 7200)

        # Rust integer division truncates toward zero; retain that cross-language
        # contract for negative news signals instead of Python's floor division.
        negative = replace(
            sentiment,
            event_id="sent.negative.001",
            sentiment_polarity_bps=-1,
            confidence_bps=1,
            novelty_score_bps=1,
        )
        self.assertEqual(negative.signal_power_bps, 0)

        with self.assertRaises(ValueError):
            SentimentVectorEvent(
                event_id="sent.002",
                causation_news_id="news.dj.001",
                event_time_ns=1000,
                instrument_id="aapl.us",
                taxonomy=EventTaxonomy.EARNINGS_RELEASE,
                sentiment_polarity_bps=15000,  # Out of bounds > 10000
                confidence_bps=9000,
                novelty_score_bps=10000,
                surprise_magnitude_bps=0,
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


if __name__ == "__main__":
    unittest.main()
