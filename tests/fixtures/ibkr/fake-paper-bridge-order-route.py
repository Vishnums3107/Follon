"""Account-consistent process fixture for the gRPC PAPER command route."""

import json
import sys

order = None

for line in sys.stdin:
    request = json.loads(line)
    operation = request["operation"]
    payload = request["payload"]
    if operation == "submit":
        order = payload
        result = {"status": "ACKNOWLEDGED", "broker_order_id": "ibkr.41", "reason": None}
    elif operation == "cancel":
        result = {}
    elif operation == "poll":
        result = []
    elif operation == "snapshot":
        result = {
            "orders": []
            if order is None
            else [
                {
                    "client_order_id": order["client_order_id"],
                    "broker_order_id": "ibkr.41",
                    "state": "ACKNOWLEDGED",
                    "filled_quantity": "0",
                }
            ],
            "positions": [],
            "cash": "100000",
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
