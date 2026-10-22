//! Phase 1 source projections and transition policy.
use crate::domain::*;
pub fn visible(p: &Principal, source: &SourceView) -> bool {
    match p.kind() {
        PrincipalKind::User => {
            source.recorder == PrincipalKind::User && source.lifecycle == Lifecycle::Live
        }
        PrincipalKind::Agent | PrincipalKind::Import => source.lifecycle != Lifecycle::Forgotten,
    }
}
pub fn authorize_lifecycle(p: &Principal, source: &SourceView) -> Result<()> {
    // Lifecycle administration must be able to restore objects hidden by the read projection.
    if p.kind() == PrincipalKind::User {
        return Ok(());
    }
    if !visible(p, source) {
        return Err(Error::NotFound);
    }
    if p.kind() == PrincipalKind::Agent && source.recorder == PrincipalKind::Agent {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}
