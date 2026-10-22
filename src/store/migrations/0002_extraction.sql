CREATE TABLE job(
 id TEXT PRIMARY KEY NOT NULL, kind TEXT NOT NULL CHECK(kind='extract'), target_id TEXT NOT NULL REFERENCES source(id),
 idem_key TEXT NOT NULL UNIQUE, state TEXT NOT NULL CHECK(state IN ('pending','leased','done','failed')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0), max_attempts INTEGER NOT NULL DEFAULT 3 CHECK(max_attempts BETWEEN 1 AND 10),
 lease_owner TEXT, lease_until TEXT, run_after TEXT NOT NULL, last_error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
) STRICT;
CREATE INDEX job_target ON job(target_id);
CREATE INDEX job_ready ON job(state,run_after,lease_until);
CREATE TABLE extraction_request(job_id TEXT PRIMARY KEY NOT NULL REFERENCES job(id), plan_json TEXT NOT NULL, prompt_hash TEXT NOT NULL, base_job TEXT REFERENCES job(id), created_at TEXT NOT NULL) STRICT;
CREATE INDEX request_base ON extraction_request(base_job);
CREATE TABLE extraction_candidate(id TEXT PRIMARY KEY NOT NULL, job_id TEXT NOT NULL REFERENCES job(id), output_json TEXT NOT NULL, output_hash TEXT NOT NULL, reviews_json TEXT NOT NULL, created_at TEXT NOT NULL) STRICT;
CREATE INDEX candidate_job ON extraction_candidate(job_id);
CREATE TABLE extraction_run(seq INTEGER PRIMARY KEY AUTOINCREMENT,job_id TEXT NOT NULL UNIQUE REFERENCES job(id), source_id TEXT NOT NULL REFERENCES source(id), prior_job TEXT REFERENCES job(id), extractor_id TEXT NOT NULL, output_hash TEXT NOT NULL, approval_actor TEXT NOT NULL CHECK(approval_actor='user'), created_at TEXT NOT NULL) STRICT;
CREATE INDEX run_source ON extraction_run(source_id,seq);
CREATE INDEX run_prior ON extraction_run(prior_job);
CREATE TABLE claim(
 id TEXT PRIMARY KEY NOT NULL, source_id TEXT NOT NULL REFERENCES source(id), revision_of TEXT REFERENCES claim(id),
 text TEXT NOT NULL CHECK(length(CAST(text AS BLOB))>0), lang TEXT NOT NULL CHECK(lang IN ('ko','en')),
 category TEXT NOT NULL CHECK(category IN ('Fact','View','Intent','Rule')),
 speaker TEXT NOT NULL CHECK(speaker IN ('user','agent','external','unknown')), speaker_label TEXT,
 basis TEXT NOT NULL CHECK(basis IN ('verbatim','paraphrase','inferred','observed','restated','operational')),
 anchor_start INTEGER NOT NULL,anchor_end INTEGER NOT NULL, anchor_quote TEXT NOT NULL,
 tkind TEXT CHECK(tkind IN ('habit','state','event','observation','decision','report_past')),
 applies_from TEXT,applies_to TEXT,standing INTEGER NOT NULL CHECK(standing IN (0,1)),index_fields TEXT,
 extractor_id TEXT NOT NULL,job_id TEXT NOT NULL REFERENCES job(id),created_at TEXT NOT NULL,
 CHECK(anchor_start>=0 AND anchor_end>anchor_start)
) STRICT;
CREATE INDEX claim_source ON claim(source_id);
CREATE INDEX claim_revision ON claim(revision_of);
CREATE INDEX claim_job ON claim(job_id);
CREATE TABLE mention(id TEXT PRIMARY KEY NOT NULL,claim_id TEXT NOT NULL REFERENCES claim(id),surface TEXT NOT NULL,span TEXT NOT NULL) STRICT;
CREATE INDEX mention_claim ON mention(claim_id);
CREATE TABLE claim_dependency(id TEXT PRIMARY KEY NOT NULL,child TEXT NOT NULL REFERENCES claim(id),parent TEXT NOT NULL REFERENCES claim(id),kind TEXT NOT NULL CHECK(kind IN ('premise','restates','alternative','informed_by')),declared_by TEXT NOT NULL CHECK(declared_by='system'),created_at TEXT NOT NULL,CHECK(child<>parent),UNIQUE(child,parent,kind)) STRICT;
CREATE INDEX dependency_child ON claim_dependency(child);
CREATE INDEX dependency_parent ON claim_dependency(parent);
CREATE TRIGGER claim_anchor BEFORE INSERT ON claim WHEN
 NEW.anchor_end>(SELECT length(CAST(content AS BLOB)) FROM source WHERE id=NEW.source_id)
 OR CAST(NEW.anchor_quote AS BLOB)<>(SELECT substr(CAST(content AS BLOB),NEW.anchor_start+1,NEW.anchor_end-NEW.anchor_start) FROM source WHERE id=NEW.source_id)
 OR (NEW.basis='verbatim' AND NEW.text<>NEW.anchor_quote)
 OR (NEW.basis='observed' AND NOT EXISTS(SELECT 1 FROM source WHERE id=NEW.source_id AND artifact_ref IS NOT NULL))
 OR NOT EXISTS(SELECT 1 FROM job WHERE id=NEW.job_id AND target_id=NEW.source_id)
 OR (NEW.revision_of IS NOT NULL AND NOT EXISTS(SELECT 1 FROM claim WHERE id=NEW.revision_of AND source_id=NEW.source_id))
 BEGIN SELECT RAISE(ABORT,'invalid claim evidence'); END;
