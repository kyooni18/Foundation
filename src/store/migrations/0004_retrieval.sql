-- Retrieval and queue-driven derived processing. Runs with foreign_keys=OFF inside one
-- transaction; the runner verifies PRAGMA foreign_key_check before commit (12-step rebuild).
-- 1. Jobs: new processing kinds, deferred/cancelled states, priority, generation fencing,
--    per-stage timing, and a small detail field (embedding model key). Every row keeps its id.
DROP TRIGGER job_target_exists;
DROP TRIGGER job_identity_fixed;
CREATE TABLE job_v4(
 id TEXT PRIMARY KEY NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('extract','revalidate','index_lexical','index_embed')),
 target_id TEXT NOT NULL, idem_key TEXT NOT NULL UNIQUE,
 state TEXT NOT NULL CHECK(state IN ('pending','leased','done','failed','deferred','cancelled')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0),
 max_attempts INTEGER NOT NULL DEFAULT 3 CHECK(max_attempts BETWEEN 1 AND 10),
 lease_owner TEXT, lease_until TEXT, run_after TEXT NOT NULL, last_error TEXT,
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 priority INTEGER NOT NULL DEFAULT 100 CHECK(priority BETWEEN 0 AND 1000),
 generation INTEGER, detail TEXT,
 first_leased_at TEXT, started_at TEXT, finished_at TEXT
) STRICT;
INSERT INTO job_v4(id,kind,target_id,idem_key,state,attempts,max_attempts,lease_owner,lease_until,run_after,last_error,created_at,updated_at)
 SELECT id,kind,target_id,idem_key,state,attempts,max_attempts,lease_owner,lease_until,run_after,last_error,created_at,updated_at FROM job;
DROP TABLE job;
ALTER TABLE job_v4 RENAME TO job;
CREATE INDEX job_target ON job(target_id);
CREATE INDEX job_ready ON job(state,run_after,lease_until);
CREATE INDEX job_kind_ready ON job(kind,state,run_after);
CREATE INDEX job_dispatch ON job(kind,state,priority DESC,created_at,id);
CREATE INDEX job_open ON job(kind) WHERE state IN ('pending','leased','deferred');
CREATE TRIGGER job_target_exists BEFORE INSERT ON job WHEN
 (NEW.kind IN ('extract','index_lexical','index_embed') AND NOT EXISTS(SELECT 1 FROM source WHERE id=NEW.target_id)) OR
 (NEW.kind='revalidate' AND NOT EXISTS(SELECT 1 FROM claim WHERE id=NEW.target_id))
 BEGIN SELECT RAISE(ABORT,'unknown job target'); END;
CREATE TRIGGER job_identity_fixed BEFORE UPDATE OF id,kind,target_id,idem_key,created_at ON job
 BEGIN SELECT RAISE(ABORT,'job identity is immutable'); END;
-- 2. Authorization projection, maintained in the same transaction as every lifecycle,
--    validity, confirmation or publication change (DATA_CONTRACTS §10). Derived, rebuildable
--    and independently checked; every retrieval result is additionally rechecked against
--    the authoritative tables.
CREATE TABLE claim_access(
 claim_id TEXT PRIMARY KEY NOT NULL REFERENCES claim(id),
 user_visible INTEGER NOT NULL CHECK(user_visible IN (0,1)),
 agent_visible INTEGER NOT NULL CHECK(agent_visible IN (0,1))
) STRICT, WITHOUT ROWID;
CREATE INDEX claim_access_user ON claim_access(claim_id) WHERE user_visible=1;
CREATE INDEX claim_access_agent ON claim_access(claim_id) WHERE agent_visible=1;
-- 3. Lexical index (derived): one row per indexed Claim and a contentless FTS5 table over
--    pre-tokenised character bigrams and ASCII words. Versioned by `tokenizer`.
CREATE TABLE claim_lex(
 lex_id INTEGER PRIMARY KEY,
 claim_id TEXT NOT NULL UNIQUE REFERENCES claim(id),
 tokenizer INTEGER NOT NULL, ntok INTEGER NOT NULL, text_sha TEXT NOT NULL, indexed_at TEXT NOT NULL
) STRICT;
CREATE VIRTUAL TABLE claim_fts USING fts5(terms, content='', contentless_delete=1, detail=none,
 tokenize="unicode61 remove_diacritics 0 tokenchars '_./:#@+-'", prefix='1');
-- 4. Semantic index (derived): vectors are isolated by embedding model identity.
CREATE TABLE embedding_model(
 model_key TEXT PRIMARY KEY NOT NULL, provider TEXT NOT NULL, model TEXT NOT NULL, revision TEXT NOT NULL,
 dimensions INTEGER NOT NULL CHECK(dimensions>0), query_prefix TEXT NOT NULL, document_prefix TEXT NOT NULL,
 created_at TEXT NOT NULL
) STRICT;
CREATE TABLE claim_vec(
 vec_id INTEGER PRIMARY KEY,
 claim_id TEXT NOT NULL REFERENCES claim(id),
 model_key TEXT NOT NULL REFERENCES embedding_model(model_key),
 text_sha TEXT NOT NULL, vec BLOB NOT NULL, created_at TEXT NOT NULL,
 UNIQUE(model_key,claim_id)
) STRICT;
CREATE INDEX claim_vec_claim ON claim_vec(claim_id);
-- 5. Version stamps for in-memory caches; bumped in the transaction that changes the data.
CREATE TABLE index_epoch(name TEXT PRIMARY KEY NOT NULL, value INTEGER NOT NULL) STRICT;
INSERT INTO index_epoch(name,value) VALUES('access',0),('lexical',0),('vectors',0);
