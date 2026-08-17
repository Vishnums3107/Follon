# Infrastructure

The foundation supports deterministic **simulation-only** replay. The compose
file provisions non-production PostgreSQL, S3-compatible object storage, the
read-only dashboard, and an opt-in storage tool. Local append-only evidence,
deterministic Parquet/DuckDB datasets, and immutable MinIO artifact operations
are implemented adapters with separate responsibilities.

## Local dependencies

```powershell
Copy-Item infra/.env.example infra/.env
# Replace both local-development passwords in infra/.env.
docker compose --env-file infra/.env -f infra/compose.dev.yml up -d
```

Open the local read-only evidence dashboard at `http://127.0.0.1:8080`. It
shows PostgreSQL/MinIO health, the implemented capability and acceptance-gate
map, and compatible `.ndjson`, `.json`, `.md`, and `.csv` evidence from the
repository `var/` directory. The dashboard is loopback-only and has no trading
controls.

For an authenticated deployment, set `FOLLON_DASHBOARD_MODE=production` plus
both dashboard credential variables. Production mode fails closed unless the
password is at least 16 characters. Basic authentication must be placed behind
operator-managed TLS; it is not a substitute for an application identity,
authorization, session, or customer-entitlement gateway.

Services bind only to loopback addresses. `infra/.env` is ignored and must
never contain broker credentials. Before deploying a service, replace local
passwords with a managed secret provider and configure encrypted backups,
retention, monitoring, and recovery drills.

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
