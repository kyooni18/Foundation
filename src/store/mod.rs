//! SQLite ownership, migrations, immutable records, and recovery.
pub(crate) mod backup;
pub(crate) mod conn;
pub(crate) mod integrity;
pub(crate) mod migrate;
use crate::domain::*;
pub use integrity::{IntegrityItem, IntegrityReport};
use rusqlite::{Connection, OptionalExtension};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};
pub(crate) struct Store {
    writer: Mutex<Connection>,
    pub path: PathBuf,
    readonly: AtomicBool,
    _ownership: conn::Ownership,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let path = conn::safe_path(path)?;
        let ownership = conn::Ownership::acquire(&path)?;
        let mut writer = conn::connect(&path, false)?;
        migrate::apply(&mut writer, &path, migrate::MIGRATIONS)?;
        integrity::require_ok(&integrity::inspect(&writer)?)?;
        Ok(Self {
            writer: Mutex::new(writer),
            path,
            readonly: AtomicBool::new(false),
            _ownership: ownership,
        })
    }
    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.writer
            .lock()
            .map_err(|_| Error::IntegrityFailure("writer mutex poisoned".into()))
    }
    pub fn writer(&self) -> Result<MutexGuard<'_, Connection>> {
        let guard = self.lock()?;
        if self.readonly.load(Ordering::SeqCst) {
            return Err(Error::IntegrityFailure(
                "writes disabled after integrity failure".into(),
            ));
        }
        Ok(guard)
    }
    pub fn reader(&self) -> Result<Connection> {
        conn::connect(&self.path, true)
    }
    pub fn verify_with(
        &self,
        derived: impl FnOnce(&Connection) -> Result<Vec<IntegrityItem>>,
    ) -> Result<IntegrityReport> {
        let mut guard = self.lock()?;
        let tx = guard.transaction()?;
        let result = integrity::inspect(&tx).and_then(|mut report| {
            if report.ok {
                report.items.extend(derived(&tx)?);
                report.ok = report.items.iter().all(|x| x.ok);
            }
            Ok(report)
        });
        if !result.as_ref().is_ok_and(|report| report.ok) {
            self.readonly.store(true, Ordering::SeqCst);
        }
        result
    }
}
pub(crate) fn get(conn: &Connection, id: &SourceId) -> Result<SourceView> {
    let raw = conn.query_row("SELECT id,recorder,recorder_label,session_id,content,content_sha256,media_type,authored_at,observed_at,captured_at,artifact_ref,retention_request,idempotency_key,request_digest, COALESCE((SELECT event FROM status_event WHERE target_kind='source' AND target_id=source.id ORDER BY seq DESC LIMIT 1),'restore') FROM source WHERE id=?1", [id.as_str()], |r| {
        Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,Option<String>>(8)?,r.get::<_,String>(9)?,r.get::<_,Option<String>>(10)?,r.get::<_,String>(11)?,r.get::<_,Option<String>>(12)?,r.get::<_,String>(13)?,r.get::<_,String>(14)?))
    }).optional()?.ok_or(Error::NotFound)?;
    Ok(SourceView {
        id: raw.0.parse()?,
        recorder: raw.1.parse()?,
        recorder_label: raw.2,
        session_id: raw.3,
        content: raw.4,
        content_sha256: raw.5,
        media_type: raw.6.parse()?,
        authored_at: raw.7.map(|v| v.parse()).transpose()?,
        observed_at: raw.8.map(|v| v.parse()).transpose()?,
        captured_at: raw.9.parse()?,
        artifact_ref: raw.10,
        retention_request: raw.11.parse()?,
        idempotency_key: raw.12,
        request_digest: raw.13,
        lifecycle: raw.14.parse::<LifecycleEvent>()?.lifecycle(),
    })
}
