//! Offline disposable Compose verification only. Never run against user data.
use prime_radiant::{
    App, Policy, domain::*, embedding::EmbeddingConfig, extraction::*, providers::*,
};
use serde_json::json;
#[tokio::main]
async fn main() {
    let path = std::env::args().nth(1).expect("disposable DB path");
    let app = App::open_with_policy(
        path,
        Policy {
            embedding: EmbeddingConfig::Off,
            ..Default::default()
        },
    )
    .unwrap();
    let user = Principal::for_local_cli(PrincipalKind::User, None, None);
    assert!(
        app.list_claims(&user).unwrap().is_empty(),
        "refuse nonempty fixture"
    );
    let text = "서버는 SQLite를 사용한다. The server uses SQLite.";
    let source = app.record_source(&user, RecordSource::plain(text)).unwrap();
    let raw = json!({"empty_reason":null,"claims":[{"text":text,"lang":"ko","category":"Fact","speaker":"user","speaker_label":null,"basis":"verbatim","anchor":{"quote":text,"occurrence":0},"tkind":null,"applies_from":null,"applies_to":null,"standing":false,"index_fields":null,"revision_of":null,"mentions":[],"dependencies":[]}]}).to_string();
    let review =
        json!({"verdict":"pass","issues":[],"checked":[0],"coverage_complete":true}).to_string();
    let fake = FakeProvider(std::sync::Mutex::new(
        [
            Ok(raw.clone()),
            Ok(review.clone()),
            Ok(review.clone()),
            Ok(review),
        ]
        .into(),
    ));
    let plan = Plan {
        prompt_version: None,
        provider: "fake".into(),
        variant: "audited".into(),
        extractor: ModelConfig {
            model: "fake".into(),
            max_output_tokens: None,
            timeout_seconds: 2,
        },
        reviewer: ModelConfig {
            model: "independent-fake".into(),
            max_output_tokens: None,
            timeout_seconds: 2,
        },
    };
    let unavailable = FakeProvider(std::sync::Mutex::new(
        [Err(ProviderError::Unavailable)].into(),
    ));
    assert!(app.work_once(&unavailable, &plan).await.is_err());
    assert!(app.list_claims(&user).unwrap().is_empty());
    // Exercise real durable backoff, without editing queue state or bypassing validation.
    tokio::time::sleep(std::time::Duration::from_secs(11)).await;
    let candidate = app.work_once(&fake, &plan).await.unwrap().unwrap();
    assert!(app.list_claims(&user).unwrap().is_empty());
    let agent = Principal::for_local_cli(PrincipalKind::Agent, None, None);
    assert!(
        app.approve_extraction(&agent, &candidate, &hash(&raw))
            .is_err()
    );
    let ids = app
        .approve_extraction(&user, &candidate, &hash(&raw))
        .unwrap();
    assert_eq!(
        ids,
        app.approve_extraction(&user, &candidate, &hash(&raw))
            .unwrap()
    );
    println!("{}", json!({"source":source.source_id,"claims":ids}));
}
