//! Authorization projection for retrieval. `claim_access` records, for every Claim, whether the
//! user projection and the agent view may see it *now*. It is rewritten in the same writer
//! transaction as every lifecycle, validity, confirmation or publication change, so a forgotten
//! or retracted Claim leaves every retrieval path immediately, whether or not its derived index
//! rows have been removed. Queued index jobs never grant or remove visibility.
//!
//! The projection is derived and rebuildable. Retrieval additionally rechecks each returned
//! Claim against the authoritative tables, so a defect here cannot widen what a caller reads.
use crate::{
    domain::*,
    integrity::{lifecycle_sql, user_claim_predicate},
};
use rusqlite::{Connection, params};

/// Current expression of the Claim, both lifecycles live (agent view without retracted).
fn agent_predicate() -> String {
    format!(
        "c.job_id=(SELECT job_id FROM extraction_run WHERE source_id=c.source_id ORDER BY seq DESC LIMIT 1) AND {}='restore' AND {}='restore'",
        lifecycle_sql("source", "s.id"),
        lifecycle_sql("claim", "c.id")
    )
}
fn expected_sql() -> String {
    format!(
        "SELECT c.id,CASE WHEN {} THEN 1 ELSE 0 END,CASE WHEN {} THEN 1 ELSE 0 END FROM claim c JOIN source s ON s.id=c.source_id",
        user_claim_predicate(),
        agent_predicate()
    )
}
pub(crate) fn bump(conn: &Connection, name: &str) -> Result<()> {
    conn.prepare_cached("UPDATE index_epoch SET value=value+1 WHERE name=?1")?
        .execute([name])?;
    Ok(())
}
/// Recompute the projection for specific Claims inside the caller's writer transaction.
pub(crate) fn refresh<'a>(
    conn: &Connection,
    ids: impl IntoIterator<Item = &'a String>,
) -> Result<()> {
    let sql = format!(
        "INSERT INTO claim_access(claim_id,user_visible,agent_visible) {} WHERE c.id=?1 \
         ON CONFLICT(claim_id) DO UPDATE SET user_visible=excluded.user_visible,agent_visible=excluded.agent_visible \
         WHERE user_visible<>excluded.user_visible OR agent_visible<>excluded.agent_visible",
        expected_sql()
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut changed = 0;
    for id in ids {
        changed += stmt.execute([id])?;
    }
    if changed > 0 {
        bump(conn, "access")?;
    }
    Ok(())
}
/// A Source's expression changed (publication, revision) or its lifecycle did.
pub(crate) fn refresh_source(conn: &Connection, source: &str) -> Result<()> {
    let ids: Vec<String> = conn
        .prepare_cached("SELECT id FROM claim WHERE source_id=?1")?
        .query_map([source], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    refresh(conn, &ids)
}
/// Set-based recomputation of every row.
pub(crate) fn rebuild(conn: &Connection) -> Result<usize> {
    conn.execute("DELETE FROM claim_access", [])?;
    let n = conn.execute(
        &format!(
            "INSERT INTO claim_access(claim_id,user_visible,agent_visible) {}",
            expected_sql()
        ),
        [],
    )?;
    bump(conn, "access")?;
    Ok(n)
}
/// Claims whose stored projection differs from the authoritative predicates.
pub(crate) fn mismatches(conn: &Connection) -> Result<Vec<String>> {
    let sql = format!(
        "WITH expected(id,u,a) AS ({}) SELECT e.id FROM expected e LEFT JOIN claim_access x ON x.claim_id=e.id \
         WHERE x.claim_id IS NULL OR x.user_visible<>e.u OR x.agent_visible<>e.a \
         UNION ALL SELECT claim_id FROM claim_access WHERE claim_id NOT IN (SELECT id FROM claim)",
        expected_sql()
    );
    Ok(conn
        .prepare(&sql)?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?)
}
/// Which flag applies to the caller.
pub(crate) fn column(p: &Principal) -> &'static str {
    match p.kind() {
        PrincipalKind::User => "user_visible",
        _ => "agent_visible",
    }
}
