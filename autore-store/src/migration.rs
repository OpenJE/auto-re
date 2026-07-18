//! V1 -> V2 forward migration service.
//!
//! `MigrationService` safely copies a V1 SQLite database to a destination,
//! creates a backup, runs the embedded refinery migrations, validates that the
//! obsolete V1 tables are gone and the Stage 0 V2 tables are present, and
//! records the migration history.

use std::path::Path;

use autore_schema::domain::SchemaVersion;
use time::OffsetDateTime;

use crate::storage::Database;

// Obsolete V1 tables that must be dropped by the final migration.
const V1_TABLES: &[&str] = &[
    "campaigns",
    "tasks",
    "claims",
    "evidences",
    "leases",
    "functions",
    "modules",
    "binary_revisions",
];

// Stage 0 V2 tables that must exist after migration.
const V2_TABLES: &[&str] = &[
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
    "derived_project_summary",
    "derived_hypothesis_progress",
    "derived_evidence_progress",
    "derived_reverse_references",
];

/// A recorded schema migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationRecord {
    pub from: SchemaVersion,
    pub to: SchemaVersion,
    pub applied_at: OffsetDateTime,
    pub tool_version: String,
}

/// Service for migrating an existing V1 SQLite database to Stage 0 V2.
#[derive(Debug, Default, Clone, Copy)]
pub struct MigrationService;

impl MigrationService {
    /// Creates a new migration service.
    pub fn new() -> Self {
        Self
    }

    /// Copies `source_db_path` to `dest_db_path`, creates a backup, runs the
    /// refinery migrations, validates the schema, and records migration history.
    ///
    /// # Errors
    /// - `Error::NotFound` if the source database does not exist.
    /// - `Error::Conflict` if the destination database already exists.
    /// - `Error::Migration` if validation fails after migration.
    /// - `Error::Io` or `Error::Database` for underlying copy/open failures.
    pub fn migrate_from_v1(
        &self,
        source_db_path: impl AsRef<Path>,
        dest_db_path: impl AsRef<Path>,
    ) -> crate::Result<()> {
        let source = source_db_path.as_ref();
        let dest = dest_db_path.as_ref();

        if !source.exists() {
            return Err(crate::Error::NotFound(format!(
                "source database not found: {}",
                source.display()
            )));
        }

        if dest.exists() {
            return Err(crate::Error::Conflict(format!(
                "destination database already exists: {}",
                dest.display()
            )));
        }

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::copy(source, dest)?;

        let backup = dest.with_extension(dest.extension().map_or_else(
            || "bak".to_string(),
            |ext| format!("{}.bak", ext.to_string_lossy()),
        ));
        std::fs::copy(dest, backup)?;

        let db = Database::open(dest)?;

        validate_v2_schema(&db)?;
        record_migration(&db, SchemaVersion::new(1, 0), SchemaVersion::new(2, 0))?;

        Ok(())
    }
}

/// Validates that all obsolete V1 tables are absent and all V2 tables exist.
fn validate_v2_schema(db: &Database) -> crate::Result<()> {
    let conn = db.connection()?;

    for table in V1_TABLES {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [*table],
                |row| row.get(0),
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        if count > 0 {
            return Err(crate::Error::Migration(format!(
                "obsolete V1 table still present: {table}"
            )));
        }
    }

    for table in V2_TABLES {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [*table],
                |row| row.get(0),
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        if count == 0 {
            return Err(crate::Error::Migration(format!(
                "required V2 table missing: {table}"
            )));
        }
    }

    Ok(())
}

/// Records a `MigrationRecord` in the `migration_records` table.
fn record_migration(db: &Database, from: SchemaVersion, to: SchemaVersion) -> crate::Result<()> {
    let conn = db.connection()?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS migration_records (
            from_version TEXT NOT NULL,
            to_version TEXT NOT NULL,
            applied_at TEXT NOT NULL,
            tool_version TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| crate::Error::Database(e.to_string()))?;

    let now = OffsetDateTime::now_utc();
    let applied_at = now
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| crate::Error::Serialization(e.to_string()))?;
    let tool_version = env!("CARGO_PKG_VERSION").to_string();

    conn.execute(
        "INSERT INTO migration_records (from_version, to_version, applied_at, tool_version) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![from.to_string(), to.to_string(), applied_at, tool_version],
    )
    .map_err(|e| crate::Error::Database(e.to_string()))?;

    Ok(())
}

/// Reads migration records ordered by applied time, oldest first.
#[cfg(test)]
fn list_migration_records(db: &Database) -> crate::Result<Vec<MigrationRecord>> {
    let conn = db.connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT from_version, to_version, applied_at, tool_version \
             FROM migration_records ORDER BY applied_at ASC, rowid ASC",
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| {
            let from_str: String = row.get(0)?;
            let to_str: String = row.get(1)?;
            let applied_at_str: String = row.get(2)?;
            let tool_version: String = row.get(3)?;

            let from = parse_schema_version(&from_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid from_version: {e}"),
                    )),
                )
            })?;
            let to = parse_schema_version(&to_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid to_version: {e}"),
                    )),
                )
            })?;
            let applied_at = time::OffsetDateTime::parse(
                &applied_at_str,
                &time::format_description::well_known::Rfc3339,
            )
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid applied_at: {e}"),
                    )),
                )
            })?;

            Ok(MigrationRecord {
                from,
                to,
                applied_at,
                tool_version,
            })
        })
        .map_err(|e| crate::Error::Database(e.to_string()))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| crate::Error::Database(e.to_string()))
}

