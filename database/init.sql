CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS atoms_db (
  id BIGSERIAL PRIMARY KEY,
  text TEXT NOT NULL,
  embedding VECTOR(1024) NOT NULL,
  parent TEXT DEFAULT NULL,
  metadata jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS atoms_db_emb_hnsw
ON atoms_db USING hnsw (embedding vector_cosine_ops);

CREATE TABLE IF NOT EXISTS auth_keys (
  id BIGSERIAL PRIMARY KEY,
  hashed_key TEXT NOT NULL,
  mask TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

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
