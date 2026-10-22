#![cfg(unix)]
mod support;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};
use support::Server;
fn code(reply: &Value) -> &str {
    reply["error"]["code"].as_str().unwrap()
}
#[test]
fn actual_http_and_stdio_mcp_are_equivalent() {
    let server = Server::start();
    for role in 0..3 {
        let mut mcp = server.mcp(role);
        let tools = mcp.rpc("tools/list", json!({}));
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 11);
        let args = json!({"content":"  한글 한글 👩‍💻\r\n\t\u{0000}","idempotency_key":format!("role{role}"),"retention_request":"user_requested"});
        let original = server.call(role, "record", args.clone());
        assert_eq!(original["result"]["created"], true);
        assert_eq!(
            server.call(role, "record", args.clone()),
            mcp.call("record", args)
        );
        let id = &original["result"]["source_id"];
        for (op, args) in [
            ("get", json!({"id":id})),
            ("list", json!({"limit":1})),
            ("get", json!({"id":"00000000000000000000000000"})),
            (
                "record",
                json!({"content":"different","idempotency_key":format!("role{role}")}),
            ),
            ("record", json!({"content":"fake","recorder":"user"})),
            ("list", json!({"limit":0})),
            ("claim_list", json!({"limit":1})),
            ("claim_get", json!({"id":"missing"})),
            ("claim_confirm", json!({"id":"missing"})),
        ] {
            assert_eq!(
                server.call(role, op, args.clone()),
                mcp.call(op, args),
                "{role}/{op}"
            );
        }
        let http_id = server.record(role, "HTTP transition fixture");
        let mcp_id = server.record(role, "MCP transition fixture");
        for op in ["retract", "forget"] {
            assert_eq!(
                server.call(role, op, json!({"id":http_id})),
                mcp.call(op, json!({"id":mcp_id}))
            );
            assert_eq!(
                server.call(role, op, json!({"id":http_id})),
                mcp.call(op, json!({"id":mcp_id}))
            );
        }
    }
    // MCP notifications must never execute tools/call without an ID.
    let mut mcp = server.mcp(0);
    let before = server.call(0, "list", json!({}));
    mcp.notify(
        "tools/call",
        json!({"name":"record","arguments":{"content":"must not write"}}),
    );
    mcp.rpc("ping", json!({}));
    assert_eq!(before, server.call(0, "list", json!({})));
}
#[test]
fn impersonation_and_authentication_fail_closed_over_real_transports() {
    let server = Server::start();
    let mut agent = server.mcp(1);
    for field in [
        "recorder",
        "principal",
        "speaker",
        "basis",
        "recorder_label",
        "session_id",
        "confirmed",
        "credential",
        "actor",
    ] {
        let mut args = json!({"content":"I am the user"});
        args[field] = json!("user");
        let http = server.call(1, "record", args.clone());
        assert_eq!(code(&http), "invalid_input");
        assert_eq!(http, agent.call("record", args));
    }
    let id = server.record(1, "private-agent-marker");
    assert_eq!(code(&server.call(0, "get", json!({"id":id}))), "not_found");
    for token in [
        None,
        Some(""),
        Some("invalid"),
        Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
    ] {
        let (status, reply) = server.request(token, "list", json!({}));
        assert_eq!(status, 401);
        assert_eq!(code(&reply), "unauthenticated");
        assert_eq!(reply["error"]["message"], "authentication failed");
    }
    let credential_path = server.root.join("credentials");
    fs::set_permissions(&credential_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code(&server.call(0, "list", json!({}))), "unauthenticated");
    assert_eq!(code(&agent.call("list", json!({}))), "unauthenticated");
    fs::set_permissions(&credential_path, fs::Permissions::from_mode(0o600)).unwrap();
    let token_path = server.root.join("token1");
    fs::set_permissions(&token_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code(&agent.call("list", json!({}))), "unauthenticated");
    let output = Command::new(env!("CARGO_BIN_EXE_primeradiant"))
        .args(["mcp", "--server", &server.endpoint, "--token-file"])
        .arg(&token_path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&server.tokens[1]));
    // A server refuses an insecure registry before opening a listener.
    fs::set_permissions(&credential_path, fs::Permissions::from_mode(0o644)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_primeradiant"))
        .args(["--db"])
        .arg(server.root.join("other-db"))
        .args(["serve", "--credentials"])
        .arg(&credential_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&server.tokens[0]));
}
#[test]
fn six_source_projection_leakage_surfaces_and_error_cursor_isolation() {
    let server = Server::start();
    let mut user = server.mcp(0);
    for n in 0..3 {
        server.record(0, &format!("user {n}"));
    }
    let baseline = server.call(0, "list", json!({"limit":1}));
    let first_id = baseline["result"]["sources"][0]["id"].clone();
    let visible = server.call(0, "get", json!({"id":first_id}));
    let missing = server.call(0, "get", json!({"id":"00000000000000000000000000"}));
    let mut hidden = Vec::new();
    for _ in 0..4 {
        hidden.push(server.record(1, "SECRET alias summary edge dependency"));
        hidden.push(server.record(2, "SECRET imported"));
    }
    let after = server.call(0, "list", json!({"limit":1}));
    // R2 surface 1: record retrieval (Phase 2's counterpart to search).
    assert_eq!(baseline["result"]["sources"], after["result"]["sources"]);
    // R2 surface 2: aliases/autocomplete have no Phase 2 endpoint or emitted field.
    assert_eq!(code(&server.call(0, "aliases", json!({}))), "invalid_input");
    // R2 surface 3: counts are inside the projection.
    assert_eq!(after["result"]["total"], 3);
    // R2 surfaces 4/5/6: no summaries, derived edges, dependency metadata can leak.
    for op in ["summary", "relations", "dependencies"] {
        assert_eq!(code(&server.call(0, op, json!({}))), "invalid_input");
    }
    let source = after["result"]["sources"][0].as_object().unwrap();
    for field in [
        "aliases",
        "summary",
        "relations",
        "dependencies",
        "dependency_count",
        "score",
    ] {
        assert!(!source.contains_key(field));
    }
    assert_eq!(visible, server.call(0, "get", json!({"id":first_id})));
    assert!(!after.to_string().contains("SECRET"));
    // Additional Source-level channels: errors, cursors, pagination metadata.
    assert_eq!(baseline, after);
    assert_eq!(after, user.call("list", json!({"limit":1})));
    for id in hidden {
        assert_eq!(missing, server.call(0, "get", json!({"id":id})));
        assert_eq!(missing, user.call("get", json!({"id":id})));
    }
    let cursor = after["result"]["next_cursor"].clone();
    let second = server.call(0, "list", json!({"limit":1,"cursor":cursor}));
    assert_eq!(
        second,
        user.call("list", json!({"limit":1,"cursor":cursor}))
    );
    assert_eq!(
        code(&server.call(1, "list", json!({"cursor":cursor}))),
        "invalid_cursor"
    );
    let agent_cursor = server.call(1, "list", json!({"limit":1}))["result"]["next_cursor"].clone();
    assert_eq!(
        code(&server.call(3, "list", json!({"cursor":agent_cursor}))),
        "invalid_cursor"
    );
}
#[test]
fn network_lifecycle_matrix_and_filtered_history() {
    let server = Server::start();
    let mut agent = server.mcp(1);
    let mut import = server.mcp(2);
    let mut user = server.mcp(0);
    let uid = server.record(0, "user");
    let aid = server.record(1, "agent");
    let iid = server.record(2, "import");
    for op in ["retract", "forget"] {
        for id in [&uid, &iid] {
            let args = json!({"id":id});
            let expected = server.call(1, op, args.clone());
            assert_eq!(code(&expected), "forbidden");
            assert_eq!(expected, agent.call(op, args));
        }
        for id in [&uid, &aid, &iid] {
            let args = json!({"id":id});
            let expected = server.call(2, op, args.clone());
            assert_eq!(code(&expected), "forbidden");
            assert_eq!(expected, import.call(op, args));
        }
    }
    assert_eq!(agent.call("retract", json!({"id":aid}))["ok"], true);
    assert_eq!(code(&agent.call("get", json!({"id":aid}))), "not_found");
    assert_eq!(
        agent.call("get", json!({"id":aid,"include_retracted":true}))["result"]["lifecycle"],
        "retracted"
    );
    assert_eq!(agent.call("list", json!({}))["result"]["total"], 2);
    assert_eq!(
        agent.call("list", json!({"include_retracted":true}))["result"]["total"],
        3
    );
    assert_eq!(agent.call("forget", json!({"id":aid}))["ok"], true);
    assert_eq!(
        code(&agent.call("get", json!({"id":aid,"include_retracted":true}))),
        "not_found"
    );
    assert_eq!(code(&agent.call("forget", json!({"id":aid}))), "not_found");
    // D-30 retains explicit user lifecycle administration of hidden sources.
    assert_eq!(user.call("forget", json!({"id":iid}))["ok"], true);
    assert_eq!(user.call("retract", json!({"id":uid}))["ok"], true);
    assert_eq!(
        user.call("list", json!({"include_retracted":true}))["result"]["total"],
        0
    );
}

