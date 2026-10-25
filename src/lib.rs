#![recursion_limit = "512"]
pub mod api;
pub mod authentication;
pub mod authz;
pub mod budget;
pub mod claims;
pub mod embedding;
pub mod extraction;
pub mod integrity;
pub mod jobs;
mod list;
pub mod providers;
pub mod retrieval;
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
    pub(crate) policy: Policy,
    embedder: std::sync::OnceLock<Option<std::sync::Arc<dyn embedding::EmbeddingProvider>>>,
    pub(crate) vectors: std::sync::Mutex<retrieval::semantic::VectorCache>,
    pub(crate) query_cache: std::sync::Mutex<retrieval::semantic::QueryCache>,
}
/// Configurable settings. None of them are semantic truth rules or contracts.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub identity: integrity::IdentityPolicy,
    pub freshness: integrity::FreshnessPolicy,
    /// Model IDs whose temporal judgments are excluded from provisional supersession.
    pub blocked_temporal_models: Vec<String>,
    /// Which embedding model derived vectors belong to (local by default, cloud opt-in, or off).
    pub embedding: embedding::EmbeddingConfig,
    /// Queue and indexing resource limits.
    pub queue: retrieval::queue::QueueLimits,
    /// Retrieval gates (calibrated on development data).
    pub retrieval: retrieval::fusion::Params,
}
impl App {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_policy(path, Policy::default())
    }
    pub fn open_with_policy(path: impl AsRef<Path>, policy: Policy) -> Result<Self> {
        let app = Self {
            store: store::Store::open(path.as_ref())?,
            cursor_key: authentication::random_bytes()?,
            policy,
            embedder: Default::default(),
            vectors: Default::default(),
            query_cache: Default::default(),
        };
        app.repair_derived()?;
        Ok(app)
    }
    pub fn policy_embedding_identity(&self) -> Option<embedding::EmbeddingIdentity> {
        self.policy.embedding.identity()
    }
    /// The configured embedding provider (constructed on first use; model weights load lazily).
    pub fn embedder(&self) -> Option<std::sync::Arc<dyn embedding::EmbeddingProvider>> {
        self.embedder
            .get_or_init(|| self.policy.embedding.provider())
            .clone()
    }
    /// Derived tables are rebuilt from authoritative rows when missing or inconsistent
    /// (first open after migration, restored backups, a changed identity policy).
    fn repair_derived(&self) -> Result<()> {
        let mut w = self.store.writer()?;
        let tx = w.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stale = integrity::validity::mismatches(&tx)?;
        if !stale.is_empty() {
            tracing::warn!(
                claims = stale.len(),
                "rebuilding materialized claim validity"
            );
            integrity::validity::rebuild(&tx)?;
        }
        if !integrity::identity::consistent(&tx, self.policy.identity)? {
            tracing::warn!("rebuilding derived identity resolution");
            integrity::identity::rebuild(&tx, self.policy.identity)?;
        }
        let hidden = retrieval::access::mismatches(&tx)?;
        if !hidden.is_empty() {
            tracing::warn!(
                claims = hidden.len(),
                "rebuilding retrieval access projection"
            );
            retrieval::access::rebuild(&tx)?;
        }
        retrieval::queue::reconcile(&tx, &self.policy.embedding)?;
        tx.commit()?;
        Ok(())
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
            // A Source lifecycle change is the same mechanism as changing each of its Claims.
            let claims: Vec<String> = tx
                .prepare("SELECT id FROM claim WHERE source_id=?1 ORDER BY id")?
                .query_map([id.as_str()], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            integrity::validity::propagate(&tx, claims)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn verify_integrity(&self) -> Result<store::IntegrityReport> {
        let policy = self.policy.identity;
        self.store
            .verify_with(|conn| integrity::derived_report(conn, policy))
    }
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let source = self.store.reader()?;
        store::backup::snapshot(&source, path.as_ref(), store::migrate::MIGRATIONS, false)
    }
    pub fn restore_from(backup: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
        store::backup::restore(backup.as_ref(), target.as_ref())
    }
}
