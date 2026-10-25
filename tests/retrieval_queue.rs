//! Queue-driven derived processing and retrieval safety (the required case list plus races).
#[path = "support/mock_embed.rs"]
mod mock_embed;
#[path = "support/synthetic.rs"]
mod synthetic;
use mock_embed::MockEmbedder;
use prime_radiant::{
    App, Policy,
    domain::*,
    embedding::{EmbedError, EmbeddingConfig, Injected},
    retrieval::{queue::QueueLimits, search::SearchRequest},
};
use rusqlite::Connection;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use synthetic::BulkClaim;

fn user() -> Principal {
    Principal::for_local_cli(PrincipalKind::User, None, None)
}
fn agent() -> Principal {
    Principal::for_local_cli(PrincipalKind::Agent, None, None)
}
struct Fx {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    app: Arc<App>,
    mock: Arc<MockEmbedder>,
}
fn policy(mock: &Arc<MockEmbedder>) -> Policy {
    Policy {
        embedding: EmbeddingConfig::Custom(Injected(mock.clone())),
        ..Default::default()
    }
}
fn fx() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    drop(App::open(&path).unwrap());
    let mock = Arc::new(MockEmbedder::new("r1"));
    let app = Arc::new(App::open_with_policy(&path, policy(&mock)).unwrap());
    Fx {
        _dir: dir,
        path,
        app,
        mock,
    }
}
fn raw(f: &Fx) -> Connection {
    let c = Connection::open(&f.path).unwrap();
    c.busy_timeout(std::time::Duration::from_secs(30)).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    c
}
fn count(f: &Fx, sql: &str) -> i64 {
    raw(f).query_row(sql, [], |r| r.get(0)).unwrap()
}
/// Add published Claims behind the app's back, then let the app derive access/validity and
/// enqueue index jobs exactly as it does for any other published data.
fn add(f: &Fx, recorder: &str, texts: &[&str]) -> (String, Vec<String>) {
    let claims: Vec<BulkClaim> = texts
        .iter()
        .map(|t| BulkClaim {
            text: (*t).into(),
            lang: if t.is_ascii() { "en" } else { "ko" },
        })
        .collect();
    let out = synthetic::bulk_source(&raw(f), recorder, "2026-01-01T00:00:00.000Z", &claims);
    f.app.rebuild_derived(&user()).unwrap();
    f.app.reconcile_indexing().unwrap();
    out
}
fn drain(f: &Fx) {
    for _ in 0..1000 {
        let l = f.app.process_lexical_once("w").unwrap();
        let e = f.app.process_embeddings_once(f.mock.as_ref(), "w").unwrap();
        if l.jobs + l.cancelled + e.jobs + e.cancelled == 0 {
            return;
        }
    }
    panic!("queue did not drain");
}
fn texts(f: &Fx, p: &Principal, q: &str) -> Vec<String> {
    f.app
        .search(p, SearchRequest::new(q))
        .unwrap()
        .results
        .into_iter()
        .map(|h| h.claim.text)
        .collect()
}

#[test]
fn record_is_atomic_returns_before_processing_and_applies_backpressure() {
    let f = fx();
    let r = f
        .app
        .record_source(&user(), RecordSource::plain("hello world"))
        .unwrap();
    assert_eq!(r.processing, "stored_not_extracted");
    // Only the extraction job exists; no Claims, no index work, nothing processed.
    assert_eq!(
        count(
            &f,
            "SELECT count(*) FROM job WHERE state='pending' AND kind='extract'"
        ),
        1
    );
    assert_eq!(
        count(&f, "SELECT count(*) FROM job WHERE kind LIKE 'index_%'"),
        0
    );
    assert_eq!(count(&f, "SELECT count(*) FROM claim"), 0);
    // A full queue refuses the write; source and job are absent together.
    let tight = Policy {
        queue: QueueLimits {
            max_open_jobs: 1,
            ..Default::default()
        },
        ..policy(&f.mock)
    };
    drop(f.app);
    let app = App::open_with_policy(&f.path, tight).unwrap();
    assert!(matches!(
        app.record_source(&user(), RecordSource::plain("second")),
        Err(Error::QueueFull)
    ));
    let c = Connection::open(&f.path).unwrap();
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM source", [], |r| r.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM job", [], |r| r.get(0))
            .unwrap(),
        1
    );
    // An idempotent retry of the stored request is not refused by backpressure.
}

