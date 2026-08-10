# Follon IBKR PAPER gateway bridge

This bridge uses Interactive Brokers' official Python TWS API and communicates
with the Rust adapter through bounded JSON lines on private process pipes. It
cannot accept live ports or a non-`PAPER` environment, and it accepts no broker
credential. Authentication remains in TWS/IB Gateway.

Install the official Python API from a pinned TWS API distribution, review the
instrument map against IBKR contract details, enable socket clients in the PAPER
session, and use either TWS port `7497` or IB Gateway port `4002`. Run the bridge
only through `IbkrPaperBridgeProcessTransport`; stdout is reserved for protocol
messages. The current official setup and API reference are maintained in the
[IBKR TWS API documentation](https://ibkrcampus.com/campus/ibkr-api-page/twsapi-doc/).

The fixed process arguments have this shape (values are illustrative):

```text
C:\approved-python\python.exe C:\Follon\python\ibkr-gateway\src\follon_ibkr_gateway.py \
  --host 127.0.0.1 --port 7497 --client-id 7 \
  --account-id acct.paper.001 --broker-account DU_REVIEWED_ACCOUNT \
  --instrument-map C:\protected-config\ibkr-instruments.json \
  --tws-timezone America/New_York --environment PAPER --timeout-seconds 10
```

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

Run the bridge-only contract suite without TWS or `ibapi`:

```text
PYTHONPATH=python/ibkr-gateway/src python -m unittest discover -s python/ibkr-gateway/tests -v
```

This suite verifies the private protocol and fail-closed PAPER configuration.
It is not a substitute for a controlled integration test against the exact
operator-managed PAPER session and pinned official API build.
