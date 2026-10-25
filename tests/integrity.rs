//! Phase 4 knowledge integrity: validity semantics, revalidation jobs, provenance, identity,
//! temporal reads and the randomized incremental-vs-oracle property.
#[path = "support/synthetic.rs"]
mod synthetic;
use prime_radiant::{
    App, Policy,
    domain::*,
    extraction::{Plan, hash},
    integrity::{IdentityPolicy, Target, Validity},
    providers::{FakeProvider, ModelConfig},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use synthetic::{ClaimSpec, Rng, SourceSpec};

fn p(k: PrincipalKind) -> Principal {
    Principal::for_local_cli(k, None, None)
}
fn user() -> Principal {
    p(PrincipalKind::User)
}
fn agent() -> Principal {
    p(PrincipalKind::Agent)
}
fn fresh() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    drop(App::open(&path).unwrap());
    (dir, path)
}
fn spec<'a>(recorder: &'a str, texts: &[&str]) -> SourceSpec<'a> {
    SourceSpec {
        recorder,
        session: None,
        captured_at: "2026-01-01T00:00:00.000Z",
        authored_at: None,
        claims: texts.iter().map(|t| ClaimSpec::plain(*t)).collect(),
    }
}
/// (state, reasons, deps_unknown) straight from the derived table.
fn state(path: &Path, id: &str) -> (String, String, bool) {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    conn.query_row(
        "SELECT state,reasons,deps_unknown FROM claim_validity WHERE claim_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}
fn short(path: &Path, id: &str) -> String {
    let (s, reasons, _) = state(path, id);
    if reasons.is_empty() {
        s
    } else {
        format!("{s}:{reasons}")
    }
}
fn count(path: &Path, sql: &str) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}
fn plan() -> Plan {
    let model = |m: &str| ModelConfig {
        max_output_tokens: None,
        model: m.into(),
        timeout_seconds: 2,
    };
    Plan {
        prompt_version: None,
        provider: "fake".into(),
        variant: "audited".into(),
        extractor: model("fake"),
        reviewer: model("independent-fake"),
    }
}
fn candidate(text: &str, speaker: &str) -> Value {
    json!({"text":text,"lang":"en","category":"Fact","speaker":speaker,"speaker_label":null,"basis":"verbatim","anchor":{"quote":text,"occurrence":0},"tkind":null,"applies_from":null,"applies_to":null,"standing":false,"index_fields":null,"revision_of":null,"mentions":[],"dependencies":[]})
}
fn fake(raw: &str, n: usize) -> FakeProvider {
    let review = json!({"verdict":"pass","issues":[],"checked":(0..n).collect::<Vec<_>>(),"coverage_complete":true}).to_string();
    FakeProvider(std::sync::Mutex::new(
        [
            Ok(raw.into()),
            Ok(review.clone()),
            Ok(review.clone()),
            Ok(review),
        ]
        .into(),
    ))
}
/// Publish an already-recorded Source's pending job through the real approval boundary.
async fn approve(app: &App, claims: Vec<Value>) -> Vec<String> {
    let n = claims.len();
    let raw = json!({"empty_reason":null,"claims":claims}).to_string();
    let candidate = app
        .work_once(&fake(&raw, n), &plan())
        .await
        .unwrap()
        .unwrap();
    let mut ids = app
        .approve_extraction(&user(), &candidate, &hash(&raw))
        .unwrap();
    // approve_extraction returns sorted IDs; map back to candidate order by text.
    let conn_text = |id: &String| app.get_claim(&p(PrincipalKind::Agent), id).map(|c| c.text);
    let order: Vec<String> = claims
        .iter()
        .map(|c| c["text"].as_str().unwrap().to_string())
        .collect();
    ids.sort_by_key(|id| {
        let text = conn_text(id).unwrap_or_default();
        order.iter().position(|t| *t == text).unwrap_or(usize::MAX)
    });
    ids
}
async fn publish(app: &App, who: &Principal, texts: &[&str]) -> Vec<String> {
    app.record_source(who, RecordSource::plain(texts.join(" ")))
        .unwrap();
    let speaker = if who.kind() == PrincipalKind::User {
        "user"
    } else {
        "agent"
    };
    approve(app, texts.iter().map(|t| candidate(t, speaker)).collect()).await
}

#[test]
fn dependency_semantics_lifecycles_and_revalidation_jobs() {
    let (_dir, path) = fresh();
    let conn = synthetic::connect(&path);
    let (s0, a) = synthetic::source(&conn, &spec("user", &["claim a."]));
    let (_s1, rest) = synthetic::source(
        &conn,
        &spec(
            "user",
            &[
                "claim b.", "claim c.", "claim d.", "claim e.", "claim f.", "claim g.",
            ],
        ),
    );
    let (s2, h) = synthetic::source(&conn, &spec("user", &["claim h."]));
    let (_s3, x) = synthetic::source(&conn, &spec("agent", &["claim x."]));
    drop(conn);
    let app = App::open(&path).unwrap();
    let (a, h, x) = (a[0].clone(), h[0].clone(), x[0].clone());
    let [b, c, d, e, f, g] = <[String; 6]>::try_from(rest).unwrap();
    let u = user();
    let dep_ba = app.add_dependency(&u, &b, &a, "premise").unwrap();
    app.add_dependency(&u, &c, &b, "restates").unwrap();
    app.add_dependency(&u, &d, &a, "alternative").unwrap();
    app.add_dependency(&u, &d, &h, "alternative").unwrap();
    app.add_dependency(&u, &e, &a, "informed_by").unwrap();
    app.add_dependency(&u, &f, &e, "premise").unwrap();
    app.add_dependency(&u, &g, &d, "premise").unwrap();
    let all = [&a, &b, &c, &d, &e, &f, &g, &h, &x];
    let snapshot = |ids: &[&String]| ids.iter().map(|i| short(&path, i)).collect::<Vec<_>>();
    assert!(snapshot(&all).iter().all(|s| s == "valid"));
    // Idempotent declaration returns the same edge.
    assert_eq!(app.add_dependency(&u, &b, &a, "premise").unwrap(), dep_ba);

    // Mandatory propagation, sufficient alternatives and advisory uncertainty.
    app.set_claim_status(&u, &a, "retract").unwrap();
    assert_eq!(
        snapshot(&[&a, &b, &c, &d, &e, &f, &g]),
        [
            "invalid:retracted",
            "invalid:premise_invalid",
            "invalid:restates_invalid",
            "valid",
            "needs_revalidation:advisory_invalid",
            "needs_revalidation:premise_uncertain",
            "valid"
        ]
    );
    // Entering needs_revalidation scheduled exactly e and f, once each.
    assert_eq!(
        count(&path, "SELECT count(*) FROM job WHERE kind='revalidate'"),
        2
    );
    app.set_claim_status(&u, &h, "retract").unwrap();
    assert_eq!(
        snapshot(&[&d, &g]),
        ["invalid:alternatives_invalid", "invalid:premise_invalid"]
    );
    let by_claims = snapshot(&[&b, &c, &d, &e, &f, &g]);
    // Source retraction is the same mechanism as retracting its Claims.
    app.set_claim_status(&u, &a, "restore").unwrap();
    app.set_claim_status(&u, &h, "restore").unwrap();
    assert!(snapshot(&all).iter().all(|s| s == "valid"));
    app.set_source_lifecycle(&u, s0.parse().unwrap(), LifecycleEvent::Retract, None)
        .unwrap();
    app.set_source_lifecycle(&u, s2.parse().unwrap(), LifecycleEvent::Retract, None)
        .unwrap();
    assert_eq!(snapshot(&[&b, &c, &d, &e, &f, &g]), by_claims);
    assert_eq!(short(&path, &a), "invalid:source_retracted");
    app.set_source_lifecycle(&u, s0.parse().unwrap(), LifecycleEvent::Restore, None)
        .unwrap();
    app.set_source_lifecycle(&u, s2.parse().unwrap(), LifecycleEvent::Restore, None)
        .unwrap();
    assert!(snapshot(&all).iter().all(|s| s == "valid"));

    // An uncertain alternative keeps its dependent uncertain, never invalid.
    app.add_dependency(&u, &h, &x, "informed_by").unwrap();
    app.set_claim_status(&u, &x, "retract").unwrap();
    app.set_claim_status(&u, &a, "retract").unwrap();
    assert_eq!(
        snapshot(&[&h, &d, &g]),
        [
            "needs_revalidation:advisory_invalid",
            "needs_revalidation:alternatives_uncertain",
            "needs_revalidation:premise_uncertain"
        ]
    );
    app.set_claim_status(&u, &x, "restore").unwrap();
    app.set_claim_status(&u, &a, "restore").unwrap();
    assert!(snapshot(&all).iter().all(|s| s == "valid"));

    // Self-dependencies and cycles are rejected, including through retracted edges.
    for (child, parent) in [(&a, &a), (&a, &c), (&a, &b)] {
        assert!(matches!(
            app.add_dependency(&u, child, parent, "premise"),
            Err(Error::InvalidInput { .. })
        ));
    }
    app.set_integrity_lifecycle(
        &u,
        Target::Dependency,
        &dep_ba,
        LifecycleEvent::Retract,
        None,
    )
    .unwrap();
    assert!(app.add_dependency(&u, &a, &b, "premise").is_err());
    // Retracting an edge removes its effect; restoring it reinstates it.
    app.set_claim_status(&u, &a, "retract").unwrap();
    assert_eq!(short(&path, &b), "valid");
    app.set_integrity_lifecycle(
        &u,
        Target::Dependency,
        &dep_ba,
        LifecycleEvent::Restore,
        None,
    )
    .unwrap();
    assert_eq!(short(&path, &b), "invalid:premise_invalid");
    // A new dependency on an already-retracted parent is allowed and immediately invalid.
    let late = app.add_dependency(&u, &x, &a, "premise").unwrap();
    assert_eq!(short(&path, &x), "invalid:premise_invalid");
    app.set_claim_status(&u, &a, "restore").unwrap();

    // Forget then restore: only the user may restore a forgotten target.
    app.set_claim_status(&u, &a, "forget").unwrap();
    assert_eq!(short(&path, &b), "invalid:premise_invalid");
    assert!(app.set_claim_status(&agent(), &a, "restore").is_err());
    app.set_claim_status(&u, &a, "restore").unwrap();
    assert!(snapshot(&all).iter().all(|s| s == "valid"));
    // Agents cannot change user-declared edges; they manage edges on their own Claims.
    assert!(matches!(
        app.set_integrity_lifecycle(
            &agent(),
            Target::Dependency,
            &late,
            LifecycleEvent::Retract,
            None
        ),
        Err(Error::Forbidden)
    ));
    // x -> h would close a cycle through h informed_by x; agents get the same rejection.
    assert!(app.add_dependency(&agent(), &x, &h, "informed_by").is_err());
    let own = app.add_dependency(&agent(), &x, &b, "informed_by").unwrap();
    assert!(matches!(
        app.add_dependency(&agent(), &b, &x, "premise"),
        Err(Error::Forbidden)
    ));
    app.set_integrity_lifecycle(
        &agent(),
        Target::Dependency,
        &own,
        LifecycleEvent::Retract,
        None,
    )
    .unwrap();
    app.set_integrity_lifecycle(&u, Target::Dependency, &own, LifecycleEvent::Forget, None)
        .unwrap();
    assert!(matches!(
        app.set_integrity_lifecycle(
            &agent(),
            Target::Dependency,
            &own,
            LifecycleEvent::Restore,
            None
        ),
        Err(Error::NotFound)
    ));
    assert!(app.validity_mismatches().unwrap().is_empty());

    // Durable revalidation: e is uncertain again; drain and inspect outcomes.
    app.set_claim_status(&u, &a, "retract").unwrap();
    let t = |s: &str| s.parse::<Timestamp>().unwrap();
    let mut outcomes = BTreeMap::new();
    // Lease expiry: owner one stalls, owner two re-leases; one's late result is fenced.
    let stalled = app
        .lease_revalidation(
            "one",
            &t("2090-01-01T00:00:00Z"),
            &t("2090-01-01T00:01:00Z"),
        )
        .unwrap()
        .unwrap();
    let retaken = app
        .lease_revalidation(
            "two",
            &t("2090-01-01T00:02:00Z"),
            &t("2091-01-01T00:00:00Z"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        (stalled.id.as_str(), retaken.attempt),
        (retaken.id.as_str(), 2)
    );
    assert!(app.complete_revalidation(&stalled).is_err());
    let first = app.complete_revalidation(&retaken).unwrap();
    // Duplicate delivery of the same lease is a no-op returning the recorded result.
    assert_eq!(app.complete_revalidation(&retaken).unwrap(), first);
    // The oldest job (e or f) ran while its advisory parent was invalid: no model is
    // called, so the semantic question is recorded as pending, not answered.
    assert!([&e, &f].contains(&&first.claim_id));
    assert_eq!(first.outcome, "semantic_pending");
    assert_eq!(first.validity, Validity::NeedsRevalidation);
    // Once support is restored, the remaining jobs resolve by deterministic recomputation.
    app.set_claim_status(&u, &a, "restore").unwrap();
    while let Some(done) = app.revalidate_once("drain").unwrap() {
        outcomes.insert(done.claim_id.clone(), done.outcome);
    }
    assert!(outcomes.len() >= 4, "{outcomes:?}");
    assert!(outcomes.values().all(|o| o == "resolved"), "{outcomes:?}");
    assert_eq!(
        count(&path, "SELECT count(*) FROM revalidation_result"),
        count(
            &path,
            "SELECT count(*) FROM job WHERE kind='revalidate' AND state='done'"
        )
    );
    assert_eq!(
        count(
            &path,
            "SELECT count(*) FROM job WHERE kind='revalidate' AND state<>'done'"
        ),
        0
    );
    assert!(app.validity_mismatches().unwrap().is_empty());
    assert!(app.verify_integrity().unwrap().ok);
}

#[tokio::test]
async fn session_reads_and_source_based_support() {
    let (_dir, path) = fresh();
    let conn = synthetic::connect(&path);
    let (_, u) = synthetic::source(&conn, &spec("user", &["the user note.", "a second note."]));
    let (sp, p1) = synthetic::source(&conn, &spec("user", &["origin statement p."]));
    let (sr, p3) = synthetic::source(&conn, &spec("user", &["origin statement r."]));
    let (_, p2) = synthetic::source(&conn, &spec("user", &["restating p and r."]));
    let (_, ag) = synthetic::source(&conn, &spec("agent", &["agent restates p."]));
    drop(conn);
    let app = App::open(&path).unwrap();
    let u0 = u[0].clone();
    let session = agent().with_session(Some("s1".into())).unwrap();
    // The session reads u0, then records an agent Source without declaring dependencies.
    app.get_claim(&session, &u0).unwrap();
    let c = publish(&app, &session, &["derived agent statement."]).await[0].clone();
    assert_eq!(state(&path, &c), ("valid".into(), String::new(), true));
    // Reads after capture are irrelevant; reads by users and sessionless agents are not tracked.
    app.get_claim(&session, &u[1]).unwrap();
    app.get_claim(&user(), &u[1]).unwrap();
    assert_eq!(count(&path, "SELECT count(*) FROM session_read"), 2);
    app.set_claim_status(&user(), &u[1], "retract").unwrap();
    assert_eq!(short(&path, &c), "valid");
    // A relevant read becomes invalid: uncertainty, not a proven dependency or invalidity.
    app.set_claim_status(&user(), &u0, "retract").unwrap();
    assert_eq!(short(&path, &c), "needs_revalidation:session_read_invalid");
    assert_eq!(
        count(
            &path,
            "SELECT count(*) FROM revalidation_request WHERE cause='session_read'"
        ),
        1
    );
    let agent_view = app.get_claim(&agent(), &c).unwrap();
    assert_eq!(agent_view.deps_unknown, Some(true));
    assert_eq!(
        agent_view.validity_reasons.as_deref(),
        Some("session_read_invalid")
    );
    // Declaring the real dependency replaces the undeclared-dependency derivation.
    app.add_dependency(&agent(), &c, &p1[0], "informed_by")
        .unwrap();
    assert_eq!(state(&path, &c), ("valid".into(), String::new(), false));
    app.set_claim_status(&user(), &u0, "restore").unwrap();

    // Support counts distinct origin Sources through live restatements, never Claim rows.
    let support = |who: &Principal, id: &str| {
        let s = app.claim_support(who, id).expect(id);
        (s.sources, s.independence_unknown, s.claims)
    };
    app.add_dependency(&user(), &p2[0], &p1[0], "restates")
        .unwrap();
    app.add_dependency(&user(), &p2[0], &p3[0], "restates")
        .unwrap();
    app.add_dependency(&agent(), &ag[0], &p1[0], "restates")
        .unwrap();
    assert_eq!(support(&agent(), &p2[0]), (2, 0, 4));
    // The user projection does not contain the agent restatement.
    assert_eq!(support(&user(), &p2[0]), (2, 0, 3));
    // A citation collapses R into P's origin; it is provenance, not a validity dependency.
    let cite = app
        .add_citation(&user(), &sr.parse().unwrap(), &sp.parse().unwrap())
        .unwrap();
    assert_eq!(support(&agent(), &p2[0]), (1, 0, 4));
    app.set_source_lifecycle(&user(), sp.parse().unwrap(), LifecycleEvent::Retract, None)
        .unwrap();
    // The citing Source's Claim stays valid; the restatement of P does not.
    assert_eq!(short(&path, &p3[0]), "valid");
    assert_eq!(short(&path, &p2[0]), "invalid:restates_invalid");
    assert!(matches!(
        app.claim_support(&user(), &p2[0]),
        Err(Error::NotFound)
    ));
    // The user projection cannot follow the citation to a retracted Source; agents can.
    assert_eq!(support(&user(), &p3[0]), (1, 0, 1));
    assert_eq!(support(&agent(), &p3[0]), (1, 0, 1));
    app.set_source_lifecycle(&user(), sp.parse().unwrap(), LifecycleEvent::Restore, None)
        .unwrap();
    app.set_integrity_lifecycle(
        &user(),
        Target::Citation,
        &cite,
        LifecycleEvent::Retract,
        None,
    )
    .unwrap();
    assert_eq!(support(&agent(), &p2[0]), (2, 0, 4));
    // Advisory dependencies mark origin independence as unknown.
    app.add_dependency(&user(), &p3[0], &u0, "informed_by")
        .unwrap();
    assert_eq!(support(&agent(), &p2[0]), (2, 1, 4));
    // Claims citing a forgotten Source are opaque to agents, not independent.
    let cite = app
        .add_citation(&user(), &sr.parse().unwrap(), &sp.parse().unwrap())
        .unwrap();
    app.set_source_lifecycle(&user(), sp.parse().unwrap(), LifecycleEvent::Forget, None)
        .unwrap();
    assert_eq!(support(&agent(), &p3[0]), (1, 1, 1));
    assert!(matches!(
        app.claim_support(&agent(), &p1[0]),
        Err(Error::NotFound)
    ));
    // Agents cannot cite on behalf of user Sources.
    assert!(matches!(
        app.add_citation(&agent(), &sr.parse().unwrap(), &sp.parse().unwrap()),
        Err(Error::NotFound)
    ));
    let _ = cite;
    assert!(app.validity_mismatches().unwrap().is_empty());
}

#[test]
fn identity_resolution_is_precedence_ordered_reversible_and_durable() {
    let (dir, path) = fresh();
    let conn = synthetic::connect(&path);
    let (_, c1) = synthetic::source(&conn, &spec("user", &["Minsu lives in Mapo."]));
    let (_, c2) = synthetic::source(&conn, &spec("agent", &["Minsu joined the team."]));
    let m1 = synthetic::mention(&conn, &c1[0], "Minsu");
    let m2 = synthetic::mention(&conn, &c2[0], "Minsu");
    let job2: String = conn
        .query_row("SELECT job_id FROM claim WHERE id=?1", [&c2[0]], |r| {
            r.get(0)
        })
        .unwrap();
    drop(conn);
    let app = App::open(&path).unwrap();
    let (u, a) = (user(), agent());
    let e: Vec<String> = (0..5)
        .map(|_| app.create_entity(&u, "person").unwrap())
        .collect();
    let rep = |who: &Principal, id: &str| app.get_entity(who, id).unwrap().representative;
    // Sharing a name or alias never merges entities.
    app.assert_identity(&u, "alias", &e[0], "Minsu", None)
        .unwrap();
    app.assert_identity(&u, "alias", &e[3], "Minsu", None)
        .unwrap();
    assert_ne!(rep(&a, &e[0]), rep(&a, &e[3]));
    // refers_to precedence: user over agent.
    app.assert_identity(&u, "refers_to", &m1, &e[0], None)
        .unwrap();
    app.assert_identity(&a, "refers_to", &m1, &e[3], None)
        .unwrap();
    let mention_entity = |who: &Principal, claim: &str| {
        app.get_claim(who, claim).unwrap().mentions.unwrap()[0]
            .entity
            .clone()
    };
    assert_eq!(mention_entity(&a, &c1[0]), Some(rep(&a, &e[0])));
    // Transitive distinct_from protection: e0~e1 (user), e1~e2 (agent), e0 != e2 (user).
    let user_same = app
        .assert_identity(&u, "same_as", &e[1], &e[0], None)
        .unwrap();
    let agent_same = app
        .assert_identity(&a, "same_as", &e[1], &e[2], None)
        .unwrap();
    app.assert_identity(&u, "distinct_from", &e[0], &e[2], None)
        .unwrap();
    let view = app.get_entity(&a, &e[1]).unwrap();
    assert_eq!(view.members, vec![e[0].clone(), e[1].clone()]);
    assert!(view.blocked.as_ref().unwrap().contains(&agent_same));
    // An agent distinct_from cannot split a user merge.
    let agent_distinct = app
        .assert_identity(&a, "distinct_from", &e[0], &e[1], None)
        .unwrap();
    assert!(
        app.get_entity(&a, &e[0])
            .unwrap()
            .blocked
            .unwrap()
            .contains(&agent_distinct)
    );
    assert_eq!(rep(&a, &e[1]), rep(&a, &e[0]));
    // Reversible: retracting the user merge reconstructs resolution from assertions.
    app.set_integrity_lifecycle(
        &u,
        Target::IdentityAssertion,
        &user_same,
        LifecycleEvent::Retract,
        None,
    )
    .unwrap();
    assert_eq!(
        app.get_entity(&a, &e[0]).unwrap().members,
        vec![e[0].clone()]
    );
    assert_eq!(
        app.get_entity(&a, &e[1]).unwrap().members,
        vec![e[1].clone(), e[2].clone()]
    );
    app.set_integrity_lifecycle(
        &u,
        Target::IdentityAssertion,
        &user_same,
        LifecycleEvent::Restore,
        None,
    )
    .unwrap();
    assert_eq!(
        app.get_entity(&a, &e[1]).unwrap().members,
        vec![e[0].clone(), e[1].clone()]
    );
    // Agents cannot retract user assertions; the database refuses system attempts too.
    assert!(matches!(
        app.set_integrity_lifecycle(
            &a,
            Target::IdentityAssertion,
            &user_same,
            LifecycleEvent::Retract,
            None
        ),
        Err(Error::Forbidden)
    ));
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert!(raw.execute("INSERT INTO status_event(target_kind,target_id,event,actor,reason,created_at) VALUES('identity_assertion',?1,'retract','system','revision','2026-01-01T00:00:00.000Z')",[&user_same]).is_err());
    assert!(raw.execute("INSERT INTO identity_assertion(id,kind,subject,object,actor,created_at) VALUES('X','same_as',?1,?2,'model','2026-01-01T00:00:00.000Z')",[&e[3],&e[4]]).is_err());
    drop(raw);
    // Model suggestions are stored but not applied by default.
    let suggestion = app
        .suggest_identity(&job2, "same_as", &e[3], &e[4])
        .unwrap();
    let new_entity = app
        .suggest_identity(&job2, "refers_to", &m2, "new:person")
        .unwrap();
    assert!(
        app.suggest_identity(&job2, "refers_to", &m1, &e[0])
            .is_err()
    );
    let e3 = app.get_entity(&a, &e[3]).unwrap();
    assert_eq!(e3.members, vec![e[3].clone()]);
    assert!(e3.suggestions.unwrap().contains(&suggestion));
    assert_eq!(mention_entity(&a, &c2[0]), None);
    let agents_view: Vec<_> = e.iter().map(|id| app.get_entity(&a, id).unwrap()).collect();
    // User projection: only user assertions and user-projected mentions.
    app.assert_identity(&a, "alias", &e[0], "agent nickname", None)
        .unwrap();
    app.assert_identity(&a, "refers_to", &m2, &e[0], None)
        .unwrap();
    let mine = app.get_entity(&u, &e[0]).unwrap();
    assert_eq!(mine.members, vec![e[0].clone(), e[1].clone()]);
    assert!(mine.suggestions.is_none() && mine.blocked.is_none());
    let texts: Vec<_> = mine
        .aliases
        .iter()
        .map(|x| (x.origin.as_str(), x.text.as_str()))
        .collect();
    assert_eq!(texts, [("assertion", "Minsu"), ("mention", "Minsu")]);
    assert_eq!(mine.mentions.len(), 1);
    assert_eq!(mine.mentions[0].claim_id, c1[0]);
    let theirs = app.get_entity(&a, &e[0]).unwrap();
    assert!(theirs.aliases.iter().any(|x| x.text == "agent nickname"));
    assert_eq!(theirs.mentions.len(), 2);
    let agent_entity = app.create_entity(&a, "place").unwrap();
    app.assert_identity(&a, "alias", &agent_entity, "secret place", None)
        .unwrap();
    assert!(matches!(
        app.get_entity(&u, &agent_entity),
        Err(Error::NotFound)
    ));
    assert!(app.get_entity(&a, &agent_entity).is_ok());
    // Identity changes schedule revalidation of affected Claims (semantic, not deterministic).
    assert!(
        count(
            &path,
            "SELECT count(*) FROM revalidation_request WHERE cause='identity'"
        ) >= 1
    );
    // A policy that admits model same_as merges; reopening with the default splits again.
    let snapshot =
        |app: &App| -> Vec<_> { e.iter().map(|id| app.get_entity(&a, id).unwrap()).collect() };
    let before = snapshot(&app);
    drop(app);
    let permissive = Policy {
        identity: IdentityPolicy {
            model_same_as: true,
            model_refers_to: true,
        },
        ..Policy::default()
    };
    let app = App::open_with_policy(&path, permissive).unwrap();
    assert_eq!(
        app.get_entity(&a, &e[4]).unwrap().representative,
        app.get_entity(&a, &e[3]).unwrap().representative
    );
    assert!(
        app.get_claim(&a, &c2[0]).unwrap().mentions.unwrap()[0]
            .entity
            .is_some()
    );
    drop(app);
    // Restart, rebuild and backup/restore all reproduce the same derived resolution.
    let app = App::open(&path).unwrap();
    assert_eq!(snapshot(&app), before);
    app.rebuild_derived(&u).unwrap();
    assert_eq!(snapshot(&app), before);
    assert_ne!(before, agents_view);
    let backup = dir.path().join("identity.bak");
    app.backup_to(&backup).unwrap();
    let restored = dir.path().join("restored.db");
    App::restore_from(&backup, &restored).unwrap();
    let copy = App::open(&restored).unwrap();
    assert_eq!(snapshot(&copy), before);
    assert!(copy.verify_integrity().unwrap().ok);
    let _ = new_entity;
}

#[tokio::test]
async fn reextraction_keeps_retractions_validity_and_manual_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let app = App::open(&path).unwrap();
    let u = user();
    let source = app
        .record_source(&u, RecordSource::plain("We meet on Thursday."))
        .unwrap()
        .source_id;
    let mut wrong = candidate("We meet on Thursday.", "user");
    wrong["mentions"] = json!([{"surface":"Thursday","occurrence":0}]);
    let v1 = approve(&app, vec![wrong]).await[0].clone();
    let k = publish(&app, &u, &["The plan needs that meeting."]).await[0].clone();
    app.add_dependency(&u, &k, &v1, "premise").unwrap();
    let mention = app.get_claim(&agent(), &v1).unwrap().mentions.unwrap()[0]
        .id
        .clone();
    let job1: String = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT job_id FROM claim WHERE id=?1", [&v1], |r| r.get(0))
        .unwrap();
    let day = app.create_entity(&u, "event").unwrap();
    let manual = app
        .assert_identity(&u, "refers_to", &mention, &day, None)
        .unwrap();
    let model = app.suggest_identity(&job1, "alias", &day, "Thu").unwrap();
    // The first interpretation was wrong: retract it; its dependent loses support.
    app.set_claim_status(&u, &v1, "retract").unwrap();
    assert_eq!(short(&path, &k), "invalid:premise_invalid");
    app.request_extraction(&u, source, "fix").unwrap();
    let mut fixed = candidate("We meet on Thursday.", "user");
    fixed["revision_of"] = json!(v1);
    let v2 = approve(&app, vec![fixed]).await[0].clone();
    // revision_of is a version link, not a premise: no validity is inherited either way.
    assert_eq!(short(&path, &v2), "valid");
    assert_eq!(short(&path, &v1), "invalid:retracted");
    assert_eq!(short(&path, &k), "invalid:premise_invalid");
    assert!(app.get_claim(&u, &v1).is_err());
    // Re-extraction retracts the earlier model suggestion; the manual correction survives.
    let conn = rusqlite::Connection::open(&path).unwrap();
    let lifecycle = |id: &str| -> (String, String, Option<String>) {
        conn.query_row("SELECT event,actor,reason FROM status_event WHERE target_kind='identity_assertion' AND target_id=?1 ORDER BY seq DESC LIMIT 1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap_or(("restore".into(), "none".into(), None))
    };
    assert_eq!(
        lifecycle(&model),
        ("retract".into(), "system".into(), Some("revision".into()))
    );
    assert_eq!(lifecycle(&manual).0, "restore");
    // The corrected mention belongs to the retracted Claim: hidden by default, still resolved.
    let resolved = app.get_entity(&agent(), &day).unwrap();
    assert!(resolved.mentions.is_empty());
    assert!(resolved.suggestions.unwrap().is_empty());
    let history = app.get_claim_with_options(&agent(), &v1, true).unwrap();
    assert_eq!(
        history.mentions.unwrap()[0].entity.as_deref(),
        Some(day.as_str())
    );
    assert!(app.validity_mismatches().unwrap().is_empty());
}

/// Foundation-Research R2 C dev set (experiments/phase2/c_temporal/data.py). Temporal fields
/// follow its extraction conventions; supersession/exception/backfill are the gold relations
/// asserted by the user instead of model judgments.
#[test]
fn as_of_reproduces_r2_history_examples_and_preserves_history() {
    type S<'a> = (
        &'a str,
        &'a str,
        &'a str,
        Option<&'a str>,
        Option<&'a str>,
        bool,
    );
    let statements: [S; 21] = [
        ("w1", "2026-03-01", "habit", None, None, false),
        ("w2", "2026-07-10", "habit", None, None, false),
        ("a1", "2026-04-02", "state", None, None, false),
        ("a2", "2026-09-01", "event", None, None, false),
        (
            "b1",
            "2026-10-01",
            "report_past",
            None,
            Some("2026-09-24"),
            false,
        ),
        (
            "c1",
            "2026-10-10",
            "decision",
            Some("2026-10-11"),
            None,
            false,
        ),
        ("x1", "2026-09-20", "observation", None, None, true),
        ("x2", "2026-10-05", "event", None, None, false),
        ("m1", "2026-01-10", "state", None, None, false),
        ("k1", "2026-02-03", "observation", None, None, false),
        ("v1", "2026-02-01", "state", Some("2026-02-01"), None, false),
        ("v2", "2026-06-01", "state", None, None, false),
        ("v3", "2026-09-01", "state", None, None, false),
        ("h1", "2026-03-01", "state", None, None, false),
        (
            "h2",
            "2026-10-15",
            "report_past",
            Some("2025-06-01"),
            Some("2025-08-31"),
            false,
        ),
        ("t1", "2026-04-01", "habit", None, None, false),
        (
            "t2",
            "2026-10-14",
            "event",
            Some("2026-10-14"),
            Some("2026-10-14"),
            false,
        ),
        ("g1", "2026-02-01", "habit", None, None, false),
        (
            "g2",
            "2026-10-19",
            "habit",
            Some("2026-10-19"),
            Some("2026-10-25"),
            false,
        ),
        ("p1", "2026-01-02", "state", None, None, false),
        ("p2", "2026-08-20", "event", None, None, false),
    ];
    let timelines: BTreeMap<&str, Vec<&str>> = [
        ("wake", vec!["w1", "w2"]),
        ("projA", vec!["a1", "a2"]),
        ("editorB", vec!["b1"]),
        ("toolC", vec!["c1"]),
        ("printer", vec!["x1", "x2"]),
        ("minsu_home", vec!["m1"]),
        ("parking", vec!["k1"]),
        ("veg", vec!["v1", "v2", "v3"]),
        ("home", vec!["h1", "h2"]),
        ("commute", vec!["t1", "t2"]),
        ("meeting", vec!["g1", "g2"]),
        ("phone", vec!["p1", "p2"]),
    ]
    .into();
    let relations = [
        ("supersedes", "w2", "w1"),
        ("supersedes", "a2", "a1"),
        ("supersedes", "x2", "x1"),
        ("supersedes", "v2", "v1"),
        ("supersedes", "v3", "v2"),
        ("supersedes", "p2", "p1"),
        ("exception", "t2", "t1"),
        ("exception", "g2", "g1"),
        ("backfill", "h2", "h1"),
    ];
    type Q<'a> = (&'a str, &'a str, &'a str, &'a [&'a str], bool);
    let queries: [Q; 22] = [
        ("q_wake_now", "wake", "2026-10-21", &["w2"], true),
        ("q_wake_may", "wake", "2026-05-01", &["w1"], false),
        ("q_projA_now", "projA", "2026-10-21", &["a2"], false),
        ("q_projA_jun", "projA", "2026-06-01", &["a1"], false),
        ("q_editor_sep", "editorB", "2026-09-15", &["b1"], false),
        ("q_editor_now", "editorB", "2026-10-21", &["b1"], false),
        ("q_toolC_before", "toolC", "2026-10-05", &[], false),
        ("q_toolC_now", "toolC", "2026-10-21", &["c1"], true),
        ("q_printer_now", "printer", "2026-10-21", &["x2"], false),
        ("q_printer_sep", "printer", "2026-09-25", &["x1"], true),
        ("q_minsu_now", "minsu_home", "2026-10-21", &["m1"], true),
        ("q_parking_now", "parking", "2026-10-21", &["k1"], true),
        ("q_veg_now", "veg", "2026-10-21", &["v3"], false),
        ("q_veg_jul", "veg", "2026-07-01", &["v2"], false),
        ("q_veg_mar", "veg", "2026-03-01", &["v1"], false),
        ("q_home_now", "home", "2026-10-21", &["h1"], true),
        ("q_home_2025", "home", "2025-08-01", &["h2"], false),
        ("q_commute_now", "commute", "2026-10-21", &["t1"], true),
        (
            "q_meeting_now",
            "meeting",
            "2026-10-21",
            &["g1", "g2"],
            false,
        ),
        ("q_meeting_nov", "meeting", "2026-11-03", &["g1"], false),
        ("q_phone_now", "phone", "2026-10-21", &["p2"], false),
        ("q_phone_mar", "phone", "2026-03-01", &["p1"], false),
    ];
    let (_dir, path) = fresh();
    let conn = synthetic::connect(&path);
    let midnight = |d: &str| format!("{d}T00:00:00.000Z");
    let mut ids: BTreeMap<&str, String> = BTreeMap::new();
    for (key, said, tkind, from, to, uncertain) in statements {
        let (from, to, said) = (from.map(midnight), to.map(midnight), midnight(said));
        let (_, claim) = synthetic::source(
            &conn,
            &SourceSpec {
                recorder: "user",
                session: None,
                captured_at: &said,
                authored_at: Some(&said),
                claims: vec![ClaimSpec {
                    text: format!("statement {key}."),
                    tkind: Some(tkind),
                    applies_from: from.as_deref(),
                    applies_to: to.as_deref(),
                    index_fields: uncertain.then_some(r#"{"certainty":"uncertain"}"#),
                }],
            },
        );
        ids.insert(key, claim[0].clone());
    }
    drop(conn);
    let app = App::open(&path).unwrap();
    for (kind, subject, object) in relations {
        app.assert_relation(&user(), kind, &ids[subject], &ids[object], None, None)
            .unwrap();
    }
    let page = |who: &Principal, at: &str, present: bool| {
        let at: Timestamp = format!("{at}T12:00:00Z").parse().unwrap();
        app.claims_as_of(
            who,
            Some(at),
            Some(present),
            prime_radiant::ListSources {
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let mut flag_mismatch = Vec::new();
    for who in [user(), agent()] {
        flag_mismatch.clear();
        for (name, timeline, at, gold, gold_flag) in queries {
            let present = at == "2026-10-21";
            let result = page(&who, at, present);
            let members: Vec<&String> = timelines[timeline].iter().map(|k| &ids[k]).collect();
            let items: Vec<_> = result
                .items
                .iter()
                .filter(|i| members.contains(&&i.claim.id))
                .collect();
            let mut applies: Vec<&str> = items
                .iter()
                .filter(|i| i.status == "applies")
                .map(|i| *ids.iter().find(|(_, v)| **v == i.claim.id).unwrap().0)
                .collect();
            applies.sort();
            assert_eq!(applies, gold, "{name}");
            // History is never dropped: every statement of the timeline is still returned.
            assert_eq!(items.len(), members.len(), "{name}");
            let flag = items
                .iter()
                .any(|i| i.status == "applies" && !i.freshness.is_empty());
            if flag != gold_flag {
                flag_mismatch.push(name);
            }
        }
        // R2 `history` reported 9/10-11/12 flag accuracy; the faithful rule flags the old
        // weekly-meeting habit even though the gold set did not.
        assert_eq!(flag_mismatch, ["q_meeting_now"]);
    }
    // Supersession does not invalidate the earlier Claim or remove it from Claim reads.
    let now = page(&user(), "2026-10-21", true);
    let w1 = now.items.iter().find(|i| i.claim.id == ids["w1"]).unwrap();
    assert_eq!((w1.status, w1.history), ("superseded", true));
    assert_eq!(w1.superseded_by, vec![ids["w2"].clone()]);
    assert_eq!(short(&path, &ids["w1"]), "valid");
    assert!(app.get_claim(&user(), &ids["w1"]).is_ok());
}

#[test]
fn temporal_unknowns_judgments_and_relation_authority() {
    let (_dir, path) = fresh();
    let conn = synthetic::connect(&path);
    let one =
        |conn: &rusqlite::Connection, captured: &str, authored: Option<&str>, c: ClaimSpec| {
            synthetic::source(
                conn,
                &SourceSpec {
                    recorder: "user",
                    session: None,
                    captured_at: captured,
                    authored_at: authored,
                    claims: vec![c],
                },
            )
            .1[0]
                .clone()
        };
    let state = |text: &str| ClaimSpec {
        tkind: Some("state"),
        ..ClaimSpec::plain(text)
    };
    let late = one(
        &conn,
        "2026-10-30T00:00:00.000Z",
        None,
        state("captured late."),
    );
    let early = one(
        &conn,
        "2026-01-01T00:00:00.000Z",
        None,
        state("captured early."),
    );
    let future = one(
        &conn,
        "2026-01-01T00:00:00.000Z",
        Some("2026-01-01T00:00:00.000Z"),
        ClaimSpec {
            applies_from: Some("2027-01-01T00:00:00.000Z"),
            ..state("starts next year.")
        },
    );
    let open = one(
        &conn,
        "2026-01-01T00:00:00.000Z",
        Some("2026-01-01T00:00:00.000Z"),
        ClaimSpec {
            applies_from: Some("2026-01-01T00:00:00.000Z"),
            ..state("ongoing since january.")
        },
    );
    let ended = one(
        &conn,
        "2026-01-01T00:00:00.000Z",
        Some("2026-01-01T00:00:00.000Z"),
        ClaimSpec {
            applies_to: Some("2026-03-01T00:00:00.000Z"),
            ..state("until march.")
        },
    );
    let older = one(
        &conn,
        "2026-02-01T00:00:00.000Z",
        Some("2026-02-01T00:00:00.000Z"),
        state("old setting."),
    );
    let newer = one(
        &conn,
        "2026-03-01T00:00:00.000Z",
        Some("2026-03-01T00:00:00.000Z"),
        state("new setting."),
    );
    let (_, agent_claim) = synthetic::source(&conn, &spec("agent", &["agent statement."]));
    drop(conn);
    let app = App::open(&path).unwrap();
    let status = |app: &App,
                  who: &Principal,
                  at: &str,
                  present: bool|
     -> BTreeMap<String, (&'static str, Vec<&'static str>, bool)> {
        app.claims_as_of(
            who,
            Some(at.parse().unwrap()),
            Some(present),
            prime_radiant::ListSources {
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap()
        .items
        .into_iter()
        .map(|i| (i.claim.id, (i.status, i.freshness, i.provisional)))
        .collect()
    };
    let at = status(&app, &user(), "2026-10-21T00:00:00Z", true);
    // Unknown authored time: only capture is a known upper bound; nothing is synthesized.
    assert_eq!(at[&late].0, "unknown");
    assert_eq!(at[&early], ("applies", vec!["stale"], false));
    assert_eq!(at[&future].0, "scheduled");
    assert_eq!(at[&open], ("applies", vec!["stale"], false));
    assert_eq!(at[&ended].0, "ended");
    let later = status(&app, &user(), "2026-11-01T00:00:00Z", true);
    assert_eq!(later[&late], ("applies", vec!["age_unknown"], false));
    // Aging is a hint, never invalidity.
    assert_eq!(short(&path, &early), "valid");
    // A model judgment is a hint for agents only and never becomes a relation.
    let judgment = prime_radiant::integrity::temporal::TemporalJudgment {
        claim_a: newer.clone(),
        claim_b: older.clone(),
        provider: "fake".into(),
        model_id: "fake-temporal".into(),
        prompt_hash: "p".into(),
        input_digest: "d".into(),
        raw_label: "supersedes".into(),
        output_json: "{}".into(),
    };
    let j = app.record_temporal_judgment(&judgment).unwrap();
    assert_eq!(app.record_temporal_judgment(&judgment).unwrap(), j);
    assert_eq!(count(&path, "SELECT count(*) FROM relation"), 0);
    let agent_now = status(&app, &agent(), "2026-10-21T00:00:00Z", false);
    assert_eq!(agent_now[&older], ("superseded", vec![], true));
    assert_eq!(
        status(&app, &user(), "2026-10-21T00:00:00Z", false)[&older].0,
        "applies"
    );
    assert!(
        app.claim_relations(&user(), &older)
            .unwrap()
            .judgments
            .is_none()
    );
    assert_eq!(
        app.claim_relations(&agent(), &older)
            .unwrap()
            .judgments
            .unwrap()
            .len(),
        1
    );
    // An agent assertion is provisional and absent from the user projection.
    let provisional = app
        .assert_relation(&agent(), "supersedes", &newer, &older, None, Some(&j))
        .unwrap();
    assert_eq!(
        app.claim_relations(&agent(), &older).unwrap().relations[0].standing,
        "provisional"
    );
    assert!(
        app.claim_relations(&user(), &older)
            .unwrap()
            .relations
            .is_empty()
    );
    assert_eq!(
        status(&app, &user(), "2026-10-21T00:00:00Z", false)[&older].0,
        "applies"
    );
    // A user relation on the pair overrides the model hint; the user's view follows it.
    app.assert_relation(&user(), "exception", &newer, &older, None, None)
        .unwrap();
    app.set_integrity_lifecycle(
        &agent(),
        Target::Relation,
        &provisional,
        LifecycleEvent::Retract,
        None,
    )
    .unwrap();
    assert_eq!(
        status(&app, &agent(), "2026-10-21T00:00:00Z", false)[&older].0,
        "applies"
    );
    // Conflict relations are user-only and out of Phase 4 scope; the database also refuses.
    assert!(
        app.assert_relation(&agent(), "contradiction", &newer, &older, None, None)
            .is_err()
    );
    assert!(
        app.assert_relation(&user(), "contradiction", &newer, &older, None, None)
            .is_err()
    );
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert!(raw.execute("INSERT INTO relation(id,kind,subject,object,actor,created_at) VALUES('R','contradiction',?1,?2,'agent','2026-01-01T00:00:00.000Z')",[&newer,&older]).is_err());
    assert!(
        raw.execute("UPDATE model_judgment SET raw_label='compatible'", [])
            .is_err()
    );
    drop(raw);
    // Agents cannot relate records they cannot see; imports cannot assert relations.
    app.set_claim_status(&user(), &agent_claim[0], "forget")
        .unwrap();
    assert!(matches!(
        app.assert_relation(&agent(), "supersedes", &agent_claim[0], &older, None, None),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        app.assert_relation(
            &p(PrincipalKind::Import),
            "supersedes",
            &newer,
            &older,
            None,
            None
        ),
        Err(Error::Forbidden)
    ));
    // A blocked model is excluded from provisional supersession by configuration only.
    drop(app);
    let app = App::open_with_policy(
        &path,
        Policy {
            blocked_temporal_models: vec!["fake-temporal".into()],
            ..Policy::default()
        },
    )
    .unwrap();
    app.set_integrity_lifecycle(
        &user(),
        Target::Relation,
        &app.claim_relations(&user(), &older).unwrap().relations[0].id,
        LifecycleEvent::Retract,
        None,
    )
    .unwrap();
    assert_eq!(
        status(&app, &agent(), "2026-10-21T00:00:00Z", false)[&older].0,
        "applies"
    );
}

#[derive(Clone, Debug)]
enum Op {
    Claim(usize, &'static str),
    Source(usize, LifecycleEvent),
    AddDep(usize, usize, &'static str),
    Dep(usize, LifecycleEvent),
    Read(usize, usize),
    Import(usize, usize, usize),
}
const KINDS: [&str; 4] = ["premise", "restates", "alternative", "informed_by"];
const EVENTS: [LifecycleEvent; 3] = [
    LifecycleEvent::Retract,
    LifecycleEvent::Forget,
    LifecycleEvent::Restore,
];
fn generate(seed: u64, n: usize) -> Vec<Op> {
    let mut r = Rng::new(seed ^ 0x5eed);
    (0..n)
        .map(|_| match r.below(100) {
            0..=24 => Op::Claim(
                r.below(1000),
                ["retract", "forget", "restore", "restore"][r.below(4)],
            ),
            25..=34 => Op::Source(r.below(1000), EVENTS[r.below(3)]),
            35..=64 => Op::AddDep(r.below(1000), r.below(1000), KINDS[r.below(4)]),
            65..=79 => Op::Dep(r.below(1000), EVENTS[r.below(3)]),
            80..=89 => Op::Read(r.below(3), r.below(1000)),
            _ => Op::Import(r.below(3), 1 + r.below(3), r.below(3)),
        })
        .collect()
}
/// Ancestors over every declared edge, including retracted ones (the cycle rule).
fn reaches(edges: &[(usize, usize)], from: usize, to: usize) -> bool {
    let mut stack = vec![from];
    let mut seen = std::collections::HashSet::new();
    while let Some(x) = stack.pop() {
        if x == to {
            return true;
        }
        if seen.insert(x) {
            stack.extend(edges.iter().filter(|(c, _)| *c == x).map(|(_, p)| *p));
        }
    }
    false
}
/// Replay `ops` on a fresh database built from `seed`; the first failing step, if any.
#[derive(Default)]
struct Coverage {
    reasons: std::collections::BTreeSet<String>,
    deps_unknown: usize,
    cycles_rejected: usize,
    imports: usize,
}
async fn replay(seed: u64, ops: &[Op], seen: &mut Coverage) -> Option<(usize, String)> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    drop(App::open(&path).unwrap());
    let mut r = Rng::new(seed);
    let conn = synthetic::connect(&path);
    let mut claims = Vec::new();
    let mut sources = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut deps: Vec<String> = Vec::new();
    let sessions = ["s0", "s1", "s2"];
    for s in 0..8 {
        let recorder = ["user", "agent", "import"][r.below(3)];
        let n = 2 + r.below(4);
        let texts: Vec<String> = (0..n).map(|i| format!("seed claim {s}.{i}.")).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let mut sp = spec(recorder, &refs);
        sp.session = (recorder == "agent").then(|| sessions[r.below(3)]);
        let (source, ids) = synthetic::source(&conn, &sp);
        sources.push(source);
        for id in ids {
            let child = claims.len();
            claims.push(id);
            // Mixed DAG: each new Claim may depend on earlier ones (acyclic by construction).
            for _ in 0..r.below(3) {
                if child > 0 {
                    let parent = r.below(child);
                    if !edges.contains(&(child, parent)) {
                        deps.push(synthetic::dependency(
                            &conn,
                            &claims[child],
                            &claims[parent],
                            KINDS[r.below(4)],
                        ));
                        edges.push((child, parent));
                    }
                }
            }
        }
    }
    drop(conn);
    let app = App::open(&path).unwrap();
    if let Some(bad) = app.validity_mismatches().unwrap().first() {
        return Some((0, format!("initial rebuild mismatch {bad}")));
    }
    let u = user();
    for (step, op) in ops.iter().enumerate() {
        match op.clone() {
            Op::Claim(i, event) => {
                let _ = app.set_claim_status(&u, &claims[i % claims.len()], event);
            }
            Op::Source(i, event) => {
                let id = sources[i % sources.len()].parse().unwrap();
                let _ = app.set_source_lifecycle(&u, id, event, None);
            }
            Op::AddDep(c, pa, kind) => {
                let (c, pa) = (c % claims.len(), pa % claims.len());
                let expect_cycle = c == pa || reaches(&edges, pa, c);
                let known = edges.contains(&(c, pa));
                match app.add_dependency(&u, &claims[c], &claims[pa], kind) {
                    Ok(id) => {
                        if expect_cycle {
                            return Some((step, format!("cycle accepted {op:?}")));
                        }
                        if !deps.contains(&id) {
                            deps.push(id);
                        }
                        if !known {
                            edges.push((c, pa));
                        }
                    }
                    Err(_) if expect_cycle => seen.cycles_rejected += 1,
                    Err(e) => return Some((step, format!("{op:?} rejected: {e}"))),
                }
            }
            Op::Dep(i, event) if !deps.is_empty() => {
                let _ = app.set_integrity_lifecycle(
                    &u,
                    Target::Dependency,
                    &deps[i % deps.len()],
                    event,
                    None,
                );
            }
            Op::Dep(..) => (),
            Op::Read(s, i) => {
                let reader = agent().with_session(Some(sessions[s].into())).unwrap();
                let _ = app.get_claim(&reader, &claims[i % claims.len()]);
            }
            Op::Import(kind, n, s) => {
                // Historical import through the approval boundary; later edges may point
                // either way relative to existing Claims (late-discovered dependencies).
                let who = match kind {
                    0 => user(),
                    1 => agent().with_session(Some(sessions[s].into())).unwrap(),
                    _ => p(PrincipalKind::Import),
                };
                let texts: Vec<String> = (0..n).map(|i| format!("import {step}.{i}.")).collect();
                let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
                let speaker = if kind == 0 { "user" } else { "agent" };
                let source = app
                    .record_source(&who, RecordSource::plain(refs.join(" ")))
                    .unwrap()
                    .source_id;
                sources.push(source.to_string());
                claims.extend(
                    approve(&app, refs.iter().map(|t| candidate(t, speaker)).collect()).await,
                );
                seen.imports += 1;
            }
        }
        let raw = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        for row in raw
            .prepare("SELECT reasons,deps_unknown FROM claim_validity")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))
            .unwrap()
        {
            let (reasons, unknown) = row.unwrap();
            seen.reasons.extend(
                reasons
                    .split(',')
                    .filter(|x| !x.is_empty())
                    .map(str::to_owned),
            );
            seen.deps_unknown += usize::from(unknown);
        }
        let bad = app.validity_mismatches().unwrap();
        if !bad.is_empty() {
            let at: Vec<usize> = bad
                .iter()
                .filter_map(|id| claims.iter().position(|c| c == id))
                .collect();
            return Some((
                step,
                format!("{op:?} left materialized validity stale for claims {at:?}"),
            ));
        }
    }
    if !app.verify_integrity().unwrap().ok {
        return Some((ops.len(), "integrity report failed".into()));
    }
    None
}
#[tokio::test]
async fn randomized_graph_operations_match_full_recomputation_oracle() {
    // Fixed, recorded seeds; PRIMERADIANT_PROPERTY_SEEDS=a,b,c adds more.
    let mut seeds: Vec<u64> = (1..=12).map(|i| 0x9a4e_0000 + i).collect();
    if let Ok(extra) = std::env::var("PRIMERADIANT_PROPERTY_SEEDS") {
        seeds.extend(
            extra
                .split(',')
                .filter_map(|s| s.trim().parse::<u64>().ok()),
        );
    }
    let mut executed = 0;
    let mut seen = Coverage::default();
    for seed in seeds {
        let ops = generate(seed, 120);
        if let Some((step, why)) = replay(seed, &ops, &mut seen).await {
            // Greedy minimization: drop operations while the replay still fails.
            let mut minimal = ops[..=step.min(ops.len() - 1)].to_vec();
            let mut i = minimal.len();
            while i > 0 {
                i -= 1;
                let mut candidate = minimal.clone();
                candidate.remove(i);
                if replay(seed, &candidate, &mut Coverage::default())
                    .await
                    .is_some()
                {
                    minimal = candidate;
                }
            }
            panic!("seed {seed} failed at step {step}: {why}\nminimized ops: {minimal:?}");
        }
        executed += ops.len();
    }
    assert!(executed >= 12 * 120);
    // Every validity reason, undeclared session dependencies, cycles and imports occurred.
    let all = [
        "advisory_invalid",
        "alternatives_invalid",
        "alternatives_uncertain",
        "forgotten",
        "premise_invalid",
        "premise_uncertain",
        "restates_invalid",
        "retracted",
        "session_read_invalid",
        "source_forgotten",
        "source_retracted",
    ];
    assert_eq!(
        seen.reasons.iter().map(String::as_str).collect::<Vec<_>>(),
        all
    );
    assert!(seen.deps_unknown > 0 && seen.cycles_rejected > 0 && seen.imports > 0);
    eprintln!(
        "property coverage: ops={executed} deps_unknown_rows={} cycles_rejected={} imports={}",
        seen.deps_unknown, seen.cycles_rejected, seen.imports
    );
}
#[test]
fn dependency_insertion_order_does_not_change_validity() {
    let mut r = Rng::new(0x0de7);
    let n = 40;
    let mut edges = Vec::new();
    for child in 1..n {
        for _ in 0..r.below(3) {
            let parent = r.below(child);
            if !edges.iter().any(|(c, p, _)| (*c, *p) == (child, parent)) {
                edges.push((child, parent, KINDS[r.below(4)]));
            }
        }
    }
    let retracted: Vec<usize> = (0..n).filter(|_| r.chance(0.2)).collect();
    let mut outcomes = Vec::new();
    for order in 0..3 {
        let (_dir, path) = fresh();
        let conn = synthetic::connect(&path);
        let texts: Vec<String> = (0..n).map(|i| format!("historical {i}.")).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let (_, ids) = synthetic::source(&conn, &spec("user", &refs));
        drop(conn);
        let app = App::open(&path).unwrap();
        let mut plan = edges.clone();
        match order {
            0 => (),
            1 => plan.reverse(),
            _ => {
                for i in (1..plan.len()).rev() {
                    plan.swap(i, r.below(i + 1));
                }
            }
        }
        let retract = |app: &App| {
            for &i in &retracted {
                app.set_claim_status(&user(), &ids[i], "retract").unwrap();
            }
        };
        // Order 1 applies lifecycle history before any edge exists; others after or mixed.
        if order == 1 {
            retract(&app);
        }
        for (k, (child, parent, kind)) in plan.iter().enumerate() {
            app.add_dependency(&user(), &ids[*child], &ids[*parent], kind)
                .unwrap();
            if order == 2 && k == plan.len() / 2 {
                retract(&app);
            }
        }
        if order == 0 {
            retract(&app);
        }
        assert!(app.validity_mismatches().unwrap().is_empty());
        outcomes.push(ids.iter().map(|id| short(&path, id)).collect::<Vec<_>>());
    }
    assert_eq!(outcomes[0], outcomes[1]);
    assert_eq!(outcomes[0], outcomes[2]);
    assert!(outcomes[0].iter().any(|s| s.starts_with("invalid")));
    assert!(outcomes[0].iter().any(|s| s == "valid"));
}

/// A database at schema v2 (Phase 3) with lifecycle history, extraction provenance,
/// Claims, mentions and declarations migrates to v3 without losing or renumbering anything.
#[test]
fn phase3_database_migrates_losslessly_with_validity_and_backup() {
    use rusqlite::params;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    for (v, name, sql) in [
        (1, "0001_init", include_str!("../src/store/migrations/0001_init.sql")),
        (2, "0002_extraction", include_str!("../src/store/migrations/0002_extraction.sql")),
    ] {
        conn.execute_batch(sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migration VALUES(?1,?2,?3,'2026-01-01T00:00:00.000Z')",
            params![v, name, hash(sql)],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", v).unwrap();
    }
    let t = "2026-02-01T00:00:00.000Z";
    let mut claims = Vec::new();
    for (i, (recorder, session, text)) in [
        ("user", None, "사용자 원문 👩‍💻 first."),
        ("agent", Some("legacy-session"), "agent original\r\nsecond."),
        ("import", None, "imported third."),
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!("01K00000000000000000000S0{i}");
        conn.execute("INSERT INTO source(id,recorder,session_id,content,content_sha256,media_type,captured_at,retention_request,request_digest) VALUES(?1,?2,?3,?4,?5,'text/plain',?6,'none',?5)",params![source,recorder,session,text,hash(text),t]).unwrap();
        let job = format!("01K00000000000000000000J0{i}");
        conn.execute("INSERT INTO job(id,kind,target_id,idem_key,state,attempts,run_after,created_at,updated_at) VALUES(?1,'extract',?2,?3,'done',1,?4,?4,?4)",params![job,source,format!("initial/{source}"),t]).unwrap();
        conn.execute("INSERT INTO extraction_request(job_id,plan_json,prompt_hash,created_at) VALUES(?1,'{}','h',?2)",params![job,t]).unwrap();
        conn.execute("INSERT INTO extraction_candidate VALUES(?1,?2,'{}',?3,'[]',?4)",params![format!("01K00000000000000000000C0{i}"),job,hash("{}"),t]).unwrap();
        conn.execute("INSERT INTO extraction_run(job_id,source_id,extractor_id,output_hash,approval_actor,created_at) VALUES(?1,?2,'legacy',?3,'user',?4)",params![job,source,hash("{}"),t]).unwrap();
        let claim = format!("01K00000000000000000000L0{i}");
        conn.execute("INSERT INTO claim(id,source_id,text,lang,category,speaker,basis,anchor_start,anchor_end,anchor_quote,standing,extractor_id,job_id,created_at) VALUES(?1,?2,?3,'en','Fact','unknown','verbatim',0,?4,?3,0,'legacy',?5,?6)",params![claim,source,text,text.len(),job,t]).unwrap();
        let first = text.chars().next().unwrap();
        let span = json!({"start":0,"end":first.len_utf8()}).to_string();
        conn.execute("INSERT INTO mention VALUES(?1,?2,?3,?4)",params![format!("01K00000000000000000000M0{i}"),claim,first.to_string(),span]).unwrap();
        claims.push(claim);
    }
    // A pending job and an exhausted failure keep their operational state.
    conn.execute("INSERT INTO job(id,kind,target_id,idem_key,state,attempts,run_after,last_error,created_at,updated_at) VALUES('01K00000000000000000000J09','extract','01K00000000000000000000S00','revision/x','failed',3,?1,'review_required',?1,?1)",[t]).unwrap();
    conn.execute("INSERT INTO claim_dependency VALUES('01K00000000000000000000D01',?1,?2,'premise','system',?3)",params![claims[1],claims[0],t]).unwrap();
    conn.execute("INSERT INTO claim_dependency VALUES('01K00000000000000000000D02',?1,?2,'informed_by','system',?3)",params![claims[2],claims[1],t]).unwrap();
    for (seq, kind, target, event, actor) in [
        (7, "source", "01K00000000000000000000S02", "retract", "user"),
        (11, "claim", claims[0].as_str(), "confirm", "user"),
        (12, "claim", claims[1].as_str(), "retract", "agent"),
        (15, "source", "01K00000000000000000000S02", "restore", "user"),
    ] {
        conn.execute("INSERT INTO status_event VALUES(?1,?2,?3,?4,?5,'audit',?6)",params![seq,kind,target,event,actor,t]).unwrap();
    }
    let tables = [
        "source",
        "status_event",
        "job",
        "extraction_request",
        "extraction_candidate",
        "extraction_run",
        "claim",
        "mention",
        "claim_dependency",
        "schema_migration",
    ];
    let dump = |conn: &rusqlite::Connection, table: &str| -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            Ok((0..n)
                .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                .collect::<Vec<_>>()
                .join("|"))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
    };
    let before: Vec<Vec<String>> = tables.iter().map(|t| dump(&conn, t)).collect();
    drop(conn);

    let app = App::open(&path).unwrap();
    assert!(app.verify_integrity().unwrap().ok);
    let conn = synthetic::connect(&path);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).unwrap(),
        3
    );
    for (table, old) in tables.iter().zip(&before) {
        let new = dump(&conn, table);
        if *table == "schema_migration" {
            assert_eq!(&new[..2], &old[..]);
            assert_eq!(new.len(), 3);
            let name: String = conn
                .query_row("SELECT name FROM schema_migration WHERE version=3", [], |r| r.get(0))
                .unwrap();
            assert_eq!(name, "0003_knowledge_integrity");
        } else {
            assert_eq!(&new, old, "{table}");
        }
    }
    // Validity is derived for legacy Claims: the agent-retracted Claim invalidates nothing
    // mandatory beyond itself; its advisory dependent becomes uncertain, not invalid.
    assert_eq!(short(&path, &claims[0]), "valid");
    assert_eq!(short(&path, &claims[1]), "invalid:retracted");
    assert_eq!(short(&path, &claims[2]), "needs_revalidation:advisory_invalid");
    // New events continue after the preserved maximum sequence number.
    app.set_claim_status(&user(), &claims[1], "restore").unwrap();
    assert_eq!(
        conn.query_row("SELECT max(seq) FROM status_event", [], |r| r.get::<_, i64>(0)).unwrap(),
        16
    );
    assert_eq!(short(&path, &claims[2]), "valid");
    // Constraints, foreign keys and append-only triggers are in force after the rebuild.
    for sql in [
        "UPDATE status_event SET reason='x'",
        "DELETE FROM claim_dependency",
        "UPDATE claim SET text='x'",
        "INSERT INTO claim_dependency VALUES('Z','missing','missing','premise','user','t')",
        "INSERT INTO status_event(target_kind,target_id,event,actor,created_at) VALUES('dependency','missing','retract','user','t')",
        "INSERT INTO job(id,kind,target_id,idem_key,state,run_after,created_at,updated_at) VALUES('Z','revalidate','missing','k','pending','t','t','t')",
        "UPDATE job SET kind='revalidate'",
        "INSERT INTO source_citation VALUES('Z','01K00000000000000000000S00','01K00000000000000000000S01','agent','t')",
    ] {
        assert!(conn.execute(sql, []).is_err(), "{sql}");
    }
    // The pre-migration backup is a verified v2 snapshot with identical authoritative rows.
    let backup = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().contains(".pre-v3.") && p.extension().is_some_and(|x| x == "bak"))
        .unwrap();
    let old = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        old.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).unwrap(),
        2
    );
    for (table, rows) in tables.iter().zip(&before) {
        assert_eq!(&dump(&old, table), rows, "{table}");
    }
}
#[test]
fn concurrent_readers_never_observe_lifecycle_without_its_validity() {
    let (_d, path) = fresh();
    let conn = synthetic::connect(&path);
    let (_, hub) = synthetic::source(&conn, &spec("user", &["hub premise."]));
    let mut children = Vec::new();
    for i in 0..400 {
        let (_, c) = synthetic::source(&conn, &spec("user", &[&format!("child {i}.")]));
        synthetic::dependency(&conn, &c[0], &hub[0], "premise");
        children.push(c[0].clone());
    }
    drop(conn);
    let app = std::sync::Arc::new(App::open(&path).unwrap());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let (app, hub, stop) = (app.clone(), hub[0].clone(), stop.clone());
        std::thread::spawn(move || {
            let mut toggles = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let event = ["retract", "restore"][toggles % 2];
                app.set_claim_status(&user(), &hub, event).unwrap();
                toggles += 1;
            }
            toggles
        })
    };
    let readers: Vec<_> = (0..3)
        .map(|r| {
            let (path, hub, children, app) =
                (path.clone(), hub[0].clone(), children.clone(), app.clone());
            std::thread::spawn(move || {
                let mut conn = rusqlite::Connection::open(&path).unwrap();
                let mut snapshots = [0usize; 2];
                for i in 0..300 {
                    // Raw snapshot: the newest hub event and every child's state agree.
                    let tx = conn.transaction().unwrap();
                    let last: String = tx.query_row("SELECT event FROM status_event WHERE target_kind='claim' AND target_id=?1 ORDER BY seq DESC LIMIT 1", [&hub], |r| r.get(0)).unwrap_or_else(|_| "restore".into());
                    let invalid: usize = tx.query_row(&format!("SELECT count(*) FROM claim_validity WHERE state='invalid' AND claim_id IN ({})", children.iter().map(|c| format!("'{c}'")).collect::<Vec<_>>().join(",")), [], |r| r.get(0)).unwrap();
                    let retracted = last == "retract";
                    assert_eq!(invalid, if retracted { children.len() } else { 0 }, "reader {r} snapshot {i}");
                    snapshots[retracted as usize] += 1;
                    drop(tx);
                    // Projection: the user never sees a child alongside a retracted hub.
                    let listed = app.list_claims(&user()).unwrap();
                    let hub_seen = listed.iter().any(|c| c.id.as_str() == hub);
                    let kids = listed.iter().filter(|c| c.id.as_str() != hub).count();
                    assert_eq!(kids, if hub_seen { children.len() } else { 0 }, "reader {r} list {i}");
                }
                snapshots
            })
        })
        .collect();
    let mut seen = [0usize; 2];
    for r in readers {
        let s = r.join().unwrap();
        seen[0] += s[0];
        seen[1] += s[1];
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let toggles = writer.join().unwrap();
    assert!(seen[0] > 0 && seen[1] > 0 && toggles > 2, "no interleaving: {seen:?} {toggles}");
    assert!(app.validity_mismatches().unwrap().is_empty());
}
