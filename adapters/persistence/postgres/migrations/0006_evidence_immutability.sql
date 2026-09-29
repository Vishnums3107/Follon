-- Retained evidence is append-only, and news belongs to a tenant (delivery state E7.7).
--
-- Migrations 0004 and 0005 promise that a rollback "never deletes or rewrites"
-- retained evidence, and the application only ever inserts it. Nothing in the
-- database stopped a role holding UPDATE, DELETE or TRUNCATE from doing either. The
-- triggers below make the refusal the database's own, beneath the application. They
-- are the application's boundary and not a defence against the database's owner, who
-- can disable a trigger, but that takes a DDL statement that shows in the logs.
--
-- The news tables also had no tenant column and no row-level security, so one
-- tenant's feed was visible to every other. They now carry the same isolation as
-- everything else. No writer in this repository has ever inserted a news row, and
-- the migration refuses to run over rows whose owner it could only guess.

CREATE OR REPLACE FUNCTION refuse_evidence_mutation() RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION '% on % is refused: retained evidence is append-only', TG_OP, TG_TABLE_NAME
        USING ERRCODE = 'restrict_violation';
END;
$$ LANGUAGE plpgsql;

-- A reference version is closed by giving it an end, once, and is otherwise fixed.
CREATE OR REPLACE FUNCTION refuse_version_rewrite() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF OLD.effective_to IS NULL
           AND NEW.effective_to IS NOT NULL
           AND to_jsonb(NEW) - 'effective_to' = to_jsonb(OLD) - 'effective_to' THEN
            RETURN NEW;
        END IF;
    END IF;
    RAISE EXCEPTION '% on % is refused: a version may only be closed, once, by setting effective_to', TG_OP, TG_TABLE_NAME
        USING ERRCODE = 'restrict_violation';
END;
$$ LANGUAGE plpgsql;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM news_headlines) OR EXISTS (SELECT 1 FROM news_sentiments) THEN
        RAISE EXCEPTION 'news rows exist without a tenant; give each an owner before migrating'
            USING ERRCODE = 'restrict_violation';
    END IF;
END;
$$;

ALTER TABLE news_sentiments DROP CONSTRAINT news_sentiments_causation_news_id_fkey;
ALTER TABLE news_sentiments DROP CONSTRAINT news_sentiments_pkey;
ALTER TABLE news_headlines DROP CONSTRAINT news_headlines_pkey;
ALTER TABLE news_headlines ADD COLUMN tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id);
ALTER TABLE news_sentiments ADD COLUMN tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id);
ALTER TABLE news_headlines ADD PRIMARY KEY (tenant_id, news_id);
ALTER TABLE news_sentiments ADD PRIMARY KEY (tenant_id, event_id);
ALTER TABLE news_sentiments
    ADD CONSTRAINT news_sentiments_tenant_news_fkey
    FOREIGN KEY (tenant_id, causation_news_id) REFERENCES news_headlines(tenant_id, news_id);

DROP INDEX IF EXISTS news_headlines_event_time_idx;
DROP INDEX IF EXISTS news_headlines_source_idx;
DROP INDEX IF EXISTS news_sentiments_instrument_time_idx;
DROP INDEX IF EXISTS news_sentiments_taxonomy_idx;
DROP INDEX IF EXISTS news_sentiments_causation_idx;
CREATE INDEX news_headlines_event_time_idx
    ON news_headlines (tenant_id, event_time_ns DESC);
CREATE INDEX news_headlines_source_idx
    ON news_headlines (tenant_id, source, event_time_ns DESC);
CREATE INDEX news_sentiments_instrument_time_idx
    ON news_sentiments (tenant_id, instrument_id, event_time_ns DESC);
CREATE INDEX news_sentiments_taxonomy_idx
    ON news_sentiments (tenant_id, taxonomy, event_time_ns DESC);
CREATE INDEX news_sentiments_causation_idx
    ON news_sentiments (tenant_id, causation_news_id);

DO $$
DECLARE
    table_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['news_headlines', 'news_sentiments']
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

DO $$
DECLARE
    table_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'domain_events', 'journal_transactions', 'journal_lines',
        'risk_policy_versions', 'broker_commands', 'broker_receipts',
        'strategy_versions', 'configuration_versions', 'audit_event_indexes',
        'news_headlines', 'news_sentiments', 'fx_pricing_snapshots',
        'venue_capabilities', 'execution_plan_evidence',
        'execution_route_decisions', 'execution_benchmark_evidence'
    ]
    LOOP
        EXECUTE format('DROP TRIGGER IF EXISTS refuse_mutation ON %I', table_name);
        EXECUTE format(
            'CREATE TRIGGER refuse_mutation BEFORE UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION refuse_evidence_mutation()',
            table_name
        );
        EXECUTE format('DROP TRIGGER IF EXISTS refuse_truncate ON %I', table_name);
        EXECUTE format(
            'CREATE TRIGGER refuse_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION refuse_evidence_mutation()',
            table_name
        );
    END LOOP;

    FOREACH table_name IN ARRAY ARRAY[
        'instrument_reference_versions', 'fx_instrument_economics_versions'
    ]
    LOOP
        EXECUTE format('DROP TRIGGER IF EXISTS refuse_mutation ON %I', table_name);
        EXECUTE format(
            'CREATE TRIGGER refuse_mutation BEFORE UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION refuse_version_rewrite()',
            table_name
        );
        EXECUTE format('DROP TRIGGER IF EXISTS refuse_truncate ON %I', table_name);
        EXECUTE format(
            'CREATE TRIGGER refuse_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION refuse_evidence_mutation()',
            table_name
        );
    END LOOP;
END;
$$;
