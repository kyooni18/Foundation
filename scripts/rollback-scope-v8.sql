-- Stop Foundation first and run with psql -v ON_ERROR_STOP=1. Scope-only rollback:
-- does not drop memories, embeddings, relations, or lifecycle history.
-- Running v8 again will reapply the repair; deploy the pre-v8 runtime if reverting.
BEGIN;
SELECT pg_advisory_xact_lock(hashtext('foundation-schema-v1'));
DO $rollback$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM foundation_schema WHERE version = 8) THEN RETURN; END IF;
  LOCK TABLE atoms IN SHARE ROW EXCLUSIVE MODE;
  IF to_regclass('atom_scope_repairs_v8') IS NULL THEN
    RAISE EXCEPTION 'Missing v8 scope repair audit; restore the verified backup instead';
  END IF;
  IF EXISTS (
    SELECT 1 FROM atom_scope_repairs_v8 r LEFT JOIN atoms a USING(id)
    WHERE a.id IS NULL OR a.scope_kind IS DISTINCT FROM r.scope_kind OR a.scope_key IS DISTINCT FROM r.new_key
  ) THEN
    RAISE EXCEPTION 'Scopes changed since repair; rollback requires manual review';
  END IF;
  IF EXISTS (
    SELECT 1 FROM atoms a LEFT JOIN atom_scope_repairs_v8 r USING(id)
    WHERE a.status <> 'superseded'
    GROUP BY a.scope_kind, COALESCE(r.old_key, a.scope_key), a.content_hash HAVING count(*) > 1
  ) THEN
    RAISE EXCEPTION 'Scope rollback would cause an identity collision';
  END IF;
  UPDATE atoms a SET scope_key = r.old_key FROM atom_scope_repairs_v8 r WHERE a.id = r.id;
  -- Preserve the repair audit for inspection. A second rollback cycle needs a new archive name.
  ALTER TABLE atom_scope_repairs_v8 RENAME TO atom_scope_repairs_v8_rolled_back;
  DELETE FROM foundation_schema WHERE version = 8;
END
$rollback$;
COMMIT;
