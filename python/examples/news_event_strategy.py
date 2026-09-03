"""Event-driven news trading strategy using the Follon Python Strategy SDK."""

from __future__ import annotations

from decimal import Decimal

from follon_strategy_sdk import (
    Bar,
    EventTaxonomy,
    OrderIntent,
    OrderType,
    SentimentVectorEvent,
    Side,
    Strategy,
    StrategyContext,
    TimeInForce,
)


class NewsEarningsMomentumStrategy(Strategy):
    """Replay-only example that emits declarative intents from sentiment vectors."""

    def __init__(self, min_confidence_bps: int = 8000, min_novelty_bps: int = 7500) -> None:
        self._min_confidence_bps = min_confidence_bps
        self._min_novelty_bps = min_novelty_bps
        self._processed_events: set[str] = set()

    def on_bar(self, context: StrategyContext, bar: Bar) -> OrderIntent | None:
        """No bar-based trades; strategy is driven by news sentiment events."""
        return None

    def on_news_sentiment(
        self, context: StrategyContext, event: SentimentVectorEvent
    ) -> OrderIntent | None:
        """Evaluates incoming sentiment vector and emits an OrderIntent if signal threshold passes."""
        if event.event_id in self._processed_events:
            return None

        # Filter low confidence, low novelty, or non-earnings/guidance events
        if (
            event.confidence_bps < self._min_confidence_bps
            or event.novelty_score_bps < self._min_novelty_bps
            or event.taxonomy not in {EventTaxonomy.EARNINGS_RELEASE, EventTaxonomy.GUIDANCE_REVISION}
        ):
            return None

        # Compute signal power (-10000 to +10000 bps)
        signal_power = event.signal_power_bps
        if abs(signal_power) < 4000:  # Minimum 40.00% signal strength
            return None

        self._processed_events.add(event.event_id)
        side = Side.BUY if signal_power > 0 else Side.SELL

        # Scale quantity (1 to 5 units based on signal power)
        units = max(1, min(5, abs(signal_power) // 2000))
        quantity = Decimal(str(units))

        return OrderIntent(
            intent_id=f"intent-news-{event.event_id}",
            account_id=context.account_id,
            strategy_id=context.strategy_id,
            instrument_id=event.instrument_id,
            correlation_id=f"corr-news-{event.event_id}",
            side=side,
            quantity=quantity,
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            rationale=f"News sentiment {event.taxonomy.value} polarity={event.sentiment_polarity_bps}bps power={signal_power}bps",
            created_at=context.replay_time,
            strategy_version=context.strategy_version,
            configuration_version=context.configuration_version,
            environment=context.environment,
        )
