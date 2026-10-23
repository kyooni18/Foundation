//! Single bounded diagnostic request through the same production spending guard.
use prime_radiant::providers::{ModelConfig, OpenRouter};
use serde::Deserialize;
use std::io::Read;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    system: String,
    input: String,
    config: ModelConfig,
}
#[tokio::main]
async fn main() {
    let result = run().await;
    match result {
        Ok(value) => println!("{value}"),
        Err(code) => {
            eprintln!("{code}");
            std::process::exit(1);
        }
    }
}
async fn run() -> Result<serde_json::Value, &'static str> {
    let mut input = String::new();
    std::io::stdin()
        .take(8 * 1024 * 1024 + 1)
        .read_to_string(&mut input)
        .map_err(|_| "request")?;
    if input.len() > 8 * 1024 * 1024 {
        return Err("request");
    }
    let request: Request = serde_json::from_str(&input).map_err(|_| "request")?;
    let provider = OpenRouter::from_environment().map_err(|_| "credentials")?;
    let (text, usage) = provider
        .complete_with_usage(&request.system, &request.input, &request.config)
        .await
        .map_err(|_| "provider_or_budget")?;
    let usage = usage.ok_or("usage")?;
    Ok(
        serde_json::json!({"raw":text,"finish_reason":"stop","usage":{"prompt_tokens":usage.prompt_tokens,"completion_tokens":usage.completion_tokens}}),
    )
}
