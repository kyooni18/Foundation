//! Provider failures deliberately exclude response bodies, URLs, and credentials.
use crate::budget::{Budget, nanos};
use serde::{Deserialize, Serialize};
pub const DEFAULT_GENERAL_MODEL: &str = "deepseek/deepseek-v4.1-flash-1017";
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
    base: String,
    ledger: Option<std::path::PathBuf>,
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
            base: "https://openrouter.ai/api/v1".into(),
            ledger: std::env::var_os("PRIMERADIANT_OPENROUTER_LEDGER").map(Into::into),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| ProviderError::Unavailable)?,
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
            base: "https://openrouter.ai/api/v1".into(),
            ledger: std::env::var_os("PRIMERADIANT_OPENROUTER_LEDGER").map(Into::into),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| ProviderError::Unavailable)?,
            key,
        })
    }
}
/// Provider-reported token counts, for optional operational budget accounting only.
/// These never influence Claim semantics, authorship or authorization.
#[derive(Debug, Clone, Copy)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}
impl OpenRouter {
    pub async fn complete_with_usage(
        &self,
        system: &str,
        input: &str,
        config: &ModelConfig,
    ) -> Result<(String, Option<TokenUsage>), ProviderError> {
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
        let output_limit = config.max_output_tokens.ok_or(ProviderError::Quota)?;
        let budget = Budget::open(self.ledger.as_deref().ok_or(ProviderError::Quota)?)?;
        // A provider-enforced, non-resetting cap is an independent backstop for price
        // changes, tokenization differences, and requests with uncertain outcomes.
        let key = self
            .client
            .get(format!("{}/key", self.base))
            .bearer_auth(&self.key)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| ProviderError::Unavailable)?;
        if !key.status().is_success() {
            return Err(ProviderError::Quota);
        }
        let key = read_json(key).await?;
        validate_key_cap(&key, budget.limit()?)?;
        if !config
            .model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-._:".contains(&b))
        {
            return Err(ProviderError::InvalidRequest);
        }
        let catalog = self
            .client
            .get(format!("{}/models/{}/endpoints", self.base, config.model))
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| ProviderError::Unavailable)?;
        if !catalog.status().is_success() {
            return Err(ProviderError::Unavailable);
        }
        let catalog = read_json(catalog).await?;
        let quote = Quote::select(&catalog)?;
        let estimate = quote.estimate(system.len(), input.len(), output_limit)?;
        if nanos(
            key["data"]["limit_remaining"]
                .as_f64()
                .ok_or(ProviderError::Quota)?,
        )? < estimate
        {
            return Err(ProviderError::Quota);
        }
        let payload = serde_json::json!({"model":config.model,"temperature":0,
            "max_tokens":output_limit,"response_format":{"type":"json_object"},
            "provider":{"only":[quote.tag],"allow_fallbacks":false,"require_parameters":true,
                "max_price":{"prompt":quote.input*1_000_000.0,"completion":quote.output*1_000_000.0,"request":quote.request}},
            "plugins":[],
            "messages":[{"role":"system","content":system},{"role":"user","content":input}]});
        // Commit before sending. Cancellation, timeouts and malformed responses retain the hold.
        let reservation = budget.reserve(&config.model, estimate)?;
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base))
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
        let body = read_json(response).await?;
        budget.settle(&reservation, &body)?;
        let result = decode_completion(&body)?;
        if config.model.to_lowercase().contains("deepseek") {
            let decoded = serde_json::from_str::<serde_json::Value>(&result.0)
                .map_err(|_| ProviderError::InvalidOutput)?;
            if !result.0.is_ascii() || !decoded.to_string().is_ascii() {
                return Err(ProviderError::LanguageRestriction);
            }
        }
        Ok(result)
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
            self.complete_with_usage(system, input, config)
                .await
                .map(|(text, _)| text)
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

