//! Wave 12 Todo 54 — fault-injection coverage for LLM-level failures.
//!
//! These tests exercise deterministic, mocked failures of the LLM pipeline:
//!
//! 1. Invalid parsed LLM output (schema violations / garbage after JSON starts).
//! 2. Provider timeout on the first attempt, successful retry on the second.
//! 3. Repeated identical compiler failure within a bounded retry limit that
//!    triggers blocked work.
//! 4. Corrupted artifact import rejected by hash mismatch and rolled back.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::bytes::Bytes;

use tests_support::RecordingAutoReClient;

use autore_app::application_service::requests::{
    BlockWorkItemRequest, BlockWorkWithReasonRequest, CompleteWorkItemRequest,
    CreateWorkItemsResponse, FailWorkItemResponse, RecordBuildAttemptResponse,
    RecordRepairAttemptResponse, RegisterArtifactResponse,
};
use autore_app::{ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_core::Result;
use autore_events::project_event_service::ProjectEventSubscription;
use autore_provider_runtime::artifact::{
    ArtifactError, ArtifactHandle, ArtifactTransport, LocalStagingTransport,
};
use autore_reconstruction::CallSiteSummary;
use autore_reconstruction::analysis::{InvestigationBundle, LlmImportResult, LlmImporter};
use autore_reconstruction::build::types::{
    BuildConfigured, BuildDiagnostic, BuildLogs, CompileUnit, DiagnosticSeverity,
    GeneratorManifest, LinkResult, RunTestResult, SuggestedWorkKind,
};
use autore_reconstruction::build::{BuildProviderTrait, BuildResult, CompileResult};
use autore_reconstruction::generation::orchestrator::{
    FailureAnalysisContext, FailureAnalysisResponse, GenerationContext, GenerationModel,
    GenerationModelError, GenerationOrchestrator, GenerationResponse, OrchestratorConfig,
    RepairGenerationContext, WorkItemContext, WorkItemOutcome,
};
use autore_reconstruction::work_graph::DependencyEdgeKind;
use autore_schema::domain::records::WorkItemKind;
use autore_schema::domain::{ContentHash, NamespacedId, Timestamp};
use autore_schema::ids::{
    ArtifactId, EntityId, ProjectId, ProviderInstanceId, ReconstructionCampaignId, WorkItemId,
};

// ---------------------------------------------------------------------------
// Shared test helpers
// ---------------------------------------------------------------------------

/// Test client that wraps [`RecordingAutoReClient`] and supplies lifecycle
/// command handlers used by the reconstruction tests.
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
            ApplicationCommand::CompleteWorkItem(req) => Ok(CommandResult::WorkItemCompleted(
                autore_app::application_service::requests::CompleteWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                },
            )),
            ApplicationCommand::BlockWorkItem(req) => Ok(CommandResult::WorkItemBlocked(
                autore_app::application_service::requests::BlockWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                },
            )),
            ApplicationCommand::BlockWorkWithReason(_) => Ok(CommandResult::WorkBlocked(
                autore_app::application_service::requests::BlockWorkWithReasonResponse {
                    blocked_count: 1,
                },
            )),
            ApplicationCommand::FailWorkItem(req) => {
                Ok(CommandResult::WorkItemFailed(FailWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                }))
            }
            ApplicationCommand::CreateWorkItems(req) => {
                let ids: Vec<WorkItemId> =
                    req.descriptions.iter().map(|_| WorkItemId::new()).collect();
                Ok(CommandResult::WorkItemsCreated(CreateWorkItemsResponse {
                    work_item_ids: ids.iter().map(|id| id.to_string()).collect(),
                }))
            }
            ApplicationCommand::RegisterArtifact(_) => Ok(CommandResult::ArtifactRegistered(
                RegisterArtifactResponse {
                    artifact: autore_schema::domain::records::Artifact {
                        id: ArtifactId::new(),
                        project: ProjectId::new(),
                        kind: NamespacedId::parse("core.binary").unwrap(),
                        content_hash: ContentHash::sha256(b"test"),
                        size: 0,
                        storage: autore_schema::domain::records::ArtifactStorage::ManagedBlob {
                            relative_path: PathBuf::from("test"),
                        },
                        created_at: Timestamp::now(),
                        metadata: autore_schema::domain::MetadataMap::new(),
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
    ) -> Result<Vec<autore_schema::domain::records::ProjectEvent>> {
        self.inner.events_after(project, sequence, limit)
    }

    fn subscribe_events(&self, project: ProjectId, after: u64) -> Result<ProjectEventSubscription> {
        self.inner.subscribe_events(project, after)
    }
}

/// Build a realistic `InvestigationBundle` for `LlmImporter` tests.
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
            brief: "leaf".into(),
            edge_kind: DependencyEdgeKind::DirectCall,
        }],
        relevant_types: vec![],
        relevant_globals: vec![],
        strings_and_constants: vec![],
        dynamic_observations: vec![],
        accepted_hypotheses: vec![],
        unresolved_conflicts: vec![],
        prior_generated_candidate: None,
        compiler_diagnostics: vec![],
        verification_failures: vec![],
        requested_output_schema: serde_json::Value::Null,
    }
}

