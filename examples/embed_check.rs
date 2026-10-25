//! Numerical parity and speed check of the local embedding runtime against reference vectors
//! produced by the official PyTorch/transformers implementation (f32, CPU).
//! Usage: embed_check [metal|cpu] [f32|f16|bf16] [batch_tokens]
use prime_radiant::embedding::{EmbeddingProvider, local::*};
use serde_json::Value;
use std::time::Instant;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut config = LocalConfig::default();
    config.device = match args.get(1).map(String::as_str) {
        Some("cpu") => DeviceChoice::Cpu,
        Some("metal") => DeviceChoice::Metal,
        _ => DeviceChoice::Auto,
    };
    config.precision = match args.get(2).map(String::as_str) {
        Some("f32") => Precision::F32,
        Some("bf16") => Precision::Bf16,
        _ => Precision::F16,
    };
    if let Some(n) = args.get(3).and_then(|s| s.parse().ok()) {
        config.batch_tokens = n;
    }
    let fixture: Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/nemotron_parity.json").unwrap(),
    )
    .unwrap();
    let texts: Vec<String> = fixture["texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().into())
        .collect();
    let provider = LocalProvider::new(config.clone());
    let t = Instant::now();
    let vectors = provider.embed_raw(&texts).expect("embedding failed");
    println!(
        "first call (includes load + digest verification): {:?}",
        t.elapsed()
    );
    let mut worst = 1.0f32;
    for (i, (v, r)) in vectors
        .iter()
        .zip(fixture["vectors"].as_array().unwrap())
        .enumerate()
    {
        let mut reference: Vec<f32> = r
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap() as f32)
            .collect();
        reference.truncate(config.dimensions);
        prime_radiant::embedding::normalize(&mut reference);
        let cos = prime_radiant::embedding::dot(v, &reference);
        worst = worst.min(cos);
        println!("text {i}: cosine to reference = {cos:.6}");
    }
    println!("worst cosine {worst:.6}");
    let batch: Vec<String> = (0..256).map(|i| format!("passage: 이것은 {i}번째 테스트 문장입니다. This is test sentence number {i} about memory retrieval.")).collect();
    let t = Instant::now();
    provider.embed_raw(&batch).unwrap();
    let e = t.elapsed();
    println!(
        "256 short texts: {:?} ({:.1} texts/s)",
        e,
        256.0 / e.as_secs_f64()
    );
}
