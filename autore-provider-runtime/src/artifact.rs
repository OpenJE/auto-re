//! Artifact transport abstraction: staging, committing, and reconciling provider-produced blobs.
//!
//! Providers write inbound artifacts to a scoped staging directory. The transport
//! independently recomputes BLAKE3 hashes on commit to prevent trusting
//! provider-supplied digests. Staging paths are instance-scoped so that
//! concurrent provider instances never collide.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use autore_schema::{ArtifactId, ContentHash, NamespacedId, ProviderInstanceId};
use bytes::Bytes;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors produced by artifact transport operations.
#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    /// An I/O error occurred during staging or commit.
    #[error("artifact I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The independently recomputed hash did not match the caller-supplied hash.
    #[error("content hash mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        /// The hash the caller claimed the content should have.
        expected: ContentHash,
        /// The hash the transport independently computed.
        actual: ContentHash,
    },

    /// The handle was already committed; duplicate commits are not allowed.
    #[error("artifact handle already committed")]
    AlreadyCommitted,

    /// The staging directory or data file was not found.
    #[error("artifact not found at staging path: {0}")]
    NotFound(PathBuf),

    /// The handle is invalid for this transport (e.g., wrong instance scope).
    #[error("invalid artifact handle: {0}")]
    InvalidHandle(String),

    /// An error during orphan sweep.
    #[error("orphan sweep error: {0}")]
    OrphanSweep(String),
}

// ---------------------------------------------------------------------------
// ArtifactLocation
// ---------------------------------------------------------------------------

/// Where a staged artifact's data can be read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactLocation {
    /// A local filesystem path (staging or canonical).
    Local(PathBuf),
    /// A remote URI (e.g., `s3://...`, `https://...`).
    Remote(String),
}

// ---------------------------------------------------------------------------
// ArtifactHandle
// ---------------------------------------------------------------------------

/// Opaque handle to a staged artifact.
///
/// Providers receive handles from `stage_inbound` and pass them to
/// `stage_outbound` or `commit_inbound`. The internal staging path is
/// instance-scoped and never exposed as a canonical artifact path.
pub struct ArtifactHandle {
    /// The path to the staging directory (`<root>/<instance_id>/<request_id>/<uuid>/`).
    staging_path: PathBuf,
    /// Unique artifact identifier within this staging session.
    artifact_uuid: Uuid,
    /// The provider instance that owns this staging slot.
    instance_id: ProviderInstanceId,
    /// Optional request correlation identifier.
    request_id: Option<String>,
    /// Whether this handle has been committed.
    committed: AtomicBool,
}

impl ArtifactHandle {
    /// Returns the staging directory path for inspection in tests.
    pub fn staging_path(&self) -> &Path {
        &self.staging_path
    }

    /// Returns the path to the staged data file.
    fn data_path(&self) -> PathBuf {
        self.staging_path.join("data")
    }

    /// Returns the artifact UUID.
    pub fn artifact_uuid(&self) -> Uuid {
        self.artifact_uuid
    }

    /// Returns the instance ID.
    pub fn instance_id(&self) -> &ProviderInstanceId {
        &self.instance_id
    }

    /// Returns the request ID, if any.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
}

// ---------------------------------------------------------------------------
// ArtifactTransport trait
// ---------------------------------------------------------------------------

/// Transport layer for staging, committing, and discarding provider artifacts.
pub trait ArtifactTransport: Send + Sync {
    /// Stage inbound bytes into a scoped staging directory and return a handle.
    fn stage_inbound(
        &self,
        bytes: Bytes,
    ) -> impl std::future::Future<Output = Result<ArtifactHandle, ArtifactError>> + Send;

    /// Return the location where staged outbound data can be read.
    fn stage_outbound(
        &self,
        handle: &ArtifactHandle,
    ) -> impl std::future::Future<Output = Result<ArtifactLocation, ArtifactError>> + Send;

    /// Commit a staged artifact after independently verifying its content hash.
    ///
    /// Returns a new `ArtifactId` (UUIDv7) on success. On hash mismatch,
    /// the staged data is discarded and `ArtifactError::HashMismatch` is returned.
    fn commit_inbound(
        &self,
        handle: &ArtifactHandle,
        kind: NamespacedId,
        hash: ContentHash,
    ) -> impl std::future::Future<Output = Result<ArtifactId, ArtifactError>> + Send;

    /// Discard a staged artifact, removing its staging directory.
    fn discard(
        &self,
        handle: &ArtifactHandle,
    ) -> impl std::future::Future<Output = Result<(), ArtifactError>> + Send;
}

// ---------------------------------------------------------------------------
// LocalStagingTransport
// ---------------------------------------------------------------------------

/// Filesystem-backed staging transport.
///
/// Staging layout: `<root>/<instance_id>/<request_id>/<artifact_uuid>/data`
pub struct LocalStagingTransport {
    /// Root staging directory (e.g., `<project>/staging/`).
    root: PathBuf,
    /// Provider instance identifier for directory scoping.
    instance_id: ProviderInstanceId,
    /// Request correlation identifier for directory scoping.
    request_id: String,
}

