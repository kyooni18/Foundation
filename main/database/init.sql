CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS atoms_db (
  id BIGSERIAL PRIMARY KEY,
  content TEXT NOT NULL,
  vector VECTOR(1024) NOT NULL,
  type TEXT NOT NULL DEFAULT 'usercreated',
  parent TEXT DEFAULT NULL,
  metadata jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CHECK (type IN ('usercreated', 'aicreated', 'imported'))
);

CREATE INDEX IF NOT EXISTS atoms_db_emb_hnsw
ON atoms_db USING hnsw (vector vector_cosine_ops);

CREATE TABLE IF NOT EXISTS auth_keys (
  id BIGSERIAL PRIMARY KEY,
  hashed_key TEXT NOT NULL,
  mask TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS settings_sessions (
  id BIGSERIAL PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS settings_sessions_expires_idx
ON settings_sessions (expires_at);

CREATE TABLE IF NOT EXISTS archive (
  id BIGSERIAL PRIMARY KEY,
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  embedding VECTOR(1024) NOT NULL,
  atoms BIGINT[] DEFAULT ARRAY[]::BIGINT[],
  metadata jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS app_settings (
  setting_key TEXT PRIMARY KEY,
  setting_value TEXT NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS sources (
  id BIGSERIAL PRIMARY KEY,
  source_uid TEXT NOT NULL UNIQUE,
  source_type TEXT NOT NULL,
  label TEXT,
  locator TEXT,
  metadata jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS source_atoms (
  source_id BIGINT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  atom_id BIGINT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (source_id, atom_id)
);

CREATE INDEX IF NOT EXISTS source_atoms_atom_idx
ON source_atoms (atom_id);

CREATE TABLE IF NOT EXISTS source_indexes (
  source_id BIGINT PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
  embedding VECTOR(1024) NOT NULL,
  atom_count INTEGER NOT NULL DEFAULT 0,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS source_indexes_emb_hnsw
ON source_indexes USING hnsw (embedding vector_cosine_ops);

CREATE TABLE IF NOT EXISTS source_links (
  source_id BIGINT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  target_source_id BIGINT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  distance DOUBLE PRECISION NOT NULL,
  method TEXT NOT NULL DEFAULT 'centroid',
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (source_id, target_source_id),
  CHECK (source_id <> target_source_id)
);

CREATE INDEX IF NOT EXISTS source_links_target_idx
ON source_links (target_source_id);

CREATE TABLE IF NOT EXISTS vaults (
  id BIGSERIAL PRIMARY KEY,
  vault_uid TEXT NOT NULL UNIQUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS vault_changes (
  id BIGSERIAL PRIMARY KEY,
  vault_id BIGINT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
  file_id BIGINT,
  device_id TEXT,
  file_path TEXT NOT NULL,
  action TEXT NOT NULL,
  changed_at_unix_ms BIGINT NOT NULL,
  content_base64 TEXT,
  content_sha256 TEXT,
  size_bytes BIGINT,
  received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CHECK (action IN ('added', 'modified', 'deleted'))
);

CREATE INDEX IF NOT EXISTS vault_changes_vault_time_idx
ON vault_changes (vault_id, changed_at_unix_ms, id);

CREATE INDEX IF NOT EXISTS vault_changes_vault_id_idx
ON vault_changes (vault_id, id);

CREATE TABLE IF NOT EXISTS vault_files (
  id BIGSERIAL PRIMARY KEY,
  vault_id BIGINT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
  file_path TEXT NOT NULL,
  name TEXT NOT NULL,
  base64 TEXT NOT NULL DEFAULT '',
  content TEXT,
  interpreted TEXT,
  subject TEXT,
  vector_subject VECTOR(1024),
  vector_content VECTOR(1024),
  content_sha256 TEXT NOT NULL DEFAULT '',
  size_bytes BIGINT NOT NULL DEFAULT 0,
  is_deleted BOOLEAN NOT NULL DEFAULT FALSE,
  updated_unix_ms BIGINT NOT NULL,
  content_version BIGINT NOT NULL DEFAULT 1,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  last_change_id BIGINT REFERENCES vault_changes(id) ON DELETE SET NULL,
  UNIQUE (vault_id, file_path)
);

CREATE INDEX IF NOT EXISTS vault_files_vault_updated_idx
ON vault_files (vault_id, updated_unix_ms DESC);

CREATE INDEX IF NOT EXISTS vault_files_vault_live_idx
ON vault_files (vault_id, is_deleted, file_path);

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_constraint
    WHERE conname = 'vault_changes_file_id_fkey'
  ) THEN
    ALTER TABLE vault_changes
    ADD CONSTRAINT vault_changes_file_id_fkey
    FOREIGN KEY (file_id) REFERENCES vault_files(id) ON DELETE SET NULL;
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS file_atoms (
  file_id BIGINT NOT NULL REFERENCES vault_files(id) ON DELETE CASCADE,
  atom_id BIGINT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (file_id, atom_id)
);

CREATE INDEX IF NOT EXISTS file_atoms_atom_idx
ON file_atoms (atom_id);

CREATE TABLE IF NOT EXISTS file_links (
  id BIGSERIAL PRIMARY KEY,
  file_a_id BIGINT NOT NULL REFERENCES vault_files(id) ON DELETE CASCADE,
  file_b_id BIGINT NOT NULL REFERENCES vault_files(id) ON DELETE CASCADE,
  subject TEXT,
  absolute DOUBLE PRECISION,
  atoms BIGINT[] DEFAULT ARRAY[]::BIGINT[],
  content TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE (file_a_id, file_b_id),
  CHECK (file_a_id <> file_b_id)
);

CREATE INDEX IF NOT EXISTS file_links_a_idx
ON file_links (file_a_id);

CREATE INDEX IF NOT EXISTS file_links_b_idx
ON file_links (file_b_id);

CREATE TABLE IF NOT EXISTS file_processing_jobs (
  id BIGSERIAL PRIMARY KEY,
  vault_id BIGINT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
  file_id BIGINT NOT NULL REFERENCES vault_files(id) ON DELETE CASCADE,
  job_type TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  file_version BIGINT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  locked_by TEXT,
  available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  started_at TIMESTAMPTZ,
  finished_at TIMESTAMPTZ,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CHECK (job_type IN ('file_enrichment', 'atomize')),
  CHECK (status IN ('pending', 'running', 'completed', 'failed', 'superseded')),
  UNIQUE (file_id, job_type, file_version)
);

CREATE INDEX IF NOT EXISTS file_processing_jobs_pending_idx
ON file_processing_jobs (status, job_type, available_at, id);
