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
    pub fn authenticate(&self, bearer: &str) -> Result<Principal> {
        self.credentials.authenticate(bearer)
    }
    pub fn call(&self, bearer: &str, operation: &str, arguments: Value) -> Reply {
        match self.authenticate(bearer) {
            Ok(p) => self.dispatch(&p, operation, arguments),
            Err(e) => Reply::error(e),
        }
    }
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
                            .list_claims_with_options(p, r.include_retracted)?
                            .into_iter()
                            .find(|c| c.id == r.id)
                            .ok_or(Error::NotFound)?,
                    )
                }
                "claim_list" => {
                    to_value(self.app.page_claims(p, parse::<ListSources>(arguments)?)?)
                }
                "claim_retract" | "claim_forget" | "claim_confirm" | "claim_unconfirm" => {
                    let r: Transition = parse(arguments)?;
                    self.app.set_claim_status(
                        p,
                        &r.id,
                        operation.strip_prefix("claim_").expect("prefix"),
                    )?;
                    Ok(json!({}))
                }
                "list" => to_value(self.app.list_sources(p, parse::<ListSources>(arguments)?)?),
                "retract" | "forget" => {
                    let r: Transition = parse(arguments)?;
                    if r.reason.as_ref().is_some_and(|s| s.len() > 4096) {
                        return Err(invalid("reason", "too long"));
                    }
                    self.app.set_source_lifecycle(
                        p,
                        r.id.parse()?,
                        if operation == "retract" {
                            LifecycleEvent::Retract
                        } else {
                            LifecycleEvent::Forget
                        },
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
            _ => 500,
        }
    }
}
