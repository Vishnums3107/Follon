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
`adapter_kind: IBKR_PAPER_BRIDGE` (E5.2a). That route cannot yet submit a
single order through it (E5.2b) or synchronize its fills (E5.2c).

The fixed process arguments have this shape (values are illustrative):

```text
C:\approved-python\python.exe C:\Follon\python\ibkr-gateway\src\follon_ibkr_gateway.py \
  --host 127.0.0.1 --port 7497 --client-id 7 \
  --account-id acct.paper.001 --broker-account DU_REVIEWED_ACCOUNT \
  --instrument-map C:\protected-config\ibkr-instruments.json \
  --tws-timezone America/New_York --environment PAPER --timeout-seconds 10
```

The gRPC PAPER route builds exactly this list from its configuration's
`ibkr_bridge` section, with `--timeout-seconds` two seconds inside the
section's `request_timeout_seconds` (see
`contracts/json-schema/v1/paper-command-route.schema.json` and
`tests/fixtures/config/paper-command-route-v1-bridge.json`).

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

Eight of its fourteen tests need neither TWS nor `ibapi`: they verify the
private protocol and the fail-closed PAPER configuration. The other six (the
four `OfficialBackendSubmitRetryTests` and two of the
`OfficialBackendExecutionTimeTests`) exercise the official backend and import
the official Python API, so without it they error with
`No module named 'ibapi'`. They pass against `ibapi` 9.81.1.post1, the only
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
