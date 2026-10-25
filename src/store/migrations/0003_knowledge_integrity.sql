-- Phase 4 knowledge integrity. Runs with foreign_keys=OFF inside one transaction;
-- the runner verifies PRAGMA foreign_key_check before commit (SQLite 12-step rebuild).
-- 1. Lifecycle events gain dependency/citation/identity/relation targets. Every row keeps its seq.
DROP TRIGGER status_no_update;
DROP TRIGGER status_no_delete;
DROP TRIGGER status_source_exists;
DROP TRIGGER status_protect_user_source;
DROP TRIGGER status_restore_forgotten;
DROP TRIGGER status_actor_scope;
DROP VIEW status_owner;
DROP INDEX status_event_target;
DROP INDEX status_event_source;
ALTER TABLE status_event RENAME TO status_event_v2;
CREATE TABLE status_event(seq INTEGER PRIMARY KEY AUTOINCREMENT,
 target_kind TEXT NOT NULL CHECK(target_kind IN ('source','claim','dependency','citation','identity_assertion','relation')),
 target_id TEXT NOT NULL,event TEXT NOT NULL CHECK(event IN ('retract','forget','restore','confirm','unconfirm')),
 actor TEXT NOT NULL CHECK(actor IN ('user','agent','import','system')),reason TEXT,created_at TEXT NOT NULL,
 CHECK(target_kind='claim' OR event IN ('retract','forget','restore'))) STRICT;
INSERT INTO status_event SELECT * FROM status_event_v2;
DROP TABLE status_event_v2;
CREATE INDEX status_event_target ON status_event(target_kind,target_id,seq);
CREATE INDEX status_event_source ON status_event(target_id);
-- 2. Dependencies may be declared after both Claims exist, by an authenticated role or system.
DROP TRIGGER dependency_no_update;
DROP TRIGGER dependency_no_delete;
DROP TRIGGER dependency_cycle;
DROP INDEX dependency_child;
DROP INDEX dependency_parent;
ALTER TABLE claim_dependency RENAME TO claim_dependency_v2;
CREATE TABLE claim_dependency(id TEXT PRIMARY KEY NOT NULL,child TEXT NOT NULL REFERENCES claim(id),parent TEXT NOT NULL REFERENCES claim(id),
 kind TEXT NOT NULL CHECK(kind IN ('premise','restates','alternative','informed_by')),
 declared_by TEXT NOT NULL CHECK(declared_by IN ('system','user','agent','import')),created_at TEXT NOT NULL,
 CHECK(child<>parent),UNIQUE(child,parent,kind)) STRICT;
INSERT INTO claim_dependency SELECT * FROM claim_dependency_v2;
DROP TABLE claim_dependency_v2;
CREATE INDEX dependency_child ON claim_dependency(child);
CREATE INDEX dependency_parent ON claim_dependency(parent);
CREATE TRIGGER dependency_no_update BEFORE UPDATE ON claim_dependency BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER dependency_no_delete BEFORE DELETE ON claim_dependency BEGIN SELECT RAISE(ABORT,'append-only'); END;
-- Acyclicity covers every edge, including retracted ones, so a later restore cannot form a cycle.
CREATE TRIGGER dependency_cycle BEFORE INSERT ON claim_dependency WHEN EXISTS(
 WITH RECURSIVE ancestors(id) AS (SELECT NEW.parent UNION SELECT parent FROM claim_dependency JOIN ancestors ON child=ancestors.id) SELECT 1 FROM ancestors WHERE id=NEW.child)
 BEGIN SELECT RAISE(ABORT,'dependency cycle'); END;
CREATE TRIGGER dependency_declarer_scope BEFORE INSERT ON claim_dependency WHEN NEW.declared_by IN ('agent','import')
 AND NOT EXISTS(SELECT 1 FROM claim c JOIN source s ON s.id=c.source_id WHERE c.id=NEW.child AND s.recorder=NEW.declared_by)
 BEGIN SELECT RAISE(ABORT,'declarer cannot change this claim'); END;
