pub use autore_core::{Error, Result};
pub use autore_schema::{domain, ids};

pub mod migration;
pub mod storage;

pub use migration::{MigrationRecord, MigrationService};
pub use storage::{
    ArtifactIntegrity, ArtifactStore, ContradictionStore, Database, EntityColumn, EntityPage,
    EntityStore, EventStore, EvidenceStore, HypothesisStore, NativeArtifactStore, OperationStore,
    Page, ProjectColumn, ProjectStore, ProviderAliasStore, ProviderStore, RunQuery,
    SqliteAliasStore, SqliteArtifactStore, SqliteContradictionStore, SqliteEntityStore,
    SqliteEventStore, SqliteEvidenceStore, SqliteHypothesisStore, SqliteOperationStore,
    SqliteProjectStore, SqliteProviderStore, SqliteVerificationStore, Transaction,
    VerificationStore, build_derived_state, build_derived_state_in_tx, emit_in_tx,
    next_project_event_sequence, with_event,
};
