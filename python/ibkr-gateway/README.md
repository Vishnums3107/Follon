# Follon IBKR PAPER gateway bridge

This bridge uses Interactive Brokers' official Python TWS API and communicates
with the Rust adapter through bounded JSON lines on private process pipes. It
cannot accept live ports or a non-`PAPER` environment, and it accepts no broker
credential. Authentication remains in TWS/IB Gateway.

This is the Python half of the IBKR boundary. Its Rust counterpart, which spawns
this bridge and implements the PAPER broker trait over it, is the
`follon-ibkr-paper-adapter` crate in
[`adapters/brokers/ibkr`](../../adapters/brokers/ibkr/README.md). The split is
deliberate: only this process imports the official `ibapi` package.

Install the official Python API from a pinned TWS API distribution, review the
instrument map against IBKR contract details, enable socket clients in the PAPER
session, and use either TWS port `7497` or IB Gateway port `4002`. Run the bridge
only through `IbkrPaperBridgeProcessTransport`; stdout is reserved for protocol
messages. The current official setup and API reference are maintained in the
[IBKR TWS API documentation](https://ibkrcampus.com/campus/ibkr-api-page/twsapi-doc/).

## What the bridge executes

Single market and limit orders, placed DAY, plus cancellation, execution
polling, account snapshots and reconnect. It has no combination (BAG)
operation, no replacement, no time in force other than DAY, and no market-data
request. The Rust adapter, `IbkrPaperGatewayAdapter`, declares exactly this
set, so the PAPER OMS refuses a combination, a GTC intent or a replacement
before any order exists, rather than leaving it `UNKNOWN` (delivery state
E5.1). The gRPC PAPER route composes it when its configuration selects
`adapter_kind: IBKR_PAPER_BRIDGE` (E5.2a). That route submits a single DAY
order and cancels it (E5.2b), and a risk manager can drain the bridge's events
and reconcile the account (E5.2c). It does not poll in the background.

## Refusals, rejections and failures

The bridge answers every request in one of three ways, and the Rust adapter
acts on the difference (delivery state E5.4):

| Answer | Meaning | What the OMS records |
| --- | --- | --- |
| `ok: true`, `status: REJECTED` | The bridge refused the submission **before contacting IBKR**: an unmapped instrument, another account, a malformed payload, a disconnected gateway, no order ID yet. The reason is `IBKR_BRIDGE_REFUSED_<CODE>`. | A clean rejection. The session stays connected. |
| `ok: true`, `{}`, then a `CANCEL_REJECTED` event | A cancellation was refused before anything was sent (an unknown client order ID, a disconnected gateway), or IBKR reported that the order was not cancelled. | The order returns to its working state. |
| `ok: false` | Anything else: the outcome may or may not have reached IBKR. | `UNKNOWN`, and the session is disconnected until it is reconnected and reconciled. |

The retry of a client order ID the bridge already knows is answered from what
it knows and is never refused as a new order would be, because that order may
already be working at IBKR.

IBKR's own message codes are classified by what they say about an order the
bridge tracks: 202 (order cancelled) and 399 (an order warning) and the
2100-2169 system warnings leave the order's state alone, because the state
arrives through `orderStatus`; 135, 136, 161, 10147 and 10148 mean a
cancellation this bridge requested did not happen; any other code on a tracked
order remains a rejection. These are the codes IBKR documents. No retained
Gateway session has produced them here, so the mapping is documented, not
measured (delivery state E5.6).

The fixed process arguments have this shape (values are illustrative):

```text
C:\approved-python\python.exe C:\Follon\python\ibkr-gateway\src\follon_ibkr_gateway.py \
  --host 127.0.0.1 --port 7497 --client-id 7 \
  --account-id acct.paper.001 --broker-account DU_REVIEWED_ACCOUNT \
  --account-currency USD \
  --instrument-map C:\protected-config\ibkr-instruments.json \
  --tws-timezone America/New_York --environment PAPER --timeout-seconds 10
```

The gRPC PAPER route builds exactly this list from its configuration's
`ibkr_bridge` section, with `--timeout-seconds` two seconds inside the
section's `request_timeout_seconds` (see
`contracts/json-schema/v1/paper-command-route.schema.json` and
`tests/fixtures/config/paper-command-route-v1-bridge.json`).

`--account-currency` is required and comes from the route's account currency,
not a second independently configurable value. Cash reconciliation uses only
`TotalCashValue` for the configured broker account, active request and exact
currency. A missing matching value fails the snapshot; `BASE` and other
currencies are never relabelled or converted. This is IBKR's reported account
summary cash, not a per-currency cash ledger or settled buying power. Configure
the PAPER books in the account's reporting currency and independently review
the opening balance before trading.

Use absolute, ACL-protected paths in the Rust process configuration. Record the
interpreter digest, official API version, bridge digest, TWS/Gateway build,
client ID, account, instrument-map digest, and time-zone database version with
each deployment. The broker account identifier is not a credential, but it is
visible to local process inspection and must still be protected as account
metadata.

The instrument map is a JSON object keyed by canonical Follon instrument ID:

```json
{
  "inst.us_equity.example": {
    "con_id": 123456,
    "symbol": "EXAMPLE",
    "security_type": "STK",
    "exchange": "SMART",
    "primary_exchange": "NASDAQ",
    "currency": "USD"
  }
}
```

Do not copy the placeholder contract into an operational deployment. Resolve
and independently verify the exact `con_id`, venue, primary exchange, currency,
lot/tick rules, account, client ID, TWS time zone, and PAPER port first.

Run the bridge contract suite:

```text
PYTHONPATH=python/ibkr-gateway/src python -m unittest discover -s python/ibkr-gateway/tests -v
```

The protocol and configuration tests need neither TWS nor `ibapi`. The
`OfficialBackend*` tests exercise the official backend and import the official
Python API, so without it they error with `No module named 'ibapi'`. The tests
pass against `ibapi` 9.81.1.post1, the only
version they have been run against. CI installs exactly that release,
hash-pinned in `requirements-ci.txt`, as the operator approved on 2026-09-29
(delivery state E4.2):

```text
python -m pip install --require-hashes -r python/ibkr-gateway/requirements-ci.txt
```

A deployment must still record and review its own distribution. On Windows, which has no system
time-zone database, `zoneinfo` also needs the `tzdata` package; without it
eight tests error with `No module named 'tzdata'`.

The suite is not a substitute for a controlled integration test against the
exact operator-managed PAPER session and pinned official API build.
