//! Content-addressed artifact storage with managed copy-in and external references.
//!
//! `ArtifactStore` provides two registration modes:
//! - **Managed**: copies the source file into `<project_dir>/artifacts/<algo>/<prefix>/<digest>`,
//!   deduplicating by content hash.
//! - **External**: records hash and size without copying; integrity verified on demand.
//!
//! SHA-256 is the default hash algorithm. BLAKE3 is used when explicitly requested
//! (e.g., when the source already has a known BLAKE3 hash from V1).

use std::path::{Path, PathBuf};

use autore_schema::domain::{
    Artifact, ArtifactStorage, ContentHash, HashAlgorithm, MetadataMap, NamespacedId, Timestamp,
};
use autore_schema::ids::{ArtifactId, ProjectId};

use crate::storage::database::Database;

// ---------------------------------------------------------------------------
// ArtifactIntegrity
// ---------------------------------------------------------------------------

/// Result of verifying an artifact's content integrity.
#[derive(Debug)]
pub struct ArtifactIntegrity {
    /// The hash recorded at registration time.
    pub expected_hash: ContentHash,
    /// The hash recomputed from the current file content.
    pub actual_hash: ContentHash,
    /// `true` when the hashes match and the artifact is intact.
    pub is_valid: bool,
}

// ---------------------------------------------------------------------------
// ArtifactStore trait
// ---------------------------------------------------------------------------

