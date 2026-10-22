CREATE TABLE schema_migration(version INTEGER PRIMARY KEY, name TEXT NOT NULL,
  checksum TEXT NOT NULL, applied_at TEXT NOT NULL) STRICT;
CREATE TRIGGER migration_no_update BEFORE UPDATE ON schema_migration BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER migration_no_delete BEFORE DELETE ON schema_migration BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TABLE source(
  id TEXT PRIMARY KEY NOT NULL,
  recorder TEXT NOT NULL CHECK(recorder IN ('user','agent','import')),
  recorder_label TEXT, session_id TEXT,
  content TEXT NOT NULL CHECK(length(CAST(content AS BLOB)) BETWEEN 1 AND 1048576),
  content_sha256 TEXT NOT NULL CHECK(length(content_sha256)=64),
  media_type TEXT NOT NULL CHECK(media_type IN ('text/plain','text/markdown')),
  authored_at TEXT, observed_at TEXT, captured_at TEXT NOT NULL,
  artifact_ref TEXT,
  retention_request TEXT NOT NULL CHECK(retention_request IN ('none','user_requested')),
  idempotency_key TEXT, request_digest TEXT NOT NULL CHECK(length(request_digest)=64),
  UNIQUE(recorder,idempotency_key)
) STRICT;
CREATE TRIGGER source_no_update BEFORE UPDATE ON source BEGIN SELECT RAISE(ABORT,'source is immutable'); END;
CREATE TRIGGER source_no_delete BEFORE DELETE ON source BEGIN SELECT RAISE(ABORT,'source is immutable'); END;
CREATE TABLE status_event(
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  target_kind TEXT NOT NULL CHECK(target_kind='source'),
  target_id TEXT NOT NULL REFERENCES source(id),
  event TEXT NOT NULL CHECK(event IN ('retract','forget','restore')),
  actor TEXT NOT NULL CHECK(actor IN ('user','agent','import','system')),
  reason TEXT, created_at TEXT NOT NULL
) STRICT;
CREATE INDEX status_event_target ON status_event(target_kind,target_id,seq);
CREATE INDEX status_event_source ON status_event(target_id);
CREATE TRIGGER status_no_update BEFORE UPDATE ON status_event BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER status_no_delete BEFORE DELETE ON status_event BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER status_source_exists BEFORE INSERT ON status_event
 WHEN NOT EXISTS(SELECT 1 FROM source WHERE id=NEW.target_id)
 BEGIN SELECT RAISE(ABORT,'unknown target'); END;
CREATE TRIGGER status_protect_user_source BEFORE INSERT ON status_event
 WHEN NEW.actor<>'user' AND EXISTS(SELECT 1 FROM source WHERE id=NEW.target_id AND recorder='user')
 BEGIN SELECT RAISE(ABORT,'only user can change user source'); END;
CREATE TRIGGER status_restore_forgotten BEFORE INSERT ON status_event
 WHEN NEW.event='restore' AND NEW.actor<>'user'
 AND (SELECT event FROM status_event WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id ORDER BY seq DESC LIMIT 1)='forget'
 BEGIN SELECT RAISE(ABORT,'only user can restore forgotten target'); END;
CREATE TRIGGER status_actor_scope BEFORE INSERT ON status_event
 WHEN NEW.actor='import'
 OR (NEW.actor='agent' AND NOT EXISTS(SELECT 1 FROM source WHERE id=NEW.target_id AND recorder='agent'))
 OR (NEW.actor='system' AND (NEW.event<>'retract' OR NEW.reason IS NULL OR length(trim(NEW.reason))=0))
 BEGIN SELECT RAISE(ABORT,'actor cannot perform transition'); END;
