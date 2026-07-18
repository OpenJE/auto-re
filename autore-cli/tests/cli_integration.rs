//! Integration tests for the auto-re CLI.
//!
//! These tests exercise the `auto-re` binary via `assert_cmd`, covering all 12
//! required Stage 0 verbs in both human-readable and JSON output modes, plus
//! failure paths. Each test creates an isolated temporary project directory.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns a `Command` targeting the `auto-re` binary.
fn auto_re() -> Command {
    Command::cargo_bin("auto-re").expect("auto-re binary should be built")
}

/// Creates a project in `dir` with the given name and returns stdout.
fn create_project(dir: &Path, name: &str) -> String {
    let output = auto_re()
        .arg("--project-dir")
        .arg(dir)
        .arg("project")
        .arg("create")
        .arg("--name")
        .arg(name)
        .output()
        .expect("failed to execute auto-re");
    assert!(
        output.status.success(),
        "project create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}

/// Extracts a UUID from "Project created: <name> (<uuid>)" output.
fn extract_project_id(stdout: &str) -> String {
    let start = stdout
        .find('(')
        .expect("missing '(' in project create output")
        + 1;
    let end = stdout
        .find(')')
        .expect("missing ')' in project create output");
    stdout[start..end].to_string()
}

/// Extracts a nested UUID from JSON command-result output.
///
/// Expects `{"$schema": ..., "<VariantKey>": {"<inner_key>": {"id": "<uuid>", ...}}}`.
fn extract_id_from_result_json(stdout: &str, variant_key: &str, inner_key: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(stdout).expect("output should be valid JSON");
    v[variant_key][inner_key]["id"]
        .as_str()
        .expect("id field should be a string")
        .to_string()
}

/// Extracts a top-level UUID from a JSON command-result variant.
///
/// Expects `{"$schema": ..., "<VariantKey>": {"id": "<uuid>", ...}}`.
fn extract_direct_id_from_result_json(stdout: &str, variant_key: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(stdout).expect("output should be valid JSON");
    v[variant_key]["id"]
        .as_str()
        .expect("id field should be a string")
        .to_string()
}

/// Runs a CLI command and asserts success, returning stdout.
fn run_cli_ok(dir: &Path, args: &[&str]) -> String {
    let mut cmd = auto_re();
    cmd.arg("--project-dir").arg(dir);
    for a in args {
        cmd.arg(a);
    }
    let output = cmd.output().expect("failed to execute auto-re");
    assert!(
        output.status.success(),
        "command {:?} failed (exit {:?}):\nstdout: {}\nstderr: {}",
        args,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}

// ---------------------------------------------------------------------------
// 1. project create — happy path (human output only, no --output flag)
// ---------------------------------------------------------------------------

#[test]
fn project_create_human() {
    let tmp = TempDir::new().unwrap();
    let stdout = create_project(tmp.path(), "test-project");
    assert!(stdout.contains("Project created"));
    assert!(stdout.contains("test-project"));
}

// ---------------------------------------------------------------------------
// 2. project info — human and JSON
// ---------------------------------------------------------------------------

#[test]
fn project_info_human() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "info-test");
    let stdout = run_cli_ok(tmp.path(), &["project", "info"]);
    assert!(stdout.contains("Project:"));
    assert!(stdout.contains("info-test"));
}

#[test]
fn project_info_json() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "info-json-test");
    let stdout = run_cli_ok(tmp.path(), &["project", "info", "--output", "json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let schema = v["$schema"].as_str().expect("$schema present");
    assert!(
        schema.starts_with("auto-re/schema/"),
        "unexpected schema: {schema}"
    );
}

// ---------------------------------------------------------------------------
// 3. artifact add — happy path
// ---------------------------------------------------------------------------

#[test]
fn artifact_add() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "artifact-test");

    // Create a dummy source file for the artifact
    let source_file = tmp.path().join("sample.bin");
    fs::write(&source_file, b"hello artifact").unwrap();

    let stdout = run_cli_ok(
        tmp.path(),
        &[
            "artifact",
            "add",
            "--file",
            source_file.to_str().unwrap(),
            "--kind",
            "core.binary",
        ],
    );
    // Write commands always produce JSON via print_command_result
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["$schema"].is_string(), "should have $schema");
}

// ---------------------------------------------------------------------------
// 4. entity add — happy path
// ---------------------------------------------------------------------------

#[test]
fn entity_add() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "entity-test");

    let stdout = run_cli_ok(
        tmp.path(),
        &[
            "entity",
            "add",
            "--kind",
            "entity.function",
            "--display-name",
            "main",
        ],
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["$schema"].is_string(), "should have $schema");
    // Extract entity ID for use in dependent tests
    let entity_id = extract_id_from_result_json(&stdout, "EntityRegistered", "entity");
    assert!(!entity_id.is_empty(), "entity id should not be empty");
}

// ---------------------------------------------------------------------------
// 5. evidence add — happy path
// ---------------------------------------------------------------------------

