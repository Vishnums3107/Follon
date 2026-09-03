---
name: follon-delivery
description: Implement, review, or investigate Follon's trading-system behavior while preserving deterministic evidence, versioned boundaries, and the replay-to-paper release scope. Use for changes across the Rust core, Python SDKs, contracts, services, desktop app, infrastructure, or operations.
---

# Follon delivery

Follon is a risk-first trading operating system. The active release is the US-equities replay-to-paper workflow; the repository is in non-connected controlled-live engineering. Later-phase code is technical evidence only, not permission to enable or market a live, commercial, or broker-backed capability.

## Establish the boundary

Before changing behavior, read the root `README.md`, the product charter, repository guide, and the documents closest to the affected contract. Treat the source code, versioned schemas/Protobuf, and tests as the implementation truth; update the documentation that describes intentional public behavior.

The non-negotiable flow is:

```text
market event -> strategy -> order intent -> risk decision -> OMS -> simulator/broker edge
-> execution -> portfolio update -> immutable audit event -> evidence UI
```

An intent is declarative. Strategies never call an adapter, access credentials, or bypass the risk kernel. Core/domain does not depend on delivery frameworks or adapters. The desktop functions as an active trading terminal.

## Preserve the evidence model

- Keep replay deterministic and attributable: identical data, configuration, strategy bundle, clock, ordering, and seed must yield identical results.
- Use canonical event ordering/timestamps and fixed-point domain values. Do not add hidden wall-clock behavior, implicit randomness, or lossy numeric conversion to trading paths.
- Make state uncertainty explicit. Duplicate or late broker evidence, restart, disconnect, and reconciliation scenarios must not create silent loss or duplicate fills; ambiguous state remains `UNKNOWN` until authoritative evidence resolves it.
- Version a contract before or with any cross-module behavior change. Update its producers, consumers, fixtures, compatibility notes, and tests in the same change.
- Preserve tenant isolation, least privilege, secret separation, idempotency, append-only evidence, and migration/recovery integrity.

## Route work deliberately

Use the workflow role that matches the task:

- `developer`: implements features, fixes, refactors, and required documentation.
- `tester`: writes/runs focused regression, replay, fault, and acceptance-evidence tests.
- `reviewer`: read-only review for correctness, safety, and missing coverage.
- `debugger`: reproduces and isolates failures before a fix is attempted.
- `release_engineer`: handles security, reliability, infrastructure, monitoring, and release evidence.
- `rd_analyst`: read-only external or architectural research that needs sources and an experiment proposal.

Use `developer` as the change owner. Run `tester` after an implementation or for a test-focused request, then use `reviewer` for independent read-only review. Use `debugger` before implementation when the failure is unclear, `release_engineer` for operational/security work, and `rd_analyst` when the decision needs research. Keep only one write owner for a coordinated change set.

## Implement and verify

Make the smallest change that satisfies the requested behavior. Add focused regression coverage at the ownership boundary, then run the narrowest meaningful checks before broad checks:

- Rust: `cargo fmt --check` and the relevant `cargo test -p <package>`; use `cargo test --workspace` for cross-workspace changes.
- Python: run the affected package's `python -m pytest` tests.
- Desktop: from `apps/desktop`, run `npm run typecheck` and `npm run test:evidence` when evidence behavior changes.
- Security/release work: run the affected `tests/security` or tool checks and report generated artifacts without committing credentials or environment secrets.

Finish with the behavior changed, contracts/invariants protected, validation actually run, and any external gate or operational evidence still required.
