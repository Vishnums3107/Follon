"""Versioned stdio worker protocol for isolated strategy execution.

The control plane owns market-data, risk, OMS, and accounting authority. This
worker receives one normalized bar at a time plus an optional bounded snapshot
of point-in-time research services, and may return at most one validated
declarative intent. It never receives adapter credentials.
"""

from __future__ import annotations

from argparse import ArgumentParser
from decimal import Decimal
from importlib.util import module_from_spec, spec_from_file_location
import json
from pathlib import Path
import sys
from typing import Any, TextIO

from .bundle import StrategyBundle, strategy_bundle_hash
from .models import Bar, OrderIntent, StrategyContext
from .services import (
    HistoricalDataService,
    MetricsSink,
    PortfolioSnapshot,
    PositionSnapshot,
    StrategyMetric,
    StrategyServices,
    StrategyStateStore,
    TimedBar,
)
from .strategy import Strategy

PROTOCOL_VERSION = 1


class WorkerProtocolError(ValueError):
    """Raised when a frame violates the immutable worker contract."""


def _required_string(frame: dict[str, Any], name: str) -> str:
    value = frame.get(name)
    if not isinstance(value, str) or not value:
        raise WorkerProtocolError(f"{name} must be a non-empty string")
    return value


def _require_exact_fields(frame: dict[str, Any], expected: set[str], name: str) -> None:
    if set(frame) != expected:
        raise WorkerProtocolError(f"{name} has missing or unknown fields")


def _decimal(value: Any, name: str) -> Decimal:
    if not isinstance(value, str):
        raise WorkerProtocolError(f"{name} must be a decimal string")
    try:
        return Decimal(value)
    except Exception as error:  # Decimal exposes implementation-specific exceptions.
        raise WorkerProtocolError(f"{name} must be a valid decimal") from error


def _context(
    payload: dict[str, Any], services: StrategyServices | None = None
) -> StrategyContext:
    _require_exact_fields(
        payload,
        {
            "account_id",
            "strategy_id",
            "strategy_version",
            "configuration_version",
            "replay_time",
            "environment",
        },
        "strategy context",
    )
    return StrategyContext(
        account_id=_required_string(payload, "account_id"),
        strategy_id=_required_string(payload, "strategy_id"),
        strategy_version=_required_string(payload, "strategy_version"),
        configuration_version=_required_string(payload, "configuration_version"),
        replay_time=_required_string(payload, "replay_time"),
        environment=_required_string(payload, "environment"),
        services=services,
    )


def _bar(payload: dict[str, Any]) -> Bar:
    _require_exact_fields(
        payload,
        {
            "instrument_id",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "interval_seconds",
            "exchange_timezone",
        },
        "market bar",
    )
    interval_seconds = payload.get("interval_seconds")
    if not isinstance(interval_seconds, int) or isinstance(interval_seconds, bool):
        raise WorkerProtocolError("interval_seconds must be an integer")
    return Bar(
        instrument_id=_required_string(payload, "instrument_id"),
        open=_decimal(payload.get("open"), "open"),
        high=_decimal(payload.get("high"), "high"),
        low=_decimal(payload.get("low"), "low"),
        close=_decimal(payload.get("close"), "close"),
        volume=_decimal(payload.get("volume"), "volume"),
        interval_seconds=interval_seconds,
        exchange_timezone=_required_string(payload, "exchange_timezone"),
    )


