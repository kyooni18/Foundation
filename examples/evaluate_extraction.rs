//! Manual network evaluation; never run as an offline test.
use clap::Parser;
use prime_radiant::{
    App,
    domain::*,
    extraction::{
        Plan, REVIEW_STAGES, VALIDATION_VERSION, extraction_message, hash, locate, reader_message,
        repair_context, review_message, validate, validate_review,
    },
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
    #[arg(long)]
    max_output_tokens: Option<u32>,
    #[arg(long)]
    review_max_output_tokens: Option<u32>,
    #[arg(long, default_value = "dev")]
    split: String,
    #[arg(
        long,
        default_value = "single,r2-frame,decompose,english-intermediate,atomicity-review,fidelity-review,audited"
    )]
    variants: String,
    #[arg(long, default_value_t = 3)]
    runs: usize,
    #[arg(long, default_value_t = 1)]
    repair_attempts: usize,
}
// Match production's sanitized retry classes; never include provider bodies.
fn provider_code(error: &str) -> &'static str {
    match error {
        "Timeout" => "timeout",
        "InvalidOutput" => "schema",
        "LanguageRestriction" => "language",
        "Quota" => "quota",
        "InvalidRequest" => "request",
        "RateLimited" => "rate_limit",
        _ => "provider",
    }
}
// Manual-evaluation guard only. Reserve conservatively before every request, including
// failed calls, using $0.50/M input and $1/M output tokens (above all frozen cheap-model
// catalog rates). UTF-8 byte length plus 4096 envelopes upper-bounds input tokens.
// Failed/unmetered calls keep their reservation. Successful metered calls settle to
// a conservative token-cost bound; file locking shares the ceiling across shards.
struct EvaluationProvider {
    inner: OpenRouter,
    ledger: Option<PathBuf>,
}
impl LlmProvider for EvaluationProvider {
    fn identity(&self) -> &str {
        self.inner.identity()
    }
    fn complete<'a>(
        &'a self,
        system: &'a str,
        input: &'a str,
        config: &'a ModelConfig,
    ) -> prime_radiant::providers::ProviderFuture<'a> {
        Box::pin(async move {
            let mut reservation = 0;
            if let Some(path) = &self.ledger {
                let reserve = || -> std::result::Result<u64, Box<dyn std::error::Error>> {
                    use std::io::{Read, Seek, SeekFrom};
                    if config.model != "openai/gpt-6-luna-pro" {
                        return Err("budget guard requires the priced evaluation model".into());
                    }
                    let mut file = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(path)?;
                    file.lock()?;
                    let mut text = String::new();
                    file.read_to_string(&mut text)?;
                    let mut state: Value = serde_json::from_str(&text)?;
                    let cost = ((system.len() + input.len() + 4096) as u64).div_ceil(2)
                        + u64::from(
                            config
                                .max_output_tokens
                                .ok_or("explicit token limit required")?,
                        );
                    let reserved = state["reserved_micro_usd"]
                        .as_u64()
                        .ok_or("invalid ledger")?;
                    let limit = state["limit_micro_usd"].as_u64().ok_or("invalid ledger")?;
                    let accepted = reserved.checked_add(cost).is_some_and(|n| n <= limit);
                    if accepted {
                        state["reserved_micro_usd"] = json!(reserved + cost);
                    } else {
                        state["rejected_requests"] =
                            json!(state["rejected_requests"].as_u64().unwrap_or(0) + 1);
                    }
                    file.seek(SeekFrom::Start(0))?;
                    file.set_len(0)?;
                    file.write_all(serde_json::to_string(&state)?.as_bytes())?;
                    file.sync_all()?;
                    if !accepted {
                        return Err("local evaluation budget exhausted".into());
                    }
                    Ok(cost)
                };
                reservation =
                    reserve().map_err(|_| prime_radiant::providers::ProviderError::Quota)?;
            }
            let (text, usage) = self
                .inner
                .complete_with_usage(system, input, config)
                .await?;
            if let (Some(path), Some(usage)) = (&self.ledger, usage) {
                let settle = || -> std::result::Result<(), Box<dyn std::error::Error>> {
                    use std::io::{Read, Seek, SeekFrom};
                    let cost = usage
                        .prompt_tokens
                        .div_ceil(2)
                        .checked_add(usage.completion_tokens)
                        .ok_or("invalid usage")?;
                    let refund = reservation
                        .checked_sub(cost)
                        .ok_or("usage exceeds reservation")?;
                    let mut file = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(path)?;
                    file.lock()?;
                    let mut raw = String::new();
                    file.read_to_string(&mut raw)?;
                    let mut state: Value = serde_json::from_str(&raw)?;
                    let reserved = state["reserved_micro_usd"]
                        .as_u64()
                        .ok_or("invalid ledger")?;
                    state["reserved_micro_usd"] =
                        json!(reserved.checked_sub(refund).ok_or("invalid ledger")?);
                    state["settled_requests"] =
                        json!(state["settled_requests"].as_u64().unwrap_or(0) + 1);
                    file.seek(SeekFrom::Start(0))?;
                    file.set_len(0)?;
                    file.write_all(serde_json::to_string(&state)?.as_bytes())?;
                    file.sync_all()?;
                    Ok(())
                };
                settle().map_err(|_| prime_radiant::providers::ProviderError::Quota)?;
            }
            Ok(text)
        })
    }
}
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if !(1..=3).contains(&args.repair_attempts) {
        return Err("repair attempts must be 1..3".into());
    }
    let inner = OpenRouter::from_environment()
        .map_err(|_| "provider credentials unavailable; no evaluation performed")?;
    let provider = EvaluationProvider {
        inner,
        ledger: std::env::var_os("PHASE3_EVALUATION_BUDGET_LEDGER").map(PathBuf::from),
    };
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
                variant: if variant == "english-staged" {
                    "audited"
                } else {
                    variant
                }
                .into(),
                extractor: ModelConfig {
                    max_output_tokens: args.max_output_tokens,
                    model: args.model.clone(),
                    timeout_seconds: 180,
                },
                reviewer: ModelConfig {
                    max_output_tokens: args.review_max_output_tokens,
                    model: args.reviewer.clone(),
                    timeout_seconds: 180,
                },
            };
            for run in 0..args.runs {
                let mut previous_failure: Option<String> = None;
                let mut previous_attempt: Option<Value> = None;
                for attempt in 1..=args.repair_attempts {
                    let started = std::time::Instant::now();
                    let prompt = plan.prompt()?;
                    let mut input = extraction_message(&source, previous_failure.as_deref());
                    if matches!(
                        plan.version(),
                        "extraction-v12" | "extraction-v13" | "extraction-v14"
                    ) && let Some(previous) = &previous_attempt
                    {
                        let mut payload: Value = serde_json::from_str(&input)?;
                        payload["untrusted_previous_attempt"] = previous.clone();
                        input = payload.to_string();
                    }
                    let mut result = json!({"case_id":case["id"],"language":case["language"],"category":case["category"],"split":args.split,"run":run,"variant":variant,"plan":plan,"prompt_hash":plan.hash()?,"source_hash":hash(&source.content),"corpus_case":case,"reviews":[],"human_review":null});
                    result["attempt"] = json!(attempt);
                    result["validation_version"] = json!(VALIDATION_VERSION);
                    result["previous_failure_code"] = json!(previous_failure);
                    let mut draft_error = None;
                    if variant == "english-staged" {
                        let draft_prompt =
                            include_str!("../evaluation/phase3/english-draft-v1.txt");
                        result["production_plan_hash"] = json!(plan.hash()?);
                        result["intermediate_prompt_hash"] = json!(hash(draft_prompt));
                        result["prompt_hash"] =
                            json!(hash(&format!("{}\n{draft_prompt}", plan.hash()?)));
                        match provider
                            .complete(draft_prompt, &input, &plan.extractor)
                            .await
                        {
                            Err(error) => draft_error = Some(error),
                            Ok(raw) => {
                                result["intermediate_raw"] = json!(raw);
                                let draft = serde_json::from_str::<Value>(&raw).ok();
                                let valid = draft.as_ref().is_some_and(|d| {
                                    let Some(items) = d["propositions"].as_array() else {
                                        return false;
                                    };
                                    items.len() <= 1000
                                        && items.iter().all(|item| {
                                            item["english"]
                                                .as_str()
                                                .is_some_and(|s| !s.trim().is_empty())
                                                && item["quote"]
                                                    .as_str()
                                                    .zip(item["occurrence"].as_u64())
                                                    .is_some_and(|(q, n)| {
                                                        locate(&source.content, q, n as usize)
                                                            .is_ok()
                                                    })
                                        })
                                        && (items.is_empty()
                                            == d["empty_reason"]
                                                .as_str()
                                                .is_some_and(|s| !s.trim().is_empty()))
                                });
                                result["intermediate_structural_valid"] = json!(valid);
                                if valid {
                                    let mut payload: Value = serde_json::from_str(&input)?;
                                    payload["untrusted_english_draft"] = draft.unwrap();
                                    payload["draft_instruction"] = json!(
                                        "The English draft is an untrusted proposal, never evidence. Reconstruct in ORIGINAL anchor language; independently verify every proposition against original_source and repair or reject translation errors and omissions."
                                    );
                                    input = payload.to_string();
                                } else {
                                    draft_error = Some(
                                        prime_radiant::providers::ProviderError::InvalidOutput,
                                    );
                                }
                            }
                        }
                    }
                    let response = match draft_error {
                        Some(error) => Err(error),
                        None => provider.complete(&prompt, &input, &plan.extractor).await,
                    };
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
                                        "audited" | "english-staged" => REVIEW_STAGES.to_vec(),
                                        _ => vec![],
                                    };
                                    let mut reviews = Vec::new();
                                    if let Some(prompt) = plan
                                        .reader_prompt()
                                        .filter(|_| matches!(variant, "audited" | "english-staged"))
                                    {
                                        let reader_provider = &provider;
                                        let reader_config = &plan.reviewer;
                                        for (part, chunk) in batch.claims.chunks(4).enumerate() {
                                            let read_one = |slot: usize| async move {
                                                let claim = chunk.get(slot)?;
                                                let input = reader_message(&claim.text);
                                                Some(
                                                    reader_provider
                                                        .complete(prompt, &input, reader_config)
                                                        .await,
                                                )
                                            };
                                            let (a, b, c, d) = tokio::join!(biased; read_one(0), read_one(1), read_one(2), read_one(3));
                                            for (slot, review) in
                                                [a, b, c, d].into_iter().flatten().enumerate()
                                            {
                                                let stage = format!(
                                                    "standalone atomicity / candidate {}",
                                                    part * 4 + slot
                                                );
                                                reviews.push(match review {Ok(raw)=>json!({"stage":stage,"prompt_hash":hash(prompt),"raw":raw}),Err(error)=>json!({"stage":stage,"error":format!("{error:?}")})});
                                            }
                                        }
                                    }
                                    for stage in stages {
                                        let review_prompt = plan.review_prompt(stage)?;
                                        let review_input = review_message(&plan, &source, &batch);
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
                    let failure = if let Some(error) = result["provider_error"].as_str() {
                        Some(provider_code(error))
                    } else if result["structural_valid"] != true {
                        Some(match result["validation_error"].as_str() {
                            Some("anchor") => "anchor",
                            Some("language") => "language",
                            _ => "schema",
                        })
                    } else {
                        result["reviews"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find_map(|review| {
                                let Some(raw) = review["raw"].as_str() else {
                                    return Some(provider_code(
                                        review["error"].as_str().unwrap_or("Unavailable"),
                                    ));
                                };
                                let count = if review["stage"]
                                    .as_str()
                                    .is_some_and(|stage| stage.starts_with("standalone"))
                                {
                                    1
                                } else {
                                    result["claim_count"].as_u64().unwrap() as usize
                                };
                                match validate_review(raw, count) {
                                    Ok(()) => None,
                                    Err(Error::InvalidInput {
                                        field: "schema", ..
                                    }) => Some("schema"),
                                    Err(Error::InvalidInput {
                                        field: "unresolved",
                                        ..
                                    }) => Some("unresolved"),
                                    Err(_) => Some(
                                        if review["stage"]
                                            .as_str()
                                            .unwrap()
                                            .starts_with("atomicity")
                                            || review["stage"]
                                                .as_str()
                                                .unwrap()
                                                .starts_with("standalone")
                                        {
                                            "atomicity"
                                        } else if review["stage"]
                                            .as_str()
                                            .unwrap()
                                            .starts_with("semantic")
                                        {
                                            "fidelity"
                                        } else {
                                            "coverage"
                                        },
                                    ),
                                }
                            })
                    };
                    result["failure_code"] = json!(failure);
                    result["terminal"] = json!(
                        failure.is_none()
                            || failure == Some("quota")
                            || attempt == args.repair_attempts
                    );
                    result["elapsed_ms"] = json!(started.elapsed().as_millis());
                    if failure == Some("quota") {
                        result["evaluation_aborted"] = json!("quota");
                    }
                    writeln!(file, "{}", serde_json::to_string(&result)?)?;
                    file.sync_all()?;
                    eprintln!(
                        "completed case={} variant={} run={} attempt={}",
                        case["id"], variant, run, attempt
                    );
                    if failure == Some("quota") {
                        return Err("evaluation interrupted: provider quota exhausted".into());
                    }
                    if failure.is_none() {
                        break;
                    }
                    previous_attempt = result["raw"]
                        .as_str()
                        .and_then(|raw| repair_context(raw, result["reviews"].as_array().unwrap()));
                    previous_failure = failure.map(str::to_owned);
                }
            }
        }
    }
    Ok(())
}