-- 3. Jobs gain deterministic revalidation; target existence is checked by kind.
CREATE TABLE job_v3(
 id TEXT PRIMARY KEY NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('extract','revalidate')), target_id TEXT NOT NULL,
 idem_key TEXT NOT NULL UNIQUE, state TEXT NOT NULL CHECK(state IN ('pending','leased','done','failed')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0), max_attempts INTEGER NOT NULL DEFAULT 3 CHECK(max_attempts BETWEEN 1 AND 10),
 lease_owner TEXT, lease_until TEXT, run_after TEXT NOT NULL, last_error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
) STRICT;
INSERT INTO job_v3 SELECT * FROM job;
DROP TABLE job;
ALTER TABLE job_v3 RENAME TO job;
CREATE INDEX job_target ON job(target_id);
CREATE INDEX job_ready ON job(state,run_after,lease_until);
CREATE INDEX job_kind_ready ON job(kind,state,run_after);
CREATE TRIGGER job_target_exists BEFORE INSERT ON job WHEN
 (NEW.kind='extract' AND NOT EXISTS(SELECT 1 FROM source WHERE id=NEW.target_id)) OR
 (NEW.kind='revalidate' AND NOT EXISTS(SELECT 1 FROM claim WHERE id=NEW.target_id))
 BEGIN SELECT RAISE(ABORT,'unknown job target'); END;
CREATE TRIGGER job_identity_fixed BEFORE UPDATE OF id,kind,target_id,idem_key,created_at ON job
 BEGIN SELECT RAISE(ABORT,'job identity is immutable'); END;
CREATE TABLE revalidation_request(job_id TEXT PRIMARY KEY NOT NULL REFERENCES job(id),claim_id TEXT NOT NULL REFERENCES claim(id),
 cause TEXT NOT NULL CHECK(cause IN ('dependency','session_read','provenance','identity')),created_at TEXT NOT NULL) STRICT;
CREATE INDEX revalidation_request_claim ON revalidation_request(claim_id);
CREATE TABLE revalidation_result(job_id TEXT PRIMARY KEY NOT NULL REFERENCES job(id),claim_id TEXT NOT NULL REFERENCES claim(id),
 outcome TEXT NOT NULL CHECK(outcome IN ('resolved','repaired','semantic_pending')),
 validity TEXT NOT NULL CHECK(validity IN ('valid','needs_revalidation','invalid')),reasons TEXT NOT NULL,created_at TEXT NOT NULL) STRICT;
CREATE INDEX revalidation_result_claim ON revalidation_result(claim_id);
CREATE TRIGGER revalidation_request_no_update BEFORE UPDATE ON revalidation_request BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER revalidation_request_no_delete BEFORE DELETE ON revalidation_request BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER revalidation_result_no_update BEFORE UPDATE ON revalidation_result BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER revalidation_result_no_delete BEFORE DELETE ON revalidation_result BEGIN SELECT RAISE(ABORT,'append-only'); END;
-- 4. Provenance. Citations are not validity dependencies.
CREATE TABLE source_citation(id TEXT PRIMARY KEY NOT NULL,child_source TEXT NOT NULL REFERENCES source(id),parent_source TEXT NOT NULL REFERENCES source(id),
 actor TEXT NOT NULL CHECK(actor IN ('user','agent','import')),created_at TEXT NOT NULL,
 CHECK(child_source<>parent_source),UNIQUE(child_source,parent_source)) STRICT;
CREATE INDEX citation_child ON source_citation(child_source);
CREATE INDEX citation_parent ON source_citation(parent_source);
CREATE TRIGGER citation_no_update BEFORE UPDATE ON source_citation BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER citation_no_delete BEFORE DELETE ON source_citation BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER citation_cycle BEFORE INSERT ON source_citation WHEN EXISTS(
 WITH RECURSIVE cited(id) AS (SELECT NEW.parent_source UNION SELECT parent_source FROM source_citation JOIN cited ON child_source=cited.id) SELECT 1 FROM cited WHERE id=NEW.child_source)
 BEGIN SELECT RAISE(ABORT,'citation cycle'); END;
CREATE TRIGGER citation_scope BEFORE INSERT ON source_citation WHEN NEW.actor<>'user'
 AND NOT EXISTS(SELECT 1 FROM source WHERE id=NEW.child_source AND recorder=NEW.actor)
 BEGIN SELECT RAISE(ABORT,'actor cannot cite for this source'); END;
CREATE TABLE session_read(seq INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT NOT NULL CHECK(length(session_id) BETWEEN 1 AND 200),
 claim_id TEXT NOT NULL REFERENCES claim(id),actor TEXT NOT NULL CHECK(actor='agent'),at TEXT NOT NULL,UNIQUE(session_id,claim_id)) STRICT;
