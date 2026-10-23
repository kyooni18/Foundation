//! Untrusted proposals, deterministic evidence checks, and independent semantic reviews.
mod json;
mod review;
use crate::{
    App,
    domain::*,
    jobs::{Lease, fenced},
    providers::*,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const VERSION: &str = "extraction-v1";
pub const VALIDATION_VERSION: &str = "source-checks-v4";
const CONTRACT: &str = include_str!("../../prompts/extraction-v1/contract.txt");
const FRAMES: &str = include_str!("../../prompts/extraction-v1/frames.txt");
const REVIEW: &str = include_str!("../../prompts/extraction-v1/review.txt");
const VARIANTS: &str = include_str!("../../prompts/extraction-v1/variants.json");
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub provider: String,
    /// Omitted v1 field preserves existing frozen request serialization and hashes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_version: Option<String>,
    pub variant: String,
    pub extractor: ModelConfig,
    pub reviewer: ModelConfig,
}
impl Plan {
    pub fn version(&self) -> &str {
        self.prompt_version.as_deref().unwrap_or(VERSION)
    }
    fn components(&self) -> Result<(&'static str, &'static str, &'static str)> {
        match self.version() {
            "extraction-v1" => Ok((CONTRACT, FRAMES, REVIEW)),
            "extraction-v2" => Ok((
                include_str!("../../prompts/extraction-v2/contract.txt"),
                include_str!("../../prompts/extraction-v2/frames.txt"),
                include_str!("../../prompts/extraction-v2/review.txt"),
            )),
            "extraction-v3" => Ok((
                include_str!("../../prompts/extraction-v3/contract.txt"),
                include_str!("../../prompts/extraction-v3/frames.txt"),
                include_str!("../../prompts/extraction-v3/review.txt"),
            )),
            "extraction-v4" => Ok((
                include_str!("../../prompts/extraction-v4/contract.txt"),
                include_str!("../../prompts/extraction-v4/frames.txt"),
                include_str!("../../prompts/extraction-v4/review.txt"),
            )),
            "extraction-v5" => Ok((
                include_str!("../../prompts/extraction-v5/contract.txt"),
                include_str!("../../prompts/extraction-v5/frames.txt"),
                include_str!("../../prompts/extraction-v5/review.txt"),
            )),
            "extraction-v6" => Ok((
                include_str!("../../prompts/extraction-v6/contract.txt"),
                include_str!("../../prompts/extraction-v6/frames.txt"),
                include_str!("../../prompts/extraction-v6/review.txt"),
            )),
            "extraction-v7" => Ok((
                include_str!("../../prompts/extraction-v7/contract.txt"),
                include_str!("../../prompts/extraction-v7/frames.txt"),
                include_str!("../../prompts/extraction-v7/review.txt"),
            )),
            "extraction-v8" => Ok((
                include_str!("../../prompts/extraction-v8/contract.txt"),
                include_str!("../../prompts/extraction-v8/frames.txt"),
                include_str!("../../prompts/extraction-v8/review.txt"),
            )),
            "extraction-v9" => Ok((
                include_str!("../../prompts/extraction-v9/contract.txt"),
                include_str!("../../prompts/extraction-v9/frames.txt"),
                include_str!("../../prompts/extraction-v9/review.txt"),
            )),
            "extraction-v10" => Ok((
                include_str!("../../prompts/extraction-v10/contract.txt"),
                include_str!("../../prompts/extraction-v10/frames.txt"),
                include_str!("../../prompts/extraction-v10/review.txt"),
            )),
            "extraction-v11" => Ok((
                include_str!("../../prompts/extraction-v11/contract.txt"),
                include_str!("../../prompts/extraction-v11/frames.txt"),
                include_str!("../../prompts/extraction-v11/review.txt"),
            )),
            "extraction-v12" | "extraction-v13" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v12/frames.txt"),
                include_str!("../../prompts/extraction-v12/review.txt"),
            )),
            "extraction-v14" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v14/frames.txt"),
                include_str!("../../prompts/extraction-v12/review.txt"),
            )),
            "extraction-v15" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v15/frames.txt"),
                include_str!("../../prompts/extraction-v15/review.txt"),
            )),
            "extraction-v16" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v16/frames.txt"),
                include_str!("../../prompts/extraction-v16/review.txt"),
            )),
            "extraction-v17" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v17/frames.txt"),
                include_str!("../../prompts/extraction-v17/review.txt"),
            )),
            "extraction-v18" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v18/frames.txt"),
                include_str!("../../prompts/extraction-v18/review.txt"),
            )),
            "extraction-v19" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v19/frames.txt"),
                include_str!("../../prompts/extraction-v19/review.txt"),
            )),
            "extraction-v20" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v20/frames.txt"),
                include_str!("../../prompts/extraction-v20/review.txt"),
            )),
            "extraction-v21" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v21/frames.txt"),
                include_str!("../../prompts/extraction-v21/review.txt"),
            )),
            "extraction-v22" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v22/frames.txt"),
                include_str!("../../prompts/extraction-v22/review.txt"),
            )),
            "extraction-v23" => Ok((
                include_str!("../../prompts/extraction-v12/contract.txt"),
                include_str!("../../prompts/extraction-v23/frames.txt"),
                include_str!("../../prompts/extraction-v23/review.txt"),
            )),
            _ => Err(invalid("version", "unknown prompt version")),
        }
    }
    pub fn prompt(&self) -> Result<String> {
        let (contract, frames, _) = self.components()?;
        let variants: Value = serde_json::from_str(VARIANTS).expect("embedded variants");
        let strategy = variants[&self.variant]
            .as_str()
            .ok_or_else(|| invalid("variant", "unknown"))?;
        let examples = match self.variant.as_str() {
            "single" => "",
            "r2-frame" => include_str!("../../prompts/extraction-v1/r2-frame.txt"),
            _ => frames,
        };
        Ok(format!("{contract}\n{examples}\n{strategy}"))
    }
    pub fn review_prompt(&self, stage: &str) -> Result<String> {
        let (_, frames, review) = self.components()?;
        Ok(format!(
            "{review}\n{frames}\nIndependent review responsibility: {stage}. Compare against original, never a translation."
        ))
    }
    pub fn reader_prompt(&self) -> Option<&'static str> {
        match self.version() {
            "extraction-v11" => Some(include_str!("../../prompts/extraction-v11/reader.txt")),
            "extraction-v12" => Some(include_str!("../../prompts/extraction-v12/reader.txt")),
            "extraction-v13" => Some(include_str!("../../prompts/extraction-v13/reader.txt")),
            "extraction-v14" | "extraction-v15" | "extraction-v16" => {
                Some(include_str!("../../prompts/extraction-v14/reader.txt"))
            }
            "extraction-v17" => Some(include_str!("../../prompts/extraction-v17/reader.txt")),
            "extraction-v18" => Some(include_str!("../../prompts/extraction-v18/reader.txt")),
            "extraction-v19" => Some(include_str!("../../prompts/extraction-v19/reader.txt")),
            "extraction-v20" => Some(include_str!("../../prompts/extraction-v20/reader.txt")),
            "extraction-v21" => Some(include_str!("../../prompts/extraction-v21/reader.txt")),
            "extraction-v22" => Some(include_str!("../../prompts/extraction-v22/reader.txt")),
            "extraction-v23" => Some(include_str!("../../prompts/extraction-v23/reader.txt")),
            _ => None,
        }
    }
    /// Profiles from v12 onward include bounded previous-attempt diagnostics.
    /// Derive this from the reader profile so new frozen profiles cannot silently
    /// diverge between the core worker and evaluation harness.
    pub fn supports_repair(&self) -> bool {
        self.reader_prompt().is_some() && self.version() != "extraction-v11"
    }
    pub fn hash(&self) -> Result<String> {
        for config in [&self.extractor, &self.reviewer] {
            if config.model.trim().is_empty()
                || !(1..=600).contains(&config.timeout_seconds)
                || config
                    .max_output_tokens
                    .is_some_and(|n| !(1..=262144).contains(&n))
            {
                return Err(invalid("configuration", "invalid model configuration"));
            }
        }
        let (_, frames, review) = self.components()?;
        let mut material = format!(
            "{}\n{}\n{review}\n{frames}\n{}",
            self.version(),
            self.prompt()?,
            serde_json::to_string(self).expect("plan")
        );
        if let Some(reader) = self.reader_prompt() {
            material.push('\n');
            material.push_str(reader);
        }
        Ok(hash(&material))
    }
}
pub fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub quote: String,
    pub occurrence: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mention {
    pub surface: String,
    pub occurrence: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub parent: usize,
    pub kind: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub text: String,
    pub lang: String,
    pub category: String,
    pub speaker: String,
    pub speaker_label: Option<String>,
    pub basis: String,
    pub anchor: Anchor,
    pub tkind: Option<String>,
    pub applies_from: Option<String>,
    pub applies_to: Option<String>,
    pub standing: bool,
    pub index_fields: Option<Value>,
    pub revision_of: Option<String>,
    pub mentions: Vec<Mention>,
    pub dependencies: Vec<Dependency>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub claims: Vec<Candidate>,
    pub empty_reason: Option<String>,
}
pub fn locate(text: &str, quote: &str, occurrence: usize) -> Result<(usize, usize)> {
    if quote.is_empty() {
        return Err(invalid("anchor", "empty"));
    }
    let start = text
        .match_indices(quote)
        .nth(occurrence)
        .map(|(n, _)| n)
        .ok_or_else(|| invalid("anchor", "not an exact occurrence"))?;
    let end = start + quote.len();
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err(invalid("anchor", "UTF-8 boundary"));
    }
    Ok((start, end))
}
fn keys(v: &Value, expected: &[&str]) -> Result<()> {
    let obj = v
        .as_object()
        .ok_or_else(|| invalid("schema", "object required"))?;
    if obj.len() != expected.len() || expected.iter().any(|k| !obj.contains_key(*k)) {
        return Err(invalid("schema", "fields differ from contract"));
    }
    Ok(())
}
fn choice(value: &str, allowed: &[&str]) -> Result<()> {
    if !allowed.contains(&value) {
        Err(invalid("schema", "invalid enum"))
    } else {
        Ok(())
    }
}
fn korean(s: &str) -> bool {
    s.chars()
        .any(|c| matches!(c as u32,0x1100..=0x11ff|0x3130..=0x318f|0xac00..=0xd7af))
}
pub fn validate(raw: &str, source: &SourceView) -> Result<Batch> {
    if raw.len() > 4 * 1024 * 1024 {
        return Err(invalid("schema", "output too large"));
    }
    let value: Value =
        json::parse(raw).map_err(|_| invalid("schema", "ambiguous or invalid JSON"))?;
    keys(&value, &["claims", "empty_reason"])?;
    let array = value["claims"]
        .as_array()
        .ok_or_else(|| invalid("schema", "claims array required"))?;
    if array.len() > 1000 {
        return Err(invalid("schema", "too many claims"));
    }
    for c in array {
        keys(
            c,
            &[
                "text",
                "lang",
                "category",
                "speaker",
                "speaker_label",
                "basis",
                "anchor",
                "tkind",
                "applies_from",
                "applies_to",
                "standing",
                "index_fields",
                "revision_of",
                "mentions",
                "dependencies",
            ],
        )?;
        keys(&c["anchor"], &["quote", "occurrence"])?;
    }
    let batch: Batch =
        serde_json::from_value(value).map_err(|_| invalid("schema", "incorrect field types"))?;
    if batch.claims.is_empty() != batch.empty_reason.is_some()
        || batch
            .empty_reason
            .as_ref()
            .is_some_and(|s| s.trim().is_empty())
    {
        return Err(invalid("schema", "empty extraction requires explanation"));
    }
    let mut unique = std::collections::HashSet::new();
    for (i, c) in batch.claims.iter().enumerate() {
        if c.text.trim().is_empty() || !unique.insert(c.text.trim()) {
            return Err(invalid("schema", "empty or duplicate claim"));
        }
        choice(&c.lang, &["ko", "en"])?;
        let outside = |text: &str| {
            text.chars().any(|ch|ch.is_alphabetic() && !(ch.is_ascii() || matches!(ch as u32,0xc0..=0x24f|0x1100..=0x11ff|0x3130..=0x318f|0xac00..=0xd7af)))
        };
        let unsupported = |text: &str| {
            outside(text) && !korean(text) && !text.chars().any(|ch| ch.is_ascii_alphabetic())
        };
        if unsupported(&c.text) || unsupported(&c.anchor.quote) {
            return Err(invalid("language", "unsupported script"));
        }
        choice(&c.category, &["Fact", "View", "Intent", "Rule"])?;
        choice(&c.speaker, &["user", "agent", "external", "unknown"])?;
        // Speaker labels cannot manufacture a user citation. Cross-source citation
        // verification is not implemented in this phase, so defer these proposals.
        if c.speaker == "user" && source.recorder != PrincipalKind::User {
            return Err(invalid(
                "provenance",
                "user attribution requires a verified user source",
            ));
        }
        choice(
            &c.basis,
            &[
                "verbatim",
                "paraphrase",
                "inferred",
                "observed",
                "restated",
                "operational",
            ],
        )?;
        if let Some(t) = &c.tkind {
            choice(
                t,
                &[
                    "habit",
                    "state",
                    "event",
                    "observation",
                    "decision",
                    "report_past",
                ],
            )?;
        }
        locate(&source.content, &c.anchor.quote, c.anchor.occurrence)?;
        let latin = |text: &str| text.chars().any(|ch| ch.is_ascii_alphabetic());
        // Mixed spans cannot be classified by character counts: preserve names and
        // leave dominant-language adjudication to source-grounded and local review.
        if (c.lang == "ko" && (!korean(&c.anchor.quote) || !korean(&c.text)))
            || (c.lang == "en" && (!latin(&c.anchor.quote) || !latin(&c.text)))
            || (c.lang == "en" && korean_matrix_evidence(&c.anchor.quote))
        {
            return Err(invalid("language", "anchor language mismatch"));
        }
        if c.basis == "verbatim" && c.text != c.anchor.quote {
            return Err(invalid("anchor", "verbatim text differs"));
        }
        if c.basis == "observed" && source.artifact_ref.is_none() {
            return Err(invalid("schema", "observation needs artifact"));
        }
        for t in [&c.applies_from, &c.applies_to].into_iter().flatten() {
            let _: Timestamp = t.parse()?;
        }
        if let (Some(a), Some(b)) = (&c.applies_from, &c.applies_to)
            && a.parse::<Timestamp>()?.as_str() > b.parse::<Timestamp>()?.as_str()
        {
            return Err(invalid("schema", "reversed interval"));
        }
        if let Some(fields) = &c.index_fields {
            keys(fields, &["stance", "holder", "certainty", "condition"])?;
            if fields
                .as_object()
                .expect("object")
                .values()
                .any(|v| !v.is_string())
            {
                return Err(invalid("schema", "index fields must be strings"));
            }
        }
        for m in &c.mentions {
            locate(&c.text, &m.surface, m.occurrence)?;
        }
        let mut deps = std::collections::HashSet::new();
        for d in &c.dependencies {
            choice(
                &d.kind,
                &["premise", "restates", "alternative", "informed_by"],
            )?;
            if d.parent >= batch.claims.len() || d.parent == i || !deps.insert((d.parent, &d.kind))
            {
                return Err(invalid("schema", "invalid dependency"));
            }
        }
    }
    fn visit(
        i: usize,
        b: &Batch,
        stack: &mut Vec<usize>,
        done: &mut std::collections::HashSet<usize>,
    ) -> Result<()> {
        if stack.contains(&i) {
            return Err(invalid("schema", "dependency cycle"));
        }
        if done.contains(&i) {
            return Ok(());
        }
        stack.push(i);
        for d in &b.claims[i].dependencies {
            visit(d.parent, b, stack, done)?;
        }
        stack.pop();
        done.insert(i);
        Ok(())
    }
    let mut done = std::collections::HashSet::new();
    for i in 0..batch.claims.len() {
        visit(i, &batch, &mut Vec::new(), &mut done)?;
    }
    Ok(batch)
}
/// Narrow grammatical evidence for Latin-heavy Korean clauses, not a general
/// language detector. English clauses containing Korean names remain supported.
fn korean_matrix_evidence(span: &str) -> bool {
    let mut markers = 0;
    let mut predicate = false;
    for token in span.split_whitespace() {
        let literal = token.trim_matches(|c: char| ".,;:!?()[]{}".contains(c));
        if [('\'', '\''), ('"', '"'), ('‘', '’'), ('“', '”'), ('`', '`')]
            .iter()
            .any(|(open, close)| literal.starts_with(*open) && literal.ends_with(*close))
        {
            continue;
        }
        let token = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if !token.chars().any(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        if ["이고", "이며", "이다", "하다", "한다", "했다", "다"]
            .iter()
            .any(|ending| token.ends_with(ending))
        {
            predicate = true;
            markers += 1;
        } else if [
            "에서", "으로", "은", "는", "의", "이", "가", "을", "를", "에", "와", "과", "로", "도",
            "만",
        ]
        .iter()
        .any(|ending| token.ends_with(ending))
        {
            markers += 1;
        }
    }
    predicate && markers >= 2
}
/// Structural hints aid enumeration; the model must still explicitly review every Claim.
/// Older frozen profiles retain their original payload shape.
pub fn review_message(plan: &Plan, source: &SourceView, batch: &Batch) -> String {
    let mut input = serde_json::json!({"original_source":source.content,"authored_at":source.authored_at,"candidate_batch":batch});
    if plan.version() == "extraction-v10" || plan.reader_prompt().is_some() {
        input["candidate_count"] = serde_json::json!(batch.claims.len());
        input["required_checked_indices"] =
            serde_json::json!((0..batch.claims.len()).collect::<Vec<_>>());
    }
    input.to_string()
}

/// No Source, anchor, attribution metadata or other Claim is supplied here.
pub fn reader_message(text: &str) -> String {
    serde_json::json!({"claim_text":text,"candidate_count":1,"required_checked_indices":[0]})
        .to_string()
}

pub fn validate_proposal_reviews(plan: &Plan, reviews: &[String], n: usize) -> Result<()> {
    let readers = if plan.reader_prompt().is_some() { n } else { 0 };
    if reviews.len() != readers + REVIEW_STAGES.len() {
        return Err(invalid("semantic", "complete review protocol required"));
    }
    if readers > 0 && reviews.iter().map(String::len).sum::<usize>() > 4 * 1024 * 1024 {
        return Err(invalid("schema", "review batch too large"));
    }
    for (i, review) in reviews.iter().enumerate() {
        validate_review(review, if i < readers { 1 } else { n })?;
    }
    Ok(())
}

pub fn validate_review(raw: &str, n: usize) -> Result<()> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Review {
        verdict: String,
        issues: Vec<String>,
        checked: Vec<usize>,
        coverage_complete: bool,
    }
    if raw.len() > 4 * 1024 * 1024 {
        return Err(invalid("schema", "review too large"));
    }
    let mut review: Review =
        serde_json::from_str(raw).map_err(|_| invalid("schema", "invalid review"))?;
    choice(&review.verdict, &["pass", "reject", "unresolved"])?;
    for issue in &review.issues {
        choice(
            issue,
            &[
                "atomicity",
                "oversplit",
                "unsupported",
                "negation",
                "attribution",
                "uncertainty",
                "condition",
                "temporal",
                "reference",
                "language",
                "coverage",
                "duplicate",
                "injection",
                "ambiguity",
            ],
        )?;
    }
    review.checked.sort_unstable();
    if review.checked != (0..n).collect::<Vec<_>>() {
        return Err(invalid("schema", "review omitted or duplicated claims"));
    }
    if review.verdict == "unresolved" {
        return Err(invalid("unresolved", "review ambiguity"));
    }
    if review.verdict != "pass" || !review.coverage_complete || !review.issues.is_empty() {
        return Err(invalid("semantic", "review rejected"));
    }
    Ok(())
}

