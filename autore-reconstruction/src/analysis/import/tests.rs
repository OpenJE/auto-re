//! Tests for the 3-level import boundary.

use autore_app::{ApplicationCommand, AutoReClient};
use autore_schema::ids::{ArtifactId, EntityId, HypothesisId, ProjectId, WorkItemId};
use serde_json::{Value, json};

use crate::analysis::bundle::InvestigationBundle;
use crate::analysis::import::{LlmImportResult, LlmImporter};
use crate::analysis::schemas::response_schema_for;
use crate::tests_support::RecordingAutoReClient;

// -- helpers ---------------------------------------------------------------

fn test_bundle() -> InvestigationBundle {
    InvestigationBundle {
        subject_identity: WorkItemId::new(),
        subject_entity_id: Some(EntityId::new()),
        static_structural_snapshot: None,
        decompilation_artifact: None,
        disassembly_artifact: None,
        cfg_summary: None,
        callers_and_callees: vec![],
        relevant_types: vec![EntityId::new()],
        relevant_globals: vec![],
        strings_and_constants: vec![],
        dynamic_observations: vec![],
        accepted_hypotheses: vec![HypothesisId::new()],
        unresolved_conflicts: vec![],
        prior_generated_candidate: None,
        compiler_diagnostics: vec![],
        verification_failures: vec![],
        requested_output_schema: response_schema_for("llm.analysis.function")
            .unwrap_or(Value::Null),
    }
}

fn valid_function_response(bundle: &InvestigationBundle) -> Value {
    let entity_ref = bundle
        .relevant_types
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    json!({
        "proposed_name": "parse_input",
        "behavior_claims": ["parses a buffer"],
        "side_effects": ["writes to global state"],
        "signature": "void parse_input(char*, int)",
        "evidence_references": [entity_ref],
        "confidence": 0.9,
        "recommended_follow_up_work": ["check callers"]
    })
}

fn make_importer<'a>(
    bundle: &'a InvestigationBundle,
    client: &'a dyn AutoReClient,
    capability_id: &str,
    attempt_count: u32,
    parsed: Value,
) -> LlmImporter<'a> {
    LlmImporter::new(
        bundle,
        capability_id,
        ArtifactId::new(),
        ArtifactId::new(),
        attempt_count,
        client,
        ProjectId::new(),
        "raw response text".to_owned(),
        parsed,
    )
}

// -- tests -----------------------------------------------------------------

#[test]
fn raw_response_always_persisted_irrespective_of_validation() {
    let bundle = test_bundle();
    let client = RecordingAutoReClient::new();

    let bad_response = json!({ "invalid": true });
    let importer = make_importer(&bundle, &client, "llm.analysis.function", 0, bad_response);
    let result = importer.import().expect("import should not error");

    assert!(matches!(result, LlmImportResult::RepairRequested { .. }));

    let evidence_count = client.count(|cmd| matches!(cmd, ApplicationCommand::AddEvidence(_)));
    assert_eq!(
        evidence_count, 1,
        "raw response must be persisted as AddEvidence even on validation failure"
    );
}

#[test]
fn parsed_result_validated_against_schema() {
    let bundle = test_bundle();
    let client = RecordingAutoReClient::new();

    let good = valid_function_response(&bundle);
    let importer = make_importer(&bundle, &client, "llm.analysis.function", 0, good);
    let result = importer.import().expect("import should not error");

    assert!(
        matches!(result, LlmImportResult::Success { .. }),
        "valid response must produce Success"
    );
}

#[test]
fn referenced_entities_must_exist_in_original_bundle() {
    let bundle = test_bundle();
    let client = RecordingAutoReClient::new();

    let bad = json!({
        "proposed_name": "f",
        "behavior_claims": ["does stuff"],
        "evidence_references": ["hallucinated-entity-id"],
        "confidence": 0.5,
        "recommended_follow_up_work": []
    });
    let importer = make_importer(&bundle, &client, "llm.analysis.function", 0, bad);
    let result = importer.import().expect("import should not error");

    assert!(
        matches!(result, LlmImportResult::RepairRequested { .. }),
        "hallucinated entity references must be rejected"
    );
}