CREATE INDEX session_read_claim ON session_read(claim_id);
CREATE INDEX source_session ON source(session_id,captured_at);
CREATE TRIGGER session_read_no_update BEFORE UPDATE ON session_read BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER session_read_no_delete BEFORE DELETE ON session_read BEGIN SELECT RAISE(ABORT,'append-only'); END;
-- 5. Model judgments are immutable records, never assertions.
CREATE TABLE model_judgment(id TEXT PRIMARY KEY NOT NULL,task TEXT NOT NULL CHECK(task IN ('conflict','temporal')),
 claim_a TEXT NOT NULL REFERENCES claim(id),claim_b TEXT NOT NULL REFERENCES claim(id),provider TEXT NOT NULL,model_id TEXT NOT NULL,
 prompt_hash TEXT NOT NULL,input_digest TEXT NOT NULL,raw_label TEXT NOT NULL,output_json TEXT NOT NULL,created_at TEXT NOT NULL,
 CHECK(claim_a<>claim_b),
 CHECK((task='temporal' AND raw_label IN ('supersedes','exception','backfill','compatible')) OR
       (task='conflict' AND raw_label IN ('contradiction','potential','temporal_change','related','insufficient','none'))),
 UNIQUE(task,claim_a,claim_b,model_id,prompt_hash,input_digest)) STRICT;
CREATE INDEX judgment_a ON model_judgment(claim_a);
CREATE INDEX judgment_b ON model_judgment(claim_b);
CREATE TRIGGER judgment_no_update BEFORE UPDATE ON model_judgment BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER judgment_no_delete BEFORE DELETE ON model_judgment BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TABLE relation(id TEXT PRIMARY KEY NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('supersedes','exception','backfill','contradiction','not_conflicting')),
 subject TEXT NOT NULL REFERENCES claim(id),object TEXT NOT NULL REFERENCES claim(id),actor TEXT NOT NULL CHECK(actor IN ('user','agent')),
 judgment_id TEXT REFERENCES model_judgment(id),reason TEXT,created_at TEXT NOT NULL,
 CHECK(subject<>object),UNIQUE(kind,subject,object,actor)) STRICT;
CREATE INDEX relation_subject ON relation(subject);
CREATE INDEX relation_object ON relation(object);
CREATE INDEX relation_judgment ON relation(judgment_id);
CREATE TRIGGER relation_no_update BEFORE UPDATE ON relation BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER relation_no_delete BEFORE DELETE ON relation BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER relation_user_only BEFORE INSERT ON relation WHEN NEW.kind IN ('contradiction','not_conflicting') AND NEW.actor<>'user'
 BEGIN SELECT RAISE(ABORT,'only user can confirm conflict relations'); END;
CREATE TRIGGER relation_supersedes_cycle BEFORE INSERT ON relation WHEN NEW.kind='supersedes' AND EXISTS(
 WITH RECURSIVE older(id) AS (SELECT NEW.object UNION SELECT r.object FROM relation r JOIN older ON r.subject=older.id WHERE r.kind='supersedes') SELECT 1 FROM older WHERE id=NEW.subject)
 BEGIN SELECT RAISE(ABORT,'supersedes cycle'); END;
-- 6. Identity: thin entities and append-only assertions; resolution is derived.
CREATE TABLE entity(id TEXT PRIMARY KEY NOT NULL,kind TEXT NOT NULL CHECK(length(kind) BETWEEN 1 AND 64),
 created_by TEXT NOT NULL CHECK(created_by IN ('user','agent','model')),created_at TEXT NOT NULL) STRICT;
CREATE TRIGGER entity_no_update BEFORE UPDATE ON entity BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER entity_no_delete BEFORE DELETE ON entity BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TABLE identity_assertion(id TEXT PRIMARY KEY NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('refers_to','same_as','distinct_from','alias')),
 subject TEXT NOT NULL,object TEXT NOT NULL CHECK(length(CAST(object AS BLOB)) BETWEEN 1 AND 1024),
 actor TEXT NOT NULL CHECK(actor IN ('user','agent','model')),reason TEXT,judgment_id TEXT REFERENCES model_judgment(id),
 job_id TEXT REFERENCES job(id),created_at TEXT NOT NULL,
 CHECK((actor='model')=(job_id IS NOT NULL)),CHECK(kind NOT IN ('same_as','distinct_from') OR subject<object)) STRICT;
CREATE INDEX identity_subject ON identity_assertion(subject);
CREATE INDEX identity_object ON identity_assertion(object);
CREATE INDEX identity_job ON identity_assertion(job_id);
CREATE INDEX identity_judgment ON identity_assertion(judgment_id);
CREATE TRIGGER identity_no_update BEFORE UPDATE ON identity_assertion BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER identity_no_delete BEFORE DELETE ON identity_assertion BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER identity_targets BEFORE INSERT ON identity_assertion WHEN
 (NEW.kind='refers_to' AND (NOT EXISTS(SELECT 1 FROM mention WHERE id=NEW.subject) OR NOT EXISTS(SELECT 1 FROM entity WHERE id=NEW.object))) OR
 (NEW.kind IN ('same_as','distinct_from') AND (NOT EXISTS(SELECT 1 FROM entity WHERE id=NEW.subject) OR NOT EXISTS(SELECT 1 FROM entity WHERE id=NEW.object))) OR
 (NEW.kind='alias' AND NOT EXISTS(SELECT 1 FROM entity WHERE id=NEW.subject))
 BEGIN SELECT RAISE(ABORT,'invalid identity target'); END;
