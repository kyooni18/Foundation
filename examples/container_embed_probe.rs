//! CPU probe using the read-only container model mount. No network access.
use prime_radiant::embedding::{EmbeddingProvider, local::*};
use std::time::Instant;
fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let provider = LocalProvider::new(LocalConfig {
        model_dir: "/model".into(),
        device: DeviceChoice::Cpu,
        ..Default::default()
    });
    for text in [
        "query: 서버의 데이터베이스는 무엇인가요?",
        "query: What database does the server use?",
    ] {
        let start = Instant::now();
        let result = provider.embed_raw(&[text.into()]);
        println!(
            "elapsed_seconds={:.3} dimensions={:?} error={:?}",
            start.elapsed().as_secs_f64(),
            result.as_ref().map(|v| v[0].len()).ok(),
            result.err()
        );
    }
    println!("{}", std::fs::read_to_string("/proc/self/status").unwrap());
}
