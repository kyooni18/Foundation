use super::{
    conn::{self, Ownership},
    integrity,
    migrate::{MIGRATIONS, Migration},
};
use crate::domain::*;
use rusqlite::{Connection, backup::Backup};
use std::{fs, path::Path, time::Duration};

pub(crate) fn snapshot(
    source: &Connection,
    destination: &Path,
    migrations: &[Migration],
    allow_empty: bool,
) -> Result<()> {
    let destination = conn::safe_path(destination)?;
    let _ownership = Ownership::acquire(&destination)?;
    if destination.exists() {
        return Err(invalid("backup", "destination already exists"));
    }
    let temporary = conn::suffix(&destination, &format!(".tmp.{}", ulid::Ulid::new()));
    let result = (|| {
        let mut copy = Connection::open(&temporary)?;
        copy.execute_batch(
            "PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;",
        )?;
        Backup::new(source, &mut copy)?.run_to_completion(128, Duration::from_millis(1), None)?;
        // Standalone backup: close/checkpoint any WAL before publishing a single file.
        copy.execute_batch("PRAGMA journal_mode=DELETE;")?;
        integrity::require_ok(&integrity::inspect_with(&copy, migrations, allow_empty)?)?;
        drop(copy);
        fs::File::open(&temporary)?.sync_all()?;
        fs::rename(&temporary, &destination)?;
        sync_parent(&destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
/// Offline only. Ownership locks also prevent restoring over an App in this process.
pub fn restore(backup: &Path, target: &Path) -> Result<()> {
    let backup = conn::safe_path(backup)?;
    let target = conn::safe_path(target)?;
    if backup == target {
        return Err(invalid("target", "must differ from backup"));
    }
    let _target_owner = Ownership::acquire(&target)?;
    let _backup_owner = Ownership::acquire(&backup)?;
    let source = conn::connect(&backup, true)?;
    integrity::require_ok(&integrity::inspect_with(&source, MIGRATIONS, true)?)?;
    let staged = conn::suffix(&target, &format!(".restore.{}", ulid::Ulid::new()));
    snapshot(&source, &staged, MIGRATIONS, true)?;
    drop(source);
    if target.exists() {
        // Preserve sidecars together with the old database, including after an unclean exit.
        let replaced = conn::suffix(&target, &format!(".replaced.{}", ulid::Ulid::new()));
        fs::rename(&target, &replaced)?;
        for sidecar in ["-wal", "-shm", "-journal"] {
            let old = conn::suffix(&target, sidecar);
            if old.exists() {
                fs::rename(old, conn::suffix(&replaced, sidecar))?;
            }
        }
    } else if ["-wal", "-shm", "-journal"]
        .iter()
        .any(|s| conn::suffix(&target, s).exists())
    {
        return Err(Error::IntegrityFailure(
            "orphan target sidecars; refusing restore".into(),
        ));
    }
    fs::rename(&staged, &target)?;
    sync_parent(&target)?;
    let restored = conn::connect(&target, true)?;
    integrity::require_ok(&integrity::inspect_with(&restored, MIGRATIONS, true)?)
}
