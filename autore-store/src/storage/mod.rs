//! Storage layer — SQLite database, schema migrations, and repository traits.

pub mod alias_store;
pub mod artifact_store;
pub mod database;
pub mod entity_store;
pub mod evidence_store;
pub mod hypothesis_store;
pub mod project_store;
pub mod provider_store;
pub mod repositories;

pub use alias_store::{
    NativeArtifactStore, ProviderAliasStore, SqliteAliasStore,
};
pub use artifact_store::{ArtifactIntegrity, ArtifactStore, SqliteArtifactStore};
pub use database::{Database, Transaction};
pub use entity_store::{EntityColumn, EntityPage, EntityStore, SqliteEntityStore};
pub use evidence_store::{EvidenceStore, SqliteEvidenceStore};
pub use hypothesis_store::{HypothesisStore, SqliteHypothesisStore};
pub use project_store::{Page, ProjectColumn, ProjectStore, SqliteProjectStore};
pub use provider_store::{ProviderStore, RunQuery, SqliteProviderStore};
pub use repositories::SqliteClaimRepository;
pub use repositories::SqliteTaskRepository;
