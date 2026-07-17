pub use autore_core::{Error, Result};
pub use autore_schema::{domain, ids};

pub mod storage;

pub use storage::{
    ArtifactIntegrity, ArtifactStore, Database, EntityColumn, EntityPage, EntityStore, Page,
    ProjectColumn, ProjectStore, ProviderStore, RunQuery, SqliteArtifactStore,
    SqliteClaimRepository, SqliteEntityStore, SqliteProjectStore, SqliteProviderStore,
    SqliteTaskRepository, Transaction,
};
