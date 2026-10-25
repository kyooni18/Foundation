//! Opt-in cloud embeddings through OpenRouter. Selecting this provider sends Claim text and
//! search queries to OpenRouter and the provider behind it; it is never an automatic fallback.
//! Paid models require the same persistent spending ledger as extraction calls (fail closed);
//! free models need no ledger but are still sent to a third party.
use super::{EmbedError, EmbeddingIdentity, EmbeddingProvider, normalize};
use crate::budget::{Budget, nanos};
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// The same model as the local default, served by OpenRouter. Free endpoints may log prompts.
pub const DEFAULT_MODEL: &str = "nvidia/nemotron-3-embed-1b:free";
const BASE: &str = "https://openrouter.ai/api/v1";
const MAX_BATCH: usize = 64;
const MAX_REQUEST_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone)]
pub struct CloudConfig {
    pub model: String,
    pub dimensions: usize,
    pub query_prefix: String,
    pub document_prefix: String,
    pub max_tokens: usize,
    pub base_url: String,
    pub timeout_seconds: u64,
}
impl CloudConfig {
    pub fn new(model: String) -> Self {
        // The default model is the Nemotron family, which requires its prefixes; other models
        // are used without prefixes unless configured.
        let nemotron = model.contains("nemotron") && model.contains("embed");
        Self {
            model,
            dimensions: 1024,
            query_prefix: if nemotron { "query: " } else { "" }.into(),
            document_prefix: if nemotron { "passage: " } else { "" }.into(),
            max_tokens: 512,
            base_url: BASE.into(),
            timeout_seconds: 60,
        }
    }
    pub fn identity(&self) -> EmbeddingIdentity {
        EmbeddingIdentity::new(
            "openrouter",
            &self.model,
            "remote",
            self.dimensions,
            &self.query_prefix,
            &self.document_prefix,
            self.max_tokens,
            false,
        )
    }
}

