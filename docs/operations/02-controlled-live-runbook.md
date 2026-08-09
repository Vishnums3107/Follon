# Controlled-live canary runbook

**Status:** implementation-era procedure. Completion of this runbook requires
named operational owners and external approval; it is not granted by this
repository.

## Non-negotiable entry criteria

1. The prior PAPER gate has retained its required clean sessions and all
   discrepancies have a reviewed resolution.
2. A release digest, strategy bundle digest, configuration digest, immutable
   instrument/calendar/data versions, risk policy version, and kill-switch
   version are recorded in the change ticket.
3. The small-capital LIVE account and independent `max_deployed_capital` ceiling
   are approved. Verify the account ID from the broker, not from a copied
   configuration file.
4. A managed secret provider has a least-privilege policy for the opaque
   reference. Test rotation in a non-production environment. Never paste a
   credential into configuration, shell history, journal, ticket, dashboard, or
   support channel.
5. The broker adapter version, gateway endpoint policy, reconnect behavior, and
   fault-injection results are reviewed and pinned. A status-only executable is
   not a broker adapter and cannot satisfy this criterion.
6. The journal directory and backup destination have restrictive ACLs; the
   backup restore and SHA-256-chain verification have been rehearsed.
7. Two distinct named operators have activation/approval roles. The execution
   operator cannot approve their own activation or order. MFA and authenticated
   identity are enforced at the control-plane boundary.

## Per-run sequence

1. Start in `SHADOW`. Record the activation interval and exact configuration
   fingerprint. Compare intended decisions with market data and broker
   observations; do not submit or connect from shadow mode.
2. Create a short `CANARY` activation only after the shadow review. Use a fresh
   immutable activation record; never edit an existing activation interval.
3. Verify that dashboard audit health is true, active kill switches are known,
   the prior journal head is preserved, and no unresolved incident exists.
4. Request the managed secret through the deployment-only provider. Record only
   the non-sensitive reference/access outcome. Establish the broker session and
   immediately synchronize and reconcile before any order.
5. For every order: create an exact intent hash, obtain a distinct approver’s
   single-use approval, verify fresh market data and limits, durably record the
   pending submission, then call the adapter once. `UNKNOWN` is not a failure
   result—stop and reconcile it before further entry work.
6. Reconcile after reconnects, broker notifications, and the session close. Do
   not overwrite internal state with broker state merely to make a report clean.
7. Count a session only after its explicit close and the latest clean
   reconciliation. Archive the configuration, activation, approvals, journal
   segment, dashboard snapshot, broker snapshot, and reconciliation report.

## Kill-switch and incident procedure

1. On an audit-write failure, unexpected position/cash/order result, stale
   market data, transport ambiguity, credential anomaly, or unavailable
   monitoring: activate the widest applicable kill switch immediately.
2. Preserve journal files and broker evidence read-only. Record the observed
   timestamps, account, client/broker IDs, dashboard audit head, and operator.
   Do not delete, rewrite, or replay an external request to “fix” the state.
3. Reconnect only through the approved adapter. Drain broker evidence, run an
   independent reconciliation, and create a durable incident for each
   discrepancy.
4. An accountable operator may add an explanation, but explanations do not
   alter the original discrepancy. The risk owner decides whether the canary
   remains paused. Resume requires a new activation where the original interval
   has expired or been revoked.

## Disaster recovery

1. Restore the journal to an isolated host, verify every schema version,
   sequence number, previous hash, entry hash, and configuration fingerprint.
2. Start with no assumed broker connection. Do not submit/cancel based on
   restored in-memory state alone.
3. Establish a new authenticated adapter session, synchronize all broker
   evidence, reconcile orders/positions/cash, and resolve every `UNKNOWN` or
   discrepancy before new work.
4. Record the restore actor, source backup identity, result, and newly observed
   journal head. Preserve the original source journal intact.

## Promotion gate

The next gate is eligible only after **60 distinct clean small-capital live
sessions**, zero unresolved accounting/reconciliation incidents, and continuous
complete auditability. An explained incident remains part of the historical
record and requires review; it does not erase a discrepancy. There is no clock
shortcut or simulation substitute for this evidence.
