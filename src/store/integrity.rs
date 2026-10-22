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
    fn new(name: &'static str) -> Self {
        Self {
            name,
            ok: true,
            violations: 0,
            first_violation: None,
        }
    }
    fn fail(&mut self, id: impl Into<String>) {
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
    for row in conn.prepare("SELECT seq FROM status_event e WHERE target_kind<>'source' OR NOT EXISTS(SELECT 1 FROM source s WHERE s.id=e.target_id)")?.query_map([], |r| r.get::<_,i64>(0))? { targets.fail(row?.to_string()); }
    let mut triggers = IntegrityItem::new("triggers");
    for name in TRIGGERS {
        let present: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='trigger' AND name=?1)",
            [name],
            |r| r.get(0),
        )?;
        if !present {
            triggers.fail(*name);
        }
    }
    let items = vec![sqlite, fks, schema, hashes, targets, triggers];
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
