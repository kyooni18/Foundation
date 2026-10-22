//! MCP 2025-06-18 newline JSON-RPC adapter; it never opens the database.
use crate::{api::Reply, authentication::Token, domain::*, transport::http::MAX_REQUEST_BYTES};
use serde_json::{Value, json};
use std::{net::IpAddr, path::PathBuf, time::Duration};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
pub struct Client {
    endpoint: String,
    token_file: PathBuf,
    http: reqwest::Client,
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
        })
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
pub fn tools() -> Value {
    let string = json!({"type":"string"});
    let get = schema(
        json!({"id":string,"include_retracted":{"type":"boolean","default":false}}),
        &["id"],
    );
    let transition = schema(json!({"id":string,"reason":string}), &["id"]);
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
        {"name":"claim_unconfirm","description":"Remove user endorsement of a Claim.","inputSchema":transition}
    ]})
}
struct Session {
    initialized: bool,
    ready: bool,
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
            "tools/list" => Some(rpc_result(id, tools())),
            "tools/call" => {
                let params = &value["params"];
                let Some(name) = params["name"].as_str() else {
                    return Some(rpc_error(id, -32602, "invalid tool request"));
                };
                if ![
                    "record",
                    "get",
                    "list",
                    "retract",
                    "forget",
                    "claim_get",
                    "claim_list",
                    "claim_retract",
                    "claim_forget",
                    "claim_confirm",
                    "claim_unconfirm",
                ]
                .contains(&name)
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
