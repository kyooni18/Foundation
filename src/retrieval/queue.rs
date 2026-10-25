//! Queue-driven derived processing on the existing SQLite job table (D-12, ARCHITECTURE §3).
//!
//! Recording a Source only stores it and enqueues its extraction job in one transaction.
//! Publishing approved Claims enqueues `index_lexical` and `index_embed` in the publication
//! transaction. Both are idempotent, at-least-once and fenced:
//! - handlers recompute their work from authoritative rows at execution time, so a duplicate,
//!   late or stale delivery cannot write anything a newer one has not already made true;
//! - visibility is *never* a job effect (see `access`): a completed or pending index job can
//!   neither reveal nor retain a hidden Claim;
//! - inference runs with no transaction open, and writes happen in short fenced transactions.
use super::{access, lexical};
use crate::{
    App,
    domain::*,
    embedding::{self, EmbedError, EmbeddingConfig, EmbeddingIdentity, EmbeddingProvider, Role},
    jobs::{Lease, fenced},
    store::integrity::sha256,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::time::Instant;

pub const PRIORITY_LEXICAL: i64 = 90;
/// Below interactive retrieval and below correctness-critical jobs (extract/revalidate = 100).
pub const PRIORITY_EMBED: i64 = 20;

/// Configurable resource limits and batching.
#[derive(Debug, Clone)]
pub struct QueueLimits {
    /// Ingestion is refused (queue_full) beyond this many open jobs.
    pub max_open_jobs: usize,
    pub lexical_jobs_per_batch: usize,
    pub embed_jobs_per_batch: usize,
    /// Texts per embedding call.
    pub embed_claims_per_call: usize,
    pub lexical_lease_seconds: i64,
    pub embed_lease_seconds: i64,
    pub retry_base_seconds: i64,
}
impl Default for QueueLimits {
    fn default() -> Self {
        Self {
            max_open_jobs: 500_000,
            lexical_jobs_per_batch: 64,
            embed_jobs_per_batch: 64,
            embed_claims_per_call: 128,
            lexical_lease_seconds: 60,
            embed_lease_seconds: 900,
            retry_base_seconds: 5,
        }
    }
}

pub(crate) fn ensure_model(conn: &Connection, id: &EmbeddingIdentity) -> Result<()> {
    conn.prepare_cached("INSERT OR IGNORE INTO embedding_model(model_key,provider,model,revision,dimensions,query_prefix,document_prefix,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)")?
        .execute(params![id.key, id.provider, id.model, id.revision, id.dimensions as i64, id.query_prefix, id.document_prefix, Timestamp::now().as_str()])?;
    Ok(())
}
fn insert_job(
    conn: &Connection,
    kind: &str,
    source: &str,
    key: &str,
    priority: i64,
    generation: Option<i64>,
    detail: Option<&str>,
) -> Result<bool> {
    let now = Timestamp::now();
    Ok(conn
        .prepare_cached("INSERT INTO job(id,kind,target_id,idem_key,state,max_attempts,run_after,created_at,updated_at,priority,generation,detail) VALUES(?1,?2,?3,?4,'pending',5,?5,?5,?5,?6,?7,?8) ON CONFLICT(idem_key) DO NOTHING")?
        .execute(params![ulid::Ulid::new().to_string(), kind, source, key, now.as_str(), priority, generation, detail])?
        == 1)
}
/// Called in the publication transaction: derived work for the new expression of a Source.
pub(crate) fn enqueue_indexing(
    conn: &Connection,
    source: &str,
    run_seq: i64,
    embedding: &EmbeddingConfig,
) -> Result<()> {
    insert_job(conn, "index_lexical", source, &format!("index_lexical/{source}/{run_seq}"), PRIORITY_LEXICAL, Some(run_seq), None)?;
    if let Some(id) = embedding.identity() {
        ensure_model(conn, &id)?;
        insert_job(conn, "index_embed", source, &format!("index_embed/{}/{source}/{run_seq}", id.key), PRIORITY_EMBED, Some(run_seq), Some(&id.key))?;
    }
    Ok(())
}
/// Backpressure: bounded queue growth. Checked inside the ingestion transaction.
pub(crate) fn admit(conn: &Connection, limits: &QueueLimits) -> Result<()> {
    let open: i64 = conn
        .prepare_cached("SELECT count(*) FROM job WHERE state IN ('pending','leased','deferred')")?
        .query_row([], |r| r.get(0))?;
    if open as usize >= limits.max_open_jobs {
        return Err(Error::QueueFull);
    }
    Ok(())
}

/// Derived rows missing for visible Claims (after a crash, restore, model change, restored
/// lifecycle or an old database) get coalesced jobs. Safe to call at any time.
pub(crate) fn reconcile(conn: &Connection, embedding: &EmbeddingConfig) -> Result<(usize, usize)> {
    let lexical: Vec<String> = conn
        .prepare("SELECT DISTINCT c.source_id FROM claim_access a JOIN claim c ON c.id=a.claim_id LEFT JOIN claim_lex l ON l.claim_id=a.claim_id WHERE (a.user_visible=1 OR a.agent_visible=1) AND (l.claim_id IS NULL OR l.tokenizer<>?1) AND NOT EXISTS(SELECT 1 FROM job j WHERE j.kind='index_lexical' AND j.target_id=c.source_id AND j.state IN ('pending','leased','deferred'))")?
        .query_map([super::tokenizer::VERSION], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let mut n_lex = 0;
    for source in &lexical {
        let seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq),0) FROM extraction_run WHERE source_id=?1", [source], |r| r.get(0))?;
        n_lex += usize::from(insert_job(conn, "index_lexical", source, &format!("index_lexical/{source}/{seq}/r{}", ulid::Ulid::new()), PRIORITY_LEXICAL, Some(seq), None)?);
    }
    let mut n_vec = 0;
    if let Some(id) = embedding.identity() {
        ensure_model(conn, &id)?;
        let sources: Vec<String> = conn
            .prepare("SELECT DISTINCT c.source_id FROM claim_access a JOIN claim c ON c.id=a.claim_id WHERE (a.user_visible=1 OR a.agent_visible=1) AND NOT EXISTS(SELECT 1 FROM claim_vec v WHERE v.claim_id=a.claim_id AND v.model_key=?1) AND NOT EXISTS(SELECT 1 FROM job j WHERE j.kind='index_embed' AND j.target_id=c.source_id AND j.detail=?1 AND j.state IN ('pending','leased','deferred'))")?
            .query_map([&id.key], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for source in &sources {
            let seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq),0) FROM extraction_run WHERE source_id=?1", [source], |r| r.get(0))?;
            n_vec += usize::from(insert_job(conn, "index_embed", source, &format!("index_embed/{}/{source}/{seq}/r{}", id.key, ulid::Ulid::new()), PRIORITY_EMBED, Some(seq), Some(&id.key))?);
        }
        // Jobs for other models are obsolete once the configured model changed.
        conn.execute("UPDATE job SET state='cancelled',last_error='obsolete',lease_owner=NULL,lease_until=NULL,finished_at=?2,updated_at=?2 WHERE kind='index_embed' AND state IN ('pending','deferred') AND detail<>?1", params![id.key, Timestamp::now().as_str()])?;
    }
    Ok((n_lex, n_vec))
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct ProcessReport {
    pub jobs: usize,
    pub claims: usize,
    pub cancelled: usize,
    pub deferred: usize,
    pub failed: usize,
    pub milliseconds: u128,
    /// Embedding calls made (model forward passes or HTTP requests are counted by the provider).
    pub embedded: usize,
}

