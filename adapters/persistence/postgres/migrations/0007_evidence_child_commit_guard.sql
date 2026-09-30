-- A retained parent cannot acquire new evidence after its first commit (E7.15).
-- The creation transaction's full xid8 is stored on new parents. Existing rows
-- remain NULL and therefore cannot gain children after this migration. The
-- parent tuple's xmin must also be current: a logical dump/restore preserves
-- the stored xid8 but gives the restored tuple a new xmin. Both checks together
-- avoid trusting a reused xid on another cluster or after 32-bit wraparound.
--
-- Only these child tables are part of their parent's original evidence. Broker
-- receipts, news sentiments, and other later-arriving observations are not.

ALTER TABLE journal_transactions ADD COLUMN created_xact_id xid8;
ALTER TABLE execution_plan_evidence ADD COLUMN created_xact_id xid8;

CREATE FUNCTION capture_evidence_parent_xact() RETURNS TRIGGER AS $$
BEGIN
    NEW.created_xact_id := pg_current_xact_id();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER capture_creation_xact
    BEFORE INSERT ON journal_transactions
    FOR EACH ROW EXECUTE FUNCTION capture_evidence_parent_xact();
CREATE TRIGGER capture_creation_xact
    BEFORE INSERT ON execution_plan_evidence
    FOR EACH ROW EXECUTE FUNCTION capture_evidence_parent_xact();
ALTER TABLE journal_transactions ENABLE ALWAYS TRIGGER capture_creation_xact;
ALTER TABLE execution_plan_evidence ENABLE ALWAYS TRIGGER capture_creation_xact;

CREATE FUNCTION refuse_late_evidence_child() RETURNS TRIGGER AS $$
DECLARE
    parent_xact_id xid8;
    parent_insert_xid xid;
BEGIN
    IF TG_TABLE_NAME = 'journal_lines' THEN
        SELECT created_xact_id, xmin INTO parent_xact_id, parent_insert_xid
          FROM journal_transactions
         WHERE transaction_id = NEW.transaction_id AND tenant_id = NEW.tenant_id;
    ELSIF TG_TABLE_NAME IN ('execution_route_decisions', 'execution_benchmark_evidence') THEN
        SELECT created_xact_id, xmin INTO parent_xact_id, parent_insert_xid
          FROM execution_plan_evidence
         WHERE tenant_id = NEW.tenant_id AND evidence_id = NEW.evidence_id;
    ELSE
        RAISE EXCEPTION 'unexpected evidence child table %', TG_TABLE_NAME
            USING ERRCODE = 'restrict_violation';
    END IF;
    IF parent_xact_id IS DISTINCT FROM pg_current_xact_id()
       OR parent_insert_xid IS DISTINCT FROM pg_current_xact_id()::xid THEN
        RAISE EXCEPTION '% must be inserted with its parent in the same transaction', TG_TABLE_NAME
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER guard_parent_commit
    BEFORE INSERT ON journal_lines
    FOR EACH ROW EXECUTE FUNCTION refuse_late_evidence_child();
CREATE TRIGGER guard_parent_commit
    BEFORE INSERT ON execution_route_decisions
    FOR EACH ROW EXECUTE FUNCTION refuse_late_evidence_child();
CREATE TRIGGER guard_parent_commit
    BEFORE INSERT ON execution_benchmark_evidence
    FOR EACH ROW EXECUTE FUNCTION refuse_late_evidence_child();
ALTER TABLE journal_lines ENABLE ALWAYS TRIGGER guard_parent_commit;
ALTER TABLE execution_route_decisions ENABLE ALWAYS TRIGGER guard_parent_commit;
ALTER TABLE execution_benchmark_evidence ENABLE ALWAYS TRIGGER guard_parent_commit;
