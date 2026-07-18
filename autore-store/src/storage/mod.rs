//! Storage layer — SQLite database, schema migrations, and repository traits.

pub mod alias_store;
pub mod artifact_store;
pub mod contradiction_store;
pub mod database;
pub mod derived;
pub mod entity_store;
pub mod event_store;
pub mod evidence_store;
pub mod hypothesis_store;
#[cfg(test)]
mod kill_resume;
pub mod operation_store;
pub mod project_store;
pub mod provider_store;
pub mod verification_store;

pub use alias_store::{NativeArtifactStore, ProviderAliasStore, SqliteAliasStore};
pub use artifact_store::{ArtifactIntegrity, ArtifactStore, SqliteArtifactStore};
pub use contradiction_store::{ContradictionStore, SqliteContradictionStore};
pub use database::{Database, Transaction};
pub use derived::{build_derived_state, build_derived_state_in_tx};
pub use entity_store::{EntityColumn, EntityPage, EntityStore, SqliteEntityStore};
pub use event_store::{
    EventStore, SqliteEventStore, emit_in_tx, next_project_event_sequence, with_event,
};
pub use evidence_store::{EvidenceStore, SqliteEvidenceStore};
pub use hypothesis_store::{HypothesisStore, SqliteHypothesisStore};
pub use operation_store::{OperationStore, SqliteOperationStore};
pub use project_store::{Page, ProjectColumn, ProjectStore, SqliteProjectStore};
pub use provider_store::{ProviderStore, RunQuery, SqliteProviderStore};
pub use verification_store::{SqliteVerificationStore, VerificationStore};
