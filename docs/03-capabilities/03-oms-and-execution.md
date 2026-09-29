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

The planning methods are deterministic contracts, not broker acceptance
evidence. The versioned gRPC API exposes scheduled execution algorithms through
arrival price, the full cancel-before-replace passive sequence, and synchronized
net-price-protected option-combination plans.

The same API also has a distinct `SubmitPaperCombo` command. It exists only
when `FOLLON_TRADING_API_PAPER_CONFIG` names a valid version-1 PAPER command
route. That route converts the protobuf request into the canonical
`ComboIntent`, requires one independently supplied observation per leg, and
calls `PaperTradingService::submit_combo_intent`; it cannot fall back to the
planning method. The configured service owns the durable journal, exact risk
policy, kill switches, adapter model, idempotency record, and optional explicit
short-exposure bound. With no configured route the RPC fails closed and no
planning response is presented as an order outcome. A configured command route
may bind to loopback without TLS; a non-loopback bind additionally requires a
server TLS identity and client CA, so the write method is not exposed on an
unauthenticated remote socket.

The route's `adapter_kind` chooses its broker adapter. `IBKR_PAPER_MODEL`, the
in-process deterministic model, fills nothing on its own. `IBKR_PAPER_BRIDGE`
starts the official-API IBKR PAPER bridge from the document's `ibkr_bridge`
section. That section is required for the bridge and refused for the model.
The route builds the bridge's argument list itself, from fixed fields: a
loopback host, a PAPER port, the client id, the broker account, the
instrument map, the TWS time zone and a timeout. So no free-form argument
reaches the process. It refuses what the bridge would refuse before any
process starts. The journal fingerprint binds the bridge endpoint, the broker
account, the client id, the time zone and the instrument map's SHA-256, so a
journal is refused under a different adapter or IBKR session.

The bridge carries single DAY orders only, so `SubmitPaperCombo` on a bridge
route is refused before risk is evaluated or an order exists. The route has
no single-order RPC yet (delivery state E5.2b), and it does not yet
synchronize fills from the bridge (E5.2c). A bridge route therefore trades
nothing today.

Every call also needs an authenticated operator. A configured route requires
the operator directory named by `FOLLON_TRADING_API_OPERATOR_DIRECTORY`.
`BeginOperatorLogin` and `CompleteOperatorLogin` check a password and then a
mandatory TOTP code, and return a bearer session. `SubmitPaperCombo` refuses
three kinds of caller before it reads the intent:

- a caller with no session, or a malformed one (`UNAUTHENTICATED`);
- a session whose role does not grant PAPER trading (`PERMISSION_DENIED`);
- a session presented for another tenant (`PERMISSION_DENIED`).

The directory serves one tenant. The PAPER journal records the operator as the
combination's `submitted_by`, and an idempotent retry must come from the same
operator. This is local PAPER engineering evidence, not external-broker or
production acceptance.

The same route exposes `ActivatePaperKillSwitch` and `ReleasePaperKillSwitch`
(E3.3b). Each names one switch by its stable key: `global`, `account:<id>`,
`strategy:<id>` or `instrument:<id>`. Each needs a session whose role grants
kill-switch operation, which only `risk_manager` does, so a trader's session is
refused (`PERMISSION_DENIED`). The PAPER journal records every change with its
operator and the server's UTC time. A repeat that changes nothing records
nothing. The local `follon-paper-status --activate/--deactivate` path is
unchanged and records no operator.

`ActivateLiveKillSwitch` and `ReleaseLiveKillSwitch` (E3.3c) apply the same
rule to controlled LIVE. They need a separate route, configured by
`FOLLON_TRADING_API_LIVE_CONFIG` and `FOLLON_TRADING_API_LIVE_JOURNAL`:

- The configuration is the version-1 document `follon-live-status` reads, and
  both parse it with the same code. The journal therefore opens only under the
  configuration fingerprint it was written with.
- The route opens the LIVE journal with an adapter that refuses every broker
  operation. It can halt controlled LIVE, but it can never place, cancel or
  reconcile an order.
- It refuses to start with only one of its two variables or without the
  operator directory. Off loopback, it requires server TLS and a client CA.
- The LIVE journal records each change with the operator as its actor and the
  server's UTC time. A repeated activation is recorded too, and a release that
  changes nothing records nothing.
- While the route runs, the API holds the LIVE journal's exclusive lock, so no
  other LIVE process can open that journal.

The Tauri desktop host has the equivalent `submit_combo_order` IPC command. Its
`ComboOrderIntent` places each leg's operator-attested observation on the leg
itself, so no leg can reach risk unpriced or priced from its limit. The native
PAPER gateway converts it into one `ComboIntent` and calls the same
`submit_combo_intent`; the in-process paper model fills the whole group
atomically only when the net price at those observations satisfies the
approved protection, sign included, and otherwise leaves it resting. Existing
`cancel_order` reaches combinations and its receipt reads both order maps. The
desktop's optional `short_exposure` bound lives only in the operator-authored
configuration file, never in the UI.

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
