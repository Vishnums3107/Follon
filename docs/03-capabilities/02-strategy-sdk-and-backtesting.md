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
defaults of `StrategyWorkerLimits`, and a caller that sets its own may choose a
frame limit from 4 KiB to 256 MiB, since a limit no machine could honour bounds
nothing. An oversized or cut-off frame, a closed pipe
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

## Corporate actions

A stock split reaches a replay in two books, and both follow it. The ledger scales its
position and its FIFO tax lots, and the replay engine scales the portfolio of its own that
a strategy's execution callback and the event stream project. Both use the same
arithmetic, quantity times the ratio and average cost divided by it, so the total cost
and the realized P&L do not move and the two books cannot drift apart.

The engine records each scaled position as a `portfolio.position_updated.v1` event of its
own: actor `portfolio_engine`, source `corporate_action`, correlation
`corr-corporate-action-<action id>`, and no cause, because no fill caused it. It is
recorded when the replay applies the split, the first bar at or after the split's
effective time, and before that bar's market event. A run with no split holds no such
event and is unchanged. A split applies to every account that holds the instrument or to
none, and each action applies once.

A split changes how many shares an account holds and what one is worth, and leaves what the
account is worth alone. The runner marks a position at the last bar of its instrument, so it
divides that mark by the ratio at the moment it applies the split. Until the instrument's next
bar, equity is what it was, and when no bar follows, the ending equity is right too. The
rebased mark is exact to the platform's eight places and is replaced by the next close.

The ledger's FIFO lots always total exactly what the position, scaled once, becomes. Scaling
lot by lot rounds each one down, so with a ratio such as a third or two thirds their total can
fall short by a unit of the last place. The shortfall goes on the newest lot, a lot that scales
to nothing is dropped, and a split that would round a lot's unit cost to nothing is refused.
A sale of the whole position is therefore never refused for want of lots.

A cash dividend is income, which only the ledger books. The engine's portfolio holds a
position and its trading P&L and no cash.

A strategy is told what an action did to its account through `Strategy::on_corporate_action`,
after the ledger and the engine have applied it: a split delivers the position as the engine
now holds it, and a dividend delivers the cash the ledger credited. An action that changed
nothing, because the account held none of the instrument or the credit rounded to no cash,
is not delivered. The strategy worker's host applies the effect to the services it builds the
portfolio snapshot from, so the next callback shows the post-split quantity, cost and mark
and the credited cash. The worker protocol and the Python SDK are unchanged: a strategy sees a
corporate action as a correct portfolio, never as a message.

A venue's response to a split, which adjusts or cancels a resting order, is not modelled.
An order resting in the instrument cannot be carried across the split, because its
quantity and limit are in pre-split units, so the replay refuses the run rather than fill
it at a price level it was not written for. That is the rule the replay already applies to
a lot-size change. Two things still do not follow a split: the risk policy's
share-denominated limits, which are the operator's configuration; and PAPER and controlled
LIVE, which apply no corporate actions at all.

Limits of the replay's split, each found in review and each a choice and not an oversight:

- A ratio that does not divide a position evenly leaves a fractional position, and the replay
  carries it exactly. An instrument whose lot size is larger than the smallest quantity cannot
  then sell it, because the risk policy refuses a quantity that is not a whole number of lots.
  A broker pays cash in lieu of the fraction, which this replay does not model.
- A reverse split that would round a held position to nothing is refused by the engine and ends
  the run, where the ledger alone would have dropped the position.
- Two actions with the same effective time apply in action-id order, and the order decides
  whether a dividend is paid on the shares before the split or after it. A dividend meant for
  the shares before the split needs an id that sorts before the split's.
- The engine applies a split to its portfolios and consumes the action before it records the
  position events. A sink that fails part-way leaves an engine that has applied it, and a runner
  can execute only once, so a failed run is over and is never resumed.

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
