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

## Local worker transport

The supported Months 3â€“5 worker is a versioned, line-delimited JSON process
protocol. It starts by announcing the SHA-256 hash of its complete declared
Python bundle, strategy identity, and strategy version. The control plane
checks that identity against the backtest specification before it sends the
first normalized bar. Each callback receives only immutable strategy context
and a normalized bar, and returns either one validated intent or `null`.

The worker process runs with a cleared environment. An operator may explicitly
provide the non-secret `FOLLON_STRATEGY_SDK_PATH` source location, which the
control plane maps to the worker's `PYTHONPATH`; otherwise deployment must use
a dedicated Python environment with `follon-strategy-sdk` installed. It has no
broker credential or adapter interface. The contract schema is
`contracts/json-schema/v1/strategy-worker-frame.schema.json`.

## Event-driven backtester

The backtester uses the same strategy API and event model as production. It must model point-in-time data, corporate actions, delistings, fees/charges, bid-ask spreads, configurable slippage, partial fills, order latency, session rules, market halts, borrow constraints, deterministic seeds, and portfolio-level capital constraints.

## Reproducibility record

Every completed backtest records the strategy bundle hash, versioned dataset, configuration version, seed, engine version, time range, instrument universe definition, and generated artifacts. A result without this record is exploratory output, not a reproducible decision artifact.

## Exit condition

For the first milestone, one example strategy runs repeatedly from a versioned dataset and configuration with identical results and event outputs.
