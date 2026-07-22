//! Wave 9 exit-criterion integration test: progressive stub→replaced
//! transitions for three functions with leaf-first dependency ordering.
//!
//! Exercises `GenerationOrchestrator` + `PatchPipeline` with a mock LLM and a
//! mock build provider that fails an early dispatch of `f_a` while `f_b` is
//! still stubbed, then succeeds once the leaf-first order is respected.
//!
//! Every canonical mutation is recorded by an `ApplicationCommand` variant.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use tests_support::RecordingAutoReClient;

use autore_app::application_service::requests::{
    BlockWorkItemResponse, CompleteWorkItemResponse, CreateWorkItemsResponse, FailWorkItemResponse,
    RecordBuildAttemptResponse, RecordRepairAttemptResponse, RegisterArtifactRequest,
    RegisterEntityRequest,
};
use autore_app::{ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_core::Result;
use autore_events::project_event_service::ProjectEventSubscription;
use autore_reconstruction::build::types::BuildDiagnostic;
use autore_reconstruction::build::{
    BuildConfigured, BuildLogs, BuildProviderTrait, BuildResult, CompileResult, CompileUnit,
    DiagnosticSeverity, DockerMsvc2002BuildProvider, DockerMsvc2002Config, GeneratorManifest,
    LinkResult, RunTestResult, SuggestedWorkKind,
};
use autore_reconstruction::generation::orchestrator::{
    FailureAnalysisContext, FailureAnalysisResponse, GenerationContext, GenerationModelError,
    GenerationResponse, RepairGenerationContext,
};
use autore_reconstruction::generation::{
    GenerationModel, GenerationOrchestrator, OrchestratorConfig, ProjectSkeletonBuilder,
    StubPolicy, WorkItemContext, WorkItemOutcome,
};
use autore_schema::domain::records::{
    ENTITY_KIND_FUNCTION, ENTITY_KIND_GLOBAL, ProjectEvent, SemanticEntity,
};
use autore_schema::domain::{MetadataMap, NamespacedId, Timestamp};
use autore_schema::ids::{EntityId, ProjectId, ReconstructionCampaignId};

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

    fn last_command_matching<F: Fn(&ApplicationCommand) -> Option<String>>(
        &self,
        pred: F,
    ) -> Option<String> {
        self.commands().iter().rev().find_map(pred)
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
            ApplicationCommand::RegisterArtifact(req) => Ok(CommandResult::ArtifactRegistered(
                autore_app::application_service::requests::RegisterArtifactResponse {
                    artifact: autore_schema::domain::records::Artifact {
                        id: autore_schema::ids::ArtifactId::new(),
                        project: req.project,
                        kind: NamespacedId::parse(&req.kind)
                            .map_err(|e| autore_core::Error::Validation(e.0))?,
                        content_hash: autore_schema::domain::ContentHash::sha256(
                            b"recording-client-stub",
                        ),
                        size: 0,
                        storage: autore_schema::domain::records::ArtifactStorage::ManagedBlob {
                            relative_path: req.source_path.clone(),
                        },
                        created_at: Timestamp::now(),
                        metadata: MetadataMap::new(),
                    },
                },
            )),
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
    fn with_function(id: &str, response: GenerationResponse) -> Self {
        let mut map = HashMap::new();
        map.insert(id.to_string(), response);
        Self {
            function_responses: Mutex::new(map),
            invocations: Mutex::new(Vec::new()),
        }
    }

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
// Fixture-aware build provider
// ---------------------------------------------------------------------------

struct FixtureBuildProvider {
    output_root: PathBuf,
    f_b_entity: EntityId,
    current_diagnostics: Mutex<Vec<BuildDiagnostic>>,
}

impl FixtureBuildProvider {
    fn new(output_root: PathBuf, f_b_entity: EntityId) -> Self {
        Self {
            output_root,
            f_b_entity,
            current_diagnostics: Mutex::new(Vec::new()),
        }
    }

    fn f_b_still_stubbed(&self) -> bool {
        let path = self.output_root.join(entity_cpp_relpath(&self.f_b_entity));
        if !path.exists() {
            return true;
        }
        std::fs::read_to_string(&path)
            .map(|s| s.contains(r#"reconstruction_status = "stubbed""#))
            .unwrap_or(true)
    }

    fn any_source_references_f_b(&self) -> bool {
        let generated_dir = self.output_root.join("src/generated");
        if !generated_dir.exists() {
            return false;
        }
        Self::collect_cpp_files(&generated_dir)
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .any(|content| content.contains("f_b()"))
    }

    fn collect_cpp_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    out.extend(Self::collect_cpp_files(&path));
                } else if path.extension().is_some_and(|e| e == "cpp") {
                    out.push(path);
                }
            }
        }
        out
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
        if self.any_source_references_f_b() && self.f_b_still_stubbed() {
            let diag = BuildDiagnostic {
                diagnostic_code: "C2065".into(),
                severity: DiagnosticSeverity::Error,
                file_path: PathBuf::from("src/generated/f_a.cpp"),
                line: 1,
                column: 0,
                message: "'f_b' : undeclared identifier".into(),
                candidate_cause: "f_b is still stubbed".into(),
                suggested_work_kind: SuggestedWorkKind::MissingDeclaration,
            };
            *self.current_diagnostics.lock().unwrap() = vec![diag];
            Ok(CompileResult {
                objects: Vec::new(),
                success: false,
                stdout: String::new(),
                stderr: "'f_b' : undeclared identifier".into(),
            })
        } else {
            *self.current_diagnostics.lock().unwrap() = Vec::new();
            Ok(CompileResult {
                objects: Vec::new(),
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
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
        _build_logs: &BuildLogs,
    ) -> BuildResult<Vec<BuildDiagnostic>> {
        Ok(self.current_diagnostics.lock().unwrap().clone())
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

fn entity_hpp_relpath(entity_id: &EntityId) -> PathBuf {
    PathBuf::from("include/recovered")
        .join(entity_id_to_relpath(entity_id))
        .with_extension("hpp")
}

fn candidate_response_for(entity_id: EntityId, body: &[u8]) -> GenerationResponse {
    GenerationResponse {
        relative_path: entity_cpp_relpath(&entity_id),
        candidate_bytes: body.to_vec(),
    }
}

fn build_orchestrator<'a>(
    tmp: &'a tempfile::TempDir,
    project: ProjectId,
    campaign_id: String,
    client: &'a TestClient,
    model: &'a MockGenerationModel,
    provider: &'a FixtureBuildProvider,
) -> GenerationOrchestrator<'a> {
    GenerationOrchestrator::new(
        tmp.path().to_path_buf(),
        project,
        campaign_id,
        provider,
        client,
        model,
        OrchestratorConfig::default(),
    )
}

// ---------------------------------------------------------------------------
// Wave 9 stub-replacement end-to-end test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn wave9_stub_replacement_leaf_first() {
    eprintln!("[wave9_stub_replacement] bootstrapping temp project + fixture binary");

    let tmp = tempfile::tempdir().expect("temp dir");
    let project = ProjectId::new();
    let campaign = ReconstructionCampaignId::new();
    let campaign_id = campaign.to_string();
    let client = TestClient::new();

    // 1. Register the small fixture binary as an artifact.
    let binary_path = fixture_binary_path();
    assert!(binary_path.exists(), "fixture binary must exist");
    client
        .execute(ApplicationCommand::RegisterArtifact(
            RegisterArtifactRequest {
                project,
                source_path: binary_path,
                kind: "core.binary".into(),
            },
        ))
        .expect("RegisterArtifact for fixture binary must succeed");

    // 2. Register canonical entities: one global + three functions.
    let global_entity = register_entity(&client, project, &ENTITY_KIND_GLOBAL, "RUNTIME_DATA");
    let f_b = register_entity(&client, project, &ENTITY_KIND_FUNCTION, "f_b");
    let f_a = register_entity(&client, project, &ENTITY_KIND_FUNCTION, "f_a");
    let f_c = register_entity(&client, project, &ENTITY_KIND_FUNCTION, "f_c");

    eprintln!(
        "[wave9_stub_replacement] entities: global={global_entity}, f_b={f_b}, f_a={f_a}, f_c={f_c}"
    );

    // 3. Generate the project skeleton with empty-body stubs so the initial
    //    tree can at least be fed through the build provider.
    let mut skeleton_builder =
        ProjectSkeletonBuilder::new(tmp.path().to_path_buf(), project, &client)
            .with_policy(StubPolicy::EmptyBody);
    skeleton_builder.add_entity(&{
        let mut e = make_entity(project, ENTITY_KIND_GLOBAL.clone(), "RUNTIME_DATA");
        e.id = global_entity;
        e
    });
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
    skeleton_builder.add_entity(&{
        let mut e = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_c");
        e.id = f_c;
        e
    });

    let manifest = skeleton_builder
        .build()
        .expect("skeleton build must succeed");
    assert_eq!(manifest.entity_count, 4);

    // 4. "Settle" the global: replace its stub with the recovered declaration
    //    and definition so function bodies can reference RUNTIME_DATA.
    let global_hpp = tmp.path().join(entity_hpp_relpath(&global_entity));
    let global_cpp = tmp.path().join(entity_cpp_relpath(&global_entity));
    std::fs::write(&global_hpp, "extern int RUNTIME_DATA[1];\n")
        .expect("write settled global header");
    std::fs::write(&global_cpp, "int RUNTIME_DATA[1] = { 42 };\n")
        .expect("write settled global definition");

    // 5. Build work-item contexts for the three functions.
    let wi_f_c = WorkItemContext {
        work_item_id: "f_c".into(),
        kind: autore_schema::domain::records::WorkItemKind::Function,
        subject_entity: Some(f_c),
        dependencies: vec![],
        cluster_members: None,
    };
    let wi_f_b = WorkItemContext {
        work_item_id: "f_b".into(),
        kind: autore_schema::domain::records::WorkItemKind::Function,
        subject_entity: Some(f_b),
        dependencies: vec!["global-runtime-data".into()],
        cluster_members: None,
    };
    let wi_f_a = WorkItemContext {
        work_item_id: "f_a".into(),
        kind: autore_schema::domain::records::WorkItemKind::Function,
        subject_entity: Some(f_a),
        dependencies: vec!["f_b".into()],
        cluster_members: None,
    };

    let all_work_items = vec![wi_f_a.clone(), wi_f_b.clone(), wi_f_c.clone()];
    let mut stubbed: HashSet<String> = ["f_a", "f_b", "f_c"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let provider = FixtureBuildProvider::new(tmp.path().to_path_buf(), f_b);

    // 6. Assert f_a is NOT dispatched while f_b is still stubbed.
    eprintln!("[wave9_stub_replacement] phase A: f_a blocked while f_b stubbed");
    {
        let model = MockGenerationModel::default();
        let mut orchestrator = build_orchestrator(
            &tmp,
            project,
            campaign_id.clone(),
            &client,
            &model,
            &provider,
        );
        let outcome = orchestrator
            .process_next_work_item(std::slice::from_ref(&wi_f_a), &stubbed)
            .await
            .expect("process_next_work_item must not error");
        assert_eq!(
            outcome,
            WorkItemOutcome::NoWork,
            "f_a must not dispatch while f_b is stubbed"
        );
        assert_eq!(
            model.invocations.lock().unwrap().len(),
            0,
            "no LLM call must be made for a blocked work item"
        );
    }

    // 7. Demonstrate the failure mode if f_a were dispatched early:
    //    build fails with MissingDeclaration (C2065) and creates a work item.
    eprintln!(
        "[wave9_stub_replacement] phase B: forced early dispatch fails with MissingDeclaration"
    );
    {
        let f_a_body = candidate_response_for(f_a, b"int f_a() { return f_b(); }\n");
        let model = MockGenerationModel::with_function("f_a", f_a_body);
        let mut orchestrator = build_orchestrator(
            &tmp,
            project,
            campaign_id.clone(),
            &client,
            &model,
            &provider,
        );
        let wi_f_a_unblocked = WorkItemContext {
            dependencies: vec![],
            ..wi_f_a.clone()
        };
        let outcome = orchestrator
            .process_next_work_item(&[wi_f_a_unblocked], &stubbed)
            .await
            .expect("process_next_work_item must not error");
        assert_eq!(
            outcome,
            WorkItemOutcome::RepairDeferred,
            "early f_a dispatch must be repair-deferred due to MissingDeclaration"
        );

        let create_work_items: Vec<_> = client
            .commands()
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)))
            .cloned()
            .collect();
        assert!(
            !create_work_items.is_empty(),
            "MissingDeclaration must create a work item"
        );
        let last = create_work_items.last().expect("at least one");
        if let ApplicationCommand::CreateWorkItems(req) = last {
            assert!(
                req.descriptions
                    .iter()
                    .any(|d| d.contains("missing_declaration")),
                "work item description must indicate missing_declaration: {:?}",
                req.descriptions
            );
        } else {
            panic!("expected CreateWorkItems");
        }

        // The failed patch should be rolled back; f_a's .cpp must still be stubbed.
        let f_a_cpp = tmp.path().join(entity_cpp_relpath(&f_a));
        let f_a_content = std::fs::read_to_string(&f_a_cpp).expect("f_a cpp readable");
        assert!(
            f_a_content.contains(r#"reconstruction_status = "stubbed""#),
            "f_a must remain stubbed after rollback"
        );
    }

    // 8. Run the happy leaf-first replacement path.
    eprintln!("[wave9_stub_replacement] phase C: leaf-first replacement of f_c, f_b, f_a");
    let f_c_body = candidate_response_for(f_c, b"int f_c(int x) { return x; }\n");
    let f_b_body = candidate_response_for(f_b, b"int f_b() { return RUNTIME_DATA[0]; }\n");
    let f_a_body = candidate_response_for(f_a, b"int f_a() { return f_b(); }\n");
    let model = MockGenerationModel::with_functions(&[
        ("f_c", f_c_body),
        ("f_b", f_b_body),
        ("f_a", f_a_body),
    ]);
    let mut orchestrator = build_orchestrator(
        &tmp,
        project,
        campaign_id.clone(),
        &client,
        &model,
        &provider,
    );

    let mut remaining_items = all_work_items.clone();
    let mut completion_order = Vec::new();
    while !remaining_items.is_empty() {
        let outcome = orchestrator
            .process_next_work_item(&remaining_items, &stubbed)
            .await
            .expect("process_next_work_item must not error");
        if outcome == WorkItemOutcome::NoWork {
            break;
        }
        if outcome == WorkItemOutcome::Completed {
            let completed_id = client
                .last_command_matching(|c| match c {
                    ApplicationCommand::CompleteWorkItem(req) => Some(req.work_item_id.clone()),
                    _ => None,
                })
                .expect("Completed outcome must issue CompleteWorkItem");
            remaining_items.retain(|w| w.work_item_id != completed_id);
            stubbed.remove(&completed_id);
            completion_order.push(completed_id);
        }
    }

    eprintln!("[wave9_stub_replacement] completion order: {completion_order:?}");

    assert_eq!(
        completion_order.len(),
        3,
        "all three functions must be completed"
    );
    let f_a_pos = completion_order
        .iter()
        .position(|x| x == "f_a")
        .expect("f_a completed");
    let f_b_pos = completion_order
        .iter()
        .position(|x| x == "f_b")
        .expect("f_b completed");
    let _f_c_pos = completion_order
        .iter()
        .position(|x| x == "f_c")
        .expect("f_c completed");
    assert!(
        f_a_pos > f_b_pos,
        "f_a must complete after f_b (downstream unblocked)"
    );
    assert!(
        f_a_pos != 0,
        "f_a must not be the first function dispatched"
    );

    // 9. Assert post-conditions: each function's .cpp is replaced, and the
    //    canonical commands for replacement were issued.
    for (entity, name) in [(f_c, "f_c"), (f_b, "f_b"), (f_a, "f_a")] {
        let cpp = tmp.path().join(entity_cpp_relpath(&entity));
        let content = std::fs::read_to_string(&cpp).expect("{name} cpp readable");
        assert!(
            !content.contains(r#"reconstruction_status = "stubbed""#),
            "{name} must be replaced"
        );
    }

    let complete_count = client.count(|c| matches!(c, ApplicationCommand::CompleteWorkItem(_)));
    let mapping_count =
        client.count(|c| matches!(c, ApplicationCommand::RegisterGeneratedSourceMapping(_)));
    let artifact_count = client.count(|c| matches!(c, ApplicationCommand::RegisterArtifact(_)));
    let build_attempt_count =
        client.count(|c| matches!(c, ApplicationCommand::RecordBuildAttempt(_)));

    // 4 entities * 2 artifacts + 1 fixture binary + 3 replaced function candidates = 12
    assert_eq!(
        complete_count, 3,
        "exactly three CompleteWorkItem commands expected"
    );
    assert_eq!(
        mapping_count, 7,
        "RegisterGeneratedSourceMapping: 4 from skeleton + 3 for replaced functions"
    );
    assert_eq!(
        artifact_count, 12,
        "RegisterArtifact: 2 per skeleton entity + 1 fixture binary + 3 replaced candidates"
    );
    assert_eq!(
        build_attempt_count, 4,
        "RecordBuildAttempt: 1 for forced-early f_a + 3 for happy-path functions"
    );

    // 10. Audit: every mutation flowed through an ApplicationCommand variant.
    for cmd in client.commands() {
        assert!(
            matches!(
                cmd,
                ApplicationCommand::RegisterArtifact(_)
                    | ApplicationCommand::RegisterEntity(_)
                    | ApplicationCommand::RegisterGeneratedSourceMapping(_)
                    | ApplicationCommand::RecordBuildAttempt(_)
                    | ApplicationCommand::ImportGeneratedSourceCandidates(_)
                    | ApplicationCommand::CreateWorkItems(_)
                    | ApplicationCommand::CompleteWorkItem(_)
                    | ApplicationCommand::FailWorkItem(_)
            ),
            "every canonical mutation must be an ApplicationCommand variant, got: {cmd:?}"
        );
    }

    eprintln!(
        "[wave9_stub_replacement] command audit passed: {} canonical mutation(s)",
        client.commands().len()
    );
    eprintln!("[OK] 3 functions: stubbed→replaced, build green, downstream unblocked");
}

// ---------------------------------------------------------------------------
// Sanity check: the skeleton + mock build provider can build green before
// any replacement. This mirrors the Wave 6 skeleton-first-build happy path.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn wave9_skeleton_builds_green_before_replacement() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let project = ProjectId::new();
    let client = TestClient::new();

    let entities = [
        make_entity(project, ENTITY_KIND_GLOBAL.clone(), "RUNTIME_DATA"),
        make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_b"),
        make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_a"),
        make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_c"),
    ];

    let mut builder = ProjectSkeletonBuilder::new(tmp.path().to_path_buf(), project, &client);
    for e in &entities {
        builder.add_entity(e);
    }
    let manifest = builder.build().expect("skeleton build must succeed");
    assert_eq!(manifest.entity_count, 4);

    let provider = DockerMsvc2002BuildProvider::new(DockerMsvc2002Config {
        image_name: "msvc2002-build:test".into(),
        cmake_generator: "NMake Makefiles".into(),
        toolchain_path: PathBuf::from("/opt/msvc2002"),
        docker_binary: Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/mock-docker-success.sh")
                .to_string_lossy()
                .into_owned(),
        ),
    });

    let source_files: Vec<PathBuf> = entities.iter().map(|e| entity_cpp_relpath(&e.id)).collect();
    let gen_manifest = GeneratorManifest {
        project_root: tmp.path().to_path_buf(),
        cmake_generator: "NMake Makefiles".into(),
        source_files: source_files.clone(),
        executable_target: "reconstruction_skeleton".into(),
    };

    let configured = provider
        .configure_project(&gen_manifest, tmp.path())
        .await
        .expect("configure_project must succeed");
    assert!(configured.success, "configure must report success");

    let units: Vec<CompileUnit> = source_files
        .iter()
        .map(|src| CompileUnit {
            source_path: src.clone(),
            object_path: PathBuf::from("build")
                .join(src.file_stem().unwrap_or_default())
                .with_extension("obj"),
        })
        .collect();

    let compiled = provider
        .compile_units(&units)
        .await
        .expect("compile_units must succeed");
    assert!(compiled.success, "compile must report success");

    let linked = provider
        .link_target(&compiled.objects)
        .await
        .expect("link_target must succeed");
    assert!(linked.success, "link must report success");

    eprintln!("[OK] skeleton builds green before stub replacement");
}
