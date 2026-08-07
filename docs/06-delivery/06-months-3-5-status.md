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
| Reproducibility record | Strategy bundle hash, dataset/reference version/hash, exact configuration ID/version/hash, seed, engine version, time range, and universe | Rust and Python use the same v2 canonical fingerprint format |
| Exact accounting | Cash, cost basis, realized/unrealized P&L, fees, splits, dividends, immutable accounting entries | Single-currency/long-only capability implemented and rejects duplicate fills/actions |
| Result artifacts and reports | Embedded complete specification, input/output/report fingerprints, canonical events, equity curve, metrics, JSON/Markdown reports, atomic completion manifest | Immutable CLI output and runner tests implemented |
| Experiment metadata | Idempotent immutable records, durable NDJSON catalog, tag search, NDJSON export | `FileExperimentStore` implemented and tested |

## Explicit boundaries beyond this gate

- Add point-in-time data controls for delistings, halts, spreads, latency,
  partial fills, borrow constraints, and portfolio capital allocation.
- Persist raw datasets and backtest artifacts to the documented Parquet/DuckDB
  and object-storage adapters.
- Add a remote worker transport only if deployment later requires a network
  boundary; the existing protobuf service remains its compatibility target.
- Complete the legal, operational, and broker-specific gates before any paper
  or live execution work. They are outside this non-live milestone.

The currently implemented ledger intentionally rejects short positions and
cross-currency accounting. Those are explicit capability boundaries, not
silent approximations.
