CREATE TABLE IF NOT EXISTS tenants (
    tenant_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (tenant_id ~ '^[a-z0-9._-]+$')
);

CREATE TABLE IF NOT EXISTS domain_events (
    event_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    aggregate_sequence BIGINT NOT NULL CHECK (aggregate_sequence > 0),
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    correlation_id TEXT NOT NULL,
    causation_id TEXT,
    idempotency_key TEXT NOT NULL,
    payload_sha256 BYTEA NOT NULL CHECK (octet_length(payload_sha256) = 32),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, aggregate_type, aggregate_id, aggregate_sequence),
    UNIQUE (tenant_id, idempotency_key),
    CHECK (event_id ~ '^[a-z0-9._-]+$'),
    CHECK (aggregate_id ~ '^[a-z0-9._-]+$'),
    CHECK (correlation_id ~ '^[a-z0-9._-]+$')
);

CREATE TABLE IF NOT EXISTS outbox_messages (
    message_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    event_id TEXT NOT NULL UNIQUE REFERENCES domain_events(event_id),
    topic TEXT NOT NULL,
    payload JSONB NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    claimed_by TEXT,
    delivered_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK ((claimed_at IS NULL) = (claimed_by IS NULL))
);

CREATE INDEX IF NOT EXISTS outbox_delivery_idx
    ON outbox_messages (tenant_id, delivered_at, available_at, created_at);

CREATE TABLE IF NOT EXISTS projection_checkpoints (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    projection_name TEXT NOT NULL,
    last_event_id TEXT NOT NULL REFERENCES domain_events(event_id),
    last_recorded_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, projection_name)
);

CREATE TABLE IF NOT EXISTS journal_transactions (
    transaction_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    reference_id TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    idempotency_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, idempotency_key)
);

CREATE TABLE IF NOT EXISTS journal_lines (
    transaction_id TEXT NOT NULL REFERENCES journal_transactions(transaction_id),
    line_number INTEGER NOT NULL CHECK (line_number > 0),
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    account_id TEXT NOT NULL,
    currency CHAR(3) NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    debit NUMERIC(38,8) NOT NULL DEFAULT 0 CHECK (debit >= 0),
    credit NUMERIC(38,8) NOT NULL DEFAULT 0 CHECK (credit >= 0),
    PRIMARY KEY (transaction_id, line_number),
    CHECK ((debit > 0 AND credit = 0) OR (credit > 0 AND debit = 0))
);

CREATE OR REPLACE FUNCTION enforce_balanced_journal() RETURNS TRIGGER AS $$
DECLARE
    target_transaction_id TEXT;
    unbalanced_count BIGINT;
BEGIN
    target_transaction_id := COALESCE(NEW.transaction_id, OLD.transaction_id);
    SELECT COUNT(*) INTO unbalanced_count
    FROM (
        SELECT currency
        FROM journal_lines
        WHERE transaction_id = target_transaction_id
        GROUP BY currency
        HAVING SUM(debit) <> SUM(credit)
    ) unbalanced;
    IF unbalanced_count > 0 THEN
        RAISE EXCEPTION 'journal transaction % is not balanced by currency', target_transaction_id;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS journal_balance_constraint ON journal_lines;
CREATE CONSTRAINT TRIGGER journal_balance_constraint
AFTER INSERT OR UPDATE OR DELETE ON journal_lines
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_balanced_journal();

CREATE TABLE IF NOT EXISTS customer_users (
    user_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    normalized_email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    totp_secret_ciphertext BYTEA,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    security_version BIGINT NOT NULL DEFAULT 1 CHECK (security_version > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, normalized_email)
);

CREATE TABLE IF NOT EXISTS customer_user_roles (
    user_id TEXT NOT NULL REFERENCES customer_users(user_id) ON DELETE CASCADE,
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    role_name TEXT NOT NULL,
    PRIMARY KEY (user_id, role_name)
);

CREATE TABLE IF NOT EXISTS customer_sessions (
    token_sha256 BYTEA PRIMARY KEY CHECK (octet_length(token_sha256) = 32),
    user_id TEXT NOT NULL REFERENCES customer_users(user_id) ON DELETE CASCADE,
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    security_version BIGINT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS risk_policy_versions (
    policy_id TEXT NOT NULL,
    version BIGINT NOT NULL CHECK (version > 0),
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    document JSONB NOT NULL,
    document_sha256 BYTEA NOT NULL CHECK (octet_length(document_sha256) = 32),
    approved_by TEXT NOT NULL,
    effective_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, policy_id, version)
);

CREATE TABLE IF NOT EXISTS broker_commands (
    command_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    broker_account_id TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('PAPER', 'LIVE')),
    command_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    approval_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS broker_receipts (
    receipt_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    command_id TEXT NOT NULL REFERENCES broker_commands(command_id),
    broker_order_id TEXT,
    state TEXT NOT NULL,
    payload JSONB NOT NULL,
    received_at TIMESTAMPTZ NOT NULL
);

ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE domain_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE outbox_messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE projection_checkpoints ENABLE ROW LEVEL SECURITY;
ALTER TABLE journal_transactions ENABLE ROW LEVEL SECURITY;
ALTER TABLE journal_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE customer_users ENABLE ROW LEVEL SECURITY;
ALTER TABLE customer_user_roles ENABLE ROW LEVEL SECURITY;
ALTER TABLE customer_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE risk_policy_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE broker_commands ENABLE ROW LEVEL SECURITY;
ALTER TABLE broker_receipts ENABLE ROW LEVEL SECURITY;

ALTER TABLE tenants FORCE ROW LEVEL SECURITY;
ALTER TABLE domain_events FORCE ROW LEVEL SECURITY;
ALTER TABLE outbox_messages FORCE ROW LEVEL SECURITY;
ALTER TABLE projection_checkpoints FORCE ROW LEVEL SECURITY;
ALTER TABLE journal_transactions FORCE ROW LEVEL SECURITY;
ALTER TABLE journal_lines FORCE ROW LEVEL SECURITY;
ALTER TABLE customer_users FORCE ROW LEVEL SECURITY;
ALTER TABLE customer_user_roles FORCE ROW LEVEL SECURITY;
ALTER TABLE customer_sessions FORCE ROW LEVEL SECURITY;
ALTER TABLE risk_policy_versions FORCE ROW LEVEL SECURITY;
ALTER TABLE broker_commands FORCE ROW LEVEL SECURITY;
ALTER TABLE broker_receipts FORCE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS tenant_isolation ON tenants;
CREATE POLICY tenant_isolation ON tenants
USING (tenant_id = current_setting('app.tenant_id', TRUE))
WITH CHECK (tenant_id = current_setting('app.tenant_id', TRUE));

DO $$
DECLARE
    table_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'domain_events', 'outbox_messages', 'projection_checkpoints',
        'journal_transactions', 'journal_lines', 'customer_users',
        'customer_user_roles', 'customer_sessions', 'risk_policy_versions',
        'broker_commands', 'broker_receipts'
    ]
    LOOP
        EXECUTE format('DROP POLICY IF EXISTS tenant_isolation ON %I', table_name);
        EXECUTE format(
            'CREATE POLICY tenant_isolation ON %I USING (tenant_id = current_setting(''app.tenant_id'', TRUE)) WITH CHECK (tenant_id = current_setting(''app.tenant_id'', TRUE))',
            table_name
        );
    END LOOP;
END;
$$;
