//! Observable processing state: per-Source stage, queue aggregates and timing percentiles.
//! Nothing here returns raw provider errors (only the safe diagnostic codes stored on jobs) and
//! per-Source detail is only produced for Sources the caller is already allowed to see.
use crate::{App, domain::*};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;

pub const STAGES: [&str; 9] = [
    "stored_not_extracted",
    "awaiting_approval",
    "published_awaiting_lexical",
    "awaiting_embedding",
    "fully_indexed",
    "no_visible_claims",
    "deferred",
    "failed",
    "stale",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Processing {
    pub stage: &'static str,
    pub last_error: Option<String>,
    pub attempts: i64,
}
fn processing(stage: &'static str, last_error: Option<String>, attempts: i64) -> Processing {
    Processing {
        stage,
        last_error,
        attempts,
    }
}

/// Where a Source is in its pipeline, computed from authoritative rows and job states.
pub fn source_stage(
    conn: &Connection,
    model_key: Option<&str>,
    source: &str,
) -> Result<Processing> {
    let extract: Option<(String, Option<String>, i64)> = conn
        .prepare_cached("SELECT state,last_error,attempts FROM job WHERE kind='extract' AND target_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1")?
        .query_row([source], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((state, error, attempts)) = extract else {
        return Ok(processing("stored_not_extracted", None, 0));
    };
    let published: bool = conn
        .prepare_cached("SELECT EXISTS(SELECT 1 FROM extraction_run WHERE source_id=?1)")?
        .query_row([source], |r| r.get(0))?;
    if !published {
        return Ok(match (state.as_str(), error.as_deref()) {
            ("failed", Some("review_required")) => processing("awaiting_approval", None, attempts),
            ("failed", _) => processing("failed", error, attempts),
            ("deferred", _) => processing("deferred", error, attempts),
            _ => processing("stored_not_extracted", error, attempts),
        });
    }
    let (visible, unlexed, stale, unvec): (i64, i64, i64, i64) = conn.query_row(
        "SELECT count(*),
                COALESCE(sum(l.claim_id IS NULL),0),
                COALESCE(sum(l.claim_id IS NOT NULL AND l.tokenizer<>?2),0),
                COALESCE(sum(?3 IS NOT NULL AND NOT EXISTS(SELECT 1 FROM claim_vec v WHERE v.claim_id=c.id AND v.model_key=?3)),0)
         FROM claim c JOIN claim_access a ON a.claim_id=c.id LEFT JOIN claim_lex l ON l.claim_id=c.id
         WHERE c.source_id=?1 AND (a.user_visible=1 OR a.agent_visible=1)",
        rusqlite::params![source, super::tokenizer::VERSION, model_key],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    if visible == 0 {
        return Ok(processing("no_visible_claims", None, 0));
    }
    let job = |kind: &str| -> Result<Option<(String, Option<String>, i64)>> {
        Ok(conn
            .prepare_cached("SELECT state,last_error,attempts FROM job WHERE kind=?1 AND target_id=?2 AND (?3 IS NULL OR detail=?3) ORDER BY created_at DESC,id DESC LIMIT 1")?
            .query_row(rusqlite::params![kind, source, if kind == "index_embed" { model_key } else { None }], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?)
    };
    let blocked = |j: Option<(String, Option<String>, i64)>, stage: &'static str| -> Processing {
        match j {
            Some((s, e, a)) if s == "failed" => processing("failed", e, a),
            Some((s, e, a)) if s == "deferred" => processing("deferred", e, a),
            Some((_, e, a)) => processing(stage, e, a),
            None => processing(stage, None, 0),
        }
    };
    if unlexed > 0 {
        return Ok(blocked(job("index_lexical")?, "published_awaiting_lexical"));
    }
    if stale > 0 {
        return Ok(processing("stale", None, 0));
    }
    if unvec > 0 {
        return Ok(blocked(job("index_embed")?, "awaiting_embedding"));
    }
    Ok(processing("fully_indexed", None, 0))
}

#[derive(Debug, Serialize, Default)]
pub struct Percentiles {
    pub count: usize,
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
    pub p99_ms: Option<i64>,
}
fn percentiles(mut v: Vec<i64>) -> Percentiles {
    v.sort_unstable();
    let at = |q: f64| {
        v.get(((v.len() as f64 * q).ceil() as usize).saturating_sub(1))
            .copied()
    };
    Percentiles {
        count: v.len(),
        p50_ms: at(0.50),
        p95_ms: at(0.95),
        p99_ms: at(0.99),
    }
}
#[derive(Debug, Serialize)]
pub struct KindStatus {
    pub pending: i64,
    pub leased: i64,
    pub deferred: i64,
    pub failed: i64,
    pub cancelled: i64,
    pub done: i64,
    pub oldest_pending_age_ms: Option<i64>,
    pub retries: i64,
    /// Time from enqueue to first lease (queue wait), recent completed jobs.
    pub wait: Percentiles,
    /// Time from last lease to completion (processing), recent completed jobs.
    pub processing: Percentiles,
    /// Time from enqueue to completion (end-to-end lag).
    pub end_to_end: Percentiles,
}
#[derive(Debug, Serialize)]
pub struct IndexProgress {
    pub visible_claims: i64,
    pub lexical_indexed: i64,
    pub embedded: i64,
    pub embedding_model: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct QueueStatus {
    pub health: &'static str,
    pub kinds: BTreeMap<String, KindStatus>,
    pub index: IndexProgress,
    pub sources_by_stage: BTreeMap<&'static str, i64>,
    pub open_jobs: i64,
    pub capacity: usize,
}
fn millis(a: &str, b: &str) -> Option<i64> {
    let a = a.parse::<Timestamp>().ok()?;
    let b = b.parse::<Timestamp>().ok()?;
    Some((b.instant() - a.instant()).whole_milliseconds() as i64)
}

impl App {
    /// Aggregate queue health for the owner. Counts cover jobs and Source totals only; no
    /// identifiers, text or hidden-record details are returned.
    pub fn queue_status(&self, p: &Principal) -> Result<QueueStatus> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let conn = self.store.reader()?;
        let now = Timestamp::now();
        let mut kinds = BTreeMap::new();
        for kind in ["extract", "revalidate", "index_lexical", "index_embed"] {
            let mut counts: BTreeMap<String, i64> = BTreeMap::new();
            for row in conn
                .prepare("SELECT state,count(*) FROM job WHERE kind=?1 GROUP BY state")?
                .query_map([kind], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })?
            {
                let (s, n) = row?;
                counts.insert(s, n);
            }
            let get = |s: &str| counts.get(s).copied().unwrap_or(0);
            let oldest: Option<String> = conn
                .prepare_cached("SELECT min(created_at) FROM job WHERE kind=?1 AND state IN ('pending','leased','deferred')")?
                .query_row([kind], |r| r.get(0))?;
            let retries: i64 = conn
                .prepare_cached("SELECT COALESCE(sum(MAX(attempts-1,0)),0) FROM job WHERE kind=?1")?
                .query_row([kind], |r| r.get(0))?;
            let (mut wait, mut proc_, mut e2e) = (vec![], vec![], vec![]);
            for row in conn
                .prepare_cached("SELECT created_at,first_leased_at,started_at,finished_at FROM job WHERE kind=?1 AND state='done' AND finished_at IS NOT NULL ORDER BY finished_at DESC LIMIT 5000")?
                .query_map([kind], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?)))?
            {
                let (created, first, started, finished) = row?;
                if let Some(f) = &first {
                    wait.extend(millis(&created, f));
                }
                if let Some(s) = &started {
                    proc_.extend(millis(s, &finished));
                }
                e2e.extend(millis(&created, &finished));
            }
            kinds.insert(
                kind.to_string(),
                KindStatus {
                    pending: get("pending"),
                    leased: get("leased"),
                    deferred: get("deferred"),
                    failed: get("failed"),
                    cancelled: get("cancelled"),
                    done: get("done"),
                    oldest_pending_age_ms: oldest.and_then(|o| millis(&o, now.as_str())),
                    retries,
                    wait: percentiles(wait),
                    processing: percentiles(proc_),
                    end_to_end: percentiles(e2e),
                },
            );
        }
        let key = self.policy.embedding.identity().map(|i| i.key);
        let (visible, lexed, embedded): (i64, i64, i64) = conn.query_row(
            "SELECT count(*),COALESCE(sum(EXISTS(SELECT 1 FROM claim_lex l WHERE l.claim_id=a.claim_id AND l.tokenizer=?1)),0),COALESCE(sum(?2 IS NOT NULL AND EXISTS(SELECT 1 FROM claim_vec v WHERE v.claim_id=a.claim_id AND v.model_key=?2)),0) FROM claim_access a WHERE a.user_visible=1",
            rusqlite::params![super::tokenizer::VERSION, key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut stages: BTreeMap<&'static str, i64> = BTreeMap::new();
        let sources: Vec<String> = conn
            .prepare("SELECT id FROM source ORDER BY id")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        if sources.len() <= 50_000 {
            for s in &sources {
                *stages
                    .entry(source_stage(&conn, key.as_deref(), s)?.stage)
                    .or_default() += 1;
            }
        }
        let open: i64 = kinds
            .values()
            .map(|k| k.pending + k.leased + k.deferred)
            .sum();
        let failed: i64 = kinds.values().map(|k| k.failed).sum();
        let deferred: i64 = kinds.values().map(|k| k.deferred).sum();
        let health = if open as usize >= self.policy.queue.max_open_jobs {
            "saturated"
        } else if failed > 0 {
            "degraded"
        } else if deferred > 0 {
            "deferred"
        } else {
            "ok"
        };
        Ok(QueueStatus {
            health,
            kinds,
            index: IndexProgress {
                visible_claims: visible,
                lexical_indexed: lexed,
                embedded,
                embedding_model: key,
            },
            sources_by_stage: stages,
            open_jobs: open,
            capacity: self.policy.queue.max_open_jobs,
        })
    }
    /// Jobs in a state, newest first; ids and safe codes only (no Source content).
    pub fn queue_jobs(
        &self,
        p: &Principal,
        state: Option<&str>,
        limit: usize,
    ) -> Result<Vec<JobView>> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let conn = self.store.reader()?;
        let rows = conn
            .prepare("SELECT id,kind,state,attempts,max_attempts,last_error,priority,created_at,updated_at,run_after FROM job WHERE (?1 IS NULL OR state=?1) ORDER BY created_at DESC,id DESC LIMIT ?2")?
            .query_map(rusqlite::params![state, limit.clamp(1, 500) as i64], |r| {
                Ok(JobView { id: r.get(0)?, kind: r.get(1)?, state: r.get(2)?, attempts: r.get(3)?, max_attempts: r.get(4)?, last_error: r.get(5)?, priority: r.get(6)?, created_at: r.get(7)?, updated_at: r.get(8)?, run_after: r.get(9)? })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }
    /// Processing stage of one Source the caller can see.
    pub fn source_processing(&self, p: &Principal, id: &SourceId) -> Result<Processing> {
        self.get_source(p, id.clone())?;
        let conn = self.store.reader()?;
        let key = self.policy.embedding.identity().map(|i| i.key);
        source_stage(&conn, key.as_deref(), id.as_str())
    }
}
#[derive(Debug, Serialize)]
pub struct JobView {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub attempts: i64,
    pub max_attempts: i64,
    pub last_error: Option<String>,
    pub priority: i64,
    pub created_at: String,
    pub updated_at: String,
    pub run_after: String,
}
