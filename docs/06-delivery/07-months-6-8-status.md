# Months 6–8 implementation status

This is a PAPER-only operational-control milestone. It implements the core
controls and their deterministic verification; it does not claim that the
30-paper-trading-day gate has been observed, and it does not authorize live
trading.

## Implemented controls

| Requirement | Implementation | Status |
| --- | --- | --- |
| Paper OMS and lifecycle safety | `PaperTradingService` creates a durable `PENDING_SUBMIT` record before crossing the broker boundary; ambiguous outcomes become `UNKNOWN` and can only be resolved by evidence/reconciliation | Implemented and tested |
| Pre-trade risk | Versioned limits cover order quantity/notional, open orders, long-only position size, aggregate realized loss, fresh instrument-matched market data, available cash, and cash reserved for working buys | Implemented and tested |
| IBKR paper boundary | `IbkrPaperAdapter` is a deterministic paper-contract model; `adapters/brokers/ibkr` provides a loopback/PAPER-only gateway adapter contract for an audited TWS/Gateway transport | Implemented; real vendor transport remains an explicit deployment prerequisite |
| Reconciliation and accounting | Independent broker order/state/fill, position, and cash snapshots are compared without overwrite; every difference becomes a durable incident | Implemented and tested |
| Kill switches | Global, account, strategy, and instrument scopes reject new work independently of strategy or broker health; local CLI activation/deactivation is journaled | Implemented and tested |
| Restart/reconnect | A process-exclusive, append-only fsynced journal snapshots recover orders, positions, executions, risk evidence, incidents, session-gate records, and immutable configuration fingerprint; bounded recovery prevents unbounded reads, and any journal write failure halts later state-changing operations; reconnect drains evidence then reconciles | Implemented and tested |
| Fault injection | Disconnect, ambiguous-submit, and duplicate-event cases are deterministic and exercised in the broker wrapper tests | Implemented and tested |
| Dashboard | Strict v1 JSON schema, immutable status-snapshot CLI, and desktop read-only projection expose PAPER environment, fingerprint, orders, kill switches, reconciliation, positions, and gate state | Implemented and typechecked |
| 30-paper-day gate | The service records only closed explicit exchange sessions with a clean reconciliation and no unexplained incident; records are immutable per date | Gate mechanism implemented; observed evidence is currently **0/30** |

## Operational acceptance sequence

1. Pin and review a real `IbkrPaperGatewayTransport` implementation against the
   configured local paper TWS/Gateway endpoint. Record the client and gateway
   versions; do not introduce a live endpoint or live credentials.
2. Run the PAPER service with a filesystem ACL that restricts the durable
   journal and the kill-switch command to the dedicated operator identity.
   Copy journal snapshots to immutable/versioned storage and rehearse restore.
3. After every reconnect and after each session close, obtain the independent
   broker snapshot and reconcile. Investigate or explicitly explain every
   incident; do not overwrite local state to make a report clean.
4. Feed the exact closed sessions selected from the versioned exchange calendar
   into the gate tracker. It becomes eligible only after 30 distinct clean
   sessions and zero unexplained incidents. Explainable incidents still need
   review; they do not silently alter the underlying reconciliation history.

The deterministic local model and acceptance tests demonstrate recovery logic;
they are not evidence of 30 real broker paper sessions. The first real paper
run must begin at zero and preserve its journal, calendar, configuration
fingerprint, gateway version, and every reconciliation result as evidence.

## Explicit boundary

This repository does not contain a production TWS/Gateway wire-protocol client,
secret provider, authenticated control-plane API, or any live broker route.
Those are required before a connected paper deployment and are mandatory before
the Months 9–11 controlled-live gate. No current command accepts `LIVE`.
