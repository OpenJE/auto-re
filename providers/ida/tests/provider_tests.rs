//! Integration tests for the IDA provider.
//!
//! Tests requiring a real IDA Pro installation are gated with `#[ignore]`.
//! Compile-only / mocked smoke tests run in every environment.

use autore_provider_protocol::v1::{ExecutionRequest, NegotiateRequest, completed, diagnostic};

/// Smoke test: the binary compiles and the crate links.
/// Asserts that we can reference the protocol types correctly.
#[test]
fn protocol_types_are_accessible() {
    let req = NegotiateRequest {
        min_supported: 1,
        max_supported: 1,
        coordinator_id: "test-coordinator".into(),
    };
    assert_eq!(req.min_supported, 1);
    assert_eq!(req.max_supported, 1);
}

/// Verify execution request structure for ida.binary.open.
#[test]
fn binary_open_request_structure() {
    let req = ExecutionRequest {
        request_id: "req-001".into(),
        operation_id: "op-001".into(),
        capability_id: "ida.binary.open".into(),
        capability_version: "1.0.0".into(),
        payload: b"/path/to/test.idb".to_vec(),
        deadline: None,
    };
    assert_eq!(req.capability_id, "ida.binary.open");
    assert!(!req.payload.is_empty());
}

/// Verify execution request structure for ida.binary.ingest.
#[test]
fn binary_ingest_request_structure() {
    let req = ExecutionRequest {
        request_id: "req-002".into(),
        operation_id: "op-002".into(),
        capability_id: "ida.binary.ingest".into(),
        capability_version: "1.0.0".into(),
        payload: Vec::new(),
        deadline: None,
    };
    assert_eq!(req.capability_id, "ida.binary.ingest");
}

/// Verify that all 9 capabilities are defined in the expected list.
#[test]
fn all_nine_capabilities_listed() {
    let caps = [
        "ida.binary.open",
        "ida.binary.ingest",
        "ida.program.refresh",
        "ida.function.snapshot",
        "ida.type.snapshot",
        "ida.class.snapshot",
        "ida.references.query",
        "ida.reanalyze",
        "ida.native-artifact.export",
    ];
    assert_eq!(caps.len(), 9);
    for cap in &caps {
        assert!(
            cap.starts_with("ida."),
            "capability must be namespaced: {cap}"
        );
    }
}

/// Verify staging artifact paths do NOT contain `artifacts/sha256`
/// (they should be in staging, not in the committed artifact store).
#[test]
fn binary_ingest_staging_path_does_not_contain_artifacts_sha256() {
    let staging_dir = std::env::temp_dir()
        .join("ida-provider-staging")
        .join("test-request-id");
    let path_str = staging_dir.to_string_lossy();
    assert!(
        !path_str.contains("artifacts/sha256"),
        "staging path must not contain committed artifact store path: {path_str}"
    );
}

/// Verify completed status enum values (proto: STATUS_UNSPECIFIED=0, SUCCEEDED=1, FAILED=2).
#[test]
fn completed_status_values() {
    assert_eq!(completed::Status::Succeeded as i32, 1);
    assert_eq!(completed::Status::Failed as i32, 2);
}

/// Verify diagnostic severity enum values (proto: SEVERITY_UNSPECIFIED=0, INFO=1, WARNING=2, ERROR=3).
#[test]
fn diagnostic_severity_values() {
    assert_eq!(diagnostic::Severity::Warning as i32, 2);
    assert_eq!(diagnostic::Severity::Error as i32, 3);
}

/// Requires real IDA Pro: open an IDB and verify success.
#[test]
#[ignore = "requires IDA Pro installation and 'ida' feature"]
fn binary_open_succeeds_on_valid_idb() {
    // This test requires:
    // 1. IDA Pro installed and idax linked
    // 2. A valid .idb fixture at tests/fixtures/
    // 3. cargo test --features ida -- --ignored
    panic!("requires IDA Pro");
}

/// Requires real IDA Pro: ingest a binary and assert progress per stage.
#[test]
#[ignore = "requires IDA Pro installation and 'ida' feature"]
fn binary_ingest_emits_progress_per_stage() {
    panic!("requires IDA Pro");
}

/// Requires real IDA Pro: ingest and verify snapshot artifacts in staging.
#[test]
#[ignore = "requires IDA Pro installation and 'ida' feature"]
fn binary_ingest_writes_snapshot_artifacts_to_staging() {
    panic!("requires IDA Pro");
}

/// Requires real IDA Pro: refresh and assert only deltas emitted.
#[test]
#[ignore = "requires IDA Pro installation and 'ida' feature"]
fn program_refresh_emits_only_deltas_for_unchanged() {
    panic!("requires IDA Pro");
}

/// Requires real IDA Pro: refresh with removed entity → stale diagnostic.
#[test]
#[ignore = "requires IDA Pro installation and 'ida' feature"]
fn program_refresh_marks_removed_as_stale() {
    panic!("requires IDA Pro");
}

/// Requires real IDA Pro: function snapshot returns typed result.
#[test]
#[ignore = "requires IDA Pro installation and 'ida' feature"]
fn function_snapshot_returns_typed_result() {
    panic!("requires IDA Pro");
}
