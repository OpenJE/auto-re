//! Wave 10 exit-criterion integration test: function-level + cluster-level
//! differential verification with a regression guard.
//!
//! Exercises `ProjectSkeletonBuilder`, `GenerationOrchestrator`,
//! `ScenarioExecutor`, and `RegressionTracker` with mock backends only (no real
//! Wine+GDB and no real LLM endpoint).

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use petgraph::graph::DiGraph;
use tests_support::RecordingAutoReClient;

use autore_app::application_service::requests::{
    BlockWorkItemResponse, CompleteWorkItemResponse, CreateWorkItemsResponse, FailWorkItemResponse,
    RecordBuildAttemptResponse, RecordRepairAttemptResponse, RegisterArtifactRequest,
    RegisterEntityRequest, ScheduleVerificationRegressionResponse,
};
use autore_app::{ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_core::Result;
use autore_events::project_event_service::ProjectEventSubscription;
use autore_reconstruction::build::types::{BuildDiagnostic, GeneratorManifest};
use autore_reconstruction::build::{
    BuildConfigured, BuildProviderTrait, BuildResult, CompileResult, CompileUnit, LinkResult,
    RunTestResult,
};
use autore_reconstruction::generation::orchestrator::{
    FailureAnalysisContext, FailureAnalysisResponse, GenerationContext, GenerationModelError,
    GenerationResponse, RepairGenerationContext,
};
use autore_reconstruction::generation::{
    GenerationModel, GenerationOrchestrator, OrchestratorConfig, ProjectSkeletonBuilder,
    StubPolicy, WorkItemContext, WorkItemOutcome,
};
use autore_reconstruction::verification::{
    ComparisonLevel, ComparisonResult, InitialState, NormalizationRule, Observation,
    ObservationBackend, ObservationError, ObservationSet, RegressionTracker, Scenario,
    ScenarioExecutor, ScenarioInput, VerificationComparison,
};
use autore_reconstruction::work_graph::{DependencyEdgeKind, WorkGraph, WorkItemNode};
use autore_schema::domain::records::{
    ENTITY_KIND_FUNCTION, ProjectEvent, SemanticEntity, WorkItemKind,
};
use autore_schema::domain::{ContentHash, MetadataMap, NamespacedId, Timestamp};
use autore_schema::ids::{ArtifactId, EntityId, ProjectId, ReconstructionCampaignId, WorkItemId};

// ---------------------------------------------------------------------------
// Test client: extends RecordingAutoReClient with Stage-1 lifecycle commands.
// ---------------------------------------------------------------------------

struct TestClient {
    inner: RecordingAutoReClient,
    commands: Mutex<Vec<ApplicationCommand>>,
}

impl TestClient {
    fn new() -> Self {
        Self {
            inner: RecordingAutoReClient::new(),
            commands: Mutex::new(Vec::new()),
        }
    }

    fn commands(&self) -> Vec<ApplicationCommand> {
        self.commands.lock().unwrap().clone()
    }

    fn count<F: Fn(&ApplicationCommand) -> bool>(&self, pred: F) -> usize {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .filter(|c| pred(c))
            .count()
    }
}

impl AutoReClient for TestClient {
    fn execute(&self, command: ApplicationCommand) -> Result<CommandResult> {
        let result = match &command {
            ApplicationCommand::RecordBuildAttempt(_) => Ok(CommandResult::BuildAttemptRecorded(
                RecordBuildAttemptResponse {
                    attempt_id: uuid::Uuid::now_v7().to_string(),
                },
            )),
            ApplicationCommand::RecordRepairAttempt(_) => Ok(CommandResult::RepairAttemptRecorded(
                RecordRepairAttemptResponse {
                    repair_id: uuid::Uuid::now_v7().to_string(),
                },
            )),
            ApplicationCommand::CompleteWorkItem(req) => {
                Ok(CommandResult::WorkItemCompleted(CompleteWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                }))
            }
            ApplicationCommand::BlockWorkItem(req) => {
                Ok(CommandResult::WorkItemBlocked(BlockWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                }))
            }
            ApplicationCommand::FailWorkItem(req) => {
                Ok(CommandResult::WorkItemFailed(FailWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                }))
            }
            ApplicationCommand::CreateWorkItems(req) => {
                Ok(CommandResult::WorkItemsCreated(CreateWorkItemsResponse {
                    work_item_ids: req
                        .descriptions
                        .iter()
                        .map(|_| uuid::Uuid::now_v7().to_string())
                        .collect(),
                }))
            }
            ApplicationCommand::RecordVerificationComparison(_) => {
                Ok(CommandResult::VerificationComparisonRecorded(
                    autore_app::application_service::requests::RecordVerificationComparisonResponse {
                        comparison_id: uuid::Uuid::now_v7().to_string(),
                    },
                ))
            }
            ApplicationCommand::ScheduleVerificationRegression(_) => {
                Ok(CommandResult::VerificationRegressionScheduled(
                    ScheduleVerificationRegressionResponse {
                        regression_id: uuid::Uuid::now_v7().to_string(),
                    },
                ))
            }
            _ => self.inner.execute(command.clone()),
        };
        self.commands.lock().unwrap().push(command);
        result
    }

    fn query(&self, query: ApplicationQuery) -> Result<QueryResult> {
        self.inner.query(query)
    }

    fn events_after(
        &self,
        project: ProjectId,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<ProjectEvent>> {
        self.inner.events_after(project, sequence, limit)
    }

    fn subscribe_events(&self, project: ProjectId, after: u64) -> Result<ProjectEventSubscription> {
        self.inner.subscribe_events(project, after)
    }
}