/// Storage operations for content-addressed artifacts.
pub trait ArtifactStore: Send + Sync {
    /// Copies `source_path` into managed content-addressed storage under the
    /// project directory. Uses SHA-256 by default. If a blob with the same
    /// hash already exists on disk, the copy is skipped (dedup).
    fn register_managed(
        &self,
        project_id: ProjectId,
        source_path: &Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact>;

    /// Lists all artifacts registered for a project.
    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Artifact>>;

    /// Like [`register_managed`] but uses BLAKE3 hashing. Call this when the
    /// source already has a known BLAKE3 hash from V1.
    fn register_managed_blake3(
        &self,
        project_id: ProjectId,
        source_path: &Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact>;

    /// Records an external file's hash and size **without** copying.
    /// The registered hash is immutable — it is never silently updated even
    /// if the external file changes.
    fn register_external(
        &self,
        project_id: ProjectId,
        canonical_path: &Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact>;

    /// Recomputes the hash of a managed or external artifact and compares it
    /// to the stored value. Returns `Ok(ArtifactIntegrity)` with the details,
    /// `Err(NotFound)` if the file is missing, or `Err(HashMismatch)` when
    /// the content has changed.
    fn verify_artifact(
        &self,
        project_id: ProjectId,
        artifact: &Artifact,
    ) -> crate::Result<ArtifactIntegrity>;

    /// Reads the raw bytes of a managed blob from disk.
    fn read_managed_blob(
        &self,
        project_id: ProjectId,
        artifact: &Artifact,
    ) -> crate::Result<Vec<u8>>;

    /// Retrieves a persisted artifact by ID.
    fn get_artifact(&self, id: ArtifactId) -> crate::Result<Option<Artifact>>;
}

// ---------------------------------------------------------------------------
// SqliteArtifactStore
// ---------------------------------------------------------------------------

/// SQLite-backed implementation of [`ArtifactStore`].
///
/// Artifact metadata is persisted in the `stage0_artifacts` table. Managed
/// blobs are written to `<base_dir>/<project_id>/artifacts/<algo>/<prefix>/<digest>`.
pub struct SqliteArtifactStore<'a> {
    db: &'a Database,
    base_dir: PathBuf,
}

impl<'a> SqliteArtifactStore<'a> {
    pub fn new(db: &'a Database, base_dir: impl Into<PathBuf>) -> Self {
        SqliteArtifactStore {
            db,
            base_dir: base_dir.into(),
        }
    }

    fn project_dir(&self, project_id: ProjectId) -> PathBuf {
        self.base_dir.join(project_id.to_string())
    }

    fn register_with_algo(
        &self,
        project_id: ProjectId,
        source_path: &Path,
        kind: NamespacedId,
        algo: HashAlgorithm,
    ) -> crate::Result<Artifact> {
        let data = std::fs::read(source_path)?;
        let size = data.len() as u64;
        let hash = compute_hash(&data, algo);
        let digest_hex = hash.digest_hex();

        let project_dir = self.project_dir(project_id);
        let blob_path = managed_blob_path(&project_dir, algo, &digest_hex);

        // Dedup: skip copy when the blob already exists on disk.
        if !blob_path.exists() {
            if let Some(parent) = blob_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&blob_path, &data)?;
        }

        let relative = blob_path
            .strip_prefix(&project_dir)
            .unwrap_or(&blob_path)
            .to_path_buf();

        let artifact = Artifact {
            id: ArtifactId::new(),
            project: project_id,
            kind,
            content_hash: hash,
            size,
            storage: ArtifactStorage::ManagedBlob {
                relative_path: relative,
            },
            created_at: Timestamp::now(),
            metadata: MetadataMap::new(),
        };

        self.insert_artifact(&artifact)?;
        Ok(artifact)
    }

    fn insert_artifact(&self, a: &Artifact) -> crate::Result<()> {
        let id_bytes = a.id.as_uuid().as_bytes().to_vec();
        let project_bytes = a.project.as_uuid().as_bytes().to_vec();
        let kind = a.kind.to_string();
        let algo = a.content_hash.algorithm.to_string();
        let digest = a.content_hash.digest.clone();
        let size = a.size as i64;
        let (storage_kind, storage_path) = match &a.storage {
            ArtifactStorage::ManagedBlob { relative_path } => (
                "managed".to_string(),
                relative_path.to_string_lossy().to_string(),
            ),
            ArtifactStorage::ExternalFile { canonical_path } => (
                "external".to_string(),
                canonical_path.to_string_lossy().to_string(),
            ),
        };
        let created_at = a.created_at.to_string();
        let metadata = serde_json::to_string(&a.metadata)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO stage0_artifacts \
             (id, project_id, kind, hash_algorithm, hash_digest, size, \
              storage_kind, storage_path, created_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                kind,
                algo,
                digest,
                size,
                storage_kind,
                storage_path,
                created_at,
                metadata,
            ],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(())
    }
}

impl ArtifactStore for SqliteArtifactStore<'_> {
    fn register_managed(
        &self,
        project_id: ProjectId,
        source_path: &Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact> {
        self.register_with_algo(project_id, source_path, kind, HashAlgorithm::Sha256)
    }

    fn register_managed_blake3(
        &self,
        project_id: ProjectId,
        source_path: &Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact> {
        self.register_with_algo(project_id, source_path, kind, HashAlgorithm::Blake3)
    }

    fn register_external(
        &self,
        project_id: ProjectId,
        canonical_path: &Path,
        kind: NamespacedId,
    ) -> crate::Result<Artifact> {
        let data = std::fs::read(canonical_path)?;
        let size = data.len() as u64;
        let hash = compute_hash(&data, HashAlgorithm::Sha256);

        let artifact = Artifact {
            id: ArtifactId::new(),
            project: project_id,
            kind,
            content_hash: hash,
            size,
            storage: ArtifactStorage::ExternalFile {
                canonical_path: canonical_path.to_path_buf(),
            },
            created_at: Timestamp::now(),
            metadata: MetadataMap::new(),
        };

        self.insert_artifact(&artifact)?;
        Ok(artifact)
    }

    fn verify_artifact(
        &self,
        project_id: ProjectId,
        artifact: &Artifact,
    ) -> crate::Result<ArtifactIntegrity> {
        let file_path = match &artifact.storage {
            ArtifactStorage::ManagedBlob { relative_path } => {
                self.project_dir(project_id).join(relative_path)
            }
            ArtifactStorage::ExternalFile { canonical_path } => canonical_path.clone(),
        };

        if !file_path.exists() {
            return Err(crate::Error::NotFound(format!(
                "artifact file not found: {}",
                file_path.display()
            )));
        }

        let data = std::fs::read(&file_path)?;
        let actual_hash = compute_hash(&data, artifact.content_hash.algorithm);
        let expected_hash = artifact.content_hash.clone();
        let is_valid = actual_hash.digest == expected_hash.digest;

        if !is_valid {
            return Err(crate::Error::HashMismatch);
        }

        Ok(ArtifactIntegrity {
            expected_hash,
            actual_hash,
            is_valid,
        })
    }

    fn read_managed_blob(
        &self,
        project_id: ProjectId,
        artifact: &Artifact,
    ) -> crate::Result<Vec<u8>> {
        match &artifact.storage {
            ArtifactStorage::ManagedBlob { relative_path } => {
                let path = self.project_dir(project_id).join(relative_path);
                std::fs::read(&path).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        crate::Error::NotFound(format!(
                            "managed blob not found: {}",
                            path.display()
                        ))
                    } else {
                        crate::Error::Io(e)
                    }
                })
            }
            ArtifactStorage::ExternalFile { .. } => Err(crate::Error::Validation(
                "read_managed_blob called on external artifact".into(),
            )),
        }
    }

    fn get_artifact(&self, id: ArtifactId) -> crate::Result<Option<Artifact>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;
        let result = conn.query_row(
            "SELECT id, project_id, kind, hash_algorithm, hash_digest, size, \
             storage_kind, storage_path, created_at, metadata \
             FROM stage0_artifacts WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_artifact,
        );
        match result {
            Ok(artifact) => Ok(Some(artifact)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Artifact>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, kind, hash_algorithm, hash_digest, size, \
                 storage_kind, storage_path, created_at, metadata \
                 FROM stage0_artifacts \
                 WHERE project_id = ?1 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        let artifacts = stmt
            .query_map(rusqlite::params![project_bytes], row_to_artifact)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        Ok(artifacts)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn compute_hash(data: &[u8], algo: HashAlgorithm) -> ContentHash {
    match algo {
        HashAlgorithm::Sha256 => ContentHash::sha256(data),
        HashAlgorithm::Blake3 => ContentHash::blake3(data),
    }
}

fn managed_blob_path(project_dir: &Path, algo: HashAlgorithm, digest_hex: &str) -> PathBuf {
    let prefix = &digest_hex[..2];
    project_dir
        .join("artifacts")
        .join(algo.to_string())
        .join(prefix)
        .join(digest_hex)
}

fn row_to_artifact(row: &rusqlite::Row<'_>) -> rusqlite::Result<Artifact> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let kind_str: String = row.get(2)?;
    let algo_str: String = row.get(3)?;
    let digest: Vec<u8> = row.get(4)?;
    let size: i64 = row.get(5)?;
    let storage_kind: String = row.get(6)?;
    let storage_path: String = row.get(7)?;
    let created_at_str: String = row.get(8)?;
    let metadata_str: String = row.get(9)?;

    let id_uuid = uuid::Uuid::from_slice(&id_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
    })?;
    let project_uuid = uuid::Uuid::from_slice(&project_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e))
    })?;

    let kind = NamespacedId::parse(&kind_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let algorithm = match algo_str.as_str() {
        "sha256" => HashAlgorithm::Sha256,
        "blake3" => HashAlgorithm::Blake3,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(ParseError(format!("unknown hash algorithm: {other}"))),
            ));
        }
    };

    let content_hash = ContentHash { algorithm, digest };

    let storage = match storage_kind.as_str() {
        "managed" => ArtifactStorage::ManagedBlob {
            relative_path: PathBuf::from(&storage_path),
        },
        "external" => ArtifactStorage::ExternalFile {
            canonical_path: PathBuf::from(&storage_path),
        },
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(ParseError(format!("unknown storage kind: {other}"))),
            ));
        }
    };

    let created_at = parse_timestamp(&created_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            8,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    let metadata: MetadataMap = serde_json::from_str(&metadata_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e))
    })?;

    Ok(Artifact {
        id: ArtifactId::from_uuid(id_uuid),
        project: ProjectId::from_uuid(project_uuid),
        kind,
        content_hash,
        size: size as u64,
        storage,
        created_at,
        metadata,
    })
}

