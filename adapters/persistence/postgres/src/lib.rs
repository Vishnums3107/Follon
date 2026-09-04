//! Transactional PostgreSQL event, outbox, and projection-checkpoint adapter.
//!
//! Every tenant-scoped operation sets `app.tenant_id` within its database
//! transaction so PostgreSQL row-level security remains a second isolation
//! boundary beneath application authorization.

use std::fmt;
use std::path::Path;

use follon_domain::{validate_canonical_id, validate_utc_timestamp};
use native_tls::{Certificate, TlsConnector};
use postgres::{Client, NoTls, Transaction};
use postgres_native_tls::MakeTlsConnector;
use serde_json::Value;
use sha2::{Digest, Sha256};

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_operating_system.sql")),
    (
        2,
        include_str!("../migrations/0002_product_projections.sql"),
    ),
    (3, include_str!("../migrations/0003_news_events.sql")),
    (
        4,
        include_str!("../migrations/0004_fx_reference_pricing.sql"),
    ),
];

/// Durable persistence failure.
#[derive(Debug)]
pub struct PersistenceError(pub String);

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PersistenceError {}

impl From<postgres::Error> for PersistenceError {
    fn from(error: postgres::Error) -> Self {
        if let Some(database) = error.as_db_error() {
            Self(format!(
                "PostgreSQL operation failed [{}]: {}",
                database.code().code(),
                database.message()
            ))
        } else {
            Self(format!("PostgreSQL operation failed: {error}"))
        }
    }
}

/// One immutable domain event and the outbox message committed with it.
#[derive(Clone, Debug, PartialEq)]
pub struct EventAppend {
    /// Unique event ID.
    pub event_id: String,
    /// Owning tenant.
    pub tenant_id: String,
    /// Aggregate category.
    pub aggregate_type: String,
    /// Aggregate identifier.
    pub aggregate_id: String,
    /// Stable event category.
    pub event_type: String,
    /// Structured event body.
    pub payload: Value,
    /// Canonical UTC occurrence timestamp.
    pub occurred_at: String,
    /// Trace correlation ID.
    pub correlation_id: String,
    /// Optional causal event or command ID.
    pub causation_id: Option<String>,
    /// Tenant-scoped command idempotency key.
    pub idempotency_key: String,
    /// Outbox routing topic.
    pub outbox_topic: String,
}

/// Outcome of an atomic event append.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendOutcome {
    /// Aggregate-local monotonic sequence number.
    pub aggregate_sequence: i64,
    /// True only when this call inserted the event and outbox message.
    pub inserted: bool,
}

/// One claimed but not yet delivered outbox item.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaimedMessage {
    /// Message identifier (equal to `outbox.<event_id>`).
    pub message_id: String,
    /// Source event ID.
    pub event_id: String,
    /// Routing topic.
    pub topic: String,
    /// Structured body.
    pub payload: Value,
    /// Delivery attempts including this claim.
    pub attempts: i32,
}

/// Synchronous PostgreSQL adapter. Callers should dedicate it to a blocking
/// service worker or wrap it behind an async blocking pool.
pub struct PostgresStore {
    client: Client,
}

impl PostgresStore {
    /// Connects without transport TLS. This constructor is intended only for
    /// loopback development and test containers.
    pub fn connect_development(connection_uri: &str) -> Result<Self, PersistenceError> {
        let client = Client::connect(connection_uri, NoTls)?;
        Ok(Self { client })
    }

    /// Connects with certificate-validated TLS. An optional PEM CA augments the
    /// platform trust store for private deployment authorities.
    pub fn connect_tls(
        connection_uri: &str,
        additional_ca_pem: Option<&Path>,
    ) -> Result<Self, PersistenceError> {
        let mut builder = TlsConnector::builder();
        if let Some(path) = additional_ca_pem {
            let pem = std::fs::read(path).map_err(|error| {
                PersistenceError(format!("cannot read PostgreSQL CA certificate: {error}"))
            })?;
            let certificate = Certificate::from_pem(&pem).map_err(|error| {
                PersistenceError(format!("invalid PostgreSQL CA certificate: {error}"))
            })?;
            builder.add_root_certificate(certificate);
        }
        let connector = builder.build().map_err(|error| {
            PersistenceError(format!("cannot configure PostgreSQL TLS: {error}"))
        })?;
        let client = Client::connect(connection_uri, MakeTlsConnector::new(connector))?;
        Ok(Self { client })
    }

