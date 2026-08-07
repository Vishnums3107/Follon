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

## Planned execution methods

- Immediate market and limit order submission.
- Passive limit order.
- Bracket orders.
- Time-weighted and participation-limited execution.
- Bounded cancel-and-replace and price chasing.
- Basket submission only after portfolio checks.

VWAP/POV, arrival-price logic, multi-venue routing, and options-combination execution are later capabilities.

## Safety requirements

- An accepted intent has a terminal state or an explicitly unresolved `UNKNOWN` state.
- Duplicate broker messages cannot create duplicate fills.
- A restart cannot silently discard working orders.
- A network interruption never proves that submission failed.
- Every state transition is validated, causal, and auditable.
- Client order IDs are generated before adapter submission and are idempotency keys where the broker supports them.

## Adapter contract

The broker adapter converts canonical submit/cancel/replace and account-data contracts to broker protocol operations, then normalizes acknowledgements, rejects, status, executions, positions, and balances back into canonical events. It does not decide risk policy or own portfolio truth.
