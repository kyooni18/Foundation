//! Adaptive retrieval controller.
//!
//! plan -> projection-safe filters -> exact / lexical -> semantic when useful -> sufficiency
//! check -> bounded graph expansion -> authoritative recheck and evidence packaging.
//!
//! Every candidate path runs inside the authenticated Principal's projection (`claim_access`,
//! maintained transactionally with lifecycle changes), corpus statistics are projection-local,
//! and every returned Claim is re-read through the authoritative visibility query
//! (`read_claims_only`). Indexes only propose candidate ids; they never grant visibility.
use super::{
    access, fusion, lexical,
    planner::{self, Depth, Intent, Plan},
    semantic,
    tokenizer::{self, Kind, Scripts},
};
use crate::{
    App,
    claims::ClaimView,
    domain::*,
    embedding::{self, EmbedError, Role},
    store::integrity::sha256,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Instant,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Filters {
    #[serde(default)]
    pub category: Vec<String>,
    #[serde(default)]
    pub speaker: Vec<String>,
    #[serde(default)]
    pub lang: Vec<String>,
    #[serde(default)]
    pub tkind: Vec<String>,
    #[serde(default)]
    pub source_id: Vec<String>,
    pub confirmed: Option<bool>,
    pub standing: Option<bool>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    /// Entity id or mention surface, resolved inside the caller's projection.
    pub entity: Option<String>,
}
/// Restricts which retrieval paths may run. Diagnostic and evaluation control only: it can
/// narrow the plan, never widen what the caller may read.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    #[default]
    Adaptive,
    LexicalOnly,
    SemanticOnly,
    /// Run every available path unconditionally (no adaptivity).
    FusedAll,
}
fn default_limit() -> usize {
    10
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub intent: Intent,
    #[serde(default)]
    pub depth: Depth,
    #[serde(default)]
    pub filters: Filters,
    pub as_of: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
    /// Agent/audit callers may include invalid Claims; the user projection never contains them.
    #[serde(default)]
    pub include_invalid: bool,
    #[serde(default)]
    pub strategy: Strategy,
}
impl SearchRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            intent: Intent::Auto,
            depth: Depth::Auto,
            filters: Filters::default(),
            as_of: None,
            limit: 10,
            cursor: None,
            include_invalid: false,
            strategy: Strategy::Adaptive,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Score {
    /// Rank-fusion value; an ordering aid, not a probability.
    pub fused: f64,
    pub bm25: Option<f64>,
    /// IDF-weighted share of query terms present.
    pub lexical_coverage: Option<f64>,
    pub lexical_rank: Option<usize>,
    pub cosine: Option<f32>,
    pub semantic_rank: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct Related {
    pub via: String,
    pub from: String,
    pub hops: usize,
}
#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub claim: ClaimView,
    /// exact | lexical | semantic | hybrid | related
    pub match_type: &'static str,
    /// exact | strong | moderate | weak
    pub evidence: &'static str,
    pub score: Score,
    pub why: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related: Option<Related>,
    /// Claims with identical text collapsed into this one (all visible to the caller).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub duplicates: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal: Option<TemporalNote>,
}
#[derive(Debug, Serialize)]
pub struct TemporalNote {
    pub status: &'static str,
    pub history: bool,
    pub superseded_by: Vec<String>,
    pub possibly_superseded_by: Vec<String>,
    pub provisional: bool,
}
#[derive(Debug, Serialize)]
pub struct Coverage {
    /// complete | degraded | incomplete
    pub status: &'static str,
    pub reasons: Vec<&'static str>,
    pub exhaustive: bool,
}
#[derive(Debug, Serialize, Default)]
pub struct Indexing {
    /// Claims visible to the caller.
    pub visible_claims: i64,
    pub lexical_pending: i64,
    pub embedding_pending: i64,
    pub embedding_model: Option<String>,
    /// Open queue work (owner only; aggregate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_jobs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources_not_yet_published: Option<i64>,
}
#[derive(Debug, Serialize, Default)]
pub struct Executed {
    pub exact: bool,
    pub lexical: bool,
    pub semantic: bool,
    pub graph: bool,
    pub temporal: bool,
}
#[derive(Debug, Serialize, Default)]
pub struct Timing {
    pub plan_ms: f64,
    pub lexical_ms: f64,
    pub embed_ms: f64,
    pub semantic_ms: f64,
    pub graph_ms: f64,
    pub recheck_ms: f64,
    pub total_ms: f64,
}
#[derive(Debug, Serialize)]
pub struct Continuation {
    pub next_cursor: Option<String>,
    pub returned: usize,
    /// Ranked candidates that passed the evidence gates (a lower bound when budgets truncate).
    pub candidates: usize,
    pub candidates_exact: bool,
}
#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchHit>,
    pub plan: Plan,
    pub strategy: Strategy,
    pub executed: Executed,
    pub semantic_used: bool,
    pub graph_used: bool,
    pub coverage: Coverage,
    pub indexing: Indexing,
    pub continuation: Continuation,
    pub timing: Timing,
    pub embedding_calls: u64,
}