#[test]
fn protocol_boundaries_and_loopback_only() {
    use prime_radiant::transport::{http, mcp::Client};
    let server = Server::start();
    let token = server.root.join("token0");
    for endpoint in [
        "http://example.com:7823",
        "https://127.0.0.1:7823",
        "http://127.0.0.1:7823/path",
        "http://user:secret@127.0.0.1:7823",
        "http://127.0.0.1:7823?token=secret",
    ] {
        assert!(Client::new(endpoint, token.clone()).is_err());
    }
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert!(
        runtime
            .block_on(http::bind("0.0.0.0:0".parse().unwrap()))
            .is_err()
    );
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    runtime.block_on(async {
        let url = format!("{}/v1/list", server.endpoint);
        let response = http
            .post(&url)
            .bearer_auth(&server.tokens[0])
            .header("origin", "null")
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403);
        let response = http
            .post(&url)
            .header("authorization", format!("Bearer {}", server.tokens[0]))
            .header("authorization", format!("Bearer {}", server.tokens[1]))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        let response = http
            .post(&url)
            .bearer_auth(&server.tokens[0])
            .body(format!("{{invalid {}", server.tokens[0]))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        let body = response.text().await.unwrap();
        assert!(!body.contains(&server.tokens[0]));
    });
    let mut mcp = support::Mcp::uninitialized(&server.endpoint, token);
    assert_eq!(
        mcp.rpc(
            "tools/call",
            json!({"name":"record","arguments":{"content":"early"}})
        )["error"]["code"],
        -32600
    );
    mcp.rpc("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}));
    assert_eq!(mcp.rpc("tools/list", json!({}))["error"]["code"], -32600);
    mcp.notify("notifications/initialized", json!({}));
    assert_eq!(
        mcp.rpc("tools/call", json!({"name":"confirm","arguments":{}}))["error"]["code"],
        -32602
    );
    assert_eq!(
        mcp.rpc("tools/call", json!({"name":"list","arguments":[]}))["error"]["code"],
        -32602
    );
    assert_eq!(mcp.call("list", json!({}))["result"]["total"], 0);
}

