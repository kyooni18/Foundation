//! Durable revalidation on the existing job queue. A job deterministically rechecks the
//! materialized validity of one Claim; whether the Claim still holds under changed advisory,
//! session-read or identity context is a semantic question. No model is called here: such
//! cases are recorded as `semantic_pending` instead of being answered.
use super::validity::{self, Validity};
use crate::{App, domain::*, jobs::Lease};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

/// Enqueue at most one outstanding revalidation per Claim, in the caller's transaction.
pub(crate) fn schedule(conn: &rusqlite::Connection, claim: &str, cause: &str) -> Result<bool> {
    let outstanding: bool = conn
        .prepare_cached("SELECT EXISTS(SELECT 1 FROM job WHERE kind='revalidate' AND target_id=?1 AND state IN ('pending','leased'))")?
        .query_row([claim], |r| r.get(0))?;
    if outstanding {
        return Ok(false);
    }
    let id = ulid::Ulid::new().to_string();
    let now = Timestamp::now();
    conn.prepare_cached("INSERT INTO job(id,kind,target_id,idem_key,state,run_after,created_at,updated_at) VALUES(?1,'revalidate',?2,?3,'pending',?4,?4,?4)")?
        .execute(params![id, claim, format!("revalidate/{claim}/{id}"), now.as_str()])?;
    conn.prepare_cached(
        "INSERT INTO revalidation_request(job_id,claim_id,cause,created_at) VALUES(?1,?2,?3,?4)",
    )?
    .execute(params![id, claim, cause, now.as_str()])?;
    Ok(true)
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Revalidated {
    pub job_id: String,
    pub claim_id: String,
    pub outcome: String,
    pub validity: Validity,
}
impl App {
    pub fn lease_revalidation(
        &self,
        owner: &str,
        now: &Timestamp,
        until: &Timestamp,
    ) -> Result<Option<Lease>> {
        self.lease_kind("revalidate", owner, now, until)
    }
    /// Apply the deterministic recheck for a leased job. Idempotent: a job whose result is
    /// already recorded (duplicate delivery, retried commit) returns that result unchanged.
    pub fn complete_revalidation(&self, lease: &Lease) -> Result<Revalidated> {
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let prior: Option<(String, String, String)> = tx
            .query_row(
                "SELECT claim_id,outcome,validity FROM revalidation_result WHERE job_id=?1",
                [&lease.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((claim_id, outcome, v)) = prior {
            return Ok(Revalidated {
                job_id: lease.id.clone(),
                claim_id,
                outcome,
                validity: Validity::parse(&v)?,
            });
        }
        crate::jobs::fenced(&tx, lease)?;
        let (claim, cause): (String, String) = tx
            .query_row(
                "SELECT j.target_id,r.cause FROM job j JOIN revalidation_request r ON r.job_id=j.id WHERE j.id=?1 AND j.kind='revalidate'",
                [&lease.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let (state, repaired) = validity::recheck(&tx, &claim)?;
        let outcome = if repaired {
            "repaired"
        } else if state.validity == Validity::NeedsRevalidation || cause == "identity" {
            "semantic_pending"
        } else {
            "resolved"
        };
        let now = Timestamp::now();
        tx.execute(
            "INSERT INTO revalidation_result(job_id,claim_id,outcome,validity,reasons,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![lease.id, claim, outcome, state.validity.as_str(), state.reasons, now.as_str()],
        )?;
        tx.execute(
            "UPDATE job SET state='done',last_error=NULL,lease_owner=NULL,lease_until=NULL,updated_at=?2,finished_at=?2 WHERE id=?1",
            params![lease.id, now.as_str()],
        )?;
        tx.commit()?;
        Ok(Revalidated {
            job_id: lease.id.clone(),
            claim_id: claim,
            outcome: outcome.into(),
            validity: state.validity,
        })
    }
    /// One worker step with bounded exponential backoff on storage failure.
    pub fn revalidate_once(&self, owner: &str) -> Result<Option<Revalidated>> {
        use time::{Duration, OffsetDateTime};
        let now = OffsetDateTime::now_utc();
        let stamp = |t: OffsetDateTime| -> Result<Timestamp> {
            t.format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| invalid("time", "format"))?
                .parse()
        };
        let Some(lease) =
            self.lease_revalidation(owner, &stamp(now)?, &stamp(now + Duration::seconds(60))?)?
        else {
            return Ok(None);
        };
        match self.complete_revalidation(&lease) {
            Ok(done) => Ok(Some(done)),
            Err(e) => {
                let backoff = Duration::seconds(2i64.pow(lease.attempt.min(10)));
                self.fail_job(&lease, "storage", &stamp(now)?, &stamp(now + backoff)?)?;
                Err(e)
            }
        }
    }
}
