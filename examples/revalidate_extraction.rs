//! Offline revalidation of saved live outputs. Original run verdicts remain intact.
use clap::Parser;
use prime_radiant::{
    App,
    domain::*,
    extraction::{VALIDATION_VERSION, hash, validate, validate_review},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::PathBuf,
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let temporary = tempfile::tempdir()?;
    let app = App::open(temporary.path().join("revalidation.db"))?;
    let user = Principal::for_local_cli(PrincipalKind::User, None, None);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(args.output)?;
    for line in std::io::BufReader::new(std::fs::File::open(args.input)?).lines() {
        let mut row: Value = serde_json::from_str(&line?)?;
        let case = &row["corpus_case"];
        let content = case["source"].as_str().ok_or("source absent")?;
        if row["source_hash"] != hash(content) {
            return Err("source hash mismatch".into());
        }
        let mut request = RecordSource::plain(content);
        request.authored_at = case["authored_at"].as_str().map(str::parse).transpose()?;
        request.idempotency_key = Some(hash(&case.to_string()));
        let id = app.record_source(&user, request)?.source_id;
        let source = app.get_source(&user, id)?;
        let checked = row["raw"].as_str().map(|raw| validate(raw, &source));
        let (valid, field, count) = match checked {
            Some(Ok(batch)) => (true, None, batch.claims.len()),
            Some(Err(Error::InvalidInput { field, .. })) => (false, Some(field), 0),
            _ => (false, Some("provider_or_schema"), 0),
        };
        let expected = match row["variant"].as_str() {
            Some("audited" | "english-staged") => 3,
            Some("atomicity-review" | "fidelity-review") => 1,
            _ => 0,
        };
        let eligible = valid
            && row["reviews"].as_array().is_some_and(|reviews| {
                reviews.len() == expected
                    && reviews.iter().all(|review| {
                        review["raw"]
                            .as_str()
                            .is_some_and(|raw| validate_review(raw, count).is_ok())
                    })
            });
        row["revalidation"] = json!({"version":VALIDATION_VERSION,"valid":valid,"error":field,"experimental_gate_eligible":eligible});
        writeln!(output, "{}", serde_json::to_string(&row)?)?;
    }
    output.sync_all()?;
    Ok(())
}
