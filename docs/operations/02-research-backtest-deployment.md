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
  or versioned object storage. The local file adapter atomically publishes
  immutable files and refuses conflicting overwrites, but is not a replacement
  for replicated object storage.
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

## Known first-release capability boundaries

Accounting is exact but deliberately limited to a single currency and long
positions. The runner rejects short positions, cross-currency accounting,
duplicate executions, duplicate corporate actions, out-of-session bars, and
input that does not match its declared dataset manifest. Delistings, halts,
borrow, latency, partial-fill, and spread models must be added before using a
result as evidence for a strategy that depends on those market conditions.
