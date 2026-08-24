# Workflow and order intent

## Canonical flow

```text
Market event → strategy → order intent → risk decision → OMS order
→ broker submission/simulation → execution → portfolio update → audit event → UI stream
```

An intent is a declarative request, not a broker command. Only the trading core may turn an approved intent into an OMS order. Strategy code never calls a broker adapter.

## Initial order-intent contract

The first schema must express:

- `intent_id`, `account_id`, `strategy_id`, `instrument_id`, and `correlation_id`.
- Side, quantity, order type, optional limit/stop price, and time-in-force.
- Strategy-provided rationale or signal reference.
- Creation time, strategy version, configuration version, and requested environment.
- Optional parent intent for brackets or baskets.

## Lifecycle ownership

| Stage | Owner | Output |
| --- | --- | --- |
| Signal | Strategy or user workflow | A versioned order intent |
| Validation | Pre-trade risk engine | Explicit approval or rejection with every applied policy |
| Lifecycle | OMS | Client and broker IDs, state transitions, replacement/cancel handling |
| Transmission | Broker adapter or simulator | Submission status and normalized broker messages |
| Accounting | Portfolio engine | Positions, cash, fees, P&L, and attribution changes |
| Evidence | Audit/replay subsystem | Immutable causal event chain |

## Initial OMS states

`CREATED`, `PENDING_RISK`, `RISK_REJECTED`, `APPROVED`, `PENDING_SUBMIT`, `SUBMITTED`, `ACKNOWLEDGED`, `PARTIALLY_FILLED`, `FILLED`, `PENDING_CANCEL`, `PENDING_REPLACE`, `CANCELLED`, `REJECTED`, `EXPIRED`, and `UNKNOWN`.

`UNKNOWN` is a safety state after ambiguous connectivity. It must lead to reconciliation, never an assumed failure.

## Broker evidence ordering and terminal invariants

Broker messages are evidence, not a request to rewrite the OMS. The OMS accepts
them in broker arrival order under these rules:

- An execution may arrive before an acknowledgement. Its broker order ID and
  execution ID establish the order as acknowledged, then apply the partial or
  full fill exactly once.
- `PENDING_CANCEL` and `PENDING_REPLACE` retain their pending meaning when a
  partial fill arrives. A full fill wins over either pending request and becomes
  `FILLED`; otherwise the broker's cancel, replacement, rejection, or expiry
  evidence resolves the pending state.
- A cancel rejection returns to `ACKNOWLEDGED` or `PARTIALLY_FILLED`, based on
  the independently tracked cumulative fill quantity. A partial order may then
  end as `CANCELLED`, `REJECTED`, or `EXPIRED` with its actual partial quantity
  retained.
- Replacements are price-only and risk-reducing. They retain one immutable OMS
  client ID and a chronological list of broker-native order IDs. Fills for any
  recognized version contribute to the same cumulative filled quantity; an
  unknown broker ID is rejected for reconciliation.
- `UNKNOWN` is resolved only by subsequent broker evidence or reconciliation.
  The durable PAPER journal and controlled-LIVE audit journal append each broker
  event application; they never overwrite the earlier `UNKNOWN` record.
- A `FILLED` order must have cumulative filled quantity exactly equal to the
  requested quantity. `CANCELLED`, `REJECTED`, and `EXPIRED` may retain partial
  fills but may not retain the complete requested quantity. A late new execution
  after one of those terminal states first creates a new `UNKNOWN` resolution
  step, then applies the authoritative execution. Other late terminal/status
  messages are retained as late evidence and do not replace the established
  terminal conclusion.
