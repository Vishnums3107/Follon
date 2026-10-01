from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock
from zoneinfo import ZoneInfoNotFoundError

from follon_ibkr_gateway import (
    BridgeFailure,
    BridgeProtocol,
    BridgeRefusal,
    InstrumentContract,
    SubmitRequest,
    create_official_backend,
    load_instruments,
    normalize_execution_time,
    normalize_order_state,
    parse_arguments,
    validate_submit,
)


class FakeBackend:
    def __init__(self) -> None:
        self.shutdown_called = False

    def submit(self, payload):
        return {"status": "ACKNOWLEDGED", "broker_order_id": "ibkr.41", "reason": None}

    def cancel(self, payload):
        return {}

    def poll(self):
        return []

    def snapshot(self, payload):
        return {"orders": [], "positions": [], "cash": "1000.00"}

    def reconnect(self):
        return {}

    def shutdown(self):
        self.shutdown_called = True


def request(operation: str, payload: dict, **updates):
    value = {
        "protocol_version": 1,
        "request_id": 7,
        "operation": operation,
        "payload": payload,
    }
    value.update(updates)
    return value


class ProtocolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.backend = FakeBackend()
        self.protocol = BridgeProtocol(self.backend)

    def test_submit_is_dispatched_with_correlated_response(self) -> None:
        response = self.protocol.handle(request("submit", {"client_order_id": "order.1"}))
        self.assertEqual(response["request_id"], 7)
        self.assertTrue(response["ok"])
        self.assertEqual(response["result"]["broker_order_id"], "ibkr.41")
        self.assertIsNone(response["error"])

    def test_unknown_fields_and_protocol_versions_are_rejected(self) -> None:
        candidate = request("poll", {})
        candidate["unexpected"] = True
        self.assertFalse(self.protocol.handle(candidate)["ok"])
        self.assertFalse(
            self.protocol.handle(request("poll", {}, protocol_version=2))["ok"]
        )
        self.assertFalse(
            self.protocol.handle(request("poll", {}, protocol_version=True))["ok"]
        )

    def test_empty_payload_is_required_for_poll_and_reconnect(self) -> None:
        self.assertFalse(self.protocol.handle(request("poll", {"unsafe": True}))["ok"])
        self.assertFalse(
            self.protocol.handle(request("reconnect", {"unsafe": True}))["ok"]
        )

    def test_backend_exceptions_are_sanitized(self) -> None:
        self.backend.poll = lambda: (_ for _ in ()).throw(RuntimeError("secret detail"))
        response = self.protocol.handle(request("poll", {}))
        self.assertFalse(response["ok"])
        self.assertEqual(response["error"], "IBKR bridge operation failed")
        self.assertNotIn("secret", json.dumps(response))


