pub use autore_core::{Error, Result};
pub use autore_schema::{domain, ids};

pub mod storage;

pub use storage::{
    ArtifactIntegrity, ArtifactStore, ContradictionStore, Database, EntityColumn, EntityPage,
    EntityStore, EvidenceStore, NativeArtifactStore, OperationStore, Page, ProjectColumn,
    ProjectStore, ProviderAliasStore, ProviderStore, RunQuery, SqliteAliasStore,
    SqliteArtifactStore, SqliteClaimRepository, SqliteContradictionStore, SqliteEntityStore,
    SqliteEvidenceStore, SqliteHypothesisStore, SqliteOperationStore, SqliteProjectStore,
    SqliteProviderStore, SqliteTaskRepository, SqliteVerificationStore, Transaction,
    VerificationStore,
};

