"""Follon PAPER-only bridge for Interactive Brokers' official Python TWS API.

The process accepts strict protocol-v1 JSON lines on stdin and writes only response JSON on
stdout. TWS/IB Gateway authentication remains out-of-process. No credential is accepted by this
program. The bridge permits loopback PAPER defaults only and maps Follon instrument IDs through a
reviewed configuration file.
"""

from __future__ import annotations

import argparse
import json
import queue
import re
import sys
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any, Protocol
from zoneinfo import ZoneInfo


PROTOCOL_VERSION = 1
CANONICAL_ID = re.compile(r"^[a-z0-9._-]+$")
UTC_TIMESTAMP = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
PAPER_PORTS = {7497, 4002}
LOOPBACK_HOSTS = {"127.0.0.1", "localhost", "::1"}


class BridgeFailure(Exception):
    """Evidence-safe protocol or broker failure."""


class Backend(Protocol):
    def submit(self, payload: dict[str, Any]) -> dict[str, Any]: ...

    def cancel(self, payload: dict[str, Any]) -> dict[str, Any]: ...

    def poll(self) -> list[dict[str, Any]]: ...

    def snapshot(self, payload: dict[str, Any]) -> dict[str, Any]: ...

    def reconnect(self) -> dict[str, Any]: ...

    def shutdown(self) -> None: ...


def _object(value: Any, expected: set[str], name: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != expected:
        raise BridgeFailure(f"{name} does not match the protocol")
    return value


def _canonical(value: Any, name: str) -> str:
    if not isinstance(value, str) or not CANONICAL_ID.fullmatch(value):
        raise BridgeFailure(f"{name} is not canonical")
    return value


def _decimal(value: Any, name: str, *, positive: bool = False) -> str:
    if not isinstance(value, str):
        raise BridgeFailure(f"{name} must be an exact decimal string")
    try:
        parsed = Decimal(value)
    except InvalidOperation as error:
        raise BridgeFailure(f"{name} must be an exact decimal string") from error
    if not parsed.is_finite() or (positive and parsed <= 0):
        raise BridgeFailure(f"{name} is outside its allowed range")
    return format(parsed, "f")


class BridgeProtocol:
    """Strict protocol dispatcher, independently testable without IBKR installed."""

    def __init__(self, backend: Backend):
        self.backend = backend

    def handle(self, candidate: Any) -> dict[str, Any]:
        request_id = 0
        try:
            request = _object(
                candidate,
                {"protocol_version", "request_id", "operation", "payload"},
                "bridge request",
            )
            if type(request["protocol_version"]) is not int or request["protocol_version"] != PROTOCOL_VERSION:
                raise BridgeFailure("unsupported bridge protocol version")
            if type(request["request_id"]) is not int or request["request_id"] < 1:
                raise BridgeFailure("bridge request_id is invalid")
            request_id = request["request_id"]
            operation = request["operation"]
            payload = request["payload"]
            if not isinstance(payload, dict):
                raise BridgeFailure("bridge payload must be an object")
            if operation == "submit":
                result = self.backend.submit(payload)
            elif operation == "cancel":
                result = self.backend.cancel(payload)
            elif operation == "poll":
                if payload:
                    raise BridgeFailure("poll payload must be empty")
                result = self.backend.poll()
            elif operation == "snapshot":
                result = self.backend.snapshot(payload)
            elif operation == "reconnect":
                if payload:
                    raise BridgeFailure("reconnect payload must be empty")
                result = self.backend.reconnect()
            elif operation == "shutdown":
                if payload:
                    raise BridgeFailure("shutdown payload must be empty")
                self.backend.shutdown()
                result = {}
            else:
                raise BridgeFailure("unsupported bridge operation")
            return {
                "protocol_version": PROTOCOL_VERSION,
                "request_id": request_id,
                "ok": True,
                "result": result,
                "error": None,
            }
        except BridgeFailure as error:
            return {
                "protocol_version": PROTOCOL_VERSION,
                "request_id": request_id,
                "ok": False,
                "result": None,
                "error": str(error),
            }
        except Exception:
            return {
                "protocol_version": PROTOCOL_VERSION,
                "request_id": request_id,
                "ok": False,
                "result": None,
                "error": "IBKR bridge operation failed",
            }


@dataclass(frozen=True)
class InstrumentContract:
    con_id: int
    symbol: str
    security_type: str
    exchange: str
    primary_exchange: str
    currency: str


def load_instruments(path: Path) -> dict[str, InstrumentContract]:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024 * 1024:
        raise BridgeFailure("instrument map is missing or too large")
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, dict) or not raw:
        raise BridgeFailure("instrument map must be a non-empty object")
    instruments: dict[str, InstrumentContract] = {}
    expected = {
        "con_id",
        "symbol",
        "security_type",
        "exchange",
        "primary_exchange",
        "currency",
    }
    for instrument_id, value in raw.items():
        _canonical(instrument_id, "instrument_id")
        contract = _object(value, expected, "instrument contract")
        if type(contract["con_id"]) is not int or contract["con_id"] <= 0:
            raise BridgeFailure("instrument con_id must be positive")
        text_values = [contract[key] for key in expected - {"con_id"}]
        if any(
            not isinstance(item, str)
            or not item
            or len(item) > 32
            or any(character in item for character in "\r\n")
            for item in text_values
        ):
            raise BridgeFailure("instrument contract text is invalid")
        instruments[instrument_id] = InstrumentContract(
            con_id=contract["con_id"],
            symbol=contract["symbol"],
            security_type=contract["security_type"],
            exchange=contract["exchange"],
            primary_exchange=contract["primary_exchange"],
            currency=contract["currency"],
        )
    return instruments


