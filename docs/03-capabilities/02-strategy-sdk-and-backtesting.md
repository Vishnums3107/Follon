# Strategy SDK and backtesting

## SDK contract

The Python SDK may:

- Subscribe to normalized market events.
- Request approved historical data.
- Define indicators and features.
- Read portfolio state.
- Emit structured logs and metrics.
- Persist strategy state.
- Submit order intents and receive fills and risk decisions.

It may not access a broker adapter or credentials.

`StrategyServices` provides the implemented bounded objects: frozen
point-in-time historical bars, deterministic SMA/EMA helpers, immutable
portfolio/cash snapshots, a 64 KiB canonical JSON state store with SHA-256
fingerprint, and a bounded structured-metrics sink. The objects expose no
broker, credential, socket, filesystem path, wall clock, or mutable platform
portfolio.

## Local worker transport

The supported Months 3â€“5 worker is a versioned, line-delimited JSON process
protocol. It starts by announcing the SHA-256 hash of its declared Python
bundle, installed SDK source, and Python runtime identity, plus the strategy
identity and strategy version. Third-party dependencies must be vendored into
the declared strategy tree. The control plane
checks that identity against the backtest specification before it sends the
first normalized bar. Each callback receives only immutable strategy context
and a normalized bar, and returns either one validated intent or `null`.

The v1 stdio protocol remains backward compatible with the minimal bar/context
frame and also accepts a strict `services` snapshot containing point-in-time
history, portfolio/cash, and host-owned state. The Rust replay host selects
that richer frame for every Python backtest worker, advances its bounded
history and portfolio projection on replayed fills, and rejects tampered state
fingerprints, future-dated metrics, malformed metrics, or protocol drift.
Workers return the updated canonical state/fingerprint and structured metrics
beside the nullable intent. Fill/risk-decision callbacks directly into Python
and a deployed remote gRPC worker remain separate integration work.

The worker process runs with a cleared environment. An operator may explicitly
provide the non-secret `FOLLON_STRATEGY_SDK_PATH` source location, which the
control plane maps to the worker's `PYTHONPATH`; otherwise deployment must use
a dedicated Python environment with `follon-strategy-sdk` installed. It has no
broker credential or adapter interface. The contract schema is
`contracts/json-schema/v1/strategy-worker-frame.schema.json`.

A worker is untrusted code behind a pipe, so the host bounds it. A frame is one
newline-terminated line of at most 16 MiB, refused before it is buffered in
full. Each callback has a 60-second deadline that also covers the write of its
request, so a worker that stops reading cannot hang a replay. These are the
defaults of `StrategyWorkerLimits`. An oversized or cut-off frame, a closed pipe
or an expired deadline is a transport fault: the worker's process is ended and
it is never asked another question, because no later answer could be matched to
a request. The deadline only ends a replay with an error; time never enters a
result.

## Event-driven backtester

The backtester uses the same strategy API and event model as production. It must model point-in-time data, corporate actions, delistings, fees/charges, bid-ask spreads, configurable slippage, partial fills, order latency, session rules, market halts, borrow constraints, deterministic seeds, and portfolio-level capital constraints.

## P&L conventions

The backtest crate holds two accounts, and they show fees in different places. Read
a P&L figure with its convention in mind.

| Figure | `BacktestLedger` (every replay's primary account) | `AdvancedBacktestAccount` |
| --- | --- | --- |
| Cost basis | Includes the buy fee (`average_cost`) | Excludes fees (`average_price`) |
| Realized P&L | **Net of every fee**: the buy fee through the basis, the sell fee deducted | **Trading P&L before charges** |
| Unrealized P&L | Net of the buy fee already paid | Before charges |
| Fees | `total_fees`, informational | `execution_charges`, and `financing_charges` for financing, each reported beside P&L |
| FIFO tax-lot P&L | Net of fees | Net of fees |

The two must agree on everything that is money. The same fills leave the same cash
and the same equity, and the difference between the two realized (or unrealized)
figures is exactly the fees. Buying 10 at 100 with a fee of 1.5, then selling 10 at
110 with a fee of 2, is a trading gain of 100 and fees of 3.5. The primary ledger
reports realized P&L of 96.5, and the advanced account reports 100 with charges of 3.5.
Both report cash of 10,096.5, and both report a FIFO tax P&L of 96.5, because each puts
fees in its lot cost. `core/backtest/tests/pnl_conventions.rs` holds every one of these
statements, and a change to either account that moves a fee to the other side fails it.

## Reproducibility record

Every completed backtest records the strategy bundle hash, versioned and
content-addressed dataset, exact configuration ID/version/content hash, seed,
engine version, time range, instrument universe definition, and generated
artifacts. The portable artifact embeds this complete specification, and a
completion manifest binds each output file by SHA-256. A result without this
record is exploratory output, not a reproducible decision artifact.

Every CLI replay also derives the advanced-account economics from its own
event stream and carries them inside the main artifact, as artifact schema
version 3. The artifact fingerprint and SHA-256 cover them, and the Markdown
report gains an "Advanced account" section. There is no separate sidecar to
read. Explicit `advanced_account` configuration supplies FX,
margin, borrow, financing, and terminal-lifecycle economics. Older v1
configuration files receive a deterministic, fully-paid cash-account profile
from their own immutable account and instrument data: 100% margin, no inferred
FX or borrow, and no fabricated financing or lifecycle events. The CLI refuses
to publish a completed result if that advanced projection fails a capital or
lifecycle validation.

## Exit condition

For the first milestone, one example strategy runs repeatedly from a versioned dataset and configuration with identical results and event outputs.
