use crate::domain::*;
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::{self, File, OpenOptions},
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub fn safe_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut resolved = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => (),
            _ => {
                resolved.push(part.as_os_str());
                if resolved.symlink_metadata().is_ok() {
                    resolved = fs::canonicalize(&resolved)?;
                }
            }
        }
        reject_cloud(&resolved)?;
    }
    reject_cloud(&resolved)?;
    Ok(resolved)
}
fn reject_cloud(path: &Path) -> Result<()> {
    let components: Vec<_> = path.components().collect();
    if components.windows(2).any(|w| {
        w[0].as_os_str() == "Library"
            && (w[1].as_os_str() == "Mobile Documents" || w[1].as_os_str() == "CloudStorage")
    }) {
        return Err(Error::UnsafeLocation(path.to_owned()));
    }
    Ok(())
}
pub fn suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
pub struct Ownership {
    _file: File,
}
impl Ownership {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(suffix(path, ".lock"))?;
        file.try_lock().map_err(|e| {
            Error::IntegrityFailure(format!(
                "database is in use; exclusive ownership required: {e}"
            ))
        })?;
        Ok(Self { _file: file })
    }
}
pub fn connect(path: &Path, readonly: bool) -> Result<Connection> {
    let flags = if readonly {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    };
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    if readonly {
        conn.pragma_update(None, "query_only", true)?;
    } else {
        let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        if mode != "wal" {
            return Err(Error::IntegrityFailure("WAL unavailable".into()));
        }
    }
    conn.execute_batch(
        "PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;",
    )?;
    Ok(conn)
}