#[test]
fn restart_resumes_and_duplicate_delivery_is_idempotent() {
    let f = fx();
    let texts_in: Vec<String> = (0..40)
        .map(|i| format!("alpha record number {i}"))
        .collect();
    for chunk in texts_in.chunks(4) {
        add(
            &f,
            "user",
            &chunk.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }
    // Process only part of the queue, then "crash" (drop the app without finishing).
    let _ = f.app.process_lexical_once("w").unwrap();
    let Fx {
        _dir,
        path,
        app,
        mock,
    } = f;
    drop(app);
    let app = Arc::new(App::open_with_policy(&path, policy(&mock)).unwrap());
    let f = Fx {
        _dir,
        path,
        app,
        mock,
    };
    drain(&f);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_lex"), 40);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 40);
    // Duplicate delivery: every job is pushed back to pending and processed again.
    raw(&f)
        .execute(
            "UPDATE job SET state='pending',attempts=0,finished_at=NULL WHERE kind LIKE 'index_%'",
            [],
        )
        .unwrap();
    drain(&f);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_lex"), 40);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 40);
    assert_eq!(
        count(
            &f,
            "SELECT count(*) FROM claim_fts WHERE claim_fts MATCH 'alpha'"
        ),
        40
    );
    assert_eq!(
        texts(&f, &user(), "alpha record number 7")
            .first()
            .map(String::as_str),
        Some("alpha record number 7")
    );
}

#[test]
fn expired_lease_is_fenced_and_late_worker_cannot_complete_or_duplicate() {
    let f = fx();
    add(
        &f,
        "user",
        &["fenced embedding one", "fenced embedding two"],
    );
    while f.app.process_lexical_once("w").unwrap().jobs > 0 {}
    *f.mock.delay.lock().unwrap() = std::time::Duration::from_millis(600);
    let slow = {
        let (app, mock) = (f.app.clone(), f.mock.clone());
        std::thread::spawn(move || app.process_embeddings_once(mock.as_ref(), "ghost").unwrap())
    };
    std::thread::sleep(std::time::Duration::from_millis(150));
    // The ghost's lease expires; a second worker takes over (attempt 2) and finishes.
    raw(&f).execute("UPDATE job SET lease_until='2000-01-01T00:00:00.000Z' WHERE kind='index_embed' AND state='leased'", []).unwrap();
    *f.mock.delay.lock().unwrap() = std::time::Duration::ZERO;
    let fast = f
        .app
        .process_embeddings_once(f.mock.as_ref(), "taker")
        .unwrap();
    assert_eq!(fast.jobs, 1);
    let late = slow.join().unwrap();
    assert_eq!(late.jobs, 0, "fenced worker must not complete the job");
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 2);
    assert_eq!(
        count(
            &f,
            "SELECT count(*) FROM job WHERE kind='index_embed' AND state='done' AND attempts=2"
        ),
        1
    );
}

#[test]
fn retry_is_bounded_idempotent_observable_and_owner_only() {
    let f = fx();
    add(&f, "user", &["retry me please"]);
    drain_lexical(&f);
    *f.mock.fail.lock().unwrap() = Some(EmbedError::Runtime);
    for _ in 0..5 {
        f.app.process_embeddings_once(f.mock.as_ref(), "w").unwrap();
        raw(&f)
            .execute(
                "UPDATE job SET run_after='2000-01-01T00:00:00.000Z' WHERE state='pending'",
                [],
            )
            .unwrap();
    }
    let jobs = f.app.queue_jobs(&user(), Some("failed"), 10).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].last_error.as_deref(), Some("runtime"));
    assert_eq!(f.app.queue_status(&user()).unwrap().health, "degraded");
    assert!(matches!(
        f.app.queue_status(&agent()),
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        f.app.retry_job(&agent(), &jobs[0].id),
        Err(Error::Forbidden)
    ));
    *f.mock.fail.lock().unwrap() = None;
    assert!(f.app.retry_job(&user(), &jobs[0].id).unwrap());
    drain(&f);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 1);
    assert_eq!(f.app.queue_status(&user()).unwrap().health, "ok");
    // Raw provider text never reaches the database.
    assert_eq!(
        count(
            &f,
            "SELECT count(*) FROM job WHERE last_error IS NOT NULL AND length(last_error)>32"
        ),
        0
    );
}
fn drain_lexical(f: &Fx) {
    while f.app.process_lexical_once("w").unwrap().jobs > 0 {}
}

