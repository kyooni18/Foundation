//! Retrieval evaluation: builds a database from a dataset JSON (evaluation/retrieval), indexes it
//! through the real queue workers with the configured embedding provider, then compares
//! lexical-only, semantic-only, gated fusion and adaptive retrieval.
//!   retrieval_eval <dataset.json> [--embedding local|off] [--splits dev,holdout] [--out file]
//!                  [--sweep] [--lex-cov X] [--sem-cos X] [--db path]
#![allow(clippy::type_complexity)]
#[path = "../tests/support/synthetic.rs"]
mod synthetic;
use prime_radiant::{
    App, Policy,
    domain::*,
    embedding::EmbeddingConfig,
    retrieval::{
        fusion::Params,
        search::{SearchRequest, Strategy},
    },
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use synthetic::BulkClaim;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}
fn principal(name: &str) -> Principal {
    Principal::for_local_cli(
        if name == "agent" {
            PrincipalKind::Agent
        } else {
            PrincipalKind::User
        },
        None,
        None,
    )
}
fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        0.0
    } else {
        v[((v.len() as f64 * q).ceil() as usize)
            .saturating_sub(1)
            .min(v.len() - 1)]
    }
}
fn rss_mb() -> f64 {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let b = u.ru_maxrss as f64;
    if cfg!(target_os = "macos") {
        b / 1e6
    } else {
        b / 1e3
    }
}

fn build(path: &std::path::Path, data: &Value) -> HashMap<String, String> {
    drop(
        App::open_with_policy(
            path,
            Policy {
                embedding: EmbeddingConfig::Off,
                ..Default::default()
            },
        )
        .unwrap(),
    );
    let conn = synthetic::connect(path);
    conn.pragma_update(None, "synchronous", "OFF").ok();
    let mut ids = HashMap::new();
    for r in data["records"].as_array().unwrap() {
        let date = format!(
            "{}T00:00:00.000Z",
            r["date"].as_str().unwrap_or("2026-01-01")
        );
        let (_, claims) = synthetic::bulk_source_at(
            &conn,
            r["recorder"].as_str().unwrap(),
            &date,
            Some(&date),
            &[BulkClaim {
                text: r["text"].as_str().unwrap().into(),
                lang: if r["lang"] == "ko" { "ko" } else { "en" },
            }],
        );
        let id = claims[0].clone();
        let status = |event: &str| {
            conn.execute("INSERT INTO status_event(target_kind,target_id,event,actor,created_at) VALUES('claim',?1,?2,'user',?3)", rusqlite::params![id, event, date]).unwrap();
        };
        match r["lifecycle"].as_str().unwrap() {
            "forgotten" => status("forget"),
            "retracted" => status("retract"),
            _ => (),
        }
        ids.insert(r["key"].as_str().unwrap().to_owned(), id);
    }
    for rel in data["relations"].as_array().unwrap() {
        conn.execute("INSERT INTO relation(id,kind,subject,object,actor,created_at) VALUES(?1,?2,?3,?4,'user','2026-01-01T00:00:00.000Z')",
            rusqlite::params![ulid::Ulid::new().to_string(), rel["kind"].as_str().unwrap(), ids[rel["subject"].as_str().unwrap()], ids[rel["object"].as_str().unwrap()]]).unwrap();
    }
    for d in data["deps"].as_array().unwrap() {
        synthetic::dependency(
            &conn,
            &ids[d["child"].as_str().unwrap()],
            &ids[d["parent"].as_str().unwrap()],
            d["kind"].as_str().unwrap(),
        );
    }
    ids
}

