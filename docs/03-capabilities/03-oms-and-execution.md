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
- Sequential display-size Iceberg schedules with strict fixed-point conservation
  and elapsed refresh interval enforcement.
- Deterministic AlgoWheel allocation across bounded non-wheel algorithms using
  exact basis-point distribution and stable tie-breaking on schedule offsets.
- Passive limit execution with monotonic post-only cancel/replace, minimum
  intervals, maximum replacements, hard parent limits, and adverse chase caps.
- Capability-gated smart routing requiring verified venue capability records and
  refusing unknown venues, duplicate records, or unsupported order kinds before
  route decisions are emitted.
- Content-addressed execution plan evidence (`ExecutionPlanEvidence`) binding
  parent orders, scheduled children, route decisions, frozen benchmarks, and
  a SHA-256 plan fingerprint under tenant RLS persistence.
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

PAPER adapter composition is account-isolated. `PaperBrokerRegistry` binds a
canonical account to one reviewed adapter instance and venue metadata, then
selects that route only from the OMS request account. Submission, cancellation,
replacement, polling, snapshot, and reconnection are explicitly account scoped.
An unknown account route, a duplicate route, or an attempted cross-account
operation fails closed; it is never redirected to a default broker. The
registry is an OMS-side wiring mechanism, not a client capability, and does not
contain broker credentials. Its route order is deterministic, while evidence
within each account remains in normalized broker arrival order.

Multi-account cash and position reporting is a read-only, fixed-point
accounting projection. It accepts only source snapshots with one common
point-in-time and configuration fingerprint, retains each source reconciliation
identity, preserves native currencies/source-account attribution and the sum of
source marked values, and does not manufacture a blended mark or authorize
cross-account netting, allocation, or settlement.
