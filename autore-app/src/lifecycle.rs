//! Project lifecycle management: create, open, close.
//!
//! This module provides the minimal lifecycle operations for auto-re projects.
//! A project consists of a directory layout with a manifest file, SQLite database,
//! artifacts directory, and a packages lock file.

use std::path::Path;

use autore_core::{Error, Result};
use autore_schema::domain::records::Project;
use autore_schema::manifest::ProjectManifest;
use autore_store::Database;

/// The project directory name within the parent directory.
const PROJECT_DIR_NAME: &str = "project.auto-re";

/// The manifest file name.
const MANIFEST_FILE_NAME: &str = "project.toml";

/// The SQLite database file name.
const DATABASE_FILE_NAME: &str = "project.sqlite3";

/// The artifacts directory name.
const ARTIFACTS_DIR_NAME: &str = "artifacts";

/// The packages lock file name.
const PACKAGES_LOCK_FILE_NAME: &str = "packages.lock";

/// Creates a new project in the given directory with the specified name.
///
/// # Layout
/// Creates the following structure under `<directory>/project.auto-re/`:
/// - `project.toml` — Project manifest (TOML)
/// - `project.sqlite3` — SQLite database with migrations applied
/// - `artifacts/` — Empty artifacts directory
/// - `packages.lock` — Empty packages lock stub
///
/// # Arguments
/// * `directory` — The parent directory where the project will be created
/// * `name` — The project name
///
/// # Returns
/// The created `Project` record.
///
/// # Errors
/// Returns an error if directory creation fails, manifest serialization fails,
/// or database initialization fails.
pub fn create_project(directory: impl AsRef<Path>, name: impl Into<String>) -> Result<Project> {
    let directory = directory.as_ref();
    let name = name.into();

    // Create the project directory structure
    let project_dir = directory.join(PROJECT_DIR_NAME);
    std::fs::create_dir_all(&project_dir)?;

    // Create the artifacts directory
    let artifacts_dir = project_dir.join(ARTIFACTS_DIR_NAME);
    std::fs::create_dir_all(&artifacts_dir)?;

    // Create the packages.lock stub file
    let packages_lock_path = project_dir.join(PACKAGES_LOCK_FILE_NAME);
    std::fs::write(&packages_lock_path, "")?;

    // Create the project record
    let project = Project::new(name);

    // Save the manifest
    let manifest_path = project_dir.join(MANIFEST_FILE_NAME);
    let manifest = ProjectManifest::new(project.clone(), manifest_path.clone());
    manifest.save(&manifest_path)?;

    // Initialize the database (applies migrations)
    let database_path = project_dir.join(DATABASE_FILE_NAME);
    let _db = Database::open(&database_path)?;

    Ok(project)
}

/// Opens an existing project from the given directory.
///
/// # Arguments
/// * `directory` — The parent directory containing the `project.auto-re/` subdirectory
///
/// # Returns
/// The loaded `Project` record.
///
/// # Errors
/// Returns an error if the manifest cannot be loaded, the schema version
/// doesn't match, or the database cannot be opened.
pub fn open_project(directory: impl AsRef<Path>) -> Result<Project> {
    let directory = directory.as_ref();
    let project_dir = directory.join(PROJECT_DIR_NAME);

    // Load the manifest
    let manifest_path = project_dir.join(MANIFEST_FILE_NAME);
    let manifest = ProjectManifest::load(&manifest_path)?;

    // Verify the schema version matches what we expect
    let expected_version = autore_schema::domain::SchemaVersion::new(2, 0);
    if manifest.schema_version != expected_version {
        return Err(Error::SchemaMismatch {
            expected: expected_version.to_string(),
            actual: manifest.schema_version.to_string(),
        });
    }

    // Open the database (verifies it exists and applies any pending migrations)
    let database_path = project_dir.join(DATABASE_FILE_NAME);
    let _db = Database::open(&database_path)?;

    Ok(manifest.project)
}

