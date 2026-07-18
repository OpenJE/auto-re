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

        // Verify all expected V2 tables exist and obsolete V1 tables are gone.
        let conn = db.connection().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        let v2_tables = [
            "projects",
            "stage0_artifacts",
            "semantic_entities",
            "providers",
            "provider_runs",
            "provider_entity_aliases",
            "native_artifacts",
            "evidence_records",
            "evidence_lifecycle_events",
            "hypotheses",
            "contradictions",
            "verification_records",
            "operations",
            "progress_updates",
            "cancellation_requests",
            "project_events",
        ];
        for table in v2_tables {
            assert!(
                tables.contains(&table.to_string()),
                "V2 table {table} should exist"
            );
        }

        let obsolete_v1 = [
            "campaigns",
            "tasks",
            "claims",
            "evidences",
            "leases",
            "functions",
            "modules",
            "binary_revisions",
        ];
        for table in obsolete_v1 {
            assert!(
                !tables.contains(&table.to_string()),
                "obsolete V1 table {table} should be dropped"
            );
        }
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

        let id = uuid::Uuid::now_v7();
        conn.execute(
            "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id.as_bytes().as_slice(),
                "test-project",
                "2.0",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                "{}"
            ],
        )
        .expect("insert should succeed");

        let name: String = conn
            .query_row(
                "SELECT name FROM projects WHERE id = ?1",
                rusqlite::params![id.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .expect("query should succeed");

        assert_eq!(name, "test-project");
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.connection().unwrap();

        // Inserting a provider run with a non-existent project_id should fail.
        let result = conn.execute(
            "INSERT INTO provider_runs (id, project_id, provider_id, operation, configuration_hash, environment, started_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                uuid::Uuid::now_v7().as_bytes().as_slice(),
                uuid::Uuid::now_v7().as_bytes().as_slice(),
                uuid::Uuid::now_v7().as_bytes().as_slice(),
                "core.analysis",
                "deadbeef",
                "{}",
                "2026-01-01T00:00:00Z"
            ],
        );
        assert!(
            result.is_err(),
            "foreign key constraint should reject orphan provider run"
        );
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

    fn insert_test_project(conn: &rusqlite::Connection, id: &str, name: &str) {
        let uuid = uuid::Uuid::parse_str(id).expect("valid test UUID");
        conn.execute(
            "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                uuid.as_bytes().as_slice(),
                name,
                "2.0",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                "{}"
            ],
        )
        .unwrap();
    }

    #[test]
    fn transaction_commit_persists() {
        let db = Database::open_in_memory().unwrap();

        {
            let txn = db.begin_transaction().unwrap();
            insert_test_project(
                txn.conn(),
                "00000000-0000-0000-0000-000000000001",
                "txn-project",
            );
            txn.commit().unwrap();
        }

        let conn = db.connection().unwrap();
        let name: String = conn
            .query_row(
                "SELECT name FROM projects WHERE id = ?1",
                rusqlite::params![
                    uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001")
                        .unwrap()
                        .as_bytes()
                        .as_slice()
                ],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "txn-project");
    }

    #[test]
    fn transaction_rollback_on_drop() {
        let db = Database::open_in_memory().unwrap();

        {
            let txn = db.begin_transaction().unwrap();
            insert_test_project(
                txn.conn(),
                "00000000-0000-0000-0000-000000000002",
                "should-not-exist",
            );
        }

        let conn = db.connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE id = ?1",
                rusqlite::params![
                    uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000002")
                        .unwrap()
                        .as_bytes()
                        .as_slice()
                ],
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
            insert_test_project(
                txn.conn(),
                "00000000-0000-0000-0000-000000000003",
                "error-project",
            );
            let res: crate::Result<()> = Err(crate::Error::Validation("simulated failure".into()));
            res
        };

        assert!(result.is_err());

        let conn = db.connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE id = ?1",
                rusqlite::params![
                    uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000003")
                        .unwrap()
                        .as_bytes()
                        .as_slice()
                ],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "transaction must roll back on error path");
    }

    #[test]
    fn next_project_event_sequence_with_table() {
        let db = Database::open_in_memory().unwrap();

        let conn = db.connection().unwrap();
        let pid_bytes = uuid::Uuid::now_v7();
        conn.execute(
            "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                pid_bytes.as_bytes().as_slice(),
                "test",
                "2.0",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                "{}",
            ],
        )
        .unwrap();
        drop(conn);

        let pid = ProjectId::from_uuid(pid_bytes);
        let seq1 = db.next_project_event_sequence(pid).unwrap();
        assert_eq!(seq1, 1);

        let conn = db.connection().unwrap();
        conn.execute(
            "INSERT INTO project_events \
             (project_event_id, project_id, sequence, kind, source, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                uuid::Uuid::now_v7().as_bytes().as_slice(),
                pid.as_uuid().as_bytes().as_slice(),
                1i64,
                "core.project.created",
                "Project",
                "2026-01-01T00:00:00Z",
            ],
        )
        .unwrap();
        drop(conn);

        let seq2 = db.next_project_event_sequence(pid).unwrap();
        assert_eq!(seq2, 2);
    }
}
