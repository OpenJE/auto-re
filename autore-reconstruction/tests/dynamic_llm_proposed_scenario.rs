//! End-to-end integration test: LLM-proposed dynamic experiment flow.
//!
//! Exercises the full pipeline from Todo 34:
//!   1. A canonical function entity is created and a work graph built.
//!   2. An LLM "proposes" a typed debugger [`Scenario`] (simulated directly).
//!   3. [`ScenarioVerifier`] validates the scenario against known entities,
//!      mapped memory segments, and an API allowlist.
//!   4. On valid, a `CreateWorkItems` command encodes the investigation intent
//!      in its description.
//!   5. A mock [`TargetRunner`] (WineGdbRunner in mock mode) executes the
//!      scenario and produces `debug.observation` artifacts.
//!   6. [`DynamicObservationImporter`] imports the observation, recomputes the
//!      target work item's fingerprint, and invalidates the originating
//!      Function analysis work item.
//!   7. On invalid, the verifier rejects the scenario and the test asserts a
//!      `FailWorkItem` + `BlockWorkWithReason` path records the rejection.
//!
//! No real LLM endpoint or real Wine/GDB is required.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use tests_support::RecordingAutoReClient;

use autore_app::application_service::requests::{
    BlockWorkWithReasonRequest, CreateWorkItemsRequest, FailWorkItemRequest,
};
use autore_app::{ApplicationCommand, AutoReClient};
use autore_reconstruction::dynamic::import::{
    DynamicObservation, DynamicObservationImporter, ObservationImport, TimestampRange,
};
use autore_reconstruction::dynamic::{
    AddressRange, Scenario, ScenarioStatus, ScenarioValidationError, ScenarioVerifier, SetupOp,
    Step, StopOp, WineGdbRunner, execute_scenario,
};
use autore_reconstruction::fingerprint::{
    FingerprintComparison, FingerprintInput, InMemorySnapshot, compute_fingerprint,
};
use autore_reconstruction::work_graph::{WorkGraph, WorkGraphBuilder};
use autore_schema::domain::records::{ENTITY_KIND_FUNCTION, SemanticEntity};
use autore_schema::domain::{ContentHash, NamespacedId, Timestamp};
use autore_schema::ids::{
    ArtifactId, BinaryRevisionId, EntityId, ProjectId, ProviderRunId, ReconstructionCampaignId,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_function_entity(project_id: ProjectId, name: &str) -> SemanticEntity {
    SemanticEntity::new(
        project_id,
        ENTITY_KIND_FUNCTION.clone(),
        None,
        Some(name.into()),
    )
}

fn make_valid_scenario(entity: EntityId, exe: ArtifactId) -> Scenario {
    Scenario::new(
        vec![SetupOp::LaunchTarget {
            exe_artifact: exe,
            env: HashMap::new(),
            working_dir: PathBuf::from("/tmp"),
        }],
        vec![
            Step::SetBreakpoint { entity },
            Step::Continue,
            Step::CaptureArguments { entity },
        ],
        vec![StopOp::StopAfterInvocationCount { count: 1 }],
    )
}

fn make_fingerprint_input() -> FingerprintInput {
    let zero = ContentHash::blake3(b"zero");
    FingerprintInput {
        static_artifact_hashes: vec![ContentHash::blake3(b"static")],
        accepted_hypotheses: vec![],
        upstream_declarations: vec![ContentHash::blake3(b"upstream")],
        dynamic_observations: vec![],
        prompt_template_version: "v1".into(),
        model_config_hash: zero.clone(),
        build_config_hash: zero.clone(),
        verification_policy_hash: zero,
    }
}

// ---------------------------------------------------------------------------
// End-to-end test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn llm_proposed_scenario_end_to_end() {
    eprintln!("[llm_experiment_flow] bootstrapping project + campaign");

    let project_id = ProjectId::new();
    let campaign_id = ReconstructionCampaignId::new();
    let binary_rev = BinaryRevisionId::new();
    let run_id = ProviderRunId::new();
    let exe_artifact = ArtifactId::new();
    let client = RecordingAutoReClient::new();

    // 1. Build minimal canonical graph: one function entity.
    let function_entity = make_function_entity(project_id, "analyze_me");
    let graph: WorkGraph = WorkGraphBuilder::build(
        &client,
        project_id,
        campaign_id,
        binary_rev,
        std::slice::from_ref(&function_entity),
        &[],
    )
    .expect("work graph build must succeed");

    let function_node = *graph
        .entity_to_node
        .get(&function_entity.id)
        .expect("function entity must map to a work-graph node");
    let function_work_item = graph.graph[function_node].work_item_id;
    eprintln!(
        "[llm_experiment_flow] function work item: {function_work_item} for entity {}",
        function_entity.id
    );

    // 2. Simulate the LLM proposing a typed Scenario AST.
    let scenario = make_valid_scenario(function_entity.id, exe_artifact);
    eprintln!("[llm_experiment_flow] LLM proposed scenario: {scenario:?}");

    // 3. ScenarioVerifier validates the proposal (security boundary).
    let mut entities_by_id = HashMap::new();
    entities_by_id.insert(function_entity.id, function_entity.clone());
    let mapped_segments = vec![AddressRange::new(0x400000, 0x500000)];
    let mut allowed_apis = HashSet::new();
    allowed_apis
        .insert(NamespacedId::parse("win32.kernel32.create-file").expect("valid allowlisted API"));

    let validation =
        ScenarioVerifier::validate(&scenario, &entities_by_id, &mapped_segments, &allowed_apis);
    assert!(
        validation.is_ok(),
        "valid scenario must pass verifier: {validation:?}"
    );
    eprintln!("[llm_experiment_flow] scenario verifier: PASS");

    // 4. On valid: schedule an investigation work item.
    //    CreateWorkItemsRequest has no `kind` field; encode intent in the description.
    client
        .execute(ApplicationCommand::CreateWorkItems(
            CreateWorkItemsRequest {
                project: project_id,
                campaign_id: campaign_id.to_string(),
                descriptions: vec![format!(
                    "Investigation: execute validated debugger scenario for function {}",
                    function_entity.id
                )],
            },
        ))
        .expect("CreateWorkItems for investigation must succeed");
    let investigation_count = client.count(|c| {
        matches!(
            c,
            ApplicationCommand::CreateWorkItems(req)
            if req.descriptions.iter().any(|d| d.contains("Investigation:"))
        )
    });
    assert!(
        investigation_count >= 1,
        "at least one CreateWorkItems encodes investigation intent"
    );
    eprintln!("[llm_experiment_flow] scheduled investigation work item");

    // 5. Execute the validated scenario via a mock TargetRunner.
    let result = execute_scenario(&WineGdbRunner::mock(), &scenario)
        .await
        .expect("scenario execution must succeed");
    assert_eq!(result.status, ScenarioStatus::Passed);
    assert!(
        !result.ctx.observations.is_empty(),
        "mock runner must produce at least one observation"
    );
    eprintln!(
        "[llm_experiment_flow] scenario executed: {} observations",
        result.ctx.observations.len()
    );

    // 6. Import the captured observation into the canonical store.
    let captured = result
        .ctx
        .observations
        .first()
        .expect("at least one observation");
    let observation = ObservationImport::new(
        DynamicObservation {
            observation_kind: NamespacedId::parse("debug.arguments")
                .expect("valid observation kind"),
            captured_artifact_id: exe_artifact,
            target_entity_id: function_entity.id,
            scenario_id: "llm-proposed-scenario-1".into(),
            timestamp_range: TimestampRange {
                start: Timestamp::now(),
                end: Timestamp::now(),
            },
            recorded_at: Timestamp::now(),
        },
        serde_json::to_vec(captured).expect("observation serializes"),
    );

    let mut snapshot = InMemorySnapshot::new();
    let base_input = make_fingerprint_input();
    let old_fp = compute_fingerprint(&base_input);
    snapshot.insert(function_work_item, base_input, old_fp);

    let importer = DynamicObservationImporter::new(&snapshot, &graph);
    let summary = importer
        .import(&observation, &client, project_id, campaign_id, run_id)
        .expect("observation import must succeed");

    assert_eq!(
        summary.fingerprint_comparison,
        FingerprintComparison::Changed,
        "new observation must change the function fingerprint"
    );
    assert!(
        summary.invalidated_work_items.contains(&function_work_item),
        "originating Function analysis work item must be invalidated"
    );
    assert!(
        client.commands().iter().any(|c| matches!(
            c,
            ApplicationCommand::InvalidateWorkItem(req)
            if req.work_item_id == function_work_item.to_string()
        )),
        "InvalidateWorkItem command must be issued for the function work item"
    );
    eprintln!(
        "[llm_experiment_flow] observation imported; invalidated {} work item(s)",
        summary.invalidated_work_items.len()
    );

    // 7. Failure path: LLM hallucinates an unmapped address.
    let mut invalid_scenario = scenario.clone();
    invalid_scenario.body.push(Step::CaptureMemoryRegion {
        addr: 0xDEAD_0000,
        size: 64,
    });
    let invalid_result = ScenarioVerifier::validate(
        &invalid_scenario,
        &entities_by_id,
        &mapped_segments,
        &allowed_apis,
    );
    assert!(
        matches!(
            invalid_result,
            Err(ScenarioValidationError::UnmappedAddress(0xDEAD_0000))
        ),
        "unmapped address must be rejected by verifier: {invalid_result:?}"
    );
    eprintln!("[llm_experiment_flow] verifier rejected hallucinated unmapped address");

    //    Record the rejection: FailWorkItem + BlockWorkWithReason (BlockedReason).
    client
        .execute(ApplicationCommand::FailWorkItem(FailWorkItemRequest {
            project: project_id,
            work_item_id: function_work_item.to_string(),
            reason: "UnvalidatedScenarioRejection: unmapped address 0xdead0000".into(),
        }))
        .expect("FailWorkItem for rejected scenario must succeed");
    client
        .execute(ApplicationCommand::BlockWorkWithReason(
            BlockWorkWithReasonRequest {
                project: project_id,
                reason: "UnvalidatedScenarioRejection".into(),
            },
        ))
        .expect("BlockWorkWithReason for rejected scenario must succeed");
    let fail_count = client.count(|c| {
        matches!(
            c,
            ApplicationCommand::FailWorkItem(req)
            if req.reason.contains("UnvalidatedScenarioRejection")
        )
    });
    let blocked_reason_count = client.count(|c| {
        matches!(
            c,
            ApplicationCommand::BlockWorkWithReason(req)
            if req.reason == "UnvalidatedScenarioRejection"
        )
    });
    assert!(
        fail_count >= 1,
        "FailWorkItem must be recorded for rejected scenario"
    );
    assert!(
        blocked_reason_count >= 1,
        "BlockWorkWithReason must record BlockedReason"
    );
    eprintln!("[llm_experiment_flow] recorded FailWorkItem + BlockWorkWithReason");

    // 8. Audit: every canonical mutation went through an ApplicationCommand.
    for cmd in client.commands() {
        assert!(
            matches!(
                cmd,
                ApplicationCommand::CreateWorkItems(_)
                    | ApplicationCommand::RecordWorkDependency(_)
                    | ApplicationCommand::RegisterArtifact(_)
                    | ApplicationCommand::ImportDynamicObservation(_)
                    | ApplicationCommand::AddEvidence(_)
                    | ApplicationCommand::InvalidateWorkItem(_)
                    | ApplicationCommand::FailWorkItem(_)
                    | ApplicationCommand::BlockWorkWithReason(_)
            ),
            "every mutation must be an ApplicationCommand, got: {cmd:?}"
        );
    }

    eprintln!(
        "[llm_experiment_flow] command audit passed: {} canonical mutation(s)",
        client.commands().len()
    );
    eprintln!(
        "[OK] experiment proposed, validated, scheduled, executed, observed+imported+op invalidated dependent analysis"
    );
}
