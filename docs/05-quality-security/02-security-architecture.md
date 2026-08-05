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