def _services(payload: dict[str, Any], replay_time: str) -> StrategyServices:
    _require_exact_fields(payload, {"history", "portfolio", "state"}, "strategy services")
    history = payload.get("history")
    portfolio = payload.get("portfolio")
    state = payload.get("state")
    if not isinstance(history, dict) or not isinstance(portfolio, dict) or not isinstance(state, dict):
        raise WorkerProtocolError("strategy services require object snapshots")

    _require_exact_fields(history, {"as_of", "records"}, "historical service")
    history_as_of = _required_string(history, "as_of")
    history_records = history.get("records")
    if history_as_of != replay_time or not isinstance(history_records, list):
        raise WorkerProtocolError("historical service must match replay time and contain records")
    parsed_history: list[TimedBar] = []
    for record in history_records:
        if not isinstance(record, dict):
            raise WorkerProtocolError("historical records must be objects")
        _require_exact_fields(record, {"event_time", "bar"}, "historical record")
        record_bar = record.get("bar")
        if not isinstance(record_bar, dict):
            raise WorkerProtocolError("historical record bar must be an object")
        parsed_history.append(
            TimedBar(
                event_time=_required_string(record, "event_time"),
                bar=_bar(record_bar),
            )
        )

    _require_exact_fields(
        portfolio,
        {"as_of", "positions", "cash_by_currency"},
        "portfolio service",
    )
    portfolio_as_of = _required_string(portfolio, "as_of")
    positions = portfolio.get("positions")
    cash = portfolio.get("cash_by_currency")
    if portfolio_as_of != replay_time or not isinstance(positions, list) or not isinstance(cash, list):
        raise WorkerProtocolError("portfolio service must match replay time and contain arrays")
    parsed_positions: list[PositionSnapshot] = []
    for position in positions:
        if not isinstance(position, dict):
            raise WorkerProtocolError("portfolio positions must be objects")
        _require_exact_fields(
            position,
            {"instrument_id", "quantity", "average_cost", "mark_price", "currency"},
            "portfolio position",
        )
        parsed_positions.append(
            PositionSnapshot(
                instrument_id=_required_string(position, "instrument_id"),
                quantity=_decimal(position.get("quantity"), "position quantity"),
                average_cost=_decimal(position.get("average_cost"), "position average_cost"),
                mark_price=_decimal(position.get("mark_price"), "position mark_price"),
                currency=_required_string(position, "currency"),
            )
        )
    parsed_cash: list[tuple[str, Decimal]] = []
    for balance in cash:
        if not isinstance(balance, dict):
            raise WorkerProtocolError("portfolio cash balances must be objects")
        _require_exact_fields(balance, {"currency", "amount"}, "portfolio cash balance")
        parsed_cash.append(
            (
                _required_string(balance, "currency"),
                _decimal(balance.get("amount"), "portfolio cash amount"),
            )
        )

    _require_exact_fields(state, {"values"}, "strategy state")
    state_values = state.get("values")
    if not isinstance(state_values, dict):
        raise WorkerProtocolError("strategy state values must be an object")
    return StrategyServices(
        history=HistoricalDataService(parsed_history, as_of=history_as_of),
        portfolio=PortfolioSnapshot(
            as_of=portfolio_as_of,
            positions=tuple(parsed_positions),
            cash_by_currency=tuple(parsed_cash),
        ),
        state=StrategyStateStore(state_values),
        metrics=MetricsSink(),
    )


def _read_frame(line: str) -> tuple[StrategyContext, Bar, bool]:
    try:
        frame = json.loads(line)
    except json.JSONDecodeError as error:
        raise WorkerProtocolError("frame is not valid JSON") from error
    if not isinstance(frame, dict):
        raise WorkerProtocolError("frame must be a JSON object")
    expected = {"protocol_version", "type", "context", "bar"}
    fields = set(frame)
    if fields not in (expected, expected | {"services"}):
        raise WorkerProtocolError("frame has missing or unknown fields")
    if frame.get("protocol_version") != PROTOCOL_VERSION:
        raise WorkerProtocolError("unsupported worker protocol version")
    if frame.get("type") != "market_bar":
        raise WorkerProtocolError("worker accepts only market_bar frames")
    context = frame.get("context")
    bar = frame.get("bar")
    if not isinstance(context, dict) or not isinstance(bar, dict):
        raise WorkerProtocolError("market_bar requires object context and bar")
    base_context = _context(context)
    service_payload = frame.get("services")
    if service_payload is None:
        return base_context, _bar(bar), False
    if not isinstance(service_payload, dict):
        raise WorkerProtocolError("services must be an object")
    services = _services(service_payload, base_context.replay_time)
    return _context(context, services), _bar(bar), True


