-- Add composite parent keys before replacing v1 single-column references.
-- Global identifiers remain unique, while these constraints also make tenant
-- ownership part of every relationship enforced by PostgreSQL.
ALTER TABLE domain_events
    ADD CONSTRAINT domain_events_tenant_event_id_key UNIQUE (tenant_id, event_id);
ALTER TABLE journal_transactions
    ADD CONSTRAINT journal_transactions_tenant_transaction_id_key UNIQUE (tenant_id, transaction_id);
ALTER TABLE customer_users
    ADD CONSTRAINT customer_users_tenant_user_id_key UNIQUE (tenant_id, user_id);
ALTER TABLE broker_commands
    ADD CONSTRAINT broker_commands_tenant_command_id_key UNIQUE (tenant_id, command_id);

ALTER TABLE outbox_messages DROP CONSTRAINT outbox_messages_event_id_fkey;
ALTER TABLE outbox_messages
    ADD CONSTRAINT outbox_messages_tenant_event_fkey
    FOREIGN KEY (tenant_id, event_id) REFERENCES domain_events(tenant_id, event_id);
ALTER TABLE projection_checkpoints DROP CONSTRAINT projection_checkpoints_last_event_id_fkey;
ALTER TABLE projection_checkpoints
    ADD CONSTRAINT projection_checkpoints_tenant_event_fkey
    FOREIGN KEY (tenant_id, last_event_id) REFERENCES domain_events(tenant_id, event_id);
ALTER TABLE journal_lines DROP CONSTRAINT journal_lines_transaction_id_fkey;
ALTER TABLE journal_lines
    ADD CONSTRAINT journal_lines_tenant_transaction_fkey
    FOREIGN KEY (tenant_id, transaction_id)
    REFERENCES journal_transactions(tenant_id, transaction_id);
ALTER TABLE customer_user_roles DROP CONSTRAINT customer_user_roles_user_id_fkey;
ALTER TABLE customer_user_roles
    ADD CONSTRAINT customer_user_roles_tenant_user_fkey
    FOREIGN KEY (tenant_id, user_id)
    REFERENCES customer_users(tenant_id, user_id) ON DELETE CASCADE;
ALTER TABLE customer_sessions DROP CONSTRAINT customer_sessions_user_id_fkey;
ALTER TABLE customer_sessions
    ADD CONSTRAINT customer_sessions_tenant_user_fkey
    FOREIGN KEY (tenant_id, user_id)
    REFERENCES customer_users(tenant_id, user_id) ON DELETE CASCADE;
ALTER TABLE broker_receipts DROP CONSTRAINT broker_receipts_command_id_fkey;
ALTER TABLE broker_receipts
    ADD CONSTRAINT broker_receipts_tenant_command_fkey
    FOREIGN KEY (tenant_id, command_id)
    REFERENCES broker_commands(tenant_id, command_id);
ALTER TABLE risk_policy_versions
    ADD CONSTRAINT risk_policy_versions_tenant_approver_fkey
    FOREIGN KEY (tenant_id, approved_by)
    REFERENCES customer_users(tenant_id, user_id);

ALTER TABLE domain_events
    ADD CONSTRAINT domain_events_payload_object_check
    CHECK (jsonb_typeof(payload) = 'object');
ALTER TABLE outbox_messages
    ADD CONSTRAINT outbox_messages_payload_object_check
    CHECK (jsonb_typeof(payload) = 'object');
ALTER TABLE broker_commands
    ADD CONSTRAINT broker_commands_payload_object_check
    CHECK (jsonb_typeof(payload) = 'object');
ALTER TABLE broker_receipts
    ADD CONSTRAINT broker_receipts_payload_object_check
    CHECK (jsonb_typeof(payload) = 'object');

CREATE TABLE IF NOT EXISTS broker_accounts (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    account_id TEXT NOT NULL,
    broker_adapter TEXT NOT NULL,
    environment TEXT NOT NULL CHECK (environment IN ('SIMULATION', 'PAPER', 'SHADOW', 'LIVE')),
    base_currency CHAR(3) NOT NULL CHECK (base_currency ~ '^[A-Z]{3}$'),
    lifecycle_state TEXT NOT NULL CHECK (lifecycle_state IN ('PENDING', 'ACTIVE', 'SUSPENDED', 'CLOSED')),
    configuration JSONB NOT NULL CHECK (jsonb_typeof(configuration) = 'object'),
    version BIGINT NOT NULL CHECK (version > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, account_id),
    CHECK (account_id ~ '^[a-z0-9._-]+$'),
    CHECK (broker_adapter ~ '^[a-z0-9._-]+$')
);

