# Follon IBKR adapter (Rust)

The `follon-ibkr-paper-adapter` crate turns Follon's normalized broker contract
into calls on an Interactive Brokers TWS or IB Gateway session running on the
same machine. It decides nothing: the PAPER OMS and risk gate have already
accepted a request before it is submitted, and the adapter holds no broker
credential (authentication stays in TWS / IB Gateway).

## Two pieces, one boundary

There are two IBKR components on purpose, one on each side of a process
boundary. They are not redundant.

| | This crate | [`python/ibkr-gateway`](../../../python/ibkr-gateway/README.md) |
| --- | --- | --- |
| Role | Implements the broker traits, validates configuration, owns the bridge process | The only code that imports IBKR's official Python TWS API (`ibapi`) |
| Faces | The PAPER OMS in `core/paper` (and `core/live` for the LIVE types below) | TWS / IB Gateway |
| Talks to the other | Spawns the bridge and speaks bounded JSON lines over private process pipes | Reads and writes those pipes; stdout is reserved for the protocol |
| Accepts | Loopback host and PAPER ports `7497` (TWS) or `4002` (Gateway) | The same PAPER ports only; it refuses live ports and any non-`PAPER` environment |

Read the [bridge README](../../../python/ibkr-gateway/README.md) for the
protocol, the instrument map and the process arguments.

## What is in the crate

- **PAPER.** `IbkrPaperGatewayConfiguration` rejects any host other than
  loopback and any port other than `7497` or `4002`.
  `IbkrPaperBridgeProcessTransport` runs the Python bridge as a child process,
  and `IbkrPaperGatewayAdapter` implements `PaperBrokerAdapter` over a transport.
  The adapter declares exactly what the bridge executes: single market and limit
  orders placed DAY, cancellation, execution polling, account snapshots and
  reconnect. A combination, a replacement or another time in force is refused by
  the PAPER OMS before an order exists.
- **Controlled LIVE types.** `IbkrLiveGatewayConfiguration` (loopback, LIVE ports
  `7496` / `4001`, positive canary limits), `IbkrControlledLiveAdapter`
  (implements `LiveBrokerAdapter` over an `IbkrLiveGatewayTransport`), and the
  capital-adapter release checks. The LIVE adapter declares single DAY orders and
  price replacement, so controlled LIVE refuses a combination or a GTC intent
  before an approval is spent. The only implementation of
  `IbkrLiveGatewayTransport` in this repository is a test fake, and the Python
  bridge refuses live ports, so nothing here reaches a LIVE session.

## Tests

```bash
cargo test -p follon-ibkr-paper-adapter
```

The tests run the real process transport against stand-in bridges under
`tests/fixtures/ibkr/` (skipped when Python is unavailable), and use a fake LIVE
transport. Most stand-ins imitate the wire format.
`refusing-paper-bridge.py` instead serves the bridge's own protocol dispatcher
with a backend that refuses, so the refusal-to-rejection mapping is tested
across the process boundary. None of them talks to a real TWS or Gateway
session or to a real `ibapi` connection. For what is and is not proven end to end, and the
open gates, see
[`docs/06-delivery/16-delivery-state.md`](../../../docs/06-delivery/16-delivery-state.md).
