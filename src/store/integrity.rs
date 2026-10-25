use super::migrate::{MIGRATIONS, Migration};
use crate::domain::*;
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[derive(Debug, Serialize)]
pub struct IntegrityItem {
    pub name: &'static str,
    pub ok: bool,
    pub violations: usize,
    pub first_violation: Option<String>,
}
impl IntegrityItem {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            ok: true,
            violations: 0,
            first_violation: None,
        }
    }
    pub(crate) fn fail(&mut self, id: impl Into<String>) {
        self.ok = false;
        self.violations += 1;
        if self.first_violation.is_none() {
            self.first_violation = Some(id.into());
        }
    }
}
#[derive(Debug, Serialize)]
pub struct IntegrityReport {
    pub ok: bool,
    pub items: Vec<IntegrityItem>,
}
pub const TRIGGERS: &[&str] = &[
    "migration_no_update",
    "migration_no_delete",
    "source_no_update",
    "source_no_delete",
    "status_no_update",
    "status_no_delete",
    "status_source_exists",
    "status_protect_user_source",
    "status_restore_forgotten",
    "status_actor_scope",
];
const V3_TRIGGERS: &[&str] = &[
    "dependency_declarer_scope",
    "job_target_exists",
    "job_identity_fixed",
    "revalidation_request_no_update",
    "revalidation_request_no_delete",
    "revalidation_result_no_update",
    "revalidation_result_no_delete",
    "citation_no_update",
    "citation_no_delete",
    "citation_cycle",
    "citation_scope",
    "session_read_no_update",
    "session_read_no_delete",
    "judgment_no_update",
    "judgment_no_delete",
    "relation_no_update",
    "relation_no_delete",
    "relation_user_only",
    "relation_supersedes_cycle",
    "entity_no_update",
    "entity_no_delete",
    "identity_no_update",
    "identity_no_delete",
    "identity_targets",
];
pub fn inspect(conn: &Connection) -> Result<IntegrityReport> {
    inspect_with(conn, MIGRATIONS, false)
}
pub(crate) fn inspect_with(
    conn: &Connection,
    migrations: &[Migration],
    allow_empty: bool,
) -> Result<IntegrityReport> {
    let mut sqlite = IntegrityItem::new("sqlite");
    for row in conn
        .prepare("PRAGMA integrity_check")?
        .query_map([], |r| r.get::<_, String>(0))?
    {
        let msg = row?;
        if msg != "ok" {
            sqlite.fail(msg);
        }
    }
    let mut fks = IntegrityItem::new("foreign_keys");
    for row in conn
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |r| {
            Ok(format!(
                "{}:{:?}",
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?
            ))
        })?
    {
        fks.fail(row?);
    }
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let max = migrations.len() as u32;
    if version > max {
        return Err(Error::SchemaTooNew {
            db: version,
            binary: max,
        });
    }
    let mut schema = IntegrityItem::new("migrations");
    let count: u32 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get(0),
    )?;
    if version == 0 && count == 0 && allow_empty {
        let items = vec![sqlite, fks, schema];
        return Ok(IntegrityReport {
            ok: items.iter().all(|x| x.violations == 0),
            items,
        });
    }
    if version == 0 {
        schema.fail("unversioned nonempty database");
    }
    let mut stmt =
        conn.prepare("SELECT version,name,checksum FROM schema_migration ORDER BY version")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut seen = 0;
    for row in rows {
        let (v, name, checksum) = row?;
        seen += 1;
        if v != seen
            || v > version
            || migrations
                .get(v.wrapping_sub(1) as usize)
                .is_none_or(|m| m.name != name || sha256(m.sql.as_bytes()) != checksum)
        {
            schema.fail(v.to_string());
        }
    }
    if seen != version {
        schema.fail("user_version does not match migration history");
    }
    let mut hashes = IntegrityItem::new("source_hashes");
    for row in conn
        .prepare("SELECT id,content,content_sha256 FROM source")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
    {
        let (id, content, hash) = row?;
        if sha256(content.as_bytes()) != hash {
            hashes.fail(id);
        }
    }
    let mut targets = IntegrityItem::new("status_targets");
    let target_sql = if version >= 4 {
        "SELECT seq FROM status_event e WHERE NOT EXISTS(SELECT 1 FROM status_owner o WHERE o.target_kind=e.target_kind AND o.target_id=e.target_id) UNION ALL SELECT rowid FROM job j WHERE (kind IN ('extract','index_lexical','index_embed') AND NOT EXISTS(SELECT 1 FROM source s WHERE s.id=j.target_id)) OR (kind='revalidate' AND NOT EXISTS(SELECT 1 FROM claim c WHERE c.id=j.target_id))"
    } else if version >= 3 {
        "SELECT seq FROM status_event e WHERE NOT EXISTS(SELECT 1 FROM status_owner o WHERE o.target_kind=e.target_kind AND o.target_id=e.target_id) UNION ALL SELECT rowid FROM job j WHERE (kind='extract' AND NOT EXISTS(SELECT 1 FROM source s WHERE s.id=j.target_id)) OR (kind='revalidate' AND NOT EXISTS(SELECT 1 FROM claim c WHERE c.id=j.target_id))"
    } else if version >= 2 {
        "SELECT seq FROM status_event e WHERE (target_kind='source' AND NOT EXISTS(SELECT 1 FROM source s WHERE s.id=e.target_id)) OR (target_kind='claim' AND NOT EXISTS(SELECT 1 FROM claim c WHERE c.id=e.target_id))"
    } else {
        "SELECT seq FROM status_event e WHERE target_kind<>'source' OR NOT EXISTS(SELECT 1 FROM source s WHERE s.id=e.target_id)"
    };
    for row in conn
        .prepare(target_sql)?
        .query_map([], |r| r.get::<_, i64>(0))?
    {
        targets.fail(row?.to_string());
    }
    let mut triggers = IntegrityItem::new("triggers");
    let mut required = TRIGGERS.to_vec();
    if version >= 2 {
        required.extend([
            "claim_no_update",
            "claim_no_delete",
            "claim_anchor",
            "mention_no_update",
            "mention_no_delete",
            "dependency_no_update",
            "dependency_no_delete",
            "dependency_cycle",
            "request_no_update",
            "request_no_delete",
            "candidate_no_update",
            "candidate_no_delete",
            "run_no_update",
            "run_no_delete",
        ]);
    }
    if version >= 3 {
        required.extend(V3_TRIGGERS);
    }
    for name in required {
        let present: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='trigger' AND name=?1)",
            [name],
            |r| r.get(0),
        )?;
        if !present {
            triggers.fail(name);
        }
    }
    let mut evidence = IntegrityItem::new("claim_evidence");
    let mut publication = IntegrityItem::new("extraction_publication");
    if version >= 2 {
        for row in conn.prepare("SELECT c.id,s.content,c.anchor_start,c.anchor_end,c.anchor_quote,c.text,c.basis FROM claim c JOIN source s ON c.source_id=s.id")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,usize>(2)?,r.get::<_,usize>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?)))? {
        let (id,content,start,end,quote,text,basis)=row?;
        if start>=end || content.get(start..end)!=Some(quote.as_str()) || (basis=="verbatim" && text!=quote){evidence.fail(id);}
      }
        for row in conn
            .prepare("SELECT id,output_json,output_hash FROM extraction_candidate")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
        {
            let (id, raw, hash) = row?;
            if sha256(raw.as_bytes()) != hash {
                evidence.fail(id);
            }
        }
        for row in conn.prepare("SELECT j.id FROM job j WHERE j.kind='extract' AND (j.state='done')<>EXISTS(SELECT 1 FROM extraction_run r WHERE r.job_id=j.id) UNION SELECT c.job_id FROM claim c WHERE NOT EXISTS(SELECT 1 FROM extraction_run r WHERE r.job_id=c.job_id AND r.source_id=c.source_id)")?.query_map([],|r|r.get::<_,String>(0))? {publication.fail(row?);}
    }
    let mut identity = IntegrityItem::new("identity_targets");
    if version >= 3 {
        for row in conn.prepare("SELECT id FROM identity_assertion a WHERE (kind='refers_to' AND (NOT EXISTS(SELECT 1 FROM mention m WHERE m.id=a.subject) OR NOT EXISTS(SELECT 1 FROM entity e WHERE e.id=a.object))) OR (kind IN ('same_as','distinct_from') AND (subject>=object OR NOT EXISTS(SELECT 1 FROM entity e WHERE e.id=a.subject) OR NOT EXISTS(SELECT 1 FROM entity e WHERE e.id=a.object))) OR (kind='alias' AND NOT EXISTS(SELECT 1 FROM entity e WHERE e.id=a.subject))")?.query_map([], |r| r.get::<_, String>(0))? {
            identity.fail(row?);
        }
    }
    let items = vec![
        sqlite,
        fks,
        schema,
        hashes,
        targets,
        triggers,
        evidence,
        publication,
        identity,
    ];
    Ok(IntegrityReport {
        ok: items.iter().all(|x| x.violations == 0),
        items,
    })
}
pub fn require_ok(report: &IntegrityReport) -> Result<()> {
    if report.ok {
        Ok(())
    } else {
        Err(Error::IntegrityFailure(format!("{report:?}")))
    }
}
