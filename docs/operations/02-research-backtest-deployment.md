# Non-live research backtest deployment

## Supported deployment boundary

The historical subsystem is deployable as a **non-live research and simulation service**.
It imports immutable historical data, runs isolated strategy workers, writes
reproducible artifacts, and maintains exact local accounting. It deliberately
has no live broker adapter, credential path, or live-trading flag. Its separate
PAPER-only control boundary is described in
[Months 6–8 status](../06-delivery/07-months-6-8-status.md); it is not a live
trading authorization.

## Required deployment controls

- Run the Rust CLI/control plane and every Python strategy worker as distinct,
  least-privileged operating-system identities.
- Install the SDK and strategy bundle into a dedicated virtual environment;
  invoke its Python executable by absolute path. The worker launch clears its
  inherited environment and exposes only stdio protocol frames. For local
  source deployments, `FOLLON_STRATEGY_SDK_PATH` is the sole explicit,
  non-secret environment value accepted by the launcher.
- Store source trade, bar, and action files in immutable versioned locations.
  Preserve the source files separately from their normalized dataset manifest.
- Retain every JSON artifact, NDJSON event stream, Markdown report, completion
  manifest, exact configuration file, and experiment catalog under write-once
  or versioned object storage. The storage adapter refuses conflicting
  overwrites, verifies remote objects after publication, and verifies recovered
  content. Production still requires reviewed encryption/key custody,
  retention/object lock, replication, monitoring, and restore drills.
- Back up the local experiment catalog and artifacts, then rehearse restoring
  them and rerunning a known specification. Compare the artifact fingerprint,
  event-output hash, and report bytes.
- Monitor worker exits and protocol failures as failed research runs. Never
  substitute an intent, inferred fill, missing price, or missing action.

## Acceptance check

For a candidate release, run both `follon-build-bars` and `follon-backtest`
twice into fresh paths. Bars, JSON artifacts, NDJSON event streams, Markdown
reports, completion manifests, and experiment records must be byte-identical.
Verify every manifest digest. Re-running one path must be idempotent; attempting
to reuse it for a different input must fail rather than overwrite evidence.

Publish canonical bars to immutable Parquet, register them in DuckDB, publish a
backtest artifact to the versioned S3-compatible store, and recover it to a new
path. Confirm the original, object metadata, and recovered SHA-256 values are
identical. The exact local commands are documented in
[`python/storage-adapter/README.md`](../../python/storage-adapter/README.md).

## Known first-release capability boundaries

The default `BacktestRunner` artifact path preserves its v1 single-currency,
long-only ledger and rejects duplicate executions/actions, out-of-session bars,
and inputs outside the declared manifest. Replay fails closed at an
instrument's effective end and supports venue/instrument halts,
full-spread/half-spread pricing, adverse slippage, bar latency, and per-bar
partial fills.

The advanced account is separately implemented and tested for point-in-time
universe membership, long/short crossings, explicit borrow availability and
recalls, cash-debit/short-borrow financing, attributed commission/exchange/
regulatory charges, fresh FX, initial-margin capital rejection, and terminal
delisting settlement. Until a public runner configuration selects that account
and records its terms in the immutable artifact, a v1 CLI result is not evidence
that those advanced controls ran. Multi-account allocation and production-size
performance evidence remain outside the current runner.
