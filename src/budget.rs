//! Durable, fail-closed OpenRouter accounting. Amounts are rounded up to nano-USD.
//! This operational database is deliberately separate from authoritative memory.
use crate::providers::ProviderError;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_LIMIT: i64 = 5_999_999_990;
#[derive(Clone)]
pub struct Budget(PathBuf);
fn quota<T>(_: T) -> ProviderError {
    ProviderError::Quota
}
pub(crate) fn nanos(value: f64) -> Result<i64, ProviderError> {
    let n = (value * 1_000_000_000.0).ceil();
    if !n.is_finite() || !(0.0..=i64::MAX as f64 / 2.0).contains(&n) {
        return Err(ProviderError::Quota);
    }
    Ok(n as i64)
}
impl Budget {
    /// Explicit provisioning only. Never overwrites, resets, or increases a budget.
    pub fn create(
        path: &Path,
        limit_usd: f64,
        prior_spend_usd: f64,
    ) -> Result<Self, ProviderError> {
        let limit = nanos(limit_usd)?;
        let prior = nanos(prior_spend_usd)?;
        if limit == 0 || limit > MAX_LIMIT {
            return Err(ProviderError::Quota);
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(path)
            .map_err(quota)?
            .sync_all()
            .map_err(quota)?;
        let budget = Self(path.to_owned());
        let conn = budget.connection()?;
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE budget(id INTEGER PRIMARY KEY CHECK(id=1), ceiling INTEGER NOT NULL, prior INTEGER NOT NULL);
            CREATE TABLE request(id TEXT PRIMARY KEY, model TEXT NOT NULL, created_at TEXT NOT NULL,
              estimate INTEGER NOT NULL, actual INTEGER, provider_id TEXT, settled_at TEXT,
              cumulative INTEGER NOT NULL, prompt_tokens INTEGER, completion_tokens INTEGER);").map_err(quota)?;
        conn.execute("INSERT INTO budget VALUES(1,?1,?2)", params![limit, prior])
            .map_err(quota)?;
        conn.execute_batch("COMMIT").map_err(quota)?;
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)
                .map_err(quota)?
                .sync_all()
                .map_err(quota)?;
        }
        Ok(budget)
    }
    pub fn open(path: &Path) -> Result<Self, ProviderError> {
        let budget = Self(path.to_owned());
        budget.limit()?;
        Ok(budget)
    }
    fn connection(&self) -> Result<Connection, ProviderError> {
        let conn = Connection::open_with_flags(&self.0, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(quota)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(quota)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(quota)?;
        Ok(conn)
    }
    pub(crate) fn limit(&self) -> Result<i64, ProviderError> {
        let limit = self
            .connection()?
            .query_row("SELECT ceiling FROM budget WHERE id=1", [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(quota)?;
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(ProviderError::Quota);
        }
        Ok(limit)
    }
    pub(crate) fn reserve(&self, model: &str, estimate: i64) -> Result<String, ProviderError> {
        let mut conn = self.connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(quota)?;
        let (limit, prior): (i64, i64) = tx
            .query_row("SELECT ceiling,prior FROM budget WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(quota)?;
        let (spent, pending): (i64,i64) = tx.query_row("SELECT COALESCE(SUM(COALESCE(actual,estimate)),0),COUNT(*) FILTER(WHERE actual IS NULL) FROM request",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(quota)?;
        let overruns: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM request WHERE actual > estimate",
                [],
                |r| r.get(0),
            )
            .map_err(quota)?;
        let total = prior
            .checked_add(spent)
            .and_then(|n| n.checked_add(estimate))
            .ok_or(ProviderError::Quota)?;
        // At most four in flight across processes. Unknown charges retain both money and a slot.
        if overruns != 0
            || estimate < 0
            || prior < 0
            || pending >= 4
            || !(1..=MAX_LIMIT).contains(&limit)
            || total > limit
        {
            return Err(ProviderError::Quota);
        }
        let id = ulid::Ulid::new().to_string();
        tx.execute(
            "INSERT INTO request(id,model,created_at,estimate,cumulative) VALUES(?1,?2,?3,?4,?5)",
            params![
                id,
                model,
                crate::domain::Timestamp::now().as_str(),
                estimate,
                total
            ],
        )
        .map_err(quota)?;
        tx.commit().map_err(quota)?;
        Ok(id)
    }
    pub(crate) fn settle(&self, id: &str, body: &serde_json::Value) -> Result<(), ProviderError> {
        let provider_id = body["id"]
            .as_str()
            .filter(|s| s.len() <= 256 && !s.is_empty())
            .ok_or(ProviderError::Quota)?;
        let actual = nanos(body["usage"]["cost"].as_f64().ok_or(ProviderError::Quota)?)?;
        let prompt = body["usage"]["prompt_tokens"]
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or(ProviderError::Quota)?;
        let completion = body["usage"]["completion_tokens"]
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or(ProviderError::Quota)?;
        let mut conn = self.connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(quota)?;
        let changed = tx.execute("UPDATE request SET actual=?2,provider_id=?3,settled_at=?4,prompt_tokens=?5,completion_tokens=?6 WHERE id=?1 AND actual IS NULL",params![id,actual,provider_id,crate::domain::Timestamp::now().as_str(),prompt,completion]).map_err(quota)?;
        if changed != 1 {
            return Err(ProviderError::Quota);
        }
        tx.execute("UPDATE request SET cumulative=(SELECT prior FROM budget)+(SELECT SUM(COALESCE(actual,estimate)) FROM request) WHERE id=?1",[id]).map_err(quota)?;
        tx.commit().map_err(quota)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_restart_concurrency_and_uncertain_charges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite");
        let budget = Budget::create(&path, 4.5, 4.0).unwrap();
        assert!(Budget::create(&path, 4.5, 0.0).is_err());
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let b = budget.clone();
                std::thread::spawn(move || b.reserve("fixture", 500_000_000))
            })
            .collect();
        let ids: Vec<_> = threads
            .into_iter()
            .filter_map(|t| t.join().unwrap().ok())
            .collect();
        assert_eq!(ids.len(), 1);
        let restarted = Budget::open(&path).unwrap();
        assert!(restarted.reserve("fixture", 1).is_err());
        assert!(
            restarted
                .settle(&ids[0], &serde_json::json!({"id":"req","usage":{}}))
                .is_err()
        );
        restarted.settle(&ids[0], &serde_json::json!({"id":"req","usage":{"cost":0.05,"prompt_tokens":3,"completion_tokens":2}})).unwrap();
        assert!(restarted.reserve("fixture", 451_000_000).is_err());
        let second = restarted.reserve("fixture", 450_000_000).unwrap();
        // Record an unexpectedly high bill without erasing it; subsequent calls fail closed.
        restarted.settle(&second,&serde_json::json!({"id":"req2","usage":{"cost":0.6,"prompt_tokens":3,"completion_tokens":2}})).unwrap();
        assert!(restarted.reserve("fixture", 0).is_err());
        assert!(nanos(f64::NAN).is_err());
        assert!(nanos(-0.1).is_err());
    }
}
