//! Entity identity (DATA_CONTRACTS §9, D-17/D-27). Mentions are immutable, entities are thin
//! IDs, assertions are append-only with lifecycle events, and resolution is derived by a
//! deterministic union-find with precedence user > agent > model. Merges are never
//! destructive: retracting an assertion and rebuilding reconstructs the earlier resolution.
use super::{claim_referable, lifecycle_sql, revalidation, user_claim_predicate};
use crate::{App, domain::*};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Whether model suggestions participate in resolution. Both default to off: model `same_as`
/// and `refers_to` remain stored suggestions (D-27).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdentityPolicy {
    pub model_same_as: bool,
    pub model_refers_to: bool,
}
#[derive(Debug, Clone)]
struct Assertion {
    id: String,
    kind: String,
    subject: String,
    object: String,
    actor: String,
}
const TIERS: [&str; 3] = ["user", "agent", "model"];
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Resolution {
    pub representative: BTreeMap<String, String>,
    pub mentions: BTreeMap<String, (String, String)>,
    pub blocked: BTreeMap<String, String>,
}
fn live_assertions(conn: &Connection) -> Result<Vec<Assertion>> {
    // Deterministic order: precedence handling iterates in creation order, then ID.
    Ok(conn
        .prepare(&format!(
            "SELECT id,kind,subject,object,actor FROM identity_assertion a WHERE {}='restore' ORDER BY created_at,id",
            lifecycle_sql("identity_assertion", "a.id")
        ))?
        .query_map([], |r| {
            Ok(Assertion {
                id: r.get(0)?,
                kind: r.get(1)?,
                subject: r.get(2)?,
                object: r.get(3)?,
                actor: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?)
}
struct Find {
    parent: HashMap<String, String>,
}
impl Find {
    fn root(&mut self, x: &str) -> String {
        let mut r = x.to_owned();
        while let Some(p) = self.parent.get(&r).filter(|p| **p != r) {
            r = p.clone();
        }
        let mut c = x.to_owned();
        while c != r {
            let next = self.parent.insert(c, r.clone()).expect("member");
            c = next;
        }
        r
    }
}
/// Pure resolution over `entities` using live assertions of the permitted actors.
fn resolve(
    entities: &[String],
    assertions: &[Assertion],
    actors: &[&str],
    policy: IdentityPolicy,
) -> Resolution {
    let mut uf = Find {
        parent: entities.iter().map(|e| (e.clone(), e.clone())).collect(),
    };
    let mut out = Resolution::default();
    let mut constraints: Vec<(String, String, String)> = Vec::new();
    for tier in TIERS.iter().filter(|t| actors.contains(t)) {
        let of = |kind: &'static str| -> Vec<&Assertion> {
            assertions
                .iter()
                .filter(|a| a.actor == *tier && a.kind == kind)
                .collect()
        };
        // Within a tier, distinct_from is installed before any same_as of that tier.
        for a in of("distinct_from") {
            if uf.root(&a.subject) == uf.root(&a.object) {
                out.blocked.insert(a.id.clone(), "precedence".into());
            } else {
                constraints.push((a.subject.clone(), a.object.clone(), a.id.clone()));
            }
        }
        if *tier == "model" && !policy.model_same_as {
            continue;
        }
        for a in of("same_as") {
            let (ra, rb) = (uf.root(&a.subject), uf.root(&a.object));
            if ra == rb {
                continue;
            }
            // Checking component roots also rejects merges implied only transitively.
            let conflict = constraints.iter().find(|(x, y, _)| {
                let (rx, ry) = (uf.root(x), uf.root(y));
                (rx == ra && ry == rb) || (rx == rb && ry == ra)
            });
            if let Some((_, _, by)) = conflict {
                out.blocked.insert(a.id.clone(), by.clone());
                continue;
            }
            let (keep, other) = if ra < rb { (ra, rb) } else { (rb, ra) };
            uf.parent.insert(other, keep);
        }
    }
    for e in entities {
        let r = uf.root(e);
        out.representative.insert(e.clone(), r);
    }
    let mut chosen: BTreeMap<String, (usize, String, String)> = BTreeMap::new();
    for a in assertions.iter().filter(|a| a.kind == "refers_to") {
        let Some(rank) = TIERS.iter().position(|t| *t == a.actor) else {
            continue;
        };
        if !actors.contains(&TIERS[rank]) || (a.actor == "model" && !policy.model_refers_to) {
            continue;
        }
        // Highest precedence wins; within a tier the latest assertion (iteration order).
        let replace = chosen.get(&a.subject).is_none_or(|(r, _, _)| rank <= *r);
        if replace {
            chosen.insert(a.subject.clone(), (rank, a.object.clone(), a.id.clone()));
        }
    }
    for (mention, (_, entity, assertion)) in chosen {
        out.mentions.insert(mention, (entity, assertion));
    }
    out
}
fn entities(conn: &Connection) -> Result<Vec<String>> {
    Ok(conn
        .prepare("SELECT id FROM entity ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?)
}
fn agent_actors(policy: IdentityPolicy) -> Vec<&'static str> {
    if policy.model_same_as || policy.model_refers_to {
        vec!["user", "agent", "model"]
    } else {
        vec!["user", "agent"]
    }
}
pub(crate) fn compute(conn: &Connection, policy: IdentityPolicy) -> Result<Resolution> {
    Ok(resolve(
        &entities(conn)?,
        &live_assertions(conn)?,
        &agent_actors(policy),
        policy,
    ))
}
fn materialized(conn: &Connection) -> Result<Resolution> {
    let mut out = Resolution::default();
    for row in conn
        .prepare("SELECT entity_id,representative FROM entity_resolution")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
    {
        let (e, r) = row?;
        out.representative.insert(e, r);
    }
    for row in conn
        .prepare("SELECT mention_id,entity_id,assertion_id FROM mention_resolution")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
    {
        let (m, e, a) = row?;
        out.mentions.insert(m, (e, a));
    }
    for row in conn
        .prepare("SELECT assertion_id,blocked_by FROM identity_blocked")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
    {
        let (a, b) = row?;
        out.blocked.insert(a, b);
    }
    Ok(out)
}
/// Rebuild every derived identity table from authoritative rows. Returns Claims whose
/// mentions now resolve to a different entity component.
pub(crate) fn rebuild(conn: &Connection, policy: IdentityPolicy) -> Result<BTreeSet<String>> {
    let before = mention_components(conn)?;
    let fresh = compute(conn, policy)?;
    conn.execute_batch(
        "DELETE FROM entity_alias; DELETE FROM mention_resolution; DELETE FROM entity_resolution; DELETE FROM identity_blocked;",
    )?;
    for (e, r) in &fresh.representative {
        conn.prepare_cached(
            "INSERT INTO entity_resolution(entity_id,representative) VALUES(?1,?2)",
        )?
        .execute(params![e, r])?;
    }
    for (m, (e, a)) in &fresh.mentions {
        conn.prepare_cached(
            "INSERT INTO mention_resolution(mention_id,entity_id,assertion_id) VALUES(?1,?2,?3)",
        )?
        .execute(params![m, e, a])?;
    }
    for (a, by) in &fresh.blocked {
        conn.prepare_cached("INSERT INTO identity_blocked(assertion_id,blocked_by) VALUES(?1,?2)")?
            .execute(params![a, by])?;
    }
    // Alias index: user/agent alias assertions and surfaces of resolved mentions. Raw text only.
    conn.execute(
        &format!(
            "INSERT INTO entity_alias(representative,alias,origin,origin_id) SELECT r.representative,a.object,'assertion',a.id FROM identity_assertion a JOIN entity_resolution r ON r.entity_id=a.subject WHERE a.kind='alias' AND a.actor IN ('user','agent') AND {}='restore'",
            lifecycle_sql("identity_assertion", "a.id")
        ),
        [],
    )?;
    conn.execute("INSERT INTO entity_alias(representative,alias,origin,origin_id) SELECT r.representative,m.surface,'mention',m.id FROM mention_resolution mr JOIN mention m ON m.id=mr.mention_id JOIN entity_resolution r ON r.entity_id=mr.entity_id", [])?;
    let after = mention_components(conn)?;
    let mut changed = BTreeSet::new();
    for mention in before.keys().chain(after.keys()) {
        if before.get(mention) != after.get(mention) {
            let (claim, _) = before.get(mention).or(after.get(mention)).expect("present");
            changed.insert(claim.clone());
        }
    }
    Ok(changed)
}
/// mention -> (claim, sorted member set of its resolved component)
fn mention_components(conn: &Connection) -> Result<BTreeMap<String, (String, String)>> {
    Ok(conn
        .prepare("SELECT mr.mention_id,m.claim_id,(SELECT group_concat(x.entity_id,',' ORDER BY x.entity_id) FROM entity_resolution x WHERE x.representative=r.representative) FROM mention_resolution mr JOIN mention m ON m.id=mr.mention_id JOIN entity_resolution r ON r.entity_id=mr.entity_id")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, String>(1)?, r.get::<_, String>(2)?))))?
        .collect::<std::result::Result<_, _>>()?)
}
/// Rebuild, then schedule revalidation for Claims whose entity context changed.
pub(crate) fn changed(conn: &Connection, policy: IdentityPolicy) -> Result<()> {
    for claim in rebuild(conn, policy)? {
        let live: bool = conn.query_row(
            &format!(
                "SELECT {}<>'forget' FROM claim WHERE id=?1",
                lifecycle_sql("claim", "id")
            ),
            [&claim],
            |r| r.get(0),
        )?;
        if live {
            revalidation::schedule(conn, &claim, "identity")?;
        }
    }
    Ok(())
}
pub(crate) fn consistent(conn: &Connection, policy: IdentityPolicy) -> Result<bool> {
    Ok(materialized(conn)? == compute(conn, policy)?)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct AliasView {
    pub text: String,
    pub origin: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MentionView {
    pub mention_id: String,
    pub claim_id: String,
    pub surface: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct EntityView {
    pub id: String,
    pub kind: String,
    pub representative: String,
    pub members: Vec<String>,
    pub aliases: Vec<AliasView>,
    pub mentions: Vec<MentionView>,
    /// Agent view only: stored model suggestions not applied by the current policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestions: Option<Vec<String>>,
    /// Agent view only: assertions preserved but blocked by a precedence or distinct_from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<Vec<String>>,
}
fn mention_claim(conn: &Connection, mention: &str) -> Result<String> {
    conn.query_row("SELECT claim_id FROM mention WHERE id=?1", [mention], |r| {
        r.get(0)
    })
    .optional()?
    .ok_or(Error::NotFound)
}
fn entity_exists(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM entity WHERE id=?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}
impl App {
    pub fn create_entity(&self, p: &Principal, kind: &str) -> Result<String> {
        if p.kind() == PrincipalKind::Import {
            return Err(Error::Forbidden);
        }
        if kind.trim().is_empty() || kind.len() > 64 {
            return Err(invalid("kind", "must be 1..=64 bytes"));
        }
        let id = ulid::Ulid::new().to_string();
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        tx.execute(
            "INSERT INTO entity(id,kind,created_by,created_at) VALUES(?1,?2,?3,?4)",
            params![id, kind, p.kind().as_str(), Timestamp::now().as_str()],
        )?;
        super::identity::changed(&tx, self.policy.identity)?;
        tx.commit()?;
        Ok(id)
    }
    /// Authenticated user/agent identity assertion. Symmetric kinds are stored canonically.
    pub fn assert_identity(
        &self,
        p: &Principal,
        kind: &str,
        subject: &str,
        object: &str,
        reason: Option<String>,
    ) -> Result<String> {
        if p.kind() == PrincipalKind::Import {
            return Err(Error::Forbidden);
        }
        let reason = super::reason(reason)?;
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let (subject, object) = check_target(&tx, Some(p), kind, subject, object)?;
        let id = insert_assertion(
            &tx,
            kind,
            &subject,
            &object,
            p.kind().as_str(),
            reason,
            None,
        )?;
        super::identity::changed(&tx, self.policy.identity)?;
        tx.commit()?;
        Ok(id)
    }
    /// Trusted in-process worker API for extractor identity proposals. Suggestions are bound
    /// to the published extraction job of the mentioned Claims and never become user or agent
    /// assertions. `object` may be `new:<kind>` for a model-created thin entity.
    pub fn suggest_identity(
        &self,
        job_id: &str,
        kind: &str,
        subject: &str,
        object: &str,
    ) -> Result<String> {
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let published: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM extraction_run WHERE job_id=?1)",
            [job_id],
            |r| r.get(0),
        )?;
        if !published {
            return Err(Error::NotFound);
        }
        let object = match object.strip_prefix("new:") {
            Some(entity_kind) if kind == "refers_to" => {
                if entity_kind.trim().is_empty() || entity_kind.len() > 64 {
                    return Err(invalid("kind", "must be 1..=64 bytes"));
                }
                let id = ulid::Ulid::new().to_string();
                tx.execute(
                    "INSERT INTO entity(id,kind,created_by,created_at) VALUES(?1,?2,'model',?3)",
                    params![id, entity_kind, Timestamp::now().as_str()],
                )?;
                id
            }
            _ => object.to_owned(),
        };
        if kind == "refers_to" {
            let in_job: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM mention m JOIN claim c ON c.id=m.claim_id WHERE m.id=?1 AND c.job_id=?2)",
                params![subject, job_id],
                |r| r.get(0),
            )?;
            if !in_job {
                return Err(Error::NotFound);
            }
        }
        let (subject, object) = check_target(&tx, None, kind, subject, &object)?;
        let id = insert_assertion(&tx, kind, &subject, &object, "model", None, Some(job_id))?;
        super::identity::changed(&tx, self.policy.identity)?;
        tx.commit()?;
        Ok(id)
    }
    pub fn get_entity(&self, p: &Principal, id: &str) -> Result<EntityView> {
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        let kind: String = tx
            .query_row("SELECT kind FROM entity WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .ok_or(Error::NotFound)?;
        match p.kind() {
            PrincipalKind::User => user_entity(&tx, id, kind),
            _ => agent_entity(&tx, id, kind, self.policy.identity),
        }
    }
}
/// `None` is the trusted in-process worker, whose mentions were already bound to its job.
fn check_target(
    conn: &Connection,
    p: Option<&Principal>,
    kind: &str,
    subject: &str,
    object: &str,
) -> Result<(String, String)> {
    match kind {
        "refers_to" => {
            let claim = mention_claim(conn, subject)?;
            let referable = match p {
                Some(p) => claim_referable(conn, p, &claim)?,
                None => true,
            };
            if !referable || !entity_exists(conn, object)? {
                return Err(Error::NotFound);
            }
            Ok((subject.into(), object.into()))
        }
        "same_as" | "distinct_from" => {
            if !entity_exists(conn, subject)? || !entity_exists(conn, object)? {
                return Err(Error::NotFound);
            }
            if subject == object {
                return Err(invalid("identity", "an entity cannot be related to itself"));
            }
            Ok(if subject < object {
                (subject.into(), object.into())
            } else {
                (object.into(), subject.into())
            })
        }
        "alias" => {
            if !entity_exists(conn, subject)? {
                return Err(Error::NotFound);
            }
            if object.is_empty() || object.len() > 1024 || object.trim().is_empty() {
                return Err(invalid("alias", "must be 1..=1024 bytes"));
            }
            Ok((subject.into(), object.into()))
        }
        _ => Err(invalid("kind", "unknown identity assertion")),
    }
}
fn insert_assertion(
    conn: &Connection,
    kind: &str,
    subject: &str,
    object: &str,
    actor: &str,
    reason: Option<String>,
    job: Option<&str>,
) -> Result<String> {
    // Idempotent per (kind, subject, object, actor, job); lifecycle is changed separately.
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM identity_assertion WHERE kind=?1 AND subject=?2 AND object=?3 AND actor=?4 AND job_id IS ?5 ORDER BY id LIMIT 1",
            params![kind, subject, object, actor, job],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = ulid::Ulid::new().to_string();
    conn.execute(
        "INSERT INTO identity_assertion(id,kind,subject,object,actor,reason,job_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![id, kind, subject, object, actor, reason, job, Timestamp::now().as_str()],
    )?;
    Ok(id)
}
/// On approved re-extraction, the system retracts earlier model suggestions of that Source.
/// User and agent assertions are untouched (the DB also forbids system changes to user rows).
pub(crate) fn retract_revised_suggestions(
    conn: &Connection,
    source: &str,
    job: &str,
    policy: IdentityPolicy,
) -> Result<()> {
    let stale: Vec<String> = conn
        .prepare(&format!(
            "SELECT a.id FROM identity_assertion a JOIN job j ON j.id=a.job_id WHERE a.actor='model' AND j.target_id=?1 AND a.job_id<>?2 AND {}='restore' ORDER BY a.id",
            lifecycle_sql("identity_assertion", "a.id")
        ))?
        .query_map(params![source, job], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    for id in &stale {
        conn.execute("INSERT INTO status_event(target_kind,target_id,event,actor,reason,created_at) VALUES('identity_assertion',?1,'retract','system','revision',?2)",params![id,Timestamp::now().as_str()])?;
    }
    if !stale.is_empty() {
        changed(conn, policy)?;
    }
    Ok(())
}
fn component(res: &Resolution, id: &str) -> (String, Vec<String>) {
    let rep = res
        .representative
        .get(id)
        .cloned()
        .unwrap_or_else(|| id.into());
    let members = res
        .representative
        .iter()
        .filter(|(_, r)| **r == rep)
        .map(|(e, _)| e.clone())
        .collect();
    (rep, members)
}
fn agent_entity(
    conn: &Connection,
    id: &str,
    kind: String,
    policy: IdentityPolicy,
) -> Result<EntityView> {
    let res = materialized(conn)?;
    let (representative, members) = component(&res, id);
    let visible_claim = super::agent_claim_predicate(false);
    let mut aliases = Vec::new();
    let mut mentions = Vec::new();
    for row in conn
        .prepare(&format!(
            "SELECT a.alias,a.origin,m.id,m.claim_id FROM entity_alias a LEFT JOIN mention m ON a.origin='mention' AND m.id=a.origin_id WHERE a.representative=?1 AND (a.origin='assertion' OR EXISTS(SELECT 1 FROM claim c JOIN source s ON s.id=c.source_id WHERE c.id=m.claim_id AND {visible_claim})) ORDER BY a.origin,a.alias,a.origin_id"
        ))?
        .query_map([&representative], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
    {
        let (text, origin, mention, claim) = row?;
        if let (Some(mention_id), Some(claim_id)) = (mention, claim) {
            mentions.push(MentionView {
                mention_id,
                claim_id,
                surface: text.clone(),
            });
        }
        aliases.push(AliasView { text, origin });
    }
    let placeholders = members.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let suggestion_filter = match (policy.model_same_as, policy.model_refers_to) {
        (true, true) => "kind='alias'",
        (true, false) => "kind IN ('alias','refers_to')",
        (false, true) => "kind IN ('alias','same_as')",
        (false, false) => "1",
    };
    let suggestions: Vec<String> = conn
        .prepare(&format!(
            "SELECT id FROM identity_assertion a WHERE actor='model' AND {} AND (subject IN ({placeholders}) OR object IN ({placeholders})) AND {}='restore' ORDER BY id",
            suggestion_filter,
            lifecycle_sql("identity_assertion", "a.id")
        ))?
        .query_map(
            rusqlite::params_from_iter(members.iter().chain(members.iter())),
            |r| r.get(0),
        )?
        .collect::<std::result::Result<_, _>>()?;
    let blocked: Vec<String> = conn
        .prepare(&format!(
            "SELECT b.assertion_id FROM identity_blocked b JOIN identity_assertion a ON a.id=b.assertion_id WHERE a.subject IN ({placeholders}) OR a.object IN ({placeholders}) ORDER BY b.assertion_id"
        ))?
        .query_map(
            rusqlite::params_from_iter(members.iter().chain(members.iter())),
            |r| r.get(0),
        )?
        .collect::<std::result::Result<_, _>>()?;
    Ok(EntityView {
        id: id.into(),
        kind,
        representative,
        members,
        aliases,
        mentions,
        suggestions: Some(suggestions),
        blocked: Some(blocked),
    })
}
/// User projection: resolution, names and mentions computed from user assertions and
/// user-projected Claims only. Entities without such evidence do not exist for the user.
fn user_entity(conn: &Connection, id: &str, kind: String) -> Result<EntityView> {
    let all = live_assertions(conn)?;
    let user_claim = user_claim_predicate();
    let mut usable = Vec::new();
    for a in all.into_iter().filter(|a| a.actor == "user") {
        if a.kind == "refers_to" {
            let visible: bool = conn.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM mention m JOIN claim c ON c.id=m.claim_id JOIN source s ON s.id=c.source_id WHERE m.id=?1 AND {user_claim})"),
                [&a.subject],
                |r| r.get(0),
            )?;
            if !visible {
                continue;
            }
        }
        usable.push(a);
    }
    let created_by_user: bool = conn.query_row(
        "SELECT created_by='user' FROM entity WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    let touched = |e: &str| {
        usable
            .iter()
            .any(|a| a.subject == e || (a.kind != "alias" && a.object == e))
    };
    if !created_by_user && !touched(id) {
        return Err(Error::NotFound);
    }
    let mut ids: BTreeSet<String> = BTreeSet::from([id.to_owned()]);
    for a in &usable {
        if a.kind == "same_as" || a.kind == "distinct_from" {
            ids.insert(a.subject.clone());
            ids.insert(a.object.clone());
        }
        if a.kind == "refers_to" {
            ids.insert(a.object.clone());
        }
    }
    let ids: Vec<String> = ids.into_iter().collect();
    let res = resolve(&ids, &usable, &["user"], IdentityPolicy::default());
    let (representative, members) = component(&res, id);
    let mut aliases = Vec::new();
    let mut mentions = Vec::new();
    for a in &usable {
        if a.kind == "alias" && members.contains(&a.subject) {
            aliases.push(AliasView {
                text: a.object.clone(),
                origin: "assertion".into(),
            });
        }
    }
    for (mention, (entity, _)) in &res.mentions {
        if !members.contains(entity) {
            continue;
        }
        let (claim_id, surface): (String, String) = conn.query_row(
            "SELECT claim_id,surface FROM mention WHERE id=?1",
            [mention],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        aliases.push(AliasView {
            text: surface.clone(),
            origin: "mention".into(),
        });
        mentions.push(MentionView {
            mention_id: mention.clone(),
            claim_id,
            surface,
        });
    }
    aliases.sort_by(|a, b| (&a.origin, &a.text).cmp(&(&b.origin, &b.text)));
    Ok(EntityView {
        id: id.into(),
        kind,
        representative,
        members,
        aliases,
        mentions,
        suggestions: None,
        blocked: None,
    })
}
/// Mentions of a Claim already known to be visible, with entities resolved in the projection.
pub(crate) fn claim_mentions(
    conn: &Connection,
    p: &Principal,
    claim: &str,
) -> Result<Vec<crate::claims::ClaimMention>> {
    let rows: Vec<(String, String, String)> = conn
        .prepare_cached("SELECT id,surface,span FROM mention WHERE claim_id=?1 ORDER BY id")?
        .query_map([claim], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let user = if p.kind() == PrincipalKind::User {
        let usable: Vec<Assertion> = live_assertions(conn)?
            .into_iter()
            .filter(|a| a.actor == "user")
            .collect();
        Some(resolve(
            &entities(conn)?,
            &usable,
            &["user"],
            IdentityPolicy::default(),
        ))
    } else {
        None
    };
    let mut out = Vec::new();
    for (id, surface, span) in rows {
        let span: serde_json::Value = serde_json::from_str(&span)
            .map_err(|_| Error::IntegrityFailure("mention span".into()))?;
        let entity = match &user {
            Some(res) => res
                .mentions
                .get(&id)
                .map(|(e, _)| res.representative.get(e).cloned().unwrap_or_else(|| e.clone())),
            None => conn
                .prepare_cached("SELECT r.representative FROM mention_resolution m JOIN entity_resolution r ON r.entity_id=m.entity_id WHERE m.mention_id=?1")?
                .query_row([&id], |r| r.get(0))
                .optional()?,
        };
        out.push(crate::claims::ClaimMention {
            id,
            surface,
            start: span["start"].as_u64().unwrap_or(0) as usize,
            end: span["end"].as_u64().unwrap_or(0) as usize,
            entity,
        });
    }
    Ok(out)
}

/// Claims (not yet visibility-filtered) mentioning an entity, given an entity id or a surface
/// string, resolved the way the caller's projection resolves identity: users only through
/// user-asserted identity, other principals through the materialized resolution. Callers still
/// intersect the result with their authorized candidate set.
pub(crate) fn claims_of_entity(conn: &Connection, p: &Principal, key: &str) -> Result<Vec<String>> {
    let rows: Vec<(String, String, String)> = conn
        .prepare("SELECT id,claim_id,surface FROM mention ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let user = if p.kind() == PrincipalKind::User {
        let usable: Vec<Assertion> = live_assertions(conn)?
            .into_iter()
            .filter(|a| a.actor == "user")
            .collect();
        Some(resolve(
            &entities(conn)?,
            &usable,
            &["user"],
            IdentityPolicy::default(),
        ))
    } else {
        None
    };
    let rep_of_entity = |entity: &str| -> Result<String> {
        Ok(match &user {
            Some(res) => res
                .representative
                .get(entity)
                .cloned()
                .unwrap_or_else(|| entity.to_owned()),
            None => conn
                .prepare_cached("SELECT representative FROM entity_resolution WHERE entity_id=?1")?
                .query_row([entity], |r| r.get(0))
                .optional()?
                .unwrap_or_else(|| entity.to_owned()),
        })
    };
    let rep_of_mention = |mention: &str| -> Result<Option<String>> {
        Ok(match &user {
            Some(res) => res.mentions.get(mention).map(|(e, _)| res.representative.get(e).cloned().unwrap_or_else(|| e.clone())),
            None => conn
                .prepare_cached("SELECT r.representative FROM mention_resolution m JOIN entity_resolution r ON r.entity_id=m.entity_id WHERE m.mention_id=?1")?
                .query_row([mention], |r| r.get(0))
                .optional()?,
        })
    };
    let lowered = key.to_lowercase();
    let is_entity: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entity WHERE id=?1)",
        [key],
        |r| r.get(0),
    )?;
    let mut reps = std::collections::BTreeSet::new();
    if is_entity {
        reps.insert(rep_of_entity(key)?);
    } else {
        for (mention, _, surface) in &rows {
            if surface.to_lowercase() == lowered {
                reps.extend(rep_of_mention(mention)?);
            }
        }
    }
    let mut claims = std::collections::BTreeSet::new();
    for (mention, claim, surface) in &rows {
        let hit = (!is_entity && surface.to_lowercase() == lowered)
            || rep_of_mention(mention)?.is_some_and(|r| reps.contains(&r));
        if hit {
            claims.insert(claim.clone());
        }
    }
    Ok(claims.into_iter().collect())
}
