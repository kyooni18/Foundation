export const SCHEMA_VERSION = 8;

export const schemaStatements = [
  `CREATE EXTENSION IF NOT EXISTS vector`,
  `CREATE EXTENSION IF NOT EXISTS pgcrypto`,
  `CREATE EXTENSION IF NOT EXISTS pg_trgm`,

  `CREATE TABLE IF NOT EXISTS foundation_schema (
    version INTEGER PRIMARY KEY,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,

  `CREATE TABLE IF NOT EXISTS atoms (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    content TEXT NOT NULL,
    normalized_content TEXT NOT NULL,
    content_hash CHAR(64) NOT NULL,
    vector HALFVEC(3072),
    vector_provider TEXT,
    vector_model TEXT,
    vector_dimensions INTEGER,
    vector_embedded_at TIMESTAMPTZ,
    type TEXT NOT NULL CHECK (type IN ('usercreated','aicreated','imported')),
    tags TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,

  `CREATE TABLE IF NOT EXISTS vaults (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vault_uid TEXT NOT NULL UNIQUE,
    name TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,

  `CREATE TABLE IF NOT EXISTS files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vault_id UUID REFERENCES vaults(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    mime_type TEXT,
    data BYTEA,
    content TEXT,
    interpreted TEXT,
    subject TEXT,

    vector_subject HALFVEC(3072),
    vector_subject_provider TEXT,
    vector_subject_model TEXT,
    vector_subject_dimensions INTEGER,
    vector_subject_embedded_at TIMESTAMPTZ,

    vector_content HALFVEC(3072),
    vector_content_provider TEXT,
    vector_content_model TEXT,
    vector_content_dimensions INTEGER,
    vector_content_embedded_at TIMESTAMPTZ,

    tags TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,

  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS vector_provider TEXT`,
  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS vector_model TEXT`,
  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS vector_dimensions INTEGER`,
  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS vector_embedded_at TIMESTAMPTZ`,

  `ALTER TABLE files ADD COLUMN IF NOT EXISTS vector_subject_provider TEXT`,
  `ALTER TABLE files ADD COLUMN IF NOT EXISTS vector_subject_model TEXT`,
  `ALTER TABLE files ADD COLUMN IF NOT EXISTS vector_subject_dimensions INTEGER`,
  `ALTER TABLE files ADD COLUMN IF NOT EXISTS vector_subject_embedded_at TIMESTAMPTZ`,
  `ALTER TABLE files ADD COLUMN IF NOT EXISTS vector_content_provider TEXT`,
  `ALTER TABLE files ADD COLUMN IF NOT EXISTS vector_content_model TEXT`,
  `ALTER TABLE files ADD COLUMN IF NOT EXISTS vector_content_dimensions INTEGER`,
  `ALTER TABLE files ADD COLUMN IF NOT EXISTS vector_content_embedded_at TIMESTAMPTZ`,

  `DO $foundation_vector_upgrade$
  DECLARE
    atom_vector_type TEXT;
    file_subject_type TEXT;
    file_content_type TEXT;
  BEGIN
    SELECT format_type(a.atttypid, a.atttypmod)
      INTO atom_vector_type
      FROM pg_attribute a
      JOIN pg_class c ON c.oid = a.attrelid
      WHERE c.relname = 'atoms' AND a.attname = 'vector' AND a.attnum > 0 AND NOT a.attisdropped;

    SELECT format_type(a.atttypid, a.atttypmod)
      INTO file_subject_type
      FROM pg_attribute a
      JOIN pg_class c ON c.oid = a.attrelid
      WHERE c.relname = 'files' AND a.attname = 'vector_subject' AND a.attnum > 0 AND NOT a.attisdropped;

    SELECT format_type(a.atttypid, a.atttypmod)
      INTO file_content_type
      FROM pg_attribute a
      JOIN pg_class c ON c.oid = a.attrelid
      WHERE c.relname = 'files' AND a.attname = 'vector_content' AND a.attnum > 0 AND NOT a.attisdropped;

    IF atom_vector_type IS DISTINCT FROM 'halfvec(3072)' THEN
      DROP INDEX IF EXISTS atoms_vector_hnsw;
      UPDATE atoms
        SET vector = NULL,
            vector_provider = NULL,
            vector_model = NULL,
            vector_dimensions = NULL,
            vector_embedded_at = NULL
        WHERE vector IS NOT NULL;
      ALTER TABLE atoms
        ALTER COLUMN vector TYPE HALFVEC(3072)
        USING NULL::HALFVEC(3072);
    END IF;

    IF file_subject_type IS DISTINCT FROM 'halfvec(3072)' THEN
      DROP INDEX IF EXISTS files_vector_subject_hnsw;
      UPDATE files
        SET vector_subject = NULL,
            vector_subject_provider = NULL,
            vector_subject_model = NULL,
            vector_subject_dimensions = NULL,
            vector_subject_embedded_at = NULL
        WHERE vector_subject IS NOT NULL;
      ALTER TABLE files
        ALTER COLUMN vector_subject TYPE HALFVEC(3072)
        USING NULL::HALFVEC(3072);
    END IF;

    IF file_content_type IS DISTINCT FROM 'halfvec(3072)' THEN
      DROP INDEX IF EXISTS files_vector_content_hnsw;
      UPDATE files
        SET vector_content = NULL,
            vector_content_provider = NULL,
            vector_content_model = NULL,
            vector_content_dimensions = NULL,
            vector_content_embedded_at = NULL
        WHERE vector_content IS NOT NULL;
      ALTER TABLE files
        ALTER COLUMN vector_content TYPE HALFVEC(3072)
        USING NULL::HALFVEC(3072);
    END IF;
  END
  $foundation_vector_upgrade$`,

  `UPDATE atoms
    SET vector = NULL,
        vector_provider = NULL,
        vector_model = NULL,
        vector_dimensions = NULL,
        vector_embedded_at = NULL
    WHERE vector IS NOT NULL
      AND (
        vector_provider IS NULL OR
        vector_model IS NULL OR
        vector_dimensions IS DISTINCT FROM 3072 OR
        vector_embedded_at IS NULL
      )`,

  `UPDATE files
    SET vector_subject = NULL,
        vector_subject_provider = NULL,
        vector_subject_model = NULL,
        vector_subject_dimensions = NULL,
        vector_subject_embedded_at = NULL
    WHERE vector_subject IS NOT NULL
      AND (
        vector_subject_provider IS NULL OR
        vector_subject_model IS NULL OR
        vector_subject_dimensions IS DISTINCT FROM 3072 OR
        vector_subject_embedded_at IS NULL
      )`,

  `UPDATE files
    SET vector_content = NULL,
        vector_content_provider = NULL,
        vector_content_model = NULL,
        vector_content_dimensions = NULL,
        vector_content_embedded_at = NULL
    WHERE vector_content IS NOT NULL
      AND (
        vector_content_provider IS NULL OR
        vector_content_model IS NULL OR
        vector_content_dimensions IS DISTINCT FROM 3072 OR
        vector_content_embedded_at IS NULL
      )`,

  `CREATE INDEX IF NOT EXISTS atoms_tags_gin ON atoms USING GIN (tags)`,
  `CREATE INDEX IF NOT EXISTS atoms_content_trgm ON atoms USING GIN (content gin_trgm_ops)`,
  `CREATE INDEX IF NOT EXISTS atoms_fts_gin ON atoms USING GIN (to_tsvector('simple', content))`,
  `CREATE INDEX IF NOT EXISTS atoms_metadata_gin ON atoms USING GIN (metadata)`,
  `CREATE INDEX IF NOT EXISTS atoms_metadata_fts_gin ON atoms USING GIN (jsonb_to_tsvector('simple', metadata, '["string","key","numeric"]'::jsonb))`,
  `CREATE INDEX IF NOT EXISTS atoms_vector_hnsw ON atoms USING hnsw (vector halfvec_cosine_ops) WHERE vector IS NOT NULL`,
  `CREATE INDEX IF NOT EXISTS atoms_vector_provenance_idx
    ON atoms (vector_provider, vector_model, vector_dimensions)
    WHERE vector IS NOT NULL`,

  `CREATE INDEX IF NOT EXISTS files_vault_idx ON files (vault_id, modified_at DESC)`,
  `CREATE INDEX IF NOT EXISTS files_tags_gin ON files USING GIN (tags)`,
  `CREATE INDEX IF NOT EXISTS files_content_trgm ON files USING GIN (content gin_trgm_ops)`,
  `CREATE INDEX IF NOT EXISTS files_subject_trgm ON files USING GIN (subject gin_trgm_ops)`,
  `CREATE INDEX IF NOT EXISTS files_fts_gin ON files USING GIN (to_tsvector('simple', coalesce(content,'') || ' ' || coalesce(interpreted,'') || ' ' || coalesce(subject,'')))`,
  `CREATE INDEX IF NOT EXISTS files_metadata_gin ON files USING GIN (metadata)`,
  `CREATE INDEX IF NOT EXISTS files_metadata_fts_gin ON files USING GIN (jsonb_to_tsvector('simple', metadata, '["string","key","numeric"]'::jsonb))`,
  `CREATE INDEX IF NOT EXISTS files_vector_subject_hnsw ON files USING hnsw (vector_subject halfvec_cosine_ops) WHERE vector_subject IS NOT NULL`,
  `CREATE INDEX IF NOT EXISTS files_vector_content_hnsw ON files USING hnsw (vector_content halfvec_cosine_ops) WHERE vector_content IS NOT NULL`,
  `CREATE INDEX IF NOT EXISTS files_vector_subject_provenance_idx
    ON files (vector_subject_provider, vector_subject_model, vector_subject_dimensions)
    WHERE vector_subject IS NOT NULL`,
  `CREATE INDEX IF NOT EXISTS files_vector_content_provenance_idx
    ON files (vector_content_provider, vector_content_model, vector_content_dimensions)
    WHERE vector_content IS NOT NULL`,

  `CREATE TABLE IF NOT EXISTS atom_file_docks (
    atom_id UUID NOT NULL REFERENCES atoms(id) ON DELETE CASCADE,
    file_id UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (atom_id, file_id)
  )`,
  `CREATE INDEX IF NOT EXISTS atom_file_docks_file_idx ON atom_file_docks (file_id, atom_id)`,

  `CREATE TABLE IF NOT EXISTS modifications (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vault_id UUID REFERENCES vaults(id) ON DELETE SET NULL,
    file_id UUID REFERENCES files(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::JSONB,
    time TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,
  `CREATE INDEX IF NOT EXISTS modifications_file_idx ON modifications (file_id, time DESC)`,
  `CREATE INDEX IF NOT EXISTS modifications_vault_idx ON modifications (vault_id, time DESC)`,

  `CREATE TABLE IF NOT EXISTS file_links (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    a_file_id UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    b_file_id UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    subject TEXT,
    absolute BOOLEAN NOT NULL DEFAULT FALSE,
    content TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (a_file_id <> b_file_id),
    UNIQUE (a_file_id, b_file_id)
  )`,
  `CREATE INDEX IF NOT EXISTS file_links_a_idx ON file_links (a_file_id)`,
  `CREATE INDEX IF NOT EXISTS file_links_b_idx ON file_links (b_file_id)`,

  `CREATE TABLE IF NOT EXISTS file_link_atoms (
    link_id UUID NOT NULL REFERENCES file_links(id) ON DELETE CASCADE,
    atom_id UUID NOT NULL REFERENCES atoms(id) ON DELETE CASCADE,
    PRIMARY KEY (link_id, atom_id)
  )`,


  `CREATE TABLE IF NOT EXISTS atom_relations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_atom_id UUID NOT NULL REFERENCES atoms(id) ON DELETE CASCADE,
    to_atom_id UUID NOT NULL REFERENCES atoms(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK (relation IN ('supersedes','conflicts_with','refines','duplicate_of')),
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (from_atom_id <> to_atom_id),
    UNIQUE (from_atom_id, to_atom_id, relation)
  )`,
  `CREATE INDEX IF NOT EXISTS atom_relations_from_idx ON atom_relations (from_atom_id, created_at DESC)`,
  `CREATE INDEX IF NOT EXISTS atom_relations_to_idx ON atom_relations (to_atom_id, created_at DESC)`,

  `CREATE TABLE IF NOT EXISTS memory_requests (
    request_id TEXT PRIMARY KEY,
    input_content TEXT NOT NULL,
    source_time TIMESTAMPTZ NOT NULL,
    result JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,

  `CREATE TABLE IF NOT EXISTS memory_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id TEXT,
    input_content TEXT NOT NULL,
    source_time TIMESTAMPTZ NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('new','duplicate','update','supersede','conflict')),
    target_atom_id UUID REFERENCES atoms(id) ON DELETE SET NULL,
    result_atom_id UUID REFERENCES atoms(id) ON DELETE SET NULL,
    confidence DOUBLE PRECISION NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    rationale TEXT NOT NULL DEFAULT '',
    candidates JSONB NOT NULL DEFAULT '[]'::JSONB,
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,
  `CREATE INDEX IF NOT EXISTS memory_events_request_idx ON memory_events (request_id, created_at DESC)`,
  `CREATE INDEX IF NOT EXISTS memory_events_result_idx ON memory_events (result_atom_id, created_at DESC)`,

  `CREATE TABLE IF NOT EXISTS external_bindings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    system TEXT NOT NULL,
    entity_kind TEXT NOT NULL,
    entity_id UUID NOT NULL,
    external_id TEXT NOT NULL,
    external_url TEXT,
    external_updated_at TIMESTAMPTZ,
    foundation_updated_at TIMESTAMPTZ,
    content_hash CHAR(64),
    last_synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (system, entity_kind, entity_id),
    UNIQUE (system, external_id)
  )`,
  `CREATE INDEX IF NOT EXISTS external_bindings_entity_idx
    ON external_bindings (entity_kind, entity_id)`,
  `CREATE INDEX IF NOT EXISTS external_bindings_sync_idx
    ON external_bindings (system, last_synced_at DESC)`,

  `CREATE TABLE IF NOT EXISTS oauth_clients (
    client_id TEXT PRIMARY KEY,
    client_name TEXT NOT NULL,
    redirect_uris JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,

  `CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    code_hash CHAR(64) PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    scopes TEXT[] NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,
  `CREATE INDEX IF NOT EXISTS oauth_authorization_codes_expiry_idx
    ON oauth_authorization_codes (expires_at)`,

  `CREATE TABLE IF NOT EXISTS oauth_tokens (
    token_hash CHAR(64) PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    token_kind TEXT NOT NULL CHECK (token_kind IN ('access','refresh')),
    scopes TEXT[] NOT NULL,
    family_id UUID,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  )`,
  `CREATE INDEX IF NOT EXISTS oauth_tokens_expiry_idx
    ON oauth_tokens (token_kind, expires_at)`,
  `CREATE INDEX IF NOT EXISTS oauth_tokens_family_idx
    ON oauth_tokens (family_id) WHERE family_id IS NOT NULL`,


  // ---- v7: memory scope isolation, lifecycle status and temporal validity ----
  `CREATE OR REPLACE FUNCTION foundation_try_timestamptz(value TEXT) RETURNS TIMESTAMPTZ
    LANGUAGE plpgsql STABLE AS $fn$
    BEGIN
      IF value IS NULL OR btrim(value) = '' THEN RETURN NULL; END IF;
      RETURN value::timestamptz;
    EXCEPTION WHEN others THEN
      RETURN NULL;
    END
    $fn$`,
  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS scope_kind TEXT NOT NULL DEFAULT 'global'`,
  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS scope_key TEXT NOT NULL DEFAULT ''`,
  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active'`,
  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS valid_from TIMESTAMPTZ`,
  `ALTER TABLE atoms ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ`,

  // One-time, non-destructive backfill. Atom ids, relations and bindings are untouched; a
  // metadata-only backup of the pre-migration atoms and relations is kept in *_backup_v6 tables.
  `DO $foundation_v7_backfill$
  BEGIN
    IF EXISTS (SELECT 1 FROM foundation_schema WHERE version >= 7) THEN RETURN; END IF;

    CREATE TABLE IF NOT EXISTS atoms_backup_v6 AS
      SELECT id, content, normalized_content, content_hash, type, tags, metadata, created_at, updated_at
      FROM atoms;
    CREATE TABLE IF NOT EXISTS atom_relations_backup_v6 AS SELECT * FROM atom_relations;

    UPDATE atoms SET status = CASE
      WHEN metadata->>'active' = 'false' THEN
        CASE WHEN metadata ? 'supersededBy'
               OR EXISTS (SELECT 1 FROM atom_relations r WHERE r.to_atom_id = atoms.id AND r.relation = 'supersedes')
             THEN 'superseded' ELSE 'forgotten' END
      ELSE 'active' END;

    UPDATE atoms SET
      valid_from = foundation_try_timestamptz(metadata->>'validFrom'),
      expires_at = foundation_try_timestamptz(metadata->>'expires');

    UPDATE atoms o SET expires_at = COALESCE(o.expires_at, s.t)
      FROM (
        SELECT r.to_atom_id AS id, min(COALESCE(n.valid_from, r.created_at)) AS t
        FROM atom_relations r JOIN atoms n ON n.id = r.from_atom_id
        WHERE r.relation = 'supersedes'
        GROUP BY r.to_atom_id
      ) s
      WHERE o.id = s.id AND o.status = 'superseded';

    UPDATE atoms SET
      scope_kind = 'project',
      scope_key = regexp_replace(lower(normalize(btrim(COALESCE(
        NULLIF(btrim(metadata->>'project'), ''),
        NULLIF(btrim(metadata->>'projectId'), ''),
        NULLIF(btrim(metadata->>'repo'), ''))), NFKC)), '[[:space:]]+', ' ', 'g')
      WHERE COALESCE(NULLIF(btrim(metadata->>'project'), ''), NULLIF(btrim(metadata->>'projectId'), ''), NULLIF(btrim(metadata->>'repo'), '')) IS NOT NULL
        AND lower(COALESCE(metadata->>'scope', 'project')) NOT IN ('global', 'conversation');

    UPDATE atoms SET
      scope_kind = 'conversation',
      scope_key = regexp_replace(lower(normalize(btrim(metadata->>'conversationId'), NFKC)), '[[:space:]]+', ' ', 'g')
      WHERE NULLIF(btrim(metadata->>'conversationId'), '') IS NOT NULL
        AND lower(COALESCE(metadata->>'scope', 'conversation')) = 'conversation';

    UPDATE atoms SET metadata = metadata || '{"scopeUnresolved":true}'::jsonb
      WHERE scope_kind = 'global' AND lower(COALESCE(metadata->>'scope', '')) IN ('project', 'conversation');
  END
  $foundation_v7_backfill$`,

  `DO $foundation_v7_constraints$
  BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'atoms_scope_kind_check') THEN
      ALTER TABLE atoms ADD CONSTRAINT atoms_scope_kind_check CHECK (scope_kind IN ('global','project','conversation'));
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'atoms_scope_key_check') THEN
      ALTER TABLE atoms ADD CONSTRAINT atoms_scope_key_check
        CHECK ((scope_kind = 'global' AND scope_key = '') OR (scope_kind <> 'global' AND scope_key <> ''));
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'atoms_status_check') THEN
      ALTER TABLE atoms ADD CONSTRAINT atoms_status_check CHECK (status IN ('active','superseded','forgotten'));
    END IF;
  END
  $foundation_v7_constraints$`,

  // Identity is (scope, content): identical facts in independent scopes never merge.
  // Superseded rows are historical copies and may repeat an active fact's content.
  `CREATE UNIQUE INDEX IF NOT EXISTS atoms_scope_hash_uidx
    ON atoms (scope_kind, scope_key, content_hash) WHERE status <> 'superseded'`,
  `DROP INDEX IF EXISTS atoms_content_hash_idx`,
  `CREATE INDEX IF NOT EXISTS atoms_content_hash_lookup_idx ON atoms (content_hash)`,
  `CREATE INDEX IF NOT EXISTS atoms_scope_status_idx ON atoms (scope_kind, scope_key, status)`,
  `CREATE INDEX IF NOT EXISTS atoms_validity_idx ON atoms (valid_from, expires_at) WHERE status <> 'forgotten'`,
  `CREATE INDEX IF NOT EXISTS files_name_trgm ON files USING GIN (name gin_trgm_ops)`,
  `CREATE INDEX IF NOT EXISTS files_project_idx ON files ((lower(metadata->>'project'))) WHERE metadata ? 'project'`,

  // v8 repairs only the demonstrable v7 transform, never arbitrary scope/metadata drift.
  // POSIX whitespace avoids JavaScript template-literal backslash interpretation entirely.
  `DO $foundation_v8_repair$
  BEGIN
    IF EXISTS (SELECT 1 FROM foundation_schema WHERE version >= 8) THEN RETURN; END IF;
    LOCK TABLE atoms IN SHARE ROW EXCLUSIVE MODE;
    CREATE TEMP TABLE foundation_scope_repair ON COMMIT DROP AS
      WITH original AS (
        SELECT a.id, a.scope_kind, a.scope_key,
          CASE WHEN a.scope_kind = 'project' THEN COALESCE(
            NULLIF(btrim(a.metadata->>'project'), ''),
            NULLIF(btrim(a.metadata->>'projectId'), ''),
            NULLIF(btrim(a.metadata->>'repo'), ''))
          WHEN a.scope_kind = 'conversation' THEN NULLIF(btrim(a.metadata->>'conversationId'), '') END AS raw
        FROM atoms a
        WHERE a.scope_kind IN ('project','conversation')
      )
      SELECT id, scope_kind, scope_key AS old_key,
        regexp_replace(lower(normalize(btrim(raw), NFKC)), '[[:space:]]+', ' ', 'g') AS new_key
      FROM original
      WHERE raw IS NOT NULL
        AND scope_key = regexp_replace(lower(normalize(btrim(raw), NFKC)), 's+', ' ', 'g')
        AND scope_key <> regexp_replace(lower(normalize(btrim(raw), NFKC)), '[[:space:]]+', ' ', 'g');

    IF EXISTS (SELECT 1 FROM foundation_scope_repair WHERE new_key = '') THEN
      RAISE EXCEPTION 'v8: ambiguous empty scope identity; no scopes repaired';
    END IF;
    -- A v6 snapshot, when present, independently corroborates the metadata identity.
    IF to_regclass('atoms_backup_v6') IS NOT NULL THEN
      IF EXISTS (
        SELECT 1 FROM foundation_scope_repair r JOIN atoms_backup_v6 b USING (id)
        WHERE r.new_key IS DISTINCT FROM regexp_replace(lower(normalize(btrim(
          CASE WHEN r.scope_kind = 'project' THEN COALESCE(NULLIF(btrim(b.metadata->>'project'), ''),
            NULLIF(btrim(b.metadata->>'projectId'), ''), NULLIF(btrim(b.metadata->>'repo'), ''))
          ELSE NULLIF(btrim(b.metadata->>'conversationId'), '') END), NFKC)), '[[:space:]]+', ' ', 'g')
      ) THEN
        RAISE EXCEPTION 'v8: original and current metadata disagree; no scopes repaired';
      END IF;
    END IF;
    IF EXISTS (
      SELECT 1 FROM atoms a LEFT JOIN foundation_scope_repair r USING (id)
      WHERE a.status <> 'superseded'
      GROUP BY a.scope_kind, COALESCE(r.new_key, a.scope_key), a.content_hash HAVING count(*) > 1
    ) THEN
      RAISE EXCEPTION 'v8: scope repair would collide with an existing identity; no scopes repaired';
    END IF;
    CREATE TABLE IF NOT EXISTS atom_scope_repairs_v8 (
      id UUID PRIMARY KEY, scope_kind TEXT NOT NULL, old_key TEXT NOT NULL, new_key TEXT NOT NULL
    );
    INSERT INTO atom_scope_repairs_v8 SELECT * FROM foundation_scope_repair;
    UPDATE atoms a SET scope_key = r.new_key FROM foundation_scope_repair r WHERE a.id = r.id;
  END
  $foundation_v8_repair$`,

  `INSERT INTO foundation_schema(version) VALUES (${SCHEMA_VERSION}) ON CONFLICT (version) DO NOTHING`
] as const;
