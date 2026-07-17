pub use autore_core::{Error, Result};
pub use autore_schema::{domain, ids};

pub mod storage;

pub use storage::{
    ArtifactIntegrity, ArtifactStore, ContradictionStore, Database, EntityColumn, EntityPage,
    EntityStore, EventStore, EvidenceStore, NativeArtifactStore, OperationStore, Page,
    ProjectColumn, ProjectStore, ProviderAliasStore, ProviderStore, RunQuery, SqliteAliasStore,
    SqliteArtifactStore, SqliteClaimRepository, SqliteContradictionStore, SqliteEntityStore,
    SqliteEventStore, SqliteEvidenceStore, SqliteHypothesisStore, SqliteOperationStore,
    SqliteProjectStore, SqliteProviderStore, SqliteTaskRepository, SqliteVerificationStore,
    Transaction, VerificationStore, emit_in_tx, next_project_event_sequence, with_event,
};
