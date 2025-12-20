CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS primary_db ( 
  id BIGSERIAL PRIMARY KEY, 
  text TEXT NOT NULL, 
  embedding VECTOR(768) NOT NULL, 
  metadata jsonb, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  );

CREATE INDEX IF NOT EXISTS primary_db_emb_hnsw
ON primary_db USING hnsw (embedding vector_cosine_ops);

CREATE TABLE IF NOT EXISTS auth_keys (
  id BIGSERIAL PRIMARY KEY,
  hashed_key TEXT NOT NULL,
  mask TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO auth_keys (hashed_key, mask) VALUES
('$argon2id$v=19$m=64000,t=3,p=1$93OfDVtUjhIp3V+SVgnzDQ$l1sSI/jSOAX9fXSW1qkNh4uabKCPyEw7vaLlYbmPrPXJUcTNKv80v0zwY9glLCv+CBtIzHVHpfV251g89Kv2UQ', 'master_key')