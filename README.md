# Follon — Solo Trading Operating System

Follon is a risk-first, multi-asset trading operating system for advanced independent traders and small professional teams. Its defining requirement is **research-to-live parity**: a strategy must behave equivalently in research, deterministic replay, simulation, paper trading, and controlled live execution.

The complete capability set is a reference architecture, not a fixed 24-month
solo-founder commitment. The active scope includes US-equities Replay, PAPER,
and controlled-LIVE order entry; the current operational status is recorded in
the [roadmap](docs/06-delivery/03-roadmap-and-gates.md).

This repository is an **active trading platform**.
It contains the decomposed product plan, versioned contracts, deterministic
research/replay and PAPER paths, advanced broker-neutral execution planning,
portfolio-wide risk, multi-currency/margin accounting, customer IAM primitives,
transactional PostgreSQL persistence, and a versioned gRPC service. The ten
operating workspaces are packaged with React/Vite and a Tauri v2 host. The
desktop uses privileged IPC commands to submit declarative order intents and
order-management requests to the application routing boundary.

The desktop provides active PAPER and LIVE order-entry controls. An IPC command
does not contact a broker itself: it creates a validated request for the
Risk/OMS route, which remains the sole authority allowed to submit through a
broker adapter. See the [master-plan conformance audit](docs/06-delivery/14-master-plan-conformance-audit.md)
for the implemented route and external operational status.
The checked-in desktop host rejects requests until a deployment configures that
Risk/OMS gateway; it never treats an unconfigured route as a broker submission.

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
- Content-addressed bar/action datasets, deterministic backtest runner,
  content-addressed configuration, portable
  self-describing result artifacts, completion manifests, and local experiment
  metadata/export.
- Python strategy contracts that can submit intents but cannot access adapters
  or credentials.
- A PAPER-only OMS with versioned risk limits, fresh-market checks, cash
  reservation, durable evidence/restart recovery, reconciliation, kill
  switches, reconnect handling, deterministic broker fault injection, and a
  bounded process transport for the official IBKR Python TWS API bridge.
- A React/TypeScript desktop trading terminal, including active PAPER and LIVE
  order-entry, cancel, and position-close controls alongside monitoring,
  operations, portfolio, identity, platform, and acceptance-gate views. Vite
  creates the web bundle and Tauri v2 supplies the privileged native package
  boundary.
- Broker-neutral EMS algorithms (immediate, TWAP, VWAP, participation,
  arrival-price, passive cancel/replace, routing, brackets, trailing stops,
  baskets, and atomic option combinations), portfolio-wide risk, and balanced
  multi-currency/margin accounting. Scheduled, passive, combination, risk, and
  margin planning are exposed through the versioned gRPC API.
- Customer IAM primitives with Argon2id, TOTP MFA, lockout, short opaque
  sessions, revocation, tenant isolation, and explicit RBAC permissions.
- Transactional PostgreSQL migrations and adapter behavior for tenant-isolated
  events/outbox, checkpoints, balanced journals, IAM, risk policies, and broker
  command receipts.
- Development and production Compose topology for the dashboard and gRPC API,
  plus production mTLS, client-certificate dashboard TLS, monitoring/alert
  rules, backup/restore-drill tooling, and ordered release-promotion gates.
- A deterministic operator workbench for fixed-point risk cockpit projections,
  attributable accounting movements, stable alerts, explicit-time schedule
  planning with typed due-time completions, predecessor-linked parameter
  validation, tamper-evident operations journal records, immutable reports,
  and replay/configuration evidence. It has
  no broker credentials, direct adapter access, wall-clock, or background
  execution capability. Trading requests enter through the separately bounded
  desktop IPC and Risk/OMS route.
- Immutable end-of-day execution-cost analysis against frozen arrival and target
  benchmarks, typed model-risk and fault-game-day journals/registers, and a
  versioned local risk-latency benchmark. These are evidence mechanisms, not
  claims of broker-backed acceptance, production availability, or legal approval.
- A deterministic European-options core for versioned chain snapshots,
  fixed-point implied volatility/Greeks, multi-leg expiry scenarios, explicit
  cash/physical exercise and assignment settlement, and explicit-time
  BACKTEST/PAPER/LIVE option-book reconciliation with separately fingerprinted
  declared export provenance. Option-combination planning is broker-neutral;
  no reviewed capital-bearing broker options transport or credential is included.
- A controlled-live safety kernel with opaque credential references, zeroizing
  secret-material boundary, a no-shell managed-helper provider, time-bounded
  four-eyes activations and approvals,
  shadow/canary separation, independent reconciliation, hash-chained durable
  audit, disaster-recovery status, and a 60-session evidence metric. The supplied
  status CLI is intentionally incapable of connecting to or submitting at a
  broker.
- Commercial-control and self-hosting primitives: typed, hash-chained tenant
  provisioning/subscription evidence; deterministic entitlements; pseudonymous
  privacy and retention plans with per-file confirmation; immutable execution
  receipts; Ed25519-signed release manifests; and restricted self-host readiness
  verification. These controls neither process payments nor prove customer
  adoption—the paying-customer gate remains external, independently evidenced
  work.
- JSON Schema, Protobuf, CI (including disposable PostgreSQL integration),
  dependency review, secret scanning, and an initial threat model.

See the [conformance audit](docs/06-delivery/14-master-plan-conformance-audit.md)
for current implementation evidence and the
[production operations runbook](docs/operations/09-production-operations-runbook.md)
for the deployment and evidence sequence.

## Initial release boundary

- US equities and ETFs through one Interactive Brokers integration.
- Bar and quote data, single account, paper then limited-capital live trading.
- Market, limit, stop, and bracket orders.
- End-of-day and intraday strategies.

High-frequency trading, custody, investment advice, unrestricted data redistribution, mobile, social features, and multiple brokers are out of scope for the initial release.
