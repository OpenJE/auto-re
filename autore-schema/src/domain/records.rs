//! Stage 0 record types — Project, Artifact, and supporting types per §7 + §8.
//!
//! These are the top-level domain records that persistence layers store and
//! query. Artifact kinds are registered as `NamespacedId` constants (NOT enum
//! variants) to allow runtime extensibility.

use std::path::PathBuf;

use crate::domain::{ContentHash, MetadataMap, NamespacedId, SchemaVersion, Timestamp};
use crate::ids::{ArtifactId, ProjectId};

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// A project — the top-level workspace container for analysis.
///
/// Projects group artifacts, entities, evidence, hypotheses, and operations
/// under a single durable identity with a schema version for forward
/// migration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub schema_version: SchemaVersion,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub metadata: MetadataMap,
}

impl Project {
    /// Creates a new project with the given name, using the current schema
    /// version (2.0) and timestamp.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Timestamp::now();
        Project {
            id: ProjectId::new(),
            name: name.into(),
            schema_version: SchemaVersion::new(2, 0),
            created_at: now,
            updated_at: now,
            metadata: MetadataMap::new(),
        }
    }

    /// Bumps `updated_at` to the current time.
    pub fn touch(&mut self) {
        self.updated_at = Timestamp::now();
    }
}

// ---------------------------------------------------------------------------
// Endianness
// ---------------------------------------------------------------------------

/// Byte order of a binary artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Endianness {
    Little,
    Big,
}

// ---------------------------------------------------------------------------
// BinaryArtifactMetadata
// ---------------------------------------------------------------------------

/// Metadata specific to binary artifacts (executables, shared libraries,
/// firmware images).
///
/// Stage 0 stores these fields but does NOT inspect or parse the binary
/// content (§8: "Stage 0 does not inspect or parse the binary").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BinaryArtifactMetadata {
    pub format: Option<NamespacedId>,
    pub architecture: Option<NamespacedId>,
    pub endianness: Option<Endianness>,
    pub preferred_image_base: Option<u64>,
}

// ---------------------------------------------------------------------------
// ArtifactStorage
// ---------------------------------------------------------------------------

/// How an artifact's content is physically stored.
///
/// Extensible via serde adjacently-tagged enum (`#[serde(tag, content)]`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ArtifactStorage {
    /// A blob managed by the auto-re storage layer (copied into the project
    /// directory tree).
    ManagedBlob { relative_path: PathBuf },
    /// A file outside the project directory, referenced by its canonical path.
    ExternalFile { canonical_path: PathBuf },
}

// ---------------------------------------------------------------------------
// Artifact
// ---------------------------------------------------------------------------

/// An immutable, content-addressed artifact associated with a project.
///
/// Artifacts are identified by their `ContentHash` (SHA-256 by default).
/// Managed artifacts are copied into the project's storage directory;
/// external artifacts are referenced by path and verified on demand.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub project: ProjectId,
    pub kind: NamespacedId,
    pub content_hash: ContentHash,
    pub size: u64,
    pub storage: ArtifactStorage,
    pub created_at: Timestamp,
    pub metadata: MetadataMap,
}

// ---------------------------------------------------------------------------
// Artifact kind constants (§8)
// ---------------------------------------------------------------------------

/// Artifact kind: a binary file (executable, shared library, firmware image).
pub static ARTIFACT_KIND_BINARY: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.binary").unwrap());

/// Artifact kind: a source tree (source files, patches, translation units).
pub static ARTIFACT_KIND_SOURCE_TREE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.source-tree").unwrap());

/// Artifact kind: output from a native analysis provider.
pub static ARTIFACT_KIND_NATIVE_PROVIDER_OUTPUT: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.native-provider-output").unwrap());

/// Artifact kind: a configuration file.
pub static ARTIFACT_KIND_CONFIGURATION: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.configuration").unwrap());

/// Artifact kind: a log file.
pub static ARTIFACT_KIND_LOG: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.log").unwrap());

/// Artifact kind: an execution trace.
pub static ARTIFACT_KIND_TRACE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.trace").unwrap());