#[test]
fn unavailable_provider_defers_without_consuming_attempts_or_sending_anywhere() {
    let f = fx();
    add(&f, "user", &["deferred work"]);
    drain_lexical(&f);
    *f.mock.fail.lock().unwrap() = Some(EmbedError::Unavailable);
    let r = f.app.process_embeddings_once(f.mock.as_ref(), "w").unwrap();
    assert_eq!(r.deferred, 1);
    let job = &f.app.queue_jobs(&user(), Some("deferred"), 10).unwrap()[0];
    assert_eq!(job.attempts, 0);
    assert_eq!(job.last_error.as_deref(), Some("provider_unavailable"));
    // Lexical search still works and the response says semantic retrieval is degraded.
    let resp = f
        .app
        .search(&user(), SearchRequest::new("deferred work"))
        .unwrap();
    assert_eq!(resp.results.len(), 1);
    assert_eq!(resp.coverage.status, "degraded");
    assert!(
        resp.coverage
            .reasons
            .contains(&"embedding_index_incomplete")
    );
    *f.mock.fail.lock().unwrap() = None;
    assert_eq!(f.app.resume_deferred(&user(), None).unwrap(), 1);
    drain(&f);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 1);
}

#[test]
fn concurrent_workers_process_each_job_once() {
    let f = fx();
    for i in 0..30 {
        add(&f, "user", &[&format!("concurrent source {i} marker{i}")]);
    }
    let hs: Vec<_> = (0..4)
        .map(|n| {
            let (app, mock) = (f.app.clone(), f.mock.clone());
            std::thread::spawn(move || {
                for _ in 0..200 {
                    let l = app.process_lexical_once(&format!("w{n}")).unwrap();
                    let e = app
                        .process_embeddings_once(mock.as_ref(), &format!("w{n}"))
                        .unwrap();
                    if l.jobs + e.jobs == 0 {
                        break;
                    }
                }
            })
        })
        .collect();
    hs.into_iter().for_each(|h| h.join().unwrap());
    assert_eq!(count(&f, "SELECT count(*) FROM claim_lex"), 30);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 30);
    assert_eq!(
        count(
            &f,
            "SELECT count(*) FROM job WHERE kind LIKE 'index_%' AND state='done'"
        ),
        60
    );
    assert_eq!(
        count(
            &f,
            "SELECT count(*) FROM job WHERE kind LIKE 'index_%' AND attempts<>1"
        ),
        0
    );
}

