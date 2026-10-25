//! MCP 2025-06-18 newline JSON-RPC adapter; it never opens the database.
use crate::{api::Reply, authentication::Token, domain::*, transport::http::MAX_REQUEST_BYTES};
use serde_json::{Value, json};
use std::{net::IpAddr, path::PathBuf, time::Duration};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
pub struct Client {
    endpoint: String,
    token_file: PathBuf,
    http: reqwest::Client,
    /// One stdio connection is one session for undeclared-dependency tracking.
    session: String,
    full_tools: bool,
}
impl Client {
    pub fn new(endpoint: &str, token_file: PathBuf) -> Result<Self> {
        let url = reqwest::Url::parse(endpoint)
            .map_err(|_| invalid("server", "invalid loopback endpoint"))?;
        let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
        if url.scheme() != "http"
            || !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(invalid("server", "numeric loopback HTTP endpoint required"));
        }
        Token::from_file(&token_file)?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| Error::Credentials)?;
        Ok(Self {
            endpoint: endpoint.trim_end_matches('/').into(),
            token_file,
            http,
            full_tools: false,
            session: format!(
                "mcp-{}",
                crate::authentication::hex(&crate::authentication::random_bytes()?[..16])
            ),
        })
    }
    /// Administrative MCP surface; regular agents use the four-tool default.
    pub fn with_full_tools(mut self) -> Self {
        self.full_tools = true;
        self
    }
    pub async fn call(&self, operation: &str, args: Value) -> Reply {
        let path = self.token_file.clone();
        let token = match tokio::task::spawn_blocking(move || Token::from_file(path)).await {
            Ok(Ok(t)) => t,
            _ => return Reply::error(Error::Credentials),
        };
        let result = self
            .http
            .post(format!("{}/v1/{operation}", self.endpoint))
            .bearer_auth(token.expose())
            .header(crate::api::Service::SESSION_HEADER, &self.session)
            .json(&args)
            .send()
            .await;
        match result {
            Ok(response) => response
                .json::<Reply>()
                .await
                .unwrap_or_else(|_| internal()),
            Err(_) => internal(),
        }
    }
    pub async fn validate(&self) -> Result<()> {
        let reply = self.call("authenticate", json!({})).await;
        if reply.ok {
            Ok(())
        } else {
            Err(Error::Credentials)
        }
    }
}
fn internal() -> Reply {
    Reply::error(Error::IntegrityFailure("transport unavailable".into()))
}
fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
fn schema(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub fn tools(full_tools: bool) -> Value {
    let string = json!({"type":"string"});
    if !full_tools {
        return json!({"tools":[
            {"name":"record","description":"Save the exact Source text. Extraction and indexing are asynchronous; saving does not make an unapproved Claim searchable.",
                "inputSchema":schema(json!({"content":string,"idempotency_key":{"type":"string"}}), &["content"])},
            {"name":"search","description":"Search visible, approved Claims. Results may be partial when extraction or semantic indexing is unavailable.",
                "inputSchema":schema(json!({"query":string,"limit":{"type":"integer","minimum":1,"maximum":100}}), &["query"])},
            {"name":"get","description":"Read an authorized original Source by its ID, for verification and citations.",
                "inputSchema":schema(json!({"id":string}), &["id"])},
            {"name":"forget","description":"Forget an authorized Source by ID; all derived Claims become invisible. Audit history is retained.",
                "inputSchema":schema(json!({"id":string}), &["id"])},
        ]});
    }
    let get = schema(
        json!({"id":string,"include_retracted":{"type":"boolean","default":false}}),
        &["id"],
    );
    let transition = schema(json!({"id":string,"reason":string}), &["id"]);
    let id = schema(json!({ "id": string }), &["id"]);
    json!({"tools":[
        {"name":"record","description":"Record exact source text using the authenticated recorder role.","inputSchema":schema(json!({"content":string,"media_type":{"enum":["text/plain","text/markdown"]},"authored_at":string,"observed_at":string,"artifact_ref":string,"retention_request":{"enum":["none","user_requested"]},"idempotency_key":string}),&["content"])},
        {"name":"get","description":"Get one source inside the authenticated projection.","inputSchema":get},
        {"name":"list","description":"List visible sources with a credential-bound cursor.","inputSchema":schema(json!({"limit":{"type":"integer","minimum":1,"maximum":100,"default":50},"include_retracted":{"type":"boolean","default":false},"cursor":string}),&[])},
        {"name":"retract","description":"Retract a source when authorized.","inputSchema":transition},
        {"name":"forget","description":"Hide a source while retaining its audit record, when authorized.","inputSchema":transition},
        {"name":"claim_get","description":"Read a visible current Claim.","inputSchema":schema(json!({"id":string,"include_retracted":{"type":"boolean"}}),&["id"])},
        {"name":"claim_list","description":"List only visible current Claims.","inputSchema":schema(json!({"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":string,"include_retracted":{"type":"boolean"}}),&[])},
        {"name":"claim_retract","description":"Retract an authorized Claim.","inputSchema":transition},
        {"name":"claim_forget","description":"Forget an authorized Claim.","inputSchema":transition},
        {"name":"claim_confirm","description":"User endorsement of a Claim.","inputSchema":transition},
        {"name":"claim_unconfirm","description":"Remove user endorsement of a Claim.","inputSchema":transition},
        {"name":"restore","description":"Restore a retracted or forgotten source when authorized.","inputSchema":transition},
        {"name":"claim_restore","description":"Restore an authorized Claim.","inputSchema":transition},
        {"name":"claims_as_of","description":"Temporal reading of visible current Claims at an instant; superseded Claims are returned as history.","inputSchema":schema(json!({"as_of":string,"present":{"type":"boolean"},"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":string}),&[])},
        {"name":"claim_dependencies","description":"Dependencies of a visible Claim inside the projection.","inputSchema":get},
        {"name":"claim_support","description":"Distinct origin Sources for a visible Claim (not a probability).","inputSchema":id},
        {"name":"claim_relations","description":"Temporal relations of a visible Claim.","inputSchema":id},
        {"name":"dependency_add","description":"Declare a dependency between existing Claims.","inputSchema":schema(json!({"child":string,"parent":string,"kind":{"enum":["premise","restates","alternative","informed_by"]}}),&["child","parent","kind"])},
        {"name":"citation_add","description":"Record that one Source cites another (provenance only).","inputSchema":schema(json!({"child_source":string,"parent_source":string}),&["child_source","parent_source"])},
        {"name":"entity_create","description":"Create a thin entity ID.","inputSchema":schema(json!({"kind":string}),&["kind"])},
        {"name":"entity_get","description":"Resolve an entity inside the projection.","inputSchema":id},
        {"name":"identity_assert","description":"Assert refers_to, same_as, distinct_from or alias.","inputSchema":schema(json!({"kind":{"enum":["refers_to","same_as","distinct_from","alias"]},"subject":string,"object":string,"reason":string}),&["kind","subject","object"])},
        {"name":"relation_add","description":"Assert a supersedes, exception or backfill relation.","inputSchema":schema(json!({"kind":{"enum":["supersedes","exception","backfill"]},"subject":string,"object":string,"reason":string,"judgment_id":string}),&["kind","subject","object"])},
        {"name":"integrity_lifecycle","description":"Retract, forget or restore a dependency, citation, identity assertion or relation.","inputSchema":schema(json!({"target":{"enum":["dependency","citation","identity_assertion","relation"]},"id":string,"event":{"enum":["retract","forget","restore"]},"reason":string}),&["target","id","event"])},
        {"name":"search","description":"Adaptive local retrieval over visible Claims: deterministic query plan, exact/lexical/semantic evidence, bounded graph and temporal expansion. Reports degraded or incomplete coverage; never claims exhaustiveness the indexes cannot back.","inputSchema":schema(json!({"query":string,"intent":{"enum":["auto","exact","lexical","semantic","relational","temporal","comprehensive"]},"depth":{"enum":["auto","focused","balanced","exhaustive"]},"filters":{"type":"object","properties":{"category":{"type":"array","items":string},"speaker":{"type":"array","items":string},"lang":{"type":"array","items":string},"tkind":{"type":"array","items":string},"source_id":{"type":"array","items":string},"confirmed":{"type":"boolean"},"standing":{"type":"boolean"},"created_after":string,"created_before":string,"entity":string},"additionalProperties":false},"as_of":string,"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":string,"include_invalid":{"type":"boolean"},"strategy":{"enum":["adaptive","lexical_only","semantic_only","fused_all"]}}),&["query"])},
        {"name":"source_processing","description":"Processing stage of a visible Source (stored, awaiting approval, awaiting indexing, indexed, failed).","inputSchema":id},
        {"name":"queue_status","description":"Owner-only aggregate queue health, backlog, retries, timing percentiles and index progress.","inputSchema":schema(json!({}),&[])},
        {"name":"queue_jobs","description":"Owner-only list of jobs with safe diagnostic codes.","inputSchema":schema(json!({"state":{"enum":["pending","leased","done","failed","deferred","cancelled"]},"limit":{"type":"integer","minimum":1,"maximum":500}}),&[])},
        {"name":"queue_retry","description":"Owner-only explicit retry of a failed, deferred or cancelled job.","inputSchema":id},
    ]})
}
struct Session {
    initialized: bool,
    ready: bool,
    full_tools: bool,
}
impl Session {
    async fn receive(&mut self, client: &Client, value: Value) -> Option<Value> {
        let Some(object) = value.as_object() else {
            return Some(rpc_error(Value::Null, -32600, "invalid request"));
        };
        let id = object.get("id").cloned();
        if value["jsonrpc"] != "2.0"
            || !value["method"].is_string()
            || id
                .as_ref()
                .is_some_and(|v| !v.is_string() && !v.is_i64() && !v.is_u64())
        {
            return Some(rpc_error(Value::Null, -32600, "invalid request"));
        }
        let method = value["method"].as_str().unwrap_or("");
        let Some(id) = id else {
            // Notifications never perform application operations and never receive replies.
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            return None;
        };
        if method == "initialize" {
            if self.initialized
                || !value["params"]["protocolVersion"].is_string()
                || !value["params"]["capabilities"].is_object()
                || !value["params"]["clientInfo"].is_object()
            {
                return Some(rpc_error(id, -32602, "invalid initialization"));
            }
            self.initialized = true;
            return Some(rpc_result(
                id,
                json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"primeradiant","version":env!("CARGO_PKG_VERSION")}}),
            ));
        }
        if method == "ping" {
            return Some(rpc_result(id, json!({})));
        }
        if !self.ready {
            return Some(rpc_error(id, -32600, "initialization required"));
        }
        match method {
            "tools/list" => Some(rpc_result(id, tools(self.full_tools))),
            "tools/call" => {
                let params = &value["params"];
                let Some(name) = params["name"].as_str() else {
                    return Some(rpc_error(id, -32602, "invalid tool request"));
                };
                let known = tools(self.full_tools);
                if !known["tools"]
                    .as_array()
                    .expect("static tools")
                    .iter()
                    .any(|t| t["name"] == name)
                {
                    return Some(rpc_error(id, -32602, "unknown tool"));
                }
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                if !args.is_object() {
                    return Some(rpc_error(id, -32602, "invalid tool arguments"));
                }
                let reply = client.call(name, args).await;
                Some(rpc_result(
                    id,
                    json!({"content":[{"type":"text","text":serde_json::to_string(&reply).expect("reply serialization")}],"structuredContent":reply,"isError":!reply.ok}),
                ))
            }
            _ => Some(rpc_error(id, -32601, "method not found")),
        }
    }
}
/// Bounded line framing; no stdout diagnostics and no unbounded read_line allocation.
pub async fn run<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    client: Client,
    mut input: R,
    mut output: W,
) -> Result<()> {
    client.validate().await?;
    let mut session = Session {
        initialized: false,
        ready: false,
        full_tools: client.full_tools,
    };
    let mut line = Vec::new();
    loop {
        let available = input.fill_buf().await?;
        if available.is_empty() {
            break;
        }
        let n = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        if line.len() + n > MAX_REQUEST_BYTES {
            return Err(invalid("request", "frame too large"));
        }
        let complete = available[n - 1] == b'\n';
        line.extend_from_slice(&available[..n]);
        input.consume(n);
        if !complete {
            continue;
        }
        let response = match serde_json::from_slice::<Value>(&line) {
            Ok(v) => session.receive(&client, v).await,
            Err(_) => Some(rpc_error(Value::Null, -32700, "parse error")),
        };
        line.clear();
        if let Some(response) = response {
            let bytes = serde_json::to_vec(&response)
                .map_err(|_| invalid("response", "serialization failed"))?;
            output.write_all(&bytes).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
    Ok(())
}
