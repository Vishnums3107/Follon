"""Versioned domain values exposed to isolated strategy workers."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal
from enum import StrEnum
import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .services import StrategyServices


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
    services: StrategyServices | None = None

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


class NewsSource(StrEnum):
    """Declared provenance labels for local-fixture news payloads."""

    DOW_JONES = "DOW_JONES"
    BLOOMBERG_BPIPE = "BLOOMBERG_BPIPE"
    REFINITIV_MRN = "REFINITIV_MRN"
    SEC_EDGAR = "SEC_EDGAR"
    FED_BLS = "FED_BLS"


class EventTaxonomy(StrEnum):
    """Categorized event classification taxonomy."""

    EARNINGS_RELEASE = "EARNINGS_RELEASE"
    GUIDANCE_REVISION = "GUIDANCE_REVISION"
    M_AND_A = "M_AND_A"
    FDA_DECISION = "FDA_DECISION"
    MACRO_CPI = "MACRO_CPI"
    MACRO_FED_RATE = "MACRO_FED_RATE"
    LITIGATION = "LITIGATION"


@dataclass(frozen=True, slots=True)
class NewsHeadlineEvent:
    """Normalized news headline provided to strategy callbacks."""

    news_id: str
    source: NewsSource
    headline: str
    raw_body_hash: str
    sequence_number: int
    event_time_ns: int
    receive_time_ns: int
    entity_tickers: tuple[str, ...]

    def __post_init__(self) -> None:
        _canonical_id("news_id", self.news_id)
        if not isinstance(self.source, NewsSource):
            raise ValueError("source must be a supported NewsSource")
        if not self.headline.strip():
            raise ValueError("headline text cannot be empty")
        if len(self.raw_body_hash) != 64 or any(character not in "0123456789abcdef" for character in self.raw_body_hash):
            raise ValueError("raw_body_hash must be a 64-character SHA256 hex string")
        if (
            not isinstance(self.sequence_number, int)
            or isinstance(self.sequence_number, bool)
            or self.sequence_number < 0
            or not isinstance(self.event_time_ns, int)
            or isinstance(self.event_time_ns, bool)
            or not isinstance(self.receive_time_ns, int)
            or isinstance(self.receive_time_ns, bool)
            or self.event_time_ns <= 0
            or self.receive_time_ns <= 0
        ):
            raise ValueError("event_time_ns and receive_time_ns must be positive")
        for ticker in self.entity_tickers:
            _canonical_id("entity_ticker", ticker)


@dataclass(frozen=True, slots=True)
class SentimentVectorEvent:
    """Extracted sentiment vector event provided to event-driven news strategies."""

    event_id: str
    causation_news_id: str
    event_time_ns: int
    instrument_id: str
    taxonomy: EventTaxonomy
    sentiment_polarity_bps: int
    confidence_bps: int
    novelty_score_bps: int
    surprise_magnitude_bps: int

    def __post_init__(self) -> None:
        _canonical_id("event_id", self.event_id)
        _canonical_id("causation_news_id", self.causation_news_id)
        _canonical_id("instrument_id", self.instrument_id)
        if not isinstance(self.taxonomy, EventTaxonomy):
            raise ValueError("taxonomy must be a supported EventTaxonomy")
        if not isinstance(self.event_time_ns, int) or isinstance(self.event_time_ns, bool) or self.event_time_ns <= 0:
            raise ValueError("event_time_ns must be positive")
        if any(
            not isinstance(value, int) or isinstance(value, bool)
            for value in (
                self.sentiment_polarity_bps,
                self.confidence_bps,
                self.novelty_score_bps,
                self.surprise_magnitude_bps,
            )
        ):
            raise ValueError("news sentiment scores must be integers")
        if not -10000 <= self.sentiment_polarity_bps <= 10000:
            raise ValueError("sentiment_polarity_bps must be between -10000 and 10000")
        if not 0 <= self.confidence_bps <= 10000:
            raise ValueError("confidence_bps must be between 0 and 10000")
        if not 0 <= self.novelty_score_bps <= 10000:
            raise ValueError("novelty_score_bps must be between 0 and 10000")

    @property
    def signal_power_bps(self) -> int:
        """Returns composite signal power in basis points."""
        numerator = self.sentiment_polarity_bps * self.confidence_bps * self.novelty_score_bps
        return numerator // 100_000_000 if numerator >= 0 else -((-numerator) // 100_000_000)