#[derive(Default)]
struct Acc {
    n: usize,
    r1: f64,
    r5: f64,
    r10: f64,
    mrr: f64,
    ndcg: f64,
    prec5: f64,
    nans: usize,
    fp: usize,
    viol: usize,
    intent_ok: usize,
    intent_n: usize,
    depth_ok: usize,
    depth_n: usize,
    covn: usize,
    cov_full: usize,
    false_complete: usize,
    lat: Vec<f64>,
    emb: u64,
    sem: usize,
    graph: usize,
}
impl Acc {
    fn json(&mut self) -> Value {
        let n = self.n.max(1) as f64;
        let hits = (self.n - self.nans).max(1) as f64;
        json!({"queries": self.n, "with_gold": self.n - self.nans,
            "recall@1": self.r1 / hits, "recall@5": self.r5 / hits, "recall@10": self.r10 / hits, "mrr": self.mrr / hits, "ndcg@10": self.ndcg / hits, "precision@5": self.prec5 / hits,
            "no_answer_queries": self.nans, "no_answer_false_positive_rate": if self.nans > 0 { self.fp as f64 / self.nans as f64 } else { 0.0 },
            "must_not_violations": self.viol,
            "intent_accuracy": if self.intent_n > 0 { self.intent_ok as f64 / self.intent_n as f64 } else { f64::NAN },
            "depth_accuracy": if self.depth_n > 0 { self.depth_ok as f64 / self.depth_n as f64 } else { f64::NAN },
            "exhaustive_full_recall": if self.covn > 0 { self.cov_full as f64 / self.covn as f64 } else { f64::NAN },
            "false_completion_rate": self.false_complete as f64 / n,
            "latency_ms": {"p50": pct(&mut self.lat.clone(), 0.5), "p95": pct(&mut self.lat.clone(), 0.95), "p99": pct(&mut self.lat.clone(), 0.99)},
            "embedding_calls_per_query": self.emb as f64 / n, "semantic_used_rate": self.sem as f64 / n, "graph_used_rate": self.graph as f64 / n})
    }
}

