# Operator workbench runbook

**Status:** local, evidence-only procedure. It does not authorize paper or live trading, configuration changes, approvals, or schedule execution.

## Inputs and trust boundary

Use only an approved, versioned `operations-configuration` file. The command hashes its exact UTF-8 bytes and rejects unknown fields, non-canonical IDs, invalid fixed-point decimals, invalid UTC timestamps, duplicate accounting entries, invalid parameter bounds, non-canonical parameter lineage, future approvals, and missing two-person approval evidence for risk-sensitive parameters. The six reserved cockpit risk parameters are compiled policy identifiers and must exactly equal their enforced limits.

Supply an authoritative `--as-of` UTC time for every view. Do not infer it from the workstation clock. A report can be reproduced only when the configuration file, command arguments, strategy/data/replay hashes, and journal segment are preserved together.

## Daily evidence sequence

1. Validate the selected configuration with `validate-config`; use `config-diff <previous> <target> <changes.json>` to compare the emitted configuration and parameter fingerprints with the approved change record.
2. Generate the dashboard with `dashboard ... --as-of <UTC> --journal <path>`. Preserve its projection fingerprint with the source configuration and journal cursor. If the journal is unhealthy, the dashboard emits a CRITICAL journal alert; treat its sequence and head as untrustworthy and investigate before relying on the dashboard.
3. Review `CRITICAL` alerts before any operational decision. A failed audit or reconciliation signal, active kill switch, or hard risk breach is not resolved by re-running a report.
4. Confirm attribution entries are accounted economic evidence, not an estimate copied from a display. Preserve the source ledger/backtest artifact that created them.
5. Generate the schedule plan and perform the named procedure through its approved system of record. This planner deliberately does not spawn a process. Only after completion, append `complete-schedule` with its unique evidence ID; the typed v2 record requires a due enabled schedule, records the configured due instant, and binds it to the exact configuration and parameter fingerprint. Prior configuration-revision completion records are intentionally out of scope.
6. Publish the immutable Markdown report. If the same path already has different bytes, stop: do not overwrite the existing evidence.
7. Record an explicit non-secret journal fact only after the associated work is complete. Use a unique idempotency key; preserve the resulting JSON line and journal head hash with the report.

## Execution cost, model-risk, and game-day evidence

Run `follon-tca <tca-v1.json> <new-tca.json>` after the session with frozen
arrival and target benchmarks, all fills, and exact explicit fees. It refuses
duplicate fill evidence, overfills, stale/non-positive benchmarks, malformed
identities, and incompatible accounting. Preserve the JSON, Markdown summary,
and manifest together; an implementation-shortfall result is a measurement, not
proof of acceptable trading quality.

Record an evidence-based strategy decision only after preserving the exact
strategy bundle and backtest artifacts:

```powershell
follon-operations model-risk-record --record-id <id> --actor <operator.id> `
  --occurred-at <UTC> --strategy-id <id> --strategy-version <version> `
  --strategy-bundle-hash <sha256> --backtest-artifact-hash <sha256> `
  --decision <PROMOTE|DEMOTE|HOLD> --change-summary <one-line-text> `
  --reason <one-line-text> --journal <journal.ndjson>
follon-operations model-risk-register <journal.ndjson> <new-register.json>
```

Run a fault-injection game day on the approved cadence; recover, reconcile, and
retain independent test evidence before writing the typed result:

```powershell
follon-operations game-day-record --record-id <id> --actor <operator.id> `
  --occurred-at <UTC> --scenario-id <id> --result <PASS|FAIL> `
  --fault-plan-hash <sha256> --evidence-hash <sha256> `
  --reconciliation-hash <sha256> --postmortem-summary <one-line-text> `
  --journal <journal.ndjson>
follon-operations game-day-register <journal.ndjson> <new-register.json>
```

Both typed records are hash-chained and validate their exact evidence shape.
They establish only that a declared, linked artifact was recorded; a human must
review the source evidence. Generate risk latency observations with
`follon-risk-benchmark <risk-benchmark-v1.json> <new-benchmark.json>` using a
frozen policy/snapshot/candidate. It measures only the local core on the
machine and build that ran it; it is not a 99.9% availability claim or a
production load test.

The workbench only validates hash-bound approval evidence. It does not verify
human identities, signatures, authorization policy ownership, or revocation;
do not treat the local `config-diff` artifact as an execution authorization.

## Failure handling

- A journal parse, hash, sequence, or predecessor failure is evidence of an integrity issue. Preserve the original file read-only, do not edit it, and investigate from a copied artifact.
- An operations dashboard is a projection. It must not be used to change an OMS state, clear a discrepancy, deactivate a kill switch, or approve a risk parameter.
- Never put a secret, credential reference, token, password, private key, or multiline diagnostic into journal detail fields. The local guard rejects many dangerous field names but cannot prove a value is safe.
- A due schedule is an operator reminder, not proof that its task ran. `complete-schedule` is a durable declaration after the approved process has completed; preserve the independent system-of-record evidence with its journal line.

## Desktop evidence view

The desktop shell accepts only schema-shaped dashboard snapshots, controlled-live/PAPER monitoring snapshots, or canonical NDJSON. It is a local renderer with no authenticated control channel. Review the source fingerprints and journal head alongside the cockpit instead of relying on screenshots.