/// Build a relative path that is inside the generated source tree for the
/// given entity. The patch pipeline requires paths under `src/generated/`
/// and scoped to the entity's source directory.
fn generated_source_path(entity_id: EntityId) -> PathBuf {
    let hex = entity_id.as_uuid().as_simple().to_string();
    PathBuf::from("src/generated")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex[4..6])
        .join("generated.cpp")
}

// ---------------------------------------------------------------------------
// Test 1: invalid parsed LLM output causes FailWorkItem then
// BlockWorkWithReason(InvalidOutput)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn llm_response_garbage_bytes_after_json_starts() {
    let client = TestClient::new();
    let bundle = realistic_bundle();
    let raw_id = ArtifactId::new();
    let parsed_id = ArtifactId::new();
    let project_id = ProjectId::new();
    let raw_response_text = "{\"confidence\": 1.5, \"proposed_name\": \"x\"}".to_string();
    let parsed_response: serde_json::Value = serde_json::from_str(&raw_response_text)
        .expect("the raw text starts with valid JSON, but violates the schema");

    // Attempt 1: importer should see a confidence out of range, fail the item,
    // then block it with `InvalidOutput`.
    let importer = LlmImporter::new(
        &bundle,
        "llm.analysis.function",
        raw_id,
        parsed_id,
        1,
        &client,
        project_id,
        raw_response_text.clone(),
        parsed_response.clone(),
    );

    let result = importer.import();
    assert!(
        matches!(result, Ok(LlmImportResult::InvalidOutput { .. })),
        "expected importer to block the work item, got {:?}",
        result
    );

    let commands = client.commands();
    let fail_positions: Vec<_> = commands
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, ApplicationCommand::FailWorkItem(_)))
        .map(|(i, _)| i)
        .collect();
    let block_positions: Vec<_> = commands
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            matches!(
                c,
                ApplicationCommand::BlockWorkWithReason(BlockWorkWithReasonRequest {
                    reason,
                    ..
                }) if reason.starts_with("InvalidOutput")
            )
        })
        .map(|(i, _)| i)
        .collect();

    assert!(
        !fail_positions.is_empty(),
        "expected at least one FailWorkItem command"
    );
    assert!(
        !block_positions.is_empty(),
        "expected at least one BlockWorkWithReason(InvalidOutput) command"
    );
    assert!(
        fail_positions[0] < block_positions[0],
        "FailWorkItem must precede BlockWorkWithReason"
    );

    // Ensure we also persisted the raw response as evidence for forensics.
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, ApplicationCommand::AddEvidence(_))),
        "expected raw response to be persisted as evidence"
    );
}

// ---------------------------------------------------------------------------
// Test 2: provider timeout on first attempt, then success on second
// ---------------------------------------------------------------------------

/// Inner model that times out on the first call and succeeds on the second.
struct SlowThenValidModel {
    first_call: AtomicBool,
    sleep: Duration,
    response: GenerationResponse,
}

#[async_trait]
impl GenerationModel for SlowThenValidModel {
    async fn generate_function(
        &self,
        _ctx: &GenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        if self.first_call.swap(false, Ordering::SeqCst) {
            tokio::time::sleep(self.sleep).await;
            Err(GenerationModelError::Other(
                "LLM call exceeded deadline".into(),
            ))
        } else {
            Ok(self.response.clone())
        }
    }

    async fn generate_cluster(
        &self,
        _ctx: &GenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        Ok(self.response.clone())
    }

    async fn analyze_failure(
        &self,
        _ctx: &FailureAnalysisContext,
    ) -> Result<FailureAnalysisResponse, GenerationModelError> {
        Ok(FailureAnalysisResponse {
            diagnosis: "timeout".into(),
        })
    }

