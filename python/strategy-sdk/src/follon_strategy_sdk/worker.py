"""Versioned stdio worker protocol for isolated strategy execution.

The Rust control plane owns all market-data, risk, OMS, and accounting logic.
This worker receives one normalized bar at a time and may return at most one
validated declarative intent. It never receives adapter credentials.
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
from .strategy import Strategy

PROTOCOL_VERSION = 1


class WorkerProtocolError(ValueError):
    """Raised when a frame violates the immutable worker contract."""


def _required_string(frame: dict[str, Any], name: str) -> str:
    value = frame.get(name)
    if not isinstance(value, str) or not value:
        raise WorkerProtocolError(f"{name} must be a non-empty string")
    return value


def _decimal(value: Any, name: str) -> Decimal:
    if not isinstance(value, str):
        raise WorkerProtocolError(f"{name} must be a decimal string")
    try:
        return Decimal(value)
    except Exception as error:  # Decimal exposes implementation-specific exceptions.
        raise WorkerProtocolError(f"{name} must be a valid decimal") from error


def _context(payload: dict[str, Any]) -> StrategyContext:
    return StrategyContext(
        account_id=_required_string(payload, "account_id"),
        strategy_id=_required_string(payload, "strategy_id"),
        strategy_version=_required_string(payload, "strategy_version"),
        configuration_version=_required_string(payload, "configuration_version"),
        replay_time=_required_string(payload, "replay_time"),
        environment=_required_string(payload, "environment"),
    )


def _bar(payload: dict[str, Any]) -> Bar:
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


def _read_frame(line: str) -> tuple[StrategyContext, Bar]:
    try:
        frame = json.loads(line)
    except json.JSONDecodeError as error:
        raise WorkerProtocolError("frame is not valid JSON") from error
    if not isinstance(frame, dict):
        raise WorkerProtocolError("frame must be a JSON object")
    if frame.get("protocol_version") != PROTOCOL_VERSION:
        raise WorkerProtocolError("unsupported worker protocol version")
    if frame.get("type") != "market_bar":
        raise WorkerProtocolError("worker accepts only market_bar frames")
    context = frame.get("context")
    bar = frame.get("bar")
    if not isinstance(context, dict) or not isinstance(bar, dict):
        raise WorkerProtocolError("market_bar requires object context and bar")
    return _context(context), _bar(bar)


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
            context, bar = _read_frame(line)
            if (
                context.strategy_id != bundle.strategy_id
                or context.strategy_version != bundle.strategy_version
            ):
                raise WorkerProtocolError("context does not match announced strategy bundle")
            intent = strategy.on_bar(context, bar)
            if intent is not None and not isinstance(intent, OrderIntent):
                raise WorkerProtocolError("strategy must return an OrderIntent or None")
            _write_frame(
                destination,
                {
                    "intent": intent.as_payload() if intent is not None else None,
                    "protocol_version": PROTOCOL_VERSION,
                    "type": "strategy_output",
                },
            )
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
