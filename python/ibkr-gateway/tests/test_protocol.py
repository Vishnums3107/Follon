from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from follon_ibkr_gateway import (
    BridgeFailure,
    BridgeProtocol,
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


if __name__ == "__main__":
    unittest.main()
