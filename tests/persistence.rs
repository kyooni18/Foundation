use prime_radiant::{App, domain::*};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
use tempfile::TempDir;
fn p(kind: PrincipalKind) -> Principal {
    Principal::for_local_cli(kind, None, None)
}
fn user() -> Principal {
    p(PrincipalKind::User)
}
fn fixture() -> (TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::open(dir.path().join("db")).unwrap();
    (dir, app)
}
fn keyed(text: &str, key: &str) -> RecordSource {
    let mut r = RecordSource::plain(text);
    r.idempotency_key = Some(key.into());
    r
}
fn count(conn: &Connection, table: &str) -> usize {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
#[test]
fn t1_exact_bytes_and_input_validation() {
    let (_dir, app) = fixture();
    let texts = [
        "한글은 조사까지",
        "한글",
        "English",
        "한글 English",
        "👩‍👩‍👧‍👦👨🏽‍💻",
        "\r\n  \t끝 \r\n",
        "\0embedded\0",
    ];
    for text in texts {
        let result = app
            .record_source(&user(), RecordSource::plain(text))
            .unwrap();
        let source = app.get_source(&user(), result.source_id).unwrap();
        assert_eq!(source.content.as_bytes(), text.as_bytes());
        assert_eq!(
            source.content_sha256,
            format!("{:x}", Sha256::digest(text.as_bytes()))
        );
        assert!(source.authored_at.is_none());
        assert!(source.observed_at.is_none());
        assert_eq!(source.captured_at.as_str().len(), 24);
    }
    assert!(app.record_source(&user(), RecordSource::plain("")).is_err());
    assert!(
        app.record_source(
            &user(),
            RecordSource::plain("a".repeat(MAX_CONTENT_BYTES + 1))
        )
        .is_err()
    );
    for key in ["".to_owned(), "한".into(), "x\n".into(), "x".repeat(201)] {
        assert!(app.record_source(&user(), keyed("ok", &key)).is_err());
    }
    let timestamp: Timestamp = "2026-10-22T18:10:20.123456+09:00".parse().unwrap();
    assert_eq!(timestamp.as_str(), "2026-10-22T09:10:20.123Z");
    assert!("not a date".parse::<Timestamp>().is_err());
}
#[test]
fn t2_authoritative_tables_are_immutable() {
    let (dir, app) = fixture();
    let id = app
        .record_source(&user(), RecordSource::plain("immutable"))
        .unwrap()
        .source_id;
    app.set_source_lifecycle(&user(), id, LifecycleEvent::Retract, None)
        .unwrap();
    let conn = Connection::open(dir.path().join("db")).unwrap();
    for sql in [
        "UPDATE source SET content='changed'",
        "DELETE FROM source",
        "UPDATE status_event SET reason='changed'",
        "DELETE FROM status_event",
        "UPDATE schema_migration SET checksum='changed'",
        "DELETE FROM schema_migration",
    ] {
        assert!(conn.execute(sql, []).is_err(), "{sql}");
    }
}
#[test]
fn t3_idempotency_scope_and_digest() {
    let (dir, app) = fixture();
    let first = app.record_source(&user(), keyed("same", "key")).unwrap();
    let second = app.record_source(&user(), keyed("same", "key")).unwrap();
    assert!(first.created);
    assert!(!second.created);
    assert_eq!(first.source_id, second.source_id);
    assert!(matches!(
        app.record_source(&user(), keyed("different", "key")),
        Err(Error::IdempotencyConflict { .. })
    ));
    let agent = app
        .record_source(&p(PrincipalKind::Agent), keyed("same", "key"))
        .unwrap();
    assert_ne!(first.source_id, agent.source_id);
    for _ in 0..2 {
        assert!(
            app.record_source(&user(), RecordSource::plain("same"))
                .unwrap()
                .created
        );
    }
    let conn = Connection::open(dir.path().join("db")).unwrap();
    assert_eq!(count(&conn, "source"), 4);
    let labeled = Principal::for_local_cli(
        PrincipalKind::User,
        Some("different label".into()),
        Some("session".into()),
    );
    assert!(
        !app.record_source(&labeled, keyed("same", "key"))
            .unwrap()
            .created
    );
    let original = keyed("metadata", "meta");
    app.record_source(&user(), original.clone()).unwrap();
    let variants: Vec<RecordSource> = (0..6)
        .map(|n| {
            let mut r = original.clone();
            match n {
                0 => r.media_type = MediaType::TextMarkdown,
                1 => r.authored_at = Some("2020-01-01T00:00:00Z".parse().unwrap()),
                2 => r.observed_at = Some("2020-01-01T00:00:00Z".parse().unwrap()),
                3 => r.artifact_ref = Some(String::new()),
                4 => r.retention_request = RetentionRequest::UserRequested,
                _ => r.content.push('!'),
            };
            r
        })
        .collect();
    for r in variants {
        assert!(matches!(
            app.record_source(&user(), r),
            Err(Error::IdempotencyConflict { .. })
        ));
    }
    let mut a = keyed("time", "time");
    a.authored_at = Some("2020-01-01T09:00:00+09:00".parse().unwrap());
    let mut b = a.clone();
    b.authored_at = Some("2020-01-01T00:00:00.000Z".parse().unwrap());
    app.record_source(&user(), a).unwrap();
    assert!(!app.record_source(&user(), b).unwrap().created);
}
#[test]
fn t4_concurrent_duplicate() {
    let (dir, app) = fixture();
    let app = Arc::new(app);
    let barrier = Arc::new(Barrier::new(32));
    let handles: Vec<_> = (0..32)
        .map(|_| {
            let app = app.clone();
            let b = barrier.clone();
            thread::spawn(move || {
                b.wait();
                app.record_source(&user(), keyed("duplicate", "key"))
                    .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.created).count(), 1);
    assert!(results.iter().all(|r| r.source_id == results[0].source_id));
    assert_eq!(
        count(&Connection::open(dir.path().join("db")).unwrap(), "source"),
        1
    );
}
#[test]
fn t5_concurrent_mixed() {
    let (dir, app) = fixture();
    let app = Arc::new(app);
    let running = Arc::new(AtomicBool::new(true));
    let start = Arc::new(Barrier::new(8));
    let first = Arc::new(std::sync::OnceLock::<SourceId>::new());
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let app = app.clone();
            let running = running.clone();
            let start = start.clone();
            let first = first.clone();
            thread::spawn(move || {
                start.wait();
                let mut reads = 0;
                while running.load(Ordering::Relaxed) {
                    let Some(id) = first.get() else {
                        thread::yield_now();
                        continue;
                    };
                    assert_eq!(
                        app.get_source(&user(), id.clone()).unwrap().content,
                        "write"
                    );
                    reads += 1;
                }
                reads
            })
        })
        .collect();
    let now = std::time::Instant::now();
    let writers: Vec<_> = (0..4)
        .map(|w| {
            let app = app.clone();
            let start = start.clone();
            let first = first.clone();
            thread::spawn(move || {
                start.wait();
                for n in 0..1000 {
                    let recorded = app
                        .record_source(&user(), keyed("write", &format!("{w}:{n}")))
                        .unwrap();
                    let _ = first.set(recorded.source_id);
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    running.store(false, Ordering::Relaxed);
    let reads: usize = readers.into_iter().map(|r| r.join().unwrap()).sum();
    assert!(reads > 0);
    assert_eq!(
        count(&Connection::open(dir.path().join("db")).unwrap(), "source"),
        4000
    );
    assert!(app.verify_integrity().unwrap().ok);
    eprintln!(
        "T5: 4000 writes, {reads} reads, {:?}; BUSY errors=0",
        now.elapsed()
    );
}
#[test]
fn t8_corruption_latches_writes_off() {
    let (dir, app) = fixture();
    app.record_source(&user(), RecordSource::plain("original"))
        .unwrap();
    assert!(app.verify_integrity().unwrap().ok);
    let conn = Connection::open(dir.path().join("db")).unwrap();
    conn.execute_batch("DROP TRIGGER source_no_update; UPDATE source SET content='tampered';")
        .unwrap();
    let report = app.verify_integrity().unwrap();
    assert!(!report.ok);
    assert_eq!(
        report
            .items
            .iter()
            .find(|x| x.name == "source_hashes")
            .unwrap()
            .violations,
        1
    );
    assert_eq!(
        report
            .items
            .iter()
            .find(|x| x.name == "triggers")
            .unwrap()
            .violations,
        1
    );
    assert!(matches!(
        app.record_source(&user(), RecordSource::plain("refuse")),
        Err(Error::IntegrityFailure(_))
    ));
    drop(app);
    assert!(App::open(dir.path().join("db")).is_err());
}
#[test]
fn t9_backup_restore_while_writing() {
    let (dir, app) = fixture();
    let app = Arc::new(app);
    app.record_source(&user(), RecordSource::plain("seed"))
        .unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let writer = {
        let app = app.clone();
        let running = running.clone();
        thread::spawn(move || {
            let mut n = 0;
            while running.load(Ordering::Relaxed) {
                app.record_source(&user(), keyed("ongoing", &n.to_string()))
                    .unwrap();
                n += 1;
                if n == 1 {
                    ready_tx.send(()).unwrap();
                }
                thread::sleep(std::time::Duration::from_millis(1));
            }
            n
        })
    };
    ready_rx.recv().unwrap();
    let backup = dir.path().join("backup");
    app.backup_to(&backup).unwrap();
    running.store(false, Ordering::Relaxed);
    assert!(writer.join().unwrap() > 0);
    assert!(app.backup_to(&backup).is_err());
    let target = dir.path().join("target");
    {
        let old = App::open(&target).unwrap();
        old.record_source(&user(), RecordSource::plain("old target"))
            .unwrap();
        assert!(App::restore_from(&backup, &target).is_err());
    }
    App::restore_from(&backup, &target).unwrap();
    let restored = App::open(&target).unwrap();
    assert!(restored.verify_integrity().unwrap().ok);
    let backup_conn = Connection::open(&backup).unwrap();
    let restored_conn = Connection::open(&target).unwrap();
    fn rows(conn: &Connection) -> Vec<(String, String, String)> {
        conn.prepare("SELECT id,content,content_sha256 FROM source ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }
    assert_eq!(rows(&backup_conn), rows(&restored_conn));
    let replaced = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("target.replaced.")
        })
        .unwrap();
    assert_eq!(
        rows(&Connection::open(replaced).unwrap())[0].1,
        "old target"
    );
    let bad = dir.path().join("corrupt");
    let mut bytes = std::fs::read(&backup).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&bad, bytes).unwrap();
    drop(restored);
    assert!(App::restore_from(&bad, &target).is_err());
    assert_eq!(rows(&backup_conn), rows(&restored_conn));
}
#[test]
fn t10_no_authorship_promotion() {
    let (_dir, app) = fixture();
    let agent = p(PrincipalKind::Agent);
    let mut req = RecordSource::plain("remember this user statement");
    req.retention_request = RetentionRequest::UserRequested;
    let id = app.record_source(&agent, req).unwrap().source_id;
    assert_eq!(
        app.get_source(&agent, id.clone()).unwrap().recorder,
        PrincipalKind::Agent
    );
    assert!(matches!(app.get_source(&user(), id), Err(Error::NotFound)));
}
#[test]
fn t11_lifecycle_permissions_and_noops() {
    let (dir, app) = fixture();
    let agent = p(PrincipalKind::Agent);
    let import = p(PrincipalKind::Import);
    let uid = app
        .record_source(&user(), RecordSource::plain("user"))
        .unwrap()
        .source_id;
    let aid = app
        .record_source(&agent, RecordSource::plain("agent"))
        .unwrap()
        .source_id;
    let iid = app
        .record_source(&import, RecordSource::plain("import"))
        .unwrap()
        .source_id;
    for event in [LifecycleEvent::Retract, LifecycleEvent::Forget] {
        assert!(matches!(
            app.set_source_lifecycle(&agent, uid.clone(), event, None),
            Err(Error::Forbidden)
        ));
    }
    assert!(matches!(
        app.set_source_lifecycle(&import, aid.clone(), LifecycleEvent::Retract, None),
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        app.set_source_lifecycle(&agent, iid.clone(), LifecycleEvent::Retract, None),
        Err(Error::Forbidden)
    ));
    app.set_source_lifecycle(&agent, aid.clone(), LifecycleEvent::Forget, None)
        .unwrap();
    assert!(matches!(
        app.set_source_lifecycle(&agent, aid.clone(), LifecycleEvent::Restore, None),
        Err(Error::NotFound)
    ));
    let conn = Connection::open(dir.path().join("db")).unwrap();
    for (id, actor, event, reason) in [
        (&uid, "agent", "retract", None),
        (&uid, "agent", "forget", None),
        (&uid, "system", "retract", Some("revision")),
        (&aid, "import", "retract", None),
        (&iid, "agent", "retract", None),
        (&aid, "agent", "restore", None),
        (&aid, "system", "forget", Some("revision")),
        (&aid, "system", "retract", None),
    ] {
        assert!(conn.execute("INSERT INTO status_event(target_kind,target_id,event,actor,reason,created_at) VALUES('source',?1,?2,?3,?4,'2026-01-01T00:00:00.000Z')",params![id.as_str(),event,actor,reason]).is_err());
    }
    for id in [uid, aid, iid] {
        for event in [
            LifecycleEvent::Restore,
            LifecycleEvent::Retract,
            LifecycleEvent::Forget,
            LifecycleEvent::Restore,
        ] {
            app.set_source_lifecycle(&user(), id.clone(), event, None)
                .unwrap();
            let before = count(&conn, "status_event");
            app.set_source_lifecycle(&user(), id.clone(), event, None)
                .unwrap();
            assert_eq!(count(&conn, "status_event"), before);
        }
    }
    assert!(app.verify_integrity().unwrap().ok);
}
#[test]
fn t12_cloud_paths_and_single_owner() {
    let dir = tempfile::tempdir().unwrap();
    for folder in ["Mobile Documents", "CloudStorage"] {
        let cloud = dir.path().join("Library").join(folder);
        assert!(matches!(
            App::open(cloud.join("new/db")),
            Err(Error::UnsafeLocation(_))
        ));
        std::fs::create_dir_all(&cloud).unwrap();
        #[cfg(unix)]
        {
            let link = dir.path().join(folder);
            std::os::unix::fs::symlink(&cloud, &link).unwrap();
            assert!(matches!(
                App::open(link.join("db")),
                Err(Error::UnsafeLocation(_))
            ));
        }
    }
    let db = dir.path().join("db");
    let _owner = App::open(&db).unwrap();
    assert!(App::open(&db).is_err());
    #[cfg(unix)]
    {
        let link = dir.path().join("alias");
        std::os::unix::fs::symlink(&db, &link).unwrap();
        assert!(App::open(link).is_err());
    }
}
