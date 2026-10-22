//! Manual network evaluation; never run as an offline test.
use clap::Parser;
use prime_radiant::{
    App,
    domain::*,
    extraction::{Plan, REVIEW_STAGES, extraction_message, hash, validate},
    providers::{LlmProvider, ModelConfig, OpenRouter},
};
use serde_json::{Value, json};
use std::{io::Write, path::PathBuf};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    corpus: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    model: String,
    #[arg(long)]
    reviewer: String,
    #[arg(long)]
    prompt_version: Option<String>,
    #[arg(long, default_value = "dev")]
    split: String,
    #[arg(
        long,
        default_value = "single,r2-frame,decompose,english-intermediate,atomicity-review,fidelity-review,audited"
    )]
    variants: String,
    #[arg(long, default_value_t = 3)]
    runs: usize,
}
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let provider = OpenRouter::from_environment()
        .map_err(|_| "provider credentials unavailable; no evaluation performed")?;
    let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(args.corpus)?)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(args.output)?;
    let tmp = tempfile::tempdir()?;
    let app = App::open(tmp.path().join("eval.db"))?;
    let user = Principal::for_local_cli(PrincipalKind::User, None, None);
    for case in cases.iter().filter(|c| c["split"] == args.split) {
        let mut request = RecordSource::plain(case["source"].as_str().ok_or("case source absent")?);
        request.authored_at = case["authored_at"].as_str().map(str::parse).transpose()?;
        let id = app.record_source(&user, request)?.source_id;
        let source = app.get_source(&user, id)?;
        for variant in args.variants.split(',') {
            let plan = Plan {
                prompt_version: args.prompt_version.clone(),
                provider: "openrouter".into(),
                variant: variant.into(),
                extractor: ModelConfig {
                    model: args.model.clone(),
                    timeout_seconds: 180,
                },
                reviewer: ModelConfig {
                    model: args.reviewer.clone(),
                    timeout_seconds: 180,
                },
            };
            for run in 0..args.runs {
                let started = std::time::Instant::now();
                let prompt = plan.prompt()?;
                let input = extraction_message(&source, None);
                let response = provider.complete(&prompt, &input, &plan.extractor).await;
                let mut result = json!({"case_id":case["id"],"language":case["language"],"category":case["category"],"split":args.split,"run":run,"variant":variant,"plan":plan,"prompt_hash":plan.hash()?,"source_hash":hash(&source.content),"corpus_case":case,"reviews":[],"human_review":null});
                match response {
                    Err(e) => {
                        result["provider_error"] = json!(format!("{e:?}"));
                    }
                    Ok(raw) => {
                        result["raw"] = json!(raw);
                        match validate(&raw, &source) {
                            Err(error) => {
                                result["structural_valid"] = json!(false);
                                result["validation_error"] = json!(match error {
                                    Error::InvalidInput { field, .. } => field,
                                    _ => "validation",
                                });
                            }
                            Ok(batch) => {
                                result["structural_valid"] = json!(true);
                                result["claim_count"] = json!(batch.claims.len());
                                let stages = match variant {
                                    "atomicity-review" => vec![REVIEW_STAGES[0]],
                                    "fidelity-review" => {
                                        vec![REVIEW_STAGES[1]]
                                    }
                                    "audited" => REVIEW_STAGES.to_vec(),
                                    _ => vec![],
                                };
                                let mut reviews = Vec::new();
                                for stage in stages {
                                    let review_prompt = plan.review_prompt(stage)?;
                                    let review_input=json!({"original_source":source.content,"authored_at":source.authored_at,"candidate_batch":batch}).to_string();
                                    let review = provider
                                        .complete(&review_prompt, &review_input, &plan.reviewer)
                                        .await;
                                    reviews.push(match review {Ok(raw)=>json!({"stage":stage,"prompt_hash":hash(&review_prompt),"raw":raw}),Err(e)=>json!({"stage":stage,"error":format!("{e:?}")})});
                                }
                                result["reviews"] = json!(reviews);
                            }
                        }
                    }
                }
                result["elapsed_ms"] = json!(started.elapsed().as_millis());
                writeln!(file, "{}", serde_json::to_string(&result)?)?;
                file.sync_all()?;
                eprintln!(
                    "completed case={} variant={} run={}",
                    case["id"], variant, run
                );
            }
        }
    }
    Ok(())
}
