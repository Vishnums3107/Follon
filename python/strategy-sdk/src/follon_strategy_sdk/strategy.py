"""Strategy extension point deliberately limited to canonical domain values."""

from __future__ import annotations

from abc import ABC, abstractmethod

from .models import Bar, NewsHeadlineEvent, OrderIntent, SentimentVectorEvent, StrategyContext


class Strategy(ABC):
    """A strategy may emit an intent; it cannot access broker credentials or adapters."""

    @abstractmethod
    def on_bar(self, context: StrategyContext, bar: Bar) -> OrderIntent | None:
        """Handle one normalized bar and optionally request one order intent."""

        raise NotImplementedError

    def on_news_sentiment(
        self, context: StrategyContext, event: SentimentVectorEvent
    ) -> OrderIntent | None:
        """Handle one sentiment vector event and optionally request one order intent."""
        return None

    def on_news_headline(
        self, context: StrategyContext, event: NewsHeadlineEvent
    ) -> None:
        """Handle one normalized news headline event for strategy state tracking."""
        pass
