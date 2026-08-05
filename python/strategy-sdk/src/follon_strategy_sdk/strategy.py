"""Strategy extension point deliberately limited to canonical domain values."""

from __future__ import annotations

from abc import ABC, abstractmethod

from .models import Bar, OrderIntent, StrategyContext


class Strategy(ABC):
    """A strategy may emit an intent; it cannot access broker credentials or adapters."""

    @abstractmethod
    def on_bar(self, context: StrategyContext, bar: Bar) -> OrderIntent | None:
        """Handle one normalized bar and optionally request one order intent."""

        raise NotImplementedError