fn run(
    app: &App,
    data: &Value,
    ids: &HashMap<String, String>,
    splits: &[String],
    strategy: Strategy,
) -> (Value, BTreeMap<String, Value>) {
    let rev: HashMap<&String, &String> = ids.iter().map(|(k, v)| (v, k)).collect();
    let mut total = Acc::default();
    let mut by_cat: BTreeMap<String, Acc> = BTreeMap::new();
    for q in data["queries"].as_array().unwrap() {
        if !splits.iter().any(|s| q["split"] == s.as_str()) {
            continue;
        }
        let gold: Vec<&str> = q["gold"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g.as_str().unwrap())
            .collect();
        let must_not: Vec<&str> = q["must_not"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g.as_str().unwrap())
            .collect();
        let cat = q["cat"].as_str().unwrap().to_owned();
        let comprehensive = cat == "comprehensive";
        let mut req = SearchRequest::new(q["text"].as_str().unwrap());
        req.strategy = strategy;
        req.limit = if comprehensive { 50 } else { 10 };
        req.as_of = q["as_of"].as_str().map(str::to_owned);
        let t = std::time::Instant::now();
        let resp = app
            .search(&principal(q["principal"].as_str().unwrap()), req)
            .unwrap();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let ranked: Vec<&str> = resp
            .results
            .iter()
            .map(|h| rev.get(&h.claim.id).map_or("?", |k| k.as_str()))
            .collect();
        for acc in [&mut total, by_cat.entry(cat.clone()).or_default()] {
            acc.n += 1;
            acc.lat.push(ms);
            acc.emb += resp.embedding_calls;
            acc.sem += usize::from(resp.semantic_used);
            acc.graph += usize::from(resp.graph_used);
            acc.viol += ranked.iter().filter(|r| must_not.contains(r)).count();
            if let Some(label) = q["intent"].as_str().filter(|i| *i != "auto") {
                acc.intent_n += 1;
                acc.intent_ok +=
                    usize::from(serde_json::to_value(resp.plan.intent).unwrap() == label);
            }
            if let Some(label) = q["depth"].as_str().filter(|i| *i != "auto") {
                acc.depth_n += 1;
                acc.depth_ok +=
                    usize::from(serde_json::to_value(resp.plan.depth).unwrap() == label);
            }
            if gold.is_empty() {
                acc.nans += 1;
                acc.fp += usize::from(!ranked.is_empty());
            } else {
                let rank = |k: usize| {
                    gold.iter()
                        .filter(|g| ranked.iter().take(k).any(|r| r == *g))
                        .count() as f64
                        / gold.len() as f64
                };
                acc.r1 += rank(1);
                acc.r5 += rank(5);
                acc.r10 += rank(10);
                acc.mrr += ranked
                    .iter()
                    .position(|r| gold.contains(r))
                    .map_or(0.0, |p| 1.0 / (p as f64 + 1.0));
                acc.prec5 +=
                    ranked.iter().take(5).filter(|r| gold.contains(r)).count() as f64 / 5.0;
                let dcg: f64 = ranked
                    .iter()
                    .take(10)
                    .enumerate()
                    .filter(|(_, r)| gold.contains(r))
                    .map(|(i, _)| 1.0 / (i as f64 + 2.0).log2())
                    .sum();
                let ideal: f64 = (0..gold.len().min(10))
                    .map(|i| 1.0 / (i as f64 + 2.0).log2())
                    .sum();
                acc.ndcg += dcg / ideal;
                if comprehensive {
                    acc.covn += 1;
                    let full = gold.iter().all(|g| ranked.contains(g));
                    acc.cov_full += usize::from(full);
                    acc.false_complete += usize::from(resp.coverage.exhaustive && !full);
                }
            }
            if resp.coverage.exhaustive
                && !gold.iter().all(|g| ranked.contains(g))
                && !comprehensive
            {
                acc.false_complete += 1;
            }
        }
    }
    (
        total.json(),
        by_cat.into_iter().map(|(k, mut a)| (k, a.json())).collect(),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data: Value = serde_json::from_str(&std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let splits: Vec<String> = arg(&args, "--splits")
        .unwrap_or_else(|| "dev,holdout".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    let embedding = EmbeddingConfig::parse(
        &arg(&args, "--embedding").unwrap_or_else(|| "local".into()),
        None,
        None,
    )
    .expect("embedding");
    let dir = tempfile::tempdir().unwrap();
    let path = arg(&args, "--db")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.path().join("eval.db"));
    let t0 = std::time::Instant::now();
    let ids = build(&path, &data);
    let build_s = t0.elapsed().as_secs_f64();
    let mut params = Params::default();
    if let Some(v) = arg(&args, "--lex-cov") {
        params.lexical_min_coverage = v.parse().unwrap();
    }
    if let Some(v) = arg(&args, "--sem-cos") {
        params.semantic_min_cosine = v.parse().unwrap();
    }
    let open = |p: Params| {
        App::open_with_policy(
            &path,
            Policy {
                embedding: embedding.clone(),
                retrieval: p,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let mut app = open(params);
    let user = principal("user");
    app.rebuild_derived(&user).unwrap();
    app.reconcile_indexing().unwrap();
    let t = std::time::Instant::now();
    let mut embed_s = 0.0;
    loop {
        let l = app.process_lexical_once("eval").unwrap();
        let te = std::time::Instant::now();
        let e = match app.embedder() {
            Some(p) => app.process_embeddings_once(p.as_ref(), "eval").unwrap(),
            None => Default::default(),
        };
        embed_s += te.elapsed().as_secs_f64();
        if e.deferred + e.failed > 0 {
            eprintln!("embedding unavailable: {e:?}");
            break;
        }
        if l.jobs + l.cancelled + e.jobs + e.cancelled == 0 {
            break;
        }
    }
    let index_s = t.elapsed().as_secs_f64();
    if args.iter().any(|a| a == "--sweep") {
        let mut rows = vec![];
        for cov in [0.0, 0.2, 0.3, 0.4, 0.5, 0.6] {
            for cos in [0.0, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5] {
                drop(app);
                app = open(Params {
                    lexical_min_coverage: cov,
                    semantic_min_cosine: cos,
                    ..params
                });
                let (mut t, _) = run(&app, &data, &ids, &["dev".to_owned()], Strategy::FusedAll);
                t["lexical_min_coverage"] = json!(cov);
                t["semantic_min_cosine"] = json!(cos);
                rows.push(t);
            }
        }
        let out = json!({"sweep_on": "dev", "rows": rows});
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return;
    }
    // Cold query (model already resident from indexing in this process; query cache empty).
    let mut report = json!({"records": data["records"].as_array().unwrap().len(), "params": params, "embedding": app.policy_embedding_identity().map(|i| i.key),
        "build_seconds": build_s, "index_seconds": index_s, "embed_seconds": embed_s, "splits": splits});
    let mut strategies = BTreeMap::new();
    for (name, s) in [
        ("lexical_only", Strategy::LexicalOnly),
        ("semantic_only", Strategy::SemanticOnly),
        ("fused_all", Strategy::FusedAll),
        ("adaptive", Strategy::Adaptive),
    ] {
        app.clear_query_cache();
        let (total, cats) = run(&app, &data, &ids, &splits, s);
        strategies.insert(name, json!({"total": total, "by_category": cats}));
    }
    // Warm repeat of the adaptive run (query-embedding cache populated).
    let (warm, _) = run(&app, &data, &ids, &splits, Strategy::Adaptive);
    report["strategies"] = json!(strategies);
    report["adaptive_warm_latency_ms"] = warm["latency_ms"].clone();
    report["peak_rss_mb"] = json!(rss_mb());
    report["db_bytes"] = json!(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
    let text = serde_json::to_string_pretty(&report).unwrap();
    if let Some(out) = arg(&args, "--out") {
        std::fs::write(out, &text).unwrap();
    } else {
        println!("{text}");
    }
}
