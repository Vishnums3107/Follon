# Python components

The supported strategy SDK and IBKR PAPER bridge preserve the approved
event/intent and broker boundaries. Strategies never receive broker credentials
or direct adapter access.

The [storage adapter](storage-adapter/README.md) provides deterministic Parquet
publication, a durable DuckDB dataset catalog, and immutable S3-compatible
artifact publication/recovery for trusted operator workflows.