#[test]
fn obsolete_generation_never_overwrites_newer_state() {
    let f = fx();
    let (source, old) = add(
        &f,
        "user",
        &[
            "old expression of the source",
            "new expression of the source",
        ],
    );
    // A newer accepted run supersedes the old one before the old job executes.
    let c = raw(&f);
    let job = ulid::Ulid::new().to_string();
    let t = "2026-02-01T00:00:00.000Z";
    c.execute("INSERT INTO job(id,kind,target_id,idem_key,state,attempts,run_after,created_at,updated_at) VALUES(?1,'extract',?2,?3,'done',1,?4,?4,?4)", rusqlite::params![job, source, format!("revision/{source}"), t]).unwrap();
    c.execute("INSERT INTO extraction_run(job_id,source_id,extractor_id,output_hash,approval_actor,created_at) VALUES(?1,?2,'synthetic','h','user',?3)", rusqlite::params![job, source, t]).unwrap();
    let new = ulid::Ulid::new().to_string();
    c.execute("INSERT INTO claim(id,source_id,text,lang,category,speaker,basis,anchor_start,anchor_end,anchor_quote,standing,extractor_id,job_id,created_at) VALUES(?1,?2,'new expression of the source','en','Fact','user','verbatim',29,57,'new expression of the source',0,'synthetic',?3,?4)", rusqlite::params![new, source, job, t]).unwrap();
    f.app.rebuild_derived(&user()).unwrap();
    f.app.reconcile_indexing().unwrap();
    drain(&f);
    // The stale-generation jobs were cancelled; only the current expression is indexed.
    assert_eq!(
        count(
            &f,
            &format!(
                "SELECT count(*) FROM claim_lex WHERE claim_id IN ('{}','{}')",
                old[0], old[1]
            )
        ),
        0
    );
    assert_eq!(
        count(
            &f,
            &format!("SELECT count(*) FROM claim_lex WHERE claim_id='{new}'")
        ),
        1
    );
    let old_hits = f
        .app
        .search(&user(), SearchRequest::new("old expression of the source"))
        .unwrap()
        .results;
    assert!(
        old_hits.iter().all(|h| !old.contains(&h.claim.id)),
        "superseded expression is not visible or indexed"
    );
    let hits = f
        .app
        .search(&user(), SearchRequest::new("new expression of the source"))
        .unwrap()
        .results;
    assert_eq!(
        hits.iter().map(|h| h.claim.id.as_str()).collect::<Vec<_>>(),
        vec![new.as_str()]
    );
    assert!(
        count(
            &f,
            "SELECT count(*) FROM job WHERE state='cancelled' AND last_error='obsolete'"
        ) >= 1
    );
}

#[test]
fn batched_embeddings_map_to_the_right_claims() {
    let f = fx();
    let many: Vec<String> = (0..150)
        .map(|i| format!("unique statement {i} about topic {}", i * 7))
        .collect();
    add(
        &f,
        "user",
        &many.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    drain(&f);
    // One embedding call carries many Claims, and each stored vector is its own text's vector.
    assert!(f.mock.calls_made() <= 3, "{} calls", f.mock.calls_made());
    let c = raw(&f);
    let rows: Vec<(String, Vec<u8>)> = c
        .prepare("SELECT c.text,v.vec FROM claim c JOIN claim_vec v ON v.claim_id=c.id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert_eq!(rows.len(), 150);
    for (text, blob) in rows.iter().step_by(11) {
        let expect = prime_radiant::embedding::embed(
            f.mock.as_ref(),
            prime_radiant::embedding::Role::Document,
            std::slice::from_ref(text),
        )
        .unwrap()
        .remove(0);
        let got: Vec<f32> = blob
            .chunks_exact(2)
            .map(|b| half::f16::from_le_bytes([b[0], b[1]]).to_f32())
            .collect();
        let cos: f32 = expect.iter().zip(&got).map(|(a, b)| a * b).sum();
        assert!(cos > 0.999, "{text}: {cos}");
    }
}

#[test]
fn model_change_invalidates_only_that_models_vectors() {
    let f = fx();
    add(
        &f,
        "user",
        &["model scoped vector one", "model scoped vector two"],
    );
    drain(&f);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 2);
    let Fx {
        _dir,
        path,
        app,
        mock: _,
    } = f;
    drop(app);
    let mock2 = Arc::new(MockEmbedder::new("r2"));
    let app = Arc::new(App::open_with_policy(&path, policy(&mock2)).unwrap());
    let f = Fx {
        _dir,
        path,
        app,
        mock: mock2,
    };
    // Old vectors untouched; new model has none yet, so semantic coverage is reported incomplete.
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 2);
    let r = f
        .app
        .search(&user(), SearchRequest::new("model scoped"))
        .unwrap();
    assert!(r.coverage.reasons.contains(&"embedding_index_incomplete") || !r.semantic_used);
    assert_eq!(f.app.queue_status(&user()).unwrap().index.embedded, 0);
    drain(&f);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 4);
    assert_eq!(
        count(&f, "SELECT count(DISTINCT model_key) FROM claim_vec"),
        2
    );
    assert_eq!(f.app.queue_status(&user()).unwrap().index.embedded, 2);
}

