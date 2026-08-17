# Penetration-test runbook

This is an authorization and evidence procedure, not a claim that a penetration
test has been completed. Test only assets for which the organization has written
authorization, a named owner, a time window, contacts, safe-harbor terms, and a
defined stop condition. Do not test broker endpoints, payment providers,
customer production data, or shared infrastructure without their separate
written authorization.

## Scope to assess

- Hosted authentication/authorization and tenant isolation once those services
  are deployed. The current repository has no hosted identity implementation.
- Commercial-ledger append authorization and operational access to provider
  evidence; confirm no payment card or raw customer identity reaches the ledger.
- Self-hosted deployment host, reverse proxy/TLS, volume permissions,
  `managed_command` secret helper, trusted-key distribution, image provenance,
  release-manifest/signature verification, and backup/restore path.
- Privacy inventory and retention executor: path traversal, symlink handling,
  hash/plan substitution, legal-hold bypass, concurrent-writer behavior, and
  immutable evidence overwrite attempts.
- Existing control-plane, paper, live, desktop, SDK, and IBKR bridge boundaries
  in the [foundation threat model](01-foundation-threat-model.md).

## Required test cases

| Area | Required result |
| --- | --- |
| Tenant ledger | Unprivileged users cannot append, replace, truncate, or replay an event ID; a modified record fails chain verification. |
| Entitlement | Expired, suspended, cancelled, missing, or mismatched-plan subscription evidence denies mutable access. |
| Release chain | Modified artifact, signature, manifest, key ID, or self-host pointer blocks readiness. |
| Secrets | No credential reaches CLI argument logs, environment dumps, canonical JSON, compose files, image layers, or evidence artifacts. |
| Privacy deletion | Absolute/traversal/symlink paths, changed files, wrong plan hashes, audit evidence, and legal holds fail closed. |
| Compose host | Container has no network, no ambient Docker socket, no privileged capabilities, a read-only root filesystem, and only explicit mounts. |
| Supply chain | CI runs secret scanning and dependency audit; release build/revision/SBOM/artifacts are signed and independently verified. |

## Execution and reporting

1. Create a staging tenant and synthetic data set. Never use customer content.
2. Capture starting release manifest, trusted-key digest, inventory digest,
   configuration fingerprint, and commit SHA.
3. Run automated dependency/secret checks and the full test suite, then execute
   the approved manual/dynamic cases. Preserve commands, redacted request IDs,
   timestamps, versions, impact, and reproduction proof.
4. Classify findings by business impact and exploit prerequisites; create an
   owner and due date. A critical/high finding blocks a paying-customer rollout
   until retested and closed or formally risk-accepted by the accountable owner.
5. Re-run the exact failed case after remediation and attach its new evidence to
   the signed release decision. Do not merely mark a ticket resolved.

The final report must state scope exclusions, testing dates, tools and versions,
tester identity, all findings (including accepted risk), evidence locations,
and retest status. It must not contain credentials, customer data, exploit code
that creates an uncontrolled production risk, or raw payment-provider payloads.