class ConfigurationTests(unittest.TestCase):
    def test_parser_refuses_live_public_and_live_port_configurations(self) -> None:
        base = [
            "--host", "127.0.0.1",
            "--port", "7497",
            "--client-id", "7",
            "--account-id", "acct.paper.1",
            "--broker-account", "DU1234567",
            "--instrument-map", "instruments.json",
            "--tws-timezone", "America/New_York",
            "--environment", "PAPER",
        ]
        self.assertEqual(parse_arguments(base).port, 7497)

        for name, value in (("--environment", "LIVE"), ("--host", "example.com"), ("--port", "7496")):
            modified = list(base)
            index = modified.index(name) + 1
            modified[index] = value
            with self.subTest(name=name, value=value), self.assertRaises(BridgeFailure):
                parse_arguments(modified)

    def test_instrument_map_is_strict_and_typed(self) -> None:
        instrument = {
            "aapl.xnas": {
                "con_id": 265598,
                "symbol": "AAPL",
                "security_type": "STK",
                "exchange": "SMART",
                "primary_exchange": "NASDAQ",
                "currency": "USD",
            }
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "instruments.json"
            path.write_text(json.dumps(instrument), encoding="utf-8")
            loaded = load_instruments(path)
            self.assertEqual(loaded["aapl.xnas"].con_id, 265598)

            instrument["aapl.xnas"]["unexpected"] = True
            path.write_text(json.dumps(instrument), encoding="utf-8")
            with self.assertRaises(BridgeFailure):
                load_instruments(path)

    def test_vendor_values_are_normalized_without_losing_partial_fill_state(self) -> None:
        self.assertEqual(normalize_order_state("Submitted", "1", "2"), "PARTIALLY_FILLED")
        self.assertEqual(normalize_order_state("Filled", "3", "0"), "FILLED")
        self.assertEqual(
            normalize_execution_time("20260102 09:31:00", "America/New_York"),
            "2026-01-02T14:31:00Z",
        )
        self.assertEqual(
            normalize_execution_time("20260309-09:31:00 America/New_York", "UTC"),
            "2026-03-09T13:31:00Z",
        )


class _FakeContract:
    def __init__(self, con_id: int) -> None:
        self.conId = con_id


class _FakeExecution:
    def __init__(
        self,
        *,
        exec_id: str,
        order_ref: str,
        order_id: int,
        shares: str,
        price: str,
        time: str,
        client_id: int = 7,
        account: str = "DU1234567",
    ) -> None:
        self.execId = exec_id
        self.orderRef = order_ref
        self.orderId = order_id
        self.shares = shares
        self.price = price
        self.time = time
        self.cumQty = shares
        self.clientId = client_id
        self.acctNumber = account


class _FakeCommissionReport:
    def __init__(self, exec_id: str, commission: str) -> None:
        self.execId = exec_id
        self.commission = commission


_INSTRUMENT = {
    "aapl.xnas": {
        "con_id": 265598,
        "symbol": "AAPL",
        "security_type": "STK",
        "exchange": "SMART",
        "primary_exchange": "NASDAQ",
        "currency": "USD",
    }
}

_BASE_ARGUMENTS = [
    "--host", "127.0.0.1",
    "--port", "7497",
    "--client-id", "7",
    "--account-id", "acct.paper.1",
    "--broker-account", "DU1234567",
    "--instrument-map", "instruments.json",
    "--tws-timezone", "America/New_York",
    "--environment", "PAPER",
]


def _build_official_backend():
    """Builds a real OfficialBackend wired to a fake, in-process IBKR connection.

    ``ibapi.client.EClient.connect``/``run``/``isConnected``/``disconnect`` are patched so the
    handshake completes synchronously without a real TWS/Gateway socket, letting the tests
    exercise the actual production ``submit``/``execDetails``/``commissionReport`` code paths.
    """
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "instruments.json"
        path.write_text(json.dumps(_INSTRUMENT), encoding="utf-8")
        instruments = load_instruments(path)
    arguments = parse_arguments(_BASE_ARGUMENTS)

    def fake_connect(self, host, port, clientId) -> None:  # noqa: N803 - matches ibapi signature
        del host, port, clientId
        self.nextValidId(1)
        self.managedAccounts(arguments.broker_account)

    with (
        mock.patch("ibapi.client.EClient.connect", fake_connect),
        mock.patch("ibapi.client.EClient.run", lambda self: None),
        mock.patch("ibapi.client.EClient.isConnected", lambda self: True),
        mock.patch("ibapi.client.EClient.disconnect", lambda self: None),
    ):
        backend = create_official_backend(arguments, instruments)
    # The mock.patch context above only needs to cover the handshake; the tests that follow
    # call submit()/execDetails() outside of it, so pin the "connected" signal on the instance
    # itself (an instance attribute shadows the class method and is not auto-bound as one, so
    # a zero-arg lambda is called correctly as `self.app.isConnected()`).
    backend.app.isConnected = lambda: True
    return backend


def _submit_payload(client_order_id: str) -> dict:
    return {
        "client_order_id": client_order_id,
        "account_id": "acct.paper.1",
        "instrument_id": "aapl.xnas",
        "side": "BUY",
        "quantity": "10",
        "limit_price": None,
    }


class OfficialBackendSnapshotTests(unittest.TestCase):
    def test_snapshot_sees_other_clients_orders_without_owning_them(self) -> None:
        backend = _build_official_backend()
        requested: list[str] = []

        def all_open_orders() -> None:
            requested.append("all_open_orders")
            for order_id, account, client_id, order_ref, perm_id in (
                (301, "DU1234567", 8, "order.ours", 901),
                (302, "DU0000000", 8, "order.other.account", 902),
                (303, "DU1234567", 7, "order.ours", 903),
            ):
                backend.app.openOrder(
                    order_id,
                    _FakeContract(265598),
                    SimpleNamespace(
                        account=account,
                        clientId=client_id,
                        orderRef=order_ref,
                        permId=perm_id,
                    ),
                    SimpleNamespace(status="Submitted"),
                )
            backend.app.openOrderEnd()

        def account_summary(request_id: int, group: str, tags: str) -> None:
            self.assertEqual((group, tags), ("All", "TotalCashValue"))
            backend.app.accountSummary(request_id, "DU1234567", "TotalCashValue", "1000", "USD")
            backend.app.accountSummaryEnd(request_id)

        backend.app.reqOpenOrders = lambda: self.fail("snapshot requested only this client's orders")
        backend.app.reqAllOpenOrders = all_open_orders
        backend.app.reqCompletedOrders = lambda api_only: backend.app.completedOrdersEnd()
        backend.app.reqPositions = lambda: backend.app.positionEnd()
        backend.app.reqAccountSummary = account_summary
        backend.app.cancelPositions = lambda: None
        backend.app.cancelAccountSummary = lambda request_id: None
        backend._refresh_executions = lambda: None

        snapshot = backend.snapshot({"account_id": "acct.paper.1"})

        self.assertEqual(requested, ["all_open_orders"])
        self.assertEqual(
            [order["client_order_id"] for order in snapshot["orders"]],
            ["order.ours", "unmapped-ibkr-order-901"],
        )
        self.assertNotIn(301, backend.app.client_by_order)
        self.assertEqual(set(backend.app.order_by_client), {"order.ours"})
        self.assertEqual(backend.app.order_by_client["order.ours"], 303)
        self.assertEqual(backend.app.next_order_id, 304)
        backend.app.orderStatus(301, "Filled", 1, 0, 0.0, 901, 0, 0.0, 8, "", 0.0)
        self.assertTrue(backend.app.events.empty())

    def test_foreign_executions_never_enter_the_oms_event_queue(self) -> None:
        for client_id, account in ((8, "DU1234567"), (7, "DU0000000")):
            with self.subTest(client_id=client_id, account=account):
                backend = _build_official_backend()
                execution = _FakeExecution(
                    exec_id="exec.foreign",
                    order_ref="order.ours",
                    order_id=301,
                    shares="1",
                    price="100",
                    time="20260102 09:31:00 America/New_York",
                    client_id=client_id,
                    account=account,
                )
                backend.app.execDetails(9101, _FakeContract(265598), execution)
                backend.app.commissionReport(_FakeCommissionReport("exec.foreign", "1"))
                self.assertNotIn("exec.foreign", backend.app.execution_data)
                self.assertTrue(backend.app.events.empty())


class OfficialBackendSubmitRetryTests(unittest.TestCase):
    """Regression coverage for the submit() idempotency-retry lookup (Bug 1)."""

    def _seed_existing_order(self, backend, client_order_id: str, order_id: int, state: str) -> None:
        with backend.app.condition:
            backend.app.order_by_client[client_order_id] = order_id
            backend.app.orders[client_order_id] = {
                "client_order_id": client_order_id,
                "broker_order_id": f"ibkr-paper-order-{order_id}",
                "state": state,
                "filled_quantity": "0",
            }

    def test_retry_against_a_rejected_order_reports_rejected_not_acknowledged(self) -> None:
        backend = _build_official_backend()
        self._seed_existing_order(backend, "order.retry.rejected", 555, "REJECTED")
        response = backend.submit(_submit_payload("order.retry.rejected"))
        self.assertNotEqual(response["status"], "ACKNOWLEDGED")
        self.assertEqual(
            response,
            {"status": "REJECTED", "broker_order_id": None, "reason": "IBKR_PAPER_REJECTED"},
        )

    def test_retry_against_a_cancelled_order_reports_rejected_not_acknowledged(self) -> None:
        backend = _build_official_backend()
        self._seed_existing_order(backend, "order.retry.cancelled", 556, "CANCELLED")
        response = backend.submit(_submit_payload("order.retry.cancelled"))
        self.assertNotEqual(response["status"], "ACKNOWLEDGED")
        self.assertEqual(
            response,
            {"status": "REJECTED", "broker_order_id": None, "reason": "IBKR_PAPER_CANCELLED"},
        )

    def test_retry_against_a_still_pending_order_reports_unknown_not_acknowledged(self) -> None:
        backend = _build_official_backend()
        self._seed_existing_order(backend, "order.retry.pending", 557, "PENDING_SUBMIT")
        response = backend.submit(_submit_payload("order.retry.pending"))
        self.assertNotEqual(response["status"], "ACKNOWLEDGED")
        self.assertEqual(
            response,
            {
                "status": "UNKNOWN",
                "broker_order_id": None,
                "reason": "IBKR_SUBMIT_OUTCOME_UNKNOWN",
            },
        )

    def test_retry_against_a_genuinely_acknowledged_order_still_reports_acknowledged(self) -> None:
        backend = _build_official_backend()
        self._seed_existing_order(backend, "order.retry.live", 558, "ACKNOWLEDGED")
        response = backend.submit(_submit_payload("order.retry.live"))
        self.assertEqual(
            response,
            {
                "status": "ACKNOWLEDGED",
                "broker_order_id": "ibkr-paper-order-558",
                "reason": None,
            },
        )


class OfficialBackendExecutionTimeTests(unittest.TestCase):
    """Regression coverage for malformed execution timezones (Bug 2)."""

    def test_normalize_execution_time_raises_zoneinfo_not_found_for_unknown_zone(self) -> None:
        # ZoneInfoNotFoundError is a KeyError subclass, not a ValueError -- this is exactly what
        # a bare `except (BridgeFailure, ValueError)` around normalize_execution_time would miss.
        self.assertFalse(issubclass(ZoneInfoNotFoundError, ValueError))
        with self.assertRaises(ZoneInfoNotFoundError):
            normalize_execution_time("20260102 09:31:00 Not/AZone", "UTC")

    def test_execution_with_unrecognized_timezone_is_skipped_not_raised(self) -> None:
        backend = _build_official_backend()
        contract = _FakeContract(265598)
        execution = _FakeExecution(
            exec_id="exec.unknown.tz",
            order_ref="order.exec.unknown.tz",
            order_id=777,
            shares="5",
            price="101.50",
            time="20260102 09:31:00 Not/AZone",
        )
        # Must degrade gracefully instead of raising out of the IBKR reader-thread callback.
        backend.app.execDetails(9101, contract, execution)
        backend.app.commissionReport(_FakeCommissionReport("exec.unknown.tz", "1.00"))
        self.assertTrue(backend.app.events.empty())
        self.assertNotIn("exec.unknown.tz", backend.app.emitted_executions)

    def test_execution_with_a_recognized_timezone_is_still_emitted(self) -> None:
        backend = _build_official_backend()
        contract = _FakeContract(265598)
        execution = _FakeExecution(
            exec_id="exec.known.tz",
            order_ref="order.exec.known.tz",
            order_id=778,
            shares="5",
            price="101.50",
            time="20260102 09:31:00 America/New_York",
        )
        backend.app.execDetails(9101, contract, execution)
        backend.app.commissionReport(_FakeCommissionReport("exec.known.tz", "1.00"))
        self.assertIn("exec.known.tz", backend.app.emitted_executions)
        event = backend.app.events.get_nowait()
        self.assertEqual(event["event_type"], "EXECUTION")
        self.assertEqual(event["executed_at"], "2026-01-02T14:31:00Z")


def _raise(error: Exception):
    raise error


class RefusalProtocolTests(unittest.TestCase):
    """A request refused before IBKR is contacted is a rejection, not a failure (E5.4)."""

    def setUp(self) -> None:
        self.backend = FakeBackend()
        self.protocol = BridgeProtocol(self.backend)

    def test_a_refused_submit_is_a_clean_rejection_not_a_failure(self) -> None:
        self.backend.submit = lambda payload: _raise(
            BridgeRefusal("INSTRUMENT_UNMAPPED", "private detail")
        )
        response = self.protocol.handle(request("submit", {}))
        self.assertTrue(response["ok"])
        self.assertIsNone(response["error"])
        self.assertEqual(
            response["result"],
            {
                "status": "REJECTED",
                "broker_order_id": None,
                "reason": "IBKR_BRIDGE_REFUSED_INSTRUMENT_UNMAPPED",
            },
        )
        self.assertNotIn("private", json.dumps(response))

    def test_a_submit_failure_that_is_not_a_refusal_stays_a_failure(self) -> None:
        # Whether the order reached IBKR is unknown, so the rejection above
        # must not be extended to it.
        self.backend.submit = lambda payload: _raise(BridgeFailure("ambiguous"))
        response = self.protocol.handle(request("submit", {}))
        self.assertFalse(response["ok"])
        self.assertIsNone(response["result"])

    def test_a_refusal_raised_by_another_operation_stays_a_failure(self) -> None:
        self.backend.poll = lambda: _raise(BridgeRefusal("GATEWAY_DISCONNECTED", "down"))
        response = self.protocol.handle(request("poll", {}))
        self.assertFalse(response["ok"])
        self.assertEqual(response["error"], "down")


_CONTRACT = InstrumentContract(
    con_id=265598,
    symbol="AAPL",
    security_type="STK",
    exchange="SMART",
    primary_exchange="NASDAQ",
    currency="USD",
)


class SubmitValidationTests(unittest.TestCase):
    """Every check the bridge makes before IBKR is contacted, without `ibapi`."""

    def refusal_code(self, **changes) -> str:
        payload = _submit_payload("order.validate.1") | changes
        with self.assertRaises(BridgeRefusal) as caught:
            validate_submit(payload, "acct.paper.1", {"aapl.xnas": _CONTRACT})
        return caught.exception.code

    def test_a_valid_payload_is_normalized(self) -> None:
        request_ = validate_submit(
            _submit_payload("order.validate.1") | {"quantity": "10.50", "limit_price": "101.25"},
            "acct.paper.1",
            {"aapl.xnas": _CONTRACT},
        )
        self.assertEqual(
            request_,
            SubmitRequest(
                client_order_id="order.validate.1",
                contract=_CONTRACT,
                side="BUY",
                quantity="10.50",
                limit_price="101.25",
            ),
        )

    def test_each_refusal_carries_its_own_code(self) -> None:
        self.assertEqual(self.refusal_code(instrument_id="msft.xnas"), "INSTRUMENT_UNMAPPED")
        self.assertEqual(self.refusal_code(account_id="acct.other"), "ACCOUNT_MISMATCH")
        self.assertEqual(self.refusal_code(side="HOLD"), "INVALID_SIDE")
        self.assertEqual(self.refusal_code(side=["BUY"]), "INVALID_SIDE")

    def test_malformed_values_are_an_invalid_request(self) -> None:
        for changes in (
            {"quantity": "0"},
            {"quantity": "-1"},
            {"quantity": "many"},
            {"quantity": 5},
            {"limit_price": "0"},
            {"limit_price": "cheap"},
            {"client_order_id": "Not Canonical"},
            {"instrument_id": "BAD ID"},
        ):
            with self.subTest(changes=changes):
                self.assertEqual(self.refusal_code(**changes), "INVALID_REQUEST")

    def test_a_payload_that_does_not_match_the_protocol_is_an_invalid_request(self) -> None:
        extra = _submit_payload("order.validate.1") | {"unexpected": True}
        missing = {
            key: value
            for key, value in _submit_payload("order.validate.1").items()
            if key != "limit_price"
        }
        for payload in (extra, missing, "not an object"):
            with self.subTest(payload=payload), self.assertRaises(BridgeRefusal) as caught:
                validate_submit(payload, "acct.paper.1", {"aapl.xnas": _CONTRACT})
            self.assertEqual(caught.exception.code, "INVALID_REQUEST")


class OfficialBackendRefusalTests(unittest.TestCase):
    """The real `OfficialBackend` refuses before anything is transmitted (E5.4)."""

    def _backend(self):
        backend = _build_official_backend()
        backend.app.placeOrder = mock.Mock()
        backend.app.cancelOrder = mock.Mock()
        return backend

    def _refusal_code(self, backend, payload) -> str:
        with self.assertRaises(BridgeRefusal) as caught:
            backend.submit(payload)
        return caught.exception.code

    def test_a_refused_submit_transmits_nothing_and_consumes_no_order_id(self) -> None:
        for name, prepare, payload_changes, code in (
            ("unmapped instrument", lambda app: None, {"instrument_id": "msft.xnas"}, "INSTRUMENT_UNMAPPED"),
            ("wrong account", lambda app: None, {"account_id": "acct.other"}, "ACCOUNT_MISMATCH"),
            ("gateway disconnected", lambda app: setattr(app, "connected_ready", False), {}, "GATEWAY_DISCONNECTED"),
            ("no order id yet", lambda app: setattr(app, "next_order_id", None), {}, "ORDER_ID_UNAVAILABLE"),
        ):
            with self.subTest(name):
                backend = self._backend()
                prepare(backend.app)
                order_id_before = backend.app.next_order_id
                payload = _submit_payload("order.refused.1") | payload_changes
                self.assertEqual(self._refusal_code(backend, payload), code)
                backend.app.placeOrder.assert_not_called()
                self.assertEqual(backend.app.next_order_id, order_id_before)
                self.assertEqual(backend.app.order_by_client, {})
                self.assertEqual(backend.app.client_by_order, {})

    def test_a_disconnected_gateway_does_not_reject_the_retry_of_a_known_order(self) -> None:
        # The order may already be working at IBKR, so its retry is answered from
        # what the bridge knows, never refused as a new order would be.
        backend = self._backend()
        with backend.app.condition:
            backend.app.order_by_client["order.retry.known"] = 900
            backend.app.orders["order.retry.known"] = {
                "client_order_id": "order.retry.known",
                "broker_order_id": "ibkr-paper-order-900",
                "state": "ACKNOWLEDGED",
                "filled_quantity": "0",
            }
            backend.app.connected_ready = False
        response = backend.submit(_submit_payload("order.retry.known"))
        self.assertEqual(response["status"], "ACKNOWLEDGED")
        backend.app.placeOrder.assert_not_called()

    def test_cancelling_an_unknown_order_is_reported_as_a_rejected_cancellation(self) -> None:
        backend = self._backend()
        self.assertEqual(backend.cancel({"client_order_id": "order.never.sent"}), {})
        backend.app.cancelOrder.assert_not_called()
        self.assertEqual(
            backend.app.events.get_nowait(),
            {
                "event_type": "CANCEL_REJECTED",
                "client_order_id": "order.never.sent",
                "reason": "IBKR_BRIDGE_REFUSED_ORDER_UNKNOWN",
            },
        )
        self.assertTrue(backend.app.events.empty())

    def test_cancelling_while_disconnected_is_reported_as_a_rejected_cancellation(self) -> None:
        backend = self._backend()
        with backend.app.condition:
            backend.app.order_by_client["order.live.1"] = 901
            backend.app.connected_ready = False
        self.assertEqual(backend.cancel({"client_order_id": "order.live.1"}), {})
        backend.app.cancelOrder.assert_not_called()
        self.assertEqual(
            backend.app.events.get_nowait()["reason"], "IBKR_BRIDGE_REFUSED_GATEWAY_DISCONNECTED"
        )
        self.assertNotIn("order.live.1", backend.app.cancel_requested)

    def test_cancelling_a_known_order_is_sent_and_remembered(self) -> None:
        backend = self._backend()
        with backend.app.condition:
            backend.app.order_by_client["order.live.2"] = 902
        self.assertEqual(backend.cancel({"client_order_id": "order.live.2"}), {})
        backend.app.cancelOrder.assert_called_once_with(902, "")
        self.assertIn("order.live.2", backend.app.cancel_requested)
        self.assertTrue(backend.app.events.empty())

    def test_a_malformed_cancel_payload_is_still_a_failure(self) -> None:
        backend = self._backend()
        with self.assertRaises(BridgeFailure):
            backend.cancel({"client_order_id": "Not Canonical"})
        self.assertTrue(backend.app.events.empty())


class OfficialBackendErrorCodeTests(unittest.TestCase):
    """IBKR message codes are classified by what they say about a tracked order (E5.4).

    The codes come from IBKR's documentation, not from a retained Gateway session
    (delivery state E5.6), so these tests pin the classification, not IBKR's behavior.
    """

    ORDER_ID = 555
    CLIENT_ORDER_ID = "order.err.1"

    def _backend_with_working_order(self):
        backend = _build_official_backend()
        with backend.app.condition:
            backend.app.client_by_order[self.ORDER_ID] = self.CLIENT_ORDER_ID
            backend.app.order_by_client[self.CLIENT_ORDER_ID] = self.ORDER_ID
            backend.app.orders[self.CLIENT_ORDER_ID] = {
                "client_order_id": self.CLIENT_ORDER_ID,
                "broker_order_id": f"ibkr-paper-order-{self.ORDER_ID}",
                "state": "ACKNOWLEDGED",
                "filled_quantity": "0",
            }
        return backend

    def _state(self, backend) -> str:
        return backend.app.orders[self.CLIENT_ORDER_ID]["state"]

    def test_notices_and_warnings_leave_a_working_order_working(self) -> None:
        for code in (131, 202, 399, 404, 2100, 2109, 2169):
            with self.subTest(code=code):
                backend = self._backend_with_working_order()
                backend.app.error(self.ORDER_ID, code, "notice", "")
                self.assertEqual(self._state(backend), "ACKNOWLEDGED")
                self.assertTrue(backend.app.events.empty())

    def _backend_placing_an_order(self):
        """An order handed to IBKR whose first callback has not arrived."""
        backend = _build_official_backend()
        with backend.app.condition:
            backend.app.client_by_order[self.ORDER_ID] = self.CLIENT_ORDER_ID
            backend.app.order_by_client[self.CLIENT_ORDER_ID] = self.ORDER_ID
        return backend

    def _drain(self, backend) -> list[dict]:
        events = []
        while not backend.app.events.empty():
            events.append(backend.app.events.get_nowait())
        return events

    def test_a_held_or_amended_order_is_acknowledged_not_rejected(self) -> None:
        # 404: held while shares are located for a short sale. 131: an attribute is ignored.
        # IBKR reports the message first and the status after, and the order works.
        for code in (404, 131):
            with self.subTest(code=code):
                backend = self._backend_placing_an_order()
                backend.app.error(self.ORDER_ID, code, "the order is held", "")
                backend.app.orderStatus(
                    self.ORDER_ID, "PreSubmitted", 0, 10, 0.0, 1, 0, 0.0, 7, "", 0.0
                )
                self.assertEqual(self._state(backend), "ACKNOWLEDGED")
                self.assertEqual(
                    self._drain(backend),
                    [
                        {
                            "event_type": "ACKNOWLEDGED",
                            "client_order_id": self.CLIENT_ORDER_ID,
                            "broker_order_id": f"ibkr-paper-order-{self.ORDER_ID}",
                        }
                    ],
                )

    def test_a_finished_order_is_not_reopened_by_a_late_status(self) -> None:
        for label, finish, finished_state in (
            ("rejected", lambda app: app.error(self.ORDER_ID, 201, "rejected", ""), "REJECTED"),
            (
                "cancelled",
                lambda app: app.orderStatus(
                    self.ORDER_ID, "Cancelled", 0, 10, 0.0, 1, 0, 0.0, 7, "", 0.0
                ),
                "CANCELLED",
            ),
            (
                "filled",
                lambda app: app.orderStatus(
                    self.ORDER_ID, "Filled", 10, 0, 100.0, 1, 0, 100.0, 7, "", 0.0
                ),
                "FILLED",
            ),
        ):
            with self.subTest(label=label):
                backend = self._backend_with_working_order()
                finish(backend.app)
                self.assertEqual(self._state(backend), finished_state)
                reported = self._drain(backend)
                # The same order, in the order IBKR can deliver a stale callback.
                for status, filled, remaining in (
                    ("PreSubmitted", 0, 10),
                    ("Submitted", 0, 10),
                    ("Submitted", 4, 6),
                ):
                    backend.app.orderStatus(
                        self.ORDER_ID, status, filled, remaining, 0.0, 1, 0, 0.0, 7, "", 0.0
                    )
                self.assertEqual(self._state(backend), finished_state)
                self.assertEqual(
                    backend.app.orders[self.CLIENT_ORDER_ID]["filled_quantity"],
                    "10" if finished_state == "FILLED" else "0",
                )
                self.assertEqual(self._drain(backend), [], f"{label}: {reported}")

    def test_a_late_error_does_not_reject_a_finished_order_or_report_a_second_rejection(self) -> None:
        for code in (201, 110):
            for state in ("REJECTED", "FILLED", "CANCELLED"):
                with self.subTest(code=code, state=state):
                    backend = self._backend_with_working_order()
                    backend.app.orders[self.CLIENT_ORDER_ID]["state"] = state
                    backend.app.error(self.ORDER_ID, code, "rejected", "")
                    self.assertEqual(self._state(backend), state)
                    self.assertTrue(backend.app.events.empty())

    def test_the_warning_band_ends_where_ibkr_documents_it(self) -> None:
        for code, state in ((2099, "REJECTED"), (2100, "ACKNOWLEDGED"), (2169, "ACKNOWLEDGED"), (2170, "REJECTED")):
            with self.subTest(code=code):
                backend = self._backend_with_working_order()
                backend.app.error(self.ORDER_ID, code, "message", "")
                self.assertEqual(self._state(backend), state)

    def test_a_failed_cancellation_this_bridge_requested_is_reported_once(self) -> None:
        for code in (135, 136, 161, 10147, 10148):
            with self.subTest(code=code):
                backend = self._backend_with_working_order()
                backend.app.cancel_requested.add(self.CLIENT_ORDER_ID)
                backend.app.error(self.ORDER_ID, code, "cannot cancel", "")
                self.assertEqual(self._state(backend), "ACKNOWLEDGED")
                self.assertEqual(
                    backend.app.events.get_nowait(),
                    {
                        "event_type": "CANCEL_REJECTED",
                        "client_order_id": self.CLIENT_ORDER_ID,
                        "reason": f"IBKR_ERROR_{code}",
                    },
                )
                backend.app.error(self.ORDER_ID, code, "cannot cancel", "")
                self.assertTrue(backend.app.events.empty())

    def test_a_cancel_failure_the_bridge_did_not_ask_for_changes_nothing(self) -> None:
        backend = self._backend_with_working_order()
        backend.app.error(self.ORDER_ID, 10148, "cannot cancel", "")
        self.assertEqual(self._state(backend), "ACKNOWLEDGED")
        self.assertTrue(backend.app.events.empty())

    def test_a_terminal_status_forgets_the_cancel_request(self) -> None:
        backend = self._backend_with_working_order()
        backend.app.cancel_requested.add(self.CLIENT_ORDER_ID)
        backend.app.orderStatus(
            self.ORDER_ID, "Cancelled", 0, 10, 0.0, 0, 0, 0.0, 7, "", 0.0
        )
        self.assertNotIn(self.CLIENT_ORDER_ID, backend.app.cancel_requested)

    def test_an_order_rejection_still_rejects(self) -> None:
        for code in (201, 110):
            with self.subTest(code=code):
                backend = self._backend_with_working_order()
                backend.app.error(self.ORDER_ID, code, "rejected", "")
                self.assertEqual(self._state(backend), "REJECTED")
                self.assertEqual(
                    backend.app.events.get_nowait(),
                    {
                        "event_type": "REJECTED",
                        "client_order_id": self.CLIENT_ORDER_ID,
                        "reason": f"IBKR_ERROR_{code}",
                    },
                )

    def test_an_error_for_no_tracked_order_changes_nothing(self) -> None:
        backend = self._backend_with_working_order()
        backend.app.error(-1, 201, "rejected", "")
        backend.app.error(999, 10148, "cannot cancel", "")
        self.assertEqual(self._state(backend), "ACKNOWLEDGED")
        self.assertTrue(backend.app.events.empty())


if __name__ == "__main__":
    unittest.main()
