//! Capability-specific parser tests for the 7 LLM analysis capabilities.
//!
//! For each capability, two tests are provided:
//!
//! * `<capability>_happy` — validates a well-formed response through the
//!   full `LlmImporter::import()` pipeline and asserts the canonical
//!   `AddHypothesis` commands match the expected claims.
//!
//! * `<capability>_<rule>` — validates a response that violates exactly one
//!   spec §8.6 invariant (named after the rule). Runs with `attempt_count=1`
//!   so the retry budget is exhausted and the pipeline issues exactly one
//!   `FailWorkItem` and one `BlockWorkWithReason` command and zero
//!   `AddHypothesis` commands.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use autore_app::application_service::requests::AddHypothesisRequest;
use autore_app::{ApplicationCommand, AutoReClient};
use autore_reconstruction::CallSiteSummary;
use autore_reconstruction::analysis::{InvestigationBundle, LlmImportResult, LlmImporter};
use autore_reconstruction::work_graph::DependencyEdgeKind;
use autore_schema::ids::{
    ArtifactId, ConflictRecordId, EntityId, HypothesisId, ProjectId, WorkItemId,
};
use serde_json::Value;
use tests_support::RecordingAutoReClient;

// ---------------------------------------------------------------------------
// Bundle builder
// ---------------------------------------------------------------------------

