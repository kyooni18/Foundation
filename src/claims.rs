//! Claim publication and projection. Models cannot approve, confirm, or set recorder.
use crate::{
    App,
    domain::*,
    extraction::{self, Plan},
};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
fn lifecycle(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Lifecycle> {
    match row.get::<_, String>(column)?.as_str() {
        "restore" => Ok(Lifecycle::Live),
        "retract" => Ok(Lifecycle::Retracted),
        "forget" => Ok(Lifecycle::Forgotten),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other("invalid lifecycle event")),
        )),
    }
}
#[derive(Debug, Serialize)]
pub struct ClaimView {
    pub id: String,
    pub source_id: String,
    pub text: String,
    pub lang: String,
    pub category: String,
    pub speaker: String,
    pub basis: String,
    pub anchor_start: Option<usize>,
    pub anchor_end: Option<usize>,
    pub anchor_quote: Option<String>,
    pub speaker_label: Option<String>,
    pub tkind: Option<String>,
    pub applies_from: Option<String>,
    pub applies_to: Option<String>,
    pub standing: bool,
    pub index_fields: Option<String>,
    pub created_at: String,
    pub lifecycle: Lifecycle,
    pub source_lifecycle: Lifecycle,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<serde_json::Value>,
    pub confirmed: bool,
    pub needs_revalidation: bool,
}
impl App {
    /// Local user review is bound to an exact candidate hash, not merely a job identifier.
    /// This approves extraction fidelity; it does not confirm the truth of a Claim.
    pub fn approve_extraction(
        &self,
        p: &Principal,
        candidate_id: &str,
        expected_hash: &str,
    ) -> Result<Vec<String>> {
        if p.kind() != PrincipalKind::User {
            return Err(Error::Forbidden);
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let (job, raw, digest, reviews): (String, String, String, String) = tx
            .query_row(
                "SELECT job_id,output_json,output_hash,reviews_json FROM extraction_candidate WHERE id=?1",
                [candidate_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if digest != expected_hash || extraction::hash(&raw) != digest {
            return Err(invalid("approval", "candidate hash mismatch"));
        }
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM extraction_run WHERE job_id=?1 AND output_hash=?2)",
            params![job, digest],
            |r| r.get(0),
        )?;
        if done {
            return Ok(tx
                .prepare("SELECT id FROM claim WHERE job_id=?1 ORDER BY id")?
                .query_map([job], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?);
        }
        let (source_id,plan,base,prompt_hash):(String,String,Option<String>,String)=tx.query_row("SELECT j.target_id,r.plan_json,r.base_job,r.prompt_hash FROM job j JOIN extraction_request r ON r.job_id=j.id WHERE j.id=?1 AND j.state='failed' AND j.last_error='review_required'",[&job],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(Error::NotFound)?;
        let source = crate::store::get(&tx, &source_id.parse()?)?;
        if source.lifecycle != Lifecycle::Live {
            return Err(Error::NotFound);
        }
        let current: Option<String> = tx
            .query_row(
                "SELECT job_id FROM extraction_run WHERE source_id=?1 ORDER BY seq DESC LIMIT 1",
                [&source_id],
                |r| r.get(0),
            )
            .optional()?;
        if current != base {
            return Err(invalid("approval", "stale extraction version"));
        }
        // A changed interpretation must never resurrect explicitly hidden knowledge.
        let hidden:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM claim c WHERE c.source_id=?1 AND COALESCE((SELECT event FROM status_event WHERE target_kind='claim' AND target_id=c.id AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1),'restore')='forget')",[&source_id],|r|r.get(0))?;
        if hidden {
            return Err(invalid(
                "approval",
                "source has forgotten historical claims; lifecycle review required",
            ));
        }
        let batch = extraction::validate(&raw, &source)?;
        let reviews = extraction::stored_reviews(&reviews)?;
        let plan: Plan = serde_json::from_str(&plan)
            .map_err(|_| invalid("plan", "invalid stored configuration"))?;
        extraction::validate_proposal_reviews_bound(&plan, &reviews, &batch, &source)?;
        if plan.hash()? != prompt_hash {
            return Err(invalid("plan", "frozen prompt hash mismatch"));
        }
        let extractor = format!(
            "{}:{}:{}:{}",
            plan.provider,
            plan.version(),
            plan.extractor.model,
            prompt_hash
        );
        let ids: Vec<String> = batch
            .claims
            .iter()
            .map(|_| ulid::Ulid::new().to_string())
            .collect();
        for (c, id) in batch.claims.iter().zip(&ids) {
            if let Some(revision) = &c.revision_of {
                let valid: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM claim WHERE id=?1 AND source_id=?2 AND job_id=?3)",
                    params![revision, source_id, base],
                    |r| r.get(0),
                )?;
                if !valid {
                    return Err(invalid("revision_of", "not in preceding source version"));
                }
            }
            let (start, end) =
                extraction::locate(&source.content, &c.anchor.quote, c.anchor.occurrence)?;
            // Metadata follows the same UTC-millisecond storage contract as Sources.
            // Keep the raw proposal/hash and original anchor bytes unchanged.
            let applies_from = c
                .applies_from
                .as_deref()
                .map(str::parse::<Timestamp>)
                .transpose()?;
            let applies_to = c
                .applies_to
                .as_deref()
                .map(str::parse::<Timestamp>)
                .transpose()?;
            tx.execute("INSERT INTO claim(id,source_id,revision_of,text,lang,category,speaker,speaker_label,basis,anchor_start,anchor_end,anchor_quote,tkind,applies_from,applies_to,standing,index_fields,extractor_id,job_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",params![id,source_id,c.revision_of,c.text,c.lang,c.category,c.speaker,c.speaker_label,c.basis,start,end,c.anchor.quote,c.tkind,applies_from.as_ref().map(Timestamp::as_str),applies_to.as_ref().map(Timestamp::as_str),c.standing,c.index_fields.as_ref().map(ValueString::json),extractor,job,Timestamp::now().as_str()])?;
            for m in &c.mentions {
                let (a, b) = extraction::locate(&c.text, &m.surface, m.occurrence)?;
                tx.execute(
                    "INSERT INTO mention VALUES(?1,?2,?3,?4)",
                    params![
                        ulid::Ulid::new().to_string(),
                        id,
                        m.surface,
                        serde_json::json!({"start":a,"end":b}).to_string()
                    ],
                )?;
            }
        }
        for (c, id) in batch.claims.iter().zip(&ids) {
            for d in &c.dependencies {
                tx.execute(
                    "INSERT INTO claim_dependency VALUES(?1,?2,?3,?4,'system',?5)",
                    params![
                        ulid::Ulid::new().to_string(),
                        id,
                        ids[d.parent],
                        d.kind,
                        Timestamp::now().as_str()
                    ],
                )?;
            }
        }
        tx.execute("INSERT INTO extraction_run(job_id,source_id,prior_job,extractor_id,output_hash,approval_actor,created_at) VALUES(?1,?2,?3,?4,?5,'user',?6)",params![job,source_id,base,extractor,digest,Timestamp::now().as_str()])?;
        tx.execute(
            "UPDATE job SET state='done',last_error=NULL,updated_at=?2 WHERE id=?1",
            params![job, Timestamp::now().as_str()],
        )?;
        tx.commit()?;
        let mut ids = ids;
        ids.sort();
        Ok(ids)
    }
    pub fn list_claims(&self, p: &Principal) -> Result<Vec<ClaimView>> {
        self.list_claims_with_options(p, false)
    }
    pub fn list_claims_with_options(
        &self,
        p: &Principal,
        include_retracted: bool,
    ) -> Result<Vec<ClaimView>> {
        self.read_claims(p, include_retracted, false)
    }
    fn read_claims(
        &self,
        p: &Principal,
        include_retracted: bool,
        history: bool,
    ) -> Result<Vec<ClaimView>> {
        let conn = self.store.reader()?;
        // Filter before projection: no source/claim totals, hidden IDs, revision links or job metadata.
        let sql="WITH visible AS (SELECT c.*,s.recorder,
     COALESCE((SELECT event FROM status_event WHERE target_kind='source' AND target_id=s.id ORDER BY seq DESC LIMIT 1),'restore') AS source_state,
     COALESCE((SELECT event FROM status_event WHERE target_kind='claim' AND target_id=c.id AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1),'restore') AS claim_state,
     COALESCE((SELECT event FROM status_event WHERE target_kind='claim' AND target_id=c.id AND event IN ('confirm','unconfirm') ORDER BY seq DESC LIMIT 1),'unconfirm')='confirm' AS confirmed,
     EXISTS(SELECT 1 FROM claim_dependency d JOIN claim sibling ON sibling.id=d.child WHERE sibling.job_id=c.job_id) AS needs_revalidation
     FROM claim c JOIN source s ON s.id=c.source_id WHERE ((?3 AND ?1<>'user') OR c.job_id=(SELECT job_id FROM extraction_run WHERE source_id=c.source_id ORDER BY seq DESC LIMIT 1)))
     SELECT id,source_id,text,lang,category,speaker,basis,anchor_start,anchor_end,anchor_quote,confirmed,needs_revalidation,speaker_label,tkind,applies_from,applies_to,standing,index_fields,created_at,claim_state,source_state,recorder,extractor_id,job_id,CASE WHEN COALESCE((SELECT event FROM status_event WHERE target_kind='claim' AND target_id=visible.revision_of AND event IN ('retract','forget','restore') ORDER BY seq DESC LIMIT 1),'restore')<>'forget' THEN revision_of END FROM visible
     WHERE source_state<>'forget' AND claim_state<>'forget' AND ((?2 AND ?1<>'user') OR (source_state='restore' AND claim_state='restore')) AND (?1<>'user' OR (NOT needs_revalidation AND ((recorder='user' AND speaker='user' AND basis='verbatim') OR confirmed))) ORDER BY id";
        Ok(conn
            .prepare(sql)?
            .query_map(params![p.kind().as_str(), include_retracted,history], |r| {
                Ok(ClaimView {
                    id: r.get(0)?,
                    source_id: r.get(1)?,
                    text: r.get(2)?,
                    lang: r.get(3)?,
                    category: r.get(4)?,
                    speaker: r.get(5)?,
                    basis: r.get(6)?,
                    anchor_start: if p.kind() == PrincipalKind::User {
                        None
                    } else {
                        Some(r.get(7)?)
                    },
                    anchor_end: if p.kind() == PrincipalKind::User {
                        None
                    } else {
                        Some(r.get(8)?)
                    },
                    anchor_quote: if p.kind() == PrincipalKind::User {
                        None
                    } else {
                        Some(r.get(9)?)
                    },
                    provenance:if p.kind()==PrincipalKind::User {None}else{Some(serde_json::json!({"extractor_id":r.get::<_,String>(22)?,"job_id":r.get::<_,String>(23)?,"revision_of":r.get::<_,Option<String>>(24)?}))},
                    confirmed: r.get(10)?,
                    needs_revalidation: r.get(11)?,
                    speaker_label: if p.kind() == PrincipalKind::User {
                        None
                    } else {
                        r.get(12)?
                    },
                    tkind: r.get(13)?,
                    applies_from: if p.kind() == PrincipalKind::User { None } else { r.get(14)? },
                    applies_to: if p.kind() == PrincipalKind::User { None } else { r.get(15)? },
                    standing: r.get(16)?,
                    index_fields: if p.kind() == PrincipalKind::User {
                        None
                    } else {
                        r.get(17)?
                    },
                    created_at: r.get(18)?,
                    lifecycle: lifecycle(r,19)?,
                    source_lifecycle: lifecycle(r,20)?,
                    recorder: if p.kind() == PrincipalKind::User {
                        None
                    } else {
                        Some(r.get(21)?)
                    },
                })
            })?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub fn get_claim(&self, p: &Principal, id: &str) -> Result<ClaimView> {
        self.get_claim_with_options(p, id, false)
    }
    pub fn get_claim_with_options(
        &self,
        p: &Principal,
        id: &str,
        include_retracted: bool,
    ) -> Result<ClaimView> {
        self.read_claims(p, include_retracted, true)?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or(Error::NotFound)
    }
    pub fn set_claim_status(&self, p: &Principal, id: &str, event: &str) -> Result<()> {
        if !["confirm", "unconfirm", "retract", "forget", "restore"].contains(&event) {
            return Err(invalid("event", "unknown"));
        }
        if p.kind() == PrincipalKind::Import
            || (["confirm", "unconfirm"].contains(&event) && p.kind() != PrincipalKind::User)
        {
            return Err(Error::Forbidden);
        }
        let mut w = self.store.writer()?;
        let tx = w.transaction()?;
        let source_id: String = tx
            .query_row("SELECT source_id FROM claim WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or(Error::NotFound)?;
        let source = crate::store::get(&tx, &source_id.parse()?)?;
        crate::authz::authorize_lifecycle(p, &source)?;
        let axis = if ["confirm", "unconfirm"].contains(&event) {
            "event IN ('confirm','unconfirm')"
        } else {
            "event IN ('retract','forget','restore')"
        };
        let prior:Option<String>=tx.query_row(&format!("SELECT event FROM status_event WHERE target_kind='claim' AND target_id=?1 AND {axis} ORDER BY seq DESC LIMIT 1"),[id],|r|r.get(0)).optional()?;
        let default = if ["confirm", "unconfirm"].contains(&event) {
            "unconfirm"
        } else {
            "restore"
        };
        if prior.as_deref().unwrap_or(default) == event {
            return Ok(());
        }
        tx.execute("INSERT INTO status_event(target_kind,target_id,event,actor,created_at) VALUES('claim',?1,?2,?3,?4)",params![id,event,p.kind().as_str(),Timestamp::now().as_str()])?;
        tx.commit()?;
        Ok(())
    }
}
trait ValueString {
    fn json(&self) -> String;
}
impl ValueString for serde_json::Value {
    fn json(&self) -> String {
        self.to_string()
    }
}

#[derive(Serialize)]
pub struct ClaimPage {
    pub claims: Vec<ClaimView>,
    pub total: usize,
    pub next_cursor: Option<String>,
}
impl App {
    pub fn page_claims(&self, p: &Principal, request: crate::ListSources) -> Result<ClaimPage> {
        use crate::authentication::{hex, unhex};
        use hmac::Mac;
        if !(1..=100).contains(&request.limit) {
            return Err(invalid("limit", "must be 1..=100"));
        }
        let after = if let Some(cursor) = request.cursor {
            if cursor.len() > 1024 {
                return Err(Error::InvalidCursor);
            }
            let (body, sig) = cursor.split_once('.').ok_or(Error::InvalidCursor)?;
            let body = unhex(body).ok_or(Error::InvalidCursor)?;
            let sig = unhex(sig).ok_or(Error::InvalidCursor)?;
            self.cursor_mac(p, &body)
                .verify_slice(&sig)
                .map_err(|_| Error::InvalidCursor)?;
            let pos: (String, bool, String) =
                serde_json::from_slice(&body).map_err(|_| Error::InvalidCursor)?;
            if pos.0 != "claims-v1"
                || pos.1 != request.include_retracted
                || pos.2.parse::<SourceId>().is_err()
            {
                return Err(Error::InvalidCursor);
            }
            pos.2
        } else {
            String::new()
        };
        let visible = self.list_claims_with_options(p, request.include_retracted)?;
        let total = visible.len();
        let mut remaining = visible.into_iter().filter(|c| c.id > after);
        let claims: Vec<_> = remaining.by_ref().take(request.limit as usize).collect();
        let next_cursor = if remaining.next().is_some() {
            let body = serde_json::to_vec(&(
                "claims-v1",
                request.include_retracted,
                &claims.last().expect("positive limit").id,
            ))
            .expect("cursor");
            let sig = self.cursor_mac(p, &body).finalize().into_bytes();
            Some(format!("{}.{}", hex(&body), hex(&sig)))
        } else {
            None
        };
        Ok(ClaimPage {
            claims,
            total,
            next_cursor,
        })
    }
}
