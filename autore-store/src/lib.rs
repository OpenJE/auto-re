pub use autore_core::{Error, Result};
pub use autore_schema::{domain, ids};

pub mod storage;

pub use storage::{
    ArtifactIntegrity, ArtifactStore, Database, EntityColumn, EntityPage, EntityStore,
    NativeArtifactStore, Page, ProjectColumn, ProjectStore, ProviderAliasStore, ProviderStore,
    RunQuery, SqliteAliasStore, SqliteArtifactStore, SqliteClaimRepository, SqliteEntityStore,
    SqliteProjectStore, SqliteProviderStore, SqliteTaskRepository, Transaction,
};
