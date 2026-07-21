//! Integration tests for the artifact transport module.

use std::collections::HashSet;

use autore_provider_runtime::artifact::{
    ArtifactError, ArtifactLocation, ArtifactTransport, LocalStagingTransport, StagingReconciler,
};
use autore_schema::{ContentHash, NamespacedId, ProviderInstanceId};
use bytes::Bytes;
use tempfile::TempDir;

/// Creates a 1 KiB blob of deterministic bytes for testing.
fn test_blob_1kib() -> Bytes {
    let data: Vec<u8> = (0u16..1024).map(|i| (i % 256) as u8).collect();
    Bytes::from(data)
}

/// Creates a `LocalStagingTransport` rooted in a temp directory.
fn test_transport(tmp: &TempDir) -> LocalStagingTransport {
    LocalStagingTransport::new(
        tmp.path().to_path_buf(),
        ProviderInstanceId::new(),
        "req-001".to_string(),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn staging_layout_is_instance_scoped() {
    let tmp = TempDir::new().unwrap();
    let transport = test_transport(&tmp);
    let blob = test_blob_1kib();

    let handle = transport.stage_inbound(blob).await.unwrap();

    let staging = handle.staging_path().to_string_lossy().to_string();
    let instance_id_str = handle.instance_id().to_string();

    assert!(
        staging.contains(&instance_id_str),
        "staging path must contain instance_id: {staging}"
    );
    assert!(
        staging.contains("req-001"),
        "staging path must contain request_id: {staging}"
    );
    assert!(
        staging.contains(&handle.artifact_uuid().to_string()),
        "staging path must contain artifact UUID: {staging}"
    );

    // Cleanup
    transport.discard(&handle).await.unwrap();
}

#[tokio::test]
async fn handle_never_exposes_canonical_path() {
    let tmp = TempDir::new().unwrap();
    let transport = test_transport(&tmp);
    let blob = test_blob_1kib();

    let handle = transport.stage_inbound(blob).await.unwrap();
    let location = transport.stage_outbound(&handle).await.unwrap();

    match &location {
        ArtifactLocation::Local(path) => {
            let path_str = path.to_string_lossy();
            assert!(
                !path_str.contains("artifacts/sha256"),
                "outbound must not expose canonical sha256 path: {path_str}"
            );
            assert!(
                !path_str.contains("artifacts/blake3"),
                "outbound must not expose canonical blake3 path: {path_str}"
            );
        }
        ArtifactLocation::Remote(_) => {
            panic!("expected Local location, got Remote");
        }
    }

    transport.discard(&handle).await.unwrap();
}

#[tokio::test]
async fn commit_recomputes_hash_independently() {
    let tmp = TempDir::new().unwrap();
    let transport = test_transport(&tmp);
    let blob = test_blob_1kib();

    // Compute the correct BLAKE3 hash independently.
    let expected_hash = ContentHash::blake3(&blob);

    let handle = transport.stage_inbound(blob).await.unwrap();
    let kind = NamespacedId::parse("core.binary").unwrap();

    let artifact_id = transport
        .commit_inbound(&handle, kind, expected_hash.clone())
        .await
        .unwrap();

    // The returned ArtifactId should be a valid UUID.
    assert!(
        artifact_id.as_uuid().get_version_num() > 0,
        "artifact ID should be a valid UUID"
    );

    // The data file should still exist (canonical copy is app layer's job).
    let data_path = handle.staging_path().join("data");
    assert!(
        std::fs::exists(&data_path).unwrap(),
        "staged data must remain after commit"
    );
}

#[tokio::test]
async fn commit_rejects_mismatched_hash() {
    let tmp = TempDir::new().unwrap();
    let transport = test_transport(&tmp);
    let blob = test_blob_1kib();

    let handle = transport.stage_inbound(blob).await.unwrap();
    let kind = NamespacedId::parse("core.binary").unwrap();

    // Provide a deliberately wrong hash.
    let wrong_hash = ContentHash::blake3(b"wrong content");

    let result = transport.commit_inbound(&handle, kind, wrong_hash).await;

    assert!(
        matches!(result, Err(ArtifactError::HashMismatch { .. })),
        "expected HashMismatch, got: {result:?}"
    );

    // Staging directory should be cleaned up after mismatch.
    assert!(
        !std::fs::exists(handle.staging_path()).unwrap(),
        "staging dir must be removed after hash mismatch"
    );
}

#[tokio::test]
async fn discard_removes_staging_dir() {
    let tmp = TempDir::new().unwrap();
    let transport = test_transport(&tmp);
    let blob = test_blob_1kib();

    let handle = transport.stage_inbound(blob).await.unwrap();
    let staging_path = handle.staging_path().to_path_buf();

    assert!(
        std::fs::exists(&staging_path).unwrap(),
        "staging dir must exist before discard"
    );

    transport.discard(&handle).await.unwrap();

    assert!(
        !std::fs::exists(&staging_path).unwrap(),
        "staging dir must be removed after discard"
    );
}

#[tokio::test]
async fn reconciler_sweeps_orphans_on_startup() {
    let tmp = TempDir::new().unwrap();
    let instance_id = ProviderInstanceId::new();

    // Manually create orphan request directories.
    let instance_dir = tmp.path().join(instance_id.to_string());
    std::fs::create_dir_all(instance_dir.join("orphan-req-1")).unwrap();
    std::fs::create_dir_all(instance_dir.join("orphan-req-2")).unwrap();
    std::fs::write(
        instance_dir.join("orphan-req-1").join("dummy"),
        "leftover data",
    )
    .unwrap();

    let reconciler = StagingReconciler::new(tmp.path().to_path_buf(), instance_id);
    let active: HashSet<String> = HashSet::new(); // No active operations.

    let removed = reconciler.sweep(&active).await.unwrap();

    assert_eq!(removed.len(), 2, "should remove 2 orphan directories");
    assert!(
        !std::fs::exists(instance_dir.join("orphan-req-1")).unwrap(),
        "orphan-req-1 must be removed"
    );
    assert!(
        !std::fs::exists(instance_dir.join("orphan-req-2")).unwrap(),
        "orphan-req-2 must be removed"
    );
}