CREATE TABLE IF NOT EXISTS strategy_versions (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    strategy_id TEXT NOT NULL,
    version BIGINT NOT NULL CHECK (version > 0),
    bundle_sha256 BYTEA NOT NULL CHECK (octet_length(bundle_sha256) = 32),
    runtime_contract_version TEXT NOT NULL,
    metadata JSONB NOT NULL CHECK (jsonb_typeof(metadata) = 'object'),
    created_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, strategy_id, version),
    UNIQUE (tenant_id, strategy_id, bundle_sha256),
    CHECK (strategy_id ~ '^[a-z0-9._-]+$'),
    FOREIGN KEY (tenant_id, created_by) REFERENCES customer_users(tenant_id, user_id)
);

CREATE TABLE IF NOT EXISTS configuration_versions (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    configuration_id TEXT NOT NULL,
    version BIGINT NOT NULL CHECK (version > 0),
    document JSONB NOT NULL CHECK (jsonb_typeof(document) = 'object'),
    document_sha256 BYTEA NOT NULL CHECK (octet_length(document_sha256) = 32),
    approved_by TEXT NOT NULL,
    effective_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, configuration_id, version),
    UNIQUE (tenant_id, configuration_id, document_sha256),
    CHECK (configuration_id ~ '^[a-z0-9._-]+$'),
    FOREIGN KEY (tenant_id, approved_by) REFERENCES customer_users(tenant_id, user_id)
);

CREATE TABLE IF NOT EXISTS instrument_reference_versions (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    instrument_id TEXT NOT NULL,
    version BIGINT NOT NULL CHECK (version > 0),
    document JSONB NOT NULL CHECK (jsonb_typeof(document) = 'object'),
    document_sha256 BYTEA NOT NULL CHECK (octet_length(document_sha256) = 32),
    effective_from TIMESTAMPTZ NOT NULL,
    effective_to TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, instrument_id, version),
    CHECK (instrument_id ~ '^[a-z0-9._-]+$'),
    CHECK (effective_to IS NULL OR effective_to > effective_from)
);

CREATE TABLE IF NOT EXISTS order_projections (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    order_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    strategy_id TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    parent_order_id TEXT,
    client_order_id TEXT NOT NULL,
    broker_order_id TEXT,
    idempotency_key TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'CREATED', 'PENDING_RISK', 'RISK_REJECTED', 'APPROVED',
        'PENDING_SUBMIT', 'SUBMITTED', 'ACKNOWLEDGED', 'PARTIALLY_FILLED',
        'FILLED', 'PENDING_CANCEL', 'PENDING_REPLACE', 'CANCELLED',
        'REJECTED', 'EXPIRED', 'UNKNOWN'
    )),
    side TEXT NOT NULL CHECK (side IN ('BUY', 'SELL')),
    order_type TEXT NOT NULL CHECK (order_type IN ('MARKET', 'LIMIT', 'STOP', 'STOP_LIMIT')),
    time_in_force TEXT NOT NULL CHECK (time_in_force IN ('DAY', 'GTC', 'IOC', 'FOK')),
    quantity NUMERIC(38,8) NOT NULL CHECK (quantity > 0),
    filled_quantity NUMERIC(38,8) NOT NULL DEFAULT 0 CHECK (filled_quantity >= 0 AND filled_quantity <= quantity),
    limit_price NUMERIC(38,8) CHECK (limit_price > 0),
    stop_price NUMERIC(38,8) CHECK (stop_price > 0),
    projection_version BIGINT NOT NULL CHECK (projection_version > 0),
    source_event_id TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, order_id),
    FOREIGN KEY (tenant_id, account_id) REFERENCES broker_accounts(tenant_id, account_id),
    FOREIGN KEY (tenant_id, parent_order_id) REFERENCES order_projections(tenant_id, order_id),
    FOREIGN KEY (tenant_id, source_event_id) REFERENCES domain_events(tenant_id, event_id),
    UNIQUE (tenant_id, account_id, client_order_id),
    UNIQUE (tenant_id, account_id, idempotency_key),
    CHECK (order_id ~ '^[a-z0-9._-]+$'),
    CHECK (instrument_id ~ '^[a-z0-9._-]+$'),
    CHECK (client_order_id ~ '^[a-z0-9._-]+$'),
    CHECK (idempotency_key ~ '^[a-z0-9._-]+$'),
    CHECK (
        (order_type = 'MARKET' AND limit_price IS NULL AND stop_price IS NULL) OR
        (order_type = 'LIMIT' AND limit_price IS NOT NULL AND stop_price IS NULL) OR
        (order_type = 'STOP' AND limit_price IS NULL AND stop_price IS NOT NULL) OR
        (order_type = 'STOP_LIMIT' AND limit_price IS NOT NULL AND stop_price IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS order_projection_state_idx
    ON order_projections (tenant_id, account_id, state, occurred_at DESC);
CREATE INDEX IF NOT EXISTS order_projection_strategy_idx
    ON order_projections (tenant_id, strategy_id, occurred_at DESC);

CREATE TABLE IF NOT EXISTS execution_projections (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    execution_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    broker_execution_id TEXT,
    instrument_id TEXT NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('BUY', 'SELL')),
    quantity NUMERIC(38,8) NOT NULL CHECK (quantity > 0),
    price NUMERIC(38,8) NOT NULL CHECK (price > 0),
    fee NUMERIC(38,8) NOT NULL DEFAULT 0 CHECK (fee >= 0),
    currency CHAR(3) NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    source_event_id TEXT NOT NULL,
    executed_at TIMESTAMPTZ NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, execution_id),
    FOREIGN KEY (tenant_id, order_id) REFERENCES order_projections(tenant_id, order_id),
    FOREIGN KEY (tenant_id, source_event_id) REFERENCES domain_events(tenant_id, event_id),
    UNIQUE (tenant_id, broker_execution_id),
    CHECK (execution_id ~ '^[a-z0-9._-]+$'),
    CHECK (instrument_id ~ '^[a-z0-9._-]+$')
);

