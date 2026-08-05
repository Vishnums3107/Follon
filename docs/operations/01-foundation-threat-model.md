# Foundation threat model

**Status:** initial working model ? review before broker connectivity or any
hosted deployment.

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
| Trading core ? broker adapter | Credential theft or unaudited transmission | No adapter exists in the first slice; future adapters receive a secret-provider interface only, never strategy code |
| Client ? control plane | Forged state transition or misleading environment | Client is projection-only; all state is event-derived and `SIMULATION` is displayed persistently |
| Event store ? replay | Event deletion, mutation, or duplicate processing | Append-only canonical events, event-ID idempotency, causal links, deterministic replay comparison |
| Local developer machine | Secrets in code, fixtures, or logs | `.gitignore`, no credentials in the SDK, fixture review, and secret scanning required before broker work |

## First-slice attack and failure cases

1. A strategy fabricates an approved order. Mitigation: it can emit only an
   intent; `OmsOrder::from_approved_intent` requires a matching risk decision.
2. A caller replays an event or broker message. Mitigation: event stores reject
   duplicate immutable event IDs; later broker adapters must apply the same rule
   to execution IDs.
3. A policy is silently changed. Mitigation: each decision and every event
   records a configuration and policy version.
4. A UI implies a fill or P&L without evidence. Mitigation: the UI consumes the
   server-owned event trail and shows the actual causal payload.
5. A simulator result is misrepresented as a live result. Mitigation: the
   initial client is simulation-only and has no broker or credential surface.

## Required before a broker adapter exists

- Review this model with the intended deployment model and legal/compliance
  boundary.
- Add managed/OS-keychain secret providers with rotation and access audit.
- Add authenticated identity, role checks, rate limiting, and request
  idempotency at the public boundary.
- Add signed releases, SBOM generation, dependency and secret scanning, and an
  incident response procedure.
- Perform fault-injection tests for disconnect, duplicate, late, and conflicting
  broker messages.

This document does not approve live trading. It records the implementation
boundary for a deterministic non-live foundation.