def create_official_backend(arguments: argparse.Namespace, instruments: dict[str, InstrumentContract]) -> Backend:
    try:
        from ibapi.client import EClient
        from ibapi.commission_report import CommissionReport
        from ibapi.contract import Contract
        from ibapi.execution import ExecutionFilter
        from ibapi.order import Order
        from ibapi.wrapper import EWrapper
    except ImportError as error:
        raise BridgeFailure(
            "official IBKR Python TWS API is not installed in the bridge environment"
        ) from error

    class Application(EWrapper, EClient):
        def __init__(self) -> None:
            EClient.__init__(self, self)
            self.condition = threading.Condition()
            self.connected_ready = False
            self.managed_accounts: set[str] = set()
            self.next_order_id: int | None = None
            self.client_by_order: dict[int, str] = {}
            self.order_by_client: dict[str, int] = {}
            self.orders: dict[str, dict[str, str]] = {}
            self.positions: dict[str, str] = {}
            self.cash: str | None = None
            self.events: queue.SimpleQueue[dict[str, Any]] = queue.SimpleQueue()
            self.execution_data: dict[str, tuple[Any, Any]] = {}
            self.commissions: dict[str, str] = {}
            self.emitted_executions: set[str] = set()
            self.open_orders_done = False
            self.positions_done = False
            self.account_summary_done = False
            self.executions_done = False
            self.completed_orders_done = False
            self.active_execution_request: int | None = None
            self.active_account_summary_request: int | None = None
            self.execution_ids_by_request: dict[int, set[str]] = {}

        def nextValidId(self, orderId: int) -> None:  # noqa: N802 - official callback
            with self.condition:
                self.next_order_id = orderId
                self.connected_ready = True
                self.condition.notify_all()

        def managedAccounts(self, accountsList: str) -> None:  # noqa: N802
            with self.condition:
                self.managed_accounts = {item for item in accountsList.split(",") if item}
                self.condition.notify_all()

        def connectionClosed(self) -> None:  # noqa: N802
            with self.condition:
                self.connected_ready = False
                self.condition.notify_all()

        def openOrder(self, orderId: int, contract: Any, order: Any, orderState: Any) -> None:  # noqa: N802
            if getattr(order, "account", "") != arguments.broker_account:
                return
            client_order_id = getattr(order, "orderRef", "")
            if not CANONICAL_ID.fullmatch(client_order_id or ""):
                client_order_id = f"unmapped-ibkr-order-{orderId}"
            with self.condition:
                self.client_by_order[orderId] = client_order_id
                self.order_by_client[client_order_id] = orderId
                state = normalize_order_state(getattr(orderState, "status", "Submitted"), "0", "0")
                self.orders[client_order_id] = {
                    "client_order_id": client_order_id,
                    "broker_order_id": f"ibkr-paper-order-{orderId}",
                    "state": state,
                    "filled_quantity": self.orders.get(client_order_id, {}).get(
                        "filled_quantity", "0"
                    ),
                }
                self.condition.notify_all()

        def orderStatus(  # noqa: N802
            self,
            orderId: int,
            status: str,
            filled: Any,
            remaining: Any,
            avgFillPrice: float,
            permId: int,
            parentId: int,
            lastFillPrice: float,
            clientId: int,
            whyHeld: str,
            mktCapPrice: float,
        ) -> None:
            del avgFillPrice, permId, parentId, lastFillPrice, clientId, whyHeld, mktCapPrice
            with self.condition:
                client_order_id = self.client_by_order.get(orderId)
                if client_order_id is None:
                    return
                normalized = normalize_order_state(status, str(filled), str(remaining))
                previous = self.orders.get(client_order_id, {}).get("state")
                self.orders[client_order_id] = {
                    "client_order_id": client_order_id,
                    "broker_order_id": f"ibkr-paper-order-{orderId}",
                    "state": normalized,
                    "filled_quantity": str(filled),
                }
                if normalized in {"ACKNOWLEDGED", "PARTIALLY_FILLED"} and previous not in {
                    "ACKNOWLEDGED",
                    "PARTIALLY_FILLED",
                    "FILLED",
                }:
                    self.events.put(
                        {
                            "event_type": "ACKNOWLEDGED",
                            "client_order_id": client_order_id,
                            "broker_order_id": f"ibkr-paper-order-{orderId}",
                        }
                    )
                elif normalized == "CANCELLED" and previous != "CANCELLED":
                    self.events.put(
                        {
                            "event_type": "CANCELLED",
                            "client_order_id": client_order_id,
                            "reason": "IBKR_PAPER_CANCELLED",
                        }
                    )
                elif normalized == "REJECTED" and previous != "REJECTED":
                    self.events.put(
                        {
                            "event_type": "REJECTED",
                            "client_order_id": client_order_id,
                            "reason": "IBKR_PAPER_REJECTED",
                        }
                    )
                self.condition.notify_all()

        def completedOrder(self, contract: Any, order: Any, orderState: Any) -> None:  # noqa: N802
            order_id = getattr(order, "orderId", 0) or getattr(order, "permId", 0)
            if type(order_id) is int and order_id > 0:
                self.openOrder(order_id, contract, order, orderState)

        def completedOrdersEnd(self) -> None:  # noqa: N802
            with self.condition:
                self.completed_orders_done = True
                self.condition.notify_all()

        def openOrderEnd(self) -> None:  # noqa: N802
            with self.condition:
                self.open_orders_done = True
                self.condition.notify_all()

        def position(self, account: str, contract: Any, position: Any, avgCost: float) -> None:
            del avgCost
            if account != arguments.broker_account:
                return
            instrument_id = instrument_by_con_id.get(
                contract.conId, f"unmapped-ibkr-conid-{contract.conId}"
            )
            with self.condition:
                self.positions[instrument_id] = str(position)

        def positionEnd(self) -> None:  # noqa: N802
            with self.condition:
                self.positions_done = True
                self.condition.notify_all()

        def accountSummary(  # noqa: N802
            self, reqId: int, account: str, tag: str, value: str, currency: str
        ) -> None:
            del currency
            if (
                reqId == self.active_account_summary_request
                and account == arguments.broker_account
                and tag == "TotalCashValue"
            ):
                with self.condition:
                    self.cash = value

        def accountSummaryEnd(self, reqId: int) -> None:  # noqa: N802
            with self.condition:
                if reqId == self.active_account_summary_request:
                    self.account_summary_done = True
                    self.condition.notify_all()

        def execDetails(self, reqId: int, contract: Any, execution: Any) -> None:  # noqa: N802
            with self.condition:
                self.execution_data[execution.execId] = (contract, execution)
                self.execution_ids_by_request.setdefault(reqId, set()).add(execution.execId)
                client_order_id = getattr(execution, "orderRef", "") or self.client_by_order.get(
                    execution.orderId, ""
                )
                if CANONICAL_ID.fullmatch(client_order_id or ""):
                    self.client_by_order[execution.orderId] = client_order_id
                    self.order_by_client[client_order_id] = execution.orderId
                    existing = self.orders.get(client_order_id, {})
                    cumulative = safe_nonnegative_decimal(
                        getattr(execution, "cumQty", execution.shares)
                    )
                    if cumulative is not None:
                        state = existing.get("state", "UNKNOWN")
                        if state not in {"FILLED", "CANCELLED", "REJECTED"} and Decimal(
                            cumulative
                        ) > 0:
                            state = "PARTIALLY_FILLED"
                        self.orders[client_order_id] = {
                            "client_order_id": client_order_id,
                            "broker_order_id": f"ibkr-paper-order-{execution.orderId}",
                            "state": state,
                            "filled_quantity": cumulative,
                        }
                self._emit_execution_if_complete(execution.execId)
                self.condition.notify_all()

        def execDetailsEnd(self, reqId: int) -> None:  # noqa: N802
            with self.condition:
                if reqId == self.active_execution_request:
                    self.executions_done = True
                    self.condition.notify_all()

        def commissionReport(self, commissionReport: CommissionReport) -> None:  # noqa: N802
            with self.condition:
                self.commissions[commissionReport.execId] = str(commissionReport.commission)
                self._emit_execution_if_complete(commissionReport.execId)

        def _emit_execution_if_complete(self, execution_id: str) -> None:
            if (
                execution_id in self.emitted_executions
                or execution_id not in self.execution_data
                or execution_id not in self.commissions
            ):
                return
            contract, execution = self.execution_data[execution_id]
            client_order_id = getattr(execution, "orderRef", "") or self.client_by_order.get(
                execution.orderId, ""
            )
            instrument_id = instrument_by_con_id.get(contract.conId)
            if not CANONICAL_ID.fullmatch(client_order_id or "") or instrument_id is None:
                return
            fee = safe_decimal(self.commissions[execution_id])
            quantity = safe_positive_decimal(execution.shares)
            price = safe_positive_decimal(execution.price)
            if fee is None or quantity is None or price is None:
                return
            try:
                executed_at = normalize_execution_time(
                    execution.time, arguments.tws_timezone
                )
            except (BridgeFailure, ValueError):
                return
            self.emitted_executions.add(execution_id)
            self.events.put(
                {
                    "event_type": "EXECUTION",
                    "execution_id": canonical_execution_id(execution_id),
                    "client_order_id": client_order_id,
                    "broker_order_id": f"ibkr-paper-order-{execution.orderId}",
                    "quantity": quantity,
                    "price": price,
                    "fee": fee,
                    "executed_at": executed_at,
                }
            )

        def error(self, reqId: int, *details: Any) -> None:  # type: ignore[override]
            # API 10.33 added errorTime before errorCode; accept both official signatures.
            if len(details) >= 3 and type(details[0]) is int and type(details[1]) is int:
                errorCode = details[1]
            elif details and type(details[0]) is int:
                errorCode = details[0]
            else:
                errorCode = -1
            if errorCode in {1100, 2110}:
                with self.condition:
                    self.connected_ready = False
                    self.condition.notify_all()
                return
            if errorCode in {1101, 1102}:
                with self.condition:
                    self.connected_ready = True
                    self.condition.notify_all()
                return
            if errorCode in {2104, 2106, 2107, 2108, 2158}:
                return
            with self.condition:
                client_order_id = self.client_by_order.get(reqId)
                if client_order_id is not None:
                    self.orders[client_order_id] = {
                        "client_order_id": client_order_id,
                        "broker_order_id": f"ibkr-paper-order-{reqId}",
                        "state": "REJECTED",
                        "filled_quantity": self.orders.get(client_order_id, {}).get(
                            "filled_quantity", "0"
                        ),
                    }
                    self.events.put(
                        {
                            "event_type": "REJECTED",
                            "client_order_id": client_order_id,
                            "reason": f"IBKR_ERROR_{errorCode}",
                        }
                    )
                self.condition.notify_all()

    instrument_by_con_id = {contract.con_id: key for key, contract in instruments.items()}
    application = Application()

    class OfficialBackend:
        def __init__(self) -> None:
            self.app = application
            self.api_thread: threading.Thread | None = None
            self.next_request_id = 9101
            self._connect()

        def _connect(self) -> None:
            if self.app.isConnected():
                self.app.disconnect()
            if self.api_thread is not None and self.api_thread.is_alive():
                self.api_thread.join(timeout=arguments.timeout_seconds)
                if self.api_thread.is_alive():
                    raise BridgeFailure("prior IBKR API reader did not stop")
            with self.app.condition:
                self.app.connected_ready = False
                self.app.managed_accounts.clear()
            try:
                self.app.connect(arguments.host, arguments.port, clientId=arguments.client_id)
            except Exception as error:
                raise BridgeFailure("IBKR PAPER gateway connection failed") from error
            self.api_thread = threading.Thread(target=self.app.run, daemon=True)
            self.api_thread.start()
            self._wait(
                lambda: self.app.connected_ready
                and arguments.broker_account in self.app.managed_accounts,
                "IBKR PAPER gateway did not confirm the configured account",
            )

        def _require_connected(self) -> None:
            if not self.app.isConnected() or not self.app.connected_ready:
                raise BridgeFailure("IBKR PAPER gateway is disconnected")

        def _wait(self, predicate: Any, message: str) -> None:
            deadline = time.monotonic() + arguments.timeout_seconds
            with self.app.condition:
                while not predicate():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise BridgeFailure(message)
                    self.app.condition.wait(timeout=remaining)

        def _allocate_request_id(self) -> int:
            request_id = self.next_request_id
            self.next_request_id += 1
            return request_id

        def _refresh_executions(self) -> None:
            request_id = self._allocate_request_id()
            with self.app.condition:
                self.app.executions_done = False
                self.app.active_execution_request = request_id
                self.app.execution_ids_by_request[request_id] = set()
            self.app.reqExecutions(request_id, ExecutionFilter())
            self._wait(
                lambda: self.app.executions_done
                and all(
                    execution_id in self.app.commissions
                    for execution_id in self.app.execution_ids_by_request.get(request_id, set())
                ),
                "IBKR execution refresh timed out",
            )
            with self.app.condition:
                self.app.active_execution_request = None

        def submit(self, payload: dict[str, Any]) -> dict[str, Any]:
            self._require_connected()
            expected = {
                "client_order_id",
                "account_id",
                "instrument_id",
                "side",
                "quantity",
                "limit_price",
            }
            request = _object(payload, expected, "submit payload")
            client_order_id = _canonical(request["client_order_id"], "client_order_id")
            if _canonical(request["account_id"], "account_id") != arguments.account_id:
                raise BridgeFailure("submit account does not match bridge configuration")
            instrument_id = _canonical(request["instrument_id"], "instrument_id")
            contract_config = instruments.get(instrument_id)
            if contract_config is None:
                raise BridgeFailure("instrument is absent from the reviewed IBKR map")
            if request["side"] not in {"BUY", "SELL"}:
                raise BridgeFailure("submit side is invalid")
            quantity = _decimal(request["quantity"], "quantity", positive=True)
            limit_price = request["limit_price"]
            if limit_price is not None:
                limit_price = _decimal(limit_price, "limit_price", positive=True)
            with self.app.condition:
                existing = self.app.order_by_client.get(client_order_id)
                if existing is not None:
                    return {
                        "status": "ACKNOWLEDGED",
                        "broker_order_id": f"ibkr-paper-order-{existing}",
                        "reason": None,
                    }
                if self.app.next_order_id is None:
                    raise BridgeFailure("IBKR next order ID is unavailable")
                order_id = self.app.next_order_id
                self.app.next_order_id += 1
                self.app.client_by_order[order_id] = client_order_id
                self.app.order_by_client[client_order_id] = order_id
            contract = Contract()
            contract.conId = contract_config.con_id
            contract.symbol = contract_config.symbol
            contract.secType = contract_config.security_type
            contract.exchange = contract_config.exchange
            contract.primaryExchange = contract_config.primary_exchange
            contract.currency = contract_config.currency
            order = Order()
            order.account = arguments.broker_account
            order.action = request["side"]
            order.totalQuantity = Decimal(quantity)
            order.orderType = "MKT" if limit_price is None else "LMT"
            if limit_price is not None:
                order.lmtPrice = float(Decimal(limit_price))
            order.tif = "DAY"
            order.orderRef = client_order_id
            order.transmit = True
            self.app.placeOrder(order_id, contract, order)
            try:
                self._wait(
                    lambda: self.app.orders.get(client_order_id, {}).get("state")
                    in {
                        "ACKNOWLEDGED",
                        "PARTIALLY_FILLED",
                        "FILLED",
                        "REJECTED",
                        "CANCELLED",
                    },
                    "IBKR submit acknowledgement timed out",
                )
            except BridgeFailure:
                return {
                    "status": "UNKNOWN",
                    "broker_order_id": None,
                    "reason": "IBKR_SUBMIT_OUTCOME_UNKNOWN",
                }
            state = self.app.orders[client_order_id]["state"]
            if state == "REJECTED":
                return {
                    "status": "REJECTED",
                    "broker_order_id": None,
                    "reason": "IBKR_PAPER_REJECTED",
                }
            return {
                "status": "ACKNOWLEDGED",
                "broker_order_id": f"ibkr-paper-order-{order_id}",
                "reason": None,
            }

        def cancel(self, payload: dict[str, Any]) -> dict[str, Any]:
            self._require_connected()
            request = _object(payload, {"client_order_id"}, "cancel payload")
            client_order_id = _canonical(request["client_order_id"], "client_order_id")
            order_id = self.app.order_by_client.get(client_order_id)
            if order_id is None:
                raise BridgeFailure("IBKR does not know the client order ID")
            self.app.cancelOrder(order_id, "")
            return {}

        def poll(self) -> list[dict[str, Any]]:
            self._require_connected()
            events: list[dict[str, Any]] = []
            while True:
                try:
                    events.append(self.app.events.get_nowait())
                except queue.Empty:
                    return events

        def snapshot(self, payload: dict[str, Any]) -> dict[str, Any]:
            self._require_connected()
            request = _object(payload, {"account_id"}, "snapshot payload")
            if _canonical(request["account_id"], "account_id") != arguments.account_id:
                raise BridgeFailure("snapshot account does not match bridge configuration")
            with self.app.condition:
                self.app.open_orders_done = False
                self.app.positions_done = False
                self.app.account_summary_done = False
                self.app.completed_orders_done = False
                self.app.positions = {}
                self.app.cash = None
                self.app.orders = {}
            self.app.reqOpenOrders()
            self.app.reqCompletedOrders(True)
            self.app.reqPositions()
            account_request_id = self._allocate_request_id()
            with self.app.condition:
                self.app.active_account_summary_request = account_request_id
            self.app.reqAccountSummary(account_request_id, "All", "TotalCashValue")
            self._refresh_executions()
            self._wait(
                lambda: self.app.open_orders_done
                and self.app.positions_done
                and self.app.account_summary_done
                and self.app.completed_orders_done,
                "IBKR account snapshot timed out",
            )
            self.app.cancelPositions()
            self.app.cancelAccountSummary(account_request_id)
            with self.app.condition:
                self.app.active_account_summary_request = None
            if self.app.cash is None:
                raise BridgeFailure("IBKR account snapshot has no cash value")
            return {
                "orders": sorted(
                    self.app.orders.values(), key=lambda item: item["client_order_id"]
                ),
                "positions": [
                    {"instrument_id": key, "quantity": value}
                    for key, value in sorted(self.app.positions.items())
                ],
                "cash": self.app.cash,
            }

        def reconnect(self) -> dict[str, Any]:
            self._connect()
            self._refresh_executions()
            return {}

        def shutdown(self) -> None:
            if self.app.isConnected():
                self.app.disconnect()

    return OfficialBackend()


