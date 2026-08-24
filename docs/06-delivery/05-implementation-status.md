# Implementation status

This record maps implemented work to the roadmap gates. It is not a release
approval and does not override the unchecked legal, customer, and deployment
decisions in [foundation readiness](01-foundation-readiness.md).

## Active gate and scope freeze

The active work is Release 1 replay-to-paper acceptance and founder-led
customer validation. Later-phase primitives are present, but their external
gates are still open: **0/30** observed paper sessions, **0/60** controlled-live
sessions, **0/5** unaided design partners, **0** independent broker-backed
options reconciliation sessions, and no paying-customer evidence. Do not add a
broker, asset class, India order flow, FIX, multi-account/team surface, or more
commercial infrastructure until the earlier gates in the
[roadmap](03-roadmap-and-gates.md) are independently evidenced.

## Months 0–2: foundation

| Requirement | Status | Evidence |
| --- | --- | --- |
| Canonical event envelope and decimal accounting | Implemented | `core/domain`, versioned JSON schemas |
| Historical bar import, persisted event loading, and append-only local event log | Implemented | `import_historical_bars`, `load_persisted_market_bars`, `FileEventStore`, CSV fixture |
| Deterministic strategy → intent → risk → OMS → fill → portfolio flow | Implemented for simulation | `core/control-plane` replay tests and `follon-replay` CLI |
| Versioned instrument reference data | Implemented and active | `core/instrument::InstrumentRegistry` is a replay precondition |
| Explicit exchange-session model | Implemented and active | `core/instrument::StaticTradingCalendar` blocks out-of-session replay bars |
| Python strategy SDK boundary | Implemented | `python/strategy-sdk` and example strategy |
| Desktop evidence shell | Implemented | `apps/desktop`, projection-only WebSocket view |
| CI, lockfile advisory audits, secret scanning | Configured | `.github/workflows/ci.yml`; native dependency review is an explicit opt-in when the repository plan supports it |

## Months 0–5 gate status

The Months 0–5 non-live research engineering exit gate passed locally on
2026-08-05:

- `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, and
  `cargo clippy --workspace --all-targets -- -D warnings` pass.
- `cargo test --workspace --all-targets` passes the full Rust suite, including
  canonical persisted-event recovery, cumulative accounting, content-addressed
  configuration, corporate actions, and immutable artifact publication.
- Python contract tests, JSON-schema parsing, and TypeScript typechecking pass.
- Two fresh end-to-end runs produced byte-for-byte identical normalized bars,
  JSON artifacts, persisted NDJSON events, Markdown reports, completion
  manifests, and experiment records. Repeating a completed run in place was
  idempotent.
- The evidence client builds to static JavaScript and validates a local NDJSON
  causal chain before rendering it.

The Windows build prerequisite is Visual Studio Build Tools with the **Desktop
development with C++** workload. From its developer command prompt, run:

```powershell
cargo test --workspace
cargo run -p follon-cli --bin follon-replay -- tests/fixtures/historical-bars/spy-one-minute.csv var/follon-events.ndjson
```

The original replay runner remains long-only and single-currency for v1
artifact compatibility. A separate advanced backtest account now implements
point-in-time universe controls, long/short crossings, borrow limits/recalls,
financing, attributed charges, delistings, multi-currency FX, and atomic margin
capital checks. It does not retroactively relabel legacy artifacts; public CLI
orchestration of the advanced account remains explicit follow-on integration.
PAPER controls are described in
[Months 6–8 status](07-months-6-8-status.md). Passing this gate is evidence for
the bounded historical-research product, not approval for capital-bearing
operation.

## Deployment artifact status

`infra/compose.dev.yml` provisions loopback-only non-production PostgreSQL,
MinIO, the React dashboard, and the gRPC trading API. Production Compose adds
database TLS, gRPC mTLS, client-certificate dashboard TLS, secret-file ingress,
monitoring and alerts without creating a production database, CA, secret store,
or broker credential. Both development and production configurations validate.
Container build/runtime health was not rerun on 2026-08-24 because the local
Docker Desktop engine was unavailable.

## Platform and advanced-kernel status

Advanced EMS planning, portfolio-wide risk, multi-currency/margin accounting,
customer IAM/MFA/RBAC, transactional PostgreSQL event/outbox persistence, the
versioned gRPC topology, React/Vite/Tauri packaging, and a review-bound
controlled-LIVE adapter are implemented and tested at repository boundaries.
The adapter has no configured real LIVE transport, credential, signed review
record, or capital session. See the
[master-plan conformance audit](14-master-plan-conformance-audit.md) for exact
coverage and remaining requirements.

## Months 12–14 operator-workbench status

The local risk cockpit, attribution, alerts, schedule planner, parameter/config
validation, journal, reports, replay-facing desktop view, and strict contracts
are implemented as deterministic evidence workflows. The related status record
is [Months 12–14 status](09-months-12-14-status.md). The required external
design-partner adoption result is not yet observed: **0/5** partners have
completed normal work unaided in repository evidence.

## Months 15–17 options status

The deterministic European-options model, chain fingerprint, implied
volatility/Greeks, multi-leg expiry scenarios, option-book reconciliation, CLI
artifacts, desktop evidence view, and strict contracts are implemented. The
external broker-backed acceptance result remains unobserved: **0 sessions**.
See [Months 15–17 status](10-months-15-17-status.md) for the model boundary and
required production evidence.

## Months 18–20 commercial-controls status

Local provisioning, subscription-evidence, privacy/retention, signed-release,
and self-host-readiness primitives are implemented and tested. They do not
process payment or demonstrate adoption. The external commercial gate remains
unobserved: **0/10** paying professionals and **0/3** paying organizations in
repository evidence. See [Months 18–20 status](11-months-18-20-status.md).

## Unified dashboard integration

All implemented capability domains and the ten documented primary screens have
workspace-specific read-only projections in the Docker dashboard, including
runtime/gate health, datasets, experiments, strategy identities, backtest
comparison, OMS lifecycle evidence, risk, portfolio/attribution, causal replay,
journals, options, and commercial/deployment status. Recursive source evidence
and explicit source/documentation/gate metadata remain available. External and
not-yet-implemented product work is recorded without being marked complete. See the
[dashboard feature integration status](12-dashboard-feature-integration-status.md).

## Documentation-driven continuation

The first four internal steps are implemented: explicit
spread with limit protection, point-in-time halt controls, and persistent
latency/partial-fill simulation, followed by deterministic Parquet, validated
DuckDB registration, and immutable S3-compatible storage. Effective-dated
instrument lookup also has a fail-closed post-end regression test. Production
storage policy, real operating evidence, and external production gates remain
distinct. Continue in the ordered
[step-by-step implementation matrix](13-step-by-step-implementation-matrix.md).