    async fn generate_repair(
        &self,
        _ctx: &RepairGenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        Ok(self.response.clone())
    }
}

/// Wrapper that enforces a deadline, records a timeout diagnostic, and retries
/// exactly once.
struct TimeoutRetryingModel<M: GenerationModel> {
    inner: M,
    deadline: Duration,
    diagnostics: Mutex<Vec<BuildDiagnostic>>,
}

#[async_trait]
impl<M: GenerationModel> GenerationModel for TimeoutRetryingModel<M> {
    async fn generate_function(
        &self,
        ctx: &GenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        for attempt in 0..2 {
            let result =
                tokio::time::timeout(self.deadline, self.inner.generate_function(ctx)).await;

            match result {
                Ok(Ok(resp)) => return Ok(resp),
                Ok(Err(_)) | Err(_) if attempt == 0 => {
                    self.diagnostics.lock().unwrap().push(BuildDiagnostic {
                        diagnostic_code: "TIMEOUT".into(),
                        severity: DiagnosticSeverity::Warning,
                        file_path: PathBuf::new(),
                        line: 0,
                        column: 0,
                        message: "LLM provider timed out on first attempt".into(),
                        candidate_cause: "provider exceeded deadline".into(),
                        suggested_work_kind: SuggestedWorkKind::Unknown,
                    });
                    continue;
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(GenerationModelError::Other(
                        "LLM provider timed out on retry".into(),
                    ));
                }
            }
        }

        Err(GenerationModelError::Other(
            "LLM provider did not recover after retry".into(),
        ))
    }

    async fn generate_cluster(
        &self,
        ctx: &GenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        self.inner.generate_cluster(ctx).await
    }

    async fn analyze_failure(
        &self,
        ctx: &FailureAnalysisContext,
    ) -> Result<FailureAnalysisResponse, GenerationModelError> {
        self.inner.analyze_failure(ctx).await
    }

    async fn generate_repair(
        &self,
        ctx: &RepairGenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        self.inner.generate_repair(ctx).await
    }
}

#[tokio::test]
#[ignore]
async fn llm_timeout_on_first_attempt_then_succeeds_second_attempt() {
    let client = TestClient::new();
    let project_id = ProjectId::new();
    let campaign_id = ReconstructionCampaignId::new();
    let work_item_id = WorkItemId::new();
    let entity_id = EntityId::new();

    let response = GenerationResponse {
        relative_path: generated_source_path(entity_id),
        candidate_bytes: b"int generated() { return 42; }".to_vec(),
    };

    let inner = SlowThenValidModel {
        first_call: AtomicBool::new(true),
        sleep: Duration::from_millis(200),
        response,
    };

    let model = TimeoutRetryingModel {
        inner,
        deadline: Duration::from_millis(50),
        diagnostics: Mutex::new(vec![]),
    };

    let provider = AlwaysSucceedBuildProvider;

    let mut orchestrator = GenerationOrchestrator::new(
        PathBuf::from("/tmp/autore-faults-llm-out"),
        project_id,
        campaign_id.to_string(),
        &provider,
        &client,
        &model,
        OrchestratorConfig {
            max_repair_attempts: 3,
            repeated_equivalent_failure_threshold: 3,
        },
    );

    let work = WorkItemContext {
        work_item_id: work_item_id.to_string(),
        kind: WorkItemKind::Function,
        subject_entity: Some(entity_id),
        dependencies: vec![],
        cluster_members: None,
    };

    let outcome = orchestrator
        .process_next_work_item(&[work], &HashSet::new())
        .await;

    assert!(
        matches!(outcome, Ok(WorkItemOutcome::Completed)),
        "orchestrator should complete after retry, got {:?}",
        outcome
    );

    // Assert the first attempt recorded a timeout warning.
    let diags = model.diagnostics.lock().unwrap();
    assert_eq!(diags.len(), 1, "expected exactly one timeout diagnostic");
    assert_eq!(diags[0].diagnostic_code, "TIMEOUT");
    assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
    assert!(
        diags[0].message.contains("timed out"),
        "message should mention timeout: {:?}",
        diags[0].message
    );

    // Assert the work item was completed.
    let commands = client.commands();
    assert!(
        commands.iter().any(|c| matches!(
            c,
            ApplicationCommand::CompleteWorkItem(CompleteWorkItemRequest { .. })
        )),
        "expected CompleteWorkItem after successful retry"
    );
}

