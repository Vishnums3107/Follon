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

`CREATED`, `PENDING_RISK`, `RISK_REJECTED`, `APPROVED`, `PENDING_SUBMIT`, `SUBMITTED`, `ACKNOWLEDGED`, `PARTIALLY_FILLED`, `FILLED`, `PENDING_CANCEL`, `CANCELLED`, `REJECTED`, `EXPIRED`, and `UNKNOWN`.

`UNKNOWN` is a safety state after ambiguous connectivity. It must lead to reconciliation, never an assumed failure.