def normalize_order_state(status: str, filled: str, remaining: str) -> str:
    try:
        filled_value = Decimal(filled)
        remaining_value = Decimal(remaining)
    except InvalidOperation:
        filled_value = Decimal(0)
        remaining_value = Decimal(0)
    if status == "Filled" or (filled_value > 0 and remaining_value == 0):
        return "FILLED"
    if filled_value > 0:
        return "PARTIALLY_FILLED"
    return {
        "PendingSubmit": "PENDING_SUBMIT",
        "PreSubmitted": "ACKNOWLEDGED",
        "Submitted": "ACKNOWLEDGED",
        "PendingCancel": "PENDING_CANCEL",
        "ApiCancelled": "CANCELLED",
        "Cancelled": "CANCELLED",
        "Inactive": "REJECTED",
    }.get(status, "UNKNOWN")


def safe_decimal(value: Any) -> str | None:
    try:
        parsed = Decimal(str(value))
    except InvalidOperation:
        return None
    if not parsed.is_finite() or abs(parsed) > Decimal("1000000000"):
        return None
    return format(parsed, "f")


def safe_nonnegative_decimal(value: Any) -> str | None:
    normalized = safe_decimal(value)
    if normalized is None or Decimal(normalized) < 0:
        return None
    return normalized


def safe_positive_decimal(value: Any) -> str | None:
    normalized = safe_decimal(value)
    if normalized is None or Decimal(normalized) <= 0:
        return None
    return normalized


