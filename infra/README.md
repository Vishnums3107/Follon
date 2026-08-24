# Infrastructure

The development topology provisions non-production PostgreSQL, S3-compatible
object storage, the read-only React dashboard, the gRPC trading API, and an
opt-in storage tool. Local append-only evidence, transactional event/outbox
persistence, deterministic Parquet/DuckDB datasets, and immutable MinIO artifact
operations remain separate adapters.

## Local dependencies

```powershell
Copy-Item infra/.env.example infra/.env
# Replace both local-development passwords in infra/.env.
docker compose --env-file infra/.env -f infra/compose.dev.yml up -d
```

Open the local read-only evidence dashboard at `http://127.0.0.1:8080`. The
gRPC service listens on `127.0.0.1:50051`. The dashboard shows service health,
the implemented capability and acceptance-gate map, and compatible `.ndjson`,
`.json`, `.md`, and `.csv` evidence from the repository `var/` directory. It is
loopback-only and has no trading controls.

Development dashboard Basic authentication is an operator-only compatibility
boundary; it is not the customer IAM service. Do not expose the development
topology to an untrusted network.

Services bind only to loopback addresses. `infra/.env` is ignored and must
never contain broker credentials. The development API accepts a development
database URL; the production API refuses direct connection-string injection.

## Production candidate topology

`compose.production.yml` accepts only deployment-supplied digest-pinned images
and secret files. It requires PostgreSQL TLS, gRPC mutual TLS, dashboard TLS
with a client certificate, and a protected dashboard password file. It does not
create a production database, CA, secret store, or broker credential.

Combine monitoring only after the deployment owns a routed Alertmanager
configuration and monitoring client identity:

```powershell
docker compose -f infra/compose.production.yml -f infra/compose.monitoring.yml config
```

The checked-in probes and rules cover endpoint availability, missing scrapes,
and certificate expiry. They do not prove a named on-call rotation or incident
response. Follow the
[production operations and evidence runbook](../docs/operations/09-production-operations-runbook.md)
before any staging or production promotion.

## Storage tool

The storage image is an opt-in one-shot Compose profile. It is not a daemon and
does not remain running after a command completes. From the repository root:

```powershell
docker compose --env-file infra/.env -f infra/compose.dev.yml --profile tools run --rm storage `
  publish-bars /fixtures/historical-bars/spy-one-minute.csv /var/follon/datasets/spy-bars-v1.parquet `
  --dataset-id dataset.spy-bars --dataset-version v1

docker compose --env-file infra/.env -f infra/compose.dev.yml --profile tools run --rm storage `
  register-dataset /var/follon/datasets/spy-bars-v1.parquet /var/follon/catalog/research.duckdb

docker compose --env-file infra/.env -f infra/compose.dev.yml --profile tools run --rm storage `
  ensure-bucket --bucket follon-evidence-dev
```

The local MinIO image has no KMS, so only the loopback/internal MinIO endpoint
may use `--allow-unencrypted-development` when publishing an artifact.
Production publication requests server-side AES-256 encryption by default and
must be paired with deployment-owned key, retention, replication, and recovery
controls. Credentials come only from the standard AWS provider/environment
chain; the CLI has no secret arguments.

## Replay container

Build the non-live CLI image from the repository root:

```powershell
docker build -f infra/Dockerfile.replay -t follon-replay:local .
```

The image has no broker client, credential interface implementation, or live
execution mode. It is suitable for deterministic historical replay only.

This directory will contain Terraform, container definitions, and monitoring configuration. Initial infrastructure should use managed PostgreSQL, S3-compatible object storage, managed secrets, and simple deployment topology; Kubernetes is deferred.
