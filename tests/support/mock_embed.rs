//! Deterministic hashed character-bigram embedding for queue/plumbing tests. It is not a quality
//! model: quality is measured with the real local model in the evaluation.
#![allow(dead_code)]
use prime_radiant::embedding::{EmbedError, EmbeddingIdentity, EmbeddingProvider};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct MockEmbedder {
    id: EmbeddingIdentity,
    calls: AtomicU64,
    texts: AtomicU64,
    /// When set, every call fails with this error (provider outage).
    pub fail: std::sync::Mutex<Option<EmbedError>>,
    pub delay: std::sync::Mutex<std::time::Duration>,
}
impl MockEmbedder {
    pub fn new(revision: &str) -> Self {
        Self {
            id: EmbeddingIdentity::new(
                "mock",
                "hashed-bigram",
                revision,
                64,
                "query: ",
                "passage: ",
                512,
                true,
            ),
            calls: AtomicU64::new(0),
            texts: AtomicU64::new(0),
            fail: Default::default(),
            delay: Default::default(),
        }
    }
    pub fn calls_made(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
    pub fn texts_embedded(&self) -> u64 {
        self.texts.load(Ordering::SeqCst)
    }
}
impl EmbeddingProvider for MockEmbedder {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.id
    }
    fn embed_raw(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let delay = *self.delay.lock().unwrap();
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        if let Some(e) = *self.fail.lock().unwrap() {
            return Err(e);
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.texts.fetch_add(texts.len() as u64, Ordering::SeqCst);
        Ok(texts
            .iter()
            .map(|t| {
                let body = t
                    .strip_prefix("passage: ")
                    .or_else(|| t.strip_prefix("query: "))
                    .unwrap_or(t);
                let chars: Vec<char> = body
                    .to_lowercase()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                let mut v = vec![0f32; 64];
                for w in chars.windows(2) {
                    let h = (w[0] as u64)
                        .wrapping_mul(1_000_003)
                        .wrapping_add(w[1] as u64)
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
                    v[(h >> 58) as usize] += 1.0;
                }
                if chars.len() == 1 {
                    v[(chars[0] as usize) % 64] = 1.0;
                }
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                v.iter().map(|x| x / n).collect()
            })
            .collect())
    }
    fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}