// ---------------------------------------------------------------------------
// Mock generation model
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct MockGenerationModel {
    function_responses: Mutex<HashMap<String, GenerationResponse>>,
    invocations: Mutex<Vec<String>>,
}

impl MockGenerationModel {
    fn with_functions(responses: &[(&str, GenerationResponse)]) -> Self {
        let mut map = HashMap::new();
        for (id, resp) in responses {
            map.insert(id.to_string(), resp.clone());
        }
        Self {
            function_responses: Mutex::new(map),
            invocations: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl GenerationModel for MockGenerationModel {
    async fn generate_function(
        &self,
        ctx: &GenerationContext,
    ) -> std::result::Result<GenerationResponse, GenerationModelError> {
        self.invocations
            .lock()
            .unwrap()
            .push(ctx.work_item_id.clone());
        let guard = self.function_responses.lock().unwrap();
        guard
            .get(&ctx.work_item_id)
            .cloned()
            .ok_or_else(|| GenerationModelError::Other("no mock response".into()))
    }

    async fn generate_cluster(
        &self,
        _ctx: &GenerationContext,
    ) -> std::result::Result<GenerationResponse, GenerationModelError> {
        Err(GenerationModelError::Other("cluster not mocked".into()))
    }

    async fn analyze_failure(
        &self,
        _ctx: &FailureAnalysisContext,
    ) -> std::result::Result<FailureAnalysisResponse, GenerationModelError> {
        Ok(FailureAnalysisResponse {
            diagnosis: "analyzed".into(),
        })
    }

    async fn generate_repair(
        &self,
        _ctx: &RepairGenerationContext,
    ) -> std::result::Result<GenerationResponse, GenerationModelError> {
        Err(GenerationModelError::Other("repair not mocked".into()))
    }
}

// ---------------------------------------------------------------------------
// Fixture-aware build provider: always green for the small fixture.
// ---------------------------------------------------------------------------

struct FixtureBuildProvider {
    current_diagnostics: Mutex<Vec<BuildDiagnostic>>,
}

impl FixtureBuildProvider {
    fn new() -> Self {
        Self {
            current_diagnostics: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl BuildProviderTrait for FixtureBuildProvider {
    async fn configure_project(
        &self,
        _manifest: &GeneratorManifest,
        _project_root: &Path,
    ) -> BuildResult<BuildConfigured> {
        Ok(BuildConfigured {
            build_dir: PathBuf::from("build"),
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn compile_units(&self, _units: &[CompileUnit]) -> BuildResult<CompileResult> {
        *self.current_diagnostics.lock().unwrap() = Vec::new();
        Ok(CompileResult {
            objects: Vec::new(),
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn link_target(&self, _target_artifacts: &[PathBuf]) -> BuildResult<LinkResult> {
        Ok(LinkResult {
            executable: PathBuf::from("build/reconstruction.exe"),
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn run_test(&self, _test_target: &str) -> BuildResult<RunTestResult> {
        Ok(RunTestResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    async fn collect_diagnostics(
        &self,
        _build_logs: &autore_reconstruction::build::BuildLogs,
    ) -> BuildResult<Vec<BuildDiagnostic>> {
        Ok(self.current_diagnostics.lock().unwrap().clone())
    }
}

// ---------------------------------------------------------------------------
// Mock observation backend: returns canned observations for original/candidate.
// ---------------------------------------------------------------------------

struct MockObservationBackend {
    original: ObservationSet,
    candidate: ObservationSet,
}

#[async_trait]
impl ObservationBackend for MockObservationBackend {
    async fn capture(
        &self,
        scenario: &Scenario,
        target_artifact_id: ArtifactId,
    ) -> std::result::Result<ObservationSet, ObservationError> {
        if target_artifact_id == scenario.executable_artifact_id {
            Ok(self.original.clone())
        } else {
            Ok(self.candidate.clone())
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello")
}

fn make_entity(project: ProjectId, kind: NamespacedId, name: &str) -> SemanticEntity {
    SemanticEntity {
        id: EntityId::new(),
        project,
        kind,
        stable_key: None,
        display_name: Some(name.into()),
        created_at: Timestamp::now(),
        metadata: MetadataMap::new(),
    }
}

fn register_entity(
    client: &dyn AutoReClient,
    project: ProjectId,
    kind: &NamespacedId,
    name: &str,
) -> EntityId {
    let req = RegisterEntityRequest {
        project,
        kind: kind.to_string(),
        stable_key: None,
        display_name: Some(name.into()),
    };
    match client
        .execute(ApplicationCommand::RegisterEntity(req))
        .expect("RegisterEntity must succeed")
    {
        CommandResult::EntityRegistered(resp) => resp.entity.id,
        other => panic!("expected EntityRegistered, got {other:?}"),
    }
}

fn register_artifact(
    client: &dyn AutoReClient,
    project: ProjectId,
    source_path: impl Into<PathBuf>,
    kind: &str,
) -> ArtifactId {
    let req = RegisterArtifactRequest {
        project,
        source_path: source_path.into(),
        kind: kind.to_string(),
    };
    match client
        .execute(ApplicationCommand::RegisterArtifact(req))
        .expect("RegisterArtifact must succeed")
    {
        CommandResult::ArtifactRegistered(resp) => resp.artifact.id,
        other => panic!("expected ArtifactRegistered, got {other:?}"),
    }
}

fn entity_id_to_relpath(entity_id: &EntityId) -> PathBuf {
    let hex = entity_id.as_uuid().as_simple().to_string();
    PathBuf::from(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex[4..6])
        .join(&hex)
}

fn entity_cpp_relpath(entity_id: &EntityId) -> PathBuf {
    PathBuf::from("src/generated")
        .join(entity_id_to_relpath(entity_id))
        .with_extension("cpp")
}

fn candidate_response_for(entity_id: EntityId, body: &[u8]) -> GenerationResponse {
    GenerationResponse {
        relative_path: entity_cpp_relpath(&entity_id),
        candidate_bytes: body.to_vec(),
    }
}

fn debug_kind(name: &str) -> NamespacedId {
    NamespacedId::parse(&format!("debug.{name}")).unwrap()
}

fn observation_set(
    scenario_id: impl Into<String>,
    target_artifact_id: ArtifactId,
    entity_id: EntityId,
    rax: i64,
) -> ObservationSet {
    ObservationSet::new(scenario_id, target_artifact_id)
        .add_observation(
            Observation::new(debug_kind("register"), serde_json::json!({"rax": rax}))
                .with_entity(entity_id),
        )
        .with_exit_code(0)
}

fn build_work_graph(
    entities: &[EntityId],
    edges: &[(usize, usize, DependencyEdgeKind)],
) -> WorkGraph {
    let mut graph = DiGraph::new();
    let mut entity_to_node = HashMap::new();
    let mut work_item_to_node = HashMap::new();

    for entity_id in entities {
        let work_item_id = WorkItemId::new();
        let idx = graph.add_node(WorkItemNode {
            work_item_id,
            kind: WorkItemKind::Function,
            entity_id: Some(*entity_id),
        });
        entity_to_node.insert(*entity_id, idx);
        work_item_to_node.insert(work_item_id, idx);
    }

    for (src, tgt, kind) in edges {
        let src_idx = entity_to_node[&entities[*src]];
        let tgt_idx = entity_to_node[&entities[*tgt]];
        graph.add_edge(src_idx, tgt_idx, *kind);
    }

    WorkGraph {
        graph,
        entity_to_node,
        work_item_to_node,
    }
}

fn assert_passes(comparison: &VerificationComparison) {
    assert!(
        matches!(
            comparison.overall,
            ComparisonResult::Equal | ComparisonResult::EquivalentUnderNormalization
        ),
        "comparison must pass, got {:?}",
        comparison.overall
    );
    assert!(
        !comparison.execution_failed(),
        "comparison must not report execution failure"
    );
}

// ---------------------------------------------------------------------------
// Wave 10 differential verification end-to-end test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn wave10_function_and_cluster_differential_verification() {
    eprintln!("[wave10_differential] bootstrapping temp project + fixture binary");

    let tmp = tempfile::tempdir().expect("temp dir");
    let project = ProjectId::new();
    let campaign = ReconstructionCampaignId::new();
    let client = Arc::new(TestClient::new());

    // 1. Register the small fixture binary as an artifact.
    let binary_path = fixture_binary_path();
    assert!(binary_path.exists(), "fixture binary must exist");
    let binary_artifact_id = register_artifact(&*client, project, binary_path, "core.binary");

    // 2. Register canonical function entities f_a and f_b with a call dependency.
    let f_b = register_entity(&*client, project, &ENTITY_KIND_FUNCTION, "f_b");
    let f_a = register_entity(&*client, project, &ENTITY_KIND_FUNCTION, "f_a");
    eprintln!("[wave10_differential] entities: f_b={f_b}, f_a={f_a}");

    // 3. Generate the project skeleton so functions start as stubs.
    let mut skeleton_builder =
        ProjectSkeletonBuilder::new(tmp.path().to_path_buf(), project, &*client)
            .with_policy(StubPolicy::EmptyBody);
    skeleton_builder.add_entity(&{
        let mut e = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_b");
        e.id = f_b;
        e
    });
    skeleton_builder.add_entity(&{
        let mut e = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_a");
        e.id = f_a;
        e
    });
    let manifest = skeleton_builder
        .build()
        .expect("skeleton build must succeed");
    assert_eq!(manifest.entity_count, 2);

    for entity in [f_b, f_a] {
        let cpp = tmp.path().join(entity_cpp_relpath(&entity));
        let content = std::fs::read_to_string(&cpp).expect("stub cpp readable");
        assert!(
            content.contains(r#"reconstruction_status = "stubbed""#),
            "entity must start stubbed"
        );
    }

    // 4. Replace stubs with candidate implementations using the generation orchestrator.
    let wi_f_b = WorkItemContext {
        work_item_id: "f_b".into(),
        kind: WorkItemKind::Function,
        subject_entity: Some(f_b),
        dependencies: vec![],
        cluster_members: None,
    };
    let wi_f_a = WorkItemContext {
        work_item_id: "f_a".into(),
        kind: WorkItemKind::Function,
        subject_entity: Some(f_a),
        dependencies: vec!["f_b".into()],
        cluster_members: None,
    };
    let all_work_items = vec![wi_f_a.clone(), wi_f_b.clone()];
    let mut stubbed: HashSet<String> = ["f_a", "f_b"].iter().map(|s| s.to_string()).collect();

    let f_b_body = candidate_response_for(f_b, b"int f_b(int x) { return x + 1; }\n");
    let f_a_body = candidate_response_for(f_a, b"int f_a(int x) { return f_b(x) + 1; }\n");
    let model = MockGenerationModel::with_functions(&[("f_b", f_b_body), ("f_a", f_a_body)]);
    let provider = FixtureBuildProvider::new();
    let mut orchestrator = GenerationOrchestrator::new(
        tmp.path().to_path_buf(),
        project,
        campaign.to_string(),
        &provider,
        &*client,
        &model,
        OrchestratorConfig::default(),
    );

    eprintln!("[wave10_differential] running leaf-first stub replacement");
    let mut completion_order = Vec::new();
    while !stubbed.is_empty() {
        let outcome = orchestrator
            .process_next_work_item(&all_work_items, &stubbed)
            .await
            .expect("process_next_work_item must not error");
        if outcome == WorkItemOutcome::NoWork {
            break;
        }
        if outcome == WorkItemOutcome::Completed {
            let completed_id = client
                .commands()
                .iter()
                .rev()
                .find_map(|c| match c {
                    ApplicationCommand::CompleteWorkItem(req) => Some(req.work_item_id.clone()),
                    _ => None,
                })
                .expect("Completed outcome must issue CompleteWorkItem");
            stubbed.remove(&completed_id);
            completion_order.push(completed_id);
        }
    }

    eprintln!("[wave10_differential] completion order: {completion_order:?}");
    assert_eq!(
        completion_order.len(),
        2,
        "both functions must be completed"
    );
    let f_a_pos = completion_order
        .iter()
        .position(|x| x == "f_a")
        .expect("f_a completed");
    let f_b_pos = completion_order
        .iter()
        .position(|x| x == "f_b")
        .expect("f_b completed");
    assert!(
        f_a_pos > f_b_pos,
        "f_a must complete after f_b (downstream unblocked)"
    );

    for (entity, name) in [(f_b, "f_b"), (f_a, "f_a")] {
        let cpp = tmp.path().join(entity_cpp_relpath(&entity));
        let content = std::fs::read_to_string(&cpp).expect("{name} cpp readable");
        assert!(
            !content.contains(r#"reconstruction_status = "stubbed""#),
            "{name} must be replaced"
        );
    }

    // 5. Build scenarios: function-level for f_a and cluster-level for f_a+f_b.
    let initial_state = InitialState::new(HashMap::new(), vec![], tmp.path()).with_seed(42);
    let input_seed = ScenarioInput::new(
        NamespacedId::parse("verify.input.stdin").unwrap(),
        serde_json::json!({"seed": "X"}),
    );

    let function_scenario = Scenario::new(
        "wave10-f_a-function-seed-x",
        "f_a",
        f_a,
        initial_state.clone(),
        binary_artifact_id,
        ArtifactId::new(),
        vec![],
        ComparisonLevel::Function,
    )
    .add_input(input_seed.clone())
    .add_normalization_rule(NormalizationRule::RandomSeed { placeholder: 0 });

    let cluster_scenario = Scenario::new(
        "wave10-f_a-f_b-cluster-seed-x",
        "cluster-f_a-f_b",
        f_a,
        initial_state,
        binary_artifact_id,
        ArtifactId::new(),
        vec![],
        ComparisonLevel::Cluster,
    )
    .add_input(input_seed)
    .add_normalization_rule(NormalizationRule::RandomSeed { placeholder: 0 });

    // 6. Capture original and candidate observations with the same mock backend.
    let original_func = observation_set(
        function_scenario.id.clone(),
        function_scenario.executable_artifact_id,
        f_a,
        42,
    );
    let candidate_func = observation_set(
        function_scenario.id.clone(),
        function_scenario.candidate_artifact_id,
        f_a,
        42,
    );
    let backend = Arc::new(MockObservationBackend {
        original: original_func,
        candidate: candidate_func,
    });

    let client_arc: Arc<dyn AutoReClient> = client.clone();
    let executor = ScenarioExecutor::new(project, client_arc.clone(), backend);

    // 7. Function-level comparison.
    eprintln!("[wave10_differential] running function-level comparison");
    let original_obs = executor
        .execute_original(&function_scenario)
        .await
        .expect("original execution must succeed");
    let candidate_obs = executor
        .execute_candidate(&function_scenario, function_scenario.candidate_artifact_id)
        .await
        .expect("candidate execution must succeed");
    let function_comparison = executor
        .compare_and_record(&function_scenario, &original_obs, &candidate_obs)
        .await
        .expect("function comparison must succeed");
    assert_passes(&function_comparison);

    // 8. Cluster-level comparison.
    eprintln!("[wave10_differential] running cluster-level comparison");
    let original_cluster = observation_set(
        cluster_scenario.id.clone(),
        cluster_scenario.executable_artifact_id,
        f_a,
        84,
    );
    let candidate_cluster = observation_set(
        cluster_scenario.id.clone(),
        cluster_scenario.candidate_artifact_id,
        f_a,
        84,
    );
    let cluster_backend = Arc::new(MockObservationBackend {
        original: original_cluster,
        candidate: candidate_cluster,
    });
    let cluster_executor = ScenarioExecutor::new(project, client_arc.clone(), cluster_backend);

    let original_cluster_obs = cluster_executor
        .execute_original(&cluster_scenario)
        .await
        .expect("original cluster execution must succeed");
    let candidate_cluster_obs = cluster_executor
        .execute_candidate(&cluster_scenario, cluster_scenario.candidate_artifact_id)
        .await
        .expect("candidate cluster execution must succeed");
    let cluster_comparison = cluster_executor
        .compare_and_record(
            &cluster_scenario,
            &original_cluster_obs,
            &candidate_cluster_obs,
        )
        .await
        .expect("cluster comparison must succeed");
    assert_passes(&cluster_comparison);

    // 9. Force a regression: replace f_b with a different candidate implementation
    //    (simulating a stub-to-candidate replacement that changes callee behavior),
    //    then use the regression tracker to identify and schedule re-verification
    //    of the dependent f_a.
    eprintln!("[wave10_differential] forcing regression on f_b and re-verifying f_a");
    let f_b_cpp = tmp.path().join(entity_cpp_relpath(&f_b));
    std::fs::write(&f_b_cpp, b"int f_b(int x) { return x + 2; }\n")
        .expect("write changed f_b candidate");

    let graph = build_work_graph(&[f_a, f_b], &[(0, 1, DependencyEdgeKind::BuildDependency)]);

    let mut tracker = RegressionTracker::new();
    let mut fingerprints = HashMap::new();
    fingerprints.insert(f_b.to_string(), ContentHash::from_bytes(b"old-fp"));
    tracker.register_verification(
        f_a,
        vec![function_scenario.id.clone()],
        fingerprints,
        vec!["shared_type".into()],
        "debug".into(),
    );

    let affected = tracker.compute_affected_entities(f_b, &graph);
    assert!(
        affected.contains(&f_a),
        "f_a must be affected when f_b changes"
    );

    let regression_ids = tracker
        .schedule_regressions(&*client_arc, project, &affected)
        .expect("schedule regressions must succeed");
    assert!(
        !regression_ids.is_empty(),
        "at least one regression must be scheduled"
    );

    let schedule_count =
        client.count(|c| matches!(c, ApplicationCommand::ScheduleVerificationRegression(_)));
    assert_eq!(
        schedule_count, 1,
        "exactly one ScheduleVerificationRegression expected"
    );

    // 10. Re-run verification for the affected entity with consistent mock backend.
    let rerun_scenario = function_scenario.clone();
    let rerun_original = executor
        .execute_original(&rerun_scenario)
        .await
        .expect("regression original execution must succeed");
    let rerun_candidate = executor
        .execute_candidate(&rerun_scenario, rerun_scenario.candidate_artifact_id)
        .await
        .expect("regression candidate execution must succeed");
    let rerun_comparison = executor
        .compare_and_record(&rerun_scenario, &rerun_original, &rerun_candidate)
        .await
        .expect("regression comparison must succeed");
    assert_passes(&rerun_comparison);

    // 11. Audit: every mutation flowed through an ApplicationCommand variant.
    for cmd in client.commands() {
        assert!(
            matches!(
                cmd,
                ApplicationCommand::RegisterArtifact(_)
                    | ApplicationCommand::RegisterEntity(_)
                    | ApplicationCommand::RegisterGeneratedSourceMapping(_)
                    | ApplicationCommand::RecordBuildAttempt(_)
                    | ApplicationCommand::ImportGeneratedSourceCandidates(_)
                    | ApplicationCommand::CompleteWorkItem(_)
                    | ApplicationCommand::ImportDynamicObservation(_)
                    | ApplicationCommand::RecordVerificationComparison(_)
                    | ApplicationCommand::ScheduleVerificationRegression(_)
            ),
            "every canonical mutation must be an ApplicationCommand variant, got: {cmd:?}"
        );
    }

    println!("[OK] function-verified + cluster-verified + regression-passed");
}

// Extension helper to make the assertion API read naturally.
trait ComparisonExt {
    fn execution_failed(&self) -> bool;
}

impl ComparisonExt for VerificationComparison {
    fn execution_failed(&self) -> bool {
        self.overall == ComparisonResult::ExecutionFailed || self.counts.execution_failed_count > 0
    }
}
