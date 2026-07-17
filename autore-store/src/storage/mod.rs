//! Storage layer — SQLite database, schema migrations, and repository traits.

pub mod artifact_store;
pub mod database;
pub mod entity_store;
pub mod project_store;
pub mod repositories;

pub use artifact_store::{ArtifactIntegrity, ArtifactStore, SqliteArtifactStore};
pub use database::{Database, Transaction};
pub use entity_store::{EntityColumn, EntityPage, EntityStore, SqliteEntityStore};
pub use project_store::{Page, ProjectColumn, ProjectStore, SqliteProjectStore};
pub use repositories::SqliteClaimRepository;
pub use repositories::SqliteTaskRepository;
