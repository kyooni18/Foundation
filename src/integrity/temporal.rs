//! Temporal relations and as_of reads (DATA_CONTRACTS §7, §8; D-18/D-19).
//! Model judgments are immutable hints; relations are user/agent assertions. Supersession
//! never invalidates or hides a Claim: superseded Claims are returned as history. Unknown
//! times stay unknown; server capture time is used only as a factual upper bound.
use super::{claim_referable, claim_visible, lifecycle_sql, validity::Validity};
use crate::{App, ListSources, claims::ClaimView, domain::*};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

/// Provisional freshness thresholds (D-19). Settings, not truth rules: age never invalidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreshnessPolicy {
    pub habit_state_days: i64,
    pub observation_days: i64,
}
impl Default for FreshnessPolicy {
    fn default() -> Self {
        Self {
            habit_state_days: 60,
            observation_days: 14,
        }
    }
}
/// A model's temporal judgment about (claim_a, claim_b); `supersedes` means a supersedes b.
#[derive(Debug, Clone)]
pub struct TemporalJudgment {
    pub claim_a: String,
    pub claim_b: String,
    pub provider: String,
    pub model_id: String,
    pub prompt_hash: String,
    pub input_digest: String,
    pub raw_label: String,
    pub output_json: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RelationView {
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub object: String,
    pub actor: String,
    /// Computed from the actor: user assertions are confirmed, agent assertions provisional.
    pub standing: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct JudgmentView {
    pub id: String,
    pub claim_a: String,
    pub claim_b: String,
    pub model_id: String,
    pub raw_label: String,
}
#[derive(Debug, Serialize)]
pub struct RelationsView {
    pub relations: Vec<RelationView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judgments: Option<Vec<JudgmentView>>,
}
#[derive(Debug, Serialize)]
pub struct TemporalItem {
    pub claim: ClaimView,
    /// applies | not_yet_stated | scheduled | ended | superseded | shadowed | unsupported | unknown
    pub status: &'static str,
    /// Earlier or inapplicable Claims are retained and returned as history, never hidden.
    pub history: bool,
    pub superseded_by: Vec<String>,
    pub possibly_superseded_by: Vec<String>,
    /// True when supersession comes only from an agent assertion or a model judgment.
    pub provisional: bool,
    pub freshness: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct TemporalPage {
    pub as_of: String,
    pub present: bool,
    pub items: Vec<TemporalItem>,
    pub total: usize,
    pub next_cursor: Option<String>,
}
fn time(s: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).map_err(|_| Error::IntegrityFailure("stored time".into()))
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tri {
    Yes,
    No,
    Unknown,
}
/// Statement time: exact when authored_at is known; otherwise only "no later than capture".
#[derive(Clone, Copy)]
struct Said {
    lo: Option<OffsetDateTime>,
    hi: OffsetDateTime,
}
impl Said {
    fn after(self, a: OffsetDateTime) -> Tri {
        if self.lo.is_some_and(|lo| lo > a) {
            Tri::Yes
        } else if self.hi <= a {
            Tri::No
        } else {
            Tri::Unknown
        }
    }
}
struct Facts {
    tkind: Option<String>,
    from: Option<OffsetDateTime>,
    to: Option<OffsetDateTime>,
    said: Said,
    uncertain: bool,
    validity: Validity,
}
impl Facts {
    /// When the statement starts to describe the world (R2 `effective`), compared with `a`.
    fn effective_after(&self, a: OffsetDateTime) -> Tri {
        if let Some(f) = self.from {
            return if f > a { Tri::Yes } else { Tri::No };
        }
        if let Some(t) = self.to {
            // An end without a start describes a period of unknown start (R2 v2 fix).
            let before_said = if self.said.lo.is_some_and(|lo| t < lo) {
                Tri::Yes
            } else if self.said.hi <= t {
                Tri::No
            } else {
                Tri::Unknown
            };
            return match before_said {
                Tri::Yes => Tri::No,
                Tri::No => self.said.after(a),
                Tri::Unknown if self.said.after(a) == Tri::No => Tri::No,
                Tri::Unknown => Tri::Unknown,
            };
        }
        self.said.after(a)
    }
    fn kind(&self, k: &str) -> bool {
        self.tkind.as_deref() == Some(k)
    }
}
/// (tkind, applies_from, applies_to, index_fields, recorder, authored_at, captured_at, validity)
type FactRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
);
fn facts(conn: &Connection, p: &Principal, id: &str) -> Result<Facts> {
    let (tkind, from, to, index, recorder, authored, captured, v): FactRow = conn.query_row(
        "SELECT c.tkind,c.applies_from,c.applies_to,c.index_fields,s.recorder,s.authored_at,s.captured_at,v.state FROM claim c JOIN source s ON s.id=c.source_id JOIN claim_validity v ON v.claim_id=c.id WHERE c.id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )?;
    // D-40/D-55: the user projection may not use model metadata derived from unconfirmed
    // non-user Source text; such Claims keep only the server capture bound.
    let trusted = p.kind() != PrincipalKind::User || recorder == "user";
    let parse = |s: Option<String>| s.map(|s| time(&s)).transpose();
    let captured = time(&captured)?;
    let uncertain = trusted
        && index
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|v| v["certainty"] == "uncertain");
    Ok(Facts {
        tkind,
        from: if trusted { parse(from)? } else { None },
        to: if trusted { parse(to)? } else { None },
        said: match authored.filter(|_| trusted) {
            Some(a) => {
                let a = time(&a)?;
                Said { lo: Some(a), hi: a }
            }
            None => Said {
                lo: None,
                hi: captured,
            },
        },
        uncertain,
        validity: Validity::parse(&v)?,
    })
}
impl App {
    /// Trusted in-process worker API. Records a hint only; it never writes a relation,
    /// rewrites a Claim or creates a user/agent assertion. Idempotent per contract key.
    pub fn record_temporal_judgment(&self, j: &TemporalJudgment) -> Result<String> {
        if !["supersedes", "exception", "backfill", "compatible"].contains(&j.raw_label.as_str()) {
            return Err(invalid("raw_label", "unknown temporal label"));
        }
        serde_json::from_str::<serde_json::Value>(&j.output_json)
            .map_err(|_| invalid("output_json", "invalid JSON"))?;
        for (field, v) in [
            ("provider", &j.provider),
            ("model_id", &j.model_id),
            ("prompt_hash", &j.prompt_hash),
            ("input_digest", &j.input_digest),
        ] {
            if v.is_empty() || v.len() > 512 {
                return Err(invalid(field, "must be 1..=512 bytes"));
            }
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let key = params![
            j.claim_a,
            j.claim_b,
            j.model_id,
            j.prompt_hash,
            j.input_digest
        ];
        let existing: Option<String> = tx
            .query_row("SELECT id FROM model_judgment WHERE task='temporal' AND claim_a=?1 AND claim_b=?2 AND model_id=?3 AND prompt_hash=?4 AND input_digest=?5", key, |r| r.get(0))
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        for c in [&j.claim_a, &j.claim_b] {
            tx.query_row("SELECT 1 FROM claim WHERE id=?1", [c], |_| Ok(()))
                .optional()?
                .ok_or(Error::NotFound)?;
        }
        let id = ulid::Ulid::new().to_string();
        tx.execute("INSERT INTO model_judgment(id,task,claim_a,claim_b,provider,model_id,prompt_hash,input_digest,raw_label,output_json,created_at) VALUES(?1,'temporal',?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![id,j.claim_a,j.claim_b,j.provider,j.model_id,j.prompt_hash,j.input_digest,j.raw_label,j.output_json,Timestamp::now().as_str()])?;
        tx.commit()?;
        Ok(id)
    }
    /// User or agent temporal assertion. Conflict relations belong to Phase 7.
    pub fn assert_relation(
        &self,
        p: &Principal,
        kind: &str,
        subject: &str,
        object: &str,
        reason: Option<String>,
        judgment_id: Option<&str>,
    ) -> Result<String> {
        if p.kind() == PrincipalKind::Import {
            return Err(Error::Forbidden);
        }
        if !["supersedes", "exception", "backfill"].contains(&kind) {
            return Err(invalid("kind", "unsupported relation"));
        }
        let reason = super::reason(reason)?;
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        if !claim_referable(&tx, p, subject)? || !claim_referable(&tx, p, object)? {
            return Err(Error::NotFound);
        }
        if subject == object {
            return Err(invalid("relation", "a Claim cannot relate to itself"));
        }
        if let Some(j) = judgment_id {
            let pair: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM model_judgment WHERE id=?1 AND ((claim_a=?2 AND claim_b=?3) OR (claim_a=?3 AND claim_b=?2)))", params![j, subject, object], |r| r.get(0))?;
            if !pair {
                return Err(Error::NotFound);
            }
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM relation WHERE kind=?1 AND subject=?2 AND object=?3 AND actor=?4",
                params![kind, subject, object, p.kind().as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        let id = ulid::Ulid::new().to_string();
        tx.execute("INSERT INTO relation(id,kind,subject,object,actor,judgment_id,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, kind, subject, object, p.kind().as_str(), judgment_id, reason, Timestamp::now().as_str()])
            .map_err(|e| match e.sqlite_error_code() {
                Some(rusqlite::ErrorCode::ConstraintViolation) => invalid("relation", "supersedes cycle"),
                _ => e.into(),
            })?;
        tx.commit()?;
        Ok(id)
    }
    pub fn claim_relations(&self, p: &Principal, id: &str) -> Result<RelationsView> {
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        if !claim_visible(&tx, p, id, false)? {
            return Err(Error::NotFound);
        }
        let mut relations = Vec::new();
        for r in visible_relations(&tx, p)? {
            if r.subject == id || r.object == id {
                relations.push(r);
            }
        }
        let judgments = if p.kind() == PrincipalKind::User {
            None
        } else {
            let mut out = Vec::new();
            for j in visible_judgments(&tx, p)? {
                if j.claim_a == id || j.claim_b == id {
                    out.push(j);
                }
            }
            Some(out)
        };
        Ok(RelationsView {
            relations,
            judgments,
        })
    }
    /// Temporal reading of the visible current Claims at `as_of` (default: now, present).
    pub fn claims_as_of(
        &self,
        p: &Principal,
        as_of: Option<Timestamp>,
        present: Option<bool>,
        request: ListSources,
    ) -> Result<TemporalPage> {
        if !(1..=100).contains(&request.limit) || request.include_retracted {
            return Err(invalid("limit", "must be 1..=100"));
        }
        let present = present.unwrap_or(as_of.is_none());
        let as_of = as_of.unwrap_or_else(Timestamp::now);
        let domain = format!("asof-v1/{}/{present}", as_of.as_str());
        let after = self.claim_cursor_decode(p, &domain, request.cursor.as_deref())?;
        let a = time(as_of.as_str())?;
        let visible = self.read_claims(p, false, false)?;
        let statuses = {
            let mut conn = self.store.reader()?;
            let tx = conn.transaction()?;
            evaluate(&tx, p, &visible, a, present, &self.policy)?
        };
        let total = visible.len();
        let mut rest = visible.into_iter().filter(|c| c.id > after);
        let claims: Vec<ClaimView> = rest.by_ref().take(request.limit as usize).collect();
        let next_cursor = if rest.next().is_some() {
            Some(self.claim_cursor_encode(p, &domain, &claims.last().expect("limit").id))
        } else {
            None
        };
        let ids: Vec<String> = claims.iter().map(|c| c.id.clone()).collect();
        self.note_reads(p, &ids)?;
        let mut statuses = statuses;
        let items = claims
            .into_iter()
            .map(|claim| {
                let (status, superseded_by, possibly, provisional, freshness) =
                    statuses.remove(&claim.id).expect("evaluated");
                TemporalItem {
                    history: status != "applies",
                    claim,
                    status,
                    superseded_by,
                    possibly_superseded_by: possibly,
                    provisional,
                    freshness,
                }
            })
            .collect();
        Ok(TemporalPage {
            as_of: as_of.as_str().into(),
            present,
            items,
            total,
            next_cursor,
        })
    }
}
fn visible_relations(conn: &Connection, p: &Principal) -> Result<Vec<RelationView>> {
    let actors = if p.kind() == PrincipalKind::User {
        "actor='user'"
    } else {
        "actor IN ('user','agent')"
    };
    let mut out = Vec::new();
    for row in conn
        .prepare(&format!(
            "SELECT id,kind,subject,object,actor FROM relation r WHERE {actors} AND {}='restore' ORDER BY id",
            lifecycle_sql("relation", "r.id")
        ))?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
    {
        let (id, kind, subject, object, actor) = row?;
        // Both ends must be inside the projection; otherwise the edge does not exist here.
        if claim_visible(conn, p, &subject, false)? && claim_visible(conn, p, &object, false)? {
            out.push(RelationView {
                standing: if actor == "user" {
                    "confirmed"
                } else {
                    "provisional"
                }
                .into(),
                id,
                kind,
                subject,
                object,
                actor,
            });
        }
    }
    Ok(out)
}
fn visible_judgments(conn: &Connection, p: &Principal) -> Result<Vec<JudgmentView>> {
    if p.kind() == PrincipalKind::User {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for row in conn
        .prepare("SELECT id,claim_a,claim_b,model_id,raw_label FROM model_judgment WHERE task='temporal' ORDER BY id")?
        .query_map([], |r| {
            Ok(JudgmentView {
                id: r.get(0)?,
                claim_a: r.get(1)?,
                claim_b: r.get(2)?,
                model_id: r.get(3)?,
                raw_label: r.get(4)?,
            })
        })?
    {
        let j = row?;
        if claim_visible(conn, p, &j.claim_a, false)? && claim_visible(conn, p, &j.claim_b, false)? {
            out.push(j);
        }
    }
    Ok(out)
}
type Status = (
    &'static str,
    Vec<String>,
    Vec<String>,
    bool,
    Vec<&'static str>,
);
/// The R2 `history` read rule over the projection's Claims and relations.
fn evaluate(
    conn: &Connection,
    p: &Principal,
    visible: &[ClaimView],
    a: OffsetDateTime,
    present: bool,
    policy: &crate::Policy,
) -> Result<HashMap<String, Status>> {
    let ids: BTreeSet<&str> = visible.iter().map(|c| c.id.as_str()).collect();
    let mut facts_of: BTreeMap<&str, Facts> = BTreeMap::new();
    for id in &ids {
        facts_of.insert(id, facts(conn, p, id)?);
    }
    let relations = visible_relations(conn, p)?;
    let unsupported = |id: &str| facts_of[id].validity == Validity::Invalid;
    // Effective supersession: live relations (subject still supported) and, for non-user
    // views, model `supersedes` hints not overridden by any user relation on the pair.
    let mut supersedes: Vec<(String, String, bool)> = Vec::new();
    let mut exception_subjects = BTreeSet::new();
    let mut backfill_subjects = BTreeSet::new();
    let mut component: BTreeMap<String, String> = ids
        .iter()
        .map(|id| (id.to_string(), id.to_string()))
        .collect();
    fn root(c: &BTreeMap<String, String>, mut x: String) -> String {
        while c[&x] != x {
            x = c[&x].clone();
        }
        x
    }
    fn join(c: &mut BTreeMap<String, String>, x: &str, y: &str) {
        let (rx, ry) = (root(c, x.into()), root(c, y.into()));
        if rx != ry {
            let (keep, other) = if rx < ry { (rx, ry) } else { (ry, rx) };
            c.insert(other, keep);
        }
    }
    for r in &relations {
        join(&mut component, &r.subject, &r.object);
        match r.kind.as_str() {
            "supersedes" if !unsupported(&r.subject) => {
                supersedes.push((r.subject.clone(), r.object.clone(), r.actor != "user"))
            }
            "exception" => {
                exception_subjects.insert(r.subject.clone());
            }
            "backfill" => {
                backfill_subjects.insert(r.subject.clone());
            }
            _ => (),
        }
    }
    let user_pairs: BTreeSet<(String, String)> = relations
        .iter()
        .filter(|r| r.actor == "user")
        .flat_map(|r| {
            [
                (r.subject.clone(), r.object.clone()),
                (r.object.clone(), r.subject.clone()),
            ]
        })
        .collect();
    for j in visible_judgments(conn, p)? {
        if j.raw_label != "supersedes"
            || policy.blocked_temporal_models.contains(&j.model_id)
            || unsupported(&j.claim_a)
            || unsupported(&j.claim_b)
            || user_pairs.contains(&(j.claim_a.clone(), j.claim_b.clone()))
        {
            continue;
        }
        join(&mut component, &j.claim_a, &j.claim_b);
        supersedes.push((j.claim_a, j.claim_b, true));
    }
    let report_past = |id: &str| facts_of[id].kind("report_past") || backfill_subjects.contains(id);
    let mut out: HashMap<String, Status> = HashMap::new();
    let mut candidates: BTreeSet<&str> = BTreeSet::new();
    for id in &ids {
        let f = &facts_of[id];
        let covers = f.to.is_some_and(|t| a <= t);
        let reports = f.from.is_some_and(|from| from <= a) && f.kind("report_past");
        let status = if f.validity == Validity::Invalid {
            Some("unsupported")
        } else if !covers && !reports && f.said.after(a) == Tri::Yes {
            Some("not_yet_stated")
        } else if !covers && !reports && f.said.after(a) == Tri::Unknown {
            Some("unknown")
        } else {
            match f.effective_after(a) {
                Tri::Yes => Some("scheduled"),
                Tri::Unknown => Some("unknown"),
                Tri::No => {
                    let limited = exception_subjects.contains(*id)
                        || f.kind("habit")
                        || f.kind("state")
                        || f.kind("decision");
                    if limited && f.to.is_some_and(|t| a > t) {
                        Some("ended")
                    } else {
                        None
                    }
                }
            }
        };
        match status {
            Some(s) => {
                out.insert(id.to_string(), (s, vec![], vec![], false, vec![]));
            }
            None => {
                candidates.insert(id);
            }
        }
    }
    // Only a statement that is itself applicable at as_of supersedes (R2).
    let mut superseded: BTreeMap<&str, (Vec<String>, bool)> = BTreeMap::new();
    for (newer, older, provisional) in &supersedes {
        if candidates.contains(newer.as_str()) && candidates.contains(older.as_str()) {
            let e = superseded.entry(older.as_str()).or_default();
            e.0.push(newer.clone());
            e.1 |= *provisional;
        }
    }
    let remaining: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|id| !superseded.contains_key(id))
        .collect();
    for (id, (by, provisional)) in &superseded {
        let mut by = by.clone();
        by.sort();
        by.dedup();
        out.insert(
            id.to_string(),
            ("superseded", by, vec![], *provisional, vec![]),
        );
    }
    for id in &remaining {
        let f = &facts_of[id];
        let mine = root(&component, id.to_string());
        let others = remaining
            .iter()
            .any(|o| o != id && !report_past(o) && root(&component, o.to_string()) == mine);
        // Past reports fill in only when nothing else in their timeline describes as_of.
        if report_past(id) && others && f.to.is_none_or(|t| a > t) {
            out.insert(id.to_string(), ("shadowed", vec![], vec![], false, vec![]));
            continue;
        }
        let mut freshness = Vec::new();
        if f.uncertain {
            freshness.push("uncertain");
        }
        if present {
            if f.kind("decision") {
                freshness.push("unexecuted_decision");
            }
            let limit = if f.kind("observation") {
                Some(policy.freshness.observation_days)
            } else if f.kind("habit") || f.kind("state") {
                Some(policy.freshness.habit_state_days)
            } else {
                None
            };
            if let Some(days) = limit {
                let limit = Duration::days(days);
                match f.said.lo {
                    Some(said) if a - said > limit => freshness.push("stale"),
                    Some(_) => (),
                    // Capture is a known upper bound of the statement time.
                    None if a - f.said.hi > limit => freshness.push("stale"),
                    None => freshness.push("age_unknown"),
                }
            }
        }
        // A superseder whose own start is unknown at as_of is reported, not applied.
        let mut possibly: Vec<String> = supersedes
            .iter()
            .filter(|(newer, older, _)| {
                older == id && out.get(newer.as_str()).is_some_and(|s| s.0 == "unknown")
            })
            .map(|(newer, _, _)| newer.clone())
            .collect();
        possibly.sort();
        possibly.dedup();
        out.insert(
            id.to_string(),
            ("applies", vec![], possibly, false, freshness),
        );
    }
    Ok(out)
}
