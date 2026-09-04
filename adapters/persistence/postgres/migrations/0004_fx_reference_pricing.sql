-- Phase 2 FX reference/pricing evidence.  This migration is additive only.
-- Rollback disables the FX reference/pricing readers; it never deletes the
-- retained source contract or pricing evidence from these tables.

CREATE TABLE IF NOT EXISTS fx_instrument_economics_versions (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    instrument_id TEXT NOT NULL,
    version BIGINT NOT NULL CHECK (version > 0),
    product TEXT NOT NULL CHECK (product IN ('FX_SPOT', 'FX_FORWARD', 'FX_SWAP')),
    base_currency CHAR(3) NOT NULL CHECK (base_currency ~ '^[A-Z]{3}$'),
    quote_currency CHAR(3) NOT NULL CHECK (quote_currency ~ '^[A-Z]{3}$'),
    terms JSONB NOT NULL CHECK (jsonb_typeof(terms) = 'object'),
    document_sha256 BYTEA NOT NULL CHECK (octet_length(document_sha256) = 32),
    effective_from TIMESTAMPTZ NOT NULL,
    effective_to TIMESTAMPTZ,
    source_event_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, instrument_id, version),
    UNIQUE (tenant_id, instrument_id, document_sha256),
    CHECK (instrument_id ~ '^[a-z0-9._-]+$'),
    CHECK (base_currency <> quote_currency),
    CHECK (effective_to IS NULL OR effective_to > effective_from),
    FOREIGN KEY (tenant_id, source_event_id)
        REFERENCES domain_events(tenant_id, event_id)
);

CREATE TABLE IF NOT EXISTS fx_pricing_snapshots (
    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id),
    snapshot_id TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    reference_version TEXT NOT NULL,
    product TEXT NOT NULL CHECK (product IN ('FX_SPOT', 'FX_FORWARD', 'FX_SWAP')),
    base_currency CHAR(3) NOT NULL CHECK (base_currency ~ '^[A-Z]{3}$'),
    quote_currency CHAR(3) NOT NULL CHECK (quote_currency ~ '^[A-Z]{3}$'),
    source_id TEXT NOT NULL,
    source_sequence BIGINT NOT NULL CHECK (source_sequence >= 0),
    source_time TIMESTAMPTZ NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    terms JSONB NOT NULL CHECK (jsonb_typeof(terms) = 'object'),
    terms_sha256 BYTEA NOT NULL CHECK (octet_length(terms_sha256) = 32),
    source_event_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, snapshot_id),
    UNIQUE (tenant_id, instrument_id, source_id, source_sequence),
    CHECK (snapshot_id ~ '^[a-z0-9._-]+$'),
    CHECK (instrument_id ~ '^[a-z0-9._-]+$'),
    CHECK (reference_version ~ '^[a-z0-9._-]+$'),
    CHECK (source_id ~ '^[a-z0-9._-]+$'),
    CHECK (base_currency <> quote_currency),
    CHECK (source_time <= received_at),
    FOREIGN KEY (tenant_id, source_event_id)
        REFERENCES domain_events(tenant_id, event_id)
);

CREATE INDEX IF NOT EXISTS fx_pricing_snapshots_lookup_idx
    ON fx_pricing_snapshots (tenant_id, instrument_id, product, received_at DESC, source_sequence DESC);

DO $$
DECLARE
    table_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'fx_instrument_economics_versions', 'fx_pricing_snapshots'
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
