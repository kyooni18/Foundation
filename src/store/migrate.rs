use super::{backup, conn, integrity};
use crate::domain::*;
use rusqlite::{Connection, TransactionBehavior, params};
use std::path::Path;
pub(crate) struct Migration {
    pub name: &'static str,
    pub sql: &'static str,
}
pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    name: "0001_init",
    sql: include_str!("migrations/0001_init.sql"),
}];
pub(crate) fn apply(conn: &mut Connection, path: &Path, migrations: &[Migration]) -> Result<()> {
    integrity::require_ok(&integrity::inspect_with(conn, migrations, true)?)?;
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, migration) in migrations.iter().enumerate().skip(version as usize) {
        let next = i + 1;
        let dest = conn::suffix(path, &format!(".pre-v{next}.{}.bak", ulid::Ulid::new()));
        backup::snapshot(conn, &dest, migrations, true)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO schema_migration(version,name,checksum,applied_at) VALUES(?1,?2,?3,?4)",
            params![
                next,
                migration.name,
                integrity::sha256(migration.sql.as_bytes()),
                Timestamp::now().as_str()
            ],
        )?;
        tx.pragma_update(None, "user_version", next)?;
        tx.commit()?;
        tracing::info!(version = next, "migration committed");
    }
    Ok(())
}