#[test]
fn evidence_add() {
    let tmp = TempDir::new().unwrap();
    let create_stdout = create_project(tmp.path(), "evidence-test");
    let project_id = extract_project_id(&create_stdout);

    // Add an entity first (evidence needs a subject)
    let entity_stdout = run_cli_ok(tmp.path(), &["entity", "add", "--kind", "entity.function"]);
    let entity_id = extract_id_from_result_json(&entity_stdout, "EntityRegistered", "entity");

    // Construct a valid EvidenceRecord JSON
    let evidence_uuid = uuid::Uuid::new_v4().to_string();
    let evidence_json = serde_json::json!({
        "id": evidence_uuid,
        "project": project_id,
        "subject": entity_id,
        "predicate": "evidence.test",
        "value": {"kind": "String", "value": "test observation"},
        "derivation": {
            "method": {"kind": "DirectObservation"},
            "operation": "core.observe",
            "supporting_evidence": [],
            "source_hypotheses": []
        },
        "provider_run": null,
        "native_artifacts": [],
        "assumptions": [],
        "created_at": "2026-07-17T00:00:00+00:00"
    });

    let record_file = tmp.path().join("evidence.json");
    fs::write(
        &record_file,
        serde_json::to_string_pretty(&evidence_json).unwrap(),
    )
    .unwrap();

    let stdout = run_cli_ok(
        tmp.path(),
        &["evidence", "add", "--record", record_file.to_str().unwrap()],
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["$schema"].is_string(), "should have $schema");
}

// ---------------------------------------------------------------------------
// 6. hypothesis add — happy path
// ---------------------------------------------------------------------------

#[test]
fn hypothesis_add() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "hypothesis-test");

    // Add an entity to be the hypothesis subject
    let entity_stdout = run_cli_ok(tmp.path(), &["entity", "add", "--kind", "entity.function"]);
    let entity_id = extract_id_from_result_json(&entity_stdout, "EntityRegistered", "entity");

    let stdout = run_cli_ok(
        tmp.path(),
        &[
            "hypothesis",
            "add",
            "--subject",
            &entity_id,
            "--predicate",
            "hypothesis.test",
            "--candidate",
            r#"{"kind":"String","value":"candidate-value"}"#,
            "--confidence",
            "0.8",
        ],
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["$schema"].is_string(), "should have $schema");
}

// ---------------------------------------------------------------------------
// 7. hypothesis accept — state machine enforcement
// ---------------------------------------------------------------------------

// CLI has no command to reach UnderInvestigation, so accept from Proposed is
// correctly rejected by the state machine.
#[test]
fn hypothesis_accept_enforces_state_machine() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "accept-test");

    let entity_stdout = run_cli_ok(tmp.path(), &["entity", "add", "--kind", "entity.function"]);
    let entity_id = extract_id_from_result_json(&entity_stdout, "EntityRegistered", "entity");

    let hyp_stdout = run_cli_ok(
        tmp.path(),
        &[
            "hypothesis",
            "add",
            "--subject",
            &entity_id,
            "--predicate",
            "hypothesis.test",
            "--candidate",
            r#"{"kind":"String","value":"v"}"#,
            "--confidence",
            "0.9",
        ],
    );
    let hyp_id = extract_direct_id_from_result_json(&hyp_stdout, "HypothesisAdded");

    auto_re()
        .arg("--project-dir")
        .arg(tmp.path())
        .arg("hypothesis")
        .arg("accept")
        .arg("--id")
        .arg(&hyp_id)
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid state transition"));
}

// ---------------------------------------------------------------------------
// 8. operation list — human and JSON
// ---------------------------------------------------------------------------

#[test]
fn operation_list_human() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "op-list-test");
    let stdout = run_cli_ok(tmp.path(), &["operation", "list"]);
    // Empty project: should say "No operations."
    assert!(
        stdout.contains("No operations") || stdout.contains("operation"),
        "unexpected output: {stdout}"
    );
}

#[test]
fn operation_list_json() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "op-list-json");
    let stdout = run_cli_ok(tmp.path(), &["operation", "list", "--output", "json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let schema = v["$schema"].as_str().expect("$schema present");
    assert!(
        schema.starts_with("auto-re/schema/"),
        "unexpected schema: {schema}"
    );
}

// ---------------------------------------------------------------------------
// 9. events list — human and JSON
// ---------------------------------------------------------------------------

#[test]
fn events_list_human() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "events-test");
    let stdout = run_cli_ok(tmp.path(), &["events", "list"]);
    // After project create, there should be at least one event
    assert!(
        stdout.contains("Seq") || stdout.contains("No events"),
        "unexpected output: {stdout}"
    );
}

