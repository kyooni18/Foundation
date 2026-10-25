#![cfg(unix)]
use prime_radiant::{
    App, ListSources,
    authentication::{Credentials, Token},
    domain::*,
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    app: App,
    principals: Vec<Principal>,
    credentials: Credentials,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let registry = root.join("credentials");
        for (i, kind) in [
            PrincipalKind::User,
            PrincipalKind::Agent,
            PrincipalKind::Import,
            PrincipalKind::Agent,
        ]
        .iter()
        .enumerate()
        {
            Credentials::issue(
                &registry,
                root.join(format!("token{i}")),
                *kind,
                Some(format!("client{i}")),
            )
            .unwrap();
        }
        let credentials = Credentials::open(registry).unwrap();
        let principals = (0..4)
            .map(|i| {
                credentials
                    .authenticate_file(root.join(format!("token{i}")))
                    .unwrap()
            })
            .collect();
        Self {
            app: App::open(root.join("db")).unwrap(),
            _dir: dir,
            root,
            principals,
            credentials,
        }
    }
    fn record(&self, role: usize, content: &str) -> SourceId {
        self.app
            .record_source(&self.principals[role], RecordSource::plain(content))
            .unwrap()
            .source_id
    }
}
#[test]
fn complete_core_authorization_matrix() {
    let f = Fixture::new();
    for actor in 0..4 {
        for recorder in 0..3 {
            for state in [Lifecycle::Live, Lifecycle::Retracted, Lifecycle::Forgotten] {
                for event in [
                    LifecycleEvent::Retract,
                    LifecycleEvent::Forget,
                    LifecycleEvent::Restore,
                ] {
                    let id = f.record(recorder, "matrix");
                    let initial = match state {
                        Lifecycle::Live => LifecycleEvent::Restore,
                        Lifecycle::Retracted => LifecycleEvent::Retract,
                        Lifecycle::Forgotten => LifecycleEvent::Forget,
                    };
                    f.app
                        .set_source_lifecycle(&f.principals[0], id.clone(), initial, None)
                        .unwrap();
                    let p = &f.principals[actor];
                    let visible = state == Lifecycle::Live && (actor != 0 || recorder == 0);
                    assert_eq!(
                        f.app.get_source(p, id.clone()).is_ok(),
                        visible,
                        "read {actor}/{recorder}/{state}"
                    );
                    let history_visible = if actor == 0 {
                        recorder == 0 && state == Lifecycle::Live
                    } else {
                        state != Lifecycle::Forgotten
                    };
                    assert_eq!(
                        f.app.get_source_with_options(p, id.clone(), true).is_ok(),
                        history_visible
                    );
                    let result = f.app.set_source_lifecycle(p, id, event, None);
                    if actor == 0
                        || ((actor == 1 || actor == 3)
                            && recorder == 1
                            && state != Lifecycle::Forgotten)
                    {
                        assert!(
                            result.is_ok(),
                            "{actor}/{recorder}/{state}/{event}: {result:?}"
                        );
                    } else if state == Lifecycle::Forgotten {
                        assert!(matches!(result, Err(Error::NotFound)));
                    } else {
                        assert!(matches!(result, Err(Error::Forbidden)));
                    }
                }
            }
        }
    }
    let unknown = "00000000000000000000000000".parse::<SourceId>().unwrap();
    for p in &f.principals {
        assert!(matches!(
            f.app
                .set_source_lifecycle(p, unknown.clone(), LifecycleEvent::Forget, None),
            Err(Error::NotFound)
        ));
    }
    assert!(f.app.verify_integrity().unwrap().ok);
}
#[test]
fn credentials_fail_closed_and_revoke_independently() {
    let f = Fixture::new();
    let token_path = f.root.join("token1");
    let raw = fs::read_to_string(&token_path).unwrap();
    assert!(f.credentials.authenticate(&raw).is_ok());
    for bad in ["", "user", &"f".repeat(64)] {
        assert!(matches!(
            f.credentials.authenticate(bad),
            Err(Error::Unauthenticated)
        ));
    }
    for mode in [0o644, 0o640, 0o666, 0o400, 0o700, 0o4600] {
        fs::set_permissions(&token_path, fs::Permissions::from_mode(mode)).unwrap();
        assert!(matches!(
            Token::from_file(&token_path),
            Err(Error::Credentials)
        ));
    }
    fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600)).unwrap();
    let linked = f.root.join("linked");
    symlink(&token_path, &linked).unwrap();
    assert!(Token::from_file(linked).is_err());
    let hard = f.root.join("hard");
    fs::hard_link(&token_path, &hard).unwrap();
    assert!(Token::from_file(&token_path).is_err());
    fs::remove_file(hard).unwrap();
    assert!(Token::from_file(f.root.join("absent")).is_err());
    assert!(Token::from_file(&f.root).is_err());
    let registry = f.root.join("credentials");
    fs::set_permissions(&registry, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Credentials::open(&registry).is_err());
    assert!(f.credentials.authenticate(&raw).is_err());
    fs::set_permissions(&registry, fs::Permissions::from_mode(0o600)).unwrap();
    let mut data: serde_json::Value =
        serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
    data["entries"].as_array_mut().unwrap().remove(1);
    fs::write(&registry, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(matches!(
        f.credentials.authenticate(&raw),
        Err(Error::Unauthenticated)
    ));
    assert!(
        f.credentials
            .authenticate_file(f.root.join("token3"))
            .is_ok()
    );
    fs::write(&registry, format!("{{bad registry {raw}")).unwrap();
    let error = f.credentials.authenticate(&raw).unwrap_err().to_string();
    assert!(!error.contains(&raw));
}
#[test]
fn pagination_is_projection_scoped_and_credential_bound() {
    let f = Fixture::new();
    let user = &f.principals[0];
    let agent = &f.principals[1];
    for n in 0..7 {
        f.record(0, &format!("visible {n}"));
    }
    let first = f
        .app
        .list_sources(
            user,
            ListSources {
                limit: 2,
                ..Default::default()
            },
        )
        .unwrap();
    let before = serde_json::to_value(&first).unwrap();
    let cursor = first.next_cursor.clone().unwrap();
    for n in 0..12 {
        f.record(1, &format!("hidden {n}"));
    }
    let forgotten = f.record(0, "hidden forgotten");
    f.app
        .set_source_lifecycle(user, forgotten, LifecycleEvent::Forget, None)
        .unwrap();
    let retracted = f.record(0, "hidden retracted");
    f.app
        .set_source_lifecycle(user, retracted, LifecycleEvent::Retract, None)
        .unwrap();
    assert_eq!(
        before,
        serde_json::to_value(
            f.app
                .list_sources(
                    user,
                    ListSources {
                        limit: 2,
                        ..Default::default()
                    }
                )
                .unwrap()
        )
        .unwrap()
    );
    let mut ids = Vec::new();
    let mut next = None;
    loop {
        let page = f
            .app
            .list_sources(
                user,
                ListSources {
                    limit: 2,
                    cursor: next,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(page.total, 7);
        ids.extend(page.sources.into_iter().map(|s| s.id.to_string()));
        next = page.next_cursor;
        if next.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 7);
    assert!(ids.windows(2).all(|p| p[0] < p[1]));
    for p in [&f.principals[1], &f.principals[2]] {
        assert!(matches!(
            f.app.list_sources(
                p,
                ListSources {
                    cursor: Some(cursor.clone()),
                    ..Default::default()
                }
            ),
            Err(Error::InvalidCursor)
        ));
    }
    let agent_cursor = f
        .app
        .list_sources(
            agent,
            ListSources {
                limit: 1,
                ..Default::default()
            },
        )
        .unwrap()
        .next_cursor
        .unwrap();
    assert!(matches!(
        f.app.list_sources(
            &f.principals[3],
            ListSources {
                cursor: Some(agent_cursor),
                ..Default::default()
            }
        ),
        Err(Error::InvalidCursor)
    ));
    let mut tampered = cursor.clone().into_bytes();
    tampered[0] = if tampered[0] == b'0' { b'1' } else { b'0' };
    assert!(matches!(
        f.app.list_sources(
            user,
            ListSources {
                cursor: Some(String::from_utf8(tampered).unwrap()),
                ..Default::default()
            }
        ),
        Err(Error::InvalidCursor)
    ));
    assert!(matches!(
        f.app.list_sources(
            user,
            ListSources {
                cursor: Some(cursor),
                include_retracted: true,
                ..Default::default()
            }
        ),
        Err(Error::InvalidCursor)
    ));
    for limit in [0, 101, u32::MAX] {
        assert!(
            f.app
                .list_sources(
                    user,
                    ListSources {
                        limit,
                        ..Default::default()
                    }
                )
                .is_err()
        );
    }
}
#[test]
fn lifecycle_filtering_and_live_cursor_recheck() {
    let f = Fixture::new();
    let user = &f.principals[0];
    let agent = &f.principals[1];
    let live = f.record(0, "live");
    let retracted = f.record(1, "retracted");
    let forgotten = f.record(1, "forgotten");
    f.app
        .set_source_lifecycle(user, retracted.clone(), LifecycleEvent::Retract, None)
        .unwrap();
    f.app
        .set_source_lifecycle(user, forgotten, LifecycleEvent::Forget, None)
        .unwrap();
    assert_eq!(
        f.app.list_sources(agent, Default::default()).unwrap().total,
        1
    );
    assert_eq!(
        f.app
            .list_sources(
                agent,
                ListSources {
                    include_retracted: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .total,
        2
    );
    assert!(matches!(
        f.app.get_source(agent, retracted.clone()),
        Err(Error::NotFound)
    ));
    assert_eq!(
        f.app
            .get_source_with_options(agent, retracted, true)
            .unwrap()
            .lifecycle,
        Lifecycle::Retracted
    );
    for _ in 0..4 {
        f.record(0, "page");
    }
    let page = f
        .app
        .list_sources(
            user,
            ListSources {
                limit: 1,
                ..Default::default()
            },
        )
        .unwrap();
    let remaining = f
        .app
        .list_sources(
            user,
            ListSources {
                cursor: page.next_cursor.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let victim = remaining.sources.last().unwrap().id.clone();
    f.app
        .set_source_lifecycle(user, victim.clone(), LifecycleEvent::Forget, None)
        .unwrap();
    let next = f
        .app
        .list_sources(
            user,
            ListSources {
                cursor: page.next_cursor,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!next.sources.iter().any(|s| s.id == victim));
    assert_eq!(next.total, 4);
    assert_eq!(
        f.app.get_source(user, live).unwrap().lifecycle,
        Lifecycle::Live
    );
}
#[test]
fn metadata_cannot_become_authority_with_forward_migration() {
    let f = Fixture::new();
    let agent = &f.principals[1];
    let mut r = RecordSource::plain("user said this");
    r.retention_request = RetentionRequest::UserRequested;
    r.idempotency_key = Some("shared-role-key".into());
    let first = f.app.record_source(agent, r.clone()).unwrap();
    assert!(
        !f.app
            .record_source(&f.principals[3], r.clone())
            .unwrap()
            .created
    );
    let user_record = f.app.record_source(&f.principals[0], r).unwrap();
    assert_ne!(first.source_id, user_record.source_id);
    assert!(matches!(
        f.app.get_source(&f.principals[0], first.source_id.clone()),
        Err(Error::NotFound)
    ));
    let view = f.app.get_source(agent, first.source_id).unwrap();
    assert_eq!(view.recorder, PrincipalKind::Agent);
    assert_eq!(view.recorder_label.as_deref(), Some("client1"));
    let conn = rusqlite::Connection::open(f.root.join("db")).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM schema_migration", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        3
    );
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        json!(tables),
        json!([
            "claim",
            "claim_dependency",
            "claim_validity",
            "entity",
            "entity_alias",
            "entity_resolution",
            "extraction_candidate",
            "extraction_request",
            "extraction_run",
            "identity_assertion",
            "identity_blocked",
            "job",
            "mention",
            "mention_resolution",
            "model_judgment",
            "relation",
            "revalidation_request",
            "revalidation_result",
            "schema_migration",
            "session_read",
            "source",
            "source_citation",
            "sqlite_sequence",
            "status_event"
        ])
    );
}