impl LocalStagingTransport {
    /// Creates a new local staging transport.
    pub fn new(root: PathBuf, instance_id: ProviderInstanceId, request_id: String) -> Self {
        LocalStagingTransport {
            root,
            instance_id,
            request_id,
        }
    }

    /// Returns the scoped request directory: `<root>/<instance_id>/<request_id>/`.
    fn request_dir(&self) -> PathBuf {
        self.root
            .join(self.instance_id.to_string())
            .join(&self.request_id)
    }

    /// Builds the staging directory for a given UUID:
    /// `<root>/<instance_id>/<request_id>/<uuid>/`
    fn staging_dir_for(&self, uuid: Uuid) -> PathBuf {
        self.request_dir().join(uuid.to_string())
    }
}

impl ArtifactTransport for LocalStagingTransport {
    async fn stage_inbound(&self, bytes: Bytes) -> Result<ArtifactHandle, ArtifactError> {
        let artifact_uuid = Uuid::now_v7();
        let staging_dir = self.staging_dir_for(artifact_uuid);
        let data_path = staging_dir.join("data");

        tokio::fs::create_dir_all(&staging_dir).await?;
        tokio::fs::write(&data_path, &bytes).await?;

        Ok(ArtifactHandle {
            staging_path: staging_dir,
            artifact_uuid,
            instance_id: self.instance_id,
            request_id: Some(self.request_id.clone()),
            committed: AtomicBool::new(false),
        })
    }

    async fn stage_outbound(
        &self,
        handle: &ArtifactHandle,
    ) -> Result<ArtifactLocation, ArtifactError> {
        let data_path = handle.data_path();
        if !tokio::fs::try_exists(&data_path).await? {
            return Err(ArtifactError::NotFound(data_path));
        }
        Ok(ArtifactLocation::Local(data_path))
    }

    async fn commit_inbound(
        &self,
        handle: &ArtifactHandle,
        _kind: NamespacedId,
        hash: ContentHash,
    ) -> Result<ArtifactId, ArtifactError> {
        if handle
            .committed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(ArtifactError::AlreadyCommitted);
        }

        let data_path = handle.data_path();
        if !tokio::fs::try_exists(&data_path).await? {
            return Err(ArtifactError::NotFound(data_path));
        }

        let staged_bytes = tokio::fs::read(&data_path).await?;
        let actual_hash = ContentHash::blake3(&staged_bytes);

        if actual_hash != hash {
            // Discard on mismatch — do not leave corrupt staging data.
            let _ = tokio::fs::remove_dir_all(&handle.staging_path).await;
            return Err(ArtifactError::HashMismatch {
                expected: hash,
                actual: actual_hash,
            });
        }

        // Hash matches: leave staged data in place (canonical copy is the
        // application layer's responsibility in a later todo).
        Ok(ArtifactId::from_uuid(Uuid::now_v7()))
    }

    async fn discard(&self, handle: &ArtifactHandle) -> Result<(), ArtifactError> {
        if tokio::fs::try_exists(&handle.staging_path).await? {
            tokio::fs::remove_dir_all(&handle.staging_path).await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// StagingReconciler
// ---------------------------------------------------------------------------

/// Removes orphan staging directories whose request IDs are no longer active.
pub struct StagingReconciler {
    /// Root staging directory.
    root: PathBuf,
    /// Provider instance identifier for directory scoping.
    instance_id: ProviderInstanceId,
}

impl StagingReconciler {
    /// Creates a new reconciler scoped to the given instance.
    pub fn new(root: PathBuf, instance_id: ProviderInstanceId) -> Self {
        StagingReconciler { root, instance_id }
    }

    /// Sweeps orphan request directories under `<root>/<instance_id>/`.
    ///
    /// Any directory whose name is NOT in `active_operations` is removed
    /// recursively. Returns the paths of removed directories.
    pub async fn sweep(
        &self,
        active_operations: &HashSet<String>,
    ) -> Result<Vec<PathBuf>, ArtifactError> {
        let instance_dir = self.root.join(self.instance_id.to_string());
        if !tokio::fs::try_exists(&instance_dir).await? {
            return Ok(Vec::new());
        }

        let mut removed = Vec::new();
        let mut entries = tokio::fs::read_dir(&instance_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await.map_err(|e| {
                ArtifactError::OrphanSweep(format!("failed to read entry type: {e}"))
            })?;
            if !file_type.is_dir() {
                continue;
            }

            let dir_name = entry.file_name().to_string_lossy().to_string();
            if active_operations.contains(&dir_name) {
                continue;
            }

            let dir_path = entry.path();
            tokio::fs::remove_dir_all(&dir_path).await.map_err(|e| {
                ArtifactError::OrphanSweep(format!("failed to remove {}: {e}", dir_path.display()))
            })?;
            removed.push(dir_path);
        }

        Ok(removed)
    }
}