#[test]
fn retracted_and_forgotten_claims_never_leak_through_stale_indexes() {
    let f = fx();
    let (source, ids) = add(
        &f,
        "user",
        &["leakcheck apple", "leakcheck banana", "leakcheck cherry"],
    );
    drain(&f);
    assert_eq!(texts(&f, &user(), "leakcheck").len(), 3);
    f.app.set_claim_status(&user(), &ids[0], "forget").unwrap();
    f.app.set_claim_status(&user(), &ids[1], "retract").unwrap();
    // No index job has run since; the derived rows still exist, but nothing returns them.
    assert_eq!(count(&f, "SELECT count(*) FROM claim_lex"), 3);
    for p in [user(), agent()] {
        let t = texts(&f, &p, "leakcheck");
        assert_eq!(t, vec!["leakcheck cherry".to_string()]);
        assert!(
            texts(&f, &p, "leakcheck apple")
                .iter()
                .all(|x| !x.contains("apple"))
        );
        assert!(
            texts(&f, &p, "leakcheck banana")
                .iter()
                .all(|x| !x.contains("banana"))
        );
    }
    f.app
        .set_source_lifecycle(
            &user(),
            source.parse().unwrap(),
            LifecycleEvent::Forget,
            None,
        )
        .unwrap();
    assert!(texts(&f, &user(), "leakcheck").is_empty());
    assert!(texts(&f, &agent(), "leakcheck").is_empty());
    let r = f
        .app
        .search(&user(), SearchRequest::new("leakcheck"))
        .unwrap();
    assert_eq!(
        r.indexing.visible_claims, 0,
        "hidden Claims are not counted"
    );
    f.app
        .set_source_lifecycle(
            &user(),
            source.parse().unwrap(),
            LifecycleEvent::Restore,
            None,
        )
        .unwrap();
    assert_eq!(
        texts(&f, &user(), "leakcheck").len(),
        1,
        "forgotten Claim stays forgotten after source restore; retracted Claim is not user-visible"
    );
}

#[test]
fn partial_indexing_is_reported_never_claimed_complete() {
    let f = fx();
    add(&f, "user", &["partial index coverage text"]);
    let none = f
        .app
        .search(&user(), SearchRequest::new("partial index"))
        .unwrap();
    assert!(none.results.is_empty());
    assert!(none.coverage.reasons.contains(&"lexical_index_incomplete"));
    assert_eq!(none.coverage.status, "degraded");
    drain_lexical(&f);
    let lex = f
        .app
        .search(&user(), SearchRequest::new("partial index coverage text"))
        .unwrap();
    assert_eq!(lex.results.len(), 1);
    assert!(lex.coverage.reasons.contains(&"embedding_index_incomplete"));
    assert!(!lex.coverage.exhaustive);
    drain(&f);
    let done = f
        .app
        .search(&user(), SearchRequest::new("partial index coverage text"))
        .unwrap();
    // Semantic retrieval is always similarity-bounded, so coverage is never "complete" with it.
    assert!(!done.coverage.exhaustive);
}

#[test]
fn rebuild_preserves_authoritative_data_and_results() {
    let f = fx();
    add(
        &f,
        "user",
        &[
            "rebuild alpha beta",
            "rebuild gamma delta",
            "한국어 문장 재구축 시험",
        ],
    );
    drain(&f);
    let before = texts(&f, &user(), "rebuild alpha");
    let dump = |f: &Fx| -> Vec<String> {
        raw(f).prepare("SELECT id||text||source_id FROM claim UNION ALL SELECT id||content_sha256 FROM source ORDER BY 1").unwrap().query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect()
    };
    let auth = dump(&f);
    f.app.rebuild_lexical(&user()).unwrap();
    f.app.rebuild_vectors(&user()).unwrap();
    assert_eq!(count(&f, "SELECT count(*) FROM claim_lex"), 0);
    assert!(
        texts(&f, &user(), "rebuild alpha").is_empty(),
        "no index, no results; not a stale answer"
    );
    drain(&f);
    assert_eq!(texts(&f, &user(), "rebuild alpha"), before);
    assert_eq!(dump(&f), auth);
    assert!(f.app.verify_integrity().unwrap().ok);
}

