//! Provider failures deliberately exclude response bodies, URLs, and credentials.
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, time::Duration};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub model: String,
    pub timeout_seconds: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderError {
    Unavailable,
    Timeout,
    InvalidOutput,
    LanguageRestriction,
}
pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<String, ProviderError>> + Send + 'a>>;
pub trait LlmProvider: Send + Sync {
    fn identity(&self) -> &str;
    fn complete<'a>(
        &'a self,
        system: &'a str,
        input: &'a str,
        config: &'a ModelConfig,
    ) -> ProviderFuture<'a>;
}
pub struct OpenRouter {
    client: reqwest::Client,
    key: String,
}
impl OpenRouter {
    pub fn from_environment() -> Result<Self, ProviderError> {
        let key = std::env::var("OPENROUTER_API_KEY").map_err(|_| ProviderError::Unavailable)?;
        if key.trim().is_empty() {
            return Err(ProviderError::Unavailable);
        }
        Ok(Self {
            client: reqwest::Client::new(),
            key,
        })
    }
}
impl LlmProvider for OpenRouter {
    fn identity(&self) -> &str {
        "openrouter"
    }
    fn complete<'a>(
        &'a self,
        system: &'a str,
        input: &'a str,
        config: &'a ModelConfig,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            // D-24: DeepSeek is restricted to English-only payloads, including review context.
            if config.model.to_lowercase().contains("deepseek")
                && (!system.is_ascii() || !input.is_ascii())
            {
                return Err(ProviderError::LanguageRestriction);
            }
            if config.model.is_empty() || !(1..=600).contains(&config.timeout_seconds) {
                return Err(ProviderError::Unavailable);
            }
            let response=self.client.post("https://openrouter.ai/api/v1/chat/completions").bearer_auth(&self.key)
            .timeout(Duration::from_secs(config.timeout_seconds))
            .json(&serde_json::json!({"model":config.model,"temperature":0,"response_format":{"type":"json_object"},"messages":[{"role":"system","content":system},{"role":"user","content":input}]}))
            .send().await.map_err(|e|if e.is_timeout(){ProviderError::Timeout}else{ProviderError::Unavailable})?;
            if !response.status().is_success() {
                return Err(ProviderError::Unavailable);
            }
            let body: serde_json::Value = response
                .json()
                .await
                .map_err(|_| ProviderError::InvalidOutput)?;
            body["choices"][0]["message"]["content"]
                .as_str()
                .map(String::from)
                .ok_or(ProviderError::InvalidOutput)
        })
    }
}
/// Scripted deterministic provider; records no secrets and makes no network requests.
pub struct FakeProvider(
    pub std::sync::Mutex<std::collections::VecDeque<Result<String, ProviderError>>>,
);
impl LlmProvider for FakeProvider {
    fn identity(&self) -> &str {
        "fake"
    }
    fn complete<'a>(
        &'a self,
        _system: &'a str,
        _input: &'a str,
        _config: &'a ModelConfig,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.0
                .lock()
                .map_err(|_| ProviderError::Unavailable)?
                .pop_front()
                .unwrap_or(Err(ProviderError::Unavailable))
        })
    }
}
