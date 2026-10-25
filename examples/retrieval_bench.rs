//! Large-corpus benchmark: synthetic Claims (Korean/English mix, near duplicates, identifiers,
//! revisions, dependencies, hidden/forgotten/retracted) indexed through the real queue workers.
//!   retrieval_bench --claims 100000 --embed 100000 --out report.json [--embedding local|off]
#[path = "../tests/support/synthetic.rs"]
mod synthetic;
use prime_radiant::{
    App, Policy,
    domain::*,
    embedding::EmbeddingConfig,
    retrieval::search::{SearchRequest, Strategy},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use synthetic::{BulkClaim, Rng};

fn arg(a: &[String], n: &str) -> Option<String> {
    a.iter()
        .position(|x| x == n)
        .and_then(|i| a.get(i + 1).cloned())
}
fn pct(v: &mut [f64]) -> Value {
    v.sort_by(f64::total_cmp);
    let at = |q: f64| {
        if v.is_empty() {
            0.0
        } else {
            v[((v.len() as f64 * q).ceil() as usize)
                .saturating_sub(1)
                .min(v.len() - 1)]
        }
    };
    json!({"n": v.len(), "p50_ms": at(0.5), "p95_ms": at(0.95), "p99_ms": at(0.99)})
}
fn rss_mb() -> f64 {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    if cfg!(target_os = "macos") {
        u.ru_maxrss as f64 / 1e6
    } else {
        u.ru_maxrss as f64 / 1e3
    }
}
const SUBJ_EN: &[&str] = &[
    "The payments service",
    "Our mobile app",
    "The data pipeline",
    "The search index",
    "The auth gateway",
    "The billing job",
    "The admin console",
    "The cache layer",
    "The ingestion worker",
    "The report exporter",
    "The notification hub",
    "The scheduler",
];
const VERB_EN: &[&str] = &[
    "retries failed requests",
    "stores snapshots",
    "rotates credentials",
    "validates payloads",
    "compacts old records",
    "emits audit events",
    "throttles bursts",
    "migrates schemas",
    "replays dead letters",
    "prunes expired sessions",
];
const WHEN_EN: &[&str] = &[
    "every night",
    "after each deploy",
    "on the first of the month",
    "when load is high",
    "during maintenance windows",
    "every fifteen minutes",
    "on weekends only",
    "before release freezes",
];
const SUBJ_KO: &[&str] = &[
    "결제 서비스는",
    "모바일 앱은",
    "데이터 파이프라인은",
    "검색 인덱스는",
    "인증 게이트웨이는",
    "정산 작업은",
    "관리자 콘솔은",
    "캐시 계층은",
    "수집 워커는",
    "리포트 추출기는",
    "알림 허브는",
    "스케줄러는",
];
const VERB_KO: &[&str] = &[
    "실패한 요청을 재시도한다",
    "스냅샷을 저장한다",
    "인증 정보를 교체한다",
    "페이로드를 검증한다",
    "오래된 기록을 압축한다",
    "감사 이벤트를 남긴다",
    "폭주 트래픽을 제한한다",
    "스키마를 이전한다",
    "데드레터를 재처리한다",
    "만료된 세션을 정리한다",
];
const WHEN_KO: &[&str] = &[
    "매일 밤",
    "배포할 때마다",
    "매월 1일에",
    "부하가 높을 때",
    "점검 시간에",
    "15분마다",
    "주말에만",
    "릴리스 동결 전에",
];
const MODS: &[&str] = &[
    "billing", "auth", "search", "ingest", "cache", "report", "sched", "notify",
];
fn text(r: &mut Rng, i: usize) -> (String, &'static str) {
    let ko = r.chance(0.5);
    let (s, v, w) = if ko {
        (SUBJ_KO, VERB_KO, WHEN_KO)
    } else {
        (SUBJ_EN, VERB_EN, WHEN_EN)
    };
    let mut t = if ko {
        format!(
            "{} {} {}.",
            s[r.below(s.len())],
            w[r.below(w.len())],
            v[r.below(v.len())]
        )
    } else {
        format!(
            "{} {} {}.",
            s[r.below(s.len())],
            v[r.below(v.len())],
            w[r.below(w.len())]
        )
    };
    if r.chance(0.15) {
        t = format!("{t} 참조: SVC-{}", 1000 + i % 9000);
        if !ko {
            t = t.replace("참조", "ref");
        }
    }
    if r.chance(0.10) {
        t = format!(
            "{t} src/{}/{}.rs",
            MODS[r.below(MODS.len())],
            ["handler", "client", "worker", "types"][r.below(4)]
        );
    }
    (t, if ko { "ko" } else { "en" })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = arg(&args, "--claims").map_or(100_000, |v| v.parse().unwrap());
    let embed_n: usize = arg(&args, "--embed").map_or(0, |v| v.parse().unwrap());
    let embedding = EmbeddingConfig::parse(
        &arg(&args, "--embedding").unwrap_or_else(|| "local".into()),
        None,
        None,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = arg(&args, "--db")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.path().join("bench.db"));
    let mut report = json!({"claims": n, "machine": {"os": std::env::consts::OS, "arch": std::env::consts::ARCH, "cpus": std::thread::available_parallelism().map_or(0, |n| n.get())}});
    let save = |r: &Value| {
        if let Some(o) = arg(&args, "--out") {
            std::fs::write(o, serde_json::to_string_pretty(r).unwrap()).unwrap();
        }
    };
    drop(
        App::open_with_policy(
            &path,
            Policy {
                embedding: EmbeddingConfig::Off,
                ..Default::default()
            },
        )
        .unwrap(),
    );
    // 1. Corpus.
    let t = Instant::now();
    let mut rng = Rng::new(20261025);
    let mut all: Vec<(String, String)> = Vec::with_capacity(n); // (claim id, text)
    {
        let mut conn = synthetic::connect(&path);
        conn.pragma_update(None, "synchronous", "OFF").unwrap();
        let tx = conn.transaction().unwrap();
        let mut made = 0;
        while made < n {
            let k = (1 + rng.below(5)).min(n - made);
            let mut claims = vec![];
            for j in 0..k {
                let (mut t, lang) = text(&mut rng, made + j);
                if !all.is_empty() && rng.chance(0.10) {
                    // Near duplicate of an earlier Claim with one changed word.
                    let base = all[rng.below(all.len())].1.clone();
                    t = base.replacen("the", "a", 1) + " (copy)";
                }
                claims.push(BulkClaim { text: t, lang });
            }
            let recorder = if rng.chance(0.02) { "agent" } else { "user" };
            let day = 1 + made % 27;
            let when = format!("2026-{:02}-{:02}T00:00:00.000Z", 1 + (made / 9000) % 9, day);
            let (_, ids) = synthetic::bulk_source_at(&tx, recorder, &when, Some(&when), &claims);
            for (c, id) in claims.iter().zip(ids) {
                if rng.chance(0.03) {
                    tx.execute("INSERT INTO status_event(target_kind,target_id,event,actor,created_at) VALUES('claim',?1,?2,'user',?3)", rusqlite::params![id, if rng.chance(0.5) { "forget" } else { "retract" }, when]).unwrap();
                }
                if all.len() > 10 && rng.chance(0.05) {
                    let parent = &all[rng.below(all.len())].0;
                    tx.execute("INSERT OR IGNORE INTO claim_dependency(id,child,parent,kind,declared_by,created_at) VALUES(?1,?2,?3,'informed_by','system',?4)", rusqlite::params![ulid::Ulid::new().to_string(), id, parent, when]).ok();
                }
                all.push((id, c.text.clone()));
            }
            made += k;
        }
        tx.commit().unwrap();
    }
    report["generate_insert_seconds"] = json!(t.elapsed().as_secs_f64());
    // 2. Cold start: open derives access/validity and reconciles index jobs.
    let t = Instant::now();
    let app = Arc::new(
        App::open_with_policy(
            &path,
            Policy {
                embedding: embedding.clone(),
                ..Default::default()
            },
        )
        .unwrap(),
    );
    report["cold_start_open_seconds"] = json!(t.elapsed().as_secs_f64());
    let user = Principal::for_local_cli(PrincipalKind::User, None, None);
    report["queue_after_open"] = json!(
        app.queue_status(&user)
            .unwrap()
            .kinds
            .iter()
            .map(|(k, v)| (k.clone(), v.pending))
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    // 3. Ingestion latency/throughput (synchronous path only; queue-first).
    let mut lat = vec![];
    let t = Instant::now();
    for i in 0..2000 {
        let s = Instant::now();
        app.record_source(
            &user,
            RecordSource::plain(format!("benchmark ingestion record {i} {}", rng.next())),
        )
        .unwrap();
        lat.push(s.elapsed().as_secs_f64() * 1000.0);
    }
    report["ingest"] = json!({"latency": pct(&mut lat), "enqueue_throughput_per_s": 2000.0 / t.elapsed().as_secs_f64()});
    save(&report);
    // 4. Lexical background throughput.
    let t = Instant::now();
    let mut indexed = 0;
    loop {
        let r = app.process_lexical_once("bench").unwrap();
        indexed += r.claims;
        if r.jobs + r.cancelled == 0 {
            break;
        }
    }
    let secs = t.elapsed().as_secs_f64();
    report["lexical_index"] =
        json!({"claims": indexed, "seconds": secs, "claims_per_s": indexed as f64 / secs});
    save(&report);
    // 5. Embedding (real provider); a configurable prefix of the corpus.
    if embed_n > 0
        && let Some(provider) = app.embedder()
    {
        let status = app.queue_status(&user).unwrap();
        let target = embed_n.min(status.index.visible_claims as usize);
        let t = Instant::now();
        let mut done = 0usize;
        let mut samples = vec![];
        while done < target.saturating_sub(target / 10) {
            let s = Instant::now();
            let r = app
                .process_embeddings_once(provider.as_ref(), "bench")
                .unwrap();
            if r.failed + r.deferred > 0 {
                report["embedding_error"] = json!(format!("{r:?}"));
                break;
            }
            if r.jobs + r.cancelled == 0 {
                break;
            }
            done += r.claims;
            samples.push(r.claims as f64 / s.elapsed().as_secs_f64().max(1e-9));
            eprintln!("embedded {done}/{target}");
        }
        let secs = t.elapsed().as_secs_f64();
        report["embedding"] = json!({"claims": done, "seconds": secs, "claims_per_s": done as f64 / secs, "provider_calls": provider.calls()});
        save(&report);
    }
    // 6. Search latency by strategy (known-item queries drawn from stored text).
    let queries: Vec<(String, String)> = (0..200)
        .map(|_| {
            let (id, t) = &all[rng.below(all.len())];
            (
                id.clone(),
                t.split_whitespace().take(5).collect::<Vec<_>>().join(" "),
            )
        })
        .collect();
    let mut by = serde_json::Map::new();
    for (name, s) in [
        ("lexical_only", Strategy::LexicalOnly),
        ("semantic_only", Strategy::SemanticOnly),
        ("adaptive", Strategy::Adaptive),
    ] {
        if s != Strategy::LexicalOnly && embed_n == 0 {
            continue;
        }
        app.clear_query_cache();
        let mut lat = vec![];
        let (mut hit, mut seen) = (0, 0);
        for (id, q) in &queries {
            let mut req = SearchRequest::new(q.clone());
            req.strategy = s;
            let t = Instant::now();
            let r = app.search(&user, req).unwrap();
            lat.push(t.elapsed().as_secs_f64() * 1000.0);
            seen += 1;
            hit += usize::from(r.results.iter().any(|h| &h.claim.id == id));
        }
        by.insert(name.into(), json!({"latency": pct(&mut lat), "known_item_recall@10_visible_and_indexed": hit as f64 / seen as f64}));
    }
    report["search"] = Value::Object(by);
    save(&report);
    // 7. Concurrent ingestion + search while background workers drain the rest.
    let stop = Arc::new(AtomicBool::new(false));
    let workers: Vec<_> = {
        let mut hs = vec![];
        let a = app.clone();
        let st = stop.clone();
        hs.push(std::thread::spawn(move || {
            while !st.load(Ordering::SeqCst) {
                if a.process_lexical_once("bg-l").map_or(0, |r| r.jobs) == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }));
        if let Some(p) = app.embedder()
            && embed_n > 0
        {
            let a = app.clone();
            let st = stop.clone();
            hs.push(std::thread::spawn(move || {
                prime_radiant::retrieval::queue::background_qos();
                while !st.load(Ordering::SeqCst) {
                    if a.process_embeddings_once(p.as_ref(), "bg-e")
                        .map_or(0, |r| r.jobs)
                        == 0
                    {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            }));
        }
        hs
    };
    let (mut rec, mut srch) = (vec![], vec![]);
    for i in 0..100 {
        let token = format!("zzfresh{i}x{}", rng.below(1_000_000));
        let t = Instant::now();
        app.record_source(&user, RecordSource::plain(format!("fresh note {token}")))
            .unwrap();
        rec.push(t.elapsed().as_secs_f64() * 1000.0);
        let s = Instant::now();
        let mut req = SearchRequest::new(
            all[rng.below(all.len())]
                .1
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" "),
        );
        req.limit = 10;
        app.search(&user, req).unwrap();
        srch.push(s.elapsed().as_secs_f64() * 1000.0);
    }
    stop.store(true, Ordering::SeqCst);
    workers.into_iter().for_each(|h| h.join().unwrap());
    report["concurrent_ingest_and_search"] = json!({"record": pct(&mut rec), "search": pct(&mut srch), "note": "background lexical and embedding workers running"});
    // 8. Index freshness lag (queue wait + processing) and rebuild/incremental.
    let st = app.queue_status(&user).unwrap();
    report["queue_timing"] = serde_json::to_value(&st.kinds).unwrap();
    let t = Instant::now();
    app.rebuild_lexical(&user).unwrap();
    while app.process_lexical_once("rb").unwrap().jobs > 0 {}
    report["full_lexical_rebuild_seconds"] = json!(t.elapsed().as_secs_f64());
    report["peak_rss_mb"] = json!(rss_mb());
    report["db_bytes"] = json!(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
    report["integrity_ok"] = json!(app.verify_integrity().unwrap().ok);
    save(&report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
