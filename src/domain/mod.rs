//! Values and pure validation. No SQL or filesystem operations.
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid {field}: {reason}")]
    InvalidInput { field: &'static str, reason: String },
    #[error("idempotency conflict: {source_id}")]
    IdempotencyConflict { source_id: SourceId },
    #[error("authentication failed")]
    Unauthenticated,
    #[error("credential file unavailable or insecure")]
    Credentials,
    #[error("invalid cursor")]
    InvalidCursor,
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("integrity failure: {0}")]
    IntegrityFailure(String),
    #[error("schema too new: database {db}, binary {binary}")]
    SchemaTooNew { db: u32, binary: u32 },
    #[error("unsafe database location: {0}")]
    UnsafeLocation(std::path::PathBuf),
    #[error(transparent)]
    Storage(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
pub fn invalid(field: &'static str, reason: impl Into<String>) -> Error {
    Error::InvalidInput {
        field,
        reason: reason.into(),
    }
}
macro_rules! text_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $value)] $variant),+ }
        impl $name { pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $value),+ } } }
        impl fmt::Display for $name { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) } }
        impl FromStr for $name {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self> { match s { $($value => Ok(Self::$variant)),+, _ => Err(invalid(stringify!($name), s)) } }
        }
    }
}
text_enum!(PrincipalKind { User => "user", Agent => "agent", Import => "import" });
text_enum!(MediaType { TextPlain => "text/plain", TextMarkdown => "text/markdown" });
text_enum!(RetentionRequest { None => "none", UserRequested => "user_requested" });
text_enum!(LifecycleEvent { Retract => "retract", Forget => "forget", Restore => "restore" });
text_enum!(Lifecycle { Live => "live", Retracted => "retracted", Forgotten => "forgotten" });
impl LifecycleEvent {
    pub fn lifecycle(self) -> Lifecycle {
        match self {
            Self::Retract => Lifecycle::Retracted,
            Self::Forget => Lifecycle::Forgotten,
            Self::Restore => Lifecycle::Live,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Principal {
    pub(crate) kind: PrincipalKind,
    pub(crate) label: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) credential: Option<[u8; 32]>,
}
impl Principal {
    /// Explicit local developer CLI/test entry point; not a transport authentication API.
    pub fn for_local_cli(
        kind: PrincipalKind,
        label: Option<String>,
        session_id: Option<String>,
    ) -> Self {
        Self {
            kind,
            label,
            session_id,
            credential: None,
        }
    }
    pub fn kind(&self) -> PrincipalKind {
        self.kind
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct SourceId(String);
impl SourceId {
    pub(crate) fn new() -> Self {
        Self(ulid::Ulid::new().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for SourceId {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        ulid::Ulid::from_string(s)
            .map(|id| Self(id.to_string()))
            .map_err(|e| invalid("source_id", e.to_string()))
    }
}
impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Timestamp(String);
impl Timestamp {
    pub(crate) fn now() -> Self {
        Self::from_datetime(OffsetDateTime::now_utc())
    }
    fn from_datetime(t: OffsetDateTime) -> Self {
        let t = t.to_offset(UtcOffset::UTC);
        Self(format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute(),
            t.second(),
            t.millisecond()
        ))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for Timestamp {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let t =
            OffsetDateTime::parse(s, &Rfc3339).map_err(|e| invalid("timestamp", e.to_string()))?;
        let utc = t.to_offset(UtcOffset::UTC);
        if !(0..=9999).contains(&utc.year()) {
            return Err(invalid("timestamp", "UTC year out of range"));
        }
        Ok(Self::from_datetime(utc))
    }
}
pub const MAX_CONTENT_BYTES: usize = 1024 * 1024;
#[derive(Debug, Clone)]
pub struct RecordSource {
    pub content: String,
    pub media_type: MediaType,
    pub authored_at: Option<Timestamp>,
    pub observed_at: Option<Timestamp>,
    pub artifact_ref: Option<String>,
    pub retention_request: RetentionRequest,
    pub idempotency_key: Option<String>,
}
impl RecordSource {
    pub fn plain(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            media_type: MediaType::TextPlain,
            authored_at: None,
            observed_at: None,
            artifact_ref: None,
            retention_request: RetentionRequest::None,
            idempotency_key: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.content.is_empty() || self.content.len() > MAX_CONTENT_BYTES {
            return Err(invalid("content", "must be 1..=1048576 bytes"));
        }
        if let Some(key) = &self.idempotency_key
            && (key.is_empty()
                || key.len() > 200
                || !key.bytes().all(|b| (0x20..=0x7e).contains(&b)))
        {
            return Err(invalid(
                "idempotency_key",
                "must be 1..=200 printable ASCII bytes",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct Recorded {
    pub source_id: SourceId,
    pub created: bool,
}
#[derive(Debug, Serialize)]
pub struct SourceView {
    pub id: SourceId,
    pub recorder: PrincipalKind,
    pub recorder_label: Option<String>,
    pub session_id: Option<String>,
    pub content: String,
    pub content_sha256: String,
    pub media_type: MediaType,
    pub authored_at: Option<Timestamp>,
    pub observed_at: Option<Timestamp>,
    pub captured_at: Timestamp,
    pub artifact_ref: Option<String>,
    pub retention_request: RetentionRequest,
    pub idempotency_key: Option<String>,
    pub request_digest: String,
    pub lifecycle: Lifecycle,
}
