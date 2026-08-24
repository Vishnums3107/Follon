# Foundation threat model

**Status:** PAPER-control working model; review before connecting a real IBKR
paper gateway or any hosted deployment. It does not approve live trading.

## Assets

- Broker credentials and access tokens.
- Risk-policy, strategy-bundle, and configuration versions.
- Immutable trading/audit event history.
- Account, position, and P&L data.
- Desktop session credentials when identity is added.

## Trust boundaries

| Boundary | Threat | Baseline control |
| --- | --- | --- |
| Strategy worker ? trading core | Malicious or malformed intent | Versioned protobuf/JSON contracts, ingress validation, deterministic risk decision before OMS creation |
| Trading core ? paper broker adapter | Credential theft, endpoint escalation, or unaudited transmission | `PAPER` is required at the type/configuration boundary; the fixed-process IBKR bridge permits loopback paper ports only, uses bounded private pipes, and accepts no broker credential; strategy code never receives adapter access |
| Client ? control plane | Forged state transition or misleading environment | Client is projection-only; dashboard declares `PAPER`, validates a strict server-owned schema, and offers no state-changing control |
| Event store ? replay | Event deletion, mutation, or duplicate processing | Append-only canonical events, event-ID idempotency, causal links, deterministic replay comparison |
| Local developer machine | Secrets in code, fixtures, or logs | `.gitignore`, no credentials in the SDK, fixture review, and secret scanning required before broker work |
| Approval/control-plane ? live core | Self-approval, altered limits, replayed order approval | Time-bounded four-eyes activation and single-use approval bind to exact intent/configuration hashes; an authenticated identity/role service remains required before connection |
| Live core ? live adapter | Credential disclosure, unintended transmission, ambiguous network outcome | Opaque secret references and zeroizing secret material reach only the adapter boundary; durable pending state precedes submission and ambiguity is `UNKNOWN` until reconciliation |
| Audit journal ? recovery | Journal tampering, rollback, inherited stale broker session | Process-exclusive fsynced SHA-256 hash chain is verified on open; mismatch fails closed; recovery always starts disconnected and requires synchronization/reconciliation |

## First-slice attack and failure cases

1. A strategy fabricates an approved order. Mitigation: it can emit only an
   intent; `OmsOrder::from_approved_intent` requires a matching risk decision.
2. A caller replays an event or broker message. Mitigation: event stores reject
   duplicate immutable event IDs; paper OMS deduplicates execution IDs and its
   fault suite proves duplicate delivery is a no-op.
3. A policy is silently changed. Mitigation: each paper journal snapshot
   carries a SHA-256 configuration fingerprint and restart rejects a mismatch.
4. A UI implies a fill or P&L without evidence. Mitigation: the UI consumes the
   server-owned event trail and shows the actual causal payload.
5. A network loss is treated as a failed order/cancel. Mitigation: the OMS
   durably enters `UNKNOWN`, reconnects, drains evidence, then reconciles;
   it never blindly retries an ambiguous submit.
6. A simulator, PAPER, or controlled-live monitor result is misrepresented.
   Mitigation: the client is projection-only; strict dashboard schemas declare
   `PAPER` or `LIVE` plus mode, and the client has no credential or order-action
   surface.
7. A live operator approves their own order or reuses an approval. Mitigation:
   activation and order approvals reject matching requester/approver identities,
   bind exact hashes, expire, and record consumption before broker submission.
8. A reconciliation difference is concealed by replacing local accounting.
   Mitigation: live reconciliation never overwrites either source; it creates
   immutable incidents, and only an attributable explanation changes the
   unresolved-incident projection.

## Required before a real IBKR paper gateway is connected

- Review this model with the intended deployment model and legal/compliance
  boundary.
- Configure and independently review the fixed managed vault/OS-keychain helper
  with least privilege, rotation, access audit, no child processes, and no
  plaintext fallback; do not place credentials in configuration or journals.
- Add authenticated identity, role checks, rate limiting, and request
  idempotency at the public boundary.
- Add signed releases, SBOM generation, dependency and secret scanning, and an
  incident response procedure.
- Perform fault-injection tests for disconnect, duplicate, late, and conflicting
  broker messages against the pinned vendor transport and record its version.

This document does not approve live trading. It records the implementation
boundary for a deterministic non-live foundation.