const MAX_QUERY_BYTES: usize = 4096;
fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}
fn safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
}
fn list(
    field: &'static str,
    values: &[String],
    allowed: Option<&[&str]>,
) -> Result<Option<String>> {
    if values.is_empty() {
        return Ok(None);
    }
    if values.len() > 32 {
        return Err(invalid(field, "too many values"));
    }
    for v in values {
        let ok = match allowed {
            Some(a) => a.contains(&v.as_str()),
            None => safe_token(v),
        };
        if !ok {
            return Err(invalid(field, "unsupported value"));
        }
    }
    Ok(Some(
        values
            .iter()
            .map(|v| format!("'{v}'"))
            .collect::<Vec<_>>()
            .join(","),
    ))
}

impl App {
    /// Trusted SQL fragment over aliases `c` (claim) and `v` (claim_validity). All values are
    /// validated against closed vocabularies or strict charsets before being inlined.
    fn filter_sql(
        &self,
        conn: &rusqlite::Connection,
        p: &Principal,
        f: &Filters,
        include_invalid: bool,
    ) -> Result<String> {
        let mut parts: Vec<String> = Vec::new();
        if !(include_invalid && p.kind() != PrincipalKind::User) {
            parts.push("v.state<>'invalid'".into());
        }
        if let Some(l) = list(
            "category",
            &f.category,
            Some(&["Fact", "View", "Intent", "Rule"]),
        )? {
            parts.push(format!("c.category IN ({l})"));
        }
        if let Some(l) = list(
            "speaker",
            &f.speaker,
            Some(&["user", "agent", "external", "unknown"]),
        )? {
            parts.push(format!("c.speaker IN ({l})"));
        }
        if let Some(l) = list("lang", &f.lang, Some(&["ko", "en"]))? {
            parts.push(format!("c.lang IN ({l})"));
        }
        if let Some(l) = list(
            "tkind",
            &f.tkind,
            Some(&[
                "habit",
                "state",
                "event",
                "observation",
                "decision",
                "report_past",
            ]),
        )? {
            parts.push(format!("c.tkind IN ({l})"));
        }
        for s in &f.source_id {
            s.parse::<SourceId>()?;
        }
        if let Some(l) = list("source_id", &f.source_id, None)? {
            parts.push(format!("c.source_id IN ({l})"));
        }
        if let Some(c) = f.confirmed {
            let expr = crate::integrity::confirmed_sql("c.id");
            parts.push(if c { expr } else { format!("NOT ({expr})") });
        }
        if let Some(s) = f.standing {
            parts.push(format!("c.standing={}", i32::from(s)));
        }
        if let Some(t) = &f.created_after {
            parts.push(format!(
                "c.created_at>='{}'",
                t.parse::<Timestamp>()?.as_str()
            ));
        }
        if let Some(t) = &f.created_before {
            parts.push(format!(
                "c.created_at<'{}'",
                t.parse::<Timestamp>()?.as_str()
            ));
        }
        if let Some(e) = &f.entity {
            if e.is_empty() || e.len() > 256 {
                return Err(invalid("entity", "1..=256 bytes"));
            }
            let ids = crate::integrity::identity::claims_of_entity(conn, p, e)?;
            let json = serde_json::to_string(&ids.iter().take(50_000).collect::<Vec<_>>())
                .map_err(|_| invalid("entity", "serialization"))?;
            if !ids
                .iter()
                .all(|i| i.chars().all(|c| c.is_ascii_alphanumeric()))
            {
                return Err(Error::IntegrityFailure("claim id".into()));
            }
            parts.push(format!("c.id IN (SELECT value FROM json_each('{json}'))"));
        }
        Ok(parts.join(" AND "))
    }

