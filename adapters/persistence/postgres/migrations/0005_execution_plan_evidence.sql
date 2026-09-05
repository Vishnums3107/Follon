-- Phase 3 Execution plan, capability routing, and TCA benchmark evidence.
-- This migration is additive only. Rollback disables the Phase 3 readers and
-- planners; it never deletes or rewrites retained evidence from these tables.

CREATE TABLE IF NOT EXISTS venue_capabilities (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    venue TEXT NOT NULL,
    capability_version TEXT NOT NULL,
    supported_order_kinds JSONB NOT NULL CHECK (jsonb_typeof(supported_order_kinds) = 'array'),
    supports_iceberg BOOLEAN NOT NULL DEFAULT FALSE,
    min_quantity TEXT,
    max_quantity TEXT,
    document_sha256 BYTEA NOT NULL CHECK (octet_length(document_sha256) = 32),
    source_event_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, venue, capability_version),
    CHECK (venue ~ '^[a-z0-9._-]+$'),
    CHECK (capability_version ~ '^[a-z0-9._-]+$'),
    FOREIGN KEY (tenant_id, source_event_id)
        REFERENCES domain_events(tenant_id, event_id)
);

CREATE TABLE IF NOT EXISTS execution_plan_evidence (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    evidence_id TEXT NOT NULL,
    parent_order_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('BUY', 'SELL')),
    parent_quantity TEXT NOT NULL,
    limit_price TEXT,
    algorithm TEXT NOT NULL,
    children JSONB NOT NULL CHECK (jsonb_typeof(children) = 'array'),
    unallocated_quantity TEXT NOT NULL,
    source_capability_version TEXT,
    plan_sha256 BYTEA NOT NULL CHECK (octet_length(plan_sha256) = 32),
    source_event_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, evidence_id),
    UNIQUE (tenant_id, parent_order_id),
    CHECK (evidence_id ~ '^[a-z0-9._-]+$'),
    CHECK (parent_order_id ~ '^[a-z0-9._-]+$'),
    CHECK (account_id ~ '^[a-z0-9._-]+$'),
    CHECK (instrument_id ~ '^[a-z0-9._-]+$'),
    FOREIGN KEY (tenant_id, source_event_id)
        REFERENCES domain_events(tenant_id, event_id)
);

CREATE TABLE IF NOT EXISTS execution_route_decisions (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    decision_id TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    parent_order_id TEXT NOT NULL,
    venue TEXT NOT NULL,
    capability_version TEXT NOT NULL,
    allocated_quantity TEXT NOT NULL,
    all_in_price TEXT NOT NULL,
    fee_per_unit TEXT NOT NULL,
    latency_rank INTEGER NOT NULL CHECK (latency_rank >= 0),
    decision_sha256 BYTEA NOT NULL CHECK (octet_length(decision_sha256) = 32),
    source_event_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, decision_id),
    CHECK (decision_id ~ '^[a-z0-9._-]+$'),
    CHECK (parent_order_id ~ '^[a-z0-9._-]+$'),
    CHECK (venue ~ '^[a-z0-9._-]+$'),
    CHECK (capability_version ~ '^[a-z0-9._-]+$'),
    FOREIGN KEY (tenant_id, evidence_id)
        REFERENCES execution_plan_evidence(tenant_id, evidence_id),
    FOREIGN KEY (tenant_id, source_event_id)
        REFERENCES domain_events(tenant_id, event_id)
);

CREATE TABLE IF NOT EXISTS execution_benchmark_evidence (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    benchmark_id TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    parent_order_id TEXT NOT NULL,
    arrival_price TEXT NOT NULL,
    target_price TEXT NOT NULL,
    source TEXT NOT NULL,
    benchmark_sha256 BYTEA NOT NULL CHECK (octet_length(benchmark_sha256) = 32),
    source_event_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, benchmark_id),
    UNIQUE (tenant_id, parent_order_id),
    CHECK (benchmark_id ~ '^[a-z0-9._-]+$'),
    CHECK (parent_order_id ~ '^[a-z0-9._-]+$'),
    CHECK (source ~ '^[a-z0-9._-]+$'),
    FOREIGN KEY (tenant_id, evidence_id)
        REFERENCES execution_plan_evidence(tenant_id, evidence_id),
    FOREIGN KEY (tenant_id, source_event_id)
        REFERENCES domain_events(tenant_id, event_id)
);

CREATE INDEX IF NOT EXISTS execution_plan_evidence_lookup_idx
    ON execution_plan_evidence (tenant_id, account_id, instrument_id, created_at DESC);

CREATE INDEX IF NOT EXISTS execution_route_decisions_venue_idx
    ON execution_route_decisions (tenant_id, venue, created_at DESC);

DO $$
DECLARE
    table_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'venue_capabilities', 'execution_plan_evidence',
        'execution_route_decisions', 'execution_benchmark_evidence'
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
