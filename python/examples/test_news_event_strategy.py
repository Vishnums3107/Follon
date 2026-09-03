import unittest
from decimal import Decimal

from follon_strategy_sdk import (
    EventTaxonomy,
    OrderType,
    SentimentVectorEvent,
    Side,
    StrategyContext,
)
from news_event_strategy import NewsEarningsMomentumStrategy


class NewsStrategyTests(unittest.TestCase):
    def test_news_earnings_momentum_strategy(self) -> None:
        strategy = NewsEarningsMomentumStrategy(min_confidence_bps=8000, min_novelty_bps=7500)
        context = StrategyContext(
            account_id="acct.paper.001",
            strategy_id="strategy.news.momentum",
            strategy_version="v1",
            configuration_version="cfg.v1",
            replay_time="2026-09-01T14:00:00Z",
            environment="SIMULATION",
        )

        # 1. High confidence positive earnings beat -> Emits BUY OrderIntent
        positive_event = SentimentVectorEvent(
            event_id="sent.001",
            causation_news_id="news.001",
            event_time_ns=1700000000000000000,
            instrument_id="aapl.us",
            taxonomy=EventTaxonomy.EARNINGS_RELEASE,
            sentiment_polarity_bps=9000,   # +0.9000
            confidence_bps=9500,           # 95.00%
            novelty_score_bps=10000,       # 100.00%
            surprise_magnitude_bps=250,
        )
        intent = strategy.on_news_sentiment(context, positive_event)
        self.assertIsNotNone(intent)
        self.assertEqual(intent.side, Side.BUY)
        self.assertEqual(intent.instrument_id, "aapl.us")
        self.assertEqual(intent.quantity, Decimal("4"))

        # 2. Duplicate event -> Ignored (returns None)
        duplicate_intent = strategy.on_news_sentiment(context, positive_event)
        self.assertIsNone(duplicate_intent)

        # 3. Low confidence event -> Ignored (returns None)
        low_conf_event = SentimentVectorEvent(
            event_id="sent.002",
            causation_news_id="news.002",
            event_time_ns=1700000000000000000,
            instrument_id="tsla.us",
            taxonomy=EventTaxonomy.EARNINGS_RELEASE,
            sentiment_polarity_bps=9000,
            confidence_bps=5000,  # 50% < 80% threshold
            novelty_score_bps=10000,
            surprise_magnitude_bps=0,
        )
        self.assertIsNone(strategy.on_news_sentiment(context, low_conf_event))


if __name__ == "__main__":
    unittest.main()
