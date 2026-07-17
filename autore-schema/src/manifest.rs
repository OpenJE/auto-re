//! Project manifest — TOML-based project descriptor for on-disk project roots.
//!
//! The manifest (`project.toml`) is the entry point for opening an existing
//! project: it records the schema version, project identity, and timestamps.
//! Full project state (metadata, artifacts, entities) lives in the SQLite
//! database alongside the manifest.
//!
//! # Design note
//!
//! `ProjectManifest` lives in `autore-schema` (not `autore-core`) because it
//! references `Project` and other schema types. The dependency direction is
//! `autore-schema → autore-core`, so `autore-core` cannot depend on
//! `autore-schema` without creating a cycle.

use std::path::{Path, PathBuf};

use autore_core::{Error, Result};

use crate::domain::records::Project;
use crate::domain::{MetadataMap, SchemaVersion, Timestamp};
use crate::ids::ProjectId;

// ---------------------------------------------------------------------------
// ProjectManifest
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ProjectManifest {
    pub project: Project,
    pub path: PathBuf,
    pub schema_version: SchemaVersion,
}

/// Intermediate TOML representation — flat structure without nested metadata.
#[derive(serde::Serialize, serde::Deserialize)]
struct ManifestToml {
    schema_version: SchemaVersion,
    project_id: ProjectId,
    name: String,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl ProjectManifest {
    /// Creates a new manifest for the given project at the given path.
    pub fn new(project: Project, path: PathBuf) -> Self {
        let schema_version = project.schema_version;
        ProjectManifest {
            project,
            path,
            schema_version,
        }
    }

    /// Saves the manifest to the given path as TOML.
    pub fn save(&self, path: &Path) -> Result<()> {
        let toml_data = ManifestToml {
            schema_version: self.schema_version,
            project_id: self.project.id,
            name: self.project.name.clone(),
            created_at: self.project.created_at,
            updated_at: self.project.updated_at,
        };
        let toml_string =
            toml::to_string_pretty(&toml_data).map_err(|e| Error::Serialization(e.to_string()))?;
        std::fs::write(path, toml_string)?;
        Ok(())
    }

    /// Loads a manifest from the given TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let toml_data: ManifestToml =
            toml::from_str(&contents).map_err(|e| Error::Serialization(e.to_string()))?;

        let project = Project {
            id: toml_data.project_id,
            name: toml_data.name,
            schema_version: toml_data.schema_version,
            created_at: toml_data.created_at,
            updated_at: toml_data.updated_at,
            metadata: MetadataMap::new(),
        };

        Ok(ProjectManifest {
            schema_version: toml_data.schema_version,
            project,
            path: path.to_path_buf(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::records::Project;

    #[test]
    fn project_manifest_load_save() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.toml");

        let project = Project::new("test-project");
        let original_id = project.id;
        let manifest = ProjectManifest::new(project, manifest_path.clone());

        manifest.save(&manifest_path).unwrap();
        assert!(manifest_path.exists());

        let loaded = ProjectManifest::load(&manifest_path).unwrap();
        assert_eq!(loaded.project.id, original_id);
        assert_eq!(loaded.project.name, "test-project");
        assert_eq!(loaded.schema_version, SchemaVersion::new(2, 0));
        assert_eq!(loaded.path, manifest_path);
    }

    #[test]
    fn project_manifest_round_trip_preserves_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.toml");

        let project = Project::new("timestamp-test");
        let original_created = project.created_at;
        let original_updated = project.updated_at;

        let manifest = ProjectManifest::new(project, manifest_path.clone());
        manifest.save(&manifest_path).unwrap();

        let loaded = ProjectManifest::load(&manifest_path).unwrap();
        assert_eq!(loaded.project.created_at, original_created);
        assert_eq!(loaded.project.updated_at, original_updated);
    }

    #[test]
    fn project_manifest_load_missing_file() {
        let result = ProjectManifest::load(Path::new("/nonexistent/project.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn project_manifest_load_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("bad.toml");
        std::fs::write(&manifest_path, "this is not valid TOML {{{").unwrap();

        let result = ProjectManifest::load(&manifest_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::Serialization(_)));
    }

    #[test]
    fn project_manifest_fixture_round_trip() {
        let fixture = include_str!("../tests/fixtures/project_manifest.toml");
        let toml_data: ManifestToml = toml::from_str(fixture).unwrap();
        let re_serialized = toml::to_string_pretty(&toml_data).unwrap();
        assert_eq!(fixture.trim(), re_serialized.trim());
    }

    #[test]
    fn generate_manifest_fixture() {
        use uuid::Uuid;
        let project_uuid = Uuid::parse_str("01906789-abcd-7000-8000-000000000001").unwrap();
        let project_id = ProjectId::from_uuid(project_uuid);
        let ts = Timestamp::from_offset_datetime(
            time::OffsetDateTime::parse(
                "2026-01-15T10:30:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        );
        let toml_data = ManifestToml {
            schema_version: SchemaVersion::new(2, 0),
            project_id,
            name: "fixture-project".into(),
            created_at: ts,
            updated_at: ts,
        };
        let toml_string = toml::to_string_pretty(&toml_data).unwrap();
        eprintln!("=== project_manifest.toml ===\n{toml_string}=== end ===");
    }
}