#[test]
fn confidence_outside_0_to_1_rejected() {
    let bundle = test_bundle();
    let client = RecordingAutoReClient::new();

    let entity_ref = bundle
        .relevant_types
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    let bad = json!({
        "proposed_name": "f",
        "behavior_claims": ["does stuff"],
        "evidence_references": [entity_ref],
        "confidence": 1.5,
        "recommended_follow_up_work": []
    });
    let importer = make_importer(&bundle, &client, "llm.analysis.function", 0, bad);
    let result = importer.import().expect("import should not error");

    match &result {
        LlmImportResult::RepairRequested { repair_prompt } => {
            assert!(
                repair_prompt.contains("confidence"),
                "repair prompt must mention the confidence error"
            );
        }
        other => panic!("expected RepairRequested, got {other:?}"),
    }
}

#[test]
fn arbitrary_text_in_experiment_proposal_rejected() {
    let bundle = test_bundle();
    let client = RecordingAutoReClient::new();

    let bad = json!({
        "hypothesis_statement": "test hypothesis",
        "test_plan": ["run debugger"],
        "expected_observations": ["breakpoint hit"],
        "required_capabilities": ["llm.analysis.function"],
        "risk_factors": []
    });
    let importer = make_importer(&bundle, &client, "llm.experiment.design", 0, bad);
    let result = importer.import().expect("import should not error");

    assert!(
        matches!(result, LlmImportResult::RepairRequested { .. }),
        "non-debug.* capability names must be rejected in experiment proposals"
    );
}

#[test]
fn no_partial_import_on_second_failure() {
    let bundle = test_bundle();
    let client = RecordingAutoReClient::new();

    let bad = json!({ "totally": "invalid" });

    let importer = make_importer(&bundle, &client, "llm.analysis.function", 1, bad.clone());
    let result = importer.import().expect("import should not error");

    assert!(
        matches!(result, LlmImportResult::InvalidOutput { .. }),
        "second failure must return InvalidOutput"
    );

    let hypothesis_count = client.count(|cmd| matches!(cmd, ApplicationCommand::AddHypothesis(_)));
    assert_eq!(hypothesis_count, 0, "zero hypotheses on second failure");

    let fail_count = client.count(|cmd| matches!(cmd, ApplicationCommand::FailWorkItem(_)));
    assert_eq!(fail_count, 1, "FailWorkItem issued exactly once");

    let block_count = client.count(|cmd| matches!(cmd, ApplicationCommand::BlockWorkWithReason(_)));
    assert_eq!(block_count, 1, "BlockWorkWithReason issued exactly once");
}

#[test]
fn bounded_retry_attempt_count_one() {
    let bundle = test_bundle();

    let bad = json!({ "bad": "data" });

    let client0 = RecordingAutoReClient::new();
    let importer0 = make_importer(&bundle, &client0, "llm.analysis.function", 0, bad.clone());
    let result0 = importer0.import().expect("import should not error");
    assert!(
        matches!(result0, LlmImportResult::RepairRequested { .. }),
        "first failure (attempt_count=0) must return RepairRequested"
    );

    let client1 = RecordingAutoReClient::new();
    let importer1 = make_importer(&bundle, &client1, "llm.analysis.function", 1, bad);
    let result1 = importer1.import().expect("import should not error");
    assert!(
        matches!(result1, LlmImportResult::InvalidOutput { .. }),
        "second failure (attempt_count=1) must return InvalidOutput"
    );
}

#[test]
fn provenance_records_no_plaintext_secrets() {
    let bundle = test_bundle();
    let client = RecordingAutoReClient::new();

    let secret = "sk-super-secret-api-key-12345";
    let good = valid_function_response(&bundle);

    let raw_text = format!("raw response with no secret: {good}");
    let importer = LlmImporter::new(
        &bundle,
        "llm.analysis.function",
        ArtifactId::new(),
        ArtifactId::new(),
        0,
        &client,
        ProjectId::new(),
        raw_text,
        good,
    );
    let _ = importer.import().expect("import should not error");

    let commands = client.commands();
    let commands_json = serde_json::to_string(&commands).unwrap();
    assert!(
        !commands_json.contains(secret),
        "no plaintext secrets in emitted commands"
    );
}
