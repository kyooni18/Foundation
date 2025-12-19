CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS primary_db ( 
  id BIGSERIAL PRIMARY KEY, 
  text TEXT NOT NULL, 
  embedding VECTOR(768) NOT NULL, 
  metadata jsonb, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  );

CREATE INDEX IF NOT EXISTS primary_db_emb_hnsw
ON primary_db USING hnsw (embedding vector_cosine_ops);