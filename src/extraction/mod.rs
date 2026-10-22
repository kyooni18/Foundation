//! Untrusted proposals, deterministic evidence checks, and independent semantic reviews.
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
const CONTRACT: &str = include_str!("../../prompts/extraction-v1/contract.txt");
const FRAMES: &str = include_str!("../../prompts/extraction-v1/frames.txt");
const REVIEW: &str = include_str!("../../prompts/extraction-v1/review.txt");
const VARIANTS: &str = include_str!("../../prompts/extraction-v1/variants.json");
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub provider: String,
    pub variant: String,
    pub extractor: ModelConfig,
    pub reviewer: ModelConfig,
}
impl Plan {
    pub fn prompt(&self) -> Result<String> {
        let variants: Value = serde_json::from_str(VARIANTS).expect("embedded variants");
        let strategy = variants[&self.variant]
            .as_str()
            .ok_or_else(|| invalid("variant", "unknown"))?;
        let examples = match self.variant.as_str() {
            "single" => "",
            "r2-frame" => include_str!("../../prompts/extraction-v1/r2-frame.txt"),
            _ => FRAMES,
        };
        Ok(format!("{CONTRACT}\n{examples}\n{strategy}"))
    }
    pub fn hash(&self) -> Result<String> {
        Ok(hash(&format!(
            "{VERSION}\n{}\n{REVIEW}\n{FRAMES}\n{}",
            self.prompt()?,
            serde_json::to_string(self).expect("plan")
        )))
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
    let value: Value = serde_json::from_str(raw).map_err(|_| invalid("schema", "invalid JSON"))?;
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
fn review_valid(raw: &str, n: usize) -> Result<()> {
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
pub async fn propose(
    provider: &dyn LlmProvider,
    plan: &Plan,
    source: &SourceView,
) -> Result<Proposal> {
    propose_with_feedback(provider, plan, source, None, &[]).await
}
async fn propose_with_feedback(
    provider: &dyn LlmProvider,
    plan: &Plan,
    source: &SourceView,
    feedback: Option<&str>,
    prior_claims: &[Value],
) -> Result<Proposal> {
    if provider.identity() != plan.provider {
        return Err(invalid("provider", "configuration mismatch"));
    }
    let mut input: Value =
        serde_json::from_str(&extraction_message(source, feedback)).expect("payload");
    input["prior_extraction_claims"] = serde_json::json!(prior_claims);
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
                ProviderError::LanguageRestriction => "language restriction",
                _ => "unavailable or invalid output",
            },
        )
    };
    let raw = provider
        .complete(&plan.prompt()?, &input, &plan.extractor)
        .await
        .map_err(call)?;
    let batch = validate(&raw, source)?;
    let mut reviews = Vec::new();
    for stage in REVIEW_STAGES {
        let prompt = review_prompt(stage);
        let input=serde_json::json!({"original_source":source.content,"authored_at":source.authored_at,"candidate_batch":batch}).to_string();
        let result = provider
            .complete(&prompt, &input, &plan.reviewer)
            .await
            .map_err(call)?;
        review_valid(&result, batch.claims.len()).map_err(|error| {
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
        let hidden:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM claim c WHERE c.source_id=?1 AND COALESCE((SELECT event FROM status_event WHERE target_kind='claim' AND target_id=c.id AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1),'restore')<>'restore')",[&lease.source_id],|r|r.get(0))?;
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
        if proposal.reviews.len() != 3 {
            return Err(invalid("semantic", "three reviews required"));
        }
        for review in &proposal.reviews {
            review_valid(review, batch.claims.len())?;
        }
        let id = ulid::Ulid::new().to_string();
        tx.execute(
            "INSERT INTO extraction_candidate VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id,
                lease.id,
                proposal.raw,
                hash(&proposal.raw),
                serde_json::to_string(&proposal.reviews).expect("reviews"),
                Timestamp::now().as_str()
            ],
        )?;
        tx.execute("UPDATE job SET state='failed',last_error='review_required',lease_owner=NULL,lease_until=NULL,updated_at=?2 WHERE id=?1",params![lease.id,Timestamp::now().as_str()])?;
        tx.commit()?;
        Ok(id)
    }
}

impl App {
    /// Process at most one job. Failures retain the source and use bounded exponential backoff.
    pub async fn work_once(
        &self,
        provider: &dyn LlmProvider,
        plan: &Plan,
    ) -> Result<Option<String>> {
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
        let outcome = async {
            let source = self.extraction_input(&lease, plan)?;
            let previous: Option<String> = self.store.reader()?.query_row(
                "SELECT last_error FROM job WHERE id=?1",
                [&lease.id],
                |r| r.get(0),
            )?;
            let reader = self.store.reader()?;
            let prior:Vec<Value>=reader.prepare("SELECT c.id,c.text FROM claim c JOIN extraction_request r ON c.job_id=r.base_job WHERE r.job_id=?1 ORDER BY c.id")?.query_map([&lease.id],|r|Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"text":r.get::<_,String>(1)?})))?.collect::<std::result::Result<_,_>>()?;
            drop(reader);
            let proposal =
                propose_with_feedback(provider, plan, &source, previous.as_deref(), &prior).await?;
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
                    })) if reason == "timeout" => "timeout",
                    Ok(Err(Error::InvalidInput { field, .. })) => match field {
                        "anchor" => "anchor",
                        "language" => "language",
                        "schema" => "schema",
                        "semantic" | "unresolved" => "unresolved",
                        "atomicity" => "atomicity",
                        "fidelity" => "fidelity",
                        "coverage" => "coverage",
                        _ => "provider",
                    },
                    _ => "provider",
                };
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
        let rows=conn.prepare("SELECT j.id,j.target_id,j.state,j.attempts,j.last_error,c.id,c.output_hash,c.output_json,j.max_attempts,j.run_after,j.lease_until FROM job j LEFT JOIN extraction_candidate c ON c.job_id=j.id ORDER BY j.created_at,j.id")?.query_map([],|r|Ok(serde_json::json!({"job":r.get::<_,String>(0)?,"source":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"attempts":r.get::<_,u32>(3)?,"failure":r.get::<_,Option<String>>(4)?,"candidate":r.get::<_,Option<String>>(5)?,"hash":r.get::<_,Option<String>>(6)?,"output":r.get::<_,Option<String>>(7)?,"max_attempts":r.get::<_,u32>(8)?,"run_after":r.get::<_,String>(9)?,"lease_until":r.get::<_,Option<String>>(10)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(Value::Array(rows))
    }
}

/// Shared evaluation/production payload; original bytes remain authoritative.
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
