//! Phase 4 crash recovery: SIGKILL during the v2->v3 migration and during validity
//! propagation must never leave partial schema or committed lifecycle with stale validity.
#![cfg(unix)]
#[path = "support/synthetic.rs"]
mod synthetic;
use prime_radiant::{App, domain::*};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    time::Duration,
};

fn spawn(worker: &str, db: &std::path::Path) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", worker, "--ignored", "--nocapture"])
        .env("PRIMERADIANT_CRASH_DB", db)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}
fn wait_for(reader: &mut impl BufRead, prefix: &str) {
    let mut line = String::new();
    loop {
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0, "worker exited early");
        if line.starts_with(prefix) {
            return;
        }
    }
}
#[test]
#[ignore = "subprocess worker invoked by sigkill_during_migration_is_atomic"]
fn migration_worker() {
    println!("START");
    std::io::stdout().flush().unwrap();
    App::open(std::env::var_os("PRIMERADIANT_CRASH_DB").unwrap()).unwrap();
    println!("OPENED");
    std::io::stdout().flush().unwrap();
}
#[test]
fn sigkill_during_migration_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let template = dir.path().join("template.db");
    synthetic::v2_fixture(&template, 1200, 5);
    let expected = synthetic::counts(&rusqlite::Connection::open(&template).unwrap());
    let mut observed = std::collections::BTreeMap::new();
    for (round, delay) in [0u64, 5, 20, 80, 200, 500].into_iter().enumerate() {
        let db = dir.path().join(format!("round{round}.db"));
        std::fs::copy(&template, &db).unwrap();
        let mut child = spawn("migration_worker", &db);
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        wait_for(&mut reader, "START");
        std::thread::sleep(Duration::from_millis(delay));
        child.kill().unwrap();
        child.wait().unwrap();
        // Inspect before any App recovery: schema is either wholly v2 or wholly v3.
        let conn = synthetic::connect(&db);
        let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        let tables: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('claim_validity','status_event_v2','claim_dependency_v2','job_v3')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, if version == 3 { 1 } else { 0 }, "round {round}");
        assert_eq!(synthetic::counts(&conn), expected, "round {round}");
        *observed.entry(version).or_insert(0) += 1;
        drop(conn);
        let app = App::open(&db).unwrap();
        assert!(app.verify_integrity().unwrap().ok, "round {round}");
        assert!(app.validity_mismatches().unwrap().is_empty());
        drop(app);
        let conn = synthetic::connect(&db);
        assert_eq!(synthetic::counts(&conn), expected);
    }
    eprintln!("migration SIGKILL rounds by schema version observed after kill: {observed:?}");
}
#[test]
#[ignore = "subprocess worker invoked by sigkill_during_propagation_keeps_validity_consistent"]
fn propagation_worker() {
    let path = std::env::var_os("PRIMERADIANT_CRASH_DB").unwrap();
    let app = App::open(&path).unwrap();
    let user = Principal::for_local_cli(PrincipalKind::User, None, None);
    let hub: String = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT id FROM claim ORDER BY rowid LIMIT 1", [], |r| r.get(0))
        .unwrap();
    for i in 0.. {
        let event = if i % 2 == 0 { "retract" } else { "restore" };
        app.set_claim_status(&user, &hub, event).unwrap();
        println!("COMMITTED {event}");
        std::io::stdout().flush().unwrap();
    }
}
#[test]
fn sigkill_during_propagation_keeps_validity_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db");
    // One premise chain: retracting its root changes every Claim's validity.
    synthetic::v2_fixture(&db, 1000, 5);
    drop(App::open(&db).unwrap());
    let mut acknowledged = 0;
    for (round, delay) in [0u64, 7, 19, 41, 83, 151].into_iter().enumerate() {
        let mut child = spawn("propagation_worker", &db);
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        wait_for(&mut reader, "COMMITTED");
        let collector = std::thread::spawn(move || reader.lines().count());
        std::thread::sleep(Duration::from_millis(delay));
        child.kill().unwrap();
        child.wait().unwrap();
        acknowledged += 1 + collector.join().unwrap();
        // Before any App opens: committed lifecycle and materialized validity agree.
        let conn = synthetic::connect(&db);
        let stale = prime_radiant::integrity::validity_mismatches_on(&conn).unwrap();
        assert!(stale.is_empty(), "round {round}: {} stale", stale.len());
        drop(conn);
        assert!(App::open(&db).unwrap().verify_integrity().unwrap().ok);
    }
    eprintln!("propagation SIGKILL: 6 rounds, {acknowledged} acknowledged 5000-Claim propagations");
}
