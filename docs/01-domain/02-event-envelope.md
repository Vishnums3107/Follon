# Event envelope

Every significant trading operation is recorded as an immutable event. The envelope is the stable compatibility boundary for replay, audit, integration, and streaming.

## Required envelope fields

| Field | Requirement |
| --- | --- |
| `event_id` | Globally unique immutable ID |
| `event_type` | Namespaced semantic type, for example `market.bar.v1` |
| `schema_version` | Version of the payload contract |
| `event_time` | Time at the source or logical business time, stored in UTC |
| `receive_time` | Time Follon received or generated the event, stored in UTC |
| `account_id`, `strategy_id`, `instrument_id` | Canonical IDs when applicable; absence must be explicit |
| `correlation_id` | Groups one workflow, such as intent through fill |
| `causation_id` | Direct upstream event that caused this event, if any |
| `actor` | User, strategy, service, or system identity responsible |
| `source` | Origin such as IBKR, simulator, user, or trading core |
| `payload` | Validated event-specific data |
| `software_version`, `configuration_version` | Immutable identifiers used for reproducibility |

## Invariants

- An event is append-only; corrections are new events that reference the earlier event.
- Times are never overwritten and must be precision-preserving.
- Payloads must be schema validated at ingress.
- Consumers must be idempotent by `event_id`.
- Every event type needs a documented compatibility policy before it is published.

## First event families

`market.*`, `strategy.*`, `intent.*`, `risk.*`, `order.*`, `execution.*`, `portfolio.*`, `audit.*`, and `system.*`.