#[cfg(test)]
async fn read_completion(response: reqwest::Response) -> Result<String, ProviderError> {
    read_completion_with_usage(response)
        .await
        .map(|(text, _)| text)
}
#[cfg(test)]
async fn read_completion_with_usage(
    response: reqwest::Response,
) -> Result<(String, Option<TokenUsage>), ProviderError> {
    decode_completion(&read_json(response).await?)
}
async fn read_json(mut response: reqwest::Response) -> Result<serde_json::Value, ProviderError> {
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
    serde_json::from_slice(&bytes).map_err(|_| ProviderError::InvalidOutput)
}
fn decode_completion(
    body: &serde_json::Value,
) -> Result<(String, Option<TokenUsage>), ProviderError> {
    // Even valid-looking JSON is incomplete if the provider reports truncation.
    if body["choices"][0]["finish_reason"] != "stop" {
        return Err(ProviderError::InvalidOutput);
    }
    let usage = body["usage"]["prompt_tokens"]
        .as_u64()
        .zip(body["usage"]["completion_tokens"].as_u64())
        .map(|(prompt_tokens, completion_tokens)| TokenUsage {
            prompt_tokens,
            completion_tokens,
        });
    let text = body["choices"][0]["message"]["content"]
        .as_str()
        .map(String::from)
        .ok_or(ProviderError::InvalidOutput)?;
    Ok((text, usage))
}