def _metric_payload(metric: StrategyMetric) -> dict[str, object]:
    return {
        "name": metric.name,
        "observed_at": metric.observed_at,
        "tags": [{"key": key, "value": value} for key, value in metric.tags],
        "value": str(metric.value),
    }


def _write_frame(stream: TextIO, frame: dict[str, Any]) -> None:
    stream.write(json.dumps(frame, sort_keys=True, separators=(",", ":")) + "\n")
    stream.flush()


def run_worker(
    strategy: Strategy,
    bundle: StrategyBundle,
    source: TextIO = sys.stdin,
    destination: TextIO = sys.stdout,
) -> int:
    """Serves protocol frames until EOF, returning nonzero on contract failure."""

    _write_frame(
        destination,
        {
            "bundle_hash": bundle.bundle_hash,
            "protocol_version": PROTOCOL_VERSION,
            "strategy_id": bundle.strategy_id,
            "strategy_version": bundle.strategy_version,
            "type": "ready",
        },
    )
    for line in source:
        if not line.strip():
            _write_frame(
                destination,
                {"code": "EMPTY_FRAME", "protocol_version": PROTOCOL_VERSION, "type": "error"},
            )
            return 2
        try:
            context, bar, service_frame = _read_frame(line)
            if (
                context.strategy_id != bundle.strategy_id
                or context.strategy_version != bundle.strategy_version
            ):
                raise WorkerProtocolError("context does not match announced strategy bundle")
            intent = strategy.on_bar(context, bar)
            if intent is not None and not isinstance(intent, OrderIntent):
                raise WorkerProtocolError("strategy must return an OrderIntent or None")
            output: dict[str, Any] = {
                "intent": intent.as_payload() if intent is not None else None,
                "protocol_version": PROTOCOL_VERSION,
                "type": "strategy_output",
            }
            if service_frame:
                services = context.services
                if services is None:
                    raise WorkerProtocolError("service frame lost its bounded services")
                output["metrics"] = [
                    _metric_payload(metric) for metric in services.metrics.snapshot()
                ]
                output["state"] = {
                    "fingerprint": services.state.fingerprint(),
                    "values": services.state.snapshot(),
                }
            _write_frame(destination, output)
        except (ValueError, WorkerProtocolError) as error:
            _write_frame(
                destination,
                {
                    "code": "INVALID_FRAME",
                    "message": str(error),
                    "protocol_version": PROTOCOL_VERSION,
                    "type": "error",
                },
            )
            return 2
        except Exception:
            _write_frame(
                destination,
                {
                    "code": "STRATEGY_EXCEPTION",
                    "protocol_version": PROTOCOL_VERSION,
                    "type": "error",
                },
            )
            return 3
    return 0


def _load_strategy(path: Path, class_name: str) -> Strategy:
    spec = spec_from_file_location("follon_user_strategy", path)
    if spec is None or spec.loader is None:
        raise ValueError("strategy source cannot be loaded")
    module = module_from_spec(spec)
    spec.loader.exec_module(module)
    strategy_class = getattr(module, class_name, None)
    if not isinstance(strategy_class, type) or not issubclass(strategy_class, Strategy):
        raise ValueError("strategy class must inherit follon_strategy_sdk.Strategy")
    return strategy_class()


def main(argv: list[str] | None = None) -> int:
    """Runs a worker from an explicit source file and declared bundle root."""

    parser = ArgumentParser(description="Follon isolated strategy worker")
    parser.add_argument("--strategy-file", required=True, type=Path)
    parser.add_argument("--class-name", required=True)
    parser.add_argument("--bundle-root", required=True, type=Path)
    parser.add_argument("--strategy-id", required=True)
    parser.add_argument("--strategy-version", required=True)
    arguments = parser.parse_args(argv)
    try:
        bundle = StrategyBundle(
            strategy_id=arguments.strategy_id,
            strategy_version=arguments.strategy_version,
            bundle_hash=strategy_bundle_hash(arguments.bundle_root),
        )
        return run_worker(_load_strategy(arguments.strategy_file, arguments.class_name), bundle)
    except ValueError as error:
        print(f"strategy worker startup failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