fn parse_timestamp(s: &str) -> Result<Timestamp, String> {
    let dt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| format!("invalid timestamp: {e}"))?;
    Ok(Timestamp::from_offset_datetime(dt))
}

#[derive(Debug)]
struct ParseError(String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::project_store::{ProjectStore, SqliteProjectStore};
    use autore_schema::domain::ARTIFACT_KIND_BINARY;
    use autore_schema::domain::records::Project;

    fn setup() -> (tempfile::TempDir, Database) {
        let tmp = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        (tmp, db)
    }

    fn setup_with_project() -> (tempfile::TempDir, Database, ProjectId) {
        let (tmp, db) = setup();
        let project = Project::new("test-project");
        let pid = project.id;
        let store = SqliteProjectStore::new(&db);
        store.insert_project(&project).unwrap();
        (tmp, db, pid)
    }

    fn create_source_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    // -- managed_artifact_import --

    #[test]
    fn managed_artifact_import() {
        let (tmp, db, pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());

        let source = create_source_file(tmp.path(), "input.bin", b"hello world");
        let artifact = store
            .register_managed(pid, &source, ARTIFACT_KIND_BINARY.clone())
            .unwrap();

        // Verify hash algorithm and digest.
        assert_eq!(artifact.content_hash.algorithm, HashAlgorithm::Sha256);
        let expected_hash = ContentHash::sha256(b"hello world");
        assert_eq!(artifact.content_hash.digest, expected_hash.digest);
        assert_eq!(artifact.size, 11);

        // Verify managed blob exists at the correct path.
        let project_dir = tmp.path().join(pid.to_string());
        let digest_hex = artifact.content_hash.digest_hex();
        let prefix = &digest_hex[..2];
        let blob_path = project_dir
            .join("artifacts")
            .join("sha256")
            .join(prefix)
            .join(&digest_hex);
        assert!(blob_path.exists(), "blob should exist at {blob_path:?}");

        // Verify blob content matches source.
        let blob_data = std::fs::read(&blob_path).unwrap();
        assert_eq!(blob_data, b"hello world");

        // Verify storage is ManagedBlob with relative path.
        match &artifact.storage {
            ArtifactStorage::ManagedBlob { relative_path } => {
                assert!(relative_path.starts_with("artifacts/sha256/"));
            }
            _ => panic!("expected ManagedBlob storage"),
        }

        // Verify DB round-trip.
        let conn = db.connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stage0_artifacts WHERE id = ?1",
                rusqlite::params![artifact.id.as_uuid().as_bytes().as_slice()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "artifact should be persisted in DB");
    }

    // -- managed_artifact_dedup --

    #[test]
    fn managed_artifact_dedup() {
        let (tmp, db, pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());

        let source1 = create_source_file(tmp.path(), "a.bin", b"same content");
        let source2 = create_source_file(tmp.path(), "b.bin", b"same content");

        let a1 = store
            .register_managed(pid, &source1, ARTIFACT_KIND_BINARY.clone())
            .unwrap();
        let a2 = store
            .register_managed(pid, &source2, ARTIFACT_KIND_BINARY.clone())
            .unwrap();

        // Different artifact IDs.
        assert_ne!(a1.id, a2.id);

        // Same hash.
        assert_eq!(a1.content_hash, a2.content_hash);

        // Same blob path — the file should only exist once on disk.
        match (&a1.storage, &a2.storage) {
            (
                ArtifactStorage::ManagedBlob { relative_path: p1 },
                ArtifactStorage::ManagedBlob { relative_path: p2 },
            ) => {
                assert_eq!(p1, p2, "dedup should reuse the same blob path");
            }
            _ => panic!("both should be ManagedBlob"),
        }

        // Two rows in DB.
        let conn = db.connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stage0_artifacts WHERE project_id = ?1",
                rusqlite::params![pid.as_uuid().as_bytes().as_slice()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "both registrations should be in DB");
    }

    // -- managed_artifact_hash_matches_storage --

    #[test]
    fn managed_artifact_hash_matches_storage() {
        let (tmp, db, pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());

        let content = b"hash verification test data";
        let source = create_source_file(tmp.path(), "verify.bin", content);
        let artifact = store
            .register_managed(pid, &source, ARTIFACT_KIND_BINARY.clone())
            .unwrap();

        // Read back the blob and verify the hash matches the recorded content_hash.
        let blob_data = store.read_managed_blob(pid, &artifact).unwrap();
        assert_eq!(blob_data, content);

        let recomputed = ContentHash::sha256(&blob_data);
        assert_eq!(
            recomputed.digest, artifact.content_hash.digest,
            "blob on disk must match the recorded hash"
        );

        // Also verify via verify_artifact.
        let integrity = store.verify_artifact(pid, &artifact).unwrap();
        assert!(integrity.is_valid);
        assert_eq!(integrity.expected_hash, artifact.content_hash);
    }

    // -- external_artifact_hash_recorded --

    #[test]
    fn external_artifact_hash_recorded() {
        let (tmp, db, pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());

        let ext_content = b"external file content";
        let ext_path = create_source_file(tmp.path(), "external.bin", ext_content);

        let artifact = store
            .register_external(pid, &ext_path, ARTIFACT_KIND_BINARY.clone())
            .unwrap();

        // Verify hash matches.
        let expected = ContentHash::sha256(ext_content);
        assert_eq!(artifact.content_hash.digest, expected.digest);
        assert_eq!(artifact.content_hash.algorithm, HashAlgorithm::Sha256);
        assert_eq!(artifact.size, ext_content.len() as u64);

        // Verify storage is ExternalFile.
        match &artifact.storage {
            ArtifactStorage::ExternalFile { canonical_path } => {
                assert_eq!(canonical_path, &ext_path);
            }
            _ => panic!("expected ExternalFile storage"),
        }

        // Verify no blob was copied into managed storage.
        let project_dir = tmp.path().join(pid.to_string());
        let artifacts_dir = project_dir.join("artifacts");
        assert!(
            !artifacts_dir.exists(),
            "no managed artifacts directory should be created for external registration"
        );

        // Verify DB persistence.
        let conn = db.connection().unwrap();
        let storage_kind: String = conn
            .query_row(
                "SELECT storage_kind FROM stage0_artifacts WHERE id = ?1",
                rusqlite::params![artifact.id.as_uuid().as_bytes().as_slice()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(storage_kind, "external");
    }

    // -- external_artifact_change_detected --

    #[test]
    fn external_artifact_change_detected() {
        let (tmp, db, pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());

        let ext_path = create_source_file(tmp.path(), "mutable.bin", b"original content");
        let artifact = store
            .register_external(pid, &ext_path, ARTIFACT_KIND_BINARY.clone())
            .unwrap();

        // Verify before modification — should pass.
        let integrity = store.verify_artifact(pid, &artifact).unwrap();
        assert!(integrity.is_valid);

        // Modify the external file.
        std::fs::write(&ext_path, b"tampered content").unwrap();

        // Verify after modification — the hash should NOT match.
        // The registered hash must NOT be silently updated.
        let result = store.verify_artifact(pid, &artifact);
        assert!(result.is_err(), "modified external file must be detected");
        match result.unwrap_err() {
            crate::Error::HashMismatch => {}
            other => panic!("expected HashMismatch, got: {other:?}"),
        }
    }

    // -- external_artifact_missing_detected --

    #[test]
    fn external_artifact_missing_detected() {
        let (tmp, db, pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());

        let ext_path = create_source_file(tmp.path(), "removable.bin", b"ephemeral");
        let artifact = store
            .register_external(pid, &ext_path, ARTIFACT_KIND_BINARY.clone())
            .unwrap();

        // Delete the external file.
        std::fs::remove_file(&ext_path).unwrap();

        // Verify should report NotFound.
        let result = store.verify_artifact(pid, &artifact);
        assert!(result.is_err(), "missing external file must be detected");
        match result.unwrap_err() {
            crate::Error::NotFound(msg) => {
                assert!(
                    msg.contains("artifact file not found"),
                    "error should mention artifact file: {msg}"
                );
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    // -- Additional: BLAKE3 managed registration --

    #[test]
    fn managed_artifact_blake3_registration() {
        let (tmp, db, pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());

        let source = create_source_file(tmp.path(), "blake3.bin", b"blake3 test");
        let artifact = store
            .register_managed_blake3(pid, &source, ARTIFACT_KIND_BINARY.clone())
            .unwrap();

        assert_eq!(artifact.content_hash.algorithm, HashAlgorithm::Blake3);
        let expected = ContentHash::blake3(b"blake3 test");
        assert_eq!(artifact.content_hash.digest, expected.digest);

        // Verify blob is stored under blake3 directory.
        let project_dir = tmp.path().join(pid.to_string());
        let digest_hex = artifact.content_hash.digest_hex();
        let prefix = &digest_hex[..2];
        let blob_path = project_dir
            .join("artifacts")
            .join("blake3")
            .join(prefix)
            .join(&digest_hex);
        assert!(
            blob_path.exists(),
            "BLAKE3 blob should exist at {blob_path:?}"
        );
    }

    // -- Additional: trait object safety --

    #[test]
    fn artifact_store_trait_object() {
        let (tmp, db, _pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());
        fn _assert_trait_object(_: &dyn ArtifactStore) {}
        _assert_trait_object(&store);
    }

    #[test]
    fn get_artifact_round_trip() {
        let (tmp, db, pid) = setup_with_project();
        let store = SqliteArtifactStore::new(&db, tmp.path());

        let source = create_source_file(tmp.path(), "roundtrip.bin", b"round trip data");
        let artifact = store
            .register_managed(pid, &source, ARTIFACT_KIND_BINARY.clone())
            .unwrap();

        let fetched = store.get_artifact(artifact.id).unwrap().unwrap();
        assert_eq!(fetched.id, artifact.id);
        assert_eq!(fetched.project, pid);
        assert_eq!(fetched.content_hash, artifact.content_hash);
        assert_eq!(fetched.size, artifact.size);
        assert_eq!(fetched.storage, artifact.storage);
        assert_eq!(fetched.kind, artifact.kind);
    }
}
