# Months 3–5 implementation status

This reproducibility phase extends the deterministic replay foundation. Its
bounded non-live historical-research exit condition is implemented and locally
verified; it does not authorize paper or live broker connectivity.

## Implemented in this phase

| Requirement | Implementation | Status |
| --- | --- | --- |
| Normalized trade importer and bar builder | Strict v1 trade CSV, source IDs/sequences, canonical time/instrument ordering, deterministic `follon-build-bars` OHLCV command | Implemented and tested |
| Corporate actions | Strict v1 action CSV, split/dividend adjustments, action-addressed datasets and ledger entries | Implemented and tested |
| Versioned strategy SDK | Strategy + SDK + Python-runtime hashing, strict versioned stdio worker handshake, point-in-time history, deterministic indicators, portfolio snapshots, bounded state and metrics | Implemented and tested; rich services require an injecting host |
| Backtester | `BacktestRunner` drives the strategy/risk/OMS kernel through reference/session preconditions | Implemented and tested |
| Execution realism controls | Explicit full spread/half-spread pricing, adverse slippage, exact fees, limit-price protection, versioned halt intervals, bar latency, persistent working orders, and per-bar partial-fill caps | Implemented and tested 2026-08-17 |
| Reproducibility record | Strategy bundle hash, dataset/reference version/hash, exact configuration ID/version/hash, seed, engine version, time range, and universe | Rust and Python use the same v2 canonical fingerprint format |
| Exact accounting | Cash, cost basis, realized/unrealized P&L, attributed charges, splits, dividends, immutable entries, long/short crossings, borrow/financing, FX, margin and delistings | Default runner uses the legacy ledger; the separately tested advanced account implements the expanded economics |
| Result artifacts and reports | Embedded complete specification, input/output/report fingerprints, canonical events, equity curve, metrics, JSON/Markdown reports, atomic completion manifest | Immutable CLI output and runner tests implemented |
| Experiment metadata | Idempotent immutable records, durable NDJSON catalog, tag search, NDJSON export | `FileExperimentStore` implemented and tested |
| Decision-grade storage | Deterministic immutable Parquet, DuckDB hash/row validation, versioned S3-compatible publication and verified recovery, dashboard-indexed receipts | Implemented and tested locally 2026-08-17 |

## Explicit boundaries beyond this gate

- Effective-dated instruments fail closed after `effective_to`; explicit halt,
  spread, latency, partial-fill, point-in-time universe, and economic delisting
  controls are implemented.
- `AdvancedBacktestAccount` implements long/short crossings, explicit
  shortability and borrow availability, recall cover calculation, financing,
  multi-currency FX/margin valuation, and atomic initial-margin rejection. The
  existing `BacktestRunner` CLI artifact flow has not yet been migrated to that
  account, so advanced controls must not be inferred from a legacy artifact.
- Multi-account portfolio allocation remains outside the supported runner and
  is not silently approximated.
- Configure production object retention/lock, KMS/key custody, replication,
  backup restoration, monitoring, and ownership for the implemented
  Parquet/DuckDB and S3-compatible adapters.
- Add a remote worker transport only if deployment later requires a network
  boundary; the existing protobuf service remains its compatibility target.
- Complete the legal, operational, and broker-specific gates before any paper
  or live execution work. They are outside this non-live milestone.

The default runner's ledger intentionally remains single-currency and
long-only for backward-compatible v1 artifacts. The advanced account is an
explicitly selected, independently tested capability rather than a silent
change to historical artifact semantics.

The ordered continuation is maintained in the
[step-by-step implementation matrix](13-step-by-step-implementation-matrix.md).
