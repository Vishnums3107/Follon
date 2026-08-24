# Commercial controls and self-hosting runbook

Use this runbook only from a controlled operator workstation or CI signing
environment. The commands below are local evidence commands; they do not call
a payment provider, broker, secret store, or identity service.

## 1. Provision and evidence a subscription

Validate the versioned input against the corresponding contract, retain the
original provider event/receipt in the authorized billing system, and record
only its SHA-256 and a pseudonymous provider reference here.

```powershell
cargo run -p follon-cli --bin follon-admin -- provision tests/fixtures/config/commercial-provisioning-v1.json --ledger var/commercial.ndjson --event-id event.provision.acme.001 --actor operator.alice
cargo run -p follon-cli --bin follon-admin -- subscription tests/fixtures/config/commercial-subscription-v1.json --ledger var/commercial.ndjson --event-id event.subscription.acme.001 --actor billing.stripe --observed-at 2026-08-12T09:01:00Z
cargo run -p follon-cli --bin follon-admin -- entitlement tenant.acme --ledger var/commercial.ndjson --as-of 2026-08-12T10:00:00Z
```

Store the ledger on encrypted, access-controlled storage. One process can open
it for writes; readers take a stable shared lock and verify every chain record.
Any hash failure, duplicate event ID, plan mismatch, malformed timestamp, or
unrecognized event fails closed. Hosted services must use the emitted
entitlement evidence at their gateway; recording it alone does not enforce
access elsewhere.

## 2. Create and sign a release

Build release binaries and the SBOM in a clean, pinned build environment. Copy
only the final artifacts to an isolated release directory. Artifact IDs must be
canonical and supplied in strict ascending order.

```powershell
cargo run -p follon-cli --bin follon-admin -- release-manifest --release-id release.0.1.0 --version 0.1.0 --created-at 2026-08-12T10:00:00Z --source-revision <40-or-64-lowercase-git-sha> --sbom-sha256 <sbom-sha256> --artifacts-root C:\release --artifact follon.admin=follon-admin --output C:\evidence\release-manifest.json

# Run this once in an offline/managed signing environment; protect the private
# PKCS#8 output as a secret and distribute only the trusted public-key document.
cargo run -p follon-cli --bin follon-admin -- release-keygen --key-id release.key.001 --private-key C:\secure\release.key.pk8 --trusted-key C:\evidence\trusted-release-key.json
cargo run -p follon-cli --bin follon-admin -- release-sign C:\evidence\release-manifest.json --private-key C:\secure\release.key.pk8 --key-id release.key.001 --signed-at 2026-08-12T10:01:00Z --output C:\evidence\release-signature.json
cargo run -p follon-cli --bin follon-admin -- release-verify C:\evidence\release-manifest.json C:\evidence\release-signature.json C:\evidence\trusted-release-key.json --artifacts-root C:\release
```

Never reuse an existing private-key output path, commit a private key, or
replace a manifest/signature/readiness file. The CLI deliberately refuses all
of those actions. Rotate trusted public keys through a separately reviewed,
versioned deployment change; a key ID inside an untrusted manifest is not trust.

## 3. Self-host readiness

Create canonical `self-host.json` matching
`contracts/json-schema/v1/self-host-configuration.schema.json`. Set its two
release hashes from the exact manifest and signature bytes, set only a loopback
bind address, and use `managed_command` as the secret-provider boundary. The
configuration contains no credential or command string.

```powershell
cargo run -p follon-cli --bin follon-admin -- self-host-validate C:\deployment\self-host.json
cargo run -p follon-cli --bin follon-admin -- self-host-readiness C:\deployment\self-host.json C:\evidence\release-manifest.json C:\evidence\release-signature.json C:\evidence\trusted-release-key.json --artifacts-root C:\release --ledger C:\evidence\commercial.ndjson --as-of 2026-08-12T10:02:00Z --output C:\evidence\self-host-readiness.json
```

Deploy only after `self_host_readiness_schema_version: 1`, `state: READY`, and
the ledger-bound `self_hosting_allowed: true` are present in immutable readiness
evidence. Re-run readiness verification after copying artifacts, before every
start, and after any storage/release/key or entitlement change. A changed release
artifact, non-self-host plan, or expired entitlement must fail rather than start.

`infra/compose.selfhost.yml` is a deliberately restricted operator-container
profile: no network, no published ports, read-only root filesystem, dropped
capabilities, an unprivileged user, and explicit read-only release/config
mounts. It validates a pre-verified local deployment; it is not a substitute
for reverse-proxy TLS, host patching, encrypted volumes, backups, or a managed
secret provider. See `infra/self-host.env.example` before invoking it.

## Stop conditions

Stop deployment and open an incident if the ledger chain fails, an entitlement
is not `FULL` for the intended self-host tenant, release verification fails,
the trusted key is unexpected, any artifact differs, a secret appears in a
configuration/evidence file, or a validation command has been bypassed. Preserve
the failing inputs and hashes without copying customer content into tickets.
