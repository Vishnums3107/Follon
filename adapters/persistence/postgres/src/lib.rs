//! Transactional PostgreSQL event, outbox, and projection-checkpoint adapter.
//!
//! Every tenant-scoped operation sets `app.tenant_id` within its database
//! transaction so PostgreSQL row-level security remains a second isolation
//! boundary beneath application authorization.

use std::fmt;
use std::path::Path;

use follon_domain::{validate_canonical_id, validate_utc_timestamp};
use native_tls::{Certificate, TlsConnector};
use postgres::error::SqlState;
use postgres::{Client, NoTls, Transaction};
use postgres_native_tls::MakeTlsConnector;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Name PostgreSQL assigns (its default `<table>_<col>..._key` convention) to
/// the unnamed `UNIQUE (tenant_id, idempotency_key)` constraint on
/// `domain_events` declared in `migrations/0001_operating_system.sql`. Two
/// transactions that both miss the pre-insert idempotency check (the classic
/// "client retried while the original request was still in-flight" race) will
/// serialize on the aggregate's advisory lock and then race this constraint;
/// the loser's `INSERT` fails here rather than on the aggregate-sequence
/// constraint, so this is the specific violation `append_event` recovers from.
const IDEMPOTENCY_KEY_CONSTRAINT: &str = "domain_events_tenant_id_idempotency_key_key";

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
    (
        5,
        include_str!("../migrations/0005_execution_plan_evidence.sql"),
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
    ///
    /// The idempotency pre-check and the insert are necessarily separate
    /// statements, so two concurrent callers that submit the same
    /// `idempotency_key` can both miss the pre-check, both take the
    /// aggregate's advisory lock in turn, and then race the
    /// `UNIQUE (tenant_id, idempotency_key)` constraint on insert. Rather than
    /// surface that race as a raw constraint-violation error to whichever
    /// caller loses it, the loser re-reads the row the winner just committed
    /// and returns the same fingerprint-checked idempotent outcome the
    /// pre-check path would have returned had it observed the row in time.
    pub fn append_event(&mut self, event: &EventAppend) -> Result<AppendOutcome, PersistenceError> {
        validate_event(event)?;
        let canonical_payload = serde_json::to_vec(&event.payload)
            .map_err(|error| PersistenceError(format!("cannot encode event payload: {error}")))?;
        let payload_hash = Sha256::digest(&canonical_payload).to_vec();
        let mut transaction = self.client.transaction()?;
        set_tenant(&mut transaction, &event.tenant_id)?;

        if let Some(outcome) = idempotent_outcome(&mut transaction, event, &payload_hash)? {
            return Ok(outcome);
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

        // Insert inside a savepoint: on a unique-violation race we need to
        // roll back just this statement (PostgreSQL otherwise aborts the
        // whole transaction on any error) and keep using `transaction` to
        // re-read the winning row and finish the advisory-locked commit path
        // cleanly rather than starting over on a fresh connection.
        let insert_conflict = {
            let mut savepoint = transaction.savepoint("domain_event_insert")?;
            // `$8::text::timestamptz` (not a bare `::timestamptz`): PostgreSQL
            // infers a directly-cast parameter's wire type as the cast's
            // target type, so a bare `$8::timestamptz` would demand a
            // TIMESTAMPTZ-typed argument — but `occurred_at` is carried as an
            // already-validated ISO-8601 `String`, whose `ToSql` impl only
            // accepts text-like types. Casting through `text` first keeps the
            // inferred parameter type TEXT so the `String` binds, then lets
            // PostgreSQL parse it into `timestamptz` server-side.
            let inserted = savepoint.execute(
                "INSERT INTO domain_events (\
                   event_id, tenant_id, aggregate_type, aggregate_id, aggregate_sequence,\
                   event_type, payload, occurred_at, correlation_id, causation_id,\
                   idempotency_key, payload_sha256\
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8::text::timestamptz,$9,$10,$11,$12)",
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
            );
            match inserted {
                Ok(_) => {
                    savepoint.commit()?;
                    None
                }
                // Dropping `savepoint` here rolls back to it, clearing the
                // aborted-transaction state so `transaction` stays usable.
                Err(error) => Some(error),
            }
        };

        if let Some(error) = insert_conflict {
            if !is_idempotency_key_conflict(&error) {
                return Err(error.into());
            }
            // A concurrent transaction won the race and committed the same
            // idempotency key first. Recover exactly as the pre-check above
            // would have if it had observed that row in time.
            return idempotent_outcome(&mut transaction, event, &payload_hash)?.ok_or_else(|| {
                PersistenceError(
                    "idempotency key unique-violation was reported but the concurrent row \
                     could not be re-read"
                        .to_owned(),
                )
            });
        }

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

/// Looks up any existing row for `event`'s idempotency key and reconciles it
/// against `payload_hash`. Returns:
/// - `Ok(None)` when no row exists yet for this key,
/// - `Ok(Some(outcome))` with `inserted: false` when a row exists and its
///   fingerprint (payload hash, event type, aggregate type/id) matches, or
/// - `Err` when a row exists under this key with a different fingerprint.
///
/// Used both as `append_event`'s upfront idempotency check and, unchanged, as
/// the race-recovery path after a unique-violation on the same constraint —
/// the two callers must agree on what counts as "the same request replayed"
/// versus "a genuinely different payload reusing this key".
fn idempotent_outcome(
    transaction: &mut Transaction<'_>,
    event: &EventAppend,
    payload_hash: &[u8],
) -> Result<Option<AppendOutcome>, PersistenceError> {
    let Some(row) = transaction.query_opt(
        "SELECT aggregate_sequence, payload_sha256, event_type, aggregate_type, aggregate_id \
         FROM domain_events WHERE tenant_id = $1 AND idempotency_key = $2",
        &[&event.tenant_id, &event.idempotency_key],
    )?
    else {
        return Ok(None);
    };
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
    Ok(Some(AppendOutcome {
        aggregate_sequence: row.get(0),
        inserted: false,
    }))
}

/// True when `error` is a unique-violation on exactly the
/// `(tenant_id, idempotency_key)` constraint, as opposed to some other
/// constraint (e.g. the `event_id` primary key or the
/// `(tenant_id, aggregate_type, aggregate_id, aggregate_sequence)` constraint)
/// whose violation indicates a different, unrelated problem that must still
/// surface as a hard error.
fn is_idempotency_key_conflict(error: &postgres::Error) -> bool {
    error.as_db_error().is_some_and(|database_error| {
        *database_error.code() == SqlState::UNIQUE_VIOLATION
            && database_error.constraint() == Some(IDEMPOTENCY_KEY_CONSTRAINT)
    })
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
            "venue_capabilities",
            "execution_plan_evidence",
            "execution_route_decisions",
            "execution_benchmark_evidence",
        ] {
            assert!(migration_sql.contains(required), "missing {required}");
        }
        assert_eq!(MIGRATIONS.len(), 5);
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

    /// Builds a `sample_event`-shaped event whose IDs are all namespaced by
    /// `tag`, so each regression test below owns a disjoint tenant/aggregate
    /// and cannot collide with `sample_event()` or with each other when the
    /// `#[ignore]`d PostgreSQL tests run in parallel against one database.
    fn sample_event_for(tag: &str) -> EventAppend {
        EventAppend {
            event_id: format!("event.{tag}-1"),
            tenant_id: format!("tenant.{tag}"),
            aggregate_type: "order".to_owned(),
            aggregate_id: format!("order.{tag}"),
            event_type: "order.accepted".to_owned(),
            payload: serde_json::json!({"quantity": "1.00000000"}),
            occurred_at: "2026-08-24T10:00:00Z".to_owned(),
            correlation_id: format!("trace.{tag}"),
            causation_id: Some(format!("command.{tag}")),
            idempotency_key: format!("idem.{tag}"),
            outbox_topic: "orders.events".to_owned(),
        }
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn append_event_recovers_idempotent_outcome_after_losing_the_insert_race() {
        // This reproduces the *end state* of the race described in
        // `append_event`'s doc comment, not the exact interleaving: a
        // concurrent transaction commits a row for this idempotency key
        // strictly between our pre-check SELECT and our INSERT. There is no
        // internal hook to force that interleaving deterministically, so
        // instead we pre-insert the "winner" row exactly as the concurrent
        // transaction would have, bypassing `append_event` entirely, and then
        // call `append_event` and confirm it reconciles against that row via
        // `idempotent_outcome` (the same fingerprint-comparison contract the
        // non-racing pre-check path already used) rather than surfacing the
        // raw unique-violation error. See
        // `append_event_reconciles_concurrent_duplicate_submissions_of_the_same_idempotency_key`
        // below for a best-effort test that drives the actual race.
        let uri = std::env::var("FOLLON_TEST_DATABASE_URL").expect("database URL");
        let mut store = PostgresStore::connect_development(&uri).unwrap();
        store.migrate().unwrap();
        let event = sample_event_for("race-recovery");
        store.provision_tenant(&event.tenant_id, "ACME").unwrap();

        let canonical_payload = serde_json::to_vec(&event.payload).unwrap();
        let payload_hash = Sha256::digest(&canonical_payload).to_vec();
        {
            let mut winner = store.client.transaction().unwrap();
            set_tenant(&mut winner, &event.tenant_id).unwrap();
            winner
                .execute(
                    "INSERT INTO domain_events (\
                       event_id, tenant_id, aggregate_type, aggregate_id, aggregate_sequence,\
                       event_type, payload, occurred_at, correlation_id, causation_id,\
                       idempotency_key, payload_sha256\
                     ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8::text::timestamptz,$9,$10,$11,$12)",
                    &[
                        &event.event_id,
                        &event.tenant_id,
                        &event.aggregate_type,
                        &event.aggregate_id,
                        &1i64,
                        &event.event_type,
                        &event.payload,
                        &event.occurred_at,
                        &event.correlation_id,
                        &event.causation_id,
                        &event.idempotency_key,
                        &payload_hash,
                    ],
                )
                .unwrap();
            winner.commit().unwrap();
        }

        // Same idempotency key, identical payload: must succeed exactly as
        // the "winner" would report, not raise a constraint-violation error.
        let recovered = store.append_event(&event).unwrap();
        assert!(!recovered.inserted);
        assert_eq!(recovered.aggregate_sequence, 1);

        // Same idempotency key, different payload: must still be rejected,
        // exactly as the existing non-racing pre-check already rejects it.
        let mut conflicting = event.clone();
        conflicting.event_id = format!("event.{}-2", "race-recovery");
        conflicting.payload = serde_json::json!({"quantity": "2.00000000"});
        let error = store.append_event(&conflicting).unwrap_err();
        assert!(error
            .0
            .contains("idempotency key was reused with different event content"));
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn append_event_reconciles_concurrent_duplicate_submissions_of_the_same_idempotency_key() {
        // Best-effort direct reproduction of the race itself, using two
        // independent connections released from a shared barrier at the same
        // instant so both are very likely to run their pre-check SELECT
        // before either commits its INSERT. The exact interleaving isn't
        // guaranteed (there's no internal hook to force it), but regardless
        // of which caller's INSERT wins, neither may ever observe a raw
        // constraint-violation error, and both must agree on the same
        // aggregate_sequence.
        //
        // Each concurrent attempt gets its own `event_id`: a client retry
        // shares the same `idempotency_key` (and the same aggregate/payload)
        // across attempts, but `event_id` identifies this particular append
        // attempt, so two in-flight attempts for one retried command are not
        // expected to collide on it. Reusing one `event_id` for both would
        // instead race the unrelated `domain_events` primary key and never
        // exercise the idempotency-key recovery path this test targets.
        let uri = std::env::var("FOLLON_TEST_DATABASE_URL").expect("database URL");
        let mut setup_store = PostgresStore::connect_development(&uri).unwrap();
        setup_store.migrate().unwrap();
        let event = sample_event_for("concurrent-race");
        setup_store
            .provision_tenant(&event.tenant_id, "ACME")
            .unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let results: Vec<Result<AppendOutcome, PersistenceError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|attempt| {
                    let uri = uri.clone();
                    let mut event = event.clone();
                    event.event_id = format!("{}-attempt-{attempt}", event.event_id);
                    let barrier = std::sync::Arc::clone(&barrier);
                    scope.spawn(move || {
                        let mut store = PostgresStore::connect_development(&uri).unwrap();
                        barrier.wait();
                        store.append_event(&event)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });

        for result in &results {
            assert!(
                result.is_ok(),
                "concurrent duplicate submission surfaced an error instead of an \
                 idempotent outcome: {result:?}"
            );
        }
        let outcomes: Vec<AppendOutcome> =
            results.into_iter().map(|result| result.unwrap()).collect();
        assert_eq!(
            outcomes[0].aggregate_sequence,
            outcomes[1].aggregate_sequence
        );
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.inserted).count(),
            1,
            "exactly one concurrent caller should report having performed the insert"
        );
    }
}