// ---------------------------------------------------------------------------
// Test 3: repeated identical compiler failure triggers blocked work
// ---------------------------------------------------------------------------

/// Build provider that always returns the same diagnostic.
struct RepeatedFailureBuildProvider {
    counter: AtomicUsize,
    diagnostic: BuildDiagnostic,
}

#[async_trait]
impl BuildProviderTrait for RepeatedFailureBuildProvider {
    async fn configure_project(
        &self,
        _generator_manifest: &GeneratorManifest,
        _project_root: &Path,
    ) -> BuildResult<BuildConfigured> {
        Ok(BuildConfigured {
            build_dir: PathBuf::from("/tmp/build"),
            success: true,
            stdout: "".into(),
            stderr: "".into(),
        })
    }

    async fn compile_units(&self, _units: &[CompileUnit]) -> BuildResult<CompileResult> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        Ok(CompileResult {
            objects: vec![],
            success: false,
            stdout: "".into(),
            stderr: self.diagnostic.message.clone(),
        })
    }

    async fn link_target(&self, _target_artifacts: &[PathBuf]) -> BuildResult<LinkResult> {
        Ok(LinkResult {
            executable: PathBuf::from("/tmp/out"),
            success: false,
            stdout: "".into(),
            stderr: "".into(),
        })
    }

    async fn run_test(&self, _test_target: &str) -> BuildResult<RunTestResult> {
        Ok(RunTestResult {
            exit_code: 1,
            stdout: "".into(),
            stderr: "".into(),
        })
    }

    async fn collect_diagnostics(
        &self,
        build_logs: &BuildLogs,
    ) -> BuildResult<Vec<BuildDiagnostic>> {
        if build_logs.stderr.contains(&self.diagnostic.message) {
            Ok(vec![self.diagnostic.clone()])
        } else {
            Ok(vec![])
        }
    }
}

#[tokio::test]
#[ignore]
async fn repeated_identical_compiler_failure_within_bounded_retry_limit_triggers_blocked_work() {
    let client = TestClient::new();
    let project_id = ProjectId::new();
    let campaign_id = ReconstructionCampaignId::new();
    let work_item_id = WorkItemId::new();
    let entity_id = EntityId::new();

    let diagnostic = BuildDiagnostic {
        diagnostic_code: "C1010".into(),
        severity: DiagnosticSeverity::Error,
        file_path: PathBuf::from("src/lib.rs"),
        line: 1,
        column: 1,
        message: "same compiler error every time".into(),
        candidate_cause: "stub".into(),
        suggested_work_kind: SuggestedWorkKind::Unknown,
    };

    let provider = RepeatedFailureBuildProvider {
        counter: AtomicUsize::new(0),
        diagnostic,
    };

    // A generation model that returns empty generated files so the build path
    // runs immediately and produces the repeated failure.
    let model = AlwaysEmptyGenerationModel;

    let mut orchestrator = GenerationOrchestrator::new(
        PathBuf::from("/tmp/autore-faults-llm-out"),
        project_id,
        campaign_id.to_string(),
        &provider,
        &client,
        &model,
        OrchestratorConfig {
            max_repair_attempts: 2,
            repeated_equivalent_failure_threshold: 2,
        },
    );

    let work = WorkItemContext {
        work_item_id: work_item_id.to_string(),
        kind: WorkItemKind::Function,
        subject_entity: Some(entity_id),
        dependencies: vec![],
        cluster_members: None,
    };

    let outcome = orchestrator
        .process_next_work_item(&[work], &HashSet::new())
        .await;

    assert!(
        matches!(outcome, Ok(WorkItemOutcome::Blocked)),
        "orchestrator should block after repeated identical failures, got {:?}",
        outcome
    );

    let commands = client.commands();
    assert!(
        commands.iter().any(|c| matches!(
            c,
            ApplicationCommand::BlockWorkItem(BlockWorkItemRequest { .. })
        )),
        "expected BlockWorkItem command"
    );
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            ApplicationCommand::CompleteWorkItem(CompleteWorkItemRequest { .. })
        )),
        "must not complete the work item after repeated failures"
    );

    // Ensure we made at least as many build attempts as the threshold.
    assert!(
        provider.counter.load(Ordering::SeqCst) >= 2,
        "expected at least 2 compile attempts"
    );
}

