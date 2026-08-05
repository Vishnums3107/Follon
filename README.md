# Follon — Solo Trading Operating System

Follon is a risk-first, multi-asset trading operating system for advanced independent traders and small professional teams. Its defining requirement is **research-to-live parity**: a strategy must behave equivalently in research, deterministic replay, simulation, paper trading, and controlled live execution.

This repository is in its **non-live foundation** stage. It contains the
decomposed product plan, versioned contracts, a deterministic historical replay
slice, an isolated strategy SDK, and an evidence-only desktop shell. It has no
broker connectivity, credentials, paper-trading adapter, or live execution.

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
- A TypeScript desktop evidence projection that cannot alter trading state.
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
