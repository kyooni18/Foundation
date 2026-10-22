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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        App,
        domain::{Principal, PrincipalKind, RecordSource},
    };
    #[test]
    fn t7_migration_history_and_pre_v2_restore() {
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
            1
        );
        let migrations = [
            Migration {
                name: MIGRATIONS[0].name,
                sql: MIGRATIONS[0].sql,
            },
            Migration {
                name: "0002_test",
                sql: "CREATE TABLE migration_probe(value TEXT) STRICT;",
            },
        ];
        apply(&mut conn, &path, &migrations).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            2
        );
        drop(conn);
        assert!(matches!(
            App::open(&path),
            Err(Error::SchemaTooNew { db: 2, binary: 1 })
        ));
        let backup = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".pre-v2.")
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
            1
        );
        conn.execute_batch(
            "DROP TRIGGER migration_no_update; UPDATE schema_migration SET checksum='tampered';",
        )
        .unwrap();
        drop(conn);
        assert!(matches!(App::open(&path), Err(Error::IntegrityFailure(_))));
    }
}
