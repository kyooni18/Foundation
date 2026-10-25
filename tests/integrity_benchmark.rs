//! Phase 4 baseline: incremental validity propagation and the full oracle on 100k Claims.
//! Run: `cargo test --release --offline --test integrity_benchmark -- --ignored --nocapture`.
//! Writes reports/evidence/phase-4/benchmark.json. Not a pass/fail performance gate beyond
//! correctness (incremental == oracle after every measured operation).
#[path = "support/synthetic.rs"]
mod synthetic;
use prime_radiant::{App, domain::*};
use serde_json::json;
use std::time::{Duration, Instant};
use synthetic::{ClaimSpec, Rng, SourceSpec};

const CLAIMS: usize = 100_000;
const PER_SOURCE: usize = 10;
const HUB_CHILDREN: usize = 5_000;
const CHAINS: usize = 100;
const CHAIN_LEN: usize = 100;

fn non_valid(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM claim_validity WHERE state<>'valid'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}
fn median(mut v: Vec<Duration>) -> f64 {
    v.sort();
    v[v.len() / 2].as_secs_f64() * 1e3
}
#[test]
#[ignore = "benchmark; run explicitly with --release"]
fn validity_benchmark_100k_claims() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    drop(App::open(&path).unwrap());
    let started = Instant::now();
    let mut conn = synthetic::connect(&path);
    let tx = conn.transaction().unwrap();
    let mut claims: Vec<String> = Vec::with_capacity(CLAIMS);
    let mut sources = Vec::new();
    for s in 0..CLAIMS / PER_SOURCE {
        let texts: Vec<String> = (0..PER_SOURCE).map(|i| format!("bench {s}.{i}.")).collect();
        let spec = SourceSpec {
            recorder: ["user", "agent"][s % 2],
            session: None,
            captured_at: "2026-01-01T00:00:00.000Z",
            authored_at: None,
            claims: texts.iter().map(|t| ClaimSpec::plain(t.clone())).collect(),
        };
        let (id, ids) = synthetic::source(&tx, &spec);
        sources.push(id);
        claims.extend(ids);
    }
    let mut edges = 0usize;
    let mut kinds = std::collections::BTreeMap::<&str, usize>::new();
    let mut add = |child: usize, parent: usize, kind: &'static str| {
        synthetic::dependency(&tx, &claims[child], &claims[parent], kind);
        *kinds.entry(kind).or_default() += 1;
        edges += 1;
    };
    // Hub: claim 0 is the premise of HUB_CHILDREN Claims.
    for c in 1..=HUB_CHILDREN {
        add(c, 0, "premise");
    }
    // Deep chains: CHAINS premise chains of CHAIN_LEN.
    let chain_base = HUB_CHILDREN + 1;
    for k in 0..CHAINS {
        for i in 1..CHAIN_LEN {
            let c = chain_base + k * CHAIN_LEN + i;
            add(c, c - 1, ["premise", "restates"][i % 2]);
        }
    }
    // Mixed random DAG over the remaining Claims (edges always point to earlier Claims).
    let mut r = Rng::new(0x9a4e_be0c);
    let kinds_all = ["premise", "restates", "alternative", "informed_by"];
    for c in chain_base + CHAINS * CHAIN_LEN..CLAIMS {
        for _ in 0..r.below(3) {
            let parent = c - 1 - r.below(c.min(5_000) - 1);
            add(c, parent, kinds_all[r.below(4)]);
        }
    }
    drop(add);
    tx.commit().unwrap();
    drop(conn);
    let generated = started.elapsed();
    let t = Instant::now();
    let app = App::open(&path).unwrap();
    let open_rebuild = t.elapsed();
    let user = Principal::for_local_cli(PrincipalKind::User, None, None);
    let t = Instant::now();
    let oracle_size = app.validity_oracle_size().unwrap();
    let oracle = t.elapsed();
    let t = Instant::now();
    assert!(app.validity_mismatches().unwrap().is_empty());
    let compare = t.elapsed();
    assert_eq!(oracle_size, CLAIMS);

    let baseline = non_valid(&path);
    let mut results = serde_json::Map::new();
    let mut measure = |name: &str, retract: &mut dyn FnMut(), restore: &mut dyn FnMut()| {
        let (mut down, mut up, mut affected) = (Vec::new(), Vec::new(), 0);
        for _ in 0..5 {
            let t = Instant::now();
            retract();
            down.push(t.elapsed());
            affected = non_valid(&path) - baseline;
            let t = Instant::now();
            restore();
            up.push(t.elapsed());
            assert_eq!(non_valid(&path), baseline, "{name}: restore");
        }
        assert!(app.validity_mismatches().unwrap().is_empty(), "{name}");
        eprintln!(
            "{name}: retract {:.1} ms, restore {:.1} ms, affected {affected}",
            median(down.clone()),
            median(up.clone())
        );
        results.insert(name.into(), json!({"retract_ms_median": median(down), "restore_ms_median": median(up), "affected_claims": affected, "runs": 5}));
    };
    let claim_op = |id: String, event: &'static str| {
        let (app, user) = (&app, &user);
        move || app.set_claim_status(user, &id, event).unwrap()
    };
    let source_op = |id: String, event: LifecycleEvent| {
        let (app, user) = (&app, &user);
        move || {
            app.set_source_lifecycle(user, id.parse().unwrap(), event, None)
                .unwrap()
        }
    };
    let leaf = claims[CLAIMS - 1].clone();
    let chain_root = claims[chain_base].clone();
    for (name, id) in [
        ("claim_hub", claims[0].clone()),
        ("claim_chain_root", chain_root),
        ("claim_leaf", leaf),
    ] {
        measure(
            name,
            &mut claim_op(id.clone(), "retract"),
            &mut claim_op(id, "restore"),
        );
    }
    measure(
        "source_hub",
        &mut source_op(sources[0].clone(), LifecycleEvent::Retract),
        &mut source_op(sources[0].clone(), LifecycleEvent::Restore),
    );
    let mid = sources[sources.len() / 2].clone();
    measure(
        "source_mid",
        &mut source_op(mid.clone(), LifecycleEvent::Retract),
        &mut source_op(mid, LifecycleEvent::Restore),
    );
    let t = Instant::now();
    app.rebuild_derived(&user).unwrap();
    let rebuild = t.elapsed();
    let pending: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM job WHERE kind='revalidate'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(app);
    let size: u64 = ["db", "db-wal"]
        .iter()
        .filter_map(|f| std::fs::metadata(dir.path().join(f)).ok())
        .map(|m| m.len())
        .sum();
    let sh = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let report = json!({
        "claims": CLAIMS, "sources": sources.len(), "dependency_edges": edges, "edge_kinds": kinds,
        "shape": {"hub_children": HUB_CHILDREN, "chains": CHAINS, "chain_len": CHAIN_LEN, "random_dag": "0-2 parents within previous 5000 Claims, uniform kinds", "seed": "0x9a4ebe0c"},
        "baseline_non_valid": baseline,
        "generate_s": generated.as_secs_f64(), "open_with_rebuild_s": open_rebuild.as_secs_f64(),
        "oracle_full_recompute_ms": oracle.as_secs_f64() * 1e3, "oracle_compare_ms": compare.as_secs_f64() * 1e3,
        "rebuild_derived_ms": rebuild.as_secs_f64() * 1e3,
        "operations": results, "revalidation_jobs_total": pending,
        "correctness": "validity_mismatches() empty after every measured operation",
        "db_bytes": size,
        "environment": {
            "cpu": sh("sysctl", &["-n", "machdep.cpu.brand_string"]), "model": sh("sysctl", &["-n", "hw.model"]),
            "memory_bytes": sh("sysctl", &["-n", "hw.memsize"]), "os": sh("uname", &["-srm"]), "rustc": sh("rustc", &["--version"]),
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" }, "sqlite": rusqlite::version(),
            "journal": "WAL, synchronous=FULL", "storage": "tempdir on local disk"
        }
    });
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("reports/evidence/phase-4");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("benchmark.json"),
        serde_json::to_string_pretty(&report).unwrap() + "\n",
    )
    .unwrap();
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
}