#[test]
fn background_work_does_not_starve_ingestion_or_search() {
    let f = fx();
    let many: Vec<String> = (0..600)
        .map(|i| format!("background load claim {i} token{i}"))
        .collect();
    for chunk in many.chunks(50) {
        add(
            &f,
            "user",
            &chunk.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }
    drain_lexical(&f);
    *f.mock.delay.lock().unwrap() = std::time::Duration::from_millis(15);
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let (app, mock, stop) = (f.app.clone(), f.mock.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if app
                    .process_embeddings_once(mock.as_ref(), "bg")
                    .unwrap()
                    .jobs
                    == 0
                {
                    break;
                }
            }
        })
    };
    let (mut rec, mut srch) = (vec![], vec![]);
    for i in 0..40 {
        let t = std::time::Instant::now();
        f.app
            .record_source(
                &user(),
                RecordSource::plain(format!("ingest during load {i}")),
            )
            .unwrap();
        rec.push(t.elapsed().as_millis());
        let t = std::time::Instant::now();
        f.app
            .search(&user(), SearchRequest::new(format!("token{}", i * 3)))
            .unwrap();
        srch.push(t.elapsed().as_millis());
    }
    stop.store(true, Ordering::SeqCst);
    worker.join().unwrap();
    rec.sort();
    srch.sort();
    eprintln!(
        "under load: record p95 {} ms, search p95 {} ms",
        rec[37], srch[37]
    );
    assert!(
        rec[37] < 500 && srch[37] < 1000,
        "record {rec:?} search {srch:?}"
    );
}

