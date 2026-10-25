//! Shared application dispatcher. Both transports use this envelope and core policy.
use crate::{App, ListSources, authentication::Credentials, domain::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
#[derive(Clone)]
pub struct Service {
    app: Arc<App>,
    credentials: Credentials,
}
impl Service {
    pub fn new(app: App, credentials: Credentials) -> Self {
        Self {
            app: Arc::new(app),
            credentials,
        }
    }
    /// Optional worker shares the sole DB owner; transport requests still use core policy.
    pub fn start_extraction_worker(
        &self,
        provider: Arc<dyn crate::providers::LlmProvider>,
        plan: crate::extraction::Plan,
    ) -> tokio::task::JoinHandle<()> {
        let app = self.app.clone();
        tokio::spawn(async move {
            loop {
                let _ = app.work_once(provider.as_ref(), &plan).await;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        })
    }
    /// Deterministic revalidation shares the DB owner; it never calls a model provider.
    pub fn start_revalidation_worker(&self) -> tokio::task::JoinHandle<()> {
        let app = self.app.clone();
        tokio::spawn(async move {
            loop {
                let worker = app.clone();
                let ran = tokio::task::spawn_blocking(move || worker.revalidate_once("serve"))
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .flatten()
                    .is_some();
                if !ran {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        })
    }
    /// Queue-driven derived processing: lexical indexing at normal priority and embedding on a
    /// background-QoS thread so ingestion and interactive search are never starved. Jobs left
    /// by a previous process are picked up through the durable queue (leases expire).
    pub fn start_index_workers(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let owner = format!("serve-{}", ulid::Ulid::new());
        let lexical = {
            let app = self.app.clone();
            let owner = owner.clone();
            tokio::spawn(async move {
                loop {
                    let worker = app.clone();
                    let owner = owner.clone();
                    let busy = tokio::task::spawn_blocking(move || worker.process_lexical_once(&owner))
                        .await
                        .ok()
                        .and_then(|r| r.ok())
                        .is_some_and(|r| r.jobs + r.cancelled > 0);
                    if !busy {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                }
            })
        };
        let embed = {
            let app = self.app.clone();
            tokio::spawn(async move {
                let Some(provider) = app.embedder() else { return };
                loop {
                    let worker = app.clone();
                    let provider = provider.clone();
                    let owner = owner.clone();
                    let busy = tokio::task::spawn_blocking(move || {
                        crate::retrieval::queue::background_qos();
                        worker.process_embeddings_once(provider.as_ref(), &owner)
                    })
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .is_some_and(|r| r.jobs + r.cancelled > 0 && r.failed == 0 && r.deferred == 0);
                    if !busy {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            })
        };
        vec![lexical, embed]
    }
    pub fn authenticate(&self, bearer: &str) -> Result<Principal> {
        self.credentials.authenticate(bearer)
    }
    pub fn call(&self, bearer: &str, operation: &str, arguments: Value) -> Reply {
        match self.authenticate(bearer) {
            Ok(p) => self.dispatch(&p, operation, arguments),
            Err(e) => Reply::error(e),
        }
    }
    /// Header name for client session metadata. It is not authentication or authority: it
    /// only scopes undeclared-dependency tracking (DATA_CONTRACTS §6.1, P-4, D-97).
    pub const SESSION_HEADER: &'static str = "x-primeradiant-session";
    pub(crate) fn dispatch(&self, p: &Principal, operation: &str, arguments: Value) -> Reply {
        let result = (|| -> Result<Value> {
            match operation {
                "authenticate" => {
                    let _: Empty = parse(arguments)?;
                    Ok(json!({}))
                }
                "record" => {
                    let r: Record = parse(arguments)?;
                    let req = RecordSource {
                        content: r.content,
                        media_type: r.media_type.unwrap_or(MediaType::TextPlain),
                        authored_at: r.authored_at.map(|s| s.parse()).transpose()?,
                        observed_at: r.observed_at.map(|s| s.parse()).transpose()?,
                        artifact_ref: r.artifact_ref,
                        retention_request: r.retention_request.unwrap_or(RetentionRequest::None),
                        idempotency_key: r.idempotency_key,
                    };
                    to_value(self.app.record_source(p, req)?)
                }
                "get" => {
                    let r: Get = parse(arguments)?;
                    to_value(self.app.get_source_with_options(
                        p,
                        r.id.parse()?,
                        r.include_retracted,
                    )?)
                }
                "claim_get" => {
                    let r: Get = parse(arguments)?;
                    to_value(
                        self.app
                            .get_claim_with_options(p, &r.id, r.include_retracted)?,
                    )
                }
                "claim_list" => {
                    let r: ClaimList = parse(arguments)?;
                    let page = ListSources {
                        limit: r.limit,
                        include_retracted: r.include_retracted,
                        cursor: r.cursor,
                    };
                    to_value(self.app.page_claims(p, page)?)
                }
                "claims_as_of" => {
                    let r: AsOf = parse(arguments)?;
                    let as_of = r.as_of.map(|s| s.parse()).transpose()?;
                    let page = ListSources {
                        limit: r.limit,
                        include_retracted: false,
                        cursor: r.cursor,
                    };
                    to_value(self.app.claims_as_of(p, as_of, r.present, page)?)
                }
                "claim_dependencies" => {
                    let r: Get = parse(arguments)?;
                    to_value(self.app.claim_dependencies(p, &r.id, r.include_retracted)?)
                }
                "claim_support" => {
                    let r: Id = parse(arguments)?;
                    to_value(self.app.claim_support(p, &r.id)?)
                }
                "claim_relations" => {
                    let r: Id = parse(arguments)?;
                    to_value(self.app.claim_relations(p, &r.id)?)
                }
                "dependency_add" => {
                    let r: DependencyAdd = parse(arguments)?;
                    Ok(json!({"id": self.app.add_dependency(p, &r.child, &r.parent, &r.kind)?}))
                }
                "citation_add" => {
                    let r: CitationAdd = parse(arguments)?;
                    let id = self.app.add_citation(
                        p,
                        &r.child_source.parse()?,
                        &r.parent_source.parse()?,
                    )?;
                    Ok(json!({ "id": id }))
                }
                "entity_create" => {
                    let r: EntityCreate = parse(arguments)?;
                    Ok(json!({"id": self.app.create_entity(p, &r.kind)?}))
                }
                "entity_get" => {
                    let r: Id = parse(arguments)?;
                    to_value(self.app.get_entity(p, &r.id)?)
                }
                "identity_assert" => {
                    let r: Assert = parse(arguments)?;
                    let id = self
                        .app
                        .assert_identity(p, &r.kind, &r.subject, &r.object, r.reason)?;
                    Ok(json!({ "id": id }))
                }
                "relation_add" => {
                    let r: RelationAdd = parse(arguments)?;
                    let id = self.app.assert_relation(
                        p,
                        &r.kind,
                        &r.subject,
                        &r.object,
                        r.reason,
                        r.judgment_id.as_deref(),
                    )?;
                    Ok(json!({ "id": id }))
                }
                "integrity_lifecycle" => {
                    let r: IntegrityLifecycle = parse(arguments)?;
                    self.app.set_integrity_lifecycle(
                        p,
                        r.target.parse()?,
                        &r.id,
                        r.event.parse()?,
                        r.reason,
                    )?;
                    Ok(json!({}))
                }
                "claim_retract" | "claim_forget" | "claim_restore" | "claim_confirm"
                | "claim_unconfirm" => {
                    let r: Transition = parse(arguments)?;
                    self.app.set_claim_status(
                        p,
                        &r.id,
                        operation.strip_prefix("claim_").expect("prefix"),
                    )?;
                    Ok(json!({}))
                }
                "search" => {
                    let r: crate::retrieval::search::SearchRequest = parse(arguments)?;
                    to_value(self.app.search(p, r)?)
                }
                "source_processing" => {
                    let r: Id = parse(arguments)?;
                    to_value(self.app.source_processing(p, &r.id.parse()?)?)
                }
                "queue_status" => {
                    let _: Empty = parse(arguments)?;
                    to_value(self.app.queue_status(p)?)
                }
                "queue_jobs" => {
                    let r: QueueJobs = parse(arguments)?;
                    to_value(self.app.queue_jobs(p, r.state.as_deref(), r.limit)?)
                }
                "queue_retry" => {
                    let r: Id = parse(arguments)?;
                    Ok(json!({"requeued": self.app.retry_job(p, &r.id)?}))
                }
                "list" => to_value(self.app.list_sources(p, parse::<ListSources>(arguments)?)?),
                "retract" | "forget" | "restore" => {
                    let r: Transition = parse(arguments)?;
                    if r.reason.as_ref().is_some_and(|s| s.len() > 4096) {
                        return Err(invalid("reason", "too long"));
                    }
                    self.app.set_source_lifecycle(
                        p,
                        r.id.parse()?,
                        operation.parse::<LifecycleEvent>()?,
                        r.reason,
                    )?;
                    Ok(json!({}))
                }
                _ => Err(invalid("operation", "unsupported operation")),
            }
        })();
        match result {
            Ok(value) => Reply::success(value),
            Err(e) => Reply::error(e),
        }
    }
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| invalid("request", "invalid request"))
}
fn to_value<T: Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(|_| Error::IntegrityFailure("serialization failed".into()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    content: String,
    media_type: Option<MediaType>,
    authored_at: Option<String>,
    observed_at: Option<String>,
    artifact_ref: Option<String>,
    retention_request: Option<RetentionRequest>,
    idempotency_key: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Get {
    id: String,
    #[serde(default)]
    include_retracted: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimList {
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    include_retracted: bool,
    cursor: Option<String>,
}
fn default_limit() -> u32 {
    50
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AsOf {
    as_of: Option<String>,
    present: Option<bool>,
    #[serde(default = "default_limit")]
    limit: u32,
    cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyAdd {
    child: String,
    parent: String,
    kind: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CitationAdd {
    child_source: String,
    parent_source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityCreate {
    kind: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assert {
    kind: String,
    subject: String,
    object: String,
    reason: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationAdd {
    kind: String,
    subject: String,
    object: String,
    reason: Option<String>,
    judgment_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegrityLifecycle {
    target: String,
    id: String,
    event: String,
    reason: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueJobs {
    state: Option<String>,
    #[serde(default = "default_jobs")]
    limit: usize,
}
fn default_jobs() -> usize {
    50
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transition {
    id: String,
    reason: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Reply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}
impl Reply {
    pub fn success(value: Value) -> Self {
        Self {
            ok: true,
            result: Some(value),
            error: None,
        }
    }
    pub fn error(error: Error) -> Self {
        // Never serialize raw input, SQL, paths, source IDs or internal errors.
        let (code, message) = match error {
            Error::Unauthenticated | Error::Credentials => {
                ("unauthenticated", "authentication failed")
            }
            Error::NotFound => ("not_found", "not found"),
            Error::Forbidden => ("forbidden", "forbidden"),
            Error::IdempotencyConflict { .. } => ("idempotency_conflict", "idempotency conflict"),
            Error::InvalidInput { .. } => ("invalid_input", "invalid request"),
            Error::InvalidCursor => ("invalid_cursor", "invalid cursor"),
            Error::QueueFull => ("queue_full", "processing queue is full"),
            _ => ("internal", "operation failed"),
        };
        Self {
            ok: false,
            result: None,
            error: Some(ApiError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
    pub fn status(&self) -> u16 {
        match self.error.as_ref().map(|e| e.code.as_str()) {
            None => 200,
            Some("unauthenticated") => 401,
            Some("not_found") => 404,
            Some("forbidden") => 403,
            Some("idempotency_conflict") => 409,
            Some("invalid_input" | "invalid_cursor") => 400,
            Some("queue_full") => 503,
            _ => 500,
        }
    }
}