fn code_for(e: EmbedError) -> &'static str {
    e.code()
}
impl App {
    fn lease_batch(
        &self,
        kind: &str,
        owner: &str,
        n: usize,
        seconds: i64,
        detail: Option<&str>,
    ) -> Result<Vec<Lease>> {
        let now = Timestamp::now();
        let until = Timestamp::plus_seconds(seconds);
        let mut w = self.store.writer()?;
        let tx = w.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE job SET state='failed',last_error='exhausted',lease_owner=NULL,lease_until=NULL,updated_at=?1,finished_at=?1 WHERE kind=?2 AND state='leased' AND lease_until<=?1 AND attempts>=max_attempts", params![now.as_str(), kind])?;
        let rows: Vec<(String, String, u32)> = tx
            .prepare_cached("SELECT id,target_id,attempts+1 FROM job WHERE kind=?1 AND attempts<max_attempts AND ((state='pending' AND run_after<=?2) OR (state='leased' AND lease_until<=?2)) AND (?3 IS NULL OR detail=?3) ORDER BY priority DESC,created_at,id LIMIT ?4")?
            .query_map(params![kind, now.as_str(), detail, n as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let mut leases = Vec::with_capacity(rows.len());
        for (id, source_id, attempt) in rows {
            tx.prepare_cached("UPDATE job SET state='leased',attempts=?2,lease_owner=?3,lease_until=?4,updated_at=?5,started_at=?5,first_leased_at=COALESCE(first_leased_at,?5) WHERE id=?1")?
                .execute(params![id, attempt, owner, until.as_str(), now.as_str()])?;
            leases.push(Lease { id, source_id, owner: owner.into(), attempt });
        }
        tx.commit()?;
        Ok(leases)
    }
    fn finish(tx: &Connection, lease: &Lease, state: &str, code: Option<&str>) -> Result<()> {
        let now = Timestamp::now();
        tx.prepare_cached("UPDATE job SET state=?2,last_error=?3,lease_owner=NULL,lease_until=NULL,updated_at=?4,finished_at=?4 WHERE id=?1")?
            .execute(params![lease.id, state, code, now.as_str()])?;
        Ok(())
    }
    fn job_generation(tx: &Connection, lease: &Lease) -> Result<(Option<i64>, i64)> {
        let generation: Option<i64> = tx.query_row("SELECT generation FROM job WHERE id=?1", [&lease.id], |r| r.get(0))?;
        let current: i64 = tx.query_row("SELECT COALESCE(MAX(seq),0) FROM extraction_run WHERE source_id=?1", [&lease.source_id], |r| r.get(0))?;
        Ok((generation, current))
    }
    /// Index leased Sources' visible Claims lexically. All jobs of a batch commit together.
    pub fn process_lexical_once(&self, owner: &str) -> Result<ProcessReport> {
        let started = Instant::now();
        let limits = &self.policy.queue;
        let leases = self.lease_batch("index_lexical", owner, limits.lexical_jobs_per_batch, limits.lexical_lease_seconds, None)?;
        let mut report = ProcessReport::default();
        if leases.is_empty() {
            return Ok(report);
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for lease in &leases {
            if fenced(&tx, lease).is_err() {
                continue; // expired or taken over: another delivery owns the effect
            }
            let (generation, current) = Self::job_generation(&tx, lease)?;
            if generation.is_some_and(|g| g != current) {
                Self::finish(&tx, lease, "cancelled", Some("obsolete"))?;
                report.cancelled += 1;
                continue;
            }
            let claims: Vec<(String, String)> = tx
                .prepare_cached("SELECT c.id,c.text FROM claim c JOIN claim_access a ON a.claim_id=c.id WHERE c.source_id=?1 AND (a.user_visible=1 OR a.agent_visible=1) ORDER BY c.id")?
                .query_map([&lease.source_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            report.claims += lexical::index_claims(&tx, &claims)?;
            Self::finish(&tx, lease, "done", None)?;
            report.jobs += 1;
        }
        tx.commit()?;
        report.milliseconds = started.elapsed().as_millis();
        Ok(report)
    }

    /// Embed visible Claims lacking a vector for the provider's model. Texts are gathered from
    /// authoritative rows, embedded with no transaction open, and written in short fenced
    /// transactions; completed work is never redone (`UNIQUE(model_key, claim_id)`).
    pub fn process_embeddings_once(
        &self,
        provider: &dyn EmbeddingProvider,
        owner: &str,
    ) -> Result<ProcessReport> {
        let started = Instant::now();
        let limits = self.policy.queue.clone();
        let id = provider.identity().clone();
        {
            let mut w = self.store.writer()?;
            let tx = w.transaction()?;
            ensure_model(&tx, &id)?;
            tx.commit()?;
        }
        let leases = self.lease_batch("index_embed", owner, limits.embed_jobs_per_batch, limits.embed_lease_seconds, Some(&id.key))?;
        let mut report = ProcessReport::default();
        if leases.is_empty() {
            return Ok(report);
        }
        // Gather work (read snapshot, no write transaction).
        struct Item {
            lease: usize,
            claim: String,
            text: String,
        }
        let mut items: Vec<Item> = Vec::new();
        {
            let conn = self.store.reader()?;
            for (i, lease) in leases.iter().enumerate() {
                let rows = conn
                    .prepare_cached("SELECT c.id,c.text FROM claim c JOIN claim_access a ON a.claim_id=c.id WHERE c.source_id=?1 AND (a.user_visible=1 OR a.agent_visible=1) AND NOT EXISTS(SELECT 1 FROM claim_vec v WHERE v.claim_id=c.id AND v.model_key=?2) ORDER BY c.id")?
                    .query_map(params![lease.source_id, id.key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                items.extend(rows.into_iter().map(|(claim, text)| Item { lease: i, claim, text }));
            }
        }
        let mut failure: Option<EmbedError> = None;
        for chunk in items.chunks(limits.embed_claims_per_call.max(1)) {
            // Keep the leases of this chunk alive during (possibly long) inference.
            {
                let w = self.store.writer()?;
                let until = Timestamp::plus_seconds(limits.embed_lease_seconds);
                for lease in &leases {
                    w.prepare_cached("UPDATE job SET lease_until=?2 WHERE id=?1 AND state='leased' AND lease_owner=?3 AND attempts=?4")?
                        .execute(params![lease.id, until.as_str(), lease.owner, lease.attempt])?;
                }
            }
            let texts: Vec<String> = chunk.iter().map(|i| i.text.clone()).collect();
            let vectors = match embedding::embed(provider, Role::Document, &texts) {
                Ok(v) => v,
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            };
            report.embedded += 1;
            let mut w = self.store.writer()?;
            let tx = w.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut live: Vec<bool> = vec![false; leases.len()];
            for (i, lease) in leases.iter().enumerate() {
                live[i] = fenced(&tx, lease).is_ok();
            }
            let mut inserted = 0;
            for (item, vector) in chunk.iter().zip(&vectors) {
                if !live[item.lease] {
                    continue;
                }
                // Recheck at write time: still visible somewhere, text unchanged (immutable).
                let current: Option<String> = tx
                    .prepare_cached("SELECT c.text FROM claim c JOIN claim_access a ON a.claim_id=c.id WHERE c.id=?1 AND (a.user_visible=1 OR a.agent_visible=1)")?
                    .query_row([&item.claim], |r| r.get(0))
                    .optional()?;
                let Some(current) = current else { continue };
                if sha256(current.as_bytes()) != sha256(item.text.as_bytes()) {
                    continue;
                }
                inserted += tx
                    .prepare_cached("INSERT OR IGNORE INTO claim_vec(claim_id,model_key,text_sha,vec,created_at) VALUES(?1,?2,?3,?4,?5)")?
                    .execute(params![item.claim, id.key, sha256(current.as_bytes()), encode(vector), Timestamp::now().as_str()])?;
            }
            if inserted > 0 {
                access::bump(&tx, "vectors")?;
            }
            report.claims += inserted;
            tx.commit()?;
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match failure {
            None => {
                for lease in &leases {
                    if fenced(&tx, lease).is_err() {
                        continue;
                    }
                    let (generation, current) = Self::job_generation(&tx, lease)?;
                    if generation.is_some_and(|g| g != current) {
                        Self::finish(&tx, lease, "cancelled", Some("obsolete"))?;
                        report.cancelled += 1;
                    } else {
                        Self::finish(&tx, lease, "done", None)?;
                        report.jobs += 1;
                    }
                }
            }
            Some(EmbedError::Unavailable) => {
                // The provider cannot run (model files or credentials missing): keep the jobs
                // safely deferred without consuming an attempt. Nothing is sent anywhere else.
                for lease in &leases {
                    if fenced(&tx, lease).is_ok() {
                        let now = Timestamp::now();
                        tx.execute("UPDATE job SET state='deferred',last_error='provider_unavailable',attempts=attempts-1,lease_owner=NULL,lease_until=NULL,updated_at=?2 WHERE id=?1", params![lease.id, now.as_str()])?;
                        report.deferred += 1;
                    }
                }
            }
            Some(e) => {
                for lease in &leases {
                    if fenced(&tx, lease).is_ok() {
                        let backoff = limits.retry_base_seconds << lease.attempt.min(8);
                        let now = Timestamp::now();
                        let retry = Timestamp::plus_seconds(backoff);
                        tx.execute("UPDATE job SET state=CASE WHEN attempts>=max_attempts THEN 'failed' ELSE 'pending' END,last_error=?2,run_after=?3,lease_owner=NULL,lease_until=NULL,updated_at=?4,finished_at=CASE WHEN attempts>=max_attempts THEN ?4 ELSE finished_at END WHERE id=?1", params![lease.id, code_for(e), retry.as_str(), now.as_str()])?;
                        report.failed += 1;
                    }
                }
            }
        }
        tx.commit()?;
        report.milliseconds = started.elapsed().as_millis();
        Ok(report)
    }
    /// Return deferred jobs (provider was unavailable) to the queue.
    pub fn resume_deferred(&self, p: &Principal, kind: Option<&str>) -> Result<usize> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let now = Timestamp::now();
        Ok(self.store.writer()?.execute(
            "UPDATE job SET state='pending',run_after=?1,updated_at=?1,last_error=NULL WHERE state='deferred' AND (?2 IS NULL OR kind=?2)",
            params![now.as_str(), kind],
        )?)
    }
    /// Explicit owner retry of a recoverable failure. Extraction output awaiting the user's
    /// approval (`review_required`) is not a failure and cannot be retried away.
    pub fn retry_job(&self, p: &Principal, job_id: &str) -> Result<bool> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let now = Timestamp::now();
        let changed = self.store.writer()?.execute(
            "UPDATE job SET state='pending',attempts=0,run_after=?2,updated_at=?2,last_error=NULL,lease_owner=NULL,lease_until=NULL,finished_at=NULL WHERE id=?1 AND state IN ('failed','deferred','cancelled') AND COALESCE(last_error,'')<>'review_required'",
            params![job_id, now.as_str()],
        )?;
        if changed == 0 {
            let exists: bool = self.store.reader()?.query_row("SELECT EXISTS(SELECT 1 FROM job WHERE id=?1)", [job_id], |r| r.get(0))?;
            if !exists {
                return Err(Error::NotFound);
            }
        }
        Ok(changed == 1)
    }
    /// Drop all lexical rows and let the queue rebuild them from authoritative Claims.
    pub fn rebuild_lexical(&self, p: &Principal) -> Result<usize> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction_with_behavior(TransactionBehavior::Immediate)?;
        lexical::clear(&tx)?;
        let (n, _) = reconcile(&tx, &self.policy.embedding)?;
        tx.commit()?;
        Ok(n)
    }
    /// Drop the active model's vectors and let the queue re-embed.
    pub fn rebuild_vectors(&self, p: &Principal) -> Result<usize> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let Some(id) = self.policy.embedding.identity() else {
            return Ok(0);
        };
        let mut w = self.store.writer()?;
        let tx = w.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM claim_vec WHERE model_key=?1", [&id.key])?;
        access::bump(&tx, "vectors")?;
        let (_, n) = reconcile(&tx, &self.policy.embedding)?;
        tx.commit()?;
        Ok(n)
    }
    /// Run the reconciler (also done at open); returns (lexical, embedding) jobs created.
    pub fn reconcile_indexing(&self) -> Result<(usize, usize)> {
        let mut w = self.store.writer()?;
        let tx = w.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = reconcile(&tx, &self.policy.embedding)?;
        tx.commit()?;
        Ok(n)
    }
}
pub(crate) fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|x| half::f16::from_f32(*x).to_le_bytes()).collect()
}
pub(crate) fn decode(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|b| half::f16::from_le_bytes([b[0], b[1]]).to_f32())
        .collect()
}