fn validate_key_cap(body: &serde_json::Value, ceiling: i64) -> Result<(), ProviderError> {
    let data = &body["data"];
    let limit = nanos(data["limit"].as_f64().ok_or(ProviderError::Quota)?)?;
    if limit == 0 || limit > ceiling || data.get("limit_reset") != Some(&serde_json::Value::Null) {
        return Err(ProviderError::Quota);
    }
    Ok(())
}
struct Quote {
    tag: String,
    input: f64,
    output: f64,
    request: f64,
}
impl Quote {
    fn select(body: &serde_json::Value) -> Result<Self, ProviderError> {
        for endpoint in body["data"]["endpoints"]
            .as_array()
            .ok_or(ProviderError::Quota)?
        {
            let Some(pricing) = endpoint["pricing"].as_object() else {
                continue;
            };
            // Tiered/time-dependent and unfamiliar billing components need explicit support.
            let mut rates = std::collections::BTreeMap::new();
            let mut supported = true;
            for (key, value) in pricing {
                if key == "discount" {
                    continue;
                }
                let Some(rate) = value.as_str().and_then(|s| s.parse::<f64>().ok()) else {
                    supported = false;
                    break;
                };
                if nanos(rate).is_err() {
                    supported = false;
                    break;
                }
                match key.as_str() {
                    "prompt" | "completion" | "request" | "input_cache_read"
                    | "input_cache_write" => {
                        rates.insert(key.as_str(), rate);
                    }
                    _ if rate == 0.0 => {}
                    _ => {
                        supported = false;
                        break;
                    }
                }
            }
            if !supported {
                continue;
            }
            let (Some(input), Some(output), Some(tag)) = (
                rates.get("prompt"),
                rates.get("completion"),
                endpoint["tag"].as_str(),
            ) else {
                continue;
            };
            let input = input
                .max(*rates.get("input_cache_read").unwrap_or(&0.0))
                .max(*rates.get("input_cache_write").unwrap_or(&0.0));
            return Ok(Self {
                tag: tag.into(),
                input,
                output: *output,
                request: *rates.get("request").unwrap_or(&0.0),
            });
        }
        Err(ProviderError::Quota)
    }
    fn estimate(
        &self,
        system_bytes: usize,
        input_bytes: usize,
        output: u32,
    ) -> Result<i64, ProviderError> {
        // One token per UTF-8 byte plus envelope allowance, doubled for safety.
        let input = system_bytes
            .checked_add(input_bytes)
            .and_then(|n| n.checked_add(4096))
            .ok_or(ProviderError::Quota)?;
        nanos(2.0 * (input as f64 * self.input + f64::from(output) * self.output + self.request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn guarded_adapter_prices_reserves_and_records_real_usage_before_validation() {
        use axum::{
            Json, Router,
            routing::{get, post},
        };
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spend.db");
        Budget::create(&path, 4.5, 0.0).unwrap();
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = requests.clone();
        let app=Router::new()
            .route("/key",get(||async{Json(json!({"data":{"limit":4.5,"limit_reset":null,"limit_remaining":4.5}}))}))
            .route("/models/test/model/endpoints",get(||async{Json(json!({"data":{"endpoints":[{"tag":"fixture","pricing":{"prompt":"0.000001","completion":"0.000002"}}]}}))}))
            .route("/chat/completions",post(move |Json(body):Json<serde_json::Value>| {let seen=seen.clone();async move {
                assert_eq!(body["max_tokens"],100);
                assert_eq!(body["provider"]["only"],json!(["fixture"]));
                assert_eq!(body["provider"]["allow_fallbacks"],false);
                assert_eq!(body["provider"]["max_price"]["prompt"],1.0);
                seen.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
                // A truncated response is still billed and must be reconciled.
                Json(json!({"id":"fixture-request","choices":[{"finish_reason":"length","message":{"content":"{}"}}],"usage":{"cost":0.001,"prompt_tokens":4,"completion_tokens":100}}))
            }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut provider = OpenRouter {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            key: "fixture".into(),
            base: format!("http://{addr}"),
            ledger: None,
        };
        let config = ModelConfig {
            model: "test/model".into(),
            timeout_seconds: 5,
            max_output_tokens: Some(100),
        };
        assert_eq!(
            provider.complete("English", "input", &config).await,
            Err(ProviderError::Quota)
        );
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0);
        provider.ledger = Some(path.clone());
        assert_eq!(
            provider.complete("English", "input", &config).await,
            Err(ProviderError::InvalidOutput)
        );
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
        let conn = rusqlite::Connection::open(path).unwrap();
        let record: (i64, String, i64) = conn
            .query_row(
                "SELECT actual,provider_id,cumulative FROM request",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(record, (1_000_000, "fixture-request".into(), 1_000_000));
        server.abort();
    }
    #[tokio::test]
    async fn response_decoding_bounds_and_sanitized_errors() {
        assert!(
            validate_key_cap(
                &serde_json::json!({"data":{"limit":4.5,"limit_reset":null}}),
                4_500_000_000
            )
            .is_ok()
        );
        for data in [
            serde_json::json!({"limit":null,"limit_reset":null}),
            serde_json::json!({"limit":5,"limit_reset":null}),
            serde_json::json!({"limit":4.5,"limit_reset":"daily"}),
        ] {
            assert!(validate_key_cap(&serde_json::json!({"data":data}), 4_500_000_000).is_err());
        }
        let catalog = serde_json::json!({"data":{"endpoints":[{"tag":"test","pricing":{"prompt":"0.000001","completion":"0.000002","request":"0.001","input_cache_write":"0.000003"}}]}});
        let quote = Quote::select(&catalog).unwrap();
        assert_eq!(quote.estimate(10, 20, 100).unwrap(), 27_156_000);
        for pricing in [
            serde_json::json!({"prompt":"-1","completion":"0"}),
            serde_json::json!({"prompt":"NaN","completion":"0"}),
            serde_json::json!({"prompt":"1","completion":"1","new_fee":"1"}),
            serde_json::json!({"prompt":"1","completion":"1","overrides":[]}),
        ] {
            assert!(
                Quote::select(
                    &serde_json::json!({"data":{"endpoints":[{"tag":"test","pricing":pricing}]}})
                )
                .is_err()
            );
        }
        let valid = r#"{"choices":[{"finish_reason":"stop","message":{"content":"{\"claims\":[]}"}}],"usage":{"prompt_tokens":17,"completion_tokens":3}}"#;
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
        for (index, (wire, expected)) in bodies.into_iter().enumerate() {
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
            if index == 0 {
                let (text, usage) = read_completion_with_usage(response).await.unwrap();
                assert_eq!(Ok(text), expected);
                let usage = usage.unwrap();
                assert_eq!((usage.prompt_tokens, usage.completion_tokens), (17, 3));
            } else {
                assert_eq!(read_completion(response).await, expected);
            }
            task.await.unwrap();
        }
    }
}
