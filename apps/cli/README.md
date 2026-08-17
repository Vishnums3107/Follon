# Operator CLI

`follon-replay` runs the non-live, deterministic first vertical slice and
persists canonical event envelopes as NDJSON. It has no live broker flags and
will not accept credentials.

Once Rust is installed, run:

```powershell
cargo run -p follon-cli --bin follon-replay -- tests/fixtures/historical-bars/spy-one-minute.csv var/follon-events.ndjson
```

The command prints the immutable evidence trail, writes the same events to the
specified file, and reports the resulting simulated position and P&L. The
default bar, strategy, seed-free fill model, and configuration are fixed so a
second run produces the same canonical output.

With no arguments it reads the checked-in fixture and writes
`var/follon-events.ndjson`.

To replay market input from a previously persisted canonical event log instead
of CSV, use a new output path:

```powershell
cargo run -p follon-cli --bin follon-replay -- --event-log var/follon-events.ndjson var/replayed-events.ndjson
```

Only `market.bar.v1` events become replay input; every other persisted event is
kept as historical evidence. The loader rejects malformed, duplicate, or
out-of-order market events.

## Reproducible backtest artifact

`follon-backtest` runs the same non-live strategy/risk/OMS kernel, then writes
both a canonical result artifact and its canonical NDJSON event stream. The
artifact fingerprints the strategy source, versioned dataset (including any
corporate actions), the exact configuration bytes, engine, seed, output events,
and exact single-currency ledger report.

```powershell
cargo run -p follon-cli --bin follon-backtest -- tests/fixtures/historical-bars/spy-one-minute.csv var/spy-backtest.json --config tests/fixtures/config/backtest-v1.json
```

For a versioned corporate-action CSV using the header
`action_id,instrument_id,action_type,effective_at,value`, pass `--actions`:

```powershell
cargo run -p follon-cli --bin follon-backtest -- tests/fixtures/historical-bars/spy-one-minute.csv var/spy-backtest.json --actions tests/fixtures/historical-bars/spy-corporate-actions.csv
```

The command writes `var/spy-backtest.json`,
`var/spy-backtest.events.ndjson`, `var/spy-backtest.report.md`, and
`var/spy-backtest.manifest.json`. The manifest is published last and contains
the exact SHA-256 digest of every output plus the specification and
configuration fingerprints.
Re-running with identical declared inputs must produce byte-identical files.
The CLI rejects attempts to overwrite a different immutable artifact.

The versioned JSON configuration owns the account, strategy identity, dataset
identity, engine/seed, risk and fill settings, calendar sessions, and
effective-dated instrument reference data. Unknown fields, non-canonical UTC
timestamps, inconsistent references, and invalid monetary or risk limits fail
closed. The hash is calculated over the exact configuration file bytes.

To record a completed run in a durable local experiment catalog, append:

```powershell
--experiment var/experiments.ndjson experiment-example-001 run-001
```

## Deterministic trade importer and bar builder

`follon-build-bars` converts the strict normalized v1 trade contract into the
canonical historical-bar contract consumed by replay and backtesting:

```powershell
cargo run -p follon-cli --bin follon-build-bars -- tests/fixtures/historical-bars/spy-trades-v1.csv var/spy-bars.csv --interval-seconds 60 --exchange-timezone America/New_York
```

Trade IDs and per-instrument source sequences must be unique. The builder sorts
by source time/sequence, emits canonical `(event_time, instrument_id)` order,
uses exact decimals, and atomically publishes an immutable output file.

## Python strategy worker

Install `python/strategy-sdk` into a dedicated virtual environment first, or
explicitly supply the checked-out SDK source path with the non-secret
`FOLLON_STRATEGY_SDK_PATH` variable. The worker process intentionally receives
a cleared environment. Obtain the bundle hash and use an absolute Python path:

```powershell
$env:FOLLON_STRATEGY_SDK_PATH = (Resolve-Path python/strategy-sdk/src)
$env:PYTHONPATH = $env:FOLLON_STRATEGY_SDK_PATH # only for the hash command
$bundleHash = python -c "from follon_strategy_sdk import strategy_bundle_hash; print(strategy_bundle_hash('python/examples'))"
cargo run -p follon-cli --bin follon-backtest -- tests/fixtures/historical-bars/spy-one-minute.csv var/python-backtest.json --python-worker C:\path\to\python.exe python/examples/worker_buy_once_strategy.py WorkerBuyOnceStrategy python/examples strategy-example-001 strategy-example-v1 $bundleHash
```

The bundle digest includes the declared strategy tree, the installed Follon SDK
source, and the Python implementation/version/platform. Third-party strategy
dependencies must be vendored under the declared bundle root. The control plane
verifies the worker's bundle hash, strategy identity, and version before
accepting a callback. Any protocol error, changed hash, or context-mismatched
intent fails the backtest before it can be treated as a decision artifact.

## PAPER operations status and kill switch

`follon-paper-status` opens a fail-closed PAPER-only journal, validates the
versioned configuration, and writes an immutable read-only dashboard snapshot:

```powershell
cargo run -p follon-cli --bin follon-paper-status -- var/follon-paper.journal.ndjson var/follon-paper-dashboard.json --config tests/fixtures/config/paper-v1.json
```

The command accepts no live environment, endpoint, or credential option. A
local operator can persist one independent emergency control in the same
durable journal while obtaining a refreshed dashboard:

```powershell
cargo run -p follon-cli --bin follon-paper-status -- var/follon-paper.journal.ndjson var/follon-paper-kill-active.json --config tests/fixtures/config/paper-v1.json --activate global
cargo run -p follon-cli --bin follon-paper-status -- var/follon-paper.journal.ndjson var/follon-paper-kill-cleared.json --config tests/fixtures/config/paper-v1.json --deactivate global
```

Supported scopes are `global`, `account:<canonical-id>`,
`strategy:<canonical-id>`, and `instrument:<canonical-id>`. Treat write access
to the journal and the local operator command as a privileged production
capability; the command is intentionally independent of broker availability.
Because a control action changes the dashboard, it requires an unused immutable
output path and refuses to mutate the journal if that evidence path already
exists.

## Deterministic operations workbench

`follon-operations` creates local evidence artifacts from an explicitly
versioned configuration and an explicit UTC projection time. It has no broker,
credential, order-control, wall-clock, or background-execution capability.

```powershell
cargo run -p follon-cli --bin follon-operations -- validate-config tests/fixtures/config/operations-v1.json
cargo run -p follon-cli --bin follon-operations -- dashboard tests/fixtures/config/operations-v1.json var/operations-dashboard.json --as-of 2026-08-10T16:30:00Z --journal var/follon-operations.journal.ndjson
cargo run -p follon-cli --bin follon-operations -- report tests/fixtures/config/operations-v1.json var/operations-report.md --as-of 2026-08-10T16:30:00Z --journal var/follon-operations.journal.ndjson
cargo run -p follon-cli --bin follon-operations -- schedule tests/fixtures/config/operations-v1.json var/operations-schedule.json --as-of 2026-08-10T16:30:00Z --journal var/follon-operations.journal.ndjson
```

The dashboard/report bind exact source configuration bytes, parameter-set,
strategy bundle, dataset, replay event, and selected-time identities. The only
stateful operation is an explicit non-secret journal append with a unique
idempotency key:

```powershell
cargo run -p follon-cli --bin follon-operations -- journal --journal var/follon-operations.journal.ndjson --entry-id journal.report.20260810 --event-type operations.report_generated.v1 --actor operator.alice --occurred-at 2026-08-10T16:30:00Z --detail report_hash=<sha256>
```

The journal is exclusive, fsynced, and SHA-256 chained. It is not a secret
store or a replacement for the controlled-live audit journal. See the
[operator workbench runbook](../../docs/operations/03-operator-workbench-runbook.md)
for the evidence procedure and the separate design-partner adoption gate.

## Deterministic options evidence

`follon-options` accepts a frozen, versioned **European** option-chain snapshot
and emits reproducible analytics/scenario/reconciliation evidence. It does not
connect to a broker or offer an order, exercise, or assignment action.

```powershell
cargo run -p follon-cli --bin follon-options -- validate-config tests/fixtures/config/options-v1.json
cargo run -p follon-cli --bin follon-options -- analyze tests/fixtures/config/options-v1.json var/options-dashboard.json
cargo run -p follon-cli --bin follon-options -- report tests/fixtures/config/options-v1.json var/options-report.md
```

The outer dashboard configuration identity is computed from the exact loaded
configuration bytes. Each BACKTEST, PAPER, and LIVE book must independently
supply its own strategy/data/config/replay/chain/model identity, source
account/export ID, normalized source-export hash, `as_of`, and currency; the
source hash is re-computed from the complete declared book before it is
accepted. That checks internal consistency of the declared export—it is not a
substitute for ingesting or verifying a raw broker export. Reconciliation
requires an explicit `reconciled_at` UTC instant and compares books without
overwriting a discrepancy. See
[Months 15–17 status](../../docs/06-delivery/10-months-15-17-status.md) for the
European-only boundary and the external broker-backed acceptance gate.

## Commercial controls, privacy, and signed releases

`follon-admin` is a local, evidence-only commercial-control CLI. It records
typed tenant provisioning and externally evidenced subscription facts in an
exclusive, fsynced, hash-chained ledger; it does not call a payment provider,
accept cards, authenticate customers, or handle raw customer identity. It also
produces hash-bound privacy/retention plans, performs an explicitly confirmed
single-file deletion, and builds/verifies Ed25519-signed release evidence.

```powershell
cargo run -p follon-cli --bin follon-admin -- provision tests/fixtures/config/commercial-provisioning-v1.json --ledger var/commercial.ndjson --event-id event.provision.acme.001 --actor operator.alice
cargo run -p follon-cli --bin follon-admin -- subscription tests/fixtures/config/commercial-subscription-v1.json --ledger var/commercial.ndjson --event-id event.subscription.acme.001 --actor billing.stripe --observed-at 2026-08-12T09:01:00Z
cargo run -p follon-cli --bin follon-admin -- entitlement tenant.acme --ledger var/commercial.ndjson --as-of 2026-08-12T10:00:00Z
```

Plans, receipts, manifests, signatures, trusted public-key records, and
self-host readiness outputs are immutable and idempotent only when their exact
bytes match. Release private keys must remain in a managed offline/CI signing
boundary and must never be placed in this repository, a container image, or a
self-host deployment. The [commercial/self-hosting runbook](../../docs/operations/04-commercial-self-hosting-runbook.md)
and [privacy/retention runbook](../../docs/operations/05-privacy-retention-runbook.md)
contain the required operating procedure and boundaries.