/// Artifact kind: a generated candidate (e.g., proposed source code).
pub static ARTIFACT_KIND_GENERATED_CANDIDATE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.generated-candidate").unwrap());

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ExtensionData, MetadataMap, NamespacedId};
    use crate::ids::ArtifactId;
    use std::path::PathBuf;

    fn sample_project() -> Project {
        Project::new("test-project")
    }

    fn sample_artifact(project: &Project) -> Artifact {
        Artifact {
            id: ArtifactId::new(),
            project: project.id,
            kind: ARTIFACT_KIND_BINARY.clone(),
            content_hash: ContentHash::sha256(b"hello world"),
            size: 11,
            storage: ArtifactStorage::ManagedBlob {
                relative_path: PathBuf::from("sha256/ab/cdef0123"),
            },
            created_at: Timestamp::now(),
            metadata: MetadataMap::new(),
        }
    }

    #[test]
    fn project_new_sets_name_and_timestamps() {
        let p = Project::new("my-project");
        assert_eq!(p.name, "my-project");
        assert_eq!(p.schema_version, SchemaVersion::new(2, 0));
        assert!(p.metadata.is_empty());
        assert_eq!(p.created_at, p.updated_at);
    }

    #[test]
    fn project_touch_updates_updated_at() {
        let mut p = Project::new("touch-test");
        let original = *p.updated_at.as_offset_datetime();
        std::thread::sleep(std::time::Duration::from_millis(5));
        p.touch();
        assert!(p.updated_at.as_offset_datetime() > &original);
    }

    #[test]
    fn project_round_trip_json() {
        let p = sample_project();
        let json = serde_json::to_string_pretty(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(p.id, back.id);
        assert_eq!(p.name, back.name);
        assert_eq!(p.schema_version, back.schema_version);
        assert_eq!(p.created_at, back.created_at);
        assert_eq!(p.metadata, back.metadata);
    }

    #[test]
    fn project_metadata_is_typed_not_raw_json() {
        let mut p = sample_project();
        let schema = NamespacedId::parse("core.test").unwrap();
        let ext = ExtensionData::new(schema.clone(), 1, serde_json::json!({"key": "value"}));
        p.metadata.insert(schema, ext);
        assert_eq!(p.metadata.len(), 1);

        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.metadata.len(), 1);
    }

    #[test]
    fn artifact_round_trip_managed() {
        let p = sample_project();
        let a = sample_artifact(&p);
        let json = serde_json::to_string_pretty(&a).unwrap();
        let back: Artifact = serde_json::from_str(&json).unwrap();
        assert_eq!(a.id, back.id);
        assert_eq!(a.project, back.project);
        assert_eq!(a.kind, back.kind);
        assert_eq!(a.content_hash, back.content_hash);
        assert_eq!(a.size, back.size);
        assert_eq!(a.storage, back.storage);
    }

    #[test]
    fn artifact_round_trip_external() {
        let p = sample_project();
        let a = Artifact {
            id: ArtifactId::new(),
            project: p.id,
            kind: ARTIFACT_KIND_SOURCE_TREE.clone(),
            content_hash: ContentHash::sha256(b"external content"),
            size: 1024,
            storage: ArtifactStorage::ExternalFile {
                canonical_path: PathBuf::from("/usr/lib/libc.so.6"),
            },
            created_at: Timestamp::now(),
            metadata: MetadataMap::new(),
        };
        let json = serde_json::to_string_pretty(&a).unwrap();
        let back: Artifact = serde_json::from_str(&json).unwrap();
        assert_eq!(a.storage, back.storage);
    }

    #[test]
    fn artifact_kinds_registered() {
        assert_eq!(ARTIFACT_KIND_BINARY.to_string(), "core.binary");
        assert_eq!(ARTIFACT_KIND_SOURCE_TREE.to_string(), "core.source-tree");
        assert_eq!(
            ARTIFACT_KIND_NATIVE_PROVIDER_OUTPUT.to_string(),
            "core.native-provider-output"
        );
        assert_eq!(ARTIFACT_KIND_CONFIGURATION.to_string(), "core.configuration");
        assert_eq!(ARTIFACT_KIND_LOG.to_string(), "core.log");
        assert_eq!(ARTIFACT_KIND_TRACE.to_string(), "core.trace");
        assert_eq!(
            ARTIFACT_KIND_GENERATED_CANDIDATE.to_string(),
            "core.generated-candidate"
        );
    }

    #[test]
    fn binary_artifact_metadata_round_trip() {
        let meta = BinaryArtifactMetadata {
            format: Some(NamespacedId::parse("core.elf").unwrap()),
            architecture: Some(NamespacedId::parse("core.x86-64").unwrap()),
            endianness: Some(Endianness::Little),
            preferred_image_base: Some(0x400000),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: BinaryArtifactMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn binary_artifact_metadata_all_none() {
        let meta = BinaryArtifactMetadata {
            format: None,
            architecture: None,
            endianness: None,
            preferred_image_base: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: BinaryArtifactMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn endianness_variants_serialize() {
        let le = serde_json::to_string(&Endianness::Little).unwrap();
        let be = serde_json::to_string(&Endianness::Big).unwrap();
        assert_eq!(le, "\"Little\"");
        assert_eq!(be, "\"Big\"");
        let le_back: Endianness = serde_json::from_str(&le).unwrap();
        assert_eq!(le_back, Endianness::Little);
    }

    // Fixture round-trip tests — enabled after fixtures are generated.
    #[test]
    fn project_fixture_round_trip() {
        let fixture = include_str!("../../tests/fixtures/project.json");
        let p: Project = serde_json::from_str(fixture).unwrap();
        let re_serialized = serde_json::to_string_pretty(&p).unwrap();
        assert_eq!(fixture.trim(), re_serialized.trim());
    }

    #[test]
    fn artifact_fixture_managed_round_trip() {
        let fixture = include_str!("../../tests/fixtures/artifact_managed.json");
        let a: Artifact = serde_json::from_str(fixture).unwrap();
        let re_serialized = serde_json::to_string_pretty(&a).unwrap();
        assert_eq!(fixture.trim(), re_serialized.trim());
    }

    #[test]
    fn artifact_fixture_external_round_trip() {
        let fixture = include_str!("../../tests/fixtures/artifact_external.json");
        let a: Artifact = serde_json::from_str(fixture).unwrap();
        let re_serialized = serde_json::to_string_pretty(&a).unwrap();
        assert_eq!(fixture.trim(), re_serialized.trim());
    }

    /// Helper to generate fixture JSON — run once, capture output, commit fixtures.
    #[test]
    fn generate_fixtures() {
        use crate::ids::ProjectId;
        use uuid::Uuid;

        let project_uuid = Uuid::parse_str("01906789-abcd-7000-8000-000000000001").unwrap();
        let artifact_uuid = Uuid::parse_str("01906789-abcd-7000-8000-000000000002").unwrap();
        let project_id = ProjectId::from_uuid(project_uuid);
        let artifact_id = ArtifactId::from_uuid(artifact_uuid);

        let ts = Timestamp::from_offset_datetime(
            time::OffsetDateTime::parse(
                "2026-01-15T10:30:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        );

        let project = Project {
            id: project_id,
            name: "fixture-project".into(),
            schema_version: SchemaVersion::new(2, 0),
            created_at: ts,
            updated_at: ts,
            metadata: MetadataMap::new(),
        };
        let project_json = serde_json::to_string_pretty(&project).unwrap();
        eprintln!("=== project.json ===\n{project_json}\n=== end ===");

        let artifact_managed = Artifact {
            id: artifact_id,
            project: project_id,
            kind: ARTIFACT_KIND_BINARY.clone(),
            content_hash: ContentHash::sha256(b"fixture binary content"),
            size: 2048,
            storage: ArtifactStorage::ManagedBlob {
                relative_path: PathBuf::from("sha256/b9/4d27b993456789abcdef0123456789abcdef0123456789abcdef01234567"),
            },
            created_at: ts,
            metadata: MetadataMap::new(),
        };
        let managed_json = serde_json::to_string_pretty(&artifact_managed).unwrap();
        eprintln!("=== artifact_managed.json ===\n{managed_json}\n=== end ===");

        let artifact_external = Artifact {
            id: ArtifactId::from_uuid(Uuid::parse_str("01906789-abcd-7000-8000-000000000003").unwrap()),
            project: project_id,
            kind: ARTIFACT_KIND_SOURCE_TREE.clone(),
            content_hash: ContentHash::sha256(b"external source tree"),
            size: 4096,
            storage: ArtifactStorage::ExternalFile {
                canonical_path: PathBuf::from("/home/user/projects/target-binary"),
            },
            created_at: ts,
            metadata: MetadataMap::new(),
        };
        let external_json = serde_json::to_string_pretty(&artifact_external).unwrap();
        eprintln!("=== artifact_external.json ===\n{external_json}\n=== end ===");
    }
}
