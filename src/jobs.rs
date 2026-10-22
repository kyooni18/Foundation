//! Durable extraction leases. Attempt numbers fence crashed or expired workers.
use crate::{App, domain::*};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Lease {
    pub id: String,
    pub source_id: String,
    pub owner: String,
    pub attempt: u32,
}
pub(crate) fn enqueue(tx: &rusqlite::Transaction<'_>, source: &str, key: &str) -> Result<()> {
    tx.execute("INSERT INTO job(id,kind,target_id,idem_key,state,run_after,created_at,updated_at) VALUES(?1,'extract',?2,?3,'pending',?4,?4,?4) ON CONFLICT(idem_key) DO NOTHING",params![ulid::Ulid::new().to_string(),source,key,Timestamp::now().as_str()])?;
    Ok(())
}
impl App {
    pub fn backfill_extraction(&self, p: &Principal) -> Result<usize> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let ids: Vec<String> = tx
            .prepare("SELECT id FROM source")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let before = tx.total_changes();
        for id in ids {
            enqueue(&tx, &id, &format!("initial/{id}"))?;
        }
        let n = (tx.total_changes() - before) as usize;
        tx.commit()?;
        Ok(n)
    }
    pub fn request_extraction(&self, p: &Principal, id: SourceId, key: &str) -> Result<()> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        if key.is_empty() || key.len() > 200 {
            return Err(invalid("key", "invalid length"));
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let source = crate::store::get(&tx, &id)?;
        if source.lifecycle != Lifecycle::Live {
            return Err(Error::NotFound);
        }
        enqueue(&tx, id.as_str(), &format!("revision/{id}/{key}"))?;
        tx.commit()?;
        Ok(())
    }
    pub fn lease_extraction(
        &self,
        owner: &str,
        now: &Timestamp,
        until: &Timestamp,
    ) -> Result<Option<Lease>> {
        if owner.is_empty() || until.as_str() <= now.as_str() {
            return Err(invalid("lease", "invalid interval or owner"));
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        tx.execute("UPDATE job SET state='failed',last_error='exhausted',lease_owner=NULL,lease_until=NULL,updated_at=?1 WHERE state='leased' AND lease_until<=?1 AND attempts>=max_attempts",[now.as_str()])?;
        let id:Option<(String,String,u32)>=tx.query_row("SELECT id,target_id,attempts+1 FROM job WHERE attempts<max_attempts AND ((state='pending' AND run_after<=?1) OR (state='leased' AND lease_until<=?1)) ORDER BY created_at,id LIMIT 1",[now.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let lease = if let Some((id, source_id, attempt)) = id {
            tx.execute("UPDATE job SET state='leased',attempts=?2,lease_owner=?3,lease_until=?4,updated_at=?5 WHERE id=?1",params![id,attempt,owner,until.as_str(),now.as_str()])?;
            Some(Lease {
                id,
                source_id,
                owner: owner.into(),
                attempt,
            })
        } else {
            None
        };
        tx.commit()?;
        Ok(lease)
    }
    pub fn fail_extraction(
        &self,
        lease: &Lease,
        code: &str,
        now: &Timestamp,
        retry_at: &Timestamp,
    ) -> Result<bool> {
        // Diagnostics are fixed codes: never persist provider messages or source fragments.
        let allowed = [
            "provider",
            "quota",
            "request",
            "rate_limit",
            "timeout",
            "schema",
            "anchor",
            "language",
            "atomicity",
            "fidelity",
            "coverage",
            "unresolved",
            "stale",
            "lifecycle",
        ];
        if !allowed.contains(&code) || retry_at.as_str() < now.as_str() {
            return Err(invalid("failure", "invalid diagnostic or backoff"));
        }
        Ok(self.store.writer()?.execute("UPDATE job SET state=CASE WHEN attempts>=max_attempts THEN 'failed' ELSE 'pending' END,last_error=?4,run_after=?5,lease_owner=NULL,lease_until=NULL,updated_at=?6 WHERE id=?1 AND state='leased' AND lease_owner=?2 AND attempts=?3 AND lease_until>?6",params![lease.id,lease.owner,lease.attempt,code,retry_at.as_str(),now.as_str()])?==1)
    }
}
pub(crate) fn fenced(tx: &rusqlite::Transaction<'_>, lease: &Lease) -> Result<()> {
    let ok:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM job WHERE id=?1 AND target_id=?2 AND state='leased' AND lease_owner=?3 AND attempts=?4 AND lease_until>?5)",params![lease.id,lease.source_id,lease.owner,lease.attempt,Timestamp::now().as_str()],|r|r.get(0))?;
    if !ok {
        return Err(invalid("lease", "expired or replaced"));
    }
    Ok(())
}
