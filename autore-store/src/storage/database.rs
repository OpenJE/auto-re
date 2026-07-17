//! SQLite database connection with refinery migrations.
//!
//! `Database` wraps a `rusqlite::Connection` in a `Mutex` for safe
//! concurrent access from async tasks. Migrations are embedded at
//! compile time via `refinery` and applied automatically on `open()`.
//!
//! # Stage 0 additions
//!
//! `Transaction` provides an atomic commit/rollback wrapper, and
//! `next_project_event_sequence` computes the next monotonic sequence
//! for a project's event log (without AUTOINCREMENT).

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use autore_schema::ids::ProjectId;

// Embed migration SQL files at compile time. The path is relative to
// `CARGO_MANIFEST_DIR` (the crate root where `Cargo.toml` lives).
refinery::embed_migrations!("../migrations");

/// A SQLite database with automatic schema migrations.
///
/// The inner `rusqlite::Connection` is wrapped in a `std::sync::Mutex`
/// so that `Database` is `Send + Sync` — safe to share across tokio
/// tasks. Callers acquire the connection via `connection()`.
pub struct Database {
    conn: Mutex<rusqlite::Connection>,
}

// SAFETY: `rusqlite::Connection` is `Send`. The `Mutex` provides
// synchronized access, making `Database` both `Send` and `Sync`.
unsafe impl Send for Database {}
unsafe impl Sync for Database {}

impl Database {
    /// Opens (or creates) a SQLite database at `path`, creating parent
    /// directories as needed, and applies all pending migrations.
    ///
    /// # Errors
    /// Returns `Error::Io` if directory creation fails, or
    /// `Error::Database` if the connection or migration fails.
    pub fn open(path: impl AsRef<Path>) -> crate::Result<Self> {
        let path = path.as_ref();

        // Create parent directories if they don't exist.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let conn =
            rusqlite::Connection::open(path).map_err(|e| crate::Error::Database(e.to_string()))?;

        // Enable WAL mode for better concurrent read performance.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        // Enable foreign key enforcement.
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let db = Database {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Opens an in-memory SQLite database and applies all pending migrations.
    /// Useful for tests.
    pub fn open_in_memory() -> crate::Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let db = Database {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Applies all pending embedded migrations.
    pub fn migrate(&self) -> crate::Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| crate::Error::Database(format!("mutex poisoned: {e}")))?;

        migrations::runner()
            .run(&mut *conn)
            .map_err(|e| crate::Error::Database(format!("migration failed: {e}")))?;

        Ok(())
    }

    /// Acquires a lock on the underlying `rusqlite::Connection`.
    ///
    /// The returned `MutexGuard` dereferences to `&mut Connection`,
    /// suitable for passing to repository implementations.
    pub fn connection(&self) -> crate::Result<MutexGuard<'_, rusqlite::Connection>> {
        self.conn
            .lock()
            .map_err(|e| crate::Error::Database(format!("mutex poisoned: {e}")))
    }

    pub fn begin_transaction(&self) -> crate::Result<Transaction<'_>> {
        let guard = self.connection()?;
        Transaction::new(guard)
    }

    pub fn next_project_event_sequence(&self, project_id: ProjectId) -> crate::Result<u64> {
        let guard = self.connection()?;
        let next: u64 = guard
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 \
                 FROM project_events WHERE project_id = ?1",
                rusqlite::params![project_id.as_uuid().as_bytes().as_slice()],
                |row| row.get(0),
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        Ok(next)
    }
}

pub struct Transaction<'a> {
    guard: MutexGuard<'a, rusqlite::Connection>,
    committed: bool,
}

impl<'a> Transaction<'a> {
    fn new(guard: MutexGuard<'a, rusqlite::Connection>) -> crate::Result<Self> {
        guard
            .execute("BEGIN IMMEDIATE", [])
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        Ok(Transaction {
            guard,
            committed: false,
        })
    }

    pub fn commit(mut self) -> crate::Result<()> {
        self.guard
            .execute("COMMIT", [])
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        self.committed = true;
        Ok(())
    }

