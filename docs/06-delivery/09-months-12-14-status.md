# Months 12–14 operator-workbench implementation status

**Status:** deterministic, read-only operations-workbench primitives and local operator workflows are implemented and tested. This is not evidence that five design partners have completed normal work unaided. Observed partner evidence: **0/5**.

## Implemented capabilities

| Roadmap capability | Implementation | Status |
| --- | --- | --- |
| Risk cockpit | Fixed-point marked equity, gross/single-instrument exposure, drawdown, open-order, unknown-order, and reconciliation-limit projection with visible current/limit/breach state | Implemented and tested |
| Attribution | Immutable, uniquely identified accounting movements grouped by instrument and recognized P&L category; exact net and category totals | Implemented and tested |
| Journal | Process-exclusive, fsynced NDJSON journal with schema validation, idempotency keys, SHA-256 predecessor chain, duplicate rejection, size bound, stable shared-lock reads, and read-only verification | Implemented and tested |
| Alerts | Stable alert fingerprints for audit/reconciliation failure, broker disconnect, active kill switches, hard risk breaches, and due work | Implemented and tested |
| Scheduling | Explicit-time daily-UTC planner with no wall-clock access; only the typed v2 completion workflow can journal a completion, validates the configured due instant, and binds it to exact configuration/parameter fingerprints; older configuration records are safely out of scope | Implemented and tested |
| Parameter/config tools | Strict unknown-field rejecting configuration import, exact source-byte hash, bounded decimal parameter revisions with direct predecessor fingerprint linkage, and static fail-closed binding of all six cockpit risk limits to `TWO_PERSON` approval evidence under the built-in risk-limit policy | Implemented and tested |
| Replay UI | Strict operations-dashboard parser and read-only desktop view for risk, alerts, attribution, schedules, source identities, and journal cursor | Implemented and typechecked |
| Reports | Idempotently published JSON dashboard, schedule-plan JSON, and Markdown operations report from the same selected configuration and UTC `as_of` time | Implemented and tested |
| Model-risk and resilience governance | Typed, fsynced SHA-256-chained model promotion/demotion/hold records and fault-game-day outcomes with immutable evidence hashes; canonical read-only registers | Implemented and tested |
| End-of-day execution review | Immutable transaction-cost analysis over frozen arrival/target benchmarks, exact fills/fees/partial fills, and strategy/algorithm/order-type aggregation | Implemented and tested |
| Risk SLO measurement | Frozen-policy local evaluator benchmark with caller-selected warmup, count, threshold, input hash, and p99 timing artifact | Implemented and tested; not production availability evidence |

## Reproducibility boundary

Every dashboard and report is derived from an immutable configuration source hash, parameter-set fingerprint, strategy bundle hash, dataset hash, replay event hash, caller-supplied canonical UTC `as_of` instant, and verified journal cursor. The emitted projection fingerprint binds the journal health/sequence/head that can affect schedules and alerts. The operations binary does not consult a wall clock, broker, credential provider, order API, or background job executor. Repeating the same command with the same files and arguments produces byte-identical output; output paths are immutable and an identical repeat is idempotent.

The journal command is intentionally the exception: it is an explicit stateful append. It requires a caller-selected idempotency key, actor, event type, and UTC time, then fsyncs a hash-chained record. Journal details reject credential-like field names and multiline/oversized values. It is not a secret store.

## Local operator workflow

From the repository root, validate the included safe PAPER fixture and produce view artifacts with an explicit replay clock:

```powershell
cargo run -p follon-cli --bin follon-operations -- validate-config tests/fixtures/config/operations-v1.json
cargo run -p follon-cli --bin follon-operations -- dashboard tests/fixtures/config/operations-v1.json var/operations-dashboard.json --as-of 2026-08-10T16:30:00Z --journal var/follon-operations.journal.ndjson
cargo run -p follon-cli --bin follon-operations -- report tests/fixtures/config/operations-v1.json var/operations-report.md --as-of 2026-08-10T16:30:00Z --journal var/follon-operations.journal.ndjson
cargo run -p follon-cli --bin follon-operations -- schedule tests/fixtures/config/operations-v1.json var/operations-schedule.json --as-of 2026-08-10T16:30:00Z
cargo run -p follon-cli --bin follon-tca -- tests/fixtures/config/tca-v1.json var/tca.json
cargo run -p follon-cli --bin follon-risk-benchmark -- tests/fixtures/config/risk-benchmark-v1.json var/risk-benchmark.json
```

Compare parameter revisions before an approved rollout. The artifact includes
the before/after value, bounds, control classification, and approval evidence
for each change. A successor must name and fingerprint its exact direct
predecessor; a reused human revision label alone is rejected:

```powershell
cargo run -p follon-cli --bin follon-operations -- config-diff <previous-operations.json> <target-operations.json> var/parameter-changes.json
```

To record a non-secret operational fact, append it deliberately:

```powershell
cargo run -p follon-cli --bin follon-operations -- journal --journal var/follon-operations.journal.ndjson --entry-id journal.report.20260810 --event-type operations.report_generated.v1 --actor operator.alice --occurred-at 2026-08-10T16:30:00Z --detail report_hash=<sha256>
```

After a due procedure has actually completed in its approved system of record,
record its typed v2 configuration-bound completion. The command records the
configured `scheduled_for` instant and refuses pre-due completion. Re-running
the schedule view with that journal will not claim the same work is still due
for that configuration:

```powershell
cargo run -p follon-cli --bin follon-operations -- complete-schedule tests/fixtures/config/operations-v1.json --journal var/follon-operations.journal.ndjson --schedule-id schedule.reconcile --entry-id journal.schedule.reconcile.20260810 --actor operator.alice --occurred-at 2026-08-10T21:20:00Z
```

Record a completed model-risk decision or fault-injection exercise only after
the linked artifact has been independently retained. The typed commands reject
missing/invalid IDs, non-canonical time, unbounded text, unsupported outcomes,
and malformed SHA-256 evidence; use `model-risk-register` and
`game-day-register` to publish the verified read-only registers. The desktop
Execution Blotter renders TCA and local benchmark artifacts, while operations
governance registers remain available through the Journal/operations evidence
filter. None of these mechanisms create a broker, customer, or compliance
acceptance record.

Load `var/operations-dashboard.json` into the desktop shell after `npm run build` within `apps/desktop`. The UI only renders validated evidence; it has no parameter, schedule, approval, journal, or trading controls.

`TWO_PERSON` records in this local workbench are hash-bound evidence: they bind
the complete parameter subject and a compiled risk-limit policy, and cannot be
backdated after the selected `as_of` time. They are not authenticated human
authorizations. Before an operational promotion, verify signed approval
envelopes against a trusted identity/policy/revocation service and bind the
accepted policy into the actual order-risk evaluator.

## Design-partner gate remains external

The roadmap gate requires five actual design partners to complete their normal work unaided. Repository tests cannot substitute for that observation. Before marking the gate complete, retain participant consent, task definition, product version/configuration hash, start/end timestamps, unassisted completion result, and any support interaction for five separate partners. Suggested normal-work tasks are: load a cockpit, identify a risk breach or clear state, verify the strategy/data/config/replay identities, inspect attributable P&L, determine the next scheduled task, and export an immutable report.

No live broker connection, credential, order capability, or background scheduler is introduced by this phase. Those remain behind the controlled-live deployment boundary and require independent operational approval.
