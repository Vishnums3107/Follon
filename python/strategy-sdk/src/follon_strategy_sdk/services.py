"""Bounded research services exposed to isolated strategy code.

These objects contain immutable platform snapshots or worker-local state. They
never expose a broker client, credential, socket, filesystem path, or wall
clock.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal, ROUND_HALF_EVEN
import hashlib
import json
from typing import Iterable

from .models import Bar, _canonical_id, _decimal, _utc


_QUANTUM = Decimal("0.00000001")
_MAX_HISTORY_RECORDS = 1_000_000
_MAX_STATE_BYTES = 65_536
_MAX_METRICS = 10_000


def _currency(code: str) -> str:
    if len(code) != 3 or any(character not in "ABCDEFGHIJKLMNOPQRSTUVWXYZ" for character in code):
        raise ValueError("currency must be a three-letter uppercase ASCII code")
    return code


@dataclass(frozen=True, slots=True)
class TimedBar:
    """One normalized bar bound to its canonical event time."""

    event_time: str
    bar: Bar

    def __post_init__(self) -> None:
        _utc("historical bar event_time", self.event_time)


class HistoricalDataService:
    """Point-in-time historical queries over a frozen ordered snapshot."""

    def __init__(self, records: Iterable[TimedBar], *, as_of: str) -> None:
        self._as_of = _utc("historical service as_of", as_of)
        ordered = tuple(sorted(records, key=lambda item: (item.event_time, item.bar.instrument_id)))
        if len(ordered) > _MAX_HISTORY_RECORDS:
            raise ValueError("historical snapshot exceeds the worker boundary")
        identities: set[tuple[str, str]] = set()
        for record in ordered:
            identity = (record.event_time, record.bar.instrument_id)
            if identity in identities:
                raise ValueError("historical snapshot contains a duplicate instrument/time")
            if record.event_time > self._as_of:
                raise ValueError("historical snapshot contains look-ahead data")
            identities.add(identity)
        self._records = ordered

    def bars(
        self,
        instrument_id: str,
        *,
        starts_at: str,
        ends_at: str,
        limit: int = 10_000,
    ) -> tuple[TimedBar, ...]:
        """Return an inclusive-start/exclusive-end window without look-ahead."""

        _canonical_id("historical instrument_id", instrument_id)
        starts_at = _utc("historical starts_at", starts_at)
        ends_at = _utc("historical ends_at", ends_at)
        if starts_at >= ends_at or ends_at > self._as_of or not 1 <= limit <= 100_000:
            raise ValueError("invalid point-in-time historical query")
        selected = tuple(
            record
            for record in self._records
            if record.bar.instrument_id == instrument_id
            and starts_at <= record.event_time < ends_at
        )
        if len(selected) > limit:
            raise ValueError("historical query exceeds its explicit result limit")
        return selected


@dataclass(frozen=True, slots=True)
class PositionSnapshot:
    """Exact broker-independent position visible to a strategy."""

    instrument_id: str
    quantity: Decimal
    average_cost: Decimal
    mark_price: Decimal
    currency: str

    def __post_init__(self) -> None:
        _canonical_id("portfolio instrument_id", self.instrument_id)
        _decimal("portfolio quantity", self.quantity)
        _decimal("portfolio average_cost", self.average_cost)
        _decimal("portfolio mark_price", self.mark_price, positive=True)
        if self.average_cost < 0:
            raise ValueError("invalid portfolio position snapshot")
        _currency(self.currency)


@dataclass(frozen=True, slots=True)
class PortfolioSnapshot:
    """Immutable cash and position state at one replay instant."""

    as_of: str
    positions: tuple[PositionSnapshot, ...]
    cash_by_currency: tuple[tuple[str, Decimal], ...]

    def __post_init__(self) -> None:
        _utc("portfolio as_of", self.as_of)
        if len({position.instrument_id for position in self.positions}) != len(self.positions):
            raise ValueError("portfolio positions must be unique")
        currencies: set[str] = set()
        for currency, amount in self.cash_by_currency:
            _currency(currency)
            if currency in currencies:
                raise ValueError("portfolio cash currencies must be unique ISO codes")
            _decimal("portfolio cash", amount)
            currencies.add(currency)

    def position(self, instrument_id: str) -> PositionSnapshot | None:
        """Look up one position by canonical identity."""

        _canonical_id("portfolio instrument_id", instrument_id)
        return next(
            (position for position in self.positions if position.instrument_id == instrument_id),
            None,
        )


class Indicators:
    """Deterministic fixed-point indicator helpers."""

    @staticmethod
    def simple_moving_average(values: Iterable[Decimal], window: int) -> tuple[Decimal, ...]:
        """Return rolling means after a complete positive window exists."""

        values = tuple(_decimal("indicator value", value) for value in values)
        if not 1 <= window <= 100_000 or window > len(values):
            raise ValueError("invalid moving-average window")
        divisor = Decimal(window)
        return tuple(
            (sum(values[index - window:index], Decimal("0")) / divisor).quantize(
                _QUANTUM, rounding=ROUND_HALF_EVEN
            )
            for index in range(window, len(values) + 1)
        )

    @staticmethod
    def exponential_moving_average(values: Iterable[Decimal], window: int) -> tuple[Decimal, ...]:
        """Return a deterministic EMA seeded by the first complete-window SMA."""

        values = tuple(_decimal("indicator value", value) for value in values)
        if not 1 <= window <= 100_000 or window > len(values):
            raise ValueError("invalid exponential-moving-average window")
        alpha = Decimal(2) / Decimal(window + 1)
        seed = Indicators.simple_moving_average(values[:window], window)[0]
        output = [seed]
        for value in values[window:]:
            output.append(
                (alpha * value + (Decimal(1) - alpha) * output[-1]).quantize(
                    _QUANTUM, rounding=ROUND_HALF_EVEN
                )
            )
        return tuple(output)


class StrategyStateStore:
    """Bounded JSON state whose fingerprint can be retained with replay evidence."""

    def __init__(self, initial: dict[str, object] | None = None) -> None:
        self._values: dict[str, object] = {}
        for key, value in (initial or {}).items():
            self.set(key, value)

    def get(self, key: str, default: object | None = None) -> object | None:
        """Read one canonical state key."""

        _canonical_id("strategy state key", key)
        return self._values.get(key, default)

    def set(self, key: str, value: object) -> None:
        """Set one JSON-safe value if the complete state remains bounded."""

        _canonical_id("strategy state key", key)
        candidate = dict(self._values)
        candidate[key] = value
        canonical = _canonical_state(candidate)
        if len(canonical.encode("utf-8")) > _MAX_STATE_BYTES:
            raise ValueError("strategy state exceeds the 64 KiB boundary")
        self._values = candidate

    def snapshot(self) -> dict[str, object]:
        """Return a detached JSON round-tripped snapshot."""

        return json.loads(_canonical_state(self._values))

    def fingerprint(self) -> str:
        """Return the SHA-256 of canonical state bytes."""

        return hashlib.sha256(_canonical_state(self._values).encode("utf-8")).hexdigest()


@dataclass(frozen=True, slots=True)
class StrategyMetric:
    """One exact, timestamped custom strategy metric."""

    name: str
    value: Decimal
    observed_at: str
    tags: tuple[tuple[str, str], ...] = ()

    def __post_init__(self) -> None:
        _canonical_id("metric name", self.name)
        _decimal("metric value", self.value)
        _utc("metric observed_at", self.observed_at)
        if len(self.tags) > 32 or len({key for key, _ in self.tags}) != len(self.tags):
            raise ValueError("metric tags must be unique and bounded")
        for key, value in self.tags:
            _canonical_id("metric tag key", key)
            _canonical_id("metric tag value", value)


class MetricsSink:
    """Bounded worker-local metric collector returned to the control plane."""

    def __init__(self) -> None:
        self._metrics: list[StrategyMetric] = []

    def emit(self, metric: StrategyMetric) -> None:
        """Append one already validated metric."""

        if len(self._metrics) >= _MAX_METRICS:
            raise ValueError("strategy metric limit exceeded")
        self._metrics.append(metric)

    def snapshot(self) -> tuple[StrategyMetric, ...]:
        """Return metrics in deterministic emission order."""

        return tuple(self._metrics)


@dataclass(frozen=True, slots=True)
class StrategyServices:
    """Complete broker-independent service set for one callback."""

    history: HistoricalDataService
    portfolio: PortfolioSnapshot
    state: StrategyStateStore
    metrics: MetricsSink


def _canonical_state(values: dict[str, object]) -> str:
    try:
        return json.dumps(values, sort_keys=True, separators=(",", ":"), allow_nan=False)
    except (TypeError, ValueError) as error:
        raise ValueError("strategy state must contain JSON-safe finite values") from error