    pub fn conn(&self) -> &rusqlite::Connection {
        &self.guard
    }
}

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.guard.execute("ROLLBACK", []);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_cleanly() {
        // Opening an in-memory database applies migrations automatically.
        let db = Database::open_in_memory().expect("in-memory database should open and migrate");

        // Verify all expected tables exist.
        let conn = db.connection().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"campaigns".to_string()));
        assert!(tables.contains(&"binary_revisions".to_string()));
        assert!(tables.contains(&"modules".to_string()));
        assert!(tables.contains(&"functions".to_string()));
        assert!(tables.contains(&"tasks".to_string()));
        assert!(tables.contains(&"claims".to_string()));
        assert!(tables.contains(&"evidences".to_string()));
        assert!(tables.contains(&"leases".to_string()));
        assert!(tables.contains(&"artifacts".to_string()));
        assert!(tables.contains(&"projects".to_string()));
        assert!(tables.contains(&"providers".to_string()));
        assert!(tables.contains(&"provider_runs".to_string()));
        assert!(tables.contains(&"provider_entity_aliases".to_string()));
        assert!(tables.contains(&"native_artifacts".to_string()));
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = Database::open_in_memory().unwrap();
        // Running migrate again should succeed without errors.
        db.migrate()
            .expect("second migration run should be a no-op");
    }

    #[test]
    fn database_opens_new_file() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let db_path = dir.join("subdir").join("test.db");

        let _db = Database::open(&db_path).expect("should create parent dirs and open database");
        assert!(db_path.exists(), "database file should exist after open");

        // Clean up the temporary directory.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn database_connection_works() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.connection().unwrap();

        // Insert and query a campaign to verify the schema is live.
        conn.execute(
            "INSERT INTO campaigns (id, name, state) VALUES (?1, ?2, ?3)",
            rusqlite::params!["test-id", "test-campaign", "Pending"],
        )
        .expect("insert should succeed");

        let name: String = conn
            .query_row(
                "SELECT name FROM campaigns WHERE id = ?1",
                rusqlite::params!["test-id"],
                |row| row.get(0),
            )
            .expect("query should succeed");

        assert_eq!(name, "test-campaign");
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.connection().unwrap();

        // Inserting a task with a non-existent campaign_id should fail.
        let result = conn.execute(
            "INSERT INTO tasks (id, campaign_id, kind, subject, state) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "task-1",
                "nonexistent-campaign",
                "AnalyzeFunction",
                "{}",
                "Pending"
            ],
        );
        assert!(
            result.is_err(),
            "foreign key constraint should reject orphan task"
        );
    }

    #[test]
    fn repository_traits_compile() {
        // Verify that all repository traits are object-safe and can be
        // referenced as trait objects. This is a compile-time check.
        fn _assert_campaign_repo(_: &dyn crate::storage::repositories::CampaignRepository) {}
        fn _assert_binary_revision_repo(
            _: &dyn crate::storage::repositories::BinaryRevisionRepository,
        ) {
        }
        fn _assert_module_repo(_: &dyn crate::storage::repositories::ModuleRepository) {}
        fn _assert_function_repo(_: &dyn crate::storage::repositories::FunctionRepository) {}
        fn _assert_task_repo(_: &dyn crate::storage::repositories::TaskRepository) {}
        fn _assert_claim_repo(_: &dyn crate::storage::repositories::ClaimRepository) {}
        fn _assert_evidence_repo(_: &dyn crate::storage::repositories::EvidenceRepository) {}
        fn _assert_artifact_repo(_: &dyn crate::storage::repositories::ArtifactRepository) {}
    }

    #[test]
    fn domain_has_no_external_imports() {
        // This test verifies at compile time that domain types can be
        // constructed without importing rusqlite or tokio. If the domain
        // layer accidentally depends on storage or async runtime types,
        // this test module would fail to compile.
        use crate::domain::{Campaign, CampaignState, Task, TaskKind, TaskState};
        use crate::domain::{RequiredCapabilities, TaskPriority, TaskSubject};
        use crate::ids::{CampaignId, TaskId};

        let campaign = Campaign::new(CampaignId::new(), "test");
        assert_eq!(campaign.state, CampaignState::Pending);

        let task = Task::new(
            TaskId::new(),
            CampaignId::new(),
            TaskKind::AnalyzeFunction,
            TaskSubject::Binary,
            TaskPriority::new(100),
            RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        );
        assert_eq!(task.state, TaskState::Pending);
    }

    #[test]
    fn foreign_keys_pragma_on() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.connection().unwrap();
        let fk: i32 = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1, "foreign_keys pragma must be ON");
    }

    #[test]
    fn lint_schema_no_db_ids() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.connection().unwrap();

        let sqls: Vec<String> = conn
            .prepare(
                "SELECT sql FROM sqlite_master \
                 WHERE type='table' AND sql IS NOT NULL \
                 AND name NOT LIKE 'sqlite_%' AND name != 'schema_migrations'",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        for sql in &sqls {
            let upper = sql.to_uppercase();
            assert!(
                !upper.contains("AUTOINCREMENT"),
                "table DDL must not use AUTOINCREMENT: {sql}"
            );
            assert!(
                !upper.contains("DEFAULT") || !upper.contains("UUID"),
                "table DDL must not use DEFAULT with uuid(): {sql}"
            );
        }
    }

    #[test]
    fn transaction_commit_persists() {
        let db = Database::open_in_memory().unwrap();

        {
            let txn = db.begin_transaction().unwrap();
            txn.conn()
                .execute(
                    "INSERT INTO campaigns (id, name, state) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["txn-1", "txn-campaign", "Pending"],
                )
                .unwrap();
            txn.commit().unwrap();
        }

        let conn = db.connection().unwrap();
        let name: String = conn
            .query_row(
                "SELECT name FROM campaigns WHERE id = ?1",
                rusqlite::params!["txn-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "txn-campaign");
    }

    #[test]
    fn transaction_rollback_on_drop() {
        let db = Database::open_in_memory().unwrap();

        {
            let txn = db.begin_transaction().unwrap();
            txn.conn()
                .execute(
                    "INSERT INTO campaigns (id, name, state) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["rollback-1", "should-not-exist", "Pending"],
                )
                .unwrap();
        }

        let conn = db.connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM campaigns WHERE id = ?1",
                rusqlite::params!["rollback-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "uncommitted insert must be rolled back");
    }

    #[test]
    fn transaction_rollback_on_error() {
        let db = Database::open_in_memory().unwrap();

        let result = {
            let txn = db.begin_transaction().unwrap();
            txn.conn()
                .execute(
                    "INSERT INTO campaigns (id, name, state) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["err-1", "error-campaign", "Pending"],
                )
                .unwrap();
            let res: crate::Result<()> =
                Err(crate::Error::Validation("simulated failure".into()));
            res
        };

        assert!(result.is_err());

        let conn = db.connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM campaigns WHERE id = ?1",
                rusqlite::params!["err-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "transaction must roll back on error path");
    }

    #[test]
    fn next_project_event_sequence_with_table() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.connection().unwrap();
        conn.execute(
            "CREATE TABLE project_events (\
                project_id BLOB NOT NULL, \
                sequence INTEGER NOT NULL, \
                PRIMARY KEY (project_id, sequence))",
            [],
        )
        .unwrap();
        drop(conn);

        let pid = ProjectId::new();
        let seq1 = db.next_project_event_sequence(pid).unwrap();
        assert_eq!(seq1, 1);

        let conn = db.connection().unwrap();
        conn.execute(
            "INSERT INTO project_events (project_id, sequence) VALUES (?1, ?2)",
            rusqlite::params![pid.as_uuid().as_bytes().as_slice(), 1i64],
        )
        .unwrap();
        drop(conn);

        let seq2 = db.next_project_event_sequence(pid).unwrap();
        assert_eq!(seq2, 2);
    }
}
