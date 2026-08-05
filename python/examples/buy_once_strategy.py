"""First vertical-slice strategy: one canonical intent from one historical bar."""

from decimal import Decimal

from follon_strategy_sdk import Bar, OrderIntent, OrderType, Side, Strategy, StrategyContext, TimeInForce


class BuyOnceStrategy(Strategy):
    """Buys one unit once when the normalized closing price meets a threshold."""

    def __init__(self, threshold: Decimal) -> None:
        self._threshold = threshold
        self._submitted = False

    def on_bar(self, context: StrategyContext, bar: Bar) -> OrderIntent | None:
        if self._submitted or bar.close > self._threshold:
            return None
        self._submitted = True
        return OrderIntent(
            intent_id="intent-example-000001",
            account_id=context.account_id,
            strategy_id=context.strategy_id,
            instrument_id=bar.instrument_id,
            correlation_id="corr-example-000001",
            side=Side.BUY,
            quantity=Decimal("1"),
            order_type=OrderType.MARKET,
            time_in_force=TimeInForce.DAY,
            rationale="close crossed configured entry threshold",
            created_at=context.replay_time,
            strategy_version=context.strategy_version,
            configuration_version=context.configuration_version,
            environment=context.environment,
        )
