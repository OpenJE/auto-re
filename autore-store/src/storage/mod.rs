//! Storage layer — SQLite database, schema migrations, and repository traits.

pub mod database;
pub mod project_store;
pub mod repositories;

pub use database::{Database, Transaction};
pub use project_store::{Page, ProjectColumn, ProjectStore, SqliteProjectStore};
pub use repositories::SqliteClaimRepository;
pub use repositories::SqliteTaskRepository;
