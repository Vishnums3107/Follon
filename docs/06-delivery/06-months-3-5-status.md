# Months 3–5 implementation status

This reproducibility phase extends the deterministic replay foundation. Its
bounded non-live historical-research exit condition is implemented and locally
verified; it does not authorize paper or live broker connectivity.

## Implemented in this phase

| Requirement | Implementation | Status |
| --- | --- | --- |
| Normalized trade importer and bar builder | Strict v1 trade CSV, source IDs/sequences, canonical time/instrument ordering, deterministic `follon-build-bars` OHLCV command | Implemented and tested |
| Corporate actions | Strict v1 action CSV, split/dividend adjustments, action-addressed datasets and ledger entries | Implemented and tested |
| Versioned strategy SDK | Strategy + SDK + Python-runtime hashing, strict versioned stdio worker handshake | Implemented and tested |
| Backtester | `BacktestRunner` drives the strategy/risk/OMS kernel through reference/session preconditions | Implemented and tested |
| Execution realism controls | Explicit full spread/half-spread pricing, adverse slippage, exact fees, limit-price protection, versioned halt intervals, bar latency, persistent working orders, and per-bar partial-fill caps | Implemented and tested 2026-08-17 |
| Reproducibility record | Strategy bundle hash, dataset/reference version/hash, exact configuration ID/version/hash, seed, engine version, time range, and universe | Rust and Python use the same v2 canonical fingerprint format |
| Exact accounting | Cash, cost basis, realized/unrealized P&L, fees, splits, dividends, immutable accounting entries | Single-currency/long-only capability implemented and rejects duplicate fills/actions |
| Result artifacts and reports | Embedded complete specification, input/output/report fingerprints, canonical events, equity curve, metrics, JSON/Markdown reports, atomic completion manifest | Immutable CLI output and runner tests implemented |
| Experiment metadata | Idempotent immutable records, durable NDJSON catalog, tag search, NDJSON export | `FileExperimentStore` implemented and tested |
| Decision-grade storage | Deterministic immutable Parquet, DuckDB hash/row validation, versioned S3-compatible publication and verified recovery, dashboard-indexed receipts | Implemented and tested locally 2026-08-17 |

## Explicit boundaries beyond this gate

- Effective-dated instruments now fail closed after `effective_to`, and explicit
  halt, spread, latency, and partial-fill controls are implemented. Economic
  delisting settlement remains required for strategies that depend on it.
- Borrow constraints remain outside the current long-only ledger, which rejects
  short positions. Portfolio allocation remains outside the single-account
  scope and frozen by the active roadmap gate; neither is silently modeled.
- Configure production object retention/lock, KMS/key custody, replication,
  backup restoration, monitoring, and ownership for the implemented
  Parquet/DuckDB and S3-compatible adapters.
- Add a remote worker transport only if deployment later requires a network
  boundary; the existing protobuf service remains its compatibility target.
- Complete the legal, operational, and broker-specific gates before any paper
  or live execution work. They are outside this non-live milestone.

The currently implemented ledger intentionally rejects short positions and
cross-currency accounting. Those are explicit capability boundaries, not
silent approximations.

The ordered continuation is maintained in the
[step-by-step implementation matrix](13-step-by-step-implementation-matrix.md).
