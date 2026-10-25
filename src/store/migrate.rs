use super::{backup, conn, integrity};
use crate::domain::*;
use rusqlite::{Connection, TransactionBehavior, params};
use std::path::Path;
#[derive(Clone, Copy)]
pub(crate) struct Migration {
    pub name: &'static str,
    pub sql: &'static str,
}
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "0001_init",
        sql: include_str!("migrations/0001_init.sql"),
    },
    Migration {
        name: "0002_extraction",
        sql: include_str!("migrations/0002_extraction.sql"),
    },
    Migration {
        name: "0003_knowledge_integrity",
        sql: include_str!("migrations/0003_knowledge_integrity.sql"),
    },
];
pub(crate) fn apply(conn: &mut Connection, path: &Path, migrations: &[Migration]) -> Result<()> {
    integrity::require_ok(&integrity::inspect_with(conn, migrations, true)?)?;
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, migration) in migrations.iter().enumerate().skip(version as usize) {
        let next = i + 1;
        let dest = conn::suffix(path, &format!(".pre-v{next}.{}.bak", ulid::Ulid::new()));
        backup::snapshot(conn, &dest, migrations, true)?;
        // SQLite's documented table-rebuild procedure: enforcement is suspended only for
        // this transaction, every reference is rechecked before commit, then restored.
        conn.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA legacy_alter_table=ON;")?;
        let committed = commit_one(conn, migration, next);
        conn.execute_batch("PRAGMA legacy_alter_table=OFF; PRAGMA foreign_keys=ON;")?;
        committed?;
        tracing::info!(version = next, "migration committed");
    }
    Ok(())
}
fn commit_one(conn: &mut Connection, migration: &Migration, next: usize) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(migration.sql)?;
    let dangling: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
        [],
        |r| r.get(0),
    )?;
    if dangling {
        return Err(Error::IntegrityFailure(format!(
            "migration {} leaves dangling references",
            migration.name
        )));
    }
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        App,
        domain::{Principal, PrincipalKind, RecordSource},
    };
    #[test]
    fn t7_migration_history_and_pre_next_restore() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let p = Principal::for_local_cli(PrincipalKind::User, None, None);
        let app = App::open(&path).unwrap();
        let id = app
            .record_source(&p, RecordSource::plain("preserved"))
            .unwrap()
            .source_id;
        drop(app);
        let app = App::open(&path).unwrap();
        drop(app);
        let mut conn = conn::connect(&path, false).unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM schema_migration", [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            MIGRATIONS.len() as u32
        );
        let mut migrations = MIGRATIONS.to_vec();
        migrations.push(Migration {
            name: "test_next",
            sql: "CREATE TABLE migration_probe(value TEXT) STRICT;",
        });
        apply(&mut conn, &path, &migrations).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            migrations.len() as u32
        );
        drop(conn);
        assert!(matches!(
            App::open(&path),
            Err(Error::SchemaTooNew { db: 4, binary: 3 })
        ));
        let backup = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".pre-v4.")
                    && p.extension().is_some_and(|s| s == "bak")
            })
            .unwrap();
        App::restore_from(backup, &path).unwrap();
        let app = App::open(&path).unwrap();
        assert_eq!(app.get_source(&p, id).unwrap().content, "preserved");
        assert!(app.verify_integrity().unwrap().ok);
        drop(app);
        let conn = conn::connect(&path, false).unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM schema_migration", [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            MIGRATIONS.len() as u32
        );
        conn.execute_batch(
            "DROP TRIGGER migration_no_update; UPDATE schema_migration SET checksum='tampered';",
        )
        .unwrap();
        drop(conn);
        assert!(matches!(App::open(&path), Err(Error::IntegrityFailure(_))));
    }
}
