"""Stateful process-protocol fixture for the gRPC PAPER reconciliation route."""

import json
import sys
from decimal import Decimal

order = None
pending = []
cash = Decimal("100000")
positions = {}


for line in sys.stdin:
    request = json.loads(line)
    operation = request["operation"]
    payload = request["payload"]
    if operation == "submit":
        order = payload
        quantity = Decimal(payload["quantity"])
        price = Decimal("100")
        fee = Decimal("0.20")
        signed_quantity = quantity if payload["side"] == "BUY" else -quantity
        positions[payload["instrument_id"]] = signed_quantity
        cash -= signed_quantity * price + fee
        pending.append(
            {
                "event_type": "EXECUTION",
                "execution_id": "execution.fixture.1",
                "client_order_id": payload["client_order_id"],
                "broker_order_id": "ibkr.fixture.41",
                "quantity": str(quantity),
                "price": str(price),
                "fee": str(fee),
                "executed_at": "2026-01-02T14:31:00Z",
                "reason": None,
            }
        )
        result = {
            "status": "ACKNOWLEDGED",
            "broker_order_id": "ibkr.fixture.41",
            "reason": None,
        }
    elif operation == "poll":
        result = pending
        pending = []
    elif operation == "snapshot":
        result = {
            "orders": []
            if order is None
            else [
                {
                    "client_order_id": order["client_order_id"],
                    "broker_order_id": "ibkr.fixture.41",
                    "state": "FILLED",
                    "filled_quantity": order["quantity"],
                }
            ],
            "positions": [
                {"instrument_id": instrument_id, "quantity": str(quantity)}
                for instrument_id, quantity in positions.items()
            ],
            "cash": str(cash),
        }
    elif operation == "reconnect":
        result = {}
    else:
        raise RuntimeError("unsupported fixture operation")
    print(
        json.dumps(
            {
                "protocol_version": 1,
                "request_id": request["request_id"],
                "ok": True,
                "result": result,
                "error": None,
            },
            separators=(",", ":"),
        ),
        flush=True,
    )
