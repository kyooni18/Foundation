pub mod api;
pub mod authentication;
pub mod authz;
pub mod claims;
pub mod extraction;
pub mod jobs;
mod list;
pub mod providers;
pub mod transport;
pub use list::{ListSources, SourcePage};
pub mod domain;
mod ingest;
pub mod store;
use domain::*;
use rusqlite::{TransactionBehavior, params};
use std::path::Path;
pub struct App {
    pub(crate) store: store::Store,
    cursor_key: [u8; 32],
}
impl App {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            store: store::Store::open(path.as_ref())?,
            cursor_key: authentication::random_bytes()?,
        })
    }
    pub fn get_source(&self, p: &Principal, id: SourceId) -> Result<SourceView> {
        self.get_source_with_options(p, id, false)
    }
    pub fn get_source_with_options(
        &self,
        p: &Principal,
        id: SourceId,
        include_retracted: bool,
    ) -> Result<SourceView> {
        let source = store::get(&self.store.reader()?, &id)?;
        if authz::visible(p, &source)
            && (p.kind() == PrincipalKind::User
                || include_retracted
                || source.lifecycle == Lifecycle::Live)
        {
            Ok(source)
        } else {
            Err(Error::NotFound)
        }
    }
    pub fn set_source_lifecycle(
        &self,
        p: &Principal,
        id: SourceId,
        event: LifecycleEvent,
        reason: Option<String>,
    ) -> Result<()> {
        let mut writer = self.store.writer()?;
        let tx = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source = store::get(&tx, &id)?;
        authz::authorize_lifecycle(p, &source)?;
        if source.lifecycle != event.lifecycle() {
            tx.execute("INSERT INTO status_event(target_kind,target_id,event,actor,reason,created_at) VALUES('source',?1,?2,?3,?4,?5)",params![id.as_str(),event.as_str(),p.kind.as_str(),reason,Timestamp::now().as_str()])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn verify_integrity(&self) -> Result<store::IntegrityReport> {
        self.store.verify()
    }
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let source = self.store.reader()?;
        store::backup::snapshot(&source, path.as_ref(), store::migrate::MIGRATIONS, false)
    }
    pub fn restore_from(backup: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
        store::backup::restore(backup.as_ref(), target.as_ref())
    }
}
