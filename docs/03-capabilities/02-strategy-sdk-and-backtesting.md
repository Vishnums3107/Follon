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

## Event-driven backtester

The backtester uses the same strategy API and event model as production. It must model point-in-time data, corporate actions, delistings, fees/charges, bid-ask spreads, configurable slippage, partial fills, order latency, session rules, market halts, borrow constraints, deterministic seeds, and portfolio-level capital constraints.

## Reproducibility record

Every completed backtest records the strategy bundle hash, versioned and
content-addressed dataset, exact configuration ID/version/content hash, seed,
engine version, time range, instrument universe definition, and generated
artifacts. The portable artifact embeds this complete specification, and a
completion manifest binds each output file by SHA-256. A result without this
record is exploratory output, not a reproducible decision artifact.

Every CLI replay additionally produces a hashed advanced-account JSON and
Markdown sidecar. Explicit `advanced_account` configuration supplies FX,
margin, borrow, financing, and terminal-lifecycle economics. Older v1
configuration files receive a deterministic, fully-paid cash-account profile
from their own immutable account and instrument data: 100% margin, no inferred
FX or borrow, and no fabricated financing or lifecycle events. The CLI refuses
to publish a completed result if that advanced projection fails a capital or
lifecycle validation.

## Exit condition

For the first milestone, one example strategy runs repeatedly from a versioned dataset and configuration with identical results and event outputs.