/// Closes a project, releasing database handles.
///
/// This is currently a no-op marker for lifecycle documentation.
/// The `Database` uses `Mutex<Connection>` which is dropped when it goes
/// out of scope, so explicit cleanup is not required.
///
/// # Arguments
/// * `_project` — The project to close (unused)
pub fn close_project(_project: &mut Project) {
    // No-op: Database handles are released when Database is dropped.
    // This function exists to document the lifecycle and provide a
    // future hook for explicit cleanup if needed.
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn project_create_layout() {
        let temp_dir = TempDir::new().unwrap();
        let project = create_project(temp_dir.path(), "test-project").unwrap();

        // Verify the project record
        assert_eq!(project.name, "test-project");
        assert_eq!(project.schema_version, autore_schema::domain::SchemaVersion::new(2, 0));

        // Verify the directory structure
        let project_dir = temp_dir.path().join(PROJECT_DIR_NAME);
        assert!(project_dir.exists(), "project.auto-re/ directory should exist");
        assert!(project_dir.is_dir(), "project.auto-re/ should be a directory");

        // Verify manifest file
        let manifest_path = project_dir.join(MANIFEST_FILE_NAME);
        assert!(manifest_path.exists(), "project.toml should exist");
        assert!(manifest_path.is_file(), "project.toml should be a file");

        // Verify database file
        let database_path = project_dir.join(DATABASE_FILE_NAME);
        assert!(database_path.exists(), "project.sqlite3 should exist");
        assert!(database_path.is_file(), "project.sqlite3 should be a file");

        // Verify artifacts directory
        let artifacts_dir = project_dir.join(ARTIFACTS_DIR_NAME);
        assert!(artifacts_dir.exists(), "artifacts/ directory should exist");
        assert!(artifacts_dir.is_dir(), "artifacts/ should be a directory");

        // Verify packages.lock file
        let packages_lock_path = project_dir.join(PACKAGES_LOCK_FILE_NAME);
        assert!(packages_lock_path.exists(), "packages.lock should exist");
        assert!(packages_lock_path.is_file(), "packages.lock should be a file");
    }

    #[test]
    fn project_reopen_roundtrips() {
        let temp_dir = TempDir::new().unwrap();

        // Create a project
        let original_project = create_project(temp_dir.path(), "roundtrip-test").unwrap();

        // Open the project
        let reopened_project = open_project(temp_dir.path()).unwrap();

        // Verify semantic equality
        assert_eq!(original_project.id, reopened_project.id);
        assert_eq!(original_project.name, reopened_project.name);
        assert_eq!(original_project.schema_version, reopened_project.schema_version);
        assert_eq!(original_project.created_at, reopened_project.created_at);
        // Note: updated_at may differ if touch() was called, but for a fresh project it should match
        assert_eq!(original_project.updated_at, reopened_project.updated_at);
    }

    #[test]
    fn project_manifest_records_schema_version() {
        let temp_dir = TempDir::new().unwrap();
        let project = create_project(temp_dir.path(), "schema-version-test").unwrap();

        // Load the manifest directly and verify schema_version
        let manifest_path = temp_dir.path().join(PROJECT_DIR_NAME).join(MANIFEST_FILE_NAME);
        let manifest = ProjectManifest::load(&manifest_path).unwrap();

        assert_eq!(manifest.schema_version, autore_schema::domain::SchemaVersion::new(2, 0));
        assert_eq!(manifest.project.schema_version, autore_schema::domain::SchemaVersion::new(2, 0));
        assert_eq!(manifest.project.id, project.id);
        assert_eq!(manifest.project.name, project.name);
    }

    #[test]
    fn project_close_is_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let mut project = create_project(temp_dir.path(), "close-test").unwrap();

        // Close should not panic or error
        close_project(&mut project);

        // Can reopen after close
        let reopened = open_project(temp_dir.path()).unwrap();
        assert_eq!(reopened.id, project.id);
    }
}
