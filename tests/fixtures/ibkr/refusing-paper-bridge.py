"""Process fixture that serves the bridge's real dispatcher with a refusing backend.

`fake-paper-bridge.py` imitates the wire format. This runs the bridge's own
`BridgeProtocol` and `serve` loop, so the Rust transport is tested against the
real refusal-to-rejection mapping rather than a copy of it. Only the backend is
replaced: it refuses every submission and every cancellation the way the
official backend does before it contacts IBKR (delivery state E5.4).
"""

import sys
from pathlib import Path

sys.path.insert(
    0, str(Path(__file__).resolve().parents[3] / "python" / "ibkr-gateway" / "src")
)

from follon_ibkr_gateway import BridgeProtocol, BridgeRefusal, serve  # noqa: E402


class RefusingBackend:
    def __init__(self):
        self.events = []

    def submit(self, payload):
        raise BridgeRefusal("INSTRUMENT_UNMAPPED", "instrument is not in the reviewed map")

    def cancel(self, payload):
        self.events.append(
            {
                "event_type": "CANCEL_REJECTED",
                "client_order_id": payload["client_order_id"],
                "reason": "IBKR_BRIDGE_REFUSED_ORDER_UNKNOWN",
            }
        )
        return {}

    def poll(self):
        events, self.events = self.events, []
        return events

    def snapshot(self, payload):
        return {"orders": [], "positions": [], "cash": "1000.00"}

    def reconnect(self):
        return {}

    def shutdown(self):
        pass


raise SystemExit(serve(BridgeProtocol(RefusingBackend())))
