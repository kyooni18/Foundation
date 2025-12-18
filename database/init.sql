CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS docs (
  id        bigserial PRIMARY KEY,
  content   text NOT NULL,
  metadata  jsonb,
  embedding vector(1024)
);

CREATE INDEX IF NOT EXISTS docs_emb_hnsw
ON docs USING hnsw (embedding vector_cosine_ops);