pub struct CloudProvider {
    config: CloudConfig,
    identity: EmbeddingIdentity,
    key: String,
    ledger: Option<std::path::PathBuf>,
    http: reqwest::blocking::Client,
    calls: AtomicU64,
}
impl CloudProvider {
    pub fn from_environment(config: CloudConfig) -> Result<Self, EmbedError> {
        let key = if let Some(path) = std::env::var_os("OPENROUTER_API_KEY_FILE") {
            String::from_utf8(
                crate::authentication::secure_read(path.as_ref())
                    .map_err(|_| EmbedError::Unavailable)?,
            )
            .map_err(|_| EmbedError::Unavailable)?
        } else {
            std::env::var("OPENROUTER_API_KEY").map_err(|_| EmbedError::Unavailable)?
        };
        Self::new(
            config,
            key.trim().to_owned(),
            std::env::var_os("PRIMERADIANT_OPENROUTER_LEDGER").map(Into::into),
        )
    }
    pub fn new(
        config: CloudConfig,
        key: String,
        ledger: Option<std::path::PathBuf>,
    ) -> Result<Self, EmbedError> {
        if key.is_empty()
            || !config
                .model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-._:".contains(&b))
        {
            return Err(EmbedError::Unavailable);
        }
        let http = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .map_err(|_| EmbedError::Unavailable)?;
        Ok(Self {
            identity: config.identity(),
            config,
            key,
            ledger,
            http,
            calls: AtomicU64::new(0),
        })
    }
    fn free(&self) -> bool {
        self.config.model.ends_with(":free")
    }
    /// Per-token input price from the public embedding catalogue; unknown pricing fails closed.
    fn price(&self) -> Result<f64, EmbedError> {
        let response = self
            .http
            .get(format!("{}/embeddings/models", self.config.base_url))
            .send()
            .map_err(|_| EmbedError::Unavailable)?;
        if !response.status().is_success() {
            return Err(EmbedError::Unavailable);
        }
        let body: Value = response.json().map_err(|_| EmbedError::InvalidOutput)?;
        body["data"]
            .as_array()
            .and_then(|models| models.iter().find(|m| m["id"] == self.config.model))
            .and_then(|m| m["pricing"]["prompt"].as_str())
            .and_then(|p| p.parse::<f64>().ok())
            .filter(|p| p.is_finite() && *p >= 0.0)
            .ok_or(EmbedError::Quota)
    }
    fn request(&self, batch: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let bytes: usize = batch.iter().map(String::len).sum();
        if bytes > MAX_REQUEST_BYTES {
            return Err(EmbedError::InvalidRequest);
        }
        let reservation = if self.free() {
            None
        } else {
            let budget = Budget::open(self.ledger.as_deref().ok_or(EmbedError::Quota)?)
                .map_err(|_| EmbedError::Quota)?;
            // Bytes bound the token count from above for every tokenizer; the hold is committed
            // before the request is sent, and is retained if the outcome is uncertain.
            let estimate = nanos(self.price()? * bytes as f64).map_err(|_| EmbedError::Quota)?;
            let id = budget
                .reserve(&self.config.model, estimate)
                .map_err(|_| EmbedError::Quota)?;
            Some((budget, id))
        };
        let response = self
            .http
            .post(format!("{}/embeddings", self.config.base_url))
            .bearer_auth(&self.key)
            .json(&json!({"model": self.config.model, "input": batch, "encoding_format": "float"}))
            .send()
            .map_err(|e| {
                if e.is_timeout() {
                    EmbedError::Timeout
                } else {
                    EmbedError::Unavailable
                }
            })?;
        self.calls.fetch_add(1, Ordering::Relaxed);
        match response.status().as_u16() {
            200 => (),
            402 => return Err(EmbedError::Quota),
            429 => return Err(EmbedError::RateLimited),
            400 | 413 | 422 => return Err(EmbedError::InvalidRequest),
            _ => return Err(EmbedError::Unavailable),
        }
        let body: Value = response.json().map_err(|_| EmbedError::InvalidOutput)?;
        if let Some((budget, id)) = reservation {
            let price = self.price().unwrap_or(0.0);
            let tokens = body["usage"]["prompt_tokens"].as_i64();
            let cost = body["usage"]["cost"]
                .as_f64()
                .or_else(|| tokens.map(|t| t as f64 * price))
                .ok_or(EmbedError::Quota)?;
            let provider_id = body["id"]
                .as_str()
                .map_or_else(|| format!("embed-{id}"), str::to_owned);
            budget
                .settle_amount(
                    &id,
                    &provider_id,
                    nanos(cost).map_err(|_| EmbedError::Quota)?,
                    tokens.unwrap_or(0),
                )
                .map_err(|_| EmbedError::Quota)?;
        }
        let data = body["data"].as_array().ok_or(EmbedError::InvalidOutput)?;
        if data.len() != batch.len() {
            return Err(EmbedError::InvalidOutput);
        }
        let mut out: Vec<Option<Vec<f32>>> = vec![None; batch.len()];
        for (position, item) in data.iter().enumerate() {
            let index = item["index"].as_u64().map_or(position, |i| i as usize);
            let mut vector: Vec<f32> = item["embedding"]
                .as_array()
                .ok_or(EmbedError::InvalidOutput)?
                .iter()
                .map(|x| x.as_f64().map(|f| f as f32))
                .collect::<Option<_>>()
                .ok_or(EmbedError::InvalidOutput)?;
            if vector.len() < self.config.dimensions || index >= out.len() {
                return Err(EmbedError::InvalidOutput);
            }
            normalize(&mut vector);
            vector.truncate(self.config.dimensions);
            normalize(&mut vector);
            out[index] = Some(vector);
        }
        out.into_iter()
            .map(|v| v.ok_or(EmbedError::InvalidOutput))
            .collect()
    }
}
impl EmbeddingProvider for CloudProvider {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.identity
    }
    fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }
    fn embed_raw(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(MAX_BATCH) {
            out.extend(self.request(batch)?);
        }
        Ok(out)
    }
}