    fn search_cursor(&self, p: &Principal, qhash: &str, offset: usize) -> String {
        use crate::authentication::hex;
        use hmac::Mac;
        let body = serde_json::to_vec(&("search-v1", qhash, offset)).expect("cursor");
        let sig = self.cursor_mac(p, &body).finalize().into_bytes();
        format!("{}.{}", hex(&body), hex(&sig))
    }
    fn search_offset(&self, p: &Principal, qhash: &str, cursor: Option<&str>) -> Result<usize> {
        use crate::authentication::unhex;
        use hmac::Mac;
        let Some(cursor) = cursor else { return Ok(0) };
        if cursor.len() > 1024 {
            return Err(Error::InvalidCursor);
        }
        let (body, sig) = cursor.split_once('.').ok_or(Error::InvalidCursor)?;
        let body = unhex(body).ok_or(Error::InvalidCursor)?;
        let sig = unhex(sig).ok_or(Error::InvalidCursor)?;
        self.cursor_mac(p, &body)
            .verify_slice(&sig)
            .map_err(|_| Error::InvalidCursor)?;
        let (domain, hash, offset): (String, String, usize) =
            serde_json::from_slice(&body).map_err(|_| Error::InvalidCursor)?;
        if domain != "search-v1" || hash != qhash {
            return Err(Error::InvalidCursor);
        }
        Ok(offset)
    }

