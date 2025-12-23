CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS primary_db ( 
  id BIGSERIAL PRIMARY KEY, 
  text TEXT NOT NULL, 
  embedding VECTOR(1024) NOT NULL, 
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
('$argon2id$v=19$m=64000,t=3,p=1$VHSUQ7l58UdHbrBltQfFKA$fMiRFZcgw8ri8W6bczyW8kODs6F+bOKROWqAcTDgqjyv8W7aPNKZyCebvkBtYVBomwb63B2bAXLQwE2Wz+AfSw', 'master_key')

CREATE TABLE IF NOT EXISTS archive (
  id BIGSERIAL PRIMARY KEY,
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  embedding VECTOR(1024) NOT NULL,
  atoms [BIGSERIAL] DEFAULT [],
  metadata jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
)

