# Infrastructure

The foundation supports deterministic **simulation-only** replay. The compose
file provisions the documented non-production PostgreSQL and S3-compatible
object-storage dependencies, but the first slice still uses an append-only
local NDJSON event log as its persistence adapter.

## Local dependencies

```powershell
Copy-Item infra/.env.example infra/.env
# Replace both local-development passwords in infra/.env.
docker compose --env-file infra/.env -f infra/compose.dev.yml up -d
```

Services bind only to loopback addresses. `infra/.env` is ignored and must
never contain broker credentials. Before deploying a service, replace local
passwords with a managed secret provider and configure encrypted backups,
retention, monitoring, and recovery drills.

## Replay container

Build the non-live CLI image from the repository root:

```powershell
docker build -f infra/Dockerfile.replay -t follon-replay:local .
```

The image has no broker client, credential interface implementation, or live
execution mode. It is suitable for deterministic historical replay only.

This directory will contain Terraform, container definitions, and monitoring configuration. Initial infrastructure should use managed PostgreSQL, S3-compatible object storage, managed secrets, and simple deployment topology; Kubernetes is deferred.
