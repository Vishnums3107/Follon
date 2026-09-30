# Production operations and evidence runbook

**Status:** executable controls are implemented; external ownership and
operating evidence are not pre-approved by this repository.

## Promotion sequence

Only `development -> staging -> production` is valid. Build immutable images,
generate/review the SBOM, create the canonical release manifest, sign it with
the offline key, and verify every artifact. Then run the gate against the
retained acceptance ledgers:

```powershell
python tools/release_promotion_gate.py `
  --source-environment staging --target-environment production `
  --manifest <manifest.json> --signature <signature.json> `
  --trusted-key <trusted-key.json> --artifacts-root <release-root> `
  --acceptance-ledger-root <acceptance-ledger-root> `
  --acceptance-trusted-reviewers <trusted-reviewers.json> `
  --acceptance-artifact-root <acceptance-artifact-root> `
  --requester <user.id> --approver <different.user.id> `
  --change-ticket <change.id> --receipt <new-promotion-receipt.json>
```

The gate recomputes the acceptance status from the ledger root with
`tools/acceptance_evidence.py audit`, for the release the manifest names; it
takes no status document, because a supplied one could simply declare every gate
eligible. Production promotion fails while any external acceptance target is
below its required count. The receipt (schema 3) is an eligibility decision: it
binds the release, the SHA-256 of the trusted reviewer set the audit used, the
recomputed status's SHA-256, and each ledger file counted, by path, SHA-256,
record count, and chain head. The deployment system must separately retain
image-digest rollout, smoke, rollback, and approver evidence. To inspect the
status without promoting, run:

```powershell
python tools/acceptance_evidence.py audit <acceptance-ledger-root> `
  --trusted-reviewers <trusted-reviewers.json> `
  --artifact-root <acceptance-artifact-root> --release-id <release.id>
```

## Acceptance evidence

Never generate fictional sessions or customer facts. A record counts only when
every one of these holds; a record that fails one is kept in the ledger, listed
with its reason, and never counted.

- **A listed reviewer signed it, and the key is not revoked.** Each
  `*.acceptance.ndjson` line is strict schema v2 and carries the reviewer's
  Ed25519 signature over a domain-separated canonical body: every field except
  the signature and the record hash, so the previous record's hash, the release,
  the attributes and the notes are all covered. The trusted reviewer set the
  operator controls lists each key for one reviewer. **Every record in the root
  must be signed by a key the set lists for the reviewer it names, or the audit
  fails.** A record that was edited, moved in the chain or forged is caught that
  way, and so is a rejection whose reviewer was dropped from the set; before,
  each of those was silently ignored, which requalified a rejected session. An
  empty set, which is what the pipeline writes when there is none, passes only
  an empty root. An acceptance signed by a revoked key does not count
  (`REVIEWER_REVOKED`).
- **Its artifact is retained, and backs this subject alone.**
  `source_artifact_sha256` is re-hashed against a content-addressed file in the
  artifact root. A missing, altered or linked artifact is a record nobody can
  check (`ARTIFACT_UNVERIFIED`). An artifact that accepted records cite for two
  subjects, in any gate or release, counts for neither (`ARTIFACT_SHARED`), so
  thirty sessions cannot be one session log.
- **It is about this release.** `release_id` must be the release being audited
  or promoted (`OTHER_RELEASE` otherwise). A PAPER or LIVE session also names the
  environment it ran in, and no other record type has one.
- **It meets the criteria** (`CRITERIA_NOT_MET`), fixed before any session counts:
  - *A clean session* lasted at least 23,400 seconds (a regular 6.5-hour US
    equity session), submitted and reconciled at least one order, and closed
    with no `UNKNOWN` order, no reconciliation discrepancy and no unexplained
    incident. A reconnect nobody planned disqualifies it, whatever came after.
    A planned reconnect drill is recorded and never disqualifies.
  - *A design partner* completed at least one workflow unaided.
  - *The options acceptance* reconciled one broker export across BACKTEST,
    PAPER and LIVE.
  - *A paying customer* names a professional or an organisation and a
    subscription. A subscription that two customers cite counts for neither
    (`SUBSCRIPTION_SHARED`), and a customer recorded as both kinds counts as
    neither (`CUSTOMER_KIND_CONFLICT`).

The ledger itself must be intact. A record that is malformed, out of chain or
mis-hashed makes the audit fail rather than count fewer records, because a
tampered ledger cannot say which of its records are still true. Each line must be
exactly its record's canonical JSON, the bytes `append` writes: keys sorted, no
whitespace, non-ASCII escaped, and one newline. Any other spelling of a record
fails the audit, because JSON keeps the last of two duplicate keys and a line
could then read one way and count another. So does a symbolic link or junction
anywhere under the ledger root, and anything the JSON parser cannot read. A
ledger is a file named exactly `*.acceptance.ndjson`, in lowercase, on every
platform.

Counting is by distinct subject. A rejection disqualifies its subject in that
gate whether it was recorded before or after an acceptance, because the ledger
has no correction record, and so does an acceptance whose own attributes fail
the criteria: a clean acceptance of the same session does not outweigh it. Both
disqualify in every release, whatever their artifact, and whether or not their
key was later revoked (`DISQUALIFIED`). A session wrongly rejected is run again
under a new subject id.

**Reviewers are never removed from the set.** A reviewer who leaves stays
listed, so what they signed stays verifiable. A key that is lost, compromised or
no longer trusted is marked `revoked`: it still verifies what it signed, every
acceptance it signed stops counting, and every rejection it signed still
disqualifies, because revoking must never be a way to requalify a subject.
Re-review what a revoked key accepted under an active key. A reviewer whose key
changes gets a new entry with a new key id.

The gates are 30 PAPER sessions, 60 controlled-LIVE sessions, five design
partners, one broker-backed options acceptance, and **ten paying professionals
or three paying organisations**. The customer gate is the roadmap's, no longer
the technical minimum of one. The status reports how far each alternative
stands.

Reviewers work on their own machines. A record template is a JSON object of
exactly `evidence_id`, `evidence_type`, `subject_id`, `occurred_at`,
`observed_by`, `reviewed_by`, `outcome`, `notes`, `release_id`, `environment` and
`attributes`. The tool adds the schema version, the artifact's digest, the key
id, the signature, the chain link and the hash, and appends it with:

```powershell
python tools/acceptance_evidence.py append <ledger>.acceptance.ndjson `
  --record <template.json> --artifact <source-artifact> `
  --artifact-root <acceptance-artifact-root> `
  --reviewer-key <reviewer-signing.pk8> --reviewer-key-id <key.id>
```

It verifies the ledger so far, refuses a repeated `evidence_id`, retains the
artifact, validates the finished record and only then writes it. The reviewer's
key is a PKCS#8 Ed25519 key, the kind `follon-admin release-keygen` writes, and
its public half goes into the trusted reviewer set as
`contracts/json-schema/v2/trusted-reviewers.schema.json` describes, version 2,
with a `status` of `active`. The audit refuses a set holding a key that is not an
honest Ed25519 public key, such as the all-zero placeholder, or one public key
under two entries. Use a key of the reviewer's own, never the release-signing
key.

The evidence pipeline (step 23) audits only `var/acceptance/`, the operational
ledger root, for the release the pipeline's own manifest names, and every
`*.acceptance.ndjson` beneath it counts. It reads the reviewer set from
`var/acceptance-trusted-reviewers.json` and the artifacts from
`var/acceptance-artifacts/`, and writes an empty reviewer set if there is none;
it never overwrites one an operator placed there. Keep review, experiment, and
synthetic ledgers out of that directory. A ledger anywhere else under `var/` is
never counted.

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
