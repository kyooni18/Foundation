use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};

fn config(keys: PathBuf) -> Config {
    Config {
        resource: "https://memory.example/mcp".into(),
        issuer: "https://auth.example/realms/radiant".into(),
        discovery_url: "https://auth.example/realms/radiant/.well-known/openid-configuration"
            .into(),
        jwks_uri: "https://auth.example/realms/radiant/protocol/openid-connect/certs".into(),
        keys,
        dcr_registry: None,
        introspection: Introspection {
            endpoint: "https://auth.example/introspect".into(),
            client_id: "resource-server".into(),
            secret_file: PathBuf::from("unused-in-crypto-unit-test"),
        },
        bindings: vec![Binding {
            subject: "trusted".into(),
            client_id: "claude-client".into(),
            kind: PrincipalKind::Agent,
            label: "Claude".into(),
        }],
        bind: "127.0.0.1:0".parse().unwrap(),
    }
}
fn state(config: Config, jwks: JwkSet) -> Remote {
    Remote {
        config,
        http: reqwest::Client::new(),
        cache: tokio::sync::Mutex::new(Cache {
            keys: jwks,
            fetched: Instant::now(),
        }),
        slots: Arc::new(tokio::sync::Semaphore::new(32)),
        rate: std::sync::Mutex::new((Instant::now(), 0)),
    }
}
fn signing() -> (jsonwebtoken::EncodingKey, JwkSet) {
    let key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    let jwks = serde_json::from_value(json!({"keys":[{"kty":"RSA","kid":"one","use":"sig","alg":"RS256","n":URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(key.e().to_bytes_be())}]})).unwrap();
    (
        jsonwebtoken::EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes()),
        jwks,
    )
}
fn claims() -> Value {
    json!({"sub":"trusted","azp":"claude-client","iss":"https://auth.example/realms/radiant","aud":"https://memory.example/mcp","exp":now()+300,"iat":now(),"scope":"memory.read memory.write memory.forget"})
}
fn sign(key: &jsonwebtoken::EncodingKey, value: &Value, kid: &str) -> String {
    let mut header = jsonwebtoken::Header::new(Algorithm::RS256);
    header.kid = Some(kid.into());
    jsonwebtoken::encode(&header, value, key).unwrap()
}
#[test]
fn key_lifecycle_and_hash_only_storage() {
    let d = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = d.path().join("keys.db");
    key_init(&path).unwrap();
    assert!(key_init(&path).is_err());
    let key = key_issue(
        &path,
        PrincipalKind::Agent,
        "script",
        "memory.read",
        now() + 300,
    )
    .unwrap();
    let a = api_key(&path, &key).unwrap();
    assert_eq!(a.principal.kind(), PrincipalKind::Agent);
    assert_eq!(a.scopes, ["memory.read"]);
    assert!(
        !std::fs::read(&path)
            .unwrap()
            .windows(key.len())
            .any(|w| w == key.as_bytes())
    );
    let replacement = key_issue(
        &path,
        PrincipalKind::Agent,
        "script",
        "memory.read",
        now() + 300,
    )
    .unwrap();
    assert_ne!(key, replacement);
    let id = hex(&Sha256::digest(key.as_bytes())[..8]);
    key_revoke(&path, &id).unwrap();
    assert!(api_key(&path, &key).is_err());
    assert!(api_key(&path, &replacement).is_ok());
    key_db(&path)
        .unwrap()
        .execute("UPDATE api_keys SET expires=?1", [now() - 1])
        .unwrap();
    assert!(api_key(&path, &replacement).is_err());
    assert!(key_issue(&path, PrincipalKind::Agent, "x", "admin", now() + 300).is_err());
    assert!(api_key(&path, "prk_not_a_token").is_err());
}
#[tokio::test]
async fn jwt_binding_and_crypto() {
    let d = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (key, jwks) = signing();
    let mut state = state(config(d.path().join("keys")), jwks);
    let good = claims();
    let raw = sign(&key, &good, "one");
    assert_eq!(
        state.oauth(&raw).await.unwrap().principal.kind(),
        PrincipalKind::Agent
    );
    for (field, value) in [
        ("iss", json!("https://evil.example")),
        ("aud", json!("other")),
        ("sub", json!("stranger")),
        ("azp", json!("other-client")),
        ("exp", json!(now() - 60)),
        ("iat", json!(now() + 100)),
        ("nbf", json!(now() + 100)),
        ("client_id", json!("conflicting")),
    ] {
        let mut bad = good.clone();
        bad[field] = value;
        assert!(
            state.oauth(&sign(&key, &bad, "one")).await.is_err(),
            "accepted invalid {field}"
        );
    }
    let (other, _) = signing();
    assert!(state.oauth(&sign(&other, &good, "one")).await.is_err());
    assert!(state.oauth(&sign(&key, &good, "unknown")).await.is_err());
    use std::os::unix::fs::OpenOptionsExt;
    let registry = d.path().join("dcr.db");
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&registry).unwrap();
    let db = rusqlite::Connection::open(&registry).unwrap();
    db.execute_batch("CREATE TABLE clients(client_id TEXT PRIMARY KEY,tag TEXT,enabled INTEGER); INSERT INTO clients VALUES ('issued-client','claude',1)").unwrap();
    state.config.dcr_registry = Some(registry);
    state.config.bindings[0].client_id = "dcr:claude".into();
    let mut dynamic = good.clone(); dynamic["azp"] = json!("issued-client");
    assert!(state.oauth(&sign(&key,&dynamic,"one")).await.is_ok());
    dynamic["sub"] = json!("untrusted");
    assert!(state.oauth(&sign(&key,&dynamic,"one")).await.is_err());
    dynamic["sub"] = json!("trusted");
    db.execute("UPDATE clients SET enabled=0", []).unwrap();
    assert!(state.oauth(&sign(&key,&dynamic,"one")).await.is_err());
    // A rotated cache drops the old key; no token-provided URL is fetched.
    state.cache.lock().await.keys = signing().1;
    assert!(state.oauth(&raw).await.is_err());
}

