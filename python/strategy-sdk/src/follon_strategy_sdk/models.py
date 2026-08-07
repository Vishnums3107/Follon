"""Versioned domain values exposed to isolated strategy workers."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal
from enum import StrEnum
import re


_UTC_TIMESTAMP = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z")


def _canonical_id(name: str, value: str) -> str:
    if not value or any(character not in "abcdefghijklmnopqrstuvwxyz0123456789._-" for character in value):
        raise ValueError(f"{name} must be a non-empty canonical ID")
    return value


def _decimal(name: str, value: Decimal, *, positive: bool = False) -> Decimal:
    if not value.is_finite() or (positive and value <= Decimal("0")):
        raise ValueError(f"{name} must be {'a positive ' if positive else 'a finite '}decimal")
    exponent = value.as_tuple().exponent
    if exponent < -8:
        raise ValueError(f"{name} supports at most eight fractional digits")
    return value


def _utc(name: str, value: str) -> str:
    if not isinstance(value, str) or _UTC_TIMESTAMP.fullmatch(value) is None:
        raise ValueError(f"{name} must be canonical second-precision UTC")
    try:
        datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as error:
        raise ValueError(f"{name} must be canonical second-precision UTC") from error
    return value


class Side(StrEnum):
    """Direction requested by an intent."""

    BUY = "BUY"
    SELL = "SELL"


class OrderType(StrEnum):
    """First-slice order types."""

    MARKET = "MARKET"
    LIMIT = "LIMIT"


class TimeInForce(StrEnum):
    """Supported order persistence policies."""

    DAY = "DAY"
    GTC = "GTC"


@dataclass(frozen=True, slots=True)
class Bar:
    """Normalized bar provided by the control plane, never a broker SDK object."""

    instrument_id: str
    open: Decimal
    high: Decimal
    low: Decimal
    close: Decimal
    volume: Decimal
    interval_seconds: int
    exchange_timezone: str

    def __post_init__(self) -> None:
        _canonical_id("instrument_id", self.instrument_id)
        for name in ("open", "high", "low", "close"):
            _decimal(name, getattr(self, name), positive=True)
        _decimal("volume", self.volume)
        if self.interval_seconds <= 0 or self.volume < 0:
            raise ValueError("bar interval must be positive and volume cannot be negative")
        if not self.low <= self.open <= self.high or not self.low <= self.close <= self.high:
            raise ValueError("bar OHLC relationship is invalid")
        if not self.exchange_timezone:
            raise ValueError("exchange_timezone is required")


@dataclass(frozen=True, slots=True)
class StrategyContext:
    """Replay metadata supplied to every strategy callback."""

    account_id: str
    strategy_id: str
    strategy_version: str
    configuration_version: str
    replay_time: str
    environment: str = "SIMULATION"

    def __post_init__(self) -> None:
        _canonical_id("account_id", self.account_id)
        _canonical_id("strategy_id", self.strategy_id)
        if not self.strategy_version or not self.configuration_version:
            raise ValueError("strategy and replay versions/timestamp are required")
        _utc("replay_time", self.replay_time)
        if self.environment not in {"SIMULATION", "PAPER", "LIVE"}:
            raise ValueError("unsupported environment")


@dataclass(frozen=True, slots=True)
class OrderIntent:
    """Declarative request sent to the Rust risk boundary; never a broker order."""

    intent_id: str
    account_id: str
    strategy_id: str
    instrument_id: str
    correlation_id: str
    side: Side
    quantity: Decimal
    order_type: OrderType
    time_in_force: TimeInForce
    rationale: str
    created_at: str
    strategy_version: str
    configuration_version: str
    environment: str
    limit_price: Decimal | None = None

    def __post_init__(self) -> None:
        for name in ("intent_id", "account_id", "strategy_id", "instrument_id", "correlation_id"):
            _canonical_id(name, getattr(self, name))
        _decimal("quantity", self.quantity, positive=True)
        if self.limit_price is not None:
            _decimal("limit_price", self.limit_price, positive=True)
        if (self.order_type is OrderType.LIMIT) != (self.limit_price is not None):
            raise ValueError("limit_price must be present only for a limit order")
        if not self.rationale or not self.strategy_version or not self.configuration_version:
            raise ValueError("intent rationale, timestamp, and versions are required")
        _utc("created_at", self.created_at)
        if self.environment not in {"SIMULATION", "PAPER", "LIVE"}:
            raise ValueError("unsupported environment")

    def as_payload(self) -> dict[str, str | None]:
        """Returns a JSON-safe payload using exact decimal strings."""

        return {
            "intent_id": self.intent_id,
            "account_id": self.account_id,
            "strategy_id": self.strategy_id,
            "instrument_id": self.instrument_id,
            "correlation_id": self.correlation_id,
            "side": self.side.value,
            "quantity": format(self.quantity, "f"),
            "order_type": self.order_type.value,
            "limit_price": format(self.limit_price, "f") if self.limit_price is not None else None,
            "time_in_force": self.time_in_force.value,
            "rationale": self.rationale,
            "created_at": self.created_at,
            "strategy_version": self.strategy_version,
            "configuration_version": self.configuration_version,
            "environment": self.environment,
        }