-- 7. Lifecycle ownership for every target kind, then the Phase 1/3 transition triggers.
CREATE VIEW status_owner AS
 SELECT 'source' AS target_kind,id AS target_id,recorder FROM source
 UNION ALL SELECT 'claim',c.id,s.recorder FROM claim c JOIN source s ON s.id=c.source_id
 UNION ALL SELECT 'dependency',d.id,CASE WHEN d.declared_by='user' THEN 'user' ELSE s.recorder END FROM claim_dependency d JOIN claim c ON c.id=d.child JOIN source s ON s.id=c.source_id
 UNION ALL SELECT 'citation',x.id,CASE WHEN x.actor='user' THEN 'user' ELSE s.recorder END FROM source_citation x JOIN source s ON s.id=x.child_source
 UNION ALL SELECT 'identity_assertion',id,actor FROM identity_assertion
 UNION ALL SELECT 'relation',id,actor FROM relation;
CREATE TRIGGER status_source_exists BEFORE INSERT ON status_event WHEN NOT EXISTS(SELECT 1 FROM status_owner WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id)
 BEGIN SELECT RAISE(ABORT,'unknown target'); END;
CREATE TRIGGER status_protect_user_source BEFORE INSERT ON status_event WHEN NEW.actor<>'user' AND EXISTS(SELECT 1 FROM status_owner WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id AND recorder='user')
 BEGIN SELECT RAISE(ABORT,'only user can change user source'); END;
CREATE TRIGGER status_restore_forgotten BEFORE INSERT ON status_event WHEN NEW.event='restore' AND NEW.actor<>'user' AND (SELECT event FROM status_event WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1)='forget'
 BEGIN SELECT RAISE(ABORT,'only user can restore forgotten target'); END;
CREATE TRIGGER status_actor_scope BEFORE INSERT ON status_event WHEN NEW.actor='import'
 OR (NEW.event IN ('confirm','unconfirm') AND NEW.actor<>'user')
 OR (NEW.actor='agent' AND NOT EXISTS(SELECT 1 FROM status_owner WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id AND recorder='agent'))
 OR (NEW.actor='system' AND (NEW.event<>'retract' OR NEW.reason IS NULL OR length(trim(NEW.reason))=0))
 BEGIN SELECT RAISE(ABORT,'actor cannot perform transition'); END;
CREATE TRIGGER status_no_update BEFORE UPDATE ON status_event BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER status_no_delete BEFORE DELETE ON status_event BEGIN SELECT RAISE(ABORT,'append-only'); END;
-- 8. Derived, rebuildable state (not authoritative; rebuilt from the tables above).
CREATE TABLE claim_validity(claim_id TEXT PRIMARY KEY NOT NULL REFERENCES claim(id),
 state TEXT NOT NULL CHECK(state IN ('valid','needs_revalidation','invalid')),reasons TEXT NOT NULL,
 deps_unknown INTEGER NOT NULL CHECK(deps_unknown IN (0,1))) STRICT;
CREATE TABLE entity_resolution(entity_id TEXT PRIMARY KEY NOT NULL REFERENCES entity(id),representative TEXT NOT NULL REFERENCES entity(id)) STRICT;
CREATE INDEX entity_resolution_representative ON entity_resolution(representative);
CREATE TABLE mention_resolution(mention_id TEXT PRIMARY KEY NOT NULL REFERENCES mention(id),entity_id TEXT NOT NULL REFERENCES entity(id),assertion_id TEXT NOT NULL REFERENCES identity_assertion(id)) STRICT;
CREATE INDEX mention_resolution_entity ON mention_resolution(entity_id);
CREATE TABLE entity_alias(representative TEXT NOT NULL REFERENCES entity(id),alias TEXT NOT NULL,origin TEXT NOT NULL CHECK(origin IN ('assertion','mention')),
 origin_id TEXT NOT NULL,PRIMARY KEY(representative,origin,origin_id)) STRICT;
CREATE INDEX entity_alias_text ON entity_alias(alias);
CREATE TABLE identity_blocked(assertion_id TEXT PRIMARY KEY NOT NULL REFERENCES identity_assertion(id),blocked_by TEXT NOT NULL) STRICT;