CREATE TRIGGER dependency_cycle BEFORE INSERT ON claim_dependency WHEN EXISTS(
 WITH RECURSIVE ancestors(id) AS (SELECT NEW.parent UNION SELECT parent FROM claim_dependency JOIN ancestors ON child=ancestors.id) SELECT 1 FROM ancestors WHERE id=NEW.child)
 BEGIN SELECT RAISE(ABORT,'dependency cycle'); END;
DROP TRIGGER status_no_update;
DROP TRIGGER status_no_delete;
DROP TRIGGER status_source_exists;
DROP TRIGGER status_protect_user_source;
DROP TRIGGER status_restore_forgotten;
DROP TRIGGER status_actor_scope;
DROP INDEX status_event_target;
DROP INDEX status_event_source;
ALTER TABLE status_event RENAME TO status_event_v1;
CREATE TABLE status_event(seq INTEGER PRIMARY KEY AUTOINCREMENT,target_kind TEXT NOT NULL CHECK(target_kind IN ('source','claim')),target_id TEXT NOT NULL,event TEXT NOT NULL CHECK(event IN ('retract','forget','restore','confirm','unconfirm')),actor TEXT NOT NULL CHECK(actor IN ('user','agent','import','system')),reason TEXT,created_at TEXT NOT NULL,CHECK(target_kind='claim' OR event IN ('retract','forget','restore'))) STRICT;
INSERT INTO status_event SELECT * FROM status_event_v1;
DROP TABLE status_event_v1;
CREATE INDEX status_event_target ON status_event(target_kind,target_id,seq);
CREATE INDEX status_event_source ON status_event(target_id);
CREATE TRIGGER status_source_exists BEFORE INSERT ON status_event WHEN
 (NEW.target_kind='source' AND NOT EXISTS(SELECT 1 FROM source WHERE id=NEW.target_id)) OR
 (NEW.target_kind='claim' AND NOT EXISTS(SELECT 1 FROM claim WHERE id=NEW.target_id))
 BEGIN SELECT RAISE(ABORT,'unknown target'); END;
CREATE VIEW status_owner AS SELECT 'source' AS target_kind,id AS target_id,recorder FROM source UNION ALL SELECT 'claim',c.id,s.recorder FROM claim c JOIN source s ON s.id=c.source_id;
CREATE TRIGGER status_protect_user_source BEFORE INSERT ON status_event WHEN NEW.actor<>'user' AND EXISTS(SELECT 1 FROM status_owner WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id AND recorder='user')
 BEGIN SELECT RAISE(ABORT,'only user can change user source'); END;
CREATE TRIGGER status_restore_forgotten BEFORE INSERT ON status_event WHEN NEW.event='restore' AND NEW.actor<>'user' AND (SELECT event FROM status_event WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1)='forget'
 BEGIN SELECT RAISE(ABORT,'only user can restore forgotten target'); END;
CREATE TRIGGER status_actor_scope BEFORE INSERT ON status_event WHEN NEW.actor='import'
 OR (NEW.event IN ('confirm','unconfirm') AND NEW.actor<>'user')
 OR (NEW.actor='agent' AND NOT EXISTS(SELECT 1 FROM status_owner WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id AND recorder='agent'))
 OR (NEW.actor='system' AND (NEW.event<>'retract' OR NEW.reason IS NULL OR length(trim(NEW.reason))=0))
 BEGIN SELECT RAISE(ABORT,'actor cannot perform transition'); END;
CREATE TRIGGER claim_no_update BEFORE UPDATE ON claim BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER claim_no_delete BEFORE DELETE ON claim BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER mention_no_update BEFORE UPDATE ON mention BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER mention_no_delete BEFORE DELETE ON mention BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER dependency_no_update BEFORE UPDATE ON claim_dependency BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER dependency_no_delete BEFORE DELETE ON claim_dependency BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER request_no_update BEFORE UPDATE ON extraction_request BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER request_no_delete BEFORE DELETE ON extraction_request BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER candidate_no_update BEFORE UPDATE ON extraction_candidate BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER candidate_no_delete BEFORE DELETE ON extraction_candidate BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER run_no_update BEFORE UPDATE ON extraction_run BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER run_no_delete BEFORE DELETE ON extraction_run BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER status_no_update BEFORE UPDATE ON status_event BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER status_no_delete BEFORE DELETE ON status_event BEGIN SELECT RAISE(ABORT,'append-only'); END;
