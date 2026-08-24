# Follon storage adapter

This package implements the storage boundary specified in
`docs/02-architecture/02-storage-and-protocols.md`:

- canonical normalized-bar CSV to deterministic, Zstandard-compressed Parquet;
- durable DuckDB registration of immutable external Parquet datasets;
- idempotent S3/MinIO artifact publication and verified recovery.

All writes fail on conflicting immutable content. Parquet embeds dataset,
version, source SHA-256, and schema metadata. DuckDB rechecks the Parquet hash
and row count. S3 publication reads the object back and verifies its content
against SHA-256 metadata. Recovery fsyncs a temporary file before atomic local
publication.

Credentials are never accepted as command-line arguments. Use the standard AWS
environment/provider chain (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and an
optional session token or workload identity).

```powershell
python -m venv target/storage-venv
target/storage-venv/Scripts/python -m pip install -e python/storage-adapter

target/storage-venv/Scripts/follon-storage publish-bars `
  tests/fixtures/historical-bars/spy-one-minute.csv var/datasets/spy-v1.parquet `
  --dataset-id dataset.spy --dataset-version v1

target/storage-venv/Scripts/follon-storage register-dataset `
  var/datasets/spy-v1.parquet var/catalog/research.duckdb
```

For MinIO, pass `--endpoint-url http://127.0.0.1:9000` or set
`FOLLON_S3_ENDPOINT_URL`. Provision and verify the versioned bucket before
publication:

```powershell
target/storage-venv/Scripts/follon-storage ensure-bucket `
  --bucket follon-evidence-dev --endpoint-url http://127.0.0.1:9000

target/storage-venv/Scripts/follon-storage publish-artifact `
  --bucket follon-evidence-dev --key backtests/run-1/artifact.json `
  --endpoint-url http://127.0.0.1:9000 --allow-unencrypted-development `
  var/dashboard-backtest.json

target/storage-venv/Scripts/follon-storage recover-artifact `
  --bucket follon-evidence-dev --key backtests/run-1/artifact.json `
  --endpoint-url http://127.0.0.1:9000 target/recovered-artifact.json
```

The publisher conditionally creates each object key and then reads the object
back, preventing concurrent conflicting publication. It requests S3-managed
AES-256 encryption by default. A MinIO development instance without KMS may use
`--allow-unencrypted-development`, which is rejected for non-local endpoints
and must never be used as a production policy. The deployment owner must still
configure retention/object lock, replication, backup, and recovery policy.
