//! Embedding providers. The user selects a local model (default) or a cloud provider, the same
//! way an LLM provider is selected. Embeddings are derived data: every vector is keyed by an
//! immutable model identity so vectors from different models, revisions, prefixes or
//! dimensionalities are never mixed (DATA_CONTRACTS §11).
//!
//! No provider ever falls back to another: a missing local model defers work, it does not send
//! private text to a remote service.
pub mod local;
pub mod openrouter;

use crate::store::integrity::sha256;
use std::{path::PathBuf, sync::Arc};

/// Stable identity of an embedding space. `key` changes whenever any ingredient that would
/// change a vector changes, so derived vectors for other identities stay valid and isolated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingIdentity {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub revision: String,
    pub dimensions: usize,
    pub query_prefix: String,
    pub document_prefix: String,
    pub max_tokens: usize,
    /// True when text never leaves this machine.
    pub local: bool,
}
impl EmbeddingIdentity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: &str,
        model: &str,
        revision: &str,
        dimensions: usize,
        query_prefix: &str,
        document_prefix: &str,
        max_tokens: usize,
        local: bool,
    ) -> Self {
        let digest = sha256(
            format!("{provider}\0{model}\0{revision}\0{dimensions}\0{query_prefix}\0{document_prefix}\0{max_tokens}")
                .as_bytes(),
        );
        Self {
            key: format!("{provider}:{model}:d{dimensions}:{}", &digest[..12]),
            provider: provider.into(),
            model: model.into(),
            revision: revision.into(),
            dimensions,
            query_prefix: query_prefix.into(),
            document_prefix: document_prefix.into(),
            max_tokens,
            local,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Query,
    Document,
}

/// Failure categories are fixed codes: provider bodies, URLs and text are never retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedError {
    /// Model files or credentials are missing; work should be deferred, not retried hot.
    Unavailable,
    Timeout,
    RateLimited,
    Quota,
    InvalidRequest,
    InvalidOutput,
    Runtime,
}
impl EmbedError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "provider_unavailable",
            Self::Timeout => "timeout",
            Self::RateLimited => "rate_limit",
            Self::Quota => "quota",
            Self::InvalidRequest => "request",
            Self::InvalidOutput => "output",
            Self::Runtime => "runtime",
        }
    }
}

pub trait EmbeddingProvider: Send + Sync {
    fn identity(&self) -> &EmbeddingIdentity;
    /// Embed already-prefixed texts. Output has the same order and length as `texts`; every
    /// vector has `identity().dimensions` entries and unit L2 norm. Blocking: callers run it
    /// outside any database transaction and off the async runtime.
    fn embed_raw(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;
    /// Number of model forward passes / requests made so far (for evaluation accounting).
    fn calls(&self) -> u64 {
        0
    }
}

/// Apply the identity's prefix for the role, then embed.
pub fn embed(
    provider: &dyn EmbeddingProvider,
    role: Role,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, EmbedError> {
    let id = provider.identity();
    let prefix = match role {
        Role::Query => &id.query_prefix,
        Role::Document => &id.document_prefix,
    };
    let prefixed: Vec<String> = texts.iter().map(|t| format!("{prefix}{t}")).collect();
    let vectors = provider.embed_raw(&prefixed)?;
    if vectors.len() != texts.len() || vectors.iter().any(|v| v.len() != id.dimensions) {
        return Err(EmbedError::InvalidOutput);
    }
    Ok(vectors)
}

/// A caller-supplied provider (embedding applications and tests).
#[derive(Clone)]
pub struct Injected(pub Arc<dyn EmbeddingProvider>);
impl std::fmt::Debug for Injected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Injected({})", self.0.identity().key)
    }
}
/// User-selectable provider configuration.
#[derive(Debug, Clone)]
pub enum EmbeddingConfig {
    /// A provider constructed by the embedding application.
    Custom(Injected),
    /// No semantic retrieval: lexical, structured and graph retrieval only.
    Off,
    Local(local::LocalConfig),
    OpenRouter(openrouter::CloudConfig),
}
impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self::Local(local::LocalConfig::default())
    }
}
impl EmbeddingConfig {
    /// Identity without loading anything (model files are only read on first use).
    pub fn identity(&self) -> Option<EmbeddingIdentity> {
        match self {
            Self::Off => None,
            Self::Custom(i) => Some(i.0.identity().clone()),
            Self::Local(c) => Some(c.identity()),
            Self::OpenRouter(c) => Some(c.identity()),
        }
    }
    /// Construct the provider lazily-loading weights on first embed call.
    pub fn provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        match self {
            Self::Off => None,
            Self::Custom(i) => Some(i.0.clone()),
            Self::Local(c) => Some(Arc::new(local::LocalProvider::new(c.clone()))),
            Self::OpenRouter(c) => openrouter::CloudProvider::from_environment(c.clone())
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn EmbeddingProvider>),
        }
    }
    pub fn parse(name: &str, model: Option<String>, dir: Option<PathBuf>) -> Option<Self> {
        match name {
            "off" | "none" => Some(Self::Off),
            "local" => {
                let mut c = local::LocalConfig::default();
                if let Some(dir) = dir {
                    c.model_dir = dir;
                }
                Some(Self::Local(c))
            }
            "openrouter" => Some(Self::OpenRouter(openrouter::CloudConfig::new(
                model.unwrap_or_else(|| openrouter::DEFAULT_MODEL.into()),
            ))),
            _ => None,
        }
    }
}

/// Cosine similarity of two unit vectors.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}
