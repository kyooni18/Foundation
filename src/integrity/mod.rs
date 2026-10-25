//! Knowledge integrity: dependency validity, provenance, identity and temporal judgments.
//! Authoritative policy lives here; SQL persistence goes through the caller's transaction.
pub(crate) mod identity;
pub(crate) mod provenance;
pub(crate) mod revalidation;
pub mod temporal;
pub(crate) mod validity;
pub use identity::{EntityView, IdentityPolicy};
pub use provenance::Support;
pub use temporal::{FreshnessPolicy, TemporalItem, TemporalPage};
pub use validity::Validity;

use crate::domain::*;
use rusqlite::{Connection, OptionalExtension};

/// Lifecycle targets introduced by Phase 4 (Sources and Claims keep their own operations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Dependency,
    Citation,
    IdentityAssertion,
    Relation,
}
impl std::str::FromStr for Target {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "dependency" => Ok(Self::Dependency),
            "citation" => Ok(Self::Citation),
            "identity_assertion" => Ok(Self::IdentityAssertion),
            "relation" => Ok(Self::Relation),
            _ => Err(invalid("target", "unknown")),
        }
    }
}
impl Target {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Dependency => "dependency",
            Self::Citation => "citation",
            Self::IdentityAssertion => "identity_assertion",
            Self::Relation => "relation",
        }
    }
}