CREATE INDEX IF NOT EXISTS execution_projection_order_idx
    ON execution_projections (tenant_id, order_id, executed_at);

CREATE TABLE IF NOT EXISTS position_projections (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    account_id TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    currency CHAR(3) NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    quantity NUMERIC(38,8) NOT NULL,
    average_cost NUMERIC(38,8) NOT NULL CHECK (average_cost >= 0),
    realized_pnl NUMERIC(38,8) NOT NULL DEFAULT 0,
    unrealized_pnl NUMERIC(38,8) NOT NULL DEFAULT 0,
    projection_version BIGINT NOT NULL CHECK (projection_version > 0),
    source_event_id TEXT NOT NULL,
    as_of TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, account_id, instrument_id),
    FOREIGN KEY (tenant_id, account_id) REFERENCES broker_accounts(tenant_id, account_id),
    FOREIGN KEY (tenant_id, source_event_id) REFERENCES domain_events(tenant_id, event_id),
    CHECK (instrument_id ~ '^[a-z0-9._-]+$'),
    CHECK ((quantity = 0 AND average_cost = 0) OR quantity <> 0)
);

CREATE INDEX IF NOT EXISTS position_projection_account_idx
    ON position_projections (tenant_id, account_id, as_of DESC);

CREATE TABLE IF NOT EXISTS audit_event_indexes (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    event_id TEXT NOT NULL,
    account_id TEXT,
    strategy_id TEXT,
    instrument_id TEXT,
    order_id TEXT,
    actor_id TEXT,
    event_type TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant_id, event_id),
    FOREIGN KEY (tenant_id, event_id) REFERENCES domain_events(tenant_id, event_id)
);

CREATE INDEX IF NOT EXISTS audit_event_causal_lookup_idx
    ON audit_event_indexes (tenant_id, order_id, occurred_at, event_id);
CREATE INDEX IF NOT EXISTS audit_event_instrument_lookup_idx
    ON audit_event_indexes (tenant_id, instrument_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS audit_event_strategy_lookup_idx
    ON audit_event_indexes (tenant_id, strategy_id, occurred_at DESC);

CREATE TABLE IF NOT EXISTS billing_subscription_evidence (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    subscription_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_subscription_hash BYTEA NOT NULL CHECK (octet_length(provider_subscription_hash) = 32),
    plan_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('TRIAL', 'ACTIVE', 'PAST_DUE', 'CANCELLED', 'EXPIRED')),
    seats INTEGER NOT NULL CHECK (seats > 0),
    amount_minor BIGINT NOT NULL CHECK (amount_minor >= 0),
    currency CHAR(3) NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    observed_at TIMESTAMPTZ NOT NULL,
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    evidence_sha256 BYTEA NOT NULL CHECK (octet_length(evidence_sha256) = 32),
    PRIMARY KEY (tenant_id, subscription_id),
    CHECK (subscription_id ~ '^[a-z0-9._-]+$'),
    CHECK (provider ~ '^[a-z0-9._-]+$'),
    CHECK (plan_id ~ '^[a-z0-9._-]+$')
);

CREATE TABLE IF NOT EXISTS customer_recovery_codes (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    user_id TEXT NOT NULL,
    code_sha256 BYTEA NOT NULL CHECK (octet_length(code_sha256) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    consumed_at TIMESTAMPTZ,
    PRIMARY KEY (tenant_id, user_id, code_sha256),
    FOREIGN KEY (tenant_id, user_id)
        REFERENCES customer_users(tenant_id, user_id) ON DELETE CASCADE
);

DO $$
DECLARE
    table_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'broker_accounts', 'strategy_versions', 'configuration_versions',
        'instrument_reference_versions', 'order_projections',
        'execution_projections', 'position_projections', 'audit_event_indexes',
        'billing_subscription_evidence', 'customer_recovery_codes'
    ]
    LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
        EXECUTE format('DROP POLICY IF EXISTS tenant_isolation ON %I', table_name);
        EXECUTE format(
            'CREATE POLICY tenant_isolation ON %I USING (tenant_id = current_setting(''app.tenant_id'', TRUE)) WITH CHECK (tenant_id = current_setting(''app.tenant_id'', TRUE))',
            table_name
        );
    END LOOP;
END;
$$;