#[cfg(test)]
fn parse_schema_version(s: &str) -> Result<SchemaVersion, String> {
    let mut parts = s.splitn(2, '.');
    let major = parts
        .next()
        .ok_or("missing major")?
        .parse::<u32>()
        .map_err(|e| format!("invalid major: {e}"))?;
    let minor = parts
        .next()
        .ok_or("missing minor")?
        .parse::<u32>()
        .map_err(|e| format!("invalid minor: {e}"))?;
    Ok(SchemaVersion::new(major, minor))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Raw V1 schema SQL, used to build a V1-only fixture database.
    const V1_SCHEMA_SQL: &str = include_str!("../../migrations/V1__initial_schema.sql");

    fn create_v1_database(path: &Path) {
        let conn = rusqlite::Connection::open(path).expect("open V1 database");
        conn.execute_batch(V1_SCHEMA_SQL).expect("apply V1 schema");
    }

    fn v1_table_names() -> Vec<&'static str> {
        V1_TABLES.to_vec()
    }

    fn v2_table_names() -> Vec<&'static str> {
        V2_TABLES.to_vec()
    }

    fn table_exists(db: &Database, name: &str) -> bool {
        let conn = db.connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [name],
                |row| row.get(0),
            )
            .unwrap();
        count > 0
    }

    fn temp_db_paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let source = dir.path().join("v1.db");
        let dest = dir.path().join("v2.db");
        (dir, source, dest)
    }

    #[test]
    fn migration_v1_to_v2_creates_v2_tables() {
        let (_dir, source, dest) = temp_db_paths();
        create_v1_database(&source);

        MigrationService::new()
            .migrate_from_v1(&source, &dest)
            .expect("migrate should succeed");

        let db = Database::open(&dest).expect("open migrated database");
        for table in v2_table_names() {
            assert!(
                table_exists(&db, table),
                "V2 table {table} should exist after migration"
            );
        }
    }

    #[test]
    fn migration_v1_to_v2_drops_obsolete_v1() {
        let (_dir, source, dest) = temp_db_paths();
        create_v1_database(&source);

        MigrationService::new()
            .migrate_from_v1(&source, &dest)
            .expect("migrate should succeed");

        let db = Database::open(&dest).expect("open migrated database");
        for table in v1_table_names() {
            assert!(
                !table_exists(&db, table),
                "obsolete V1 table {table} should be absent after migration"
            );
        }
    }

    #[test]
    fn migration_marks_history() {
        let (_dir, source, dest) = temp_db_paths();
        create_v1_database(&source);

        MigrationService::new()
            .migrate_from_v1(&source, &dest)
            .expect("migrate should succeed");

        let db = Database::open(&dest).expect("open migrated database");
        let records = list_migration_records(&db).expect("list migration records");
        assert_eq!(records.len(), 1, "exactly one migration record expected");
        assert_eq!(records[0].from, SchemaVersion::new(1, 0));
        assert_eq!(records[0].to, SchemaVersion::new(2, 0));
        assert_eq!(records[0].tool_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn migrate_idempotent() {
        let (_dir, source, dest) = temp_db_paths();
        create_v1_database(&source);

        let service = MigrationService::new();
        service
            .migrate_from_v1(&source, &dest)
            .expect("first migrate should succeed");

        // Re-opening and re-running migrations should be a no-op.
        let db = Database::open(&dest).expect("reopen migrated database");
        db.migrate()
            .expect("second migration run should be a no-op");

        let records = list_migration_records(&db).expect("list migration records");
        assert_eq!(
            records.len(),
            1,
            "migration history should not duplicate after idempotent re-run"
        );
    }

    #[test]
    fn migrate_from_v1_fails_when_destination_exists() {
        let (_dir, source, dest) = temp_db_paths();
        create_v1_database(&source);
        std::fs::write(&dest, b"").expect("create empty dest file");

        let result = MigrationService::new().migrate_from_v1(&source, &dest);
        assert!(
            matches!(result, Err(crate::Error::Conflict(_))),
            "expected Conflict error when destination exists, got {result:?}"
        );
    }

    #[test]
    fn migrate_from_v1_fails_when_source_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let source = dir.path().join("missing.db");
        let dest = dir.path().join("v2.db");

        let result = MigrationService::new().migrate_from_v1(&source, &dest);
        assert!(
            matches!(result, Err(crate::Error::NotFound(_))),
            "expected NotFound error when source is missing, got {result:?}"
        );
    }

    #[test]
    fn migrate_from_v1_leaves_source_untouched() {
        let (_dir, source, dest) = temp_db_paths();
        create_v1_database(&source);

        MigrationService::new()
            .migrate_from_v1(&source, &dest)
            .expect("migrate should succeed");

        // Open source read-only to avoid accidentally running migrations.
        let src_conn = rusqlite::Connection::open_with_flags(
            &source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect("source still opens as V1");
        for table in v1_table_names() {
            let count: i64 = src_conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(count > 0, "source V1 table {table} should be untouched");
        }
    }
}