/// Builds a realistic bundle with at least one relevant entity, one accepted
/// hypothesis, and one caller/callee so the happy fixtures can reference
/// valid IDs.
fn realistic_bundle() -> InvestigationBundle {
    InvestigationBundle {
        subject_identity: WorkItemId::new(),
        subject_entity_id: Some(EntityId::new()),
        static_structural_snapshot: None,
        decompilation_artifact: None,
        disassembly_artifact: None,
        cfg_summary: None,
        callers_and_callees: vec![CallSiteSummary {
            work_item_id: WorkItemId::new(),
            brief: "entry point that dispatches to parse_input".into(),
            edge_kind: DependencyEdgeKind::DirectCall,
        }],
        relevant_types: vec![EntityId::new()],
        relevant_globals: vec![],
        strings_and_constants: vec![],
        dynamic_observations: vec![],
        accepted_hypotheses: vec![HypothesisId::new()],
        unresolved_conflicts: vec![ConflictRecordId::new()],
        prior_generated_candidate: None,
        compiler_diagnostics: vec![],
        verification_failures: vec![],
        requested_output_schema: Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Loads a fixture file and substitutes the `{{...}}` placeholders with real
/// IDs from the bundle so `evidence_references` and related fields reference
/// bundle-resident entities.
fn load_fixture(capability_slug: &str, variant: &str, bundle: &InvestigationBundle) -> Value {
    let raw = include_fixture(capability_slug, variant);
    let substituted = substitute_placeholders(&raw, bundle);
    serde_json::from_str(&substituted).expect("fixture must be valid JSON after substitution")
}

fn include_fixture(capability_slug: &str, variant: &str) -> String {
    match (capability_slug, variant) {
        ("function-analysis", "raw-response") => {
            include_str!("fixtures/llm/function-analysis.raw-response.json").to_owned()
        }
        ("function-analysis", "malformed") => {
            include_str!("fixtures/llm/function-analysis.malformed.json").to_owned()
        }
        ("type-analysis", "raw-response") => {
            include_str!("fixtures/llm/type-analysis.raw-response.json").to_owned()
        }
        ("type-analysis", "malformed") => {
            include_str!("fixtures/llm/type-analysis.malformed.json").to_owned()
        }
        ("class-analysis", "raw-response") => {
            include_str!("fixtures/llm/class-analysis.raw-response.json").to_owned()
        }
        ("class-analysis", "malformed") => {
            include_str!("fixtures/llm/class-analysis.malformed.json").to_owned()
        }
        ("subsystem-analysis", "raw-response") => {
            include_str!("fixtures/llm/subsystem-analysis.raw-response.json").to_owned()
        }
        ("subsystem-analysis", "malformed") => {
            include_str!("fixtures/llm/subsystem-analysis.malformed.json").to_owned()
        }
        ("conflict-analysis", "raw-response") => {
            include_str!("fixtures/llm/conflict-analysis.raw-response.json").to_owned()
        }
        ("conflict-analysis", "malformed") => {
            include_str!("fixtures/llm/conflict-analysis.malformed.json").to_owned()
        }
        ("failure-analysis", "raw-response") => {
            include_str!("fixtures/llm/failure-analysis.raw-response.json").to_owned()
        }
        ("failure-analysis", "malformed") => {
            include_str!("fixtures/llm/failure-analysis.malformed.json").to_owned()
        }
        ("experiment-design", "raw-response") => {
            include_str!("fixtures/llm/experiment-design.raw-response.json").to_owned()
        }
        ("experiment-design", "malformed") => {
            include_str!("fixtures/llm/experiment-design.malformed.json").to_owned()
        }
        other => panic!("unknown fixture: {other:?}"),
    }
}

fn substitute_placeholders(raw: &str, bundle: &InvestigationBundle) -> String {
    let entity_ref = bundle
        .relevant_types
        .first()
        .map(ToString::to_string)
        .expect("bundle has at least one relevant type");
    let subject_entity = bundle
        .subject_entity_id
        .map(|id| id.to_string())
        .expect("bundle has subject entity");
    let caller_work_item = bundle
        .callers_and_callees
        .first()
        .map(|cs| cs.work_item_id.to_string())
        .expect("bundle has at least one caller");
    let accepted_hyp = bundle
        .accepted_hypotheses
        .first()
        .map(ToString::to_string)
        .expect("bundle has at least one accepted hypothesis");

    raw.replace("{{ENTITY_REF_1}}", &entity_ref)
        .replace("{{SUBJECT_ENTITY_ID}}", &subject_entity)
        .replace("{{CALLER_WORK_ITEM}}", &caller_work_item)
        .replace("{{ACCEPTED_HYPOTHESIS}}", &accepted_hyp)
}

// ---------------------------------------------------------------------------
// Import driver
// ---------------------------------------------------------------------------

fn run_import(
    bundle: &InvestigationBundle,
    capability_id: &str,
    attempt_count: u32,
    parsed: Value,
    client: &dyn AutoReClient,
) -> LlmImportResult {
    let raw_text = serde_json::to_string(&parsed).expect("serialize");
    let importer = LlmImporter::new(
        bundle,
        capability_id,
        ArtifactId::new(),
        ArtifactId::new(),
        attempt_count,
        client,
        ProjectId::new(),
        raw_text,
        parsed,
    );
    importer.import().expect("import must not error")
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

fn assert_success_hypotheses(
    result: &LlmImportResult,
    capability_id: &str,
    expected_predicate_suffixes: &[&str],
) {
    let LlmImportResult::Success { hypotheses, .. } = result else {
        panic!("expected Success, got {result:?}");
    };

    let predicates: Vec<String> = hypotheses
        .iter()
        .filter_map(|cmd| match cmd {
            ApplicationCommand::AddHypothesis(AddHypothesisRequest { predicate, .. }) => {
                Some(predicate.clone())
            }
            _ => None,
        })
        .collect();

    for suffix in expected_predicate_suffixes {
        let expected = format!("{capability_id}.{suffix}");
        assert!(
            predicates.iter().any(|p| p == &expected),
            "expected hypothesis predicate '{expected}' not found in {predicates:?}"
        );
    }
}

fn assert_malformed_counts(result: &LlmImportResult, client: &RecordingAutoReClient) {
    assert!(
        matches!(result, LlmImportResult::InvalidOutput { .. }),
        "expected InvalidOutput on exhausted retry budget, got {result:?}"
    );

    let add_hyp = client.count(|cmd| matches!(cmd, ApplicationCommand::AddHypothesis(_)));
    let fail_work = client.count(|cmd| matches!(cmd, ApplicationCommand::FailWorkItem(_)));
    let block_reason =
        client.count(|cmd| matches!(cmd, ApplicationCommand::BlockWorkWithReason(_)));

    assert_eq!(add_hyp, 0, "zero AddHypothesis commands on InvalidOutput");
    assert_eq!(fail_work, 1, "exactly one FailWorkItem on InvalidOutput");
    assert_eq!(
        block_reason, 1,
        "exactly one BlockWorkWithReason on InvalidOutput"
    );
}

fn run_happy_case(capability_slug: &str, capability_id: &str, expected_suffixes: &[&str]) {
    let bundle = realistic_bundle();
    let client = RecordingAutoReClient::new();
    let parsed = load_fixture(capability_slug, "raw-response", &bundle);
    let result = run_import(&bundle, capability_id, 0, parsed, &client);
    assert_success_hypotheses(&result, capability_id, expected_suffixes);
}

fn run_malformed_case(capability_slug: &str, capability_id: &str) {
    let bundle = realistic_bundle();
    let client = RecordingAutoReClient::new();
    let parsed = load_fixture(capability_slug, "malformed", &bundle);
    let result = run_import(&bundle, capability_id, 1, parsed, &client);
    assert_malformed_counts(&result, &client);
}

// ===========================================================================
// Happy-path tests
// ===========================================================================

#[test]
fn function_analysis_happy() {
    run_happy_case(
        "function-analysis",
        "llm.analysis.function",
        &["behavior-claim", "function-name"],
    );
}

#[test]
fn type_analysis_happy() {
    run_happy_case("type-analysis", "llm.analysis.type", &["type-proposal"]);
}

#[test]
fn class_analysis_happy() {
    run_happy_case("class-analysis", "llm.analysis.class", &[]);
}

#[test]
fn subsystem_analysis_happy() {
    run_happy_case("subsystem-analysis", "llm.analysis.subsystem", &[]);
}

#[test]
fn conflict_analysis_happy() {
    run_happy_case("conflict-analysis", "llm.analysis.conflict", &[]);
}

#[test]
fn failure_analysis_happy() {
    run_happy_case("failure-analysis", "llm.analysis.failure", &[]);
}

#[test]
fn experiment_design_happy() {
    run_happy_case(
        "experiment-design",
        "llm.experiment.design",
        &["experiment-hypothesis"],
    );
}

// ===========================================================================
// Malformed-path tests (one spec §8.6 rule per fixture)
// ===========================================================================

#[test]
fn function_ref_entities_exist() {
    run_malformed_case("function-analysis", "llm.analysis.function");
}

#[test]
fn type_offsets_within_range() {
    run_malformed_case("type-analysis", "llm.analysis.type");
}

#[test]
fn class_vtable_id_well_formed() {
    run_malformed_case("class-analysis", "llm.analysis.class");
}

#[test]
fn subsystem_confidence_within_range() {
    run_malformed_case("subsystem-analysis", "llm.analysis.subsystem");
}

#[test]
fn conflict_evidence_in_bundle() {
    run_malformed_case("conflict-analysis", "llm.analysis.conflict");
}

#[test]
fn failure_experiment_proposal_debug_only() {
    run_malformed_case("failure-analysis", "llm.analysis.failure");
}

#[test]
fn experiment_design_debug_capability_only() {
    run_malformed_case("experiment-design", "llm.experiment.design");
}
