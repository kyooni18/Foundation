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
fn compact_mcp_exposes_only_four_safe_everyday_operations() {
    let server = Server::start();
    let mut mcp = server.mcp_compact(1);
    let tools = mcp.rpc("tools/list", json!({}));
    let listed = tools["result"]["tools"].as_array().unwrap();
    let names: Vec<_> = listed
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["record", "search", "get", "forget"]);
    // Compact schemas avoid expert-mode tuning knobs.
    assert_eq!(
        listed[1]["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    for hidden in [
        "queue_status",
        "claim_forget",
        "relation_add",
        "source_processing",
    ] {
        assert_eq!(
            mcp.rpc("tools/call", json!({"name": hidden, "arguments": {}}))["error"]["code"],
            -32602,
            "{hidden} must not be callable through the default surface"
        );
    }
    let record = mcp.call("record", json!({"content":"한국어 원문 Source 검증"}));
    assert_eq!(record["ok"], true);
    let id = record["result"]["source_id"].as_str().unwrap();
    assert_eq!(
        mcp.call("get", json!({"id":id}))["result"]["content"],
        "한국어 원문 Source 검증"
    );
    assert!(
        mcp.call("search", json!({"query":"원문"}))["result"]["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(mcp.call("forget", json!({"id":id}))["ok"], true);
    assert_eq!(mcp.call("get", json!({"id":id}))["ok"], false);
}
#[test]
fn actual_http_and_stdio_mcp_are_equivalent() {
    let server = Server::start();
    for role in 0..3 {
        let mut mcp = server.mcp(role);
        let tools = mcp.rpc("tools/list", json!({}));
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 29);
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
    let private_from = "2031-04-05T19:15:00+09:00";
    let private_to = "2031-04-05T20:15:00+09:00";
    let source_text = "서버는 로컬에서 실행된다. 비공개 추가 내용: 별도 일정 2031-04-05T19:15:00+09:00부터 2031-04-05T20:15:00+09:00까지.";
    let mut ids = Vec::new();
    let server = Server::start_with_setup(|root| {
        let app = App::open(root.join("db")).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let user = Principal::for_local_cli(PrincipalKind::User, None, None);
        for kind in [PrincipalKind::User, PrincipalKind::Agent] {
            let principal = Principal::for_local_cli(kind, None, None);
            app.record_source(&principal, RecordSource::plain(source_text))
                .unwrap();
            let mut value = json!({"claims":[{"text":"서버는 로컬에서 실행된다.","lang":"ko","category":"Fact","speaker":"user","speaker_label":"hidden metadata","basis":"verbatim","anchor":{"quote":"서버는 로컬에서 실행된다.","occurrence":0},"tkind":null,"applies_from":private_from,"applies_to":private_to,"standing":false,"index_fields":null,"revision_of":null,"mentions":[],"dependencies":[]}],"empty_reason":null});
            if kind == PrincipalKind::Agent {
                value["claims"][0]["speaker"] = json!("agent");
                value["claims"][0]["basis"] = json!("paraphrase");
                value["claims"][0]["anchor"]["quote"] = json!(source_text);
            }
            // Untrusted proposal metadata must not widen the confirmed text's scope.
            let raw = value.to_string();
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
                prompt_version: None,
                provider: "fake".into(),
                variant: "audited".into(),
                extractor: ModelConfig {
                    max_output_tokens: None,
                    model: "fake".into(),
                    timeout_seconds: 1,
                },
                reviewer: ModelConfig {
                    max_output_tokens: None,
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
    assert_eq!(user_page["result"]["claims"][0]["lifecycle"], "live");
    assert_eq!(user_page["result"]["claims"][0]["source_lifecycle"], "live");
    assert_eq!(user_page["result"]["total"], 1);
    assert!(user_page["result"]["claims"][0]["applies_from"].is_null());
    assert!(user_page["result"]["claims"][0]["applies_to"].is_null());
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
    let confirmed = server.call(0, "claim_get", json!({"id":ids[1]}));
    assert_eq!(confirmed, mcp.call("claim_get", json!({"id":ids[1]})));
    assert!(confirmed["result"]["applies_from"].is_null());
    assert!(confirmed["result"]["applies_to"].is_null());
    assert!(!confirmed.to_string().contains("2031-04-05"));
    assert!(!confirmed.to_string().contains("비공개 추가"));
    let audited = server.call(1, "claim_get", json!({"id":ids[1]}));
    assert_eq!(
        audited["result"]["applies_from"],
        "2031-04-05T10:15:00.000Z"
    );
    assert_eq!(audited["result"]["applies_to"], "2031-04-05T11:15:00.000Z");
    assert!(
        audited["result"]["anchor_quote"]
            .as_str()
            .unwrap()
            .contains(private_from)
    );
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
#[path = "support/synthetic.rs"]
mod synthetic;
#[test]
fn integrity_operations_are_equivalent_and_never_leak_across_projections() {
    use prime_radiant::{App, domain::*, integrity::temporal::TemporalJudgment};
    let mut ids = std::collections::BTreeMap::<&str, String>::new();
    let server = Server::start_with_setup(|root| {
        let db = root.join("db");
        drop(App::open(&db).unwrap());
        let conn = synthetic::connect(&db);
        let spec = |recorder, texts: &[&str]| synthetic::SourceSpec {
            recorder,
            session: None,
            captured_at: "2026-01-01T00:00:00.000Z",
            authored_at: None,
            claims: texts
                .iter()
                .map(|t| synthetic::ClaimSpec::plain(*t))
                .collect(),
        };
        let (us, u) = synthetic::source(&conn, &spec("user", &["user premise.", "user child."]));
        let (as_, a) = synthetic::source(&conn, &spec("agent", &["agent secret about Minsu."]));
        let (fs_, f) = synthetic::source(&conn, &spec("user", &["forgotten premise."]));
        let (_, v) = synthetic::source(&conn, &spec("user", &["user via forgotten."]));
        synthetic::dependency(&conn, &u[1], &u[0], "premise");
        synthetic::dependency(&conn, &a[0], &u[0], "premise");
        synthetic::dependency(&conn, &v[0], &f[0], "premise");
        let mention = synthetic::mention(&conn, &a[0], "Minsu");
        drop(conn);
        let app = App::open(&db).unwrap();
        let user = Principal::for_local_cli(PrincipalKind::User, None, None);
        let agent = Principal::for_local_cli(PrincipalKind::Agent, None, None);
        app.set_source_lifecycle(&user, fs_.parse().unwrap(), LifecycleEvent::Forget, None)
            .unwrap();
        app.add_citation(&agent, &as_.parse().unwrap(), &us.parse().unwrap())
            .unwrap();
        let eu = app.create_entity(&user, "person").unwrap();
        let ea = app.create_entity(&agent, "person").unwrap();
        app.assert_identity(&agent, "alias", &eu, "AgentSecretAlias", None)
            .unwrap();
        app.assert_identity(&agent, "refers_to", &mention, &eu, None)
            .unwrap();
        app.assert_relation(&agent, "supersedes", &a[0], &u[0], None, None)
            .unwrap();
        let judgment = app
            .record_temporal_judgment(&TemporalJudgment {
                claim_a: u[1].clone(),
                claim_b: u[0].clone(),
                provider: "fake".into(),
                model_id: "fake".into(),
                prompt_hash: "p".into(),
                input_digest: "i".into(),
                raw_label: "supersedes".into(),
                output_json: "{}".into(),
            })
            .unwrap();
        for (k, id) in [
            ("us", us),
            ("u0", u[0].clone()),
            ("u1", u[1].clone()),
            ("as", as_),
            ("a0", a[0].clone()),
            ("fs", fs_),
            ("f0", f[0].clone()),
            ("v0", v[0].clone()),
            ("eu", eu),
            ("ea", ea),
            ("mention", mention),
            ("judgment", judgment),
        ] {
            ids.insert(k, id);
        }
    });
    let id = |k: &str| ids[k].clone();
    let missing = "01ZZZZZZZZZZZZZZZZZZZZZZZZ";
    let reads = |hidden: &str| {
        vec![
            ("claim_get", json!({"id":hidden})),
            ("claim_get", json!({"id":hidden,"include_retracted":true})),
            ("claim_dependencies", json!({"id":hidden})),
            ("claim_support", json!({"id":hidden})),
            ("claim_relations", json!({"id":hidden})),
            ("entity_get", json!({"id":hidden})),
        ]
    };
    let writes = |hidden: &str, missing_source: &str| {
        vec![
            (
                "identity_assert",
                json!({"kind":"refers_to","subject":hidden,"object":id("eu")}),
            ),
            (
                "relation_add",
                json!({"kind":"supersedes","subject":id("a0"),"object":hidden}),
            ),
            (
                "dependency_add",
                json!({"child":id("a0"),"parent":hidden,"kind":"informed_by"}),
            ),
            (
                "citation_add",
                json!({"child_source":id("as"),"parent_source":missing_source}),
            ),
            ("claim_retract", json!({"id":hidden})),
        ]
    };
    let check = |role: usize, mcp: &mut support::Mcp, hidden: &Value, absent: &Value| {
        let (op, args) = (hidden[0].as_str().unwrap(), hidden[1].clone());
        let http = server.call(role, op, args.clone());
        assert_eq!(http, mcp.call(op, args), "{role} {op} {hidden}");
        let expected = server.call(role, absent[0].as_str().unwrap(), absent[1].clone());
        assert_eq!(http["error"]["code"], "not_found", "{role} {op}: {http}");
        assert_eq!(http["error"], expected["error"], "{role} {op}");
        http.to_string()
    };
    let pair = |(op, args): (&str, Value)| json!([op, args]);
    // Hidden and absent records are indistinguishable for user reads, over both transports.
    let mut user_text = String::new();
    let mut mcp = server.mcp(0);
    for hidden in ["as", "a0", "fs", "f0", "v0", "ea", "mention"] {
        for (h, a) in reads(&id(hidden)).into_iter().zip(reads(missing)) {
            user_text += &check(0, &mut mcp, &pair(h), &pair(a));
        }
    }
    // Non-user roles get the same answer for forgotten and absent records on every operation.
    for role in [1, 3] {
        let mut mcp = server.mcp(role);
        for hidden in ["f0", "fs"] {
            let probes = reads(&id(hidden))
                .into_iter()
                .chain(writes(&id(hidden), &id(hidden)));
            let absent = reads(missing)
                .into_iter()
                .chain(writes(missing, "00000000000000000000000000"));
            for (h, a) in probes.zip(absent) {
                check(role, &mut mcp, &pair(h), &pair(a));
            }
        }
    }
    // D-30: the user may author edges onto any existing record, but user reads never show them.
    let u1 = id("u1");
    for (op, args) in [
        (
            "relation_add",
            json!({"kind":"supersedes","subject":u1,"object":id("a0")}),
        ),
        (
            "dependency_add",
            json!({"child":u1,"parent":id("a0"),"kind":"informed_by"}),
        ),
    ] {
        assert_eq!(server.call(0, op, args.clone())["ok"], true, "{op}");
    }
    // Visible reads: equivalent, and free of agent-only or forgotten material.
    for (op, args) in [
        ("claim_list", json!({})),
        ("claims_as_of", json!({"as_of":"2026-06-01T00:00:00Z"})),
        ("claim_get", json!({"id":id("u0")})),
        ("claim_dependencies", json!({"id":id("u0")})),
        (
            "claim_dependencies",
            json!({"id":id("u0"),"include_retracted":true}),
        ),
        ("claim_support", json!({"id":id("u0")})),
        ("claim_relations", json!({"id":id("u0")})),
        ("claim_relations", json!({"id":id("u1")})),
        ("claim_dependencies", json!({"id":id("u1")})),
        ("claim_support", json!({"id":id("u1")})),
        ("entity_get", json!({"id":id("eu")})),
    ] {
        let http = server.call(0, op, args.clone());
        assert_eq!(http, mcp.call(op, args.clone()), "{op}");
        assert_eq!(http["ok"], true, "{op}: {http}");
        user_text += &http.to_string();
    }
    for k in ["as", "a0", "fs", "f0", "v0", "ea", "mention", "judgment"] {
        assert!(!user_text.contains(&id(k)), "user output leaks {k}");
    }
    for secret in [
        "agent secret",
        "AgentSecretAlias",
        "forgotten premise",
        "user via forgotten",
        "Minsu",
        "declared_by",
        "judgments",
    ] {
        assert!(!user_text.contains(secret), "user output leaks {secret}");
    }
    assert_eq!(
        server.call(0, "claim_list", json!({}))["result"]["total"],
        2
    );
    assert_eq!(
        server.call(0, "claim_support", json!({"id":id("u0")}))["result"],
        json!({"sources":1,"independence_unknown":0,"claims":1})
    );
    assert_eq!(
        server.call(0, "claim_dependencies", json!({"id":id("u0")}))["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        server.call(0, "claim_relations", json!({"id":id("u0")}))["result"]["relations"],
        json!([])
    );
    // The audit view sees the agent's records, but a model judgment stays non-authoritative.
    let audit = server.call(1, "claim_relations", json!({"id":id("u1")}));
    assert_eq!(
        audit,
        server
            .mcp(1)
            .call("claim_relations", json!({"id":id("u1")}))
    );
    // Only the user's own relation; the model's supersedes label created none.
    let relations = audit["result"]["relations"].as_array().unwrap();
    assert_eq!(relations.len(), 1);
    assert_eq!(
        (
            relations[0]["actor"].as_str(),
            relations[0]["object"].as_str()
        ),
        (Some("user"), Some(id("a0").as_str()))
    );
    assert_eq!(audit["result"]["judgments"][0]["id"], id("judgment"));
    assert_eq!(
        server.call(1, "claim_dependencies", json!({"id":id("u0")}))["result"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        server
            .call(1, "entity_get", json!({"id":id("eu")}))
            .to_string()
            .contains("AgentSecretAlias")
    );
    // Agents and imports cannot override or impersonate user authority.
    let user_dep =
        server.call(0, "claim_dependencies", json!({"id":id("u0")}))["result"][0]["id"].clone();
    for (role, op, args, want) in [
        (
            1,
            "integrity_lifecycle",
            json!({"target":"dependency","id":user_dep,"event":"retract"}),
            "forbidden",
        ),
        (1, "claim_retract", json!({"id":id("u0")}), "forbidden"),
        (
            1,
            "dependency_add",
            json!({"child":id("u1"),"parent":id("a0"),"kind":"informed_by"}),
            "forbidden",
        ),
        (
            1,
            "citation_add",
            json!({"child_source":id("us"),"parent_source":id("as")}),
            "forbidden",
        ),
        (
            2,
            "identity_assert",
            json!({"kind":"alias","subject":id("eu"),"object":"x"}),
            "forbidden",
        ),
        (2, "entity_create", json!({"kind":"person"}), "forbidden"),
        (
            2,
            "relation_add",
            json!({"kind":"supersedes","subject":id("u1"),"object":id("u0")}),
            "forbidden",
        ),
        (
            1,
            "relation_add",
            json!({"kind":"contradiction","subject":id("u1"),"object":id("u0")}),
            "invalid_input",
        ),
        (0, "claim_list", json!({"actor":"user"}), "invalid_input"),
    ] {
        let http = server.call(role, op, args.clone());
        assert_eq!(http, server.mcp(role).call(op, args), "{role} {op}");
        assert_eq!(code(&http), want, "{role} {op}: {http}");
    }
    assert_eq!(
        server.call(0, "claim_list", json!({}))["result"]["total"],
        2
    );
    // Cursors are bound to the principal that received them.
    for op in ["claim_list", "claims_as_of"] {
        let page = server.call(1, op, json!({"limit":1}));
        let cursor = page["result"]["next_cursor"].clone();
        assert!(cursor.is_string(), "{op}: {page}");
        let garbage = |role| server.call(role, op, json!({"limit":1,"cursor":"00.00"}));
        for role in [0, 3] {
            let stolen = server.call(role, op, json!({"limit":1,"cursor":cursor}));
            assert_eq!(stolen, garbage(role), "{op} role {role}");
            assert_eq!(code(&stolen), "invalid_cursor");
        }
    }
}
