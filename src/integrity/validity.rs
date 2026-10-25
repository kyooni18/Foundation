//! Materialized three-valued Claim validity (DATA_CONTRACTS §6, D-15/D-92).
//! `invalid` means support is no longer established, never that a proposition is false.
//! Normal operation updates the affected downward closure inside the writer transaction;
//! `oracle` independently replays authoritative history for verification and rebuilds.
use super::{lifecycle_sql, revalidation};
use crate::domain::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Validity {
    Valid,
    NeedsRevalidation,
    Invalid,
}
impl Validity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::NeedsRevalidation => "needs_revalidation",
            Self::Invalid => "invalid",
        }
    }
    pub(crate) fn parse(s: &str) -> Result<Self> {
        match s {
            "valid" => Ok(Self::Valid),
            "needs_revalidation" => Ok(Self::NeedsRevalidation),
            "invalid" => Ok(Self::Invalid),
            _ => Err(Error::IntegrityFailure("unknown validity".into())),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct State {
    pub validity: Validity,
    /// Sorted, comma-separated reason codes; empty for valid Claims.
    pub reasons: String,
    pub deps_unknown: bool,
}
fn state(validity: Validity, reasons: BTreeSet<&'static str>, deps_unknown: bool) -> State {
    State {
        validity,
        reasons: reasons.into_iter().collect::<Vec<_>>().join(","),
        deps_unknown,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Premise,
    Restates,
    Alternative,
    InformedBy,
}
fn kind(s: &str) -> Result<Kind> {
    match s {
        "premise" => Ok(Kind::Premise),
        "restates" => Ok(Kind::Restates),
        "alternative" => Ok(Kind::Alternative),
        "informed_by" => Ok(Kind::InformedBy),
        _ => Err(Error::IntegrityFailure("unknown dependency kind".into())),
    }
}
fn lifecycle_reason(l: Lifecycle, source: bool) -> Option<&'static str> {
    match (l, source) {
        (Lifecycle::Live, _) => None,
        (Lifecycle::Retracted, false) => Some("retracted"),
        (Lifecycle::Forgotten, false) => Some("forgotten"),
        (Lifecycle::Retracted, true) => Some("source_retracted"),
        (Lifecycle::Forgotten, true) => Some("source_forgotten"),
    }
}
fn event_lifecycle(event: &str) -> Result<Lifecycle> {
    Ok(event.parse::<LifecycleEvent>()?.lifecycle())
}

/// Authoritative inputs of one Claim: its own and Source lifecycle, live declared parents,
/// and (only when it declared nothing) Claims read earlier in the same agent session.
struct Node {
    own: Lifecycle,
    source: Lifecycle,
    parents: Vec<(String, Kind)>,
    deps_unknown: bool,
    reads: Vec<String>,
}
fn load(conn: &Connection, id: &str) -> Result<Node> {
    let (own, source, recorder, session, captured): (
        String,
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .prepare_cached(&format!(
            "SELECT {},{},s.recorder,s.session_id,s.captured_at FROM claim c JOIN source s ON s.id=c.source_id WHERE c.id=?1",
            lifecycle_sql("claim", "c.id"),
            lifecycle_sql("source", "s.id")
        ))?
        .query_row([id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()?
        .ok_or(Error::NotFound)?;
    let parents = conn
        .prepare_cached(&format!(
            "SELECT parent,kind FROM claim_dependency d WHERE child=?1 AND {}='restore' ORDER BY parent,kind",
            lifecycle_sql("dependency", "d.id")
        ))?
        .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .map(|row| {
            let (parent, k) = row?;
            Ok((parent, kind(&k)?))
        })
        .collect::<Result<Vec<_>>>()?;
    let reads = match session {
        Some(session) if recorder == "agent" && parents.is_empty() => conn
            .prepare_cached(
                "SELECT claim_id FROM session_read WHERE session_id=?1 AND at<=?2 AND claim_id<>?3 ORDER BY claim_id",
            )?
            .query_map(params![session, captured, id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?,
        _ => Vec::new(),
    };
    Ok(Node {
        own: event_lifecycle(&own)?,
        source: event_lifecycle(&source)?,
        deps_unknown: !reads.is_empty(),
        parents,
        reads,
    })
}
/// Phase A: invalidity depends only on lifecycles and the invalidity of mandatory parents.
fn invalid_reasons(
    node: &Node,
    invalid: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<BTreeSet<&'static str>> {
    let mut reasons = BTreeSet::new();
    reasons.extend(lifecycle_reason(node.own, false));
    reasons.extend(lifecycle_reason(node.source, true));
    let (mut alternatives, mut lost) = (0, 0);
    for (parent, k) in &node.parents {
        match k {
            Kind::Premise if invalid(parent)? => {
                reasons.insert("premise_invalid");
            }
            Kind::Restates if invalid(parent)? => {
                reasons.insert("restates_invalid");
            }
            Kind::Alternative => {
                alternatives += 1;
                lost += usize::from(invalid(parent)?);
            }
            _ => (),
        }
    }
    if alternatives > 0 && lost == alternatives {
        reasons.insert("alternatives_invalid");
    }
    Ok(reasons)
}
/// Phase B: uncertainty from mandatory parents, unmet alternatives, advisory parents and
/// undeclared session reads. Advisory and read edges consult only invalidity (Phase A), so
/// read edges never need ordering and cannot form evaluation cycles.
fn evaluate(
    node: &Node,
    invalid: &mut dyn FnMut(&str) -> Result<bool>,
    validity: &mut dyn FnMut(&str) -> Result<Validity>,
) -> Result<State> {
    let reasons = invalid_reasons(node, invalid)?;
    if !reasons.is_empty() {
        return Ok(state(Validity::Invalid, reasons, node.deps_unknown));
    }
    let mut reasons = BTreeSet::new();
    let (mut alternatives, mut sufficient) = (0, 0);
    for (parent, k) in &node.parents {
        match k {
            Kind::Premise | Kind::Restates if validity(parent)? == Validity::NeedsRevalidation => {
                reasons.insert("premise_uncertain");
            }
            Kind::Alternative => {
                alternatives += 1;
                sufficient += usize::from(validity(parent)? == Validity::Valid);
            }
            Kind::InformedBy if invalid(parent)? => {
                reasons.insert("advisory_invalid");
            }
            _ => (),
        }
    }
    if alternatives > 0 && sufficient == 0 {
        reasons.insert("alternatives_uncertain");
    }
    for read in &node.reads {
        if invalid(read)? {
            reasons.insert("session_read_invalid");
            break;
        }
    }
    let validity = if reasons.is_empty() {
        Validity::Valid
    } else {
        Validity::NeedsRevalidation
    };
    Ok(state(validity, reasons, node.deps_unknown))
}
fn stored(conn: &Connection, id: &str) -> Result<Option<State>> {
    conn.prepare_cached("SELECT state,reasons,deps_unknown FROM claim_validity WHERE claim_id=?1")?
        .query_row([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })
        .optional()?
        .map(|(v, reasons, deps_unknown)| {
            Ok(State {
                validity: Validity::parse(&v)?,
                reasons,
                deps_unknown,
            })
        })
        .transpose()
}
fn materialized(conn: &Connection, id: &str) -> Result<Validity> {
    stored(conn, id)?
        .map(|s| s.validity)
        .ok_or_else(|| Error::IntegrityFailure("missing materialized validity".into()))
}
fn write(conn: &Connection, id: &str, s: &State) -> Result<()> {
    conn.prepare_cached("INSERT INTO claim_validity(claim_id,state,reasons,deps_unknown) VALUES(?1,?2,?3,?4) ON CONFLICT(claim_id) DO UPDATE SET state=excluded.state,reasons=excluded.reasons,deps_unknown=excluded.deps_unknown")?
        .execute(params![id, s.validity.as_str(), s.reasons, s.deps_unknown])?;
    Ok(())
}
fn children(conn: &Connection, id: &str) -> Result<Vec<String>> {
    // Declared live edges, plus agent Claims whose undeclared session read included `id`.
    // CROSS JOIN pins the plan to start from the read (session_read_claim index); otherwise
    // SQLite may scan every agent Source per call.
    Ok(conn
        .prepare_cached(&format!(
            "SELECT child FROM claim_dependency d WHERE parent=?1 AND {}='restore' UNION SELECT c.id FROM session_read r CROSS JOIN source s ON s.session_id=r.session_id AND s.recorder='agent' AND s.captured_at>=r.at JOIN claim c ON c.source_id=s.id WHERE r.claim_id=?1 AND c.id<>?1",
            lifecycle_sql("dependency", "d.id")
        ))?
        .query_map([id], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?)
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Propagation {
    pub closure: usize,
    pub changed: usize,
    pub scheduled: usize,
}
/// Recompute the downward closure of `seeds` in topological order and persist changes.
/// Must run inside the same writer transaction as the authoritative change.
pub(crate) fn propagate(
    conn: &Connection,
    seeds: impl IntoIterator<Item = String>,
) -> Result<Propagation> {
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    for seed in seeds {
        if seen.insert(seed.clone()) {
            queue.push_back(seed);
        }
    }
    while let Some(id) = queue.pop_front() {
        for child in children(conn, &id)? {
            if seen.insert(child.clone()) {
                queue.push_back(child);
            }
        }
        order.push(id);
    }
    let mut nodes: HashMap<String, Node> = HashMap::with_capacity(order.len());
    for id in &order {
        nodes.insert(id.clone(), load(conn, id)?);
    }
    // Kahn over declared live edges inside the closure (the declared graph is acyclic).
    let mut indegree: HashMap<&str, usize> = order.iter().map(|id| (id.as_str(), 0)).collect();
    let mut out: HashMap<&str, Vec<&str>> = HashMap::new();
    for id in &order {
        for (parent, _) in &nodes[id].parents {
            if let Some((p, _)) = nodes.get_key_value(parent) {
                *indegree.get_mut(id.as_str()).expect("closure") += 1;
                out.entry(p.as_str()).or_default().push(id.as_str());
            }
        }
    }
    let mut ready: VecDeque<&str> = order
        .iter()
        .map(String::as_str)
        .filter(|id| indegree[id] == 0)
        .collect();
    let mut topo: Vec<&str> = Vec::with_capacity(order.len());
    while let Some(id) = ready.pop_front() {
        topo.push(id);
        for &child in out.get(id).into_iter().flatten() {
            let d = indegree.get_mut(child).expect("closure");
            *d -= 1;
            if *d == 0 {
                ready.push_back(child);
            }
        }
    }
    if topo.len() != order.len() {
        return Err(Error::IntegrityFailure(
            "dependency cycle in closure".into(),
        ));
    }
    // Inputs outside the closure are unchanged by this transaction; read them once.
    let mut outside: HashMap<String, Validity> = HashMap::new();
    for node in nodes.values() {
        for id in node.parents.iter().map(|(p, _)| p).chain(&node.reads) {
            if !nodes.contains_key(id) && !outside.contains_key(id) {
                outside.insert(id.clone(), materialized(conn, id)?);
            }
        }
    }
    let mut invalid: HashMap<&str, bool> = HashMap::with_capacity(topo.len());
    for &id in &topo {
        let reasons = invalid_reasons(&nodes[id], &mut |p| match invalid.get(p) {
            Some(v) => Ok(*v),
            None => outside
                .get(p)
                .map(|v| *v == Validity::Invalid)
                .ok_or_else(|| Error::IntegrityFailure("closure order violated".into())),
        })?;
        invalid.insert(id, !reasons.is_empty());
    }
    let mut computed: HashMap<&str, State> = HashMap::with_capacity(topo.len());
    let mut result = Propagation {
        closure: topo.len(),
        ..Default::default()
    };
    for &id in &topo {
        let new = evaluate(
            &nodes[id],
            &mut |p| match invalid.get(p) {
                Some(v) => Ok(*v),
                None => Ok(outside[p] == Validity::Invalid),
            },
            &mut |p| match computed.get(p) {
                Some(s) => Ok(s.validity),
                None => outside
                    .get(p)
                    .copied()
                    .ok_or_else(|| Error::IntegrityFailure("closure order violated".into())),
            },
        )?;
        let old = stored(conn, id)?;
        if old.as_ref() != Some(&new) {
            write(conn, id, &new)?;
            result.changed += 1;
            let entered = new.validity == Validity::NeedsRevalidation
                && old
                    .as_ref()
                    .is_none_or(|o| o.validity != new.validity || o.reasons != new.reasons);
            if entered {
                let cause = if new.reasons == "session_read_invalid" {
                    "session_read"
                } else {
                    "dependency"
                };
                result.scheduled += usize::from(revalidation::schedule(conn, id, cause)?);
            }
        }
        computed.insert(id, new);
    }
    Ok(result)
}
/// Deterministic local recheck used by revalidation jobs: recompute one Claim from its
/// authoritative inputs and materialized parents; repair the closure if they disagree.
pub(crate) fn recheck(conn: &Connection, id: &str) -> Result<(State, bool)> {
    let node = load(conn, id)?;
    let fresh = evaluate(
        &node,
        &mut |p| Ok(materialized(conn, p)? == Validity::Invalid),
        &mut |p| materialized(conn, p),
    )?;
    if stored(conn, id)?.as_ref() == Some(&fresh) {
        return Ok((fresh, false));
    }
    propagate(conn, [id.to_owned()])?;
    Ok((fresh, true))
}
pub(crate) fn of(conn: &Connection, id: &str) -> Result<(Validity, String, bool)> {
    let s = stored(conn, id)?
        .ok_or_else(|| Error::IntegrityFailure("missing materialized validity".into()))?;
    Ok((s.validity, s.reasons, s.deps_unknown))
}

/// Independent full recomputation. It replays the status-event log in sequence order instead
/// of using latest-event subqueries, evaluates rules written separately from `evaluate`, and
/// pulls parent results by depth-first memoization rather than pushing a closure.
pub(crate) fn oracle(conn: &Connection) -> Result<HashMap<String, State>> {
    let mut current: HashMap<(String, String), String> = HashMap::new();
    for row in conn
        .prepare("SELECT target_kind,target_id,event FROM status_event WHERE event IN ('retract','forget','restore') ORDER BY seq")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
    {
        let (k, id, event) = row?;
        current.insert((k, id), event);
    }
    let status = |k: &str, id: &str| -> Lifecycle {
        match current
            .get(&(k.to_owned(), id.to_owned()))
            .map(String::as_str)
        {
            Some("retract") => Lifecycle::Retracted,
            Some("forget") => Lifecycle::Forgotten,
            _ => Lifecycle::Live,
        }
    };
    struct SourceRow {
        agent: bool,
        session: Option<String>,
        captured: String,
        lifecycle: Lifecycle,
    }
    let mut sources: HashMap<String, SourceRow> = HashMap::new();
    for row in conn
        .prepare("SELECT id,recorder,session_id,captured_at FROM source")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
    {
        let (id, recorder, session, captured) = row?;
        let lifecycle = status("source", &id);
        sources.insert(
            id,
            SourceRow {
                agent: recorder == "agent",
                session,
                captured,
                lifecycle,
            },
        );
    }
    let claims: Vec<(String, String)> = conn
        .prepare("SELECT id,source_id FROM claim")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut premises: HashMap<String, Vec<String>> = HashMap::new();
    let mut restates: HashMap<String, Vec<String>> = HashMap::new();
    let mut alternatives: HashMap<String, Vec<String>> = HashMap::new();
    let mut advisory: HashMap<String, Vec<String>> = HashMap::new();
    for row in conn
        .prepare("SELECT id,child,parent,kind FROM claim_dependency")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
    {
        let (id, child, parent, k) = row?;
        if status("dependency", &id) != Lifecycle::Live {
            continue;
        }
        let bucket = match k.as_str() {
            "premise" => &mut premises,
            "restates" => &mut restates,
            "alternative" => &mut alternatives,
            "informed_by" => &mut advisory,
            _ => return Err(Error::IntegrityFailure("unknown dependency kind".into())),
        };
        bucket.entry(child).or_default().push(parent);
    }
    let declares = |c: &str| {
        [&premises, &restates, &alternatives, &advisory]
            .iter()
            .any(|m| m.contains_key(c))
    };
    let mut by_session: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for row in conn
        .prepare("SELECT session_id,claim_id,at FROM session_read")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
    {
        let (session, claim, at) = row?;
        by_session.entry(session).or_default().push((claim, at));
    }
    let mut unknown: HashMap<String, Vec<String>> = HashMap::new();
    let mut own: HashMap<String, (Lifecycle, Lifecycle)> = HashMap::new();
    for (id, source_id) in &claims {
        let source = &sources[source_id];
        own.insert(id.clone(), (status("claim", id), source.lifecycle));
        if !source.agent || declares(id) {
            continue;
        }
        if let Some(reads) = source.session.as_ref().and_then(|s| by_session.get(s)) {
            let relevant: Vec<String> = reads
                .iter()
                .filter(|(claim, at)| claim != id && at.as_str() <= source.captured.as_str())
                .map(|(claim, _)| claim.clone())
                .collect();
            if !relevant.is_empty() {
                unknown.insert(id.clone(), relevant);
            }
        }
    }
    let empty = Vec::new();
    let get = |m: &HashMap<String, Vec<String>>, id: &str| m.get(id).unwrap_or(&empty).clone();
    let mandatory = |id: &str| {
        let mut v = get(&premises, id);
        v.extend(get(&restates, id));
        v.extend(get(&alternatives, id));
        v
    };
    // Pass 1: invalidity, memoized post-order over mandatory and alternative parents.
    let mut dead: HashMap<String, BTreeSet<&'static str>> = HashMap::new();
    for (root, _) in &claims {
        let mut stack = vec![root.clone()];
        while let Some(id) = stack.last().cloned() {
            if dead.contains_key(&id) {
                stack.pop();
                continue;
            }
            let pending: Vec<String> = mandatory(&id)
                .into_iter()
                .filter(|p| !dead.contains_key(p))
                .collect();
            if !pending.is_empty() {
                stack.extend(pending);
                continue;
            }
            let (claim, source) = own[&id];
            let mut why = BTreeSet::new();
            why.extend(lifecycle_reason(claim, false));
            why.extend(lifecycle_reason(source, true));
            if get(&premises, &id).iter().any(|p| !dead[p].is_empty()) {
                why.insert("premise_invalid");
            }
            if get(&restates, &id).iter().any(|p| !dead[p].is_empty()) {
                why.insert("restates_invalid");
            }
            let alts = get(&alternatives, &id);
            if !alts.is_empty() && alts.iter().all(|p| !dead[p].is_empty()) {
                why.insert("alternatives_invalid");
            }
            dead.insert(id, why);
            stack.pop();
        }
    }
    let is_dead = |id: &str| !dead[id].is_empty();
    // Pass 2: full state; advisory and read edges only consult pass 1.
    let mut out: HashMap<String, State> = HashMap::with_capacity(claims.len());
    for (root, _) in &claims {
        let mut stack = vec![root.clone()];
        while let Some(id) = stack.last().cloned() {
            if out.contains_key(&id) {
                stack.pop();
                continue;
            }
            let pending: Vec<String> = mandatory(&id)
                .into_iter()
                .filter(|p| !out.contains_key(p))
                .collect();
            if !pending.is_empty() {
                stack.extend(pending);
                continue;
            }
            let deps_unknown = unknown.contains_key(&id);
            let s = if is_dead(&id) {
                state(Validity::Invalid, dead[&id].clone(), deps_unknown)
            } else {
                let mut why = BTreeSet::new();
                let mut required = get(&premises, &id);
                required.extend(get(&restates, &id));
                if required
                    .iter()
                    .any(|p| out[p].validity == Validity::NeedsRevalidation)
                {
                    why.insert("premise_uncertain");
                }
                let alts = get(&alternatives, &id);
                if !alts.is_empty() && !alts.iter().any(|p| out[p].validity == Validity::Valid) {
                    why.insert("alternatives_uncertain");
                }
                if get(&advisory, &id).iter().any(|p| is_dead(p)) {
                    why.insert("advisory_invalid");
                }
                if unknown
                    .get(&id)
                    .is_some_and(|r| r.iter().any(|p| is_dead(p)))
                {
                    why.insert("session_read_invalid");
                }
                let v = if why.is_empty() {
                    Validity::Valid
                } else {
                    Validity::NeedsRevalidation
                };
                state(v, why, deps_unknown)
            };
            out.insert(id, s);
            stack.pop();
        }
    }
    Ok(out)
}
/// Claims whose materialized row is missing or differs from the oracle.
pub(crate) fn mismatches(conn: &Connection) -> Result<Vec<String>> {
    let truth = oracle(conn)?;
    let mut rows: HashMap<String, State> = HashMap::new();
    for row in conn
        .prepare("SELECT claim_id,state,reasons,deps_unknown FROM claim_validity")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?
    {
        let (id, v, reasons, deps_unknown) = row?;
        rows.insert(
            id,
            State {
                validity: Validity::parse(&v)?,
                reasons,
                deps_unknown,
            },
        );
    }
    let mut bad: Vec<String> = truth
        .iter()
        .filter(|(id, s)| rows.get(*id) != Some(s))
        .map(|(id, _)| id.clone())
        .collect();
    bad.extend(rows.keys().filter(|id| !truth.contains_key(*id)).cloned());
    bad.sort();
    Ok(bad)
}
/// Replace the derived table with the oracle result (migration, restore, repair).
pub(crate) fn rebuild(conn: &Connection) -> Result<usize> {
    let truth = oracle(conn)?;
    conn.execute("DELETE FROM claim_validity", [])?;
    let mut ids: Vec<&String> = truth.keys().collect();
    ids.sort();
    for id in ids {
        write(conn, id, &truth[id])?;
    }
    Ok(truth.len())
}
