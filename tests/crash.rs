#![cfg(unix)]
use prime_radiant::{App, domain::*};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[test]
#[ignore = "subprocess worker invoked by t6"]
fn crash_worker() {
    let path = std::env::var_os("PRIMERADIANT_CRASH_DB").unwrap();
    let app = App::open(path).unwrap();
    let user = Principal::for_local_cli(PrincipalKind::User, None, None);
    let round = std::env::var("PRIMERADIANT_CRASH_ROUND").unwrap();
    for n in 0..100_000 {
        let n = format!("{round}:{n}");
        let mut req = RecordSource::plain(format!("committed record {n}: 한글 👩‍💻\r\n"));
        req.idempotency_key = Some(n.to_string());
        let r = app.record_source(&user, req).unwrap();
        println!("COMMITTED {}", r.source_id);
        std::io::stdout().flush().unwrap();
    }
}
#[test]
fn t6_sigkill_twenty_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut total = 0;
    for round in 0..20 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_worker", "--ignored", "--nocapture"])
            .env("PRIMERADIANT_CRASH_DB", &path)
            .env("PRIMERADIANT_CRASH_ROUND", round.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        // Wait until the child has actually committed; then kill at a varying offset.
        loop {
            assert!(reader.read_line(&mut first).unwrap() > 0);
            if first.starts_with("COMMITTED ") {
                break;
            }
            first.clear();
        }
        let collector = std::thread::spawn(move || {
            let mut ids = vec![first.trim().strip_prefix("COMMITTED ").unwrap().to_owned()];
            for line in reader.lines() {
                let line = line.unwrap();
                if let Some(id) = line.strip_prefix("COMMITTED ") {
                    ids.push(id.to_owned());
                }
            }
            ids
        });
        let jitter = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64
            % 25;
        std::thread::sleep(Duration::from_millis(1 + jitter));
        child.kill().unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        let ids = collector.join().unwrap();
        total += ids.len();
        let app = App::open(&path).unwrap();
        assert!(app.verify_integrity().unwrap().ok, "round {round}");
        let user = Principal::for_local_cli(PrincipalKind::User, None, None);
        for id in ids {
            let source = app.get_source(&user, id.parse().unwrap()).unwrap();
            assert_eq!(
                source.content,
                format!(
                    "committed record {}: 한글 👩‍💻\r\n",
                    source.idempotency_key.unwrap()
                )
            );
        }
        let conn = rusqlite::Connection::open(&path).unwrap();
        let partial:u32=conn.query_row("SELECT count(*) FROM source WHERE content <> 'committed record ' || idempotency_key || ': 한글 👩‍💻' || char(13) || char(10)",[],|r|r.get(0)).unwrap();
        assert_eq!(partial, 0);
    }
    eprintln!("T6: 20 SIGKILL/restart cycles, {total} acknowledged records verified");
}