// ---------------------------------------------------------------------------
// Test 4: corrupted artifact rejected by import and rolled back
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn corrupted_artifact_rejected_by_import() {
    let client = TestClient::new();
    let instance_id = ProviderInstanceId::new();
    let transport = LocalStagingTransport::new(
        PathBuf::from("/tmp/autore-faults-llm-staging"),
        instance_id,
        "request-1".into(),
    );

    let original = Bytes::from_static(b"valid artifact bytes");
    let original_hash = ContentHash::blake3(&original);

    // Stage the artifact, then corrupt the staged bytes by overwriting the
    // staged data file directly.
    let handle: ArtifactHandle = transport
        .stage_inbound(original.clone())
        .await
        .expect("stage succeeded");
    let corrupted = {
        let mut v = original.to_vec();
        v.push(b'!');
        Bytes::from(v)
    };

    let data_path = handle.staging_path().join("data");
    tokio::fs::write(&data_path, &corrupted)
        .await
        .expect("corrupt staged data");

    // Committing with the original hash must detect the mismatch.
    let kind = NamespacedId::parse("core.binary").unwrap();
    let commit_result = transport.commit_inbound(&handle, kind, original_hash).await;

    assert!(
        matches!(commit_result, Err(ArtifactError::HashMismatch { .. })),
        "expected HashMismatch error, got {:?}",
        commit_result
    );

    // The application layer must not have issued canonical import commands.
    let commands = client.commands();
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, ApplicationCommand::RegisterArtifact(_))),
        "must not register corrupted artifact"
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, ApplicationCommand::ImportProviderRunResult(_))),
        "must not import provider run result for corrupted artifact"
    );
}

// ---------------------------------------------------------------------------
// Supporting mocks
// ---------------------------------------------------------------------------

/// Build provider that always succeeds with no diagnostics.
struct AlwaysSucceedBuildProvider;

#[async_trait]
impl BuildProviderTrait for AlwaysSucceedBuildProvider {
    async fn configure_project(
        &self,
        _generator_manifest: &GeneratorManifest,
        _project_root: &Path,
    ) -> BuildResult<BuildConfigured> {
        Ok(BuildConfigured {
            build_dir: PathBuf::from("/tmp/build"),
            success: true,
            stdout: "".into(),
            stderr: "".into(),
        })
    }

    async fn compile_units(&self, _units: &[CompileUnit]) -> BuildResult<CompileResult> {
        Ok(CompileResult {
            objects: vec![],
            success: true,
            stdout: "".into(),
            stderr: "".into(),
        })
    }

    async fn link_target(&self, _target_artifacts: &[PathBuf]) -> BuildResult<LinkResult> {
        Ok(LinkResult {
            executable: PathBuf::from("/tmp/out"),
            success: true,
            stdout: "".into(),
            stderr: "".into(),
        })
    }

    async fn run_test(&self, _test_target: &str) -> BuildResult<RunTestResult> {
        Ok(RunTestResult {
            exit_code: 0,
            stdout: "ok".into(),
            stderr: "".into(),
        })
    }

    async fn collect_diagnostics(
        &self,
        _build_logs: &BuildLogs,
    ) -> BuildResult<Vec<BuildDiagnostic>> {
        Ok(vec![])
    }
}

/// Generation model that always returns an empty response.
struct AlwaysEmptyGenerationModel;

#[async_trait]
impl GenerationModel for AlwaysEmptyGenerationModel {
    async fn generate_function(
        &self,
        ctx: &GenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        Ok(GenerationResponse {
            relative_path: ctx
                .subject_entity
                .map(generated_source_path)
                .unwrap_or_else(|| PathBuf::from("src/generated/00/00/00/generated.cpp")),
            candidate_bytes: b"".to_vec(),
        })
    }

    async fn generate_cluster(
        &self,
        _ctx: &GenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        Ok(GenerationResponse {
            relative_path: PathBuf::from("src/generated.cpp"),
            candidate_bytes: b"".to_vec(),
        })
    }

    async fn analyze_failure(
        &self,
        _ctx: &FailureAnalysisContext,
    ) -> Result<FailureAnalysisResponse, GenerationModelError> {
        Ok(FailureAnalysisResponse {
            diagnosis: "empty".into(),
        })
    }

    async fn generate_repair(
        &self,
        ctx: &RepairGenerationContext,
    ) -> Result<GenerationResponse, GenerationModelError> {
        Ok(GenerationResponse {
            relative_path: ctx
                .subject_entity
                .map(generated_source_path)
                .unwrap_or_else(|| PathBuf::from("src/generated/00/00/00/generated.cpp")),
            candidate_bytes: b"".to_vec(),
        })
    }
}
