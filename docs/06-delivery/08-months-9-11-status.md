# Months 9–11 controlled-live implementation status

**Status:** controlled-live safety kernel and monitoring contracts implemented.
This is not evidence of a connected broker account or of the 60-live-day gate.
No credential provider, authenticated approval API, or broker wire-protocol
client is checked in.

## Implemented safety controls

| Requirement | Implementation | Status |
| --- | --- | --- |
| Explicit LIVE boundary | `LiveAccount` accepts only literal `LIVE`, an opaque managed credential reference, an opening cash value, and a lower independent deployed-capital ceiling | Implemented and tested |
| Secret containment | Secret bytes are represented by non-debuggable, zeroizing `SecretMaterial`; only the `LiveBrokerAdapter` connection boundary can receive them. Configuration, journal, dashboard, and error contracts accept only `SecretReference`. | Implemented; managed-vault/keychain implementation remains deployment work |
| Four-eyes activation and approvals | A `LiveActivation` binds a shadow or canary window to the exact account, risk policy, and kill-switch revision. Every canary order requires an unexpired, single-use approval for the exact intent hash, recorded by its distinct approving operator. | Implemented and tested |
| Shadow / canary separation | Shadow accepts only `SHADOW` intents and cannot reach an adapter. Canary accepts only matching `LIVE` intents after activation, managed-secret connection, pre-trade checks, and exact approval. | Implemented and tested |
| Canary limits and kill switches | Canary-specific notional/count limits supplement quantity, notional, open-order, long-only position, realized-loss, fresh-market, available-cash, reserved-cash, and deployed-capital limits. Global/account/strategy/instrument kill switches reject new work. | Implemented and tested |
| Irreversible-action audit | An exclusive, fsynced NDJSON journal forms a SHA-256 chain. A pending submission is durable before the broker call; ambiguous results are `UNKNOWN`; bad chain/state/configuration recovery fails closed. Restart records a new audit event and never inherits a broker session. | Implemented and tested |
| Reconciliation, incident, DR | Independent broker orders, fills, positions, and cash are compared without overwrite. Differences become durable incidents; only accountable explanations clear the unresolved count. Reconnect requires evidence drain and reconciliation. | Implemented and tested |
| Promotion evidence | A day counts only when an explicit closed session has the latest clean post-close reconciliation, no unresolved incident, and durable audit evidence. Days are immutable by exchange date. | Gate mechanism implemented; observed evidence is **0/60** |
| Monitoring | Strict `live-monitoring-dashboard` schema, immutable `follon-live-status` snapshots, and the desktop read-only display expose audit head, session state, reconciliation, incidents, positions, and 60-day gate state. | Implemented and typechecked |

## Deliberate deployment boundary

`follon-live-status` is deliberately non-trading. Its local adapter rejects
connect, submit, cancel, poll, snapshot, and reconnect operations. It can make
audit-backed monitoring evidence but cannot access credentials or a broker.

Before a small-capital live canary, an independently reviewed deployment must
provide all of the following:

1. A managed vault or OS-keychain `SecretProvider`, scoped to one deployment
   identity, with rotation, access logs, and no plaintext fallback.
2. A pinned, independently tested `LiveBrokerAdapter` for the approved broker
   endpoint. It must preserve OMS client IDs, normalize broker messages, and
   make no automatic retry after an ambiguous submit or cancel.
3. An authenticated control-plane service that creates activations and
   approvals with immutable requester/approver identity, roles, MFA, and
   tamper-evident central audit retention.
4. Network isolation, filesystem ACLs, monitored backup/restore, alert routing,
   incident on-call ownership, and a rehearsed kill-switch procedure.
5. Legal, broker, compliance, and operational sign-off for the exact account,
   capital ceiling, strategy bundle, configuration fingerprint, and calendar.

The deterministic tests prove the safeguards, not 60 actual live sessions.
That operational gate begins at zero and must retain every activation,
approval, journal segment, broker snapshot, reconciliation report, and
incident record.

See the [controlled-live runbook](../operations/02-controlled-live-runbook.md)
for the release and incident sequence.
