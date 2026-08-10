"""Deterministic protocol fixture for the Rust process transport test."""

import json
import sys


def response(request, result):
    return {
        "protocol_version": 1,
        "request_id": request["request_id"],
        "ok": True,
        "result": result,
        "error": None,
    }


for line in sys.stdin:
    request = json.loads(line)
    operation = request["operation"]
    if operation == "submit":
        result = {"status": "ACKNOWLEDGED", "broker_order_id": "ibkr.41", "reason": None}
    elif operation in {"cancel", "reconnect"}:
        result = {}
    elif operation == "poll":
        result = [
            {
                "event_type": "EXECUTION",
                "execution_id": "execution.1",
                "client_order_id": "order.1",
                "broker_order_id": "ibkr.41",
                "quantity": "2",
                "price": "101.25",
                "fee": "0.35",
                "executed_at": "2026-01-02T14:31:00Z",
                "reason": None,
            }
        ]
    elif operation == "snapshot":
        result = {
            "orders": [
                {
                    "client_order_id": "order.1",
                    "broker_order_id": "ibkr.41",
                    "state": "FILLED",
                    "filled_quantity": "2",
                }
            ],
            "positions": [{"instrument_id": "aapl.xnas", "quantity": "2"}],
            "cash": "797.15",
        }
    else:
        raise RuntimeError("unsupported fixture operation")
    print(json.dumps(response(request, result), separators=(",", ":")), flush=True)