    fn indexing(&self, conn: &rusqlite::Connection, p: &Principal) -> Result<Indexing> {
        let col = access::column(p);
        let key = self.policy.embedding.identity().map(|i| i.key);
        let (visible, lex, vec): (i64, i64, i64) = conn.query_row(
            &format!(
                "SELECT count(*),COALESCE(sum(l.claim_id IS NULL OR l.tokenizer<>?1),0),COALESCE(sum(?2 IS NOT NULL AND NOT EXISTS(SELECT 1 FROM claim_vec v WHERE v.claim_id=a.claim_id AND v.model_key=?2)),0) FROM claim_access a LEFT JOIN claim_lex l ON l.claim_id=a.claim_id WHERE a.{col}=1"
            ),
            params![tokenizer::VERSION, key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut out = Indexing {
            visible_claims: visible,
            lexical_pending: lex,
            embedding_pending: vec,
            embedding_model: key.clone(),
            ..Default::default()
        };
        if p.kind() == PrincipalKind::User {
            out.open_jobs = Some(conn.query_row(
                "SELECT count(*) FROM job WHERE state IN ('pending','leased','deferred')",
                [],
                |r| r.get(0),
            )?);
            out.sources_not_yet_published = Some(conn.query_row("SELECT count(*) FROM source s WHERE NOT EXISTS(SELECT 1 FROM extraction_run r WHERE r.source_id=s.id) AND s.recorder='user'", [], |r| r.get(0))?);
        }
        Ok(out)
    }

    fn query_vector(
        &self,
        text: &str,
        report: &mut Timing,
        calls: &mut u64,
    ) -> std::result::Result<Vec<f32>, EmbedError> {
        let provider = self.embedder().ok_or(EmbedError::Unavailable)?;
        let key = provider.identity().key.clone();
        if let Some(v) = self
            .query_cache
            .lock()
            .expect("query cache")
            .get(&key, text)
        {
            return Ok(v);
        }
        let t = Instant::now();
        let before = provider.calls();
        let v = embedding::embed(provider.as_ref(), Role::Query, &[text.to_owned()])?.remove(0);
        *calls += provider.calls().saturating_sub(before).max(1);
        report.embed_ms += ms(t);
        self.query_cache
            .lock()
            .expect("query cache")
            .put(&key, text, v.clone());
        Ok(v)
    }

    pub fn search(&self, p: &Principal, req: SearchRequest) -> Result<SearchResponse> {
        let started = Instant::now();
        let query = req.query.trim().to_owned();
        if query.is_empty() || query.len() > MAX_QUERY_BYTES {
            return Err(invalid("query", "1..=4096 bytes"));
        }
        if !(1..=100).contains(&req.limit) {
            return Err(invalid("limit", "must be 1..=100"));
        }
        let as_of = req
            .as_of
            .as_deref()
            .map(str::parse::<Timestamp>)
            .transpose()?;
        let qhash = sha256(
            serde_json::to_string(&(
                &query,
                req.intent,
                req.depth,
                &req.filters,
                &req.as_of,
                req.include_invalid,
                req.strategy,
            ))
            .map_err(|_| invalid("query", "serialization"))?
            .as_bytes(),
        );
        let offset = self.search_offset(p, &qhash, req.cursor.as_deref())?;
        let t_plan = Instant::now();
        let filtered = req.filters.category.len()
            + req.filters.speaker.len()
            + req.filters.lang.len()
            + req.filters.source_id.len()
            > 0;
        let plan = planner::plan(
            &query,
            req.intent,
            req.depth,
            as_of.is_some(),
            filtered,
            req.limit,
        );
        let mut timing = Timing {
            plan_ms: ms(t_plan),
            ..Default::default()
        };
        let params = self.policy.retrieval;
        let wants = |path: &str| plan.paths.iter().any(|p| p.path == path);
        let optional = |path: &str| {
            plan.paths
                .iter()
                .any(|p| p.path == path && p.when == "if_insufficient")
        };
        let deadline = std::time::Duration::from_millis(plan.budgets.deadline_ms);
        let mut reasons: Vec<&'static str> = Vec::new();
        let mut executed = Executed::default();
        let mut calls = 0u64;
        let need = offset + req.limit + 1;
        let fetch = (need * 4).max(40);

        // ---- Phase A: exact + lexical inside one read snapshot.
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        let filter_sql = self.filter_sql(&tx, p, &req.filters, req.include_invalid)?;
        let indexing = self.indexing(&tx, p)?;
        let qscripts = Scripts::of(&query);
        let mut evidence: HashMap<String, fusion::Evidence> = HashMap::new();
        let mut texts: HashMap<String, String> = HashMap::new();
        let mut lex_list: Vec<String> = Vec::new();
        let mut exact_list: Vec<String> = Vec::new();
        let mut lex_truncated = false;
        let mut population = 0;
        let run_lexical = req.strategy != Strategy::SemanticOnly;
        let t_lex = Instant::now();
        if run_lexical
            && (wants("lexical")
                || wants("exact")
                || req.strategy == Strategy::LexicalOnly
                || req.strategy == Strategy::FusedAll)
        {
            executed.lexical = true;
            let mut terms = tokenizer::query_terms(&query);
            let informative = terms
                .iter()
                .any(|t| !(t.kind == Kind::Word && tokenizer::is_function_word(&t.text)));
            if informative {
                terms.retain(|t| !(t.kind == Kind::Word && tokenizer::is_function_word(&t.text)));
            }
            let out =
                lexical::search(&tx, p, &terms, &filter_sql, plan.budgets.lexical_candidates)?;
            population = out.population;
            lex_truncated |= out.truncated;
            // Exact tier: quoted phrases and identifiers must appear verbatim (after folding).
            let phrases: Vec<String> = plan
                .signals
                .quoted
                .iter()
                .map(|q| fusion::normal_form(q))
                .collect();
            let idents: Vec<&String> = plan.signals.identifiers.iter().collect();
            let mut exact_ids: HashSet<String> = HashSet::new();
            if !(phrases.is_empty() && idents.is_empty()) {
                executed.exact = true;
                let mut exact_terms = Vec::new();
                for ph in plan
                    .signals
                    .quoted
                    .iter()
                    .chain(plan.signals.identifiers.iter())
                {
                    exact_terms.extend(tokenizer::query_terms(ph));
                }
                let mut seen = HashSet::new();
                exact_terms.retain(|t| seen.insert(t.text.clone()));
                let ex = lexical::search(
                    &tx,
                    p,
                    &exact_terms,
                    &filter_sql,
                    plan.budgets.lexical_candidates,
                )?;
                lex_truncated |= ex.truncated;
                for h in &ex.hits {
                    let norm = fusion::normal_form(&h.text);
                    let doc_terms: HashSet<String> = tokenizer::terms(&h.text)
                        .into_iter()
                        .map(|t| t.text)
                        .collect();
                    let phrase_ok = phrases.iter().all(|ph| norm.contains(ph.as_str()));
                    let ident_ok = idents.iter().all(|i| doc_terms.contains(i.as_str()));
                    if (!phrases.is_empty() || !idents.is_empty()) && phrase_ok && ident_ok {
                        exact_ids.insert(h.claim_id.clone());
                        exact_list.push(h.claim_id.clone());
                        texts.insert(h.claim_id.clone(), h.text.clone());
                        let e = evidence.entry(h.claim_id.clone()).or_default();
                        e.exact = true;
                        e.bm25 = Some(h.bm25);
                    }
                }
            }
            let mut rank = 0;
            for h in &out.hits {
                // Gate: shared script and enough of the query's informative terms.
                let gated = qscripts.overlaps(Scripts::of(&h.text))
                    && h.coverage >= params.lexical_min_coverage;
                if !gated && !exact_ids.contains(&h.claim_id) {
                    continue;
                }
                texts.insert(h.claim_id.clone(), h.text.clone());
                let e = evidence.entry(h.claim_id.clone()).or_default();
                e.bm25 = Some(h.bm25);
                e.coverage = Some(h.coverage);
                e.lexical_rank = Some(rank);
                rank += 1;
                lex_list.push(h.claim_id.clone());
                if lex_list.len() >= fetch {
                    break;
                }
            }
        }
        timing.lexical_ms = ms(t_lex);
        let strong = |evidence: &HashMap<String, fusion::Evidence>| {
            evidence
                .values()
                .filter(|e| {
                    fusion::strength(e, &params) != "weak"
                        && (e.exact
                            || e.coverage
                                .is_some_and(|c| c >= params.lexical_strong_coverage)
                            || (e.coverage.is_some() && e.cosine.is_some()))
                })
                .count()
        };
        // ---- Sufficiency decides whether optional semantic work is needed.
        let semantic_available = self.embedder().is_some();
        // Enough strong evidence: one verbatim hit settles an exact lookup; otherwise a few.
        let needed = if plan.intent == Intent::Exact {
            1
        } else {
            req.limit.min(3)
        };
        let mut run_semantic = match req.strategy {
            Strategy::LexicalOnly => false,
            Strategy::SemanticOnly | Strategy::FusedAll => true,
            Strategy::Adaptive => {
                wants("semantic")
                    && (!optional("semantic")
                        || strong(&evidence) < needed
                        || plan.depth == Depth::Exhaustive)
            }
        };
        if run_semantic && !semantic_available {
            run_semantic = false;
            reasons.push("semantic_unavailable");
        }
        let mut sem_list: Vec<String> = Vec::new();
        let mut sem_scanned = 0;
        drop(tx);
        let mut qvec: Option<Vec<f32>> = None;
        if run_semantic && started.elapsed() < deadline {
            match self.query_vector(&query, &mut timing, &mut calls) {
                Ok(v) => qvec = Some(v),
                Err(e) => {
                    reasons.push(if e == EmbedError::Unavailable {
                        "semantic_unavailable"
                    } else {
                        "semantic_failed"
                    });
                }
            }
        } else if run_semantic {
            reasons.push("deadline_exceeded");
        }
        // ---- Phase B: semantic scan, graph candidates, temporal inputs in a fresh snapshot.
        let tx = conn.transaction()?;
        if let (Some(v), Some(id)) = (&qvec, self.policy.embedding.identity()) {
            let t = Instant::now();
            let mut cache = self.vectors.lock().expect("vector cache");
            cache.refresh(&tx, &id.key, id.dimensions)?;
            let out = semantic::search(
                &tx,
                &cache,
                p,
                v,
                &filter_sql,
                fetch.min(plan.budgets.semantic_candidates.max(req.limit * 4)),
                params.semantic_min_cosine,
            )?;
            sem_scanned = out.scanned;
            executed.semantic = true;
            for (rank, (cid, cos)) in out.hits.iter().enumerate() {
                let e = evidence.entry(cid.clone()).or_default();
                e.cosine = Some(*cos);
                e.semantic_rank = Some(rank);
                sem_list.push(cid.clone());
            }
            timing.semantic_ms = ms(t);
        }
        // ---- Fusion.
        let fused = fusion::rrf(&[&lex_list, &sem_list], params.rrf_k);
        for (id, score) in &fused {
            evidence.entry(id.clone()).or_default().fused = *score;
        }
        let mut order: Vec<String> = evidence.keys().cloned().collect();
        // Exact tier first, then fused score, then lexical strength, then id (deterministic).
        order.sort_by(|a, b| {
            let (ea, eb) = (&evidence[a], &evidence[b]);
            eb.exact
                .cmp(&ea.exact)
                .then_with(|| eb.fused.total_cmp(&ea.fused))
                .then_with(|| eb.bm25.unwrap_or(0.0).total_cmp(&ea.bm25.unwrap_or(0.0)))
                .then_with(|| a.cmp(b))
        });
        let candidates = order.len();
        // Texts and Sources for duplicate suppression / per-source caps (semantic-only hits too).
        let missing: Vec<&String> = order.iter().filter(|i| !texts.contains_key(*i)).collect();
        let mut source_of: HashMap<String, String> = HashMap::new();
        {
            let json =
                serde_json::to_string(&order).map_err(|_| invalid("ids", "serialization"))?;
            let rows: Vec<(String, String, String)> = tx
                .prepare_cached("SELECT id,source_id,text FROM claim WHERE id IN (SELECT value FROM json_each(?1))")?
                .query_map([json], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<std::result::Result<_, _>>()?;
            let need_text: HashSet<&String> = missing.into_iter().collect();
            for (id, source, text) in rows {
                if need_text.contains(&id) {
                    texts.insert(id.clone(), text);
                }
                source_of.insert(id, source);
            }
        }
        let (order, duplicates) = fusion::suppress_duplicates(&order, &texts);
        let (primary, suppressed) =
            fusion::cap_per_source(order, &source_of, plan.budgets.per_source);
        let _ = suppressed;
        // ---- Graph expansion from the best seeds (bounded hops and nodes).
        let mut related: Vec<(String, Related)> = Vec::new();
        let run_graph = wants("graph")
            && req.strategy != Strategy::LexicalOnly
            && req.strategy != Strategy::SemanticOnly
            && !primary.is_empty()
            && (!optional("graph")
                || strong(&evidence) < plan.sufficient_results
                || plan.depth == Depth::Exhaustive);
        if run_graph {
            let t = Instant::now();
            executed.graph = true;
            related = self.expand_graph(
                &tx,
                p,
                &filter_sql,
                &primary[..primary.len().min(5)],
                &plan,
                &primary,
            )?;
            timing.graph_ms = ms(t);
        }
        drop(tx);
        drop(conn);
        // ---- Authoritative recheck + packaging, in rank order, until the page is satisfied.
        let temporal = wants("temporal") || as_of.is_some();
        let mut final_hits: Vec<SearchHit> = Vec::new();
        let t_recheck = Instant::now();
        let mut queue: Vec<(String, Option<Related>)> =
            primary.iter().map(|i| (i.clone(), None)).collect();
        queue.extend(related.into_iter().map(|(i, r)| (i, Some(r))));
        let chunk = (req.limit * 2).max(32);
        let mut cursor = 0;
        let mut dropped_not_yet = 0usize;
        while final_hits.len() < need && cursor < queue.len() {
            let slice = &queue[cursor..(cursor + chunk).min(queue.len())];
            cursor += slice.len();
            let ids: Vec<String> = slice.iter().map(|(i, _)| i.clone()).collect();
            let mut views: HashMap<String, ClaimView> = self
                .read_claims_only(p, false, false, Some(&ids))?
                .into_iter()
                .filter(|c| {
                    req.include_invalid && p.kind() != PrincipalKind::User
                        || c.validity != crate::integrity::Validity::Invalid
                })
                .map(|c| (c.id.clone(), c))
                .collect();
            let mut statuses: HashMap<String, crate::integrity::temporal::Status> = HashMap::new();
            if temporal && !views.is_empty() {
                executed.temporal = true;
                let a = as_of.clone().unwrap_or_else(Timestamp::now);
                statuses = self.temporal_statuses(p, &views, &a, as_of.is_none())?;
            }
            for (id, rel) in slice {
                let Some(mut claim) = views.remove(id) else {
                    continue;
                };
                let temporal_note =
                    statuses
                        .remove(id)
                        .map(
                            |(status, superseded_by, possibly, provisional, _)| TemporalNote {
                                history: status != "applies",
                                status,
                                superseded_by,
                                possibly_superseded_by: possibly,
                                provisional,
                            },
                        );
                if as_of.is_some()
                    && temporal_note
                        .as_ref()
                        .is_some_and(|t| t.status == "not_yet_stated")
                {
                    dropped_not_yet += 1;
                    continue;
                }
                let ev = evidence.get(id).cloned().unwrap_or_default();
                let match_type = if rel.is_some() {
                    "related"
                } else if ev.exact {
                    "exact"
                } else if ev.coverage.is_some() && ev.cosine.is_some() {
                    "hybrid"
                } else if ev.cosine.is_some() {
                    "semantic"
                } else {
                    "lexical"
                };
                let mut why = Vec::new();
                if ev.exact {
                    why.push("contains the quoted phrase or identifier verbatim".to_string());
                }
                if let Some(c) = ev.coverage {
                    why.push(format!(
                        "lexical: matches {:.0}% of the informative query terms",
                        c * 100.0
                    ));
                }
                if let Some(c) = ev.cosine {
                    why.push(format!(
                        "semantic: cosine {c:.2} with the query (similarity, not probability)"
                    ));
                }
                if let Some(r) = rel {
                    why.push(format!("related by {} to a top result", r.via));
                }
                claim.mentions = None;
                final_hits.push(SearchHit {
                    match_type,
                    evidence: if rel.is_some() {
                        "weak"
                    } else {
                        fusion::strength(&ev, &params)
                    },
                    score: Score {
                        fused: ev.fused,
                        bm25: ev.bm25,
                        lexical_coverage: ev.coverage,
                        lexical_rank: ev.lexical_rank,
                        cosine: ev.cosine,
                        semantic_rank: ev.semantic_rank,
                    },
                    duplicates: duplicates.get(id).cloned().unwrap_or_default(),
                    why,
                    related: rel.as_ref().map(|r| Related {
                        via: r.via.clone(),
                        from: r.from.clone(),
                        hops: r.hops,
                    }),
                    temporal: temporal_note,
                    claim,
                });
            }
        }
        timing.recheck_ms = ms(t_recheck);
        let more = final_hits.len() > offset + req.limit
            || (cursor < queue.len() && final_hits.len() >= need);
        let page: Vec<SearchHit> = final_hits
            .into_iter()
            .skip(offset)
            .take(req.limit)
            .collect();
        let ids: Vec<String> = page.iter().map(|h| h.claim.id.clone()).collect();
        self.note_reads(p, &ids)?;
        let next_cursor = more.then(|| self.search_cursor(p, &qhash, offset + page.len()));
        // ---- Coverage: never claim completeness the indexes cannot back.
        if indexing.lexical_pending > 0 && executed.lexical {
            reasons.push("lexical_index_incomplete");
        }
        if indexing.embedding_pending > 0 && (executed.semantic || run_semantic) {
            reasons.push("embedding_index_incomplete");
        }
        if lex_truncated {
            reasons.push("candidate_budget_exhausted");
        }
        if executed.semantic {
            reasons.push("semantic_similarity_bounded");
        }
        if dropped_not_yet > 0 {
            reasons.push("excluded_not_yet_stated");
        }
        let _ = (population, sem_scanned);
        reasons.sort_unstable();
        reasons.dedup();
        let status = if reasons.is_empty() {
            "complete"
        } else if reasons.iter().any(|r| {
            matches!(
                *r,
                "semantic_unavailable"
                    | "semantic_failed"
                    | "lexical_index_incomplete"
                    | "embedding_index_incomplete"
                    | "deadline_exceeded"
                    | "candidate_budget_exhausted"
            )
        }) {
            "degraded"
        } else {
            "incomplete"
        };
        timing.total_ms = ms(started);
        Ok(SearchResponse {
            continuation: Continuation {
                next_cursor,
                returned: page.len(),
                candidates,
                candidates_exact: !lex_truncated && !executed.semantic,
            },
            results: page,
            semantic_used: executed.semantic,
            graph_used: executed.graph,
            coverage: Coverage {
                exhaustive: status == "complete",
                status,
                reasons,
            },
            indexing,
            strategy: req.strategy,
            executed,
            plan,
            timing,
            embedding_calls: calls,
        })
    }

    /// One batch of temporal statuses for already-authorized Claims (relation neighbours are
    /// pulled in so supersession by Claims outside the result set is still reported).
    fn temporal_statuses(
        &self,
        p: &Principal,
        views: &HashMap<String, ClaimView>,
        as_of: &Timestamp,
        present: bool,
    ) -> Result<HashMap<String, crate::integrity::temporal::Status>> {
        let mut ids: Vec<String> = views.keys().cloned().collect();
        {
            let conn = self.store.reader()?;
            let json = serde_json::to_string(&ids).map_err(|_| invalid("ids", "serialization"))?;
            let extra: Vec<String> = conn
                .prepare("SELECT subject FROM relation WHERE object IN (SELECT value FROM json_each(?1)) UNION SELECT object FROM relation WHERE subject IN (SELECT value FROM json_each(?1))")?
                .query_map([json], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            ids.extend(extra);
        }
        ids.sort();
        ids.dedup();
        let universe = self.read_claims_only(p, false, false, Some(&ids))?;
        let a = time::OffsetDateTime::parse(
            as_of.as_str(),
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| invalid("as_of", "timestamp"))?;
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        let mut all =
            crate::integrity::temporal::evaluate(&tx, p, &universe, a, present, &self.policy)?;
        all.retain(|k, _| views.contains_key(k));
        Ok(all)
    }

    /// Bounded traversal of dependencies, relations, citations and same-Source provenance from
    /// seed Claims. Every neighbour must be inside the caller's projection and the filters.
    fn expand_graph(
        &self,
        conn: &rusqlite::Connection,
        p: &Principal,
        filter_sql: &str,
        seeds: &[String],
        plan: &Plan,
        exclude: &[String],
    ) -> Result<Vec<(String, Related)>> {
        let col = access::column(p);
        let actors = if p.kind() == PrincipalKind::User {
            "'user'"
        } else {
            "'user','agent'"
        };
        let declared = if p.kind() == PrincipalKind::User {
            "'system','user'"
        } else {
            "'system','user','agent','import'"
        };
        let mut out: Vec<(String, Related)> = Vec::new();
        let mut seen: HashSet<String> = exclude.iter().cloned().collect();
        let mut frontier: Vec<(String, String)> =
            seeds.iter().map(|s| (s.clone(), s.clone())).collect();
        let filter = if filter_sql.is_empty() {
            String::new()
        } else {
            format!("AND {filter_sql}")
        };
        for hop in 1..=plan.budgets.graph_hops {
            if frontier.is_empty() || out.len() >= plan.budgets.graph_nodes {
                break;
            }
            let json =
                serde_json::to_string(&frontier.iter().map(|(id, _)| id).collect::<Vec<_>>())
                    .map_err(|_| invalid("ids", "serialization"))?;
            let origin: HashMap<&str, &str> = frontier
                .iter()
                .map(|(id, root)| (id.as_str(), root.as_str()))
                .collect();
            let sql = format!(
                "SELECT e.other,e.kind,e.seed FROM (
                   SELECT d.parent AS other,'depends_on:'||d.kind AS kind,d.child AS seed FROM claim_dependency d WHERE d.child IN (SELECT value FROM json_each(?1)) AND d.declared_by IN ({declared}) AND {dl}='restore'
                   UNION ALL SELECT d.child,'depended_on_by:'||d.kind,d.parent FROM claim_dependency d WHERE d.parent IN (SELECT value FROM json_each(?1)) AND d.declared_by IN ({declared}) AND {dl}='restore'
                   UNION ALL SELECT r.object,'relation:'||r.kind,r.subject FROM relation r WHERE r.subject IN (SELECT value FROM json_each(?1)) AND r.actor IN ({actors}) AND {rl}='restore'
                   UNION ALL SELECT r.subject,'relation_of:'||r.kind,r.object FROM relation r WHERE r.object IN (SELECT value FROM json_each(?1)) AND r.actor IN ({actors}) AND {rl}='restore'
                   UNION ALL SELECT n.id,'revision','' || c0.id FROM claim c0 JOIN claim n ON n.revision_of=c0.id WHERE c0.id IN (SELECT value FROM json_each(?1))
                   UNION ALL SELECT c0.revision_of,'revision_of',c0.id FROM claim c0 WHERE c0.revision_of IS NOT NULL AND c0.id IN (SELECT value FROM json_each(?1))
                   UNION ALL SELECT s2.id,'same_source',c0.id FROM claim c0 JOIN claim s2 ON s2.source_id=c0.source_id AND s2.id<>c0.id WHERE c0.id IN (SELECT value FROM json_each(?1))
                   UNION ALL SELECT c2.id,'cited_source',c0.id FROM claim c0 JOIN source_citation sc ON sc.child_source=c0.source_id AND sc.actor IN ({cit}) AND {cl}='restore' JOIN claim c2 ON c2.source_id=sc.parent_source WHERE c0.id IN (SELECT value FROM json_each(?1))
                 ) e JOIN claim_access a ON a.claim_id=e.other JOIN claim c ON c.id=e.other JOIN claim_validity v ON v.claim_id=c.id
                 WHERE a.{col}=1 {filter} ORDER BY e.other,e.kind LIMIT ?2",
                dl = crate::integrity::lifecycle_sql("dependency", "d.id"),
                rl = crate::integrity::lifecycle_sql("relation", "r.id"),
                cl = crate::integrity::lifecycle_sql("citation", "sc.id"),
                cit = if p.kind() == PrincipalKind::User { "'user'" } else { "'user','agent','import'" },
            );
            let rows: Vec<(String, String, String)> = conn
                .prepare(&sql)?
                .query_map(params![json, (plan.budgets.graph_nodes * 4) as i64], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<std::result::Result<_, _>>()?;
            let mut next = Vec::new();
            for (other, kind, seed) in rows {
                if out.len() >= plan.budgets.graph_nodes {
                    break;
                }
                if !seen.insert(other.clone()) {
                    continue;
                }
                let root = origin
                    .get(seed.as_str())
                    .copied()
                    .unwrap_or(seed.as_str())
                    .to_owned();
                out.push((
                    other.clone(),
                    Related {
                        via: kind,
                        from: root.clone(),
                        hops: hop,
                    },
                ));
                next.push((other, root));
            }
            frontier = next;
        }
        Ok(out)
    }
}

/// Group counts for reports and diagnostics.
pub fn match_types(hits: &[SearchHit]) -> BTreeMap<&'static str, usize> {
    let mut m = BTreeMap::new();
    for h in hits {
        *m.entry(h.match_type).or_default() += 1;
    }
    m
}
