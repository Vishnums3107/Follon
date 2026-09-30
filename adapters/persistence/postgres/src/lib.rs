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
    (
        6,
        include_str!("../migrations/0006_evidence_immutability.sql"),
    ),
    (
        7,
        include_str!("../migrations/0007_evidence_child_commit_guard.sql"),
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
        self.migrate_through(i64::MAX)
    }

    /// Applies the embedded migrations up to and including `last_version`, which
    /// lets a test build the schema an older release left behind and upgrade it.
    fn migrate_through(&mut self, last_version: i64) -> Result<(), PersistenceError> {
        let mut transaction = self.client.transaction()?;
        transaction.batch_execute(
            "SELECT pg_advisory_xact_lock(6433752855195311);\
             CREATE TABLE IF NOT EXISTS follon_schema_migrations (\
               version BIGINT PRIMARY KEY,\
               sha256 BYTEA NOT NULL CHECK (octet_length(sha256) = 32),\
               applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()\
             );",
        )?;
        for (version, sql) in MIGRATIONS
            .iter()
            .filter(|(version, _)| *version <= last_version)
        {
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
            "refuse_evidence_mutation",
            "refuse_version_rewrite",
            "BEFORE UPDATE OR DELETE",
            "BEFORE TRUNCATE",
            "news_sentiments_tenant_news_fkey",
            "news rows exist that predate tenant ownership",
            "ENABLE ALWAYS TRIGGER refuse_mutation",
            "ENABLE ALWAYS TRIGGER refuse_truncate",
            "'infinity'::timestamptz",
            "capture_evidence_parent_xact",
            "refuse_late_evidence_child",
            "ENABLE ALWAYS TRIGGER guard_parent_commit",
        ] {
            assert!(migration_sql.contains(required), "missing {required}");
        }
        assert_eq!(MIGRATIONS.len(), 7);
        // Versions are consecutive, so a migration can be neither skipped nor reordered.
        for (index, (version, _)) in MIGRATIONS.iter().enumerate() {
            assert_eq!(*version, i64::try_from(index).unwrap() + 1);
        }
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
        // Its own tenant and identifiers, because domain events cannot be deleted and a
        // rerun against the same database must not find last run's.
        let mut store = connected_store();
        let event = sample_event_for(&run_tag("round-trip"));
        store.provision_tenant(&event.tenant_id, "ACME").unwrap();
        let first = store.append_event(&event).unwrap();
        let second = store.append_event(&event).unwrap();
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
        let mut store = connected_store();
        let tag = run_tag("race-recovery");
        let event = sample_event_for(&tag);
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
        conflicting.event_id = format!("event.{tag}-2");
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
        let mut setup_store = connected_store();
        let event = sample_event_for(&run_tag("concurrent-race"));
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

    // Retained evidence and news tenancy (delivery state E7.7).
    //
    // Retained evidence cannot be deleted, so the rows these tests write outlive them. Each
    // run therefore gets its own tenant and identifiers from `run_tag`, and the tests are
    // for a disposable database, as the ignore attribute says.

    const APPEND_ONLY: [&str; 16] = [
        "domain_events",
        "journal_transactions",
        "journal_lines",
        "risk_policy_versions",
        "broker_commands",
        "broker_receipts",
        "strategy_versions",
        "configuration_versions",
        "audit_event_indexes",
        "news_headlines",
        "news_sentiments",
        "fx_pricing_snapshots",
        "venue_capabilities",
        "execution_plan_evidence",
        "execution_route_decisions",
        "execution_benchmark_evidence",
    ];
    const VERSIONED: [&str; 2] = [
        "instrument_reference_versions",
        "fx_instrument_economics_versions",
    ];
    const ISOLATION_ROLE: &str = "follon_isolation_test";
    const SHA256_ZERO: &str = "decode(repeat('00', 32), 'hex')";

    fn run_tag(name: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{name}-{}-{nanos}", std::process::id())
    }

    /// One database test at a time. `TRUNCATE ... CASCADE`, which a test here runs on purpose
    /// to see it refused, takes an exclusive lock on every table it reaches, so it deadlocks
    /// with a test that is writing to any of them at that moment. A review saw it fail about
    /// one run in four when the tests ran in parallel, as `cargo test` does by default.
    static DATABASE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A connected, migrated store that holds the database for as long as it lives.
    struct LockedStore {
        store: PostgresStore,
        _exclusive: std::sync::MutexGuard<'static, ()>,
    }

    impl std::ops::Deref for LockedStore {
        type Target = PostgresStore;

        fn deref(&self) -> &PostgresStore {
            &self.store
        }
    }

    impl std::ops::DerefMut for LockedStore {
        fn deref_mut(&mut self) -> &mut PostgresStore {
            &mut self.store
        }
    }

    fn connected_store() -> LockedStore {
        // A test that panicked while holding the database poisons the lock, and the tests
        // after it are no less able to run.
        let exclusive = DATABASE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let uri = std::env::var("FOLLON_TEST_DATABASE_URL").expect("database URL");
        let mut store = PostgresStore::connect_development(&uri).unwrap();
        store.migrate().unwrap();
        LockedStore {
            store,
            _exclusive: exclusive,
        }
    }

    /// Runs `sql` as `tenant` and commits it.
    fn commit_as_tenant(store: &mut PostgresStore, tenant: &str, sql: &str) {
        let mut transaction = store.client.transaction().unwrap();
        set_tenant(&mut transaction, tenant).unwrap();
        transaction.batch_execute(sql).unwrap();
        transaction.commit().unwrap();
    }

    /// Runs `sql` as `tenant`, rolls it back whatever it did, and returns the database's
    /// refusal if there was one. Nothing an accepted statement did survives, so a
    /// guard that fails to refuse cannot damage the rows the test still needs.
    fn refusal(
        store: &mut PostgresStore,
        tenant: &str,
        role: Option<&str>,
        sql: &str,
    ) -> Option<String> {
        let mut transaction = store.client.transaction().unwrap();
        if let Some(role) = role {
            transaction
                .batch_execute(&format!("SET LOCAL ROLE {role}"))
                .unwrap();
        }
        set_tenant(&mut transaction, tenant).unwrap();
        transaction
            .batch_execute(sql)
            .err()
            .map(|error| PersistenceError::from(error).0)
    }

    /// How many rows a query returns as `tenant`, under `role` when given.
    fn visible_rows(
        store: &mut PostgresStore,
        tenant: &str,
        role: Option<&str>,
        query: &str,
    ) -> i64 {
        let mut transaction = store.client.transaction().unwrap();
        if let Some(role) = role {
            transaction
                .batch_execute(&format!("SET LOCAL ROLE {role}"))
                .unwrap();
        }
        set_tenant(&mut transaction, tenant).unwrap();
        transaction.query_one(query, &[]).unwrap().get(0)
    }

    /// One row in each append-only table, all owned by `tenant`, and the user the
    /// approvals name. `event_id` is the domain event `append_event` already wrote.
    fn evidence_rows_sql(tenant: &str, event_id: &str, tag: &str) -> String {
        format!(
            "INSERT INTO customer_users (user_id, tenant_id, normalized_email, password_hash) \
               VALUES ('user.{tag}', '{tenant}', 'user.{tag}@example.test', 'hash'); \
             INSERT INTO risk_policy_versions (policy_id, version, tenant_id, document, document_sha256, approved_by, effective_at) \
               VALUES ('policy.{tag}', 1, '{tenant}', '{{}}', {SHA256_ZERO}, 'user.{tag}', NOW()); \
             INSERT INTO strategy_versions (tenant_id, strategy_id, version, bundle_sha256, runtime_contract_version, metadata, created_by) \
               VALUES ('{tenant}', 'strategy.{tag}', 1, {SHA256_ZERO}, 'v1', '{{}}', 'user.{tag}'); \
             INSERT INTO configuration_versions (tenant_id, configuration_id, version, document, document_sha256, approved_by, effective_at) \
               VALUES ('{tenant}', 'configuration.{tag}', 1, '{{}}', {SHA256_ZERO}, 'user.{tag}', NOW()); \
             INSERT INTO audit_event_indexes (tenant_id, event_id, event_type, occurred_at) \
               VALUES ('{tenant}', '{event_id}', 'order.accepted', NOW()); \
             INSERT INTO journal_transactions (transaction_id, tenant_id, reference_id, occurred_at, idempotency_key) \
               VALUES ('journal.{tag}', '{tenant}', 'reference.{tag}', NOW(), 'idempotency.journal.{tag}'); \
             INSERT INTO journal_lines (transaction_id, line_number, tenant_id, account_id, currency, debit, credit) \
               VALUES ('journal.{tag}', 1, '{tenant}', 'cash', 'USD', 10, 0), ('journal.{tag}', 2, '{tenant}', 'equity', 'USD', 0, 10); \
             INSERT INTO broker_commands (command_id, tenant_id, broker_account_id, mode, command_type, payload) \
               VALUES ('command.{tag}', '{tenant}', 'account.{tag}', 'PAPER', 'submit', '{{}}'); \
             INSERT INTO broker_receipts (receipt_id, tenant_id, command_id, state, payload, received_at) \
               VALUES ('receipt.{tag}', '{tenant}', 'command.{tag}', 'ACCEPTED', '{{}}', NOW()); \
             INSERT INTO news_headlines (tenant_id, news_id, source, headline, raw_body_hash, sequence_number, event_time_ns, receive_time_ns) \
               VALUES ('{tenant}', 'news.{tag}', 'wire', 'headline', repeat('a', 64), 1, 1, 2); \
             INSERT INTO news_sentiments (tenant_id, event_id, causation_news_id, instrument_id, taxonomy, sentiment_polarity_bps, confidence_bps, novelty_score_bps, surprise_magnitude_bps, event_time_ns) \
               VALUES ('{tenant}', 'sentiment.{tag}', 'news.{tag}', 'inst.us_equity.spy', 'taxonomy.one', 0, 0, 0, 0, 1); \
             INSERT INTO fx_pricing_snapshots (tenant_id, snapshot_id, instrument_id, reference_version, product, base_currency, quote_currency, source_id, source_sequence, source_time, received_at, terms, terms_sha256) \
               VALUES ('{tenant}', 'snapshot.{tag}', 'inst.fx.eurusd', 'reference.one', 'FX_SPOT', 'EUR', 'USD', 'source.one', 0, NOW() - INTERVAL '1 minute', NOW(), '{{}}', {SHA256_ZERO}); \
             INSERT INTO venue_capabilities (tenant_id, venue, capability_version, supported_order_kinds, document_sha256) \
               VALUES ('{tenant}', 'venue.{tag}', 'v1', '[]', {SHA256_ZERO}); \
             INSERT INTO execution_plan_evidence (tenant_id, evidence_id, parent_order_id, account_id, instrument_id, side, parent_quantity, algorithm, children, unallocated_quantity, plan_sha256) \
               VALUES ('{tenant}', 'plan.{tag}', 'order.{tag}', 'account.{tag}', 'inst.us_equity.spy', 'BUY', '10', 'twap', '[]', '0', {SHA256_ZERO}); \
             INSERT INTO execution_route_decisions (tenant_id, decision_id, evidence_id, parent_order_id, venue, capability_version, allocated_quantity, all_in_price, fee_per_unit, latency_rank, decision_sha256) \
               VALUES ('{tenant}', 'decision.{tag}', 'plan.{tag}', 'order.{tag}', 'venue.{tag}', 'v1', '10', '100', '0.01', 0, {SHA256_ZERO}); \
             INSERT INTO execution_benchmark_evidence (tenant_id, benchmark_id, evidence_id, parent_order_id, arrival_price, target_price, source, benchmark_sha256) \
               VALUES ('{tenant}', 'benchmark.{tag}', 'plan.{tag}', 'order.{tag}', '100', '101', 'source.one', {SHA256_ZERO});"
        )
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn evidence_children_must_be_inserted_in_their_parents_transaction() {
        let mut store = connected_store();
        let tag = run_tag("child-transaction");
        let tenant = format!("tenant.{tag}");
        store
            .provision_tenant(&tenant, "Child transaction test")
            .unwrap();

        // The legitimate writer creates each parent and its complete child set
        // in one transaction. A savepoint still belongs to that transaction.
        commit_as_tenant(
            &mut store,
            &tenant,
            &format!(
                "INSERT INTO journal_transactions (transaction_id, tenant_id, reference_id, occurred_at, idempotency_key) \
                   VALUES ('journal.{tag}', '{tenant}', 'reference.{tag}', NOW(), 'idempotency.{tag}'); \
                 SAVEPOINT child_insert; \
                 INSERT INTO journal_lines (transaction_id, line_number, tenant_id, account_id, currency, debit, credit) \
                   VALUES ('journal.{tag}', 1, '{tenant}', 'cash', 'USD', 10, 0), \
                          ('journal.{tag}', 2, '{tenant}', 'equity', 'USD', 0, 10); \
                 RELEASE SAVEPOINT child_insert; \
                 INSERT INTO execution_plan_evidence (tenant_id, evidence_id, parent_order_id, account_id, instrument_id, side, parent_quantity, algorithm, children, unallocated_quantity, plan_sha256) \
                   VALUES ('{tenant}', 'plan.{tag}', 'order.{tag}', 'account.{tag}', 'inst.us_equity.spy', 'BUY', '10', 'twap', '[]', '0', {SHA256_ZERO}); \
                 INSERT INTO execution_route_decisions (tenant_id, decision_id, evidence_id, parent_order_id, venue, capability_version, allocated_quantity, all_in_price, fee_per_unit, latency_rank, decision_sha256) \
                   VALUES ('{tenant}', 'decision.{tag}', 'plan.{tag}', 'order.{tag}', 'venue.one', 'v1', '10', '100', '0.01', 0, {SHA256_ZERO}); \
                 INSERT INTO execution_benchmark_evidence (tenant_id, benchmark_id, evidence_id, parent_order_id, arrival_price, target_price, source, benchmark_sha256) \
                   VALUES ('{tenant}', 'benchmark.{tag}', 'plan.{tag}', 'order.{tag}', '100', '101', 'source.one', {SHA256_ZERO});"
            ),
        );

        for (table, sql) in [
            (
                "journal_lines",
                format!(
                    "INSERT INTO journal_lines (transaction_id, line_number, tenant_id, account_id, currency, debit, credit) \
                     VALUES ('journal.{tag}', 3, '{tenant}', 'cash', 'USD', 5, 0), \
                            ('journal.{tag}', 4, '{tenant}', 'equity', 'USD', 0, 5)"
                ),
            ),
            (
                "execution_route_decisions",
                format!(
                    "INSERT INTO execution_route_decisions (tenant_id, decision_id, evidence_id, parent_order_id, venue, capability_version, allocated_quantity, all_in_price, fee_per_unit, latency_rank, decision_sha256) \
                     VALUES ('{tenant}', 'late.decision.{tag}', 'plan.{tag}', 'order.{tag}', 'venue.two', 'v1', '1', '100', '0.01', 0, {SHA256_ZERO})"
                ),
            ),
            (
                "execution_benchmark_evidence",
                format!(
                    "INSERT INTO execution_benchmark_evidence (tenant_id, benchmark_id, evidence_id, parent_order_id, arrival_price, target_price, source, benchmark_sha256) \
                     VALUES ('{tenant}', 'late.benchmark.{tag}', 'plan.{tag}', 'other.order.{tag}', '100', '101', 'source.one', {SHA256_ZERO})"
                ),
            ),
        ] {
            let error = refusal(&mut store, &tenant, None, &sql)
                .unwrap_or_else(|| panic!("{table} gained a child after its parent committed"));
            assert!(error.contains("must be inserted with its parent"), "{error}");
        }

        let bypass = format!(
            "SET LOCAL session_replication_role = replica; \
             INSERT INTO journal_lines (transaction_id, line_number, tenant_id, account_id, currency, debit, credit) \
             VALUES ('journal.{tag}', 3, '{tenant}', 'cash', 'USD', 5, 0), \
                    ('journal.{tag}', 4, '{tenant}', 'equity', 'USD', 0, 5)"
        );
        let error = refusal(&mut store, &tenant, None, &bypass)
            .expect("replica mode bypassed the child guard");
        assert!(
            error.contains("must be inserted with its parent"),
            "{error}"
        );
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn preexisting_parents_cannot_gain_children_after_migration() {
        let uri = std::env::var("FOLLON_TEST_DATABASE_URL").expect("database URL");
        let config: postgres::Config = uri.parse().expect("the database URL parses");
        let scratch = format!("follon_upgrade_{}", run_tag("children").replace('-', "_"));
        let mut admin = {
            let mut config = config.clone();
            config.dbname("postgres");
            config.connect(NoTls).unwrap()
        };
        admin
            .batch_execute(&format!("CREATE DATABASE {scratch}"))
            .unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut scratch_config = config.clone();
            scratch_config.dbname(&scratch);
            let mut store = PostgresStore {
                client: scratch_config.connect(NoTls).unwrap(),
            };
            store.migrate_through(6).unwrap();
            store
                .provision_tenant("tenant.upgrade", "Upgrade test")
                .unwrap();
            commit_as_tenant(
                &mut store,
                "tenant.upgrade",
                &format!(
                    "INSERT INTO journal_transactions (transaction_id, tenant_id, reference_id, occurred_at, idempotency_key) \
                     VALUES ('journal.upgrade', 'tenant.upgrade', 'reference.upgrade', NOW(), 'idempotency.upgrade'); \
                     INSERT INTO execution_plan_evidence (tenant_id, evidence_id, parent_order_id, account_id, instrument_id, side, parent_quantity, algorithm, children, unallocated_quantity, plan_sha256) \
                     VALUES ('tenant.upgrade', 'plan.upgrade', 'order.upgrade', 'account.upgrade', 'inst.us_equity.spy', 'BUY', '10', 'twap', '[]', '0', {SHA256_ZERO})"
                ),
            );
            store.migrate().unwrap();
            for sql in [
                "INSERT INTO journal_lines (transaction_id, line_number, tenant_id, account_id, currency, debit, credit) \
                 VALUES ('journal.upgrade', 1, 'tenant.upgrade', 'cash', 'USD', 10, 0), \
                        ('journal.upgrade', 2, 'tenant.upgrade', 'equity', 'USD', 0, 10)".to_owned(),
                format!(
                    "INSERT INTO execution_route_decisions (tenant_id, decision_id, evidence_id, parent_order_id, venue, capability_version, allocated_quantity, all_in_price, fee_per_unit, latency_rank, decision_sha256) \
                     VALUES ('tenant.upgrade', 'decision.upgrade', 'plan.upgrade', 'order.upgrade', 'venue.one', 'v1', '10', '100', '0.01', 0, {SHA256_ZERO})"
                ),
            ] {
                let error = refusal(&mut store, "tenant.upgrade", None, &sql)
                    .expect("a pre-migration parent gained a child");
                assert!(error.contains("must be inserted with its parent"), "{error}");
            }
        }));
        admin
            .batch_execute(&format!("DROP DATABASE IF EXISTS {scratch} WITH (FORCE)"))
            .unwrap();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn every_evidence_table_carries_both_guards() {
        let mut store = connected_store();
        for table in APPEND_ONLY.iter().chain(VERSIONED.iter()) {
            let triggers: Vec<(String, String)> = store
                .client
                .query(
                    "SELECT tgname, tgenabled::text FROM pg_trigger \
                     WHERE tgrelid = $1::text::regclass AND NOT tgisinternal ORDER BY tgname",
                    &[table],
                )
                .unwrap()
                .iter()
                .map(|row| (row.get(0), row.get(1)))
                .collect();
            for guard in ["refuse_mutation", "refuse_truncate"] {
                let state = triggers
                    .iter()
                    .find(|(name, _)| name == guard)
                    .unwrap_or_else(|| panic!("{table} has no {guard} trigger: {triggers:?}"));
                // 'A' fires in every session role. The default, 'O', is switched off by
                // `SET session_replication_role = replica`, which is no DDL and no log line.
                assert_eq!(
                    state.1, "A",
                    "{table}'s {guard} can be switched off by a SET"
                );
            }
        }
    }

    /// The guards must survive a session that says it is a replica, which is what a bulk
    /// loader does and what turns off an ordinary trigger. Only a superuser, or a role
    /// granted the setting, can say it, so where the connection is neither, the state check
    /// in `every_evidence_table_carries_both_guards` is the whole proof.
    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn the_guards_hold_in_a_session_that_says_it_is_a_replica() {
        let mut store = connected_store();
        let tag = run_tag("replica");
        let event = sample_event_for(&tag);
        store.provision_tenant(&event.tenant_id, "Replica").unwrap();
        store.append_event(&event).unwrap();
        let mut said_it = false;
        for statement in [
            format!(
                "UPDATE domain_events SET payload = '{{}}' WHERE tenant_id = '{}'",
                event.tenant_id
            ),
            format!(
                "DELETE FROM domain_events WHERE tenant_id = '{}'",
                event.tenant_id
            ),
            "TRUNCATE domain_events CASCADE".to_owned(),
        ] {
            let mut transaction = store.client.transaction().unwrap();
            if transaction
                .batch_execute("SET LOCAL session_replication_role = replica")
                .is_err()
            {
                return;
            }
            said_it = true;
            set_tenant(&mut transaction, &event.tenant_id).unwrap();
            let error = transaction
                .batch_execute(&statement)
                .err()
                .map(|error| PersistenceError::from(error).0)
                .unwrap_or_else(|| panic!("{statement} was accepted as a replica"));
            assert!(
                error.contains("[23001]") && error.contains("retained evidence is append-only"),
                "{statement}: {error}"
            );
        }
        assert!(said_it);
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn retained_evidence_refuses_update_delete_and_truncate() {
        let mut store = connected_store();
        let tag = run_tag("immutable");
        let event = sample_event_for(&tag);
        store
            .provision_tenant(&event.tenant_id, "Evidence")
            .unwrap();
        store.append_event(&event).unwrap();
        commit_as_tenant(
            &mut store,
            &event.tenant_id,
            &evidence_rows_sql(&event.tenant_id, &event.event_id, &tag),
        );

        for table in APPEND_ONLY {
            // Scoped to this run's own rows, so it is the row this test wrote that is
            // protected and not one an earlier run left behind.
            let mine = format!("WHERE tenant_id = '{}'", event.tenant_id);
            let attempts = [
                (
                    "UPDATE",
                    format!("UPDATE {table} SET tenant_id = tenant_id {mine}"),
                ),
                ("DELETE", format!("DELETE FROM {table} {mine}")),
                ("TRUNCATE", format!("TRUNCATE {table} CASCADE")),
            ];
            for (verb, statement) in attempts {
                let error = refusal(&mut store, &event.tenant_id, None, &statement)
                    .unwrap_or_else(|| panic!("{statement} was accepted"));
                assert!(
                    error.contains("[23001]")
                        && error.contains(&format!("{verb} on "))
                        && error.contains("retained evidence is append-only"),
                    "{statement}: {error}"
                );
                if verb != "TRUNCATE" {
                    // The refusal names the table it guards.
                    assert!(
                        error.contains(&format!("{verb} on {table} is refused")),
                        "{statement}: {error}"
                    );
                }
            }
            let kept = visible_rows(
                &mut store,
                &event.tenant_id,
                None,
                &format!(
                    "SELECT count(*) FROM {table} WHERE tenant_id = '{}'",
                    event.tenant_id
                ),
            );
            assert!(kept >= 1, "{table} lost its row");
        }
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn a_reference_version_may_only_be_closed_once_and_is_otherwise_fixed() {
        let mut store = connected_store();
        let tag = run_tag("versioned");
        let tenant = format!("tenant.{tag}");
        store.provision_tenant(&tenant, "Versions").unwrap();
        let cases = [
            (
                "instrument_reference_versions",
                format!(
                    "INSERT INTO instrument_reference_versions (tenant_id, instrument_id, version, document, document_sha256, effective_from) \
                     VALUES ('{tenant}', 'inst.{tag}', 1, '{{\"tick\": 0.10}}', {SHA256_ZERO}, NOW() - INTERVAL '1 day')"
                ),
                "document = '{\"changed\": true}'",
                // The same number, written with another scale: equal to jsonb, not the same document.
                "document = '{\"tick\": 0.1}'",
            ),
            (
                "fx_instrument_economics_versions",
                format!(
                    "INSERT INTO fx_instrument_economics_versions (tenant_id, instrument_id, version, product, base_currency, quote_currency, terms, document_sha256, effective_from) \
                     VALUES ('{tenant}', 'inst.{tag}', 1, 'FX_SPOT', 'EUR', 'USD', '{{\"lot\": 1.000}}', {SHA256_ZERO}, NOW() - INTERVAL '1 day')"
                ),
                "terms = '{\"changed\": true}'",
                "terms = '{\"lot\": 1.0}'",
            ),
        ];
        // Every statement is scoped to this run's tenant, so it is this run's version that is
        // tested, and not one an earlier run left closed.
        let mine = format!("WHERE tenant_id = '{tenant}'");
        let version_message = "a version may only be closed, once";
        let evidence_message = "retained evidence is append-only";
        for (table, insert, rewrite, rescale) in cases {
            commit_as_tenant(&mut store, &tenant, &insert);
            let close = format!("UPDATE {table} SET effective_to = NOW() {mine}");
            let refused = |store: &mut PostgresStore, statement: &str, message: &str| {
                let error = refusal(store, &tenant, None, statement)
                    .unwrap_or_else(|| panic!("{table}: {statement} was accepted"));
                assert!(
                    error.contains("[23001]") && error.contains(message),
                    "{table}: {statement}: {error}"
                );
            };

            // While it is open, rewriting a version, alone or as it is closed, and
            // deleting it are refused, and so is closing it with no end, which would spend
            // the one close. Rewriting only the scale of a number as it is closed is a
            // rewrite too. Truncating the table is refused as well.
            for statement in [
                format!("UPDATE {table} SET {rewrite} {mine}"),
                format!("UPDATE {table} SET effective_to = NOW(), {rewrite} {mine}"),
                format!("UPDATE {table} SET effective_to = NOW(), {rescale} {mine}"),
                format!("UPDATE {table} SET effective_to = 'infinity' {mine}"),
                format!("DELETE FROM {table} {mine}"),
            ] {
                refused(&mut store, &statement, version_message);
            }
            refused(
                &mut store,
                &format!("TRUNCATE {table} CASCADE"),
                evidence_message,
            );

            // Closing it is allowed, once, and it is fixed after that.
            commit_as_tenant(&mut store, &tenant, &close);
            for statement in [
                close.clone(),
                format!("UPDATE {table} SET effective_to = effective_to + INTERVAL '1 day' {mine}"),
                format!("UPDATE {table} SET {rewrite} {mine}"),
                format!("DELETE FROM {table} {mine}"),
            ] {
                refused(&mut store, &statement, version_message);
            }
        }
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn news_is_owned_by_one_tenant_and_invisible_to_the_rest() {
        let mut store = connected_store();
        let tag = run_tag("news");
        let (owner, other) = (format!("tenant.{tag}-a"), format!("tenant.{tag}-b"));
        store.provision_tenant(&owner, "Owner").unwrap();
        store.provision_tenant(&other, "Other").unwrap();
        let news_id = format!("news.{tag}");
        let headline = |tenant: &str| {
            format!(
                "INSERT INTO news_headlines (tenant_id, news_id, source, headline, raw_body_hash, sequence_number, event_time_ns, receive_time_ns) \
                 VALUES ('{tenant}', '{news_id}', 'wire', 'headline', repeat('a', 64), 1, 1, 2)"
            )
        };
        let sentiment = |tenant: &str, cause: &str| {
            format!(
                "INSERT INTO news_sentiments (tenant_id, event_id, causation_news_id, instrument_id, taxonomy, sentiment_polarity_bps, confidence_bps, novelty_score_bps, surprise_magnitude_bps, event_time_ns) \
                 VALUES ('{tenant}', 'sentiment.{tag}', '{cause}', 'inst.us_equity.spy', 'taxonomy.one', 0, 0, 0, 0, 1)"
            )
        };
        commit_as_tenant(
            &mut store,
            &owner,
            &format!("{}; {}", headline(&owner), sentiment(&owner, &news_id)),
        );

        // The database's owner and its superusers bypass row-level security, as they would
        // for the application's role only if it were misconfigured. This role does not.
        store
            .client
            .batch_execute(&format!(
                "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{ISOLATION_ROLE}') \
                   THEN CREATE ROLE {ISOLATION_ROLE} NOLOGIN; END IF; END $$; \
                 GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {ISOLATION_ROLE};"
            ))
            .unwrap();
        let role = Some(ISOLATION_ROLE);
        let count = format!("SELECT count(*) FROM news_headlines WHERE news_id = '{news_id}'");

        let sentiments =
            format!("SELECT count(*) FROM news_sentiments WHERE event_id = 'sentiment.{tag}'");

        assert_eq!(visible_rows(&mut store, &owner, role, &count), 1);
        assert_eq!(visible_rows(&mut store, &owner, role, &sentiments), 1);
        assert_eq!(
            visible_rows(&mut store, &other, role, &count),
            0,
            "another tenant can see the headline"
        );
        assert_eq!(
            visible_rows(&mut store, &other, role, &sentiments),
            0,
            "another tenant can see the sentiment"
        );

        // A row must belong to a tenant that exists.
        let stranger = format!("tenant.{tag}-none");
        let error = refusal(&mut store, &stranger, role, &headline(&stranger))
            .expect("a headline for a tenant that does not exist was accepted");
        assert!(error.contains("foreign key"), "{error}");

        // Another tenant cannot write a row for the owner.
        let error = refusal(&mut store, &other, role, &headline(&owner))
            .expect("a row for another tenant was accepted");
        assert!(error.contains("row-level security"), "{error}");

        // The same identifier is a different headline for each tenant.
        assert_eq!(refusal(&mut store, &other, role, &headline(&other)), None);

        // A sentiment cannot be caused by another tenant's headline, but can by its own.
        let error = refusal(&mut store, &other, role, &sentiment(&other, &news_id))
            .expect("a sentiment on another tenant's headline was accepted");
        assert!(error.contains("foreign key"), "{error}");
        let own = format!("{}; {}", headline(&other), sentiment(&other, &news_id));
        assert_eq!(refusal(&mut store, &other, role, &own), None);
    }

    #[test]
    #[ignore = "requires FOLLON_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    fn the_migration_refuses_news_rows_it_cannot_assign_to_a_tenant() {
        let uri = std::env::var("FOLLON_TEST_DATABASE_URL").expect("database URL");
        let config: postgres::Config = uri.parse().expect("the database URL parses");
        let scratch = format!("follon_upgrade_{}", run_tag("news").replace('-', "_"));
        let mut admin = {
            let mut config = config.clone();
            config.dbname("postgres");
            config.connect(NoTls).unwrap()
        };
        admin
            .batch_execute(&format!("CREATE DATABASE {scratch}"))
            .unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut scratch_config = config.clone();
            scratch_config.dbname(&scratch);
            let mut store = PostgresStore {
                client: scratch_config.connect(NoTls).unwrap(),
            };

            // The schema an earlier release left behind, with a headline nobody owns.
            store.migrate_through(5).unwrap();
            let recorded: Vec<i64> = store
                .client
                .query(
                    "SELECT version FROM follon_schema_migrations ORDER BY version",
                    &[],
                )
                .unwrap()
                .iter()
                .map(|row| row.get(0))
                .collect();
            assert_eq!(recorded, vec![1, 2, 3, 4, 5]);
            store
                .client
                .batch_execute(
                    "INSERT INTO news_headlines (news_id, source, headline, raw_body_hash, sequence_number, event_time_ns, receive_time_ns) \
                     VALUES ('news.unowned-1', 'wire', 'headline', repeat('a', 64), 1, 1, 2); \
                     INSERT INTO news_sentiments (event_id, causation_news_id, instrument_id, taxonomy, sentiment_polarity_bps, confidence_bps, novelty_score_bps, surprise_magnitude_bps, event_time_ns) \
                     VALUES ('sentiment.unowned-1', 'news.unowned-1', 'inst.us_equity.spy', 'taxonomy.one', 0, 0, 0, 0, 1)",
                )
                .unwrap();

            // The upgrade refuses to guess an owner, leaves nothing half done, and says what
            // the operator can do, which is the one thing that is possible before the
            // migration has given the tables a tenant to assign.
            let error = store.migrate().unwrap_err();
            assert!(
                error
                    .0
                    .contains("news rows exist that predate tenant ownership")
                    && error
                        .0
                        .contains("empty news_sentiments and then news_headlines"),
                "{}",
                error.0
            );
            let applied: i64 = store
                .client
                .query_one(
                    "SELECT count(*) FROM follon_schema_migrations WHERE version = 6",
                    &[],
                )
                .unwrap()
                .get(0);
            assert_eq!(applied, 0);
            let columns: i64 = store
                .client
                .query_one(
                    "SELECT count(*) FROM information_schema.columns \
                     WHERE table_name = 'news_headlines' AND column_name = 'tenant_id'",
                    &[],
                )
                .unwrap()
                .get(0);
            assert_eq!(columns, 0, "the refused upgrade still added the column");

            // Once the operator has done what the message says, in the order it says, the same
            // upgrade goes through. The sentiment goes first because it names the headline.
            store
                .client
                .batch_execute("DELETE FROM news_sentiments; DELETE FROM news_headlines")
                .unwrap();
            store.migrate().unwrap();
            let columns: i64 = store
                .client
                .query_one(
                    "SELECT count(*) FROM information_schema.columns \
                     WHERE table_name IN ('news_headlines', 'news_sentiments') \
                       AND column_name = 'tenant_id'",
                    &[],
                )
                .unwrap()
                .get(0);
            assert_eq!(columns, 2);
        }));
        admin
            .batch_execute(&format!("DROP DATABASE IF EXISTS {scratch} WITH (FORCE)"))
            .unwrap();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
}