#[test]
fn cloud_provider_is_opt_in_budget_gated_and_never_a_fallback() {
    use prime_radiant::embedding::{
        EmbeddingProvider,
        openrouter::{CloudConfig, CloudProvider},
    };
    let dir = tempfile::tempdir().unwrap();
    // A paid cloud model refuses to run without a spending ledger, before any network I/O.
    let p = CloudProvider::new(
        CloudConfig::new("baai/bge-m3".into()),
        "test-key".into(),
        None,
    )
    .unwrap();
    assert_eq!(p.embed_raw(&["x".into()]).unwrap_err(), EmbedError::Quota);
    assert_eq!(p.calls(), 0);
    let off = App::open_with_policy(
        dir.path().join("off"),
        Policy {
            embedding: EmbeddingConfig::Off,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(off.embedder().is_none() && off.policy_embedding_identity().is_none());
    // Default is the local model, never a remote service.
    assert!(
        App::open(dir.path().join("d"))
            .unwrap()
            .policy_embedding_identity()
            .unwrap()
            .local
    );
}

#[test]
fn phase4_validity_changes_update_retrieval_in_the_same_transaction() {
    let f = fx();
    let (_, ids) = add(&f, "user", &["premise claim stays", "dependent claim goes"]);
    drain(&f);
    f.app
        .add_dependency(&user(), &ids[1], &ids[0], "premise")
        .unwrap();
    let has = |f: &Fx| {
        texts(f, &user(), "dependent claim goes").contains(&"dependent claim goes".to_string())
    };
    assert!(has(&f));
    f.app.set_claim_status(&user(), &ids[0], "retract").unwrap();
    // The dependent became invalid in the same transaction; indexes were not touched.
    assert!(!has(&f));
    let mut req = SearchRequest::new("dependent claim goes");
    req.include_invalid = true;
    assert_eq!(
        f.app
            .search(&agent(), req)
            .unwrap()
            .results
            .iter()
            .filter(|h| h.claim.text == "dependent claim goes")
            .count(),
        1,
        "audit view can include invalid Claims"
    );
    assert!(
        texts(&f, &agent(), "dependent claim goes")
            .iter()
            .all(|t| t != "dependent claim goes"),
        "but not by default"
    );
    assert!(f.app.validity_mismatches().unwrap().is_empty());
    assert!(f.app.verify_integrity().unwrap().ok);
    f.app.set_claim_status(&user(), &ids[0], "restore").unwrap();
    assert!(has(&f));
}

#[test]
fn hidden_records_never_influence_scores_counts_or_cursors() {
    let visible = [
        "incident postmortem for the payments outage",
        "payments team meeting notes",
    ];
    let run = |with_hidden: bool| {
        let f = fx();
        add(&f, "user", &visible);
        if with_hidden {
            // Agent-recorded and never confirmed: outside the user projection.
            add(
                &f,
                "agent",
                &[
                    "payments payments payments outage outage postmortem incident secret",
                    "payments outage secret one",
                    "payments outage secret two",
                ],
            );
        }
        drain(&f);
        let r = f
            .app
            .search(&user(), SearchRequest::new("payments outage postmortem"))
            .unwrap();
        let sig: Vec<(String, f64, Option<f64>, Option<f64>)> = r
            .results
            .iter()
            .map(|h| {
                (
                    h.claim.text.clone(),
                    h.score.fused,
                    h.score.bm25,
                    h.score.lexical_coverage,
                )
            })
            .collect();
        (sig, r.indexing.visible_claims, r.continuation.candidates)
    };
    let (a, a_visible, a_cand) = run(false);
    let (b, b_visible, b_cand) = run(true);
    assert_eq!(
        a, b,
        "scores and ranks identical with and without hidden records"
    );
    assert_eq!((a_visible, a_cand), (b_visible, b_cand));
    // The agent view does see its own records; cursors are credential/role bound.
    let f = fx();
    for i in 0..12 {
        add(&f, "user", &[&format!("cursor topic entry {i}")]);
    }
    drain(&f);
    let mut q = SearchRequest::new("cursor topic entry");
    q.limit = 5;
    let page = f.app.search(&user(), q.clone()).unwrap();
    let cursor = page.continuation.next_cursor.clone().expect("more results");
    q.cursor = Some(cursor.clone());
    assert_eq!(f.app.search(&user(), q.clone()).unwrap().results.len(), 5);
    assert!(matches!(
        f.app.search(&agent(), q.clone()),
        Err(Error::InvalidCursor)
    ));
    q.query = "different query".into();
    assert!(matches!(
        f.app.search(&user(), q),
        Err(Error::InvalidCursor)
    ));
}

#[test]
fn adversarial_races_never_return_forgotten_claims_and_converge() {
    let f = fx();
    let many: Vec<String> = (0..120)
        .map(|i| format!("race corpus entry {i} common words here"))
        .collect();
    let mut all = vec![];
    for chunk in many.chunks(10) {
        all.extend(
            add(
                &f,
                "user",
                &chunk.iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .1,
        );
    }
    let forgotten = Arc::new(std::sync::Mutex::new(
        std::collections::HashSet::<String>::new(),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let mut hs = vec![];
    // Index workers (restart-like: a fresh App is also opened mid-way below).
    for n in 0..2 {
        let (app, mock, stop) = (f.app.clone(), f.mock.clone(), stop.clone());
        hs.push(std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let _ = app.process_lexical_once(&format!("l{n}"));
                let _ = app.process_embeddings_once(mock.as_ref(), &format!("e{n}"));
            }
        }));
    }
    // Forgetting, retracting + restoring, revising-by-rebuild and index rebuilds.
    {
        let (app, forgotten, ids, stop) =
            (f.app.clone(), forgotten.clone(), all.clone(), stop.clone());
        hs.push(std::thread::spawn(move || {
            for (i, id) in ids.iter().enumerate().step_by(3) {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                app.set_claim_status(&user(), id, "forget").unwrap();
                forgotten.lock().unwrap().insert(id.clone());
                if i % 9 == 0 {
                    let other = &ids[(i + 1) % ids.len()];
                    let _ = app.set_claim_status(&user(), other, "retract");
                    let _ = app.set_claim_status(&user(), other, "restore");
                }
            }
        }));
    }
    {
        let (app, stop) = (f.app.clone(), stop.clone());
        hs.push(std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let _ = app.rebuild_lexical(&user());
                std::thread::sleep(std::time::Duration::from_millis(30));
                let _ = app.rebuild_vectors(&user());
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
        }));
    }
    // Searchers: anything forgotten before the search began must be absent from its results.
    let violations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let searchers: Vec<_> = (0..2)
        .map(|_| {
            let (app, forgotten, violations) =
                (f.app.clone(), forgotten.clone(), violations.clone());
            std::thread::spawn(move || {
                for _ in 0..120 {
                    let before = forgotten.lock().unwrap().clone();
                    let mut req = SearchRequest::new("race corpus entry common words");
                    req.limit = 50;
                    let r = app.search(&user(), req).unwrap();
                    for h in &r.results {
                        if before.contains(&h.claim.id) {
                            violations.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            })
        })
        .collect();
    searchers.into_iter().for_each(|h| h.join().unwrap());
    stop.store(true, Ordering::SeqCst);
    hs.into_iter().for_each(|h| h.join().unwrap());
    assert_eq!(
        violations.load(Ordering::SeqCst),
        0,
        "a forgotten Claim was returned"
    );
    // Convergence: after the dust settles every visible Claim is indexed exactly once.
    for _ in 0..5 {
        f.app.reconcile_indexing().unwrap();
        loop {
            let l = f.app.process_lexical_once("final").unwrap();
            let e = f
                .app
                .process_embeddings_once(f.mock.as_ref(), "final")
                .unwrap();
            if l.jobs + l.cancelled + e.jobs + e.cancelled == 0 {
                break;
            }
        }
    }
    let status = f.app.queue_status(&user()).unwrap();
    assert_eq!(status.index.visible_claims, status.index.lexical_indexed);
    assert_eq!(status.index.visible_claims, status.index.embedded);
    assert!(f.app.verify_integrity().unwrap().ok);
    assert!(f.app.validity_mismatches().unwrap().is_empty());
}

#[test]
#[ignore = "subprocess worker for sigkill_during_indexing_is_recoverable"]
fn index_worker() {
    let path = std::env::var_os("PRIMERADIANT_CRASH_DB").unwrap();
    let mock = Arc::new(MockEmbedder::new("r1"));
    *mock.delay.lock().unwrap() = std::time::Duration::from_millis(3);
    let app = App::open_with_policy(path, policy(&mock)).unwrap();
    println!("READY");
    loop {
        let _ = app.process_lexical_once("crash");
        let _ = app.process_embeddings_once(mock.as_ref(), "crash");
    }
}
#[test]
fn sigkill_during_indexing_is_recoverable() {
    use std::io::{BufRead, BufReader};
    let f = fx();
    let many: Vec<String> = (0..400)
        .map(|i| format!("crash recovery entry {i} words"))
        .collect();
    for chunk in many.chunks(20) {
        add(
            &f,
            "user",
            &chunk.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }
    let Fx {
        _dir,
        path,
        app,
        mock,
    } = f;
    drop(app);
    for delay in [0u64, 30, 120] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "index_worker", "--ignored", "--nocapture"])
            .env("PRIMERADIANT_CRASH_DB", &path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        while out.read_line(&mut line).unwrap() > 0 && !line.starts_with("READY") {
            line.clear();
        }
        std::thread::sleep(std::time::Duration::from_millis(delay));
        child.kill().unwrap();
        child.wait().unwrap();
        // Whatever was in flight: the database is consistent and reopens.
        let c = Connection::open(&path).unwrap();
        assert_eq!(
            c.query_row::<String, _, _>("PRAGMA integrity_check", [], |r| r.get(0))
                .unwrap(),
            "ok"
        );
    }
    let app = Arc::new(App::open_with_policy(&path, policy(&mock)).unwrap());
    // Expired leases are taken over; everything converges.
    let c = Connection::open(&path).unwrap();
    c.execute(
        "UPDATE job SET lease_until='2000-01-01T00:00:00.000Z' WHERE state='leased'",
        [],
    )
    .unwrap();
    let f = Fx {
        _dir,
        path,
        app,
        mock,
    };
    drain(&f);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_lex"), 400);
    assert_eq!(count(&f, "SELECT count(*) FROM claim_vec"), 400);
    assert!(f.app.verify_integrity().unwrap().ok);
}
