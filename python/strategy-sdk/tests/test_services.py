from decimal import Decimal
import unittest

from follon_strategy_sdk import (
    Bar,
    HistoricalDataService,
    Indicators,
    MetricsSink,
    PortfolioSnapshot,
    PositionSnapshot,
    StrategyMetric,
    StrategyStateStore,
    TimedBar,
)


def bar(close: str) -> Bar:
    value = Decimal(close)
    return Bar(
        instrument_id="inst.us_equity.spy",
        open=value,
        high=value,
        low=value,
        close=value,
        volume=Decimal("10"),
        interval_seconds=60,
        exchange_timezone="America/New_York",
    )


class StrategyServiceTests(unittest.TestCase):
    def test_history_is_point_in_time_bounded_and_rejects_lookahead(self) -> None:
        history = HistoricalDataService(
            [
                TimedBar("2026-01-02T14:30:00Z", bar("100")),
                TimedBar("2026-01-02T14:31:00Z", bar("101")),
            ],
            as_of="2026-01-02T14:31:00Z",
        )
        selected = history.bars(
            "inst.us_equity.spy",
            starts_at="2026-01-02T14:30:00Z",
            ends_at="2026-01-02T14:31:00Z",
        )
        self.assertEqual([item.bar.close for item in selected], [Decimal("100")])
        with self.assertRaises(ValueError):
            history.bars(
                "inst.us_equity.spy",
                starts_at="2026-01-02T14:30:00Z",
                ends_at="2026-01-02T14:32:00Z",
            )

    def test_indicators_state_portfolio_and_metrics_are_deterministic(self) -> None:
        values = [Decimal("1"), Decimal("2"), Decimal("3"), Decimal("4")]
        self.assertEqual(
            Indicators.simple_moving_average(values, 2),
            (Decimal("1.50000000"), Decimal("2.50000000"), Decimal("3.50000000")),
        )
        self.assertEqual(
            Indicators.exponential_moving_average(values, 2),
            (Decimal("1.50000000"), Decimal("2.50000000"), Decimal("3.50000000")),
        )
        state = StrategyStateStore({"signal.count": 1})
        fingerprint = state.fingerprint()
        state.set("signal.count", 2)
        self.assertNotEqual(fingerprint, state.fingerprint())
        self.assertEqual(state.snapshot(), {"signal.count": 2})

        portfolio = PortfolioSnapshot(
            as_of="2026-01-02T14:31:00Z",
            positions=(
                PositionSnapshot(
                    instrument_id="inst.us_equity.spy",
                    quantity=Decimal("2"),
                    average_cost=Decimal("100"),
                    mark_price=Decimal("101"),
                    currency="USD",
                ),
            ),
            cash_by_currency=(("USD", Decimal("1000")),),
        )
        self.assertEqual(portfolio.position("inst.us_equity.spy").quantity, Decimal("2"))
        with self.assertRaises(ValueError):
            PositionSnapshot(
                instrument_id="inst.us_equity.spy",
                quantity=Decimal("1"),
                average_cost=Decimal("100"),
                mark_price=Decimal("101"),
                currency="ÜSD",
            )

        metrics = MetricsSink()
        metrics.emit(
            StrategyMetric(
                name="signal.strength",
                value=Decimal("0.75"),
                observed_at="2026-01-02T14:31:00Z",
                tags=(("regime", "trend"),),
            )
        )
        self.assertEqual(metrics.snapshot()[0].value, Decimal("0.75"))


if __name__ == "__main__":
    unittest.main()