def canonical_execution_id(execution_id: str) -> str:
    digest = execution_id.lower().replace(".", "-").replace(" ", "-")
    digest = re.sub(r"[^a-z0-9._-]", "-", digest)
    return f"ibkr-paper-execution-{digest}"


def normalize_execution_time(value: str, fallback_timezone: str) -> str:
    normalized_value = value.strip().replace("-", " ", 1)
    parts = normalized_value.split()
    if len(parts) < 2:
        raise BridgeFailure("IBKR execution time is invalid")
    local = datetime.strptime(f"{parts[0]} {parts[1]}", "%Y%m%d %H:%M:%S")
    zone_name = parts[2] if len(parts) >= 3 else fallback_timezone
    instant = local.replace(tzinfo=ZoneInfo(zone_name)).astimezone(timezone.utc)
    normalized = instant.strftime("%Y-%m-%dT%H:%M:%SZ")
    if not UTC_TIMESTAMP.fullmatch(normalized):
        raise BridgeFailure("IBKR execution time is invalid")
    return normalized


def parse_arguments(values: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", required=True, type=int)
    parser.add_argument("--client-id", required=True, type=int)
    parser.add_argument("--account-id", required=True)
    parser.add_argument("--broker-account", required=True)
    parser.add_argument("--instrument-map", required=True, type=Path)
    parser.add_argument("--tws-timezone", required=True)
    parser.add_argument("--environment", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=10.0)
    arguments = parser.parse_args(values)
    if (
        arguments.host not in LOOPBACK_HOSTS
        or arguments.port not in PAPER_PORTS
        or arguments.environment != "PAPER"
        or not CANONICAL_ID.fullmatch(arguments.account_id)
        or not 0 <= arguments.client_id <= 31
        or not 0.1 <= arguments.timeout_seconds <= 60
        or not arguments.broker_account
        or len(arguments.broker_account) > 64
        or any(character in arguments.broker_account for character in "\r\n")
    ):
        raise BridgeFailure("invalid PAPER-only IBKR bridge configuration")
    ZoneInfo(arguments.tws_timezone)
    return arguments


def serve(protocol: BridgeProtocol) -> int:
    while True:
        raw_line = sys.stdin.buffer.readline(1024 * 1024 + 1)
        if not raw_line:
            break
        if len(raw_line) > 1024 * 1024 or not raw_line.endswith(b"\n"):
            while raw_line and not raw_line.endswith(b"\n"):
                raw_line = sys.stdin.buffer.readline(64 * 1024)
            response = {
                "protocol_version": PROTOCOL_VERSION,
                "request_id": 0,
                "ok": False,
                "result": None,
                "error": "bridge request exceeds the size limit",
            }
        else:
            try:
                candidate = json.loads(raw_line.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                candidate = None
            response = protocol.handle(candidate)
        sys.stdout.write(json.dumps(response, separators=(",", ":"), sort_keys=True) + "\n")
        sys.stdout.flush()
    protocol.backend.shutdown()
    return 0


def main(values: list[str] | None = None) -> int:
    try:
        arguments = parse_arguments(sys.argv[1:] if values is None else values)
        instruments = load_instruments(arguments.instrument_map)
        backend = create_official_backend(arguments, instruments)
        return serve(BridgeProtocol(backend))
    except (BridgeFailure, OSError, ValueError, json.JSONDecodeError):
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
