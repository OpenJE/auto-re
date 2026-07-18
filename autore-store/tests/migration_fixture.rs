//! Integration tests for the V1 -> V2 migration using a committed real fixture.

use std::path::PathBuf;
use std::sync::Arc;

use autore_app::application_service::requests::{
    CommandResult, ValidateProjectRequest, ValidationResult,
};
use autore_app::application_service::{ApplicationCommand, ApplicationService};
use autore_app::lifecycle::open_project;
use autore_events::project_event_service::{EventBroadcaster, LocalProjectEventService};
use autore_schema::domain::records::Project;
use autore_schema::manifest::ProjectManifest;
use autore_store::migration::MigrationService;
use autore_store::storage::{Database, ProjectStore, SqliteProjectStore};

/// Path to the committed V1 fixture database.
const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/v1_project.sqlite3"
);

fn fixture_path() -> PathBuf {
    PathBuf::from(FIXTURE_PATH)
}

fn temp_source() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let source = dir.path().join("v1.db");
    std::fs::copy(fixture_path(), &source).expect("copy fixture to temp source");
    (dir, source)
}

fn temp_dest(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("v2.db")
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

fn v1_table_names() -> Vec<&'static str> {
    vec![
        "campaigns",
        "tasks",
        "claims",
        "evidences",
        "leases",
        "functions",
        "modules",
        "binary_revisions",
    ]
}

fn v2_table_names() -> Vec<&'static str> {
    vec![
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
    ]
}

#[test]
fn successful_v1_to_v2_migration() {
    let (dir, source) = temp_source();
    let dest = temp_dest(&dir);

    MigrationService::new()
        .migrate_from_v1(&source, &dest)
        .expect("migration should succeed");

    let db = Database::open(&dest).expect("open migrated database");
    for table in v2_table_names() {
        assert!(
            table_exists(&db, table),
            "V2 table {table} should exist after migration"
        );
    }
    for table in v1_table_names() {
        assert!(
            !table_exists(&db, table),
            "obsolete V1 table {table} should be dropped"
        );
    }
}

#[test]
fn failed_migration_leaves_source_usable_and_backup_intact() {
    let (dir, source) = temp_source();
    let dest = temp_dest(&dir);

    // Pre-seed the source's refinery schema history with a divergent record so
    // that the migration runner aborts before the destination is fully migrated.
    let conn = rusqlite::Connection::open(&source).expect("open source");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS refinery_schema_history (
            version INTEGER PRIMARY KEY,
            name VARCHAR(255),
            applied_on VARCHAR(255),
            checksum VARCHAR(255)
        )",
        [],
    )
    .expect("create refinery history table");
    conn.execute(
        "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![1i32, "V1__initial_schema.sql", "2026-01-01T00:00:00Z", "0"],
    )
    .expect("insert fake history");
    drop(conn);

    let result = MigrationService::new().migrate_from_v1(&source, &dest);
    assert!(
        result.is_err(),
        "migration should fail with divergent history"
    );

    let backup = dest.with_extension("db.bak");
    assert!(backup.exists(), "backup should be created before mutation");

    // Source should still open as a pristine V1 database.
    let src_conn =
        rusqlite::Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("source still opens");
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

    // Backup should also be a valid V1 database.
    let bak_conn =
        rusqlite::Connection::open_with_flags(&backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("backup still opens");
    for table in v1_table_names() {
        let count: i64 = bak_conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0, "backup V1 table {table} should remain intact");
    }
}

#[test]
fn backup_created_before_mutation() {
    let (dir, source) = temp_source();
    let dest = temp_dest(&dir);

    MigrationService::new()
        .migrate_from_v1(&source, &dest)
        .expect("migration should succeed");

    let backup = dest.with_extension("db.bak");
    assert!(backup.exists(), "backup file should exist");

    // The backup was created before any V2 migration ran, so it must still be a
    // pristine V1 copy.
    let bak_conn =
        rusqlite::Connection::open_with_flags(&backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("backup opens");
    for table in v1_table_names() {
        let count: i64 = bak_conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0, "backup should retain V1 table {table}");
    }
    for table in v2_table_names() {
        let count: i64 = bak_conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "backup should not contain V2 table {table}");
    }
}

