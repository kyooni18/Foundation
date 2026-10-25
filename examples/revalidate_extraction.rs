//! Offline revalidation of saved live outputs. Original run verdicts remain intact.
use clap::Parser;
use prime_radiant::{
    App,
    domain::*,
    extraction::{
        Plan, VALIDATION_VERSION, hash, validate, validate_proposal_reviews_bound,
        validate_reader_review, validate_review, validate_source_review_at,
    },
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
        let batch = checked.as_ref().and_then(|result| result.as_ref().ok());
        let (valid, field, count) = match &checked {
            Some(Ok(batch)) => (true, None, batch.claims.len()),
            Some(Err(Error::InvalidInput { field, .. })) => (false, Some(*field), 0),
            _ => (false, Some("provider_or_schema"), 0),
        };
        let expected = match row["variant"].as_str() {
            Some("audited" | "english-staged") => 3,
            Some("atomicity-review" | "fidelity-review") => 1,
            _ => 0,
        };
        let plan = serde_json::from_value::<Plan>(row["plan"].clone()).ok();
        let plan_hash_valid = plan.as_ref().is_some_and(|plan| {
            plan.variant == row["variant"].as_str().unwrap_or_default()
                && plan.hash().ok().as_deref() == row["prompt_hash"].as_str()
        });
        let expected = expected
            + if plan
                .as_ref()
                .is_some_and(|plan| plan.reader_prompt().is_some())
                && matches!(row["variant"].as_str(), Some("audited" | "english-staged"))
            {
                count
            } else {
                0
            };
        let bound_review_error = if let (Some(plan), Some(batch)) =
            (plan.as_ref().filter(|p| p.source_bound_review()), batch)
        {
            let raw: Option<Vec<String>> = row["reviews"].as_array().and_then(|reviews| {
                reviews
                    .iter()
                    .map(|r| r["raw"].as_str().map(str::to_owned))
                    .collect()
            });
            let raw = raw.unwrap_or_default();
            let prefix = raw.iter().enumerate().try_for_each(|(index, text)| {
                if index < batch.claims.len() {
                    validate_reader_review(plan, text)
                } else {
                    validate_source_review_at(
                        plan,
                        plan.review_stages().get(index - batch.claims.len()).ok_or(
                            Error::InvalidInput {
                                field: "schema",
                                reason: "extra review".into(),
                            },
                        )?,
                        text,
                        batch,
                        &source,
                    )
                }
            });
            match prefix.and_then(|_| validate_proposal_reviews_bound(plan, &raw, batch, &source)) {
                Ok(()) => None,
                Err(Error::InvalidInput { field, .. }) => Some(field),
                Err(_) => Some("validation"),
            }
        } else {
            None
        };
        let eligible = valid
            && plan_hash_valid
            && row["reviews"].as_array().is_some_and(|reviews| {
                if plan.as_ref().is_some_and(Plan::source_bound_review) {
                    return bound_review_error.is_none();
                }
                reviews.len() == expected
                    && reviews.iter().all(|review| {
                        review["raw"].as_str().is_some_and(|raw| {
                            validate_review(
                                raw,
                                if review["stage"]
                                    .as_str()
                                    .is_some_and(|stage| stage.starts_with("standalone"))
                                {
                                    1
                                } else {
                                    count
                                },
                            )
                            .is_ok()
                        })
                    })
            });
        row["revalidation"] = json!({"version":VALIDATION_VERSION,"valid":valid,"error":field,"experimental_gate_eligible":eligible,"plan_hash_valid":plan_hash_valid});
        if plan.as_ref().is_some_and(Plan::source_bound_review) {
            row["revalidation"]["review_error"] = json!(bound_review_error);
        }
        writeln!(output, "{}", serde_json::to_string(&row)?)?;
    }
    output.sync_all()?;
    Ok(())
}
