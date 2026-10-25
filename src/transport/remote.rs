//! Opt-in public MCP resource server. Identity issuance belongs to the external AS.
#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
use crate::{
    api::Service,
    authentication::{hex, random_bytes, secure_read},
    domain::*,
};
use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use rmcp::{
    RoleServer, ServerHandler,
    model::*,
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub const SCOPES: [&str; 3] = ["memory.read", "memory.write", "memory.forget"];
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub subject: String,
    /// Exact access-token azp (or client_id) claim; not a redirect URI.
    pub client_id: String,
    pub kind: PrincipalKind,
    pub label: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub resource: String,
    pub issuer: String,
    pub discovery_url: String,
    pub jwks_uri: String,
    pub introspection: Introspection,
    /// Optional private registry written only by the bounded DCR admission gate.
    #[serde(default)]
    pub dcr_registry: Option<PathBuf>,
    pub keys: PathBuf,
    pub bindings: Vec<Binding>,
    /// Numeric loopback only. Reverse proxy must preserve the canonical Host.
    pub bind: std::net::SocketAddr,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Introspection {
    pub endpoint: String,
    pub client_id: String,
    pub secret_file: PathBuf,
}
fn https(s: &str) -> Result<reqwest::Url> {
    let u = reqwest::Url::parse(s).map_err(|_| Error::Credentials)?;
    if u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || u.query().is_some()
    {
        return Err(Error::Credentials);
    }
    Ok(u)
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let c: Self =
            serde_json::from_slice(&secure_read(path)?).map_err(|_| Error::Credentials)?;
        c.validate()?;
        Ok(c)
    }
    fn validate(&self) -> Result<()> {
        let resource = https(&self.resource)?;
        if resource.path() != "/mcp" || resource.port().is_some() || !self.bind.ip().is_loopback() {
            return Err(Error::Credentials);
        }
        let issuer = https(&self.issuer)?;
        for url in [
            &self.discovery_url,
            &self.jwks_uri,
            &self.introspection.endpoint,
        ] {
            if https(url)?.origin() != issuer.origin() {
                return Err(Error::Credentials);
            }
        }
        if self.introspection.client_id.is_empty() {
            return Err(Error::Credentials);
        }
        secure_read(&self.introspection.secret_file)?;
        let mut seen = std::collections::HashSet::new();
        for b in &self.bindings {
            if b.subject.is_empty()
                || b.client_id.is_empty()
                || b.label.is_empty()
                || b.label.len() > 200
                || !seen.insert((&b.subject, &b.client_id))
            {
                return Err(Error::Credentials);
            }
        }
        Ok(())
    }
}
fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
fn key_db(path: &Path) -> Result<rusqlite::Connection> {
    // File must be provisioned locally, private, owned by this uid, and not a symlink.
    secure_read(path)?;
    rusqlite::Connection::open(path).map_err(Error::from)
}
/// Local maintenance only. Raw keys are printed once, never persisted here.
pub fn key_init(path: &Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    let db = key_db(path)?;
    db.execute_batch("CREATE TABLE api_keys (id TEXT PRIMARY KEY, digest BLOB NOT NULL UNIQUE, kind TEXT NOT NULL, label TEXT NOT NULL, scopes TEXT NOT NULL, expires INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0);")?;
    Ok(())
}
pub fn key_issue(
    path: &Path,
    kind: PrincipalKind,
    label: &str,
    scopes: &str,
    expires: i64,
) -> Result<String> {
    if expires <= now()
        || label.is_empty()
        || label.len() > 200
        || scopes.split_whitespace().any(|s| !SCOPES.contains(&s))
    {
        return Err(Error::Credentials);
    }
    let raw = format!("prk_{}", hex(&random_bytes()?));
    let digest = Sha256::digest(raw.as_bytes());
    key_db(path)?.execute(
        "INSERT INTO api_keys(id,digest,kind,label,scopes,expires) VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![
            hex(&digest[..8]),
            digest.as_slice(),
            kind.as_str(),
            label,
            scopes,
            expires
        ],
    )?;
    Ok(raw)
}
pub fn key_revoke(path: &Path, id: &str) -> Result<()> {
    if key_db(path)?.execute("UPDATE api_keys SET revoked=1 WHERE id=?1", [id])? != 1 {
        return Err(Error::Credentials);
    }
    Ok(())
}
pub fn key_list(path: &Path) -> Result<Value> {
    let db = key_db(path)?;
    let mut query =
        db.prepare("SELECT id,kind,label,scopes,expires,revoked FROM api_keys ORDER BY id")?;
    let rows = query.query_map([], |r| Ok(json!({"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"label":r.get::<_,String>(2)?,"scopes":r.get::<_,String>(3)?,"expires":r.get::<_,i64>(4)?,"revoked":r.get::<_,i64>(5)? != 0})))?;
    Ok(Value::Array(
        rows.collect::<std::result::Result<Vec<_>, _>>()?,
    ))
}
#[derive(Clone)]
struct Identity {
    principal: Principal,
    scopes: Vec<String>,
}
fn principal(kind: PrincipalKind, label: String, identity: &[u8]) -> Principal {
    Principal {
        kind,
        label: Some(label),
        credential: Some(Sha256::digest(identity).into()),
        session_id: None,
    }
}
fn api_key(path: &Path, raw: &str) -> Result<Identity> {
    if raw.len() != 68
        || !raw.starts_with("prk_")
        || !raw[4..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error::Unauthenticated);
    }
    let digest = Sha256::digest(raw.as_bytes());
    let db = key_db(path)?;
    let row = db
        .query_row(
            "SELECT kind,label,scopes FROM api_keys WHERE digest=?1 AND revoked=0 AND expires>?2",
            rusqlite::params![digest.as_slice(), now()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|_| Error::Unauthenticated)?;
    Ok(Identity {
        principal: principal(row.0.parse()?, row.1, raw.as_bytes()),
        scopes: row.2.split_whitespace().map(str::to_owned).collect(),
    })
}
#[derive(Deserialize, Serialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: Value,
    exp: u64,
    iat: u64,
    scope: String,
    azp: Option<String>,
    client_id: Option<String>,
}
struct Cache {
    keys: JwkSet,
    fetched: Instant,
}
struct Remote {
    config: Config,
    http: reqwest::Client,
    cache: tokio::sync::Mutex<Cache>,
    slots: Arc<tokio::sync::Semaphore>,
    rate: std::sync::Mutex<(Instant, u32)>,
}
impl Remote {
    fn client_matches(&self, configured: &str, actual: &str) -> bool {
        if configured == actual {
            return true;
        }
        let Some(tag) = configured.strip_prefix("dcr:") else {
            return false;
        };
        let Some(path) = &self.config.dcr_registry else {
            return false;
        };
        if secure_read(path).is_err() {
            return false;
        }
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).and_then(|db| db.query_row("SELECT EXISTS(SELECT 1 FROM clients WHERE client_id=?1 AND tag=?2 AND enabled=1)", [actual, tag], |r| r.get::<_,bool>(0))).unwrap_or(false)
    }
    async fn active(&self, token: &str) -> Result<()> {
        let path = self.config.introspection.secret_file.clone();
        let bytes = tokio::task::spawn_blocking(move || secure_read(&path))
            .await
            .map_err(|_| Error::Credentials)??;
        let secret = std::str::from_utf8(&bytes)
            .map_err(|_| Error::Credentials)?
            .trim_end_matches('\n');
        if secret.is_empty() || secret.len() > 4096 {
            return Err(Error::Credentials);
        }
        let response = self
            .http
            .post(&self.config.introspection.endpoint)
            .basic_auth(&self.config.introspection.client_id, Some(secret))
            .form(&[("token", token), ("token_type_hint", "access_token")])
            .send()
            .await
            .map_err(|_| Error::Credentials)?;
        if !response.status().is_success() {
            return Err(Error::Credentials);
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Credentials)? {
            if bytes.len() + chunk.len() > 65536 {
                return Err(Error::Credentials);
            }
            bytes.extend_from_slice(&chunk);
        }
        let result: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Credentials)?;
        if result["active"] != true {
            return Err(Error::Unauthenticated);
        }
        Ok(())
    }
    async fn fetch<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| Error::Credentials)?;
        if !response.status().is_success() {
            return Err(Error::Credentials);
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Credentials)? {
            if bytes.len() + chunk.len() > 256 * 1024 {
                return Err(Error::Credentials);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Credentials)
    }
    async fn oauth(&self, raw: &str) -> Result<Identity> {
        let h = jsonwebtoken::decode_header(raw).map_err(|_| Error::Unauthenticated)?;
        if h.alg != Algorithm::RS256 {
            return Err(Error::Unauthenticated);
        }
        let kid = h.kid.ok_or(Error::Unauthenticated)?;
        let mut cache = self.cache.lock().await;
        // Periodic refresh bounds both stale signing keys and attacker-triggered fetches.
        if cache.fetched.elapsed() > Duration::from_secs(60) {
            let fresh = self.fetch(&self.config.jwks_uri).await;
            cache.fetched = Instant::now();
            cache.keys = JwkSet { keys: vec![] };
            cache.keys = fresh?;
        }
        if cache.keys.keys.is_empty() {
            return Err(Error::Credentials);
        }
        let jwk = cache.keys.find(&kid).ok_or(Error::Unauthenticated)?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| Error::Unauthenticated)?;
        drop(cache);
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.config.resource]);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub", "iat"]);
        validation.validate_nbf = true;
        validation.leeway = 5;
        let c = jsonwebtoken::decode::<Claims>(raw, &key, &validation)
            .map_err(|_| Error::Unauthenticated)?
            .claims;
        if c.iat > (now() + 5) as u64 || c.exp <= c.iat || c.exp - c.iat > 3600 {
            return Err(Error::Unauthenticated);
        }
        if c.azp.is_some() && c.client_id.is_some() && c.azp != c.client_id {
            return Err(Error::Unauthenticated);
        }
        let client = c.azp.or(c.client_id).ok_or(Error::Unauthenticated)?;
        let b = self
            .config
            .bindings
            .iter()
            .find(|b| b.subject == c.sub && self.client_matches(&b.client_id, &client))
            .ok_or(Error::Unauthenticated)?;
        let identity =
            serde_json::to_vec(&(&c.iss, &c.sub, &client)).map_err(|_| Error::Credentials)?;
        Ok(Identity {
            principal: principal(b.kind, b.label.clone(), &identity),
            scopes: c.scope.split_whitespace().map(str::to_owned).collect(),
        })
    }
    fn challenge(&self, status: StatusCode, error: &str) -> Response {
        let origin = self.config.resource.trim_end_matches("/mcp");
        let value = format!(
            "Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\", scope=\"{}\", error=\"{error}\"",
            SCOPES.join(" ")
        );
        let mut r = status.into_response();
        r.headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        r.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            value.parse().expect("validated URLs"),
        );
        r
    }
}
async fn guard(State(state): State<Arc<Remote>>, mut request: Request, next: Next) -> Response {
    // Never trust Forwarded/X-Forwarded-* for identity, Host, scheme, or client IP.
    let expected = reqwest::Url::parse(&state.config.resource).expect("validated");
    let host = &state.config.resource[8..state.config.resource.len() - 4];
    if request.headers().get_all(header::HOST).iter().count() != 1
        || request
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            != Some(host)
    {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    if request.headers().get_all(header::ORIGIN).iter().count() > 1 {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(origin) = request.headers().get(header::ORIGIN)
        && origin.to_str().ok() != Some(expected.origin().ascii_serialization().as_str()) {
            return StatusCode::FORBIDDEN.into_response();
        }
    let Ok(_permit) = state.slots.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    {
        let mut rate = state.rate.lock().expect("rate lock");
        if rate.0.elapsed() >= Duration::from_secs(60) {
            *rate = (Instant::now(), 0);
        }
        if rate.1 >= 600 {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        rate.1 += 1;
    }
    if request
        .uri()
        .path()
        .starts_with("/.well-known/oauth-protected-resource")
    {
        return next.run(request).await;
    }
    let raw = if request
        .headers()
        .get_all(header::AUTHORIZATION)
        .iter()
        .count()
        == 1
    {
        request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_once(' '))
            .and_then(|(scheme, value)| scheme.eq_ignore_ascii_case("Bearer").then_some(value))
            .filter(|v| v.len() <= 16384)
            .map(str::to_owned)
    } else {
        None
    };
    let Some(raw) = raw else {
        return state.challenge(StatusCode::UNAUTHORIZED, "invalid_token");
    };
    let identity = if raw.starts_with("prk_") {
        let path = state.config.keys.clone();
        tokio::task::spawn_blocking(move || api_key(&path, &raw))
            .await
            .unwrap_or(Err(Error::Unauthenticated))
    } else {
        tokio::time::timeout(Duration::from_secs(10), async {
            let identity = state.oauth(&raw).await?;
            state.active(&raw).await?;
            Ok(identity)
        })
        .await
        .unwrap_or(Err(Error::Credentials))
    };
    let identity = match identity {
        Ok(identity) => identity,
        Err(Error::Unauthenticated) => {
            return state.challenge(StatusCode::UNAUTHORIZED, "invalid_token");
        }
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    // Bound parsing before the SDK and issue a real HTTP scope challenge, not
    // only an error hidden inside a successful JSON-RPC envelope.
    let mut tools_list = false;
    if request.method() == axum::http::Method::POST {
        let (parts, body) = request.into_parts();
        let bytes = match tokio::time::timeout(
            Duration::from_secs(10),
            axum::body::to_bytes(body, 1024 * 1024),
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            _ => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        };
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            tools_list = v["method"] == "tools/list";
            if v["method"] == "tools/call"
                && let Some(required) = v["params"]["name"].as_str().and_then(scope)
                    && !identity.scopes.iter().any(|s| s == required) {
                        let challenge = format!(
                            "Bearer error=\"insufficient_scope\", scope=\"{required}\", resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\"",
                            state.config.resource.trim_end_matches("/mcp")
                        );
                        let mut response = (StatusCode::FORBIDDEN, axum::Json(json!({"jsonrpc":"2.0","id":v["id"],"result":{"content":[{"type":"text","text":"Additional authorization required"}],"isError":true,"_meta":{"mcp/www_authenticate":[challenge]}}}))).into_response();
                        response
                            .headers_mut()
                            .insert(header::WWW_AUTHENTICATE, challenge.parse().unwrap());
                        response
                            .headers_mut()
                            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
                        return response;
                    }
        }
        request = Request::from_parts(parts, axum::body::Body::from(bytes));
    }
    request.extensions_mut().insert(identity);
    let mut response = match tokio::time::timeout(Duration::from_secs(35), next.run(request)).await
    {
        Ok(r) => r,
        Err(_) => return StatusCode::GATEWAY_TIMEOUT.into_response(),
    };
    // rmcp models preserve standard _meta but intentionally omit nonstandard
    // top-level tool fields. Mirror the OpenAI securitySchemes extension on
    // JSON tools/list responses without forking the transport or SDK.
    if tools_list
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"))
    {
        let (mut parts, body) = response.into_parts();
        let bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let mut bytes = bytes.to_vec();
        if let Ok(mut value) = serde_json::from_slice::<Value>(&bytes)
            && let Some(tools) = value["result"]["tools"].as_array_mut() {
                for tool in tools {
                    if let Some(schemes) = tool["_meta"].get("securitySchemes").cloned() {
                        tool["securitySchemes"] = schemes;
                    }
                }
                bytes = serde_json::to_vec(&value).unwrap();
                parts.headers.remove(header::CONTENT_LENGTH);
            }
        response = Response::from_parts(parts, axum::body::Body::from(bytes));
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
#[derive(Clone)]
struct Handler {
    service: Service,
}
fn scope(name: &str) -> Option<&'static str> {
    match name {
        "get" | "search" => Some("memory.read"),
        "record" => Some("memory.write"),
        "forget" => Some("memory.forget"),
        _ => None,
    }
}
fn valid_arguments(name: &str, args: &serde_json::Map<String, Value>) -> bool {
    let (required, allowed): (&str, &[&str]) = match name {
        "record" => ("content", &["content", "idempotency_key"]),
        "search" => ("query", &["query", "limit"]),
        "get" | "forget" => ("id", &["id"]),
        _ => return false,
    };
    args.get(required).is_some_and(Value::is_string)
        && args.keys().all(|k| allowed.contains(&k.as_str()))
        && args.get("idempotency_key").is_none_or(Value::is_string)
        && args.get("limit").is_none_or(|v| v.as_u64().is_some_and(|n| (1..=100).contains(&n)))
}
impl ServerHandler for Handler {
    fn get_tool(&self, name: &str) -> Option<Tool> {
        crate::transport::mcp::tools(false)["tools"].as_array()?.iter().find(|t| t["name"] == name).and_then(|t| serde_json::from_value(t.clone()).ok())
    }
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, rmcp::ErrorData> {
        let mut tools = crate::transport::mcp::tools(false);
        for t in tools["tools"].as_array_mut().unwrap() {
            let s = scope(t["name"].as_str().unwrap()).unwrap();
            t["securitySchemes"] = json!([{"type":"oauth2","scopes":[s]}]);
            t["_meta"] = json!({"securitySchemes":[{"type":"oauth2","scopes":[s]}]});
        }
        Ok(serde_json::from_value(tools).expect("static tool schemas"))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, rmcp::ErrorData> {
        let required = scope(&request.name)
            .ok_or_else(|| rmcp::ErrorData::invalid_params("unknown tool", None))?;
        let arguments = request.arguments.clone().unwrap_or_default();
        if !valid_arguments(&request.name, &arguments) { return Err(rmcp::ErrorData::invalid_params("invalid tool arguments", None)); }
        let identity = ctx
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|p| p.extensions.get::<Identity>())
            .cloned()
            .ok_or_else(|| rmcp::ErrorData::internal_error("missing request identity", None))?;
        if !identity.scopes.iter().any(|s| s == required) {
            let result: CallToolResult = serde_json::from_value(json!({"content":[{"type":"text","text":"Additional authorization required"}],"isError":true,"_meta":{"mcp/www_authenticate":[format!("Bearer error=\"insufficient_scope\", scope=\"{required}\"")]}})).unwrap();
            return Ok(result.into());
        }
        let service = self.service.clone();
        let reply = tokio::task::spawn_blocking(move || {
            service.dispatch(
                &identity.principal,
                &request.name,
                Value::Object(arguments),
            )
        })
        .await
        .map_err(|_| rmcp::ErrorData::internal_error("service unavailable", None))?;
        let result: CallToolResult = serde_json::from_value(json!({"content":[{"type":"text","text":serde_json::to_string(&reply).unwrap()}],"structuredContent":reply,"isError":!reply.ok})).unwrap();
        Ok(result.into())
    }
}
pub async fn router(service: Service, config: Config) -> Result<Router> {
    config.validate()?;
    let http = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| Error::Credentials)?;
    let mut state = Remote {
        config,
        http,
        cache: tokio::sync::Mutex::new(Cache {
            keys: JwkSet { keys: vec![] },
            fetched: Instant::now(),
        }),
        slots: Arc::new(tokio::sync::Semaphore::new(32)),
        rate: std::sync::Mutex::new((Instant::now(), 0)),
    };
    let discovery: Value = state.fetch(&state.config.discovery_url).await?;
    if discovery["issuer"] != state.config.issuer
        || discovery["jwks_uri"] != state.config.jwks_uri
        || discovery["introspection_endpoint"] != state.config.introspection.endpoint
        || !discovery["code_challenge_methods_supported"]
            .as_array()
            .is_some_and(|a| a.contains(&json!("S256")))
    {
        return Err(Error::Credentials);
    }
    for field in ["authorization_endpoint", "token_endpoint"] {
        https(discovery[field].as_str().ok_or(Error::Credentials)?)?;
    }
    state.cache.get_mut().keys = state.fetch(&state.config.jwks_uri).await?;
    Ok(build_router(service, state))
}
fn build_router(service: Service, state: Remote) -> Router {
    let metadata = json!({"resource":state.config.resource,"authorization_servers":[state.config.issuer],"scopes_supported":SCOPES,"bearer_methods_supported":["header"]});
    let host = &state.config.resource[8..state.config.resource.len() - 4];
    let origin = reqwest::Url::parse(&state.config.resource)
        .unwrap()
        .origin()
        .ascii_serialization();
    let transport = StreamableHttpService::new(
        move || {
            Ok(Handler {
                service: service.clone(),
            })
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_allowed_hosts([host.to_owned()])
            .with_allowed_origins([format!("{origin}:443")])
            .enforce_origin_validation()
            .with_max_request_body_bytes(1024 * 1024),
    );
    let md = move || {
        let metadata = metadata.clone();
        async move { ([(header::CACHE_CONTROL, "no-store")], axum::Json(metadata)) }
    };
    Router::new()
        .route_service("/mcp", transport)
        .route("/.well-known/oauth-protected-resource", get(md.clone()))
        .route("/.well-known/oauth-protected-resource/mcp", get(md))
        .layer(middleware::from_fn_with_state(Arc::new(state), guard))
}