#[test]
fn migration_history_recorded() {
    let (dir, source) = temp_source();
    let dest = temp_dest(&dir);

    MigrationService::new()
        .migrate_from_v1(&source, &dest)
        .expect("migration should succeed");

    let db = Database::open(&dest).expect("open migrated database");
    let conn = db.connection().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'migration_records'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        count > 0,
        "migration_records table should exist after migration"
    );

    let records: Vec<(String, String, String, String)> = conn
        .prepare(
            "SELECT from_version, to_version, applied_at, tool_version \
             FROM migration_records ORDER BY applied_at ASC, rowid ASC",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(records.len(), 1, "exactly one migration record expected");
    assert_eq!(records[0].0, "1.0");
    assert_eq!(records[0].1, "2.0");
    assert!(!records[0].3.is_empty(), "tool_version should be recorded");
}

#[test]
fn validation_passes_after_migration() {
    let (dir, source) = temp_source();
    let dest = temp_dest(&dir);

    MigrationService::new()
        .migrate_from_v1(&source, &dest)
        .expect("migration should succeed");

    // Set up a project directory around the migrated database.
    let project_dir = dir.path().join("project.auto-re");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    let db_path = project_dir.join("project.sqlite3");
    std::fs::copy(&dest, &db_path).expect("copy migrated db to project dir");

    let db = Arc::new(Database::open(&db_path).expect("open project db"));
    let project = Project::new("fixture-migrated");
    let project_id = project.id;
    SqliteProjectStore::new(&db)
        .insert_project(&project)
        .expect("insert project");

    // Create the manifest and supporting files so the directory is a valid project.
    let manifest = ProjectManifest::new(project, project_dir.join("project.toml"));
    manifest.save(&manifest.path).expect("save manifest");
    std::fs::create_dir_all(project_dir.join("artifacts")).expect("create artifacts dir");
    std::fs::write(project_dir.join("packages.lock"), "").expect("create packages.lock");

    let broadcaster = Arc::new(EventBroadcaster::new());
    let events = Arc::new(LocalProjectEventService::new(db.clone(), broadcaster));
    let service = ApplicationService::new(db, events, dir.path());

    let result = service
        .execute(ApplicationCommand::ValidateProject(
            ValidateProjectRequest {
                project: project_id,
            },
        ))
        .expect("validate project should succeed");

    match result {
        CommandResult::ProjectValidated(resp) => match resp.result {
            ValidationResult::Passed(_) => {}
            ValidationResult::Failed(report) => {
                panic!(
                    "validation should pass, got failures: {:?}",
                    report.findings
                );
            }
        },
        _ => panic!("expected ProjectValidated"),
    }
}

#[test]
fn reopening_migrated_project_works() {
    let (dir, source) = temp_source();
    let dest = temp_dest(&dir);

    MigrationService::new()
        .migrate_from_v1(&source, &dest)
        .expect("migration should succeed");

    let project_dir = dir.path().join("project.auto-re");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    let db_path = project_dir.join("project.sqlite3");
    std::fs::copy(&dest, &db_path).expect("copy migrated db to project dir");

    let db = Database::open(&db_path).expect("open project db");
    let project = Project::new("fixture-reopen");
    let manifest = ProjectManifest::new(project.clone(), project_dir.join("project.toml"));
    SqliteProjectStore::new(&db)
        .insert_project(&project)
        .expect("insert project");
    drop(db);

    manifest.save(&manifest.path).expect("save manifest");
    std::fs::create_dir_all(project_dir.join("artifacts")).expect("create artifacts dir");
    std::fs::write(project_dir.join("packages.lock"), "").expect("create packages.lock");

    let reopened = open_project(dir.path()).expect("reopen migrated project");
    assert_eq!(reopened.id, project.id);
    assert_eq!(reopened.name, project.name);
}