pub struct Proposal {
    pub raw: String,
    pub reviews: Vec<String>,
    pub prompt_hash: String,
}
#[derive(Default)]
struct ProposalTrace {
    raw: Option<String>,
    reviews: Vec<String>,
}
fn append_review_trace(plan: &Plan, trace: &mut ProposalTrace, raw: &str) -> Result<()> {
    if plan.reader_prompt().is_some()
        && trace.reviews.iter().map(String::len).sum::<usize>() + raw.len() > 4 * 1024 * 1024
    {
        return Err(invalid("schema", "review batch too large"));
    }
    trace.reviews.push(raw.to_owned());
    Ok(())
}

pub(crate) fn stored_reviews(raw: &str) -> Result<Vec<String>> {
    let value: Value =
        serde_json::from_str(raw).map_err(|_| invalid("schema", "invalid review record"))?;
    let reviews = if value.is_array() {
        value
    } else {
        value["reviews"].clone()
    };
    serde_json::from_value(reviews).map_err(|_| invalid("schema", "invalid review record"))
}
fn review_record(
    tx: &rusqlite::Transaction<'_>,
    lease: &Lease,
    reviews: &[String],
) -> Result<String> {
    let previous: Option<String> =
        tx.query_row("SELECT last_error FROM job WHERE id=?1", [&lease.id], |r| {
            r.get(0)
        })?;
    let serialized: String = tx.query_row(
        "SELECT plan_json FROM extraction_request WHERE job_id=?1",
        [&lease.id],
        |r| r.get(0),
    )?;
    let plan: Plan = serde_json::from_str(&serialized)
        .map_err(|_| invalid("plan", "invalid stored configuration"))?;
    let protocol = if plan.reader_prompt().is_some() {
        "standalone-prefix-v1+source-grounded-v1"
    } else {
        "source-grounded-v1"
    };
    Ok(serde_json::json!({"reviews":reviews,"attempt":lease.attempt,"previous_failure_code":previous,"validation_version":VALIDATION_VERSION,"review_protocol":protocol}).to_string())
}
pub async fn propose(
    provider: &dyn LlmProvider,
    plan: &Plan,
    source: &SourceView,
) -> Result<Proposal> {
    propose_with_feedback(
        provider,
        plan,
        source,
        None,
        &[],
        &mut ProposalTrace::default(),
        None,
    )
    .await
}
async fn propose_with_feedback(
    provider: &dyn LlmProvider,
    plan: &Plan,
    source: &SourceView,
    feedback: Option<&str>,
    prior_claims: &[Value],
    trace: &mut ProposalTrace,
    repair: Option<&Value>,
) -> Result<Proposal> {
    if provider.identity() != plan.provider {
        return Err(invalid("provider", "configuration mismatch"));
    }
    let mut input: Value =
        serde_json::from_str(&extraction_message(source, feedback)).expect("payload");
    input["prior_extraction_claims"] = serde_json::json!(prior_claims);
    if plan.supports_repair()
        && let Some(repair) = repair
    {
        input["untrusted_previous_attempt"] = repair.clone();
    }
    let input = input.to_string();
    let call = |e: ProviderError| {
        if e == ProviderError::LanguageRestriction {
            return invalid("language", "provider language restriction");
        }
        if e == ProviderError::InvalidOutput {
            return invalid("schema", "invalid provider output");
        }
        invalid(
            "provider",
            match e {
                ProviderError::Timeout => "timeout",
                ProviderError::Quota => "quota",
                ProviderError::InvalidRequest => "request",
                ProviderError::RateLimited => "rate_limit",
                ProviderError::LanguageRestriction => "language restriction",
                _ => "unavailable or invalid output",
            },
        )
    };
    let raw = provider
        .complete(&plan.prompt()?, &input, &plan.extractor)
        .await
        .map_err(call)?;
    trace.raw = Some(raw.clone());
    let batch = validate(&raw, source)?;
    let mut reviews = Vec::new();
    if let Some(prompt) = plan.reader_prompt() {
        // Bound concurrency and memory; every request contains one Claim only.
        for chunk in batch.claims.chunks(4) {
            let read_one = |slot: usize| async move {
                let candidate = chunk.get(slot)?;
                let input = reader_message(&candidate.text);
                Some(provider.complete(prompt, &input, &plan.reviewer).await)
            };
            let (a, b, c, d) =
                tokio::join!(biased; read_one(0), read_one(1), read_one(2), read_one(3));
            for result in [a, b, c, d].into_iter().flatten() {
                let raw = result.map_err(call)?;
                append_review_trace(plan, trace, &raw)?;
                validate_review(&raw, 1).map_err(|error| {
                    if matches!(
                        error,
                        Error::InvalidInput {
                            field: "schema" | "unresolved",
                            ..
                        }
                    ) {
                        error
                    } else {
                        invalid("atomicity", "standalone reader rejected")
                    }
                })?;
                reviews.push(raw);
            }
        }
    }
    for stage in REVIEW_STAGES {
        let prompt = plan.review_prompt(stage)?;
        let input = review_message(plan, source, &batch);
        let result = provider
            .complete(&prompt, &input, &plan.reviewer)
            .await
            .map_err(call)?;
        append_review_trace(plan, trace, &result)?;
        validate_review(&result, batch.claims.len()).map_err(|error| {
            if matches!(
                error,
                Error::InvalidInput {
                    field: "unresolved" | "schema",
                    ..
                }
            ) {
                return error;
            }
            invalid(
                if stage.starts_with("atomicity") {
                    "atomicity"
                } else if stage.starts_with("semantic") {
                    "fidelity"
                } else {
                    "coverage"
                },
                "review did not establish correctness",
            )
        })?;
        reviews.push(result);
    }
    Ok(Proposal {
        raw,
        reviews,
        prompt_hash: plan.hash()?,
    })
}
impl App {
    /// Freeze model/prompt configuration and prior extraction before any provider call.
    pub fn extraction_input(&self, lease: &Lease, plan: &Plan) -> Result<SourceView> {
        let serialized = serde_json::to_string(plan).expect("plan");
        plan.hash()?;
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        fenced(&tx, lease)?;
        let source = crate::store::get(&tx, &lease.source_id.parse()?)?;
        if source.lifecycle != Lifecycle::Live {
            return Err(Error::NotFound);
        }
        let hidden:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM claim c WHERE c.source_id=?1 AND COALESCE((SELECT event FROM status_event WHERE target_kind='claim' AND target_id=c.id AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1),'restore')='forget')",[&lease.source_id],|r|r.get(0))?;
        if hidden {
            return Err(Error::NotFound);
        }
        let previous: Option<String> = tx
            .query_row(
                "SELECT plan_json || char(10) || prompt_hash FROM extraction_request WHERE job_id=?1",
                [&lease.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            if previous != format!("{serialized}\n{}", plan.hash()?) {
                return Err(invalid("plan", "job configuration is immutable"));
            }
        } else {
            tx.execute("INSERT INTO extraction_request(job_id,plan_json,base_job,created_at,prompt_hash) VALUES(?1,?2,(SELECT job_id FROM extraction_run WHERE source_id=?3 ORDER BY seq DESC LIMIT 1),?4,?5)",params![lease.id,serialized,lease.source_id,Timestamp::now().as_str(),plan.hash()?])?;
        }
        tx.commit()?;
        Ok(source)
    }
    pub fn quarantine_extraction(&self, lease: &Lease, proposal: Proposal) -> Result<String> {
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        fenced(&tx, lease)?;
        let source = crate::store::get(&tx, &lease.source_id.parse()?)?;
        if source.lifecycle != Lifecycle::Live {
            return Err(Error::NotFound);
        }
        let plan: String = tx.query_row(
            "SELECT plan_json FROM extraction_request WHERE job_id=?1",
            [&lease.id],
            |r| r.get(0),
        )?;
        let plan: Plan = serde_json::from_str(&plan)
            .map_err(|_| invalid("plan", "invalid stored configuration"))?;
        let frozen: String = tx.query_row(
            "SELECT prompt_hash FROM extraction_request WHERE job_id=?1",
            [&lease.id],
            |r| r.get(0),
        )?;
        if plan.hash()? != proposal.prompt_hash || frozen != proposal.prompt_hash {
            return Err(invalid("plan", "prompt hash mismatch"));
        }
        let batch = validate(&proposal.raw, &source)?;
        validate_proposal_reviews(&plan, &proposal.reviews, batch.claims.len())?;
        let id = ulid::Ulid::new().to_string();
        tx.execute(
            "INSERT INTO extraction_candidate VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id,
                lease.id,
                proposal.raw,
                hash(&proposal.raw),
                review_record(&tx, lease, &proposal.reviews)?,
                Timestamp::now().as_str()
            ],
        )?;
        tx.execute("UPDATE job SET state='failed',last_error='review_required',lease_owner=NULL,lease_until=NULL,updated_at=?2 WHERE id=?1",params![lease.id,Timestamp::now().as_str()])?;
        tx.commit()?;
        Ok(id)
    }
    /// Untrusted diagnostics only: never transitions a job to review_required.
    fn retain_failed_proposal(&self, lease: &Lease, trace: &ProposalTrace) -> Result<()> {
        let Some(raw) = &trace.raw else {
            return Ok(());
        };
        if raw.len() > 4 * 1024 * 1024
            || trace.reviews.iter().map(String::len).sum::<usize>() > 4 * 1024 * 1024
        {
            return Ok(());
        }
        let mut writer = self.store.writer()?;
        let tx = writer.transaction()?;
        fenced(&tx, lease)?;
        let source = crate::store::get(&tx, &lease.source_id.parse()?)?;
        if source.lifecycle != Lifecycle::Live {
            return Err(Error::NotFound);
        }
        tx.execute(
            "INSERT INTO extraction_candidate VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                ulid::Ulid::new().to_string(),
                lease.id,
                raw,
                hash(raw),
                review_record(&tx, lease, &trace.reviews)?,
                Timestamp::now().as_str()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
}

impl App {
    /// Process at most one job. Failures retain the source and use bounded exponential backoff.
    pub async fn work_once(
        &self,
        provider: &dyn LlmProvider,
        plan: &Plan,
    ) -> Result<Option<String>> {
        plan.hash()?;
        use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
        fn stamp(t: OffsetDateTime) -> Timestamp {
            t.format(&Rfc3339)
                .expect("time")
                .parse()
                .expect("timestamp")
        }
        let now = OffsetDateTime::now_utc();
        let budget = plan
            .extractor
            .timeout_seconds
            .saturating_add(plan.reviewer.timeout_seconds.saturating_mul(3))
            .clamp(1, 2400);
        let Some(lease) = self.lease_extraction(
            &ulid::Ulid::new().to_string(),
            &stamp(now),
            &stamp(now + Duration::seconds(budget as i64 + 30)),
        )?
        else {
            return Ok(None);
        };
        let mut trace = ProposalTrace::default();
        let outcome = async {
            let source = self.extraction_input(&lease, plan)?;
            let previous: Option<String> = self.store.reader()?.query_row(
                "SELECT last_error FROM job WHERE id=?1",
                [&lease.id],
                |r| r.get(0),
            )?;
            let reader = self.store.reader()?;
            let repair_row: Option<(String, String)> = if plan.supports_repair() {
                reader.query_row("SELECT output_json,reviews_json FROM extraction_candidate WHERE job_id=?1 AND json_extract(reviews_json,'$.attempt')=?2 ORDER BY created_at DESC,id DESC LIMIT 1", params![lease.id,lease.attempt.saturating_sub(1)], |r| Ok((r.get(0)?,r.get(1)?))).optional()?
            } else {
                None
            };
            let repair = repair_row.and_then(|(raw, reports)| {
                let reports: Value = serde_json::from_str(&reports).ok()?;
                repair_context(&raw, reports["reviews"].as_array()?)
            });
            let prior:Vec<Value>=reader.prepare("SELECT c.id,c.text,COALESCE((SELECT event FROM status_event WHERE target_kind='claim' AND target_id=c.id AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1),'restore') FROM claim c JOIN extraction_request r ON c.job_id=r.base_job WHERE r.job_id=?1 ORDER BY c.id")?.query_map([&lease.id],|r|Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"text":r.get::<_,String>(1)?,"lifecycle_event":r.get::<_,String>(2)?})))?.collect::<std::result::Result<_,_>>()?;
            drop(reader);
            let proposal = propose_with_feedback(
                provider,
                plan,
                &source,
                previous.as_deref(),
                &prior,
                &mut trace,
                repair.as_ref(),
            )
            .await?;
            self.quarantine_extraction(&lease, proposal)
        };
        match tokio::time::timeout(std::time::Duration::from_secs(budget), outcome).await {
            Ok(Ok(id)) => Ok(Some(id)),
            failure => {
                let code = match failure {
                    Err(_) => "timeout",
                    Ok(Err(Error::NotFound)) => "lifecycle",
                    Ok(Err(Error::InvalidInput {
                        field: "provider",
                        reason,
                    })) => match reason.as_str() {
                        "timeout" => "timeout",
                        "quota" => "quota",
                        "request" => "request",
                        "rate_limit" => "rate_limit",
                        _ => "provider",
                    },
                    Ok(Err(Error::InvalidInput { field, .. })) => match field {
                        "anchor" => "anchor",
                        "language" => "language",
                        "schema" | "timestamp" => "schema",
                        "semantic" | "unresolved" | "provenance" => "unresolved",
                        "atomicity" => "atomicity",
                        "fidelity" => "fidelity",
                        "coverage" => "coverage",
                        _ => "provider",
                    },
                    _ => "provider",
                };
                match self.retain_failed_proposal(&lease, &trace) {
                    Ok(()) | Err(Error::NotFound) => {}
                    Err(error) => return Err(error),
                }
                let now = OffsetDateTime::now_utc();
                self.fail_extraction(
                    &lease,
                    code,
                    &stamp(now),
                    &stamp(now + Duration::seconds(5_i64 * 2_i64.pow(lease.attempt.min(8)))),
                )?;
                Err(invalid("extraction", code))
            }
        }
    }
    /// Trusted local review surface; never exposed to agent transports.
    pub fn inspect_extraction(&self, p: &Principal) -> Result<Value> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let conn = self.store.reader()?;
        let rows=conn.prepare("SELECT j.id,j.target_id,j.state,j.attempts,j.last_error,c.id,c.output_hash,c.output_json,j.max_attempts,j.run_after,j.lease_until,c.reviews_json,c.created_at FROM job j LEFT JOIN extraction_candidate c ON c.job_id=j.id ORDER BY j.created_at,j.id,c.created_at,c.id")?.query_map([],|r|Ok(serde_json::json!({"job":r.get::<_,String>(0)?,"source":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"attempts":r.get::<_,u32>(3)?,"failure":r.get::<_,Option<String>>(4)?,"candidate":r.get::<_,Option<String>>(5)?,"hash":r.get::<_,Option<String>>(6)?,"output":r.get::<_,Option<String>>(7)?,"max_attempts":r.get::<_,u32>(8)?,"run_after":r.get::<_,String>(9)?,"lease_until":r.get::<_,Option<String>>(10)?,"reviews":r.get::<_,Option<String>>(11)?,"candidate_created_at":r.get::<_,Option<String>>(12)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(Value::Array(rows))
    }
}

/// Shared evaluation/production payload; original bytes remain authoritative.
/// Private same-job diagnostic context; never evidence or publication approval.
/// Reports can be stored JSON strings or evaluation envelopes. Preserve their stage.
pub fn repair_context(raw: &str, reports: &[Value]) -> Option<Value> {
    if raw.len() > 4 * 1024 * 1024 {
        return None;
    }
    let candidate: Value = serde_json::from_str(raw).ok()?;
    let n = candidate["claims"].as_array()?.len();
    let mut normalized = Vec::new();
    for (i, envelope) in reports.iter().enumerate() {
        // Core traces contain raw strings; the evaluation harness retains envelopes.
        // Both must produce exactly the same repair payload, ending at the first
        // failed review. Later research-only reviews never enrich a core retry.
        let Some(raw) = envelope.as_str().or_else(|| envelope["raw"].as_str()) else {
            break;
        };
        let Ok(report) = serde_json::from_str::<Value>(raw) else {
            break;
        };
        let stage = if i < n {
            format!("standalone candidate {i}")
        } else {
            let Some(stage) = REVIEW_STAGES.get(i - n) else {
                break;
            };
            stage.to_string()
        };
        normalized.push(serde_json::json!({"stage":stage,"report":report}));
        if validate_review(raw, if i < n { 1 } else { n }).is_err() {
            break;
        }
    }
    let reports = normalized;
    let result = serde_json::json!({"candidate_batch":candidate,"review_reports":reports,"authority":"untrusted diagnostic only; original_source remains authoritative"});
    (result.to_string().len() <= 4 * 1024 * 1024).then_some(result)
}
pub fn extraction_message(source: &SourceView, feedback: Option<&str>) -> String {
    serde_json::json!({"original_source":source.content,"authored_at":source.authored_at,"observed_at":source.observed_at,"artifact_available":source.artifact_ref.is_some(),"previous_failure_code":feedback,"prior_extraction_claims":[],"repair_instruction":"Re-analyze the original source and repair the entire batch; do not omit difficult propositions to avoid failure."}).to_string()
}
pub const REVIEW_STAGES: [&str; 3] = [
    "atomicity and independence",
    "semantic fidelity and epistemic framing",
    "whole-batch coverage, duplication and conflicting interpretation",
];
pub fn review_prompt(stage: &str) -> String {
    format!(
        "{REVIEW}\n{FRAMES}\nIndependent review responsibility: {stage}. Compare against original, never a translation."
    )
}
