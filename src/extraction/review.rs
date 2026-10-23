//! Explicit local-user inspection. Source content remains data, never instructions.
use crate::{
    App,
    domain::*,
    extraction::{self, Plan},
};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};

impl App {
    /// Return one bounded candidate and its exact original evidence for fidelity review.
    /// This is not an ordinary user projection and does not approve or endorse anything.
    pub fn extraction_review_packet(
        &self,
        principal: &Principal,
        candidate_id: &str,
    ) -> Result<Value> {
        if principal.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        let data=tx.query_row(
            "SELECT c.job_id,j.target_id,c.output_json,c.output_hash,c.reviews_json,r.plan_json,r.prompt_hash,j.state,j.last_error FROM extraction_candidate c JOIN job j ON j.id=c.job_id JOIN extraction_request r ON r.job_id=c.job_id WHERE c.id=?1",
            [candidate_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,Option<String>>(8)?)))
            .optional()?.ok_or(Error::NotFound)?;
        let (job, source_id, raw, digest, reviews, plan, prompt_hash, state, failure) = data;
        let source = crate::store::get(&tx, &source_id.parse()?)?;
        if source.lifecycle != Lifecycle::Live {
            return Err(Error::NotFound);
        }
        if extraction::hash(&raw) != digest
            || extraction::hash(&source.content) != source.content_sha256
        {
            return Err(invalid("review", "evidence hash mismatch"));
        }
        let plan: Plan =
            serde_json::from_str(&plan).map_err(|_| invalid("review", "invalid frozen plan"))?;
        if plan.hash()? != prompt_hash {
            return Err(invalid("review", "frozen prompt mismatch"));
        }
        let parsed = super::json::parse(&raw).ok();
        let validation = extraction::validate(&raw, &source);
        let structural = validation.is_ok();
        let reports = extraction::stored_reviews(&reviews)?;
        let protocol_valid = validation.as_ref().is_ok_and(|batch| {
            extraction::validate_proposal_reviews(&plan, &reports, batch.claims.len()).is_ok()
        });
        let claims=parsed.as_ref().and_then(|value|value["claims"].as_array()).map(|claims|claims.iter().enumerate().map(|(index,claim)|{
            let span=claim["anchor"]["quote"].as_str().zip(claim["anchor"]["occurrence"].as_u64()).and_then(|(quote,occurrence)|{
                usize::try_from(occurrence).ok().and_then(|occurrence|extraction::locate(&source.content,quote,occurrence).ok())
            }).map(|(start,end)|json!({"byte_start":start,"byte_end":end,"scalar_start":source.content[..start].chars().count(),"scalar_end":source.content[..end].chars().count(),"quote":&source.content[start..end]}));
            json!({"index":index,"candidate":claim,"source_span":span,"review_checks":{
                "one_independent_proposition":null,"source_support":null,"coverage":null,"negation":null,
                "condition":null,"uncertainty":null,"attribution":null,"temporal_scope":null,
                "entity_resolution":null,"original_language":null}})
        }).collect::<Vec<_>>()).unwrap_or_default();
        Ok(
            json!({"packet_version":"fidelity-review-v1","authority":"untrusted proposal; original source is evidence; model verdicts are diagnostic",
            "candidate_id":candidate_id,"job_id":job,"job_state":state,"failure":failure,
            "source":source,"candidate_hash":digest,"prompt_hash":prompt_hash,"plan":plan,
            "raw_output":raw,"claims":claims,"model_reviews":reports,
            "validation":{"version":extraction::VALIDATION_VERSION,"structural_valid":structural,"review_protocol_valid":protocol_valid},
            "span_convention":"zero-based half-open UTF-8 byte and Unicode scalar offsets, not grapheme clusters",
            "publication":"No publication by inspection. Local user approval revalidates lifecycle, frozen evidence and current source version."}),
        )
    }
}