    /// Applies every embedded migration in order under an advisory lock and
    /// refuses a checksum mismatch for any already-recorded version.
    pub fn migrate(&mut self) -> Result<(), PersistenceError> {
        let mut transaction = self.client.transaction()?;
        transaction.batch_execute(
            "SELECT pg_advisory_xact_lock(6433752855195311);\
             CREATE TABLE IF NOT EXISTS follon_schema_migrations (\
               version BIGINT PRIMARY KEY,\
               sha256 BYTEA NOT NULL CHECK (octet_length(sha256) = 32),\
               applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()\
             );",
        )?;
        for (version, sql) in MIGRATIONS {
            let checksum = Sha256::digest(sql.as_bytes()).to_vec();
            if let Some(row) = transaction.query_opt(
                "SELECT sha256 FROM follon_schema_migrations WHERE version = $1",
                &[version],
            )? {
                let recorded: Vec<u8> = row.get(0);
                if recorded != checksum {
                    return Err(PersistenceError(format!(
                        "database migration checksum mismatch at version {version}"
                    )));
                }
            } else {
                transaction.batch_execute(sql)?;
                transaction.execute(
                    "INSERT INTO follon_schema_migrations (version, sha256) VALUES ($1, $2)",
                    &[version, &checksum],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Confirms the connection is writable and PostgreSQL is accepting queries.
    pub fn health_check(&mut self) -> Result<(), PersistenceError> {
        self.client.simple_query("SELECT 1")?;
        Ok(())
    }

    /// Provisions one tenant through the same RLS context it will use later.
    pub fn provision_tenant(
        &mut self,
        tenant_id: &str,
        display_name: &str,
    ) -> Result<(), PersistenceError> {
        validate_id("tenant_id", tenant_id)?;
        if display_name.trim().is_empty() || display_name.len() > 200 {
            return Err(PersistenceError("invalid tenant display name".to_owned()));
        }
        let mut transaction = self.client.transaction()?;
        set_tenant(&mut transaction, tenant_id)?;
        transaction.execute(
            "INSERT INTO tenants (tenant_id, display_name) VALUES ($1, $2) \
             ON CONFLICT (tenant_id) DO UPDATE SET display_name = EXCLUDED.display_name",
            &[&tenant_id, &display_name],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Atomically appends one event and its outbox message. Aggregate sequence
    /// assignment is serialized with an advisory transaction lock. A repeated
    /// idempotency key succeeds only when its payload fingerprint is identical.
    pub fn append_event(&mut self, event: &EventAppend) -> Result<AppendOutcome, PersistenceError> {
        validate_event(event)?;
        let canonical_payload = serde_json::to_vec(&event.payload)
            .map_err(|error| PersistenceError(format!("cannot encode event payload: {error}")))?;
        let payload_hash = Sha256::digest(&canonical_payload).to_vec();
        let mut transaction = self.client.transaction()?;
        set_tenant(&mut transaction, &event.tenant_id)?;

        if let Some(row) = transaction.query_opt(
            "SELECT aggregate_sequence, payload_sha256, event_type, aggregate_type, aggregate_id \
             FROM domain_events WHERE tenant_id = $1 AND idempotency_key = $2",
            &[&event.tenant_id, &event.idempotency_key],
        )? {
            let existing_hash: Vec<u8> = row.get(1);
            let existing_event_type: String = row.get(2);
            let existing_aggregate_type: String = row.get(3);
            let existing_aggregate_id: String = row.get(4);
            if existing_hash != payload_hash
                || existing_event_type != event.event_type
                || existing_aggregate_type != event.aggregate_type
                || existing_aggregate_id != event.aggregate_id
            {
                return Err(PersistenceError(
                    "idempotency key was reused with different event content".to_owned(),
                ));
            }
            return Ok(AppendOutcome {
                aggregate_sequence: row.get(0),
                inserted: false,
            });
        }

        transaction.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&format!(
                "{}:{}:{}",
                event.tenant_id, event.aggregate_type, event.aggregate_id
            )],
        )?;
        let row = transaction.query_one(
            "SELECT COALESCE(MAX(aggregate_sequence), 0) + 1 \
             FROM domain_events \
             WHERE tenant_id = $1 AND aggregate_type = $2 AND aggregate_id = $3",
            &[&event.tenant_id, &event.aggregate_type, &event.aggregate_id],
        )?;
        let aggregate_sequence: i64 = row.get(0);
        transaction.execute(
            "INSERT INTO domain_events (\
               event_id, tenant_id, aggregate_type, aggregate_id, aggregate_sequence,\
               event_type, payload, occurred_at, correlation_id, causation_id,\
               idempotency_key, payload_sha256\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8::timestamptz,$9,$10,$11,$12)",
            &[
                &event.event_id,
                &event.tenant_id,
                &event.aggregate_type,
                &event.aggregate_id,
                &aggregate_sequence,
                &event.event_type,
                &event.payload,
                &event.occurred_at,
                &event.correlation_id,
                &event.causation_id,
                &event.idempotency_key,
                &payload_hash,
            ],
        )?;
        let message_id = format!("outbox.{}", event.event_id);
        transaction.execute(
            "INSERT INTO outbox_messages \
             (message_id, tenant_id, event_id, topic, payload) VALUES ($1,$2,$3,$4,$5)",
            &[
                &message_id,
                &event.tenant_id,
                &event.event_id,
                &event.outbox_topic,
                &event.payload,
            ],
        )?;
        transaction.commit()?;
        Ok(AppendOutcome {
            aggregate_sequence,
            inserted: true,
        })
    }

    /// Claims available messages without blocking other workers. Abandoned
    /// claims become available again after `claim_timeout_seconds`.
    pub fn claim_outbox(
        &mut self,
        tenant_id: &str,
        worker_id: &str,
        limit: i64,
        claim_timeout_seconds: i32,
    ) -> Result<Vec<ClaimedMessage>, PersistenceError> {
        validate_id("tenant_id", tenant_id)?;
        validate_id("worker_id", worker_id)?;
        if !(1..=1_000).contains(&limit) || !(1..=86_400).contains(&claim_timeout_seconds) {
            return Err(PersistenceError("invalid outbox claim limits".to_owned()));
        }
        let mut transaction = self.client.transaction()?;
        set_tenant(&mut transaction, tenant_id)?;
        let rows = transaction.query(
            "WITH candidates AS (\
                SELECT message_id FROM outbox_messages\
                WHERE tenant_id = $1 AND delivered_at IS NULL AND available_at <= NOW()\
                  AND (claimed_at IS NULL OR claimed_at < NOW() - ($4 * INTERVAL '1 second'))\
                ORDER BY created_at, message_id\
                FOR UPDATE SKIP LOCKED LIMIT $3\
             )\
             UPDATE outbox_messages AS message\
             SET claimed_at = NOW(), claimed_by = $2, attempts = attempts + 1\
             FROM candidates WHERE message.message_id = candidates.message_id\
             RETURNING message.message_id, message.event_id, message.topic, message.payload, message.attempts",
            &[&tenant_id, &worker_id, &limit, &claim_timeout_seconds],
        )?;
        transaction.commit()?;
        Ok(rows
            .into_iter()
            .map(|row| ClaimedMessage {
                message_id: row.get(0),
                event_id: row.get(1),
                topic: row.get(2),
                payload: row.get(3),
                attempts: row.get(4),
            })
            .collect())
    }

    /// Marks a claimed message delivered only for the owning worker.
    pub fn mark_outbox_delivered(
        &mut self,
        tenant_id: &str,
        worker_id: &str,
        message_id: &str,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.client.transaction()?;
        set_tenant(&mut transaction, tenant_id)?;
        let changed = transaction.execute(
            "UPDATE outbox_messages SET delivered_at = NOW(), claimed_at = NULL, claimed_by = NULL\
             WHERE tenant_id = $1 AND message_id = $2 AND claimed_by = $3 AND delivered_at IS NULL",
            &[&tenant_id, &message_id, &worker_id],
        )?;
        if changed != 1 {
            return Err(PersistenceError(
                "outbox message is not claimed by this worker".to_owned(),
            ));
        }
        transaction.commit()?;
        Ok(())
    }
}

fn set_tenant(transaction: &mut Transaction<'_>, tenant_id: &str) -> Result<(), PersistenceError> {
    transaction.query_one(
        "SELECT set_config('app.tenant_id', $1, TRUE)",
        &[&tenant_id],
    )?;
    Ok(())
}

fn validate_id(name: &str, value: &str) -> Result<(), PersistenceError> {
    validate_canonical_id(name, value).map_err(|error| PersistenceError(error.0))
}

fn validate_event(event: &EventAppend) -> Result<(), PersistenceError> {
    for (name, value) in [
        ("event_id", event.event_id.as_str()),
        ("tenant_id", event.tenant_id.as_str()),
        ("aggregate_type", event.aggregate_type.as_str()),
        ("aggregate_id", event.aggregate_id.as_str()),
        ("event_type", event.event_type.as_str()),
        ("correlation_id", event.correlation_id.as_str()),
        ("idempotency_key", event.idempotency_key.as_str()),
        ("outbox_topic", event.outbox_topic.as_str()),
    ] {
        validate_id(name, value)?;
    }
    if let Some(causation_id) = &event.causation_id {
        validate_id("causation_id", causation_id)?;
    }
    validate_utc_timestamp("occurred_at", &event.occurred_at)
        .map_err(|error| PersistenceError(error.0))?;
    if !event.payload.is_object() {
        return Err(PersistenceError(
            "event payload must be a JSON object".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> EventAppend {
        EventAppend {
            event_id: "event.order-1".to_owned(),
            tenant_id: "tenant.acme".to_owned(),
            aggregate_type: "order".to_owned(),
            aggregate_id: "order.one".to_owned(),
            event_type: "order.accepted".to_owned(),
            payload: serde_json::json!({"quantity": "1.00000000"}),
            occurred_at: "2026-08-24T10:00:00Z".to_owned(),
            correlation_id: "trace.one".to_owned(),
            causation_id: Some("command.one".to_owned()),
            idempotency_key: "idem.one".to_owned(),
            outbox_topic: "orders.events".to_owned(),
        }
    }

    #[test]
    fn migration_contains_durable_safety_boundaries() {
        let migration_sql = MIGRATIONS
            .iter()
            .map(|(_, sql)| *sql)
            .collect::<Vec<_>>()
            .join("\n");
        for required in [
            "FORCE ROW LEVEL SECURITY",
            "DEFERRABLE INITIALLY DEFERRED",
            "UNIQUE (tenant_id, idempotency_key)",
            "broker_receipts",
            "customer_sessions",
            "broker_accounts",
            "strategy_versions",
            "configuration_versions",
            "order_projections",
            "execution_projections",
            "position_projections",
            "audit_event_indexes",
            "billing_subscription_evidence",
            "customer_recovery_codes",
            "customer_users_tenant_user_id_key",
            "FOREIGN KEY (tenant_id, source_event_id)",
            "PENDING_REPLACE",
            "news_headlines",
            "news_sentiments",
            "fx_instrument_economics_versions",
            "fx_pricing_snapshots",
        ] {
            assert!(migration_sql.contains(required), "missing {required}");
        }
        assert_eq!(MIGRATIONS.len(), 4);
    }

    #[test]
    fn event_contract_rejects_non_object_payload_and_invalid_ids() {
        assert!(validate_event(&sample_event()).is_ok());
        let mut invalid = sample_event();
        invalid.payload = serde_json::json!([1, 2, 3]);
        assert!(validate_event(&invalid).is_err());
        invalid = sample_event();
        invalid.tenant_id = "Tenant ACME".to_owned();
        assert!(validate_event(&invalid).is_err());
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn postgres_migration_append_and_idempotency_round_trip() {
        let uri = std::env::var("FOLLON_TEST_DATABASE_URL").expect("database URL");
        let mut store = PostgresStore::connect_development(&uri).unwrap();
        store.migrate().unwrap();
        store.provision_tenant("tenant.acme", "ACME").unwrap();
        let first = store.append_event(&sample_event()).unwrap();
        let second = store.append_event(&sample_event()).unwrap();
        assert!(first.inserted);
        assert!(!second.inserted);
        assert_eq!(first.aggregate_sequence, second.aggregate_sequence);
    }
}
