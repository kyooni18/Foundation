#[path = "support/synthetic.rs"]
mod synthetic;
#[path = "support/mock_embed.rs"]
mod mock_embed;
use prime_radiant::{App, Policy, domain::*, embedding::{EmbeddingConfig, Injected}, retrieval::search::SearchRequest};
use std::sync::Arc;

#[test]
fn smoke_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    drop(App::open(&path).unwrap());
    {
        let conn = synthetic::connect(&path);
        use synthetic::BulkClaim;
        let mk = |t: &str, l: &'static str| BulkClaim { text: t.into(), lang: l };
        synthetic::bulk_source(&conn, "user", "2026-01-01T00:00:00.000Z", &[mk("The deploy script lives in scripts/deploy.sh", "en"), mk("서버 재시작은 systemctl restart api 로 한다", "ko")]);
        synthetic::bulk_source(&conn, "user", "2026-01-02T00:00:00.000Z", &[mk("Database backups run nightly at 02:00", "en")]);
    }
    let mock = Arc::new(mock_embed::MockEmbedder::new("r1"));
    let policy = Policy { embedding: EmbeddingConfig::Custom(Injected(mock.clone())), ..Default::default() };
    let app = App::open_with_policy(&path, policy).unwrap();
    let user = Principal::for_local_cli(PrincipalKind::User, None, None);
    let before = app.search(&user, SearchRequest::new("deploy")).unwrap();
    println!("before: {}", serde_json::to_string_pretty(&before.coverage).unwrap());
    assert!(before.results.is_empty());
    assert_ne!(before.coverage.status, "complete");
    while app.process_lexical_once("t").unwrap().jobs > 0 {}
    while app.process_embeddings_once(mock.as_ref(), "t").unwrap().jobs > 0 {}
    let r = app.search(&user, SearchRequest::new("deploy script")).unwrap();
    println!("{}", serde_json::to_string_pretty(&r).unwrap());
    assert_eq!(r.results[0].claim.text, "The deploy script lives in scripts/deploy.sh");
    let r = app.search(&user, SearchRequest::new("\"scripts/deploy.sh\"")).unwrap();
    assert_eq!(r.results[0].match_type, "exact");
    let r = app.search(&user, SearchRequest::new("서버 재시작")).unwrap();
    assert!(r.results[0].claim.text.contains("systemctl"));
    println!("{}", serde_json::to_string(&app.queue_status(&user).unwrap()).unwrap());
}
