from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock
from zoneinfo import ZoneInfoNotFoundError

from follon_ibkr_gateway import (
    BridgeFailure,
    BridgeProtocol,
    create_official_backend,
    load_instruments,
    normalize_execution_time,
    normalize_order_state,
    parse_arguments,
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
        self, *, exec_id: str, order_ref: str, order_id: int, shares: str, price: str, time: str
    ) -> None:
        self.execId = exec_id
        self.orderRef = order_ref
        self.orderId = order_id
        self.shares = shares
        self.price = price
        self.time = time
        self.cumQty = shares


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


if __name__ == "__main__":
    unittest.main()
