# Follon — Solo Trading Operating System

Follon is a risk-first, multi-asset trading operating system for advanced independent traders and small professional teams. Its defining requirement is **research-to-live parity**: a strategy must behave equivalently in research, deterministic replay, simulation, paper trading, and controlled live execution.

This repository is in its **non-connected controlled-live engineering** stage. It contains the
decomposed product plan, versioned contracts, a deterministic historical replay
slice, an isolated strategy SDK, a durable PAPER OMS/control boundary, and an
evidence-only desktop shell. It also contains a controlled-live safety kernel
for hash-chained audit, four-eyes activation/approvals, shadow/canary limits,
reconciliation, and monitoring. It has no checked-in live credentials,
authenticated approval service, or live broker endpoint. A fixed-process
managed-secret provider is available for an audited vault/keychain helper, but
no provider or credential is configured by the repository. The checked-in IBKR
implementation includes a PAPER-only official-API process bridge and a
deterministic local model; connecting it still requires an operator-managed TWS
or IB Gateway PAPER session and the documented acceptance gates.

## Start here

1. Read [the documentation index](docs/README.md).
2. Read the [product charter](docs/00-product/01-product-charter.md) and [first vertical slice](docs/06-delivery/02-first-vertical-slice.md).
3. Treat the [event envelope](docs/01-domain/02-event-envelope.md), [order intent](docs/01-domain/04-workflow-and-order-intent.md), and [modular-monolith ADR](docs/02-architecture/adr/0001-modular-monolith.md) as the initial implementation contracts.

The source plan is retained as `Solo Trading Operating System Master Plan.pdf`.

## Implemented foundation

- Fixed-point Rust domain types, immutable canonical event envelopes, and
  effective-dated instrument reference data.
- Historical-bar CSV import, explicit replay clock and exchange-session model,
  canonical timestamp/order validation, append-only local NDJSON event storage,
  deterministic risk/OMS/simulator, and cumulative portfolio evidence flow.
- Content-addressed bar/action datasets, deterministic backtest runner, exact
  single-currency accounting, content-addressed configuration, portable
  self-describing result artifacts, completion manifests, and local experiment
  metadata/export.
- Python strategy contracts that can submit intents but cannot access adapters
  or credentials.
- A PAPER-only OMS with versioned risk limits, fresh-market checks, cash
  reservation, durable evidence/restart recovery, reconciliation, kill
  switches, reconnect handling, deterministic broker fault injection, and a
  bounded process transport for the official IBKR Python TWS API bridge.
- A TypeScript desktop evidence projection that cannot alter trading state,
  including read-only PAPER and controlled-live monitoring dashboards.
- A controlled-live safety kernel with opaque credential references, zeroizing
  secret-material boundary, a no-shell managed-helper provider, time-bounded
  four-eyes activations and approvals,
  shadow/canary separation, independent reconciliation, hash-chained durable
  audit, disaster-recovery status, and a 60-session evidence gate. The supplied
  status CLI is intentionally incapable of connecting to or submitting at a
  broker.
- JSON Schema, Protobuf, CI, dependency review, secret scanning, and an initial
  threat model.

See [implementation status](docs/06-delivery/05-implementation-status.md) for
the active roadmap gate and local verification status.

## Initial release boundary

- US equities and ETFs through one Interactive Brokers integration.
- Bar and quote data, single account, paper then limited-capital live trading.
- Market, limit, stop, and bracket orders.
- End-of-day and intraday strategies.

High-frequency trading, custody, investment advice, unrestricted data redistribution, mobile, social features, and multiple brokers are out of scope for the initial release.
