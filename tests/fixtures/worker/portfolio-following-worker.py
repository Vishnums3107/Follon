"""Test fixture: a protocol-v1 strategy worker that trades what its portfolio says it holds.

    portfolio-following-worker.py BUNDLE_HASH STRATEGY_ID STRATEGY_VERSION

It announces the identity it is given and needs the bounded services, whose portfolio
snapshot it reads on every bar (delivery state E8.3). On its first bar it buys one share.
On its fourth it sells everything the snapshot says it holds. On every bar it reports the
quantity and the cash it was shown as metrics, so a test can read what the worker saw.

Nothing here is a strategy. It exists to show what a worker is handed after a corporate
action: a stale snapshot makes it sell the wrong quantity and report the wrong cash.
"""

import hashlib
import json
import sys
from decimal import Decimal

bundle_hash, strategy_id, strategy_version = sys.argv[1:4]
print(
    json.dumps(
        {
            "type": "ready",
            "protocol_version": 1,
            "bundle_hash": bundle_hash,
            "strategy_id": strategy_id,
            "strategy_version": strategy_version,
        }
    ),
    flush=True,
)

# The host binds a service state to its values by their fingerprint. This worker keeps none.
EMPTY_STATE_FINGERPRINT = hashlib.sha256(b"{}").hexdigest()


def plain(value):
    # `str(Decimal("0.00000000"))` is "0E-8", which is not a decimal the host accepts.
    return format(value, "f")


def metric(name, value, observed_at):
    return {"name": name, "observed_at": observed_at, "tags": [], "value": plain(value)}


def intent(context, instrument_id, side, quantity, tag):
    return {
        "account_id": context["account_id"],
        "configuration_version": context["configuration_version"],
        "correlation_id": f"corr-following-{tag}",
        "created_at": context["replay_time"],
        "environment": context["environment"],
        "instrument_id": instrument_id,
        "intent_id": f"intent-following-{tag}",
        "limit_price": None,
        "order_type": "MARKET",
        "quantity": plain(Decimal(quantity)),
        "rationale": "sizes its exit from its portfolio snapshot",
        "side": side,
        "strategy_id": context["strategy_id"],
        "strategy_version": context["strategy_version"],
        "time_in_force": "DAY",
    }


bars_seen = 0
for line in sys.stdin:
    request = json.loads(line)
    context = request["context"]
    bar = request["bar"]
    portfolio = request["services"]["portfolio"]
    held = Decimal(0)
    for position in portfolio["positions"]:
        if position["instrument_id"] == bar["instrument_id"]:
            held = Decimal(position["quantity"])
    cash = Decimal(portfolio["cash_by_currency"][0]["amount"])

    bars_seen += 1
    chosen = None
    if bars_seen == 1:
        chosen = intent(context, bar["instrument_id"], "BUY", 1, "entry")
    elif bars_seen == 4 and held > 0:
        chosen = intent(context, bar["instrument_id"], "SELL", held, "exit")

    replay_time = context["replay_time"]
    output = {
        "type": "strategy_output",
        "protocol_version": 1,
        "intent": chosen,
        "metrics": [
            metric("portfolio.quantity", held, replay_time),
            metric("portfolio.cash", cash, replay_time),
        ],
        "state": {"fingerprint": EMPTY_STATE_FINGERPRINT, "values": {}},
    }
    print(json.dumps(output, separators=(",", ":")), flush=True)
