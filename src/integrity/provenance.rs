//! Source citations, agent session reads and source-based support (DATA_CONTRACTS §6, §6.1).
//! Support counts distinct underlying Sources inside the caller's projection. It is never a
//! probability, and citations never become validity dependencies.
use super::{claim_visible, lifecycle_sql, revalidation, validity};
use crate::{App, domain::*};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Support {
    /// Distinct origin Sources reached through live restatements and citations.
    pub sources: usize,
    /// Of those, origins whose independence is not established.
    pub independence_unknown: usize,
    /// Valid Claims expressing the proposition in this projection.
    pub claims: usize,
}
fn source_visible(conn: &Connection, p: &Principal, id: &str) -> Result<bool> {
    let predicate = match p.kind() {
        PrincipalKind::User => {
            format!(
                "recorder='user' AND {}='restore'",
                lifecycle_sql("source", "id")
            )
        }
        _ => format!("{}<>'forget'", lifecycle_sql("source", "id")),
    };
    Ok(conn
        .query_row(
            &format!("SELECT 1 FROM source WHERE id=?1 AND {predicate}"),
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}
/// Revalidation for Claims whose provenance changed; their validity is unaffected.
pub(crate) fn provenance_changed(conn: &Connection, source: &str) -> Result<()> {
    let claims: Vec<String> = conn
        .prepare("SELECT id FROM claim WHERE source_id=?1 ORDER BY id")?
        .query_map([source], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    for claim in claims {
        revalidation::schedule(conn, &claim, "provenance")?;
    }
    Ok(())
}
impl App {
    pub fn add_citation(
        &self,
        p: &Principal,
        child_source: &SourceId,
        parent_source: &SourceId,
    ) -> Result<String> {
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let child = crate::store::get(&tx, child_source)?;
        crate::store::get(&tx, parent_source)?;
        if p.kind() != PrincipalKind::User {
            // Visibility before authorization: hidden and absent Sources are both NotFound.
            if !source_visible(&tx, p, child_source.as_str())?
                || !source_visible(&tx, p, parent_source.as_str())?
            {
                return Err(Error::NotFound);
            }
            if child.recorder != p.kind() {
                return Err(Error::Forbidden);
            }
        }
        if child_source == parent_source {
            return Err(invalid("citation", "self citation"));
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM source_citation WHERE child_source=?1 AND parent_source=?2",
                params![child_source.as_str(), parent_source.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        let id = ulid::Ulid::new().to_string();
        tx.execute(
            "INSERT INTO source_citation(id,child_source,parent_source,actor,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![id, child_source.as_str(), parent_source.as_str(), p.kind().as_str(), Timestamp::now().as_str()],
        )
        .map_err(|e| match e.sqlite_error_code() {
            Some(rusqlite::ErrorCode::ConstraintViolation) => invalid("citation", "cycle"),
            _ => e.into(),
        })?;
        provenance_changed(&tx, child_source.as_str())?;
        tx.commit()?;
        Ok(id)
    }
    /// Record Claims actually returned to an agent session, in the same transaction as the
    /// validity changes they cause. Users and imports have no undeclared-dependency semantics.
    pub(crate) fn note_reads(&self, p: &Principal, ids: &[String]) -> Result<()> {
        let Some(session) = p.session_id.as_deref() else {
            return Ok(());
        };
        if p.kind() != PrincipalKind::Agent || ids.is_empty() {
            return Ok(());
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let now = Timestamp::now();
        let mut inserted = false;
        for id in ids {
            inserted |= tx
                .prepare_cached("INSERT OR IGNORE INTO session_read(session_id,claim_id,actor,at) VALUES(?1,?2,'agent',?3)")?
                .execute(params![session, id, now.as_str()])?
                == 1;
        }
        if inserted {
            let affected: Vec<String> = tx
                .prepare("SELECT c.id FROM source s JOIN claim c ON c.source_id=s.id WHERE s.session_id=?1 AND s.recorder='agent' AND s.captured_at>=?2")?
                .query_map(params![session, now.as_str()], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            validity::propagate(&tx, affected)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn claim_support(&self, p: &Principal, id: &str) -> Result<Support> {
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        if !claim_visible(&tx, p, id, false)? {
            return Err(Error::NotFound);
        }
        support(&tx, p, id)
    }
}
/// Origins of the proposition expressed by `id`, computed only from records visible to `p`.
pub(crate) fn support(conn: &Connection, p: &Principal, id: &str) -> Result<Support> {
    let agent = p.kind() != PrincipalKind::User;
    let restates = format!(
        "SELECT d.child,d.parent FROM claim_dependency d WHERE d.kind='restates' AND (d.child=?1 OR d.parent=?1) AND {}='restore'",
        lifecycle_sql("dependency", "d.id")
    );
    // Restatement component of `id` (restates means the same proposition, both directions).
    let mut members: BTreeSet<String> = BTreeSet::from([id.to_owned()]);
    let mut queue = VecDeque::from([id.to_owned()]);
    let mut restating: HashSet<String> = HashSet::new();
    while let Some(c) = queue.pop_front() {
        let edges: Vec<(String, String)> = conn
            .prepare_cached(&restates)?
            .query_map([&c], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for (child, parent) in edges {
            if !claim_visible(conn, p, &child, false)? || !claim_visible(conn, p, &parent, false)? {
                continue;
            }
            restating.insert(child.clone());
            for next in [child, parent] {
                if members.insert(next.clone()) {
                    queue.push_back(next);
                }
            }
        }
    }
    let mut roots: BTreeSet<String> = BTreeSet::new();
    let mut unknown: BTreeSet<String> = BTreeSet::new();
    let mut claims = 0;
    for m in &members {
        let (v, _, deps_unknown) = validity::of(conn, m)?;
        if v != validity::Validity::Valid {
            continue;
        }
        claims += 1;
        if restating.contains(m) {
            continue; // shares its parent's origin; never independent evidence
        }
        let source: String =
            conn.query_row("SELECT source_id FROM claim WHERE id=?1", [m], |r| r.get(0))?;
        let advisory: bool = conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM claim_dependency d JOIN claim c ON c.id=d.parent JOIN source s ON s.id=c.source_id WHERE d.child=?1 AND d.kind='informed_by' AND {}='restore' AND {})",
                lifecycle_sql("dependency", "d.id"),
                if agent { super::agent_claim_predicate(true) } else { super::user_claim_predicate() }
            ),
            [m],
            |r| r.get(0),
        )?;
        let (reached, opaque) = citation_roots(conn, p, &source)?;
        let uncertain = advisory || opaque || (agent && deps_unknown);
        for root in reached {
            if uncertain {
                unknown.insert(root.clone());
            }
            roots.insert(root);
        }
    }
    Ok(Support {
        sources: roots.len(),
        independence_unknown: unknown.len(),
        claims,
    })
}
/// Follow live citations to uncited origins. A citation to a record outside the projection is
/// not followed; for non-user views it marks the origin's independence unknown.
fn citation_roots(conn: &Connection, p: &Principal, source: &str) -> Result<(Vec<String>, bool)> {
    let sql = format!(
        "SELECT x.parent_source FROM source_citation x WHERE x.child_source=?1 AND {}='restore' ORDER BY x.parent_source",
        lifecycle_sql("citation", "x.id")
    );
    let mut roots = Vec::new();
    let mut opaque = false;
    let mut seen = HashSet::from([source.to_owned()]);
    let mut queue = VecDeque::from([source.to_owned()]);
    while let Some(s) = queue.pop_front() {
        let parents: Vec<String> = conn
            .prepare_cached(&sql)?
            .query_map([&s], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let mut followed = false;
        for parent in parents {
            if !source_visible(conn, p, &parent)? {
                opaque |= p.kind() != PrincipalKind::User;
                continue;
            }
            followed = true;
            if seen.insert(parent.clone()) {
                queue.push_back(parent);
            }
        }
        if !followed {
            roots.push(s);
        }
    }
    Ok((roots, opaque))
}