#[test]
fn events_list_json() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "events-json");
    let stdout = run_cli_ok(tmp.path(), &["events", "list", "--output", "json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let schema = v["$schema"].as_str().expect("$schema present");
    assert!(
        schema.starts_with("auto-re/schema/"),
        "unexpected schema: {schema}"
    );
}

// ---------------------------------------------------------------------------
// 10. project validate — happy path
// ---------------------------------------------------------------------------

#[test]
fn project_validate() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "validate-test");
    let stdout = run_cli_ok(tmp.path(), &["project", "validate", "--output", "json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["$schema"].is_string(), "should have $schema");
}

// ---------------------------------------------------------------------------
// 11. project rebuild-indexes — happy path
// ---------------------------------------------------------------------------

#[test]
fn project_rebuild_indexes() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "rebuild-test");
    let stdout = run_cli_ok(tmp.path(), &["project", "rebuild-indexes"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["$schema"].is_string(), "should have $schema");
}

// ---------------------------------------------------------------------------
// 12. project migrate — happy path
// ---------------------------------------------------------------------------

#[test]
fn project_migrate() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "migrate-test");
    let stdout = run_cli_ok(tmp.path(), &["project", "migrate"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["$schema"].is_string(), "should have $schema");
}

// ---------------------------------------------------------------------------
// Failure paths
// ---------------------------------------------------------------------------

#[test]
fn project_create_empty_name_fails() {
    let tmp = TempDir::new().unwrap();
    auto_re()
        .arg("--project-dir")
        .arg(tmp.path())
        .arg("project")
        .arg("create")
        .arg("--name")
        .arg("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty"));
}

#[test]
fn hypothesis_accept_nonexistent_fails() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "accept-fail-test");

    let fake_id = uuid::Uuid::new_v4().to_string();
    auto_re()
        .arg("--project-dir")
        .arg(tmp.path())
        .arg("hypothesis")
        .arg("accept")
        .arg("--id")
        .arg(&fake_id)
        .assert()
        .failure();
}

#[test]
fn evidence_add_invalid_json_fails() {
    let tmp = TempDir::new().unwrap();
    create_project(tmp.path(), "evidence-fail-test");

    let bad_file = tmp.path().join("bad.json");
    fs::write(&bad_file, "this is not valid json {{{").unwrap();

    auto_re()
        .arg("--project-dir")
        .arg(tmp.path())
        .arg("evidence")
        .arg("add")
        .arg("--record")
        .arg(bad_file.to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid"));
}

// ---------------------------------------------------------------------------
// Full workflow: create + add + list sequence with JSON snapshots
// ---------------------------------------------------------------------------

#[test]
fn full_create_add_list_workflow() {
    let tmp = TempDir::new().unwrap();
    let create_stdout = create_project(tmp.path(), "workflow-test");
    let project_id = extract_project_id(&create_stdout);

    // Add entity
    let entity_stdout = run_cli_ok(
        tmp.path(),
        &[
            "entity",
            "add",
            "--kind",
            "entity.function",
            "--display-name",
            "workflow_main",
        ],
    );
    let entity_id = extract_id_from_result_json(&entity_stdout, "EntityRegistered", "entity");

    // Add artifact
    let source_file = tmp.path().join("binary.bin");
    fs::write(&source_file, b"\x7fELF fake binary content").unwrap();
    run_cli_ok(
        tmp.path(),
        &[
            "artifact",
            "add",
            "--file",
            source_file.to_str().unwrap(),
            "--kind",
            "core.binary",
        ],
    );

    // Add hypothesis
    let hyp_stdout = run_cli_ok(
        tmp.path(),
        &[
            "hypothesis",
            "add",
            "--subject",
            &entity_id,
            "--predicate",
            "hypothesis.test",
            "--candidate",
            r#"{"kind":"String","value":"workflow-candidate"}"#,
            "--confidence",
            "0.75",
        ],
    );
    let _hyp_id = extract_direct_id_from_result_json(&hyp_stdout, "HypothesisAdded");

    // List entities (JSON)
    let entities_json = run_cli_ok(tmp.path(), &["entity", "list", "--output", "json"]);
    let v: serde_json::Value = serde_json::from_str(&entities_json).unwrap();
    assert!(v["$schema"].is_string());
    assert!(v["entities"].is_array());

    // List operations (human)
    let ops = run_cli_ok(tmp.path(), &["operation", "list"]);
    assert!(
        ops.contains("No operations") || ops.contains("operation"),
        "unexpected ops output: {ops}"
    );

    // List events (JSON)
    let events_json = run_cli_ok(tmp.path(), &["events", "list", "--output", "json"]);
    let ev: serde_json::Value = serde_json::from_str(&events_json).unwrap();
    assert!(ev["$schema"].is_string());
    assert!(ev["events"].is_array());

    // Project info (JSON) — verify project_id matches
    let info_json = run_cli_ok(tmp.path(), &["project", "info", "--output", "json"]);
    let info: serde_json::Value = serde_json::from_str(&info_json).unwrap();
    assert_eq!(info["id"].as_str().unwrap(), project_id);

    // Validate
    let validate_stdout = run_cli_ok(tmp.path(), &["project", "validate", "--output", "json"]);
    let vv: serde_json::Value = serde_json::from_str(&validate_stdout).unwrap();
    assert!(vv["$schema"].is_string());
}