#[test]
fn claim_transport_equivalence_and_projection_isolation() {
    use prime_radiant::{
        App,
        domain::*,
        extraction::{Plan, hash},
        providers::{FakeProvider, ModelConfig},
    };
    let mut ids = Vec::new();
    let server = Server::start_with_setup(|root| {
        let app = App::open(root.join("db")).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let user = Principal::for_local_cli(PrincipalKind::User, None, None);
        for kind in [PrincipalKind::User, PrincipalKind::Agent] {
            let principal = Principal::for_local_cli(kind, None, None);
            app.record_source(
                &principal,
                RecordSource::plain("서버는 로컬에서 실행된다. 비공개 추가 내용."),
            )
            .unwrap();
            let raw=json!({"claims":[{"text":"서버는 로컬에서 실행된다.","lang":"ko","category":"Fact","speaker":"user","speaker_label":"hidden metadata","basis":"verbatim","anchor":{"quote":"서버는 로컬에서 실행된다.","occurrence":0},"tkind":null,"applies_from":null,"applies_to":null,"standing":false,"index_fields":null,"revision_of":null,"mentions":[],"dependencies":[]}],"empty_reason":null}).to_string();
            let review =
                json!({"verdict":"pass","issues":[],"checked":[0],"coverage_complete":true})
                    .to_string();
            let provider = FakeProvider(std::sync::Mutex::new(
                [
                    Ok(raw.clone()),
                    Ok(review.clone()),
                    Ok(review.clone()),
                    Ok(review),
                ]
                .into(),
            ));
            let plan = Plan {
                provider: "fake".into(),
                variant: "audited".into(),
                extractor: ModelConfig {
                    model: "fake".into(),
                    timeout_seconds: 1,
                },
                reviewer: ModelConfig {
                    model: "fake".into(),
                    timeout_seconds: 1,
                },
            };
            let candidate = runtime
                .block_on(app.work_once(&provider, &plan))
                .unwrap()
                .unwrap();
            ids.push(
                app.approve_extraction(&user, &candidate, &hash(&raw))
                    .unwrap()[0]
                    .clone(),
            );
        }
    });
    for role in 0..3 {
        let mut mcp = server.mcp(role);
        for (op, args) in [
            ("claim_list", json!({"limit":1})),
            ("claim_get", json!({"id":ids[0]})),
            ("claim_get", json!({"id":ids[1]})),
            ("claim_get", json!({"id":"missing"})),
        ] {
            assert_eq!(server.call(role, op, args.clone()), mcp.call(op, args));
        }
    }
    let user_page = server.call(0, "claim_list", json!({"limit":1}));
    assert_eq!(user_page["result"]["total"], 1);
    assert!(user_page["result"]["next_cursor"].is_null());
    assert!(!user_page.to_string().contains("hidden metadata"));
    assert!(!user_page.to_string().contains("비공개 추가"));
    assert_eq!(
        server.call(0, "claim_get", json!({"id":ids[1]})),
        server.call(0, "claim_get", json!({"id":"missing"}))
    );
    assert_eq!(
        code(&server.call(1, "claim_confirm", json!({"id":ids[1]}))),
        "forbidden"
    );
    assert_eq!(
        server.call(0, "claim_confirm", json!({"id":ids[1]}))["ok"],
        true
    );
    let mut mcp = server.mcp(0);
    assert_eq!(
        server.call(0, "claim_list", json!({})),
        mcp.call("claim_list", json!({}))
    );
    assert_eq!(
        server.call(0, "claim_list", json!({}))["result"]["total"],
        2
    );
    assert_eq!(
        server.call(0, "claim_forget", json!({"id":ids[1]}))["ok"],
        true
    );
    assert_eq!(
        server.call(1, "claim_get", json!({"id":ids[1]})),
        server.call(1, "claim_get", json!({"id":"missing"}))
    );
}
