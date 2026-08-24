# Production operations and evidence runbook

**Status:** executable controls are implemented; external ownership and
operating evidence are not pre-approved by this repository.

## Promotion sequence

Only `development -> staging -> production` is valid. Build immutable images,
generate/review the SBOM, create the canonical release manifest, sign it with
the offline key, and verify every artifact. Generate the external acceptance
status from retained ledgers:

```powershell
python tools/acceptance_evidence.py <acceptance-ledger-root> --output <acceptance-status.json>
python tools/release_promotion_gate.py `
  --source-environment staging --target-environment production `
  --manifest <manifest.json> --signature <signature.json> `
  --trusted-key <trusted-key.json> --artifacts-root <release-root> `
  --acceptance-status <acceptance-status.json> `
  --requester <user.id> --approver <different.user.id> `
  --change-ticket <change.id> --receipt <new-promotion-receipt.json>
```

Production promotion fails while any external acceptance target is below its
required count. The receipt is an eligibility decision; the deployment system
must separately retain image-digest rollout, smoke, rollback, and approver
evidence.

## Acceptance evidence

Never generate fictional sessions or customer facts. Each
`*.acceptance.ndjson` line is strict schema v1, binds a retained source artifact
by SHA-256, uses a distinct observer/reviewer, and chains to the prior record.
The validator counts unique accepted subjects and preserves rejected records.
The required gates are 30 PAPER sessions, 60 controlled-LIVE sessions, five
design partners, one broker-backed options acceptance, and one paying-customer
acceptance record. Commercial roadmap targets above one customer remain in the
master-plan audit and cannot be inferred from this minimum technical gate.

## Monitoring and on-call

Combine `infra/compose.production.yml` and `infra/compose.monitoring.yml` only
after supplying reviewed, digest-pinned images, monitoring client certificates,
and the deployment-owned Alertmanager configuration. The on-call owner must
prove one test page, acknowledgement, escalation, and resolution before a
capital session. Silence expiry, maintenance ownership, and paging rotations
belong to the external incident-management system.

### Endpoint unavailable

1. Stop new capital submissions with the independent kill switch.
2. Confirm whether the dashboard TLS endpoint, gRPC mTLS endpoint, database, or
   monitoring path failed; do not treat a missing probe as application health.
3. Preserve container logs, broker evidence, outbox state, and current release
   digests.
4. Reconcile broker orders, fills, positions, and cash before reconnecting.
5. Roll back only to an independently verified signed release and retain the
   incident/recovery receipt.

### Monitoring target missing

Check Prometheus configuration, black-box exporter health, certificate mounts,
DNS, and time synchronization. A monitoring blind spot is a stop condition for
new controlled-LIVE work.

### Certificate expiry

Issue replacement certificates through the deployment CA, verify SANs and
client trust, stage them, test mTLS from the monitoring identity and an operator
identity, then promote through the two-person release path. Never weaken client
verification or set insecure certificate flags to clear the alert.

## PostgreSQL backup and restore

Use libpq variables `PGHOST`, `PGPORT`, `PGDATABASE`, and `PGUSER`. Authentication
must come from a protected `PGPASSFILE` or managed identity; the tool refuses
`PGPASSWORD`.

```powershell
python tools/postgres_recovery.py backup `
  --output-directory <encrypted-immutable-backup-root> `
  --backup-id <canonical.backup.id>

python tools/postgres_recovery.py restore-drill `
  --dump <backup.dump> --manifest <backup.manifest.json> `
  --target-database follon_restore_drill_<unique_id> `
  --confirm-disposable-database follon_restore_drill_<unique_id> `
  --receipt <new-restore-receipt.json>
```

The drill verifies the backup hash and migrated schema in a newly created,
strictly named database, writes a receipt, and then removes that drill database.
The operator must additionally verify row counts, tenant isolation, broker
reconciliation, RPO/RTO, encrypted off-site custody, retention, and alerting.

## External approvals

Independent penetration testing, remediation acceptance, entity/legal/tax
review, market-data licensing, broker/API permission, terms/privacy/contracts,
production secret custody, named on-call, and actual design-partner/customer
acceptance are external facts. Store their approved artifacts outside source
control and reference only hashes and pseudonymous canonical IDs in evidence.