/// SQL expression for the current lifecycle event of a target; `id` is a trusted SQL expression.
pub(crate) fn lifecycle_sql(kind: &str, id: &str) -> String {
    format!(
        "COALESCE((SELECT event FROM status_event WHERE target_kind='{kind}' AND target_id={id} AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1),'restore')"
    )
}
pub(crate) fn lifecycle(conn: &Connection, kind: &str, id: &str) -> Result<Lifecycle> {
    let event: String = conn.query_row(
        &format!("SELECT {}", lifecycle_sql(kind, "?1")),
        [id],
        |r| r.get(0),
    )?;
    Ok(event.parse::<LifecycleEvent>()?.lifecycle())
}
pub(crate) fn confirmed_sql(id: &str) -> String {
    format!(
        "COALESCE((SELECT event FROM status_event WHERE target_kind='claim' AND target_id={id} AND event IN ('confirm','unconfirm') ORDER BY seq DESC LIMIT 1),'unconfirm')='confirm'"
    )
}
/// The user Claim projection (DATA_CONTRACTS §10) over aliases `c` (claim) and `s` (source):
/// current expression, both lifecycles live, validity not invalid, and user-verbatim or confirmed.
pub(crate) fn user_claim_predicate() -> String {
    format!(
        "c.job_id=(SELECT job_id FROM extraction_run WHERE source_id=c.source_id ORDER BY seq DESC LIMIT 1) AND {}='restore' AND {}='restore' AND (SELECT state FROM claim_validity WHERE claim_id=c.id)<>'invalid' AND ((s.recorder='user' AND c.speaker='user' AND c.basis='verbatim') OR {})",
        lifecycle_sql("source", "s.id"),
        lifecycle_sql("claim", "c.id"),
        confirmed_sql("c.id")
    )
}
/// Non-user views: everything not forgotten; retracted only when requested.
pub(crate) fn agent_claim_predicate(include_retracted: bool) -> String {
    let wanted = if include_retracted {
        "<>'forget'"
    } else {
        "='restore'"
    };
    format!(
        "{}{wanted} AND {}{wanted}",
        lifecycle_sql("source", "s.id"),
        lifecycle_sql("claim", "c.id")
    )
}
/// Visibility of one Claim in the authenticated projection. Unknown and hidden are identical.
pub(crate) fn claim_visible(
    conn: &Connection,
    p: &Principal,
    id: &str,
    include_retracted: bool,
) -> Result<bool> {
    let predicate = match p.kind() {
        PrincipalKind::User => user_claim_predicate(),
        _ => agent_claim_predicate(include_retracted),
    };
    Ok(conn
        .query_row(
            &format!(
                "SELECT 1 FROM claim c JOIN source s ON s.id=c.source_id WHERE c.id=?1 AND {predicate}"
            ),
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}
/// Claims a principal may reference when asserting a dependency or relation. The user may
/// administer any existing Claim (D-30); other roles may not reference forgotten records.
pub(crate) fn claim_referable(conn: &Connection, p: &Principal, id: &str) -> Result<bool> {
    if p.kind() == PrincipalKind::User {
        return Ok(conn
            .query_row("SELECT 1 FROM claim WHERE id=?1", [id], |_| Ok(()))
            .optional()?
            .is_some());
    }
    claim_visible(conn, p, id, true)
}
pub(crate) fn claim_recorder(conn: &Connection, id: &str) -> Result<PrincipalKind> {
    let recorder: String = conn
        .query_row(
            "SELECT s.recorder FROM claim c JOIN source s ON s.id=c.source_id WHERE c.id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    recorder.parse()
}
pub(crate) fn reason(reason: Option<String>) -> Result<Option<String>> {
    if reason.as_ref().is_some_and(|s| s.len() > 4096) {
        return Err(invalid("reason", "too long"));
    }
    Ok(reason)
}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub struct DependencyView {
    pub id: String,
    pub child: String,
    pub parent: String,
    pub kind: String,
    pub lifecycle: Lifecycle,
    /// Agent/audit view only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_by: Option<String>,
}
impl crate::App {
    /// Declare a dependency after both Claims exist (D-09). Cycles, including through
    /// retracted edges, are rejected; the child's closure is updated in the same transaction.
    pub fn add_dependency(
        &self,
        p: &Principal,
        child: &str,
        parent: &str,
        kind: &str,
    ) -> Result<String> {
        if !["premise", "restates", "alternative", "informed_by"].contains(&kind) {
            return Err(invalid("kind", "unknown dependency kind"));
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        if !claim_referable(&tx, p, child)? || !claim_referable(&tx, p, parent)? {
            return Err(Error::NotFound);
        }
        if p.kind() != PrincipalKind::User && claim_recorder(&tx, child)? != p.kind() {
            return Err(Error::Forbidden);
        }
        if child == parent {
            return Err(invalid("dependency", "self dependency"));
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM claim_dependency WHERE child=?1 AND parent=?2 AND kind=?3",
                rusqlite::params![child, parent, kind],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        let id = ulid::Ulid::new().to_string();
        tx.execute(
            "INSERT INTO claim_dependency(id,child,parent,kind,declared_by,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![id, child, parent, kind, p.kind().as_str(), Timestamp::now().as_str()],
        )
        .map_err(|e| match e.sqlite_error_code() {
            Some(rusqlite::ErrorCode::ConstraintViolation) => invalid("dependency", "cycle"),
            _ => e.into(),
        })?;
        validity::propagate(&tx, [child.to_owned()])?;
        tx.commit()?;
        Ok(id)
    }
    /// Dependencies of a visible Claim whose other end is also inside the projection.
    pub fn claim_dependencies(
        &self,
        p: &Principal,
        id: &str,
        include_retracted: bool,
    ) -> Result<Vec<DependencyView>> {
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        let include_retracted = include_retracted && p.kind() != PrincipalKind::User;
        if !claim_visible(&tx, p, id, include_retracted)? {
            return Err(Error::NotFound);
        }
        let rows: Vec<(String, String, String, String, String, String)> = tx
            .prepare(&format!(
                "SELECT id,child,parent,kind,declared_by,{} FROM claim_dependency d WHERE child=?1 OR parent=?1 ORDER BY id",
                lifecycle_sql("dependency", "d.id")
            ))?
            .query_map([id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        let mut out = Vec::new();
        for (dep, child, parent, kind, declared_by, event) in rows {
            let lifecycle = event.parse::<LifecycleEvent>()?.lifecycle();
            let shown = match lifecycle {
                Lifecycle::Live => true,
                Lifecycle::Retracted => include_retracted,
                Lifecycle::Forgotten => false,
            };
            let other = if child == id { &parent } else { &child };
            if !shown || !claim_visible(&tx, p, other, include_retracted)? {
                continue;
            }
            out.push(DependencyView {
                id: dep,
                child,
                parent,
                kind,
                lifecycle,
                declared_by: (p.kind() != PrincipalKind::User).then_some(declared_by),
            });
        }
        Ok(out)
    }
    /// Lifecycle of dependencies, citations, identity assertions and relations. Same rules as
    /// Sources/Claims: users administer any target (D-30); other roles see before they act.
    pub fn set_integrity_lifecycle(
        &self,
        p: &Principal,
        target: Target,
        id: &str,
        event: LifecycleEvent,
        reason: Option<String>,
    ) -> Result<()> {
        let reason = self::reason(reason)?;
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let owner: String = tx
            .query_row(
                "SELECT recorder FROM status_owner WHERE target_kind=?1 AND target_id=?2",
                [target.as_str(), id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let current = lifecycle(&tx, target.as_str(), id)?;
        if p.kind() != PrincipalKind::User {
            if current == Lifecycle::Forgotten || !target_visible(&tx, target, id)? {
                return Err(Error::NotFound);
            }
            if p.kind() != PrincipalKind::Agent || owner != "agent" {
                return Err(Error::Forbidden);
            }
        }
        if current == event.lifecycle() {
            return Ok(());
        }
        tx.execute(
            "INSERT INTO status_event(target_kind,target_id,event,actor,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![target.as_str(), id, event.as_str(), p.kind().as_str(), reason, Timestamp::now().as_str()],
        )?;
        match target {
            Target::Dependency => {
                let child: String = tx.query_row(
                    "SELECT child FROM claim_dependency WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )?;
                validity::propagate(&tx, [child])?;
            }
            Target::Citation => {
                let child: String = tx.query_row(
                    "SELECT child_source FROM source_citation WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )?;
                provenance::provenance_changed(&tx, &child)?;
            }
            Target::IdentityAssertion => identity::changed(&tx, self.policy.identity)?,
            Target::Relation => (),
        }
        tx.commit()?;
        Ok(())
    }
    /// Recompute every derived integrity table from authoritative rows.
    pub fn rebuild_derived(&self, p: &Principal) -> Result<usize> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let n = validity::rebuild(&tx)?;
        identity::rebuild(&tx, self.policy.identity)?;
        tx.commit()?;
        Ok(n)
    }
}
/// Non-user visibility of a Phase 4 target: every referenced record must be non-forgotten.
fn target_visible(conn: &Connection, target: Target, id: &str) -> Result<bool> {
    let not_forgotten = |kind: &str, expr: &str| format!("{}<>'forget'", lifecycle_sql(kind, expr));
    let claim = |expr: &str| {
        format!(
            "EXISTS(SELECT 1 FROM claim c JOIN source s ON s.id=c.source_id WHERE c.id={expr} AND {} AND {})",
            not_forgotten("claim", "c.id"),
            not_forgotten("source", "s.id")
        )
    };
    let sql = match target {
        Target::Dependency => format!(
            "SELECT {} AND {} FROM claim_dependency t WHERE id=?1",
            claim("t.child"),
            claim("t.parent")
        ),
        Target::Citation => format!(
            "SELECT {} AND {} FROM source_citation t WHERE id=?1",
            not_forgotten("source", "t.child_source"),
            not_forgotten("source", "t.parent_source")
        ),
        Target::IdentityAssertion => format!(
            "SELECT kind<>'refers_to' OR {} FROM identity_assertion t WHERE id=?1",
            claim("(SELECT claim_id FROM mention WHERE id=t.subject)")
        ),
        Target::Relation => format!(
            "SELECT {} AND {} FROM relation t WHERE id=?1",
            claim("t.subject"),
            claim("t.object")
        ),
    };
    Ok(conn
        .query_row(&sql, [id], |r| r.get::<_, bool>(0))
        .optional()?
        .unwrap_or(false))
}

/// Derived-table checks reported with the structural integrity report.
pub(crate) fn derived_report(
    conn: &Connection,
    policy: IdentityPolicy,
) -> Result<Vec<crate::store::IntegrityItem>> {
    let mut validity_item = crate::store::IntegrityItem::new("claim_validity");
    for id in validity::mismatches(conn)? {
        validity_item.fail(id);
    }
    let mut identity_item = crate::store::IntegrityItem::new("identity_resolution");
    if !identity::consistent(conn, policy)? {
        identity_item.fail("derived identity differs from authoritative assertions");
    }
    Ok(vec![validity_item, identity_item])
}
impl crate::App {
    /// Read-only diagnostic: Claims whose materialized validity differs from the oracle.
    pub fn validity_mismatches(&self) -> Result<Vec<String>> {
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        validity::mismatches(&tx)
    }
    /// Read-only diagnostic: time the independent full recomputation (no writes).
    pub fn validity_oracle_size(&self) -> Result<usize> {
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        Ok(validity::oracle(&tx)?.len())
    }
}
/// Diagnostic over a raw connection (e.g. a database inspected after a crash, before an App
/// repairs derived tables): Claims whose materialized validity differs from the oracle.
pub fn validity_mismatches_on(conn: &Connection) -> Result<Vec<String>> {
    validity::mismatches(conn)
}
