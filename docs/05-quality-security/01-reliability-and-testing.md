# Reliability and testing

## Non-negotiable invariants

- Accepted intents resolve to a terminal state or explicit `UNKNOWN`.
- Duplicate broker messages do not duplicate fills.
- Restart cannot silently lose working orders.
- Internal positions reconcile continuously with broker positions.
- Strategies cannot bypass risk.
- Stale data stops affected strategies.
- Configuration is versioned and attributable.
- Live builds are reproducible from source control.
- UTC timestamps retain exchange-local context.
- Kill switches work without strategy processes.
- Broker disconnect has a documented safe state.

## Test pyramid

| Test type | Focus |
| --- | --- |
| Unit | Pure domain calculations and rules |
| Property-based | OMS transitions, fill aggregation, accounting, currency, risk bounds, serialization, idempotency |
| Model-based state machine | Reordered, duplicate, late, and conflicting order/broker event sequences |
| Deterministic replay | Identical inputs/configuration produce identical outputs |
| Simulation | Synthetic exchange/broker delays, packet loss, partial fills, halts, rate limits, clock faults, auth expiry |
| Integration | Real adapter boundary and persistence behaviour |
| Fault injection | Recovery during disconnects, restart, replacement, and degraded dependencies |

## Deployment gates

Promote strategy operation in order: replay → historical simulation → paper → shadow → minimum-size live → restricted capital → normal allocation. Paper stability requires 30 trading days without unexplained order or position discrepancies; controlled live requires 60 days with complete auditability and no unresolved accounting discrepancies.
