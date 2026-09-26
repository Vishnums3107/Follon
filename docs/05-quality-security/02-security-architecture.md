# Security architecture

## Baseline

Use NIST SSDF as the secure-development process baseline and target OWASP ASVS Level 2 for initial application-security verification. Obtain independent security assessment before broad commercial release.

## Required controls

- Passkeys or MFA; short-lived sessions; session revocation.
- Role-based access control with separate trading and administration privileges.
- OS keychain for local secrets and managed secret storage for hosted deployments.
- Envelope encryption for stored broker credentials; TLS in transit; database encryption at rest.
- Immutable security audit trail, signed releases, SBOMs, pinned dependencies, reproducible builds.
- Automated dependency, static, dynamic, and secret scanning; protected branches and mandatory CI checks.
- Rate limits and request idempotency.
- Secure broker-token rotation.

## Absolute boundary

Broker credentials must never be exposed to strategy code. The strategy-worker boundary is a security boundary as well as a reliability boundary.

## First implementation controls

Before any broker connectivity: define secret interfaces, add secret scanning and dependency pinning to CI, ensure test fixtures contain no credentials, and record an initial threat model.

## Implemented local controls and remaining boundary

The repository implements the strategy/credential boundary, constant-time
dashboard credential checks, a protected password-file ingress, restrictive
browser headers, per-direct-peer authentication rate limiting, idempotent
request/artifact identities, hash-chained audit, detached signed release
manifests, pinned lockfiles, secret/advisory CI checks, and a deterministic
CycloneDX 1.6 SBOM generator at `tools/generate_sbom.py`. CI tests and retains
the SBOM as a release artifact. `core/identity` additionally enforces Argon2id
password hashing/policy/rotation, bounded TOTP challenges, hashed one-time
recovery codes, lockout, opaque hashed 15-minute sessions, immediate
security-version revocation, tenant authorization, and five server-side RBAC
roles. Since E3.3a, `follon-trading-api` enforces it on its write RPC. It loads
a one-tenant operator directory (Argon2id hashes and mandatory TOTP), provisioned
by `follon-admin operator-add`, and requires a bearer session whose role grants
PAPER trading. PostgreSQL identity rows are protected by forced tenant RLS. Production
Compose requires database TLS, gRPC mutual TLS, and client-certificate TLS at
the dashboard proxy.

These mechanisms do not provide a production vault/keychain, certificate
issuance/rotation, HSM/KMS signing custody, out-of-band MFA enrollment or
delivery, support operations, an independent DAST product run, an independent
penetration test, or security operations for an exact deployment.
`tools/dast_scan.py` is a repository-authored dynamic scan of a local loopback
deployment; it is evidence for engineering, not an independent assessment. Passkeys are
also not implemented; TOTP is the current MFA method. See the
[master-plan conformance audit](../06-delivery/14-master-plan-conformance-audit.md).
