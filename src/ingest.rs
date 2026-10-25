use crate::{App, domain::*, store::integrity::sha256};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
pub(crate) fn digest(p: &Principal, req: &RecordSource) -> String {
    let fields = [
        Some(p.kind.as_str()),
        Some(req.media_type.as_str()),
        Some(req.content.as_str()),
        req.authored_at.as_ref().map(Timestamp::as_str),
        req.observed_at.as_ref().map(Timestamp::as_str),
        req.artifact_ref.as_deref(),
        Some(req.retention_request.as_str()),
    ];
    let mut bytes = Vec::new();
    for field in fields {
        bytes.extend_from_slice(&field.map_or(u64::MAX, |s| s.len() as u64).to_be_bytes());
        if let Some(s) = field {
            bytes.extend_from_slice(s.as_bytes());
        }
    }
    sha256(&bytes)
}
impl App {
    pub fn record_source(&self, p: &Principal, req: RecordSource) -> Result<Recorded> {
        req.validate()?;
        let digest = digest(p, &req);
        let mut writer = self.store.writer()?;
        let tx = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = || -> Result<Option<Recorded>> {
            if let Some(key) = &req.idempotency_key {
                let row = tx.query_row("SELECT id,request_digest FROM source WHERE recorder=?1 AND idempotency_key=?2",params![p.kind.as_str(),key], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?))).optional()?;
                if let Some((id, old)) = row {
                    let source_id = id.parse()?;
                    if old != digest {
                        return Err(Error::IdempotencyConflict { source_id });
                    }
                    return Ok(Some(Recorded {
                        source_id,
                        created: false,
                        processing: crate::retrieval::status::source_stage(
                            &tx,
                            self.policy
                                .embedding
                                .identity()
                                .as_ref()
                                .map(|i| i.key.as_str()),
                            id.as_str(),
                        )?
                        .stage,
                    }));
                }
            }
            Ok(None)
        };
        if let Some(recorded) = existing()? {
            tx.commit()?;
            return Ok(recorded);
        }
        crate::retrieval::queue::admit(&tx, &self.policy.queue)?;
        let id = SourceId::new();
        let inserted = tx.execute("INSERT INTO source(id,recorder,recorder_label,session_id,content,content_sha256,media_type,authored_at,observed_at,captured_at,artifact_ref,retention_request,idempotency_key,request_digest) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![id.as_str(),p.kind.as_str(),p.label,p.session_id,req.content,sha256(req.content.as_bytes()),req.media_type.as_str(),req.authored_at.as_ref().map(Timestamp::as_str),req.observed_at.as_ref().map(Timestamp::as_str),Timestamp::now().as_str(),req.artifact_ref,req.retention_request.as_str(),req.idempotency_key,digest]);
        if let Err(err) = inserted {
            if err.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation)
                && let Some(recorded) = existing()?
            {
                tx.commit()?;
                return Ok(recorded);
            }
            return Err(err.into());
        }
        crate::jobs::enqueue(&tx, id.as_str(), &format!("initial/{id}"))?;
        tx.commit()?;
        Ok(Recorded {
            source_id: id,
            created: true,
            processing: "stored_not_extracted",
        })
    }
}
