# Order management and execution

## OMS responsibilities

The order-management system owns client IDs, broker IDs, the lifecycle state machine, parent-child relationships, cancels/replaces, partial fills, rejections, time-in-force, duplicate-event handling, broker reconnection, restart recovery, and end-of-day reconciliation.

## Implemented PAPER execution boundary

- A versioned `PAPER` account and risk policy are validated before any OMS
  state exists.
- The OMS records `PENDING_SUBMIT` durably before adapter submission, uses its
  generated order ID as the client idempotency key, and enters `UNKNOWN` on an
  ambiguous submit or cancel.
- Fresh instrument-matched market data is required at risk time. Working buy
  orders reserve cash, preventing a second approval from overcommitting it.
- The local IBKR paper model and the gateway adapter contract accept only the
  PAPER environment. The gateway configuration is restricted to loopback TWS
  paper port 7497 or Gateway paper port 4002.
- The concrete process transport starts only a fixed absolute executable,
  exchanges bounded correlated protocol messages over private pipes, and marks
  the session unhealthy after timeout, broken output, or protocol mismatch.

## Implemented broker-neutral execution planning

- Immediate market or price-protected limit children.
- Exact fixed-point TWAP, forecast-volume VWAP, POV/participation, and
  urgency-weighted arrival-price schedules with quantity conservation.
- Passive limit execution with monotonic post-only cancel/replace, minimum
  intervals, maximum replacements, hard parent limits, and adverse chase caps.
- Fee/latency/price-aware multi-venue routing.
- Bracket/stop-limit children, monotonic trailing stops, and portfolio-sized
  baskets.
- Ratio-bound, net-debit/net-credit-protected option combinations. An adapter
  must support a native atomic combination or reject before transmitting any
  leg; the planner never authorizes legging risk.

These are deterministic planning contracts, not broker acceptance evidence.
The versioned gRPC API exposes scheduled execution algorithms through arrival
price, the full cancel-before-replace passive sequence, and synchronized
net-price-protected option-combination plans. An adapter must still map a combo
to a native atomic broker order or reject it before transmitting any leg.

## Safety requirements

- An accepted intent has a terminal state or an explicitly unresolved `UNKNOWN` state.
- Duplicate broker messages cannot create duplicate fills.
- A restart cannot silently discard working orders.
- A network interruption never proves that submission failed.
- Every state transition is validated, causal, and auditable.
- Client order IDs are generated before adapter submission and are idempotency keys where the broker supports them.

## Adapter contract

The broker adapter converts canonical submit/cancel/replace and account-data contracts to broker protocol operations, then normalizes acknowledgements, rejects, status, executions, positions, and balances back into canonical events. It does not decide risk policy or own portfolio truth.
