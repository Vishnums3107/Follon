# Months 3–5 implementation status

This is the active reproducibility phase from the roadmap. It extends the
completed deterministic replay foundation; it does not authorize paper or live
broker connectivity.

## Implemented in this phase

| Requirement | Implementation | Status |
| --- | --- | --- |
| Normalized trade importer and bar builder | Strict v1 trade CSV, source IDs/sequences, deterministic OHLCV builder | Implemented and tested |
| Corporate actions | Strict v1 action CSV, split/dividend adjustments, action-addressed datasets and ledger entries | Implemented and tested |
| Versioned strategy SDK | Deterministic Python bundle hashing and versioned stdio worker handshake | Implemented and tested |
| Backtester | `BacktestRunner` drives the strategy/risk/OMS kernel through reference/session preconditions | Implemented and tested |
| Reproducibility record | Strategy bundle hash, dataset/reference version, config, seed, engine version, time range, and universe | Rust and Python use the same canonical fingerprint format |
| Exact accounting | Cash, cost basis, realized/unrealized P&L, fees, splits, dividends, immutable accounting entries | Single-currency/long-only capability implemented and rejects duplicate fills/actions |
| Result artifacts and reports | Input/output/report fingerprints, canonical events, equity curve, metrics, JSON and Markdown reports | Immutable CLI output and runner tests implemented |
| Experiment metadata | Idempotent immutable records, durable NDJSON catalog, tag search, NDJSON export | `FileExperimentStore` implemented and tested |

## Still required for the Months 3–5 exit gate

- Add point-in-time data controls for delistings, halts, spreads, latency,
  partial fills, borrow constraints, and portfolio capital allocation.
- Persist raw datasets and backtest artifacts to the documented Parquet/DuckDB
  and object-storage adapters.
- Add a remote worker transport only after the local stdio protocol requires a
  network boundary; the existing protobuf service remains the compatibility
  target for that future deployment.
- Complete the legal, operational, and broker-specific gates before any paper
  or live execution work. They are outside this non-live milestone.

The currently implemented ledger intentionally rejects short positions and
cross-currency accounting. Those are explicit capability boundaries, not
silent approximations.
