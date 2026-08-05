# Implementation status

This record maps implemented work to the roadmap gates. It is not a release
approval and does not override the unchecked legal, customer, and deployment
decisions in [foundation readiness](01-foundation-readiness.md).

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
| CI, dependency review, secret scanning | Configured | `.github/workflows/ci.yml` |

## Gate status

The Months 0–5 non-live research engineering exit gate passed locally on
2026-08-05:

- `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, and
  `cargo clippy --workspace --all-targets -- -D warnings` pass.
- `cargo test --workspace --all-targets` passes all 34 Rust tests, including
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

Broker connectivity, paper trading, live execution, short selling, and
cross-currency accounting remain explicitly out of scope. Passing this gate is
deployment evidence for the bounded historical-research product, not approval
for capital-bearing operation.

## Deployment artifact status

`infra/compose.dev.yml` provisions loopback-only non-production PostgreSQL and
MinIO, while `infra/Dockerfile.replay` builds a non-live replay image. The
Docker engine was unavailable on the local host during verification, so the
container build/run check remains required in CI or on a machine with Docker
Desktop's Linux engine running.