#[test]
fn compact_arguments_are_enforced_not_just_advertised() {
    for (name, args) in [("get", json!({"id":"x","include_retracted":true})), ("record", json!({"content":"x","recorder":"user"})), ("search", json!({"query":"x","limit":101})), ("queue_status", json!({}))] {
        assert!(!valid_arguments(name, args.as_object().unwrap()));
    }
    assert!(valid_arguments("record", json!({"content":"x"}).as_object().unwrap()));
}

async fn fixture() -> (
    tempfile::TempDir,
    String,
    String,
    jsonwebtoken::EncodingKey,
    tokio::task::JoinHandle<()>,
) {
    let d = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let keys = d.path().join("keys.db");
    key_init(&keys).unwrap();
    let key = key_issue(
        &keys,
        PrincipalKind::Agent,
        "script",
        "memory.read memory.write memory.forget",
        now() + 300,
    )
    .unwrap();
    let credentials = d.path().join("local.json");
    crate::authentication::Credentials::issue(
        &credentials,
        d.path().join("local.token"),
        PrincipalKind::User,
        None,
    )
    .unwrap();
    let app = crate::App::open_with_policy(
        d.path().join("memory.db"),
        crate::Policy {
            embedding: crate::embedding::EmbeddingConfig::Off,
            ..Default::default()
        },
    )
    .unwrap();
    let service = Service::new(
        app,
        crate::authentication::Credentials::open(credentials).unwrap(),
    );
    let (signer, jwks) = signing();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let secret_path = d.path().join("fixture.secret");
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&secret_path)
        .unwrap()
        .write_all(hex(&random_bytes().unwrap()).as_bytes())
        .unwrap();
    let mut config = config(keys);
    config.introspection.endpoint = format!("{url}/fixture/introspect");
    config.introspection.secret_file = secret_path;
    let router = build_router(service, state(config, jwks)).route(
        "/fixture/introspect",
        axum::routing::post(|| async { axum::Json(json!({"active":true})) }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (d, url, key, signer, task)
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}
fn rpc(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    version: &str,
    mut body: Value,
) -> reqwest::RequestBuilder {
    if version == "2026-07-28" {
        if !body["params"].is_object() {
            body["params"] = json!({});
        }
        body["params"]["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":version,"io.modelcontextprotocol/clientCapabilities":{}});
    }
    let name = body["params"]["name"].as_str().unwrap_or("").to_owned();
    let builder = client
        .post(format!("{url}/mcp"))
        .header("Host", "memory.example")
        .bearer_auth(token)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", version)
        .header("MCP-Method", body["method"].as_str().unwrap_or(""));
    let builder = if !name.is_empty() {
        builder.header("MCP-Name", name)
    } else {
        builder
    };
    builder.json(&body)
}
#[tokio::test]
async fn http_auth_protocols_scopes_and_four_tools() {
    let (d, url, key, signer, task) = fixture().await;
    let c = client();
    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
    ] {
        let md: Value = c
            .get(format!("{url}{path}"))
            .header("Host", "memory.example")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(md["resource"], "https://memory.example/mcp");
        assert_eq!(
            md["authorization_servers"][0],
            "https://auth.example/realms/radiant"
        );
    }
    let unauth = c
        .post(format!("{url}/mcp"))
        .header("Host", "memory.example")
        .send()
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);
    assert!(
        unauth.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("resource_metadata")
    );
    let oauth = sign(&signer, &claims(), "one");
    for token in [&key, &oauth] {
        for version in ["2026-07-28", "2025-11-25", "2025-06-18"] {
            if version != "2026-07-28" {
                let r = rpc(&c, &url, token, version, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})).send().await.unwrap();
                assert_eq!(r.status(), 200);
                let init: Value = r.json().await.unwrap();
                assert_eq!(init["result"]["protocolVersion"], version);
            }
            let r = rpc(
                &c,
                &url,
                token,
                version,
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            )
            .send()
            .await
            .unwrap();
            let status = r.status();
            let body = r.text().await.unwrap();
            assert_eq!(status, 200, "{body}");
            let tools: Value = serde_json::from_str(&body).unwrap();
            let tools = tools["result"]["tools"].as_array().expect(&body);
            assert_eq!(tools.len(), 4);
            assert_eq!(
                tools
                    .iter()
                    .map(|t| t["name"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["record", "search", "get", "forget"]
            );
            assert!(tools[0].get("securitySchemes").is_some());
        }
        let saved: Value = rpc(&c, &url, token, "2026-07-28", json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"record","arguments":{"content":"agent generated fixture text"}}})).send().await.unwrap().json().await.unwrap();
        assert_eq!(saved["result"]["structuredContent"]["ok"], true, "{saved}");
        let id = saved["result"]["structuredContent"]["result"]["source_id"]
            .as_str()
            .unwrap();
        let fetched: Value = rpc(&c, &url, token, "2026-07-28", json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"get","arguments":{"id":id}}})).send().await.unwrap().json().await.unwrap();
        assert_eq!(
            fetched["result"]["structuredContent"]["ok"], true,
            "{fetched}"
        );
        let searched: Value = rpc(&c, &url, token, "2026-07-28", json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"search","arguments":{"query":"agent generated fixture text"}}})).send().await.unwrap().json().await.unwrap();
        assert_eq!(
            searched["result"]["structuredContent"]["ok"], true,
            "{searched}"
        );
        let forgotten: Value = rpc(&c, &url, token, "2026-07-28", json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"forget","arguments":{"id":id}}})).send().await.unwrap().json().await.unwrap();
        assert_eq!(
            forgotten["result"]["structuredContent"]["ok"], true,
            "{forgotten}"
        );
        let hidden: Value = rpc(&c, &url, token, "2026-07-28", json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"get","arguments":{"id":id}}})).send().await.unwrap().json().await.unwrap();
        assert_eq!(
            hidden["result"]["structuredContent"]["ok"], false,
            "{hidden}"
        );
    }
    let limited = key_issue(
        &d.path().join("keys.db"),
        PrincipalKind::Agent,
        "limited",
        "memory.read",
        now() + 300,
    )
    .unwrap();
    let r = rpc(&c,&url,&limited,"2026-07-28",json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"record","arguments":{"content":"must not save"}}})).send().await.unwrap();
    assert_eq!(r.status(), 403);
    assert!(
        r.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("insufficient_scope")
    );
    let body: Value = r.json().await.unwrap();
    assert!(body["result"]["_meta"]["mcp/www_authenticate"].is_array());
    let r = rpc(
        &c,
        &url,
        &key,
        "2026-07-28",
        json!({"jsonrpc":"2.0","id":5,"method":"tools/list"}),
    )
    .header("Origin", "https://evil.example")
    .send()
    .await
    .unwrap();
    assert_eq!(r.status(), 403);
    let r = c
        .post(format!("{url}/mcp"))
        .header("Host", "evil.example")
        .header("X-Forwarded-Host", "memory.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 421);
    task.abort();
}

#[tokio::test]
async fn official_sdk_client_api_key_and_oauth() {
    use rmcp::{
        ServiceExt,
        transport::{
            StreamableHttpClientTransport,
            streamable_http_client::StreamableHttpClientTransportConfig,
        },
    };
    let (_d, url, key, signer, task) = fixture().await;
    for token in [key, sign(&signer, &claims(), "one")] {
        let mut headers = reqwest13::header::HeaderMap::new();
        headers.insert("host", "memory.example".parse().unwrap());
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        let http = reqwest13::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .build()
            .unwrap();
        let transport = StreamableHttpClientTransport::with_client(
            http,
            StreamableHttpClientTransportConfig::with_uri(format!("{url}/mcp")),
        );
        let client = ().serve(transport).await.unwrap();
        let tools = client.list_all_tools().await.unwrap();
        assert_eq!(tools.len(), 4);
        client.cancel().await.unwrap();
    }
    task.abort();
}
