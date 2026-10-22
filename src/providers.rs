//! Provider failures deliberately exclude response bodies, URLs, and credentials.
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, time::Duration};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub model: String,
    pub timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderError {
    Unavailable,
    Timeout,
    InvalidOutput,
    LanguageRestriction,
    Quota,
    InvalidRequest,
    RateLimited,
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
    pub fn from_credential_file(path: impl AsRef<std::path::Path>) -> Result<Self, ProviderError> {
        let bytes = crate::authentication::secure_read(path.as_ref())
            .map_err(|_| ProviderError::Unavailable)?;
        let key = String::from_utf8(bytes)
            .map_err(|_| ProviderError::Unavailable)?
            .trim()
            .to_owned();
        if key.is_empty() {
            return Err(ProviderError::Unavailable);
        }
        Ok(Self {
            client: reqwest::Client::new(),
            key,
        })
    }
    pub fn from_environment() -> Result<Self, ProviderError> {
        if let Some(path) = std::env::var_os("OPENROUTER_API_KEY_FILE") {
            return Self::from_credential_file(path);
        }
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
            if config
                .max_output_tokens
                .is_some_and(|n| !(1..=262144).contains(&n))
            {
                return Err(ProviderError::Unavailable);
            }
            let mut payload = serde_json::json!({"model":config.model,"temperature":0,"response_format":{"type":"json_object"},"messages":[{"role":"system","content":system},{"role":"user","content":input}]});
            if let Some(limit) = config.max_output_tokens {
                payload["max_tokens"] = limit.into();
            }
            let response = self
                .client
                .post("https://openrouter.ai/api/v1/chat/completions")
                .bearer_auth(&self.key)
                .timeout(Duration::from_secs(config.timeout_seconds))
                .json(&payload)
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        ProviderError::Timeout
                    } else {
                        ProviderError::Unavailable
                    }
                })?;
            if response.status() == reqwest::StatusCode::PAYMENT_REQUIRED {
                return Err(ProviderError::Quota);
            }
            if response.status() == reqwest::StatusCode::BAD_REQUEST {
                return Err(ProviderError::InvalidRequest);
            }
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(ProviderError::RateLimited);
            }
            if !response.status().is_success() {
                return Err(ProviderError::Unavailable);
            }
            read_completion(response).await
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

async fn read_completion(mut response: reqwest::Response) -> Result<String, ProviderError> {
    // Bound bytes before JSON decoding; Content-Length is only an early hint.
    const MAX_RESPONSE: usize = 8 * 1024 * 1024;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE as u64)
    {
        return Err(ProviderError::InvalidOutput);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        if e.is_timeout() {
            ProviderError::Timeout
        } else {
            ProviderError::Unavailable
        }
    })? {
        if chunk.len() > MAX_RESPONSE - bytes.len() {
            return Err(ProviderError::InvalidOutput);
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ProviderError::InvalidOutput)?;
    // Even valid-looking JSON is incomplete if the provider reports truncation.
    if body["choices"][0]["finish_reason"] != "stop" {
        return Err(ProviderError::InvalidOutput);
    }
    body["choices"][0]["message"]["content"]
        .as_str()
        .map(String::from)
        .ok_or(ProviderError::InvalidOutput)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn response_decoding_bounds_and_sanitized_errors() {
        let valid =
            r#"{"choices":[{"finish_reason":"stop","message":{"content":"{\"claims\":[]}"}}]}"#;
        let truncated = valid.replace("\"stop\"", "\"length\"");
        let bodies = [
            (
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    valid.len(),
                    valid
                ),
                Ok("{\"claims\":[]}".to_owned()),
            ),
            (
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    truncated.len(),
                    truncated
                ),
                Err(ProviderError::InvalidOutput),
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n".into(),
                Err(ProviderError::InvalidOutput),
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret".into(),
                Err(ProviderError::InvalidOutput),
            ),
            (
                format!(
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n800001\r\n{}\r\n0\r\n\r\n",
                    "x".repeat(8 * 1024 * 1024 + 1)
                ),
                Err(ProviderError::InvalidOutput),
            ),
        ];
        for (wire, expected) in bodies {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let task = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut req = [0; 4096];
                let _ = socket.read(&mut req).await;
                let _ = socket.write_all(wire.as_bytes()).await;
            });
            let response = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .get(format!("http://{addr}"))
                .timeout(Duration::from_secs(5))
                .send()
                .await
                .unwrap();
            assert_eq!(read_completion(response).await, expected);
            task.await.unwrap();
        }
    }
}
