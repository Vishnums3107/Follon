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
