//! Leaf-first stub-replacement generation orchestrator.
//!
//! The orchestrator selects ready `Function` or `FunctionCluster` work items
//! (leaf functions first, then small clusters), dispatches generation through
//! a pluggable [`GenerationModel`], applies the resulting candidate through
//! the controlled [`PatchPipeline`], and routes build failures through the
//! deterministic repair taxonomy before falling back to bounded LLM repair.
//!
//! All durable side effects go through [`ApplicationCommand`] variants; the
//! orchestrator never writes directly to the project database or artifact
//! storage.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use autore_app::application_service::requests::{
    BlockWorkItemRequest, CompleteWorkItemRequest, CreateWorkItemsRequest,
    RecordBuildAttemptRequest, RecordRepairAttemptRequest,
};
use autore_app::{ApplicationCommand, AutoReClient};
use autore_core::{Error, Result};
use autore_schema::domain::records::WorkItemKind;
use autore_schema::ids::{EntityId, ProjectId};

use crate::build::{
    BuildDiagnostic, BuildProviderTrait, RepairStrategy, classify, select_repair_strategy,
};
use crate::generation::patch::{CandidatePatch, PatchOutcome, PatchPipeline};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Retry and priority policy for the generation orchestrator.
#[derive(Debug, Clone, Copy)]
pub struct OrchestratorConfig {
    /// Maximum LLM repair attempts for a single work item.
    pub max_repair_attempts: u32,
    /// Number of identical diagnostic (code + line + column) occurrences
    /// that trigger a `RepeatedEquivalentFailure` block.
    pub repeated_equivalent_failure_threshold: u32,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_repair_attempts: 3,
            repeated_equivalent_failure_threshold: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Work item context
// ---------------------------------------------------------------------------

/// Lightweight read-only view of a work item used by the orchestrator.
#[derive(Debug, Clone)]
pub struct WorkItemContext {
    pub work_item_id: String,
    pub kind: WorkItemKind,
    pub subject_entity: Option<EntityId>,
    pub dependencies: Vec<String>,
    pub cluster_members: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Generation model trait
// ---------------------------------------------------------------------------

/// Input context for a generation request.
#[derive(Debug, Clone)]
pub struct GenerationContext {
    pub work_item_id: String,
    pub subject_entity: Option<EntityId>,
}

/// A candidate produced by the generation model.
#[derive(Debug, Clone)]
pub struct GenerationResponse {
    pub relative_path: PathBuf,
    pub candidate_bytes: Vec<u8>,
}

/// Input context for failure analysis.
#[derive(Debug, Clone)]
pub struct FailureAnalysisContext {
    pub work_item_id: String,
    pub subject_entity: Option<EntityId>,
    pub diagnostics: Vec<BuildDiagnostic>,
}

/// Analysis result used to guide repair generation.
#[derive(Debug, Clone)]
pub struct FailureAnalysisResponse {
    pub diagnosis: String,
}

/// Input context for repair generation.
#[derive(Debug, Clone)]
pub struct RepairGenerationContext {
    pub work_item_id: String,
    pub subject_entity: Option<EntityId>,
    pub prior_candidate_path: PathBuf,
    pub prior_candidate_bytes: Vec<u8>,
    pub analysis: FailureAnalysisResponse,
    pub diagnostics: Vec<BuildDiagnostic>,
}

/// Errors returned by a generation model.
#[derive(Debug, thiserror::Error)]
pub enum GenerationModelError {
    #[error("generation model error: {0}")]
    Other(String),
}

/// Async boundary between the orchestrator and an LLM provider.
#[async_trait]
pub trait GenerationModel: Send + Sync {
    async fn generate_function(
        &self,
        ctx: &GenerationContext,
    ) -> std::result::Result<GenerationResponse, GenerationModelError>;

    async fn generate_cluster(
        &self,
        ctx: &GenerationContext,
    ) -> std::result::Result<GenerationResponse, GenerationModelError>;

    async fn analyze_failure(
        &self,
        ctx: &FailureAnalysisContext,
    ) -> std::result::Result<FailureAnalysisResponse, GenerationModelError>;

    async fn generate_repair(
        &self,
        ctx: &RepairGenerationContext,
    ) -> std::result::Result<GenerationResponse, GenerationModelError>;
}

// ---------------------------------------------------------------------------
// Repair attempt log
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct RepairAttemptLog {
    /// Total LLM repair attempts per work item.
    attempts: HashMap<String, u32>,
    /// Occurrence counts per diagnostic key per work item.
    occurrences: HashMap<String, HashMap<DiagnosticKey, u32>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiagnosticKey {
    code: String,
    line: u32,
    column: u32,
}

impl RepairAttemptLog {
    fn llm_attempts(&self, work_item_id: &str) -> u32 {
        self.attempts.get(work_item_id).copied().unwrap_or(0)
    }

    fn record_llm_attempt(&mut self, work_item_id: &str) {
        *self.attempts.entry(work_item_id.to_string()).or_insert(0) += 1;
    }

    fn record_diagnostics(&mut self, work_item_id: &str, diagnostics: &[BuildDiagnostic]) {
        let entry = self
            .occurrences
            .entry(work_item_id.to_string())
            .or_default();
        for d in diagnostics {
            let key = DiagnosticKey {
                code: d.diagnostic_code.clone(),
                line: d.line,
                column: d.column,
            };
            *entry.entry(key).or_insert(0) += 1;
        }
    }

    fn max_occurrences(&self, work_item_id: &str) -> u32 {
        self.occurrences
            .get(work_item_id)
            .map(|m| m.values().copied().max().unwrap_or(0))
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Result of processing one work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkItemOutcome {
    Completed,
    Blocked,
    RepairDeferred,
    NoWork,
}

/// Schedules leaf-first stub replacement and routes build failures through
/// deterministic repair before bounded LLM repair.
pub struct GenerationOrchestrator<'a> {
    output_root: PathBuf,
    project_id: ProjectId,
    campaign_id: String,
    build_provider: &'a dyn BuildProviderTrait,
    client: &'a dyn AutoReClient,
    model: &'a dyn GenerationModel,
    config: OrchestratorConfig,
    attempt_log: RepairAttemptLog,
}

impl<'a> GenerationOrchestrator<'a> {
    pub fn new(
        output_root: PathBuf,
        project_id: ProjectId,
        campaign_id: String,
        build_provider: &'a dyn BuildProviderTrait,
        client: &'a dyn AutoReClient,
        model: &'a dyn GenerationModel,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            output_root,
            project_id,
            campaign_id,
            build_provider,
            client,
            model,
            config,
            attempt_log: RepairAttemptLog::default(),
        }
    }

    /// Select and process the highest-priority ready work item.
    pub async fn process_next_work_item(
        &mut self,
        work_items: &[WorkItemContext],
        stubbed: &HashSet<String>,
    ) -> Result<WorkItemOutcome> {
        let next = match self.select_next(work_items, stubbed) {
            Some(n) => n,
            None => return Ok(WorkItemOutcome::NoWork),
        };

        self.client.execute(ApplicationCommand::RecordBuildAttempt(
            RecordBuildAttemptRequest {
                project: self.project_id,
                work_item_id: next.work_item_id.clone(),
            },
        ))?;

        let outcome = self.generate_and_build(&next).await?;

        if outcome.build_success {
            self.client.execute(ApplicationCommand::CompleteWorkItem(
                CompleteWorkItemRequest {
                    project: self.project_id,
                    work_item_id: next.work_item_id.clone(),
                },
            ))?;
            self.attempt_log.attempts.remove(&next.work_item_id);
            self.attempt_log.occurrences.remove(&next.work_item_id);
            return Ok(WorkItemOutcome::Completed);
        }

        self.attempt_log
            .record_diagnostics(&next.work_item_id, &outcome.diagnostics);

        let mut deterministic_issued = false;
        let mut llm_diagnostics = Vec::new();

        for diagnostic in &outcome.diagnostics {
            let kind = classify(diagnostic);
            let strategy = select_repair_strategy(kind, diagnostic);
            match strategy {
                RepairStrategy::CreateWorkItems { kind, reason } => {
                    self.client.execute(ApplicationCommand::CreateWorkItems(
                        CreateWorkItemsRequest {
                            project: self.project_id,
                            campaign_id: self.campaign_id.clone(),
                            descriptions: vec![format!(
                                "{}: {} ({}:{})",
                                kind.as_namespaced_kind(),
                                reason,
                                diagnostic.line,
                                diagnostic.column
                            )],
                        },
                    ))?;
                    deterministic_issued = true;
                }
                RepairStrategy::RequestLlmAnalysis { .. } => {
                    llm_diagnostics.push(diagnostic.clone());
                }
                RepairStrategy::BlockWorkItem { reason } => {
                    self.client.execute(ApplicationCommand::BlockWorkItem(
                        BlockWorkItemRequest {
                            project: self.project_id,
                            work_item_id: next.work_item_id.clone(),
                            reason,
                        },
                    ))?;
                    return Ok(WorkItemOutcome::Blocked);
                }
                RepairStrategy::RequestLayoutInvestigation => {
                    self.client.execute(ApplicationCommand::CreateWorkItems(
                        CreateWorkItemsRequest {
                            project: self.project_id,
                            campaign_id: self.campaign_id.clone(),
                            descriptions: vec![format!(
                                "layout investigation: {} ({}:{})",
                                diagnostic.message, diagnostic.line, diagnostic.column
                            )],
                        },
                    ))?;
                    deterministic_issued = true;
                }
                RepairStrategy::NoAction => {}
            }
        }

        if llm_diagnostics.is_empty() {
            if deterministic_issued {
                return Ok(WorkItemOutcome::RepairDeferred);
            }
            self.client
                .execute(ApplicationCommand::BlockWorkItem(BlockWorkItemRequest {
                    project: self.project_id,
                    work_item_id: next.work_item_id.clone(),
                    reason: "no repair strategy for build failure".into(),
                }))?;
            return Ok(WorkItemOutcome::Blocked);
        }

        self.repair_loop(next, outcome, llm_diagnostics).await
    }

    /// Alias for [`process_next_work_item`].
    pub async fn run_one_cycle(
        &mut self,
        work_items: &[WorkItemContext],
        stubbed: &HashSet<String>,
    ) -> Result<WorkItemOutcome> {
        self.process_next_work_item(work_items, stubbed).await
    }

    fn select_next(
        &self,
        work_items: &[WorkItemContext],
        stubbed: &HashSet<String>,
    ) -> Option<WorkItemContext> {
        let mut ready: Vec<&WorkItemContext> = work_items
            .iter()
            .filter(|w| self.is_ready(w, stubbed))
            .collect();
        ready.sort_by_key(|w| self.priority(w, stubbed));
        ready.first().cloned().cloned()
    }

    fn is_ready(&self, item: &WorkItemContext, stubbed: &HashSet<String>) -> bool {
        match item.kind {
            WorkItemKind::Function => item.dependencies.iter().all(|dep| !stubbed.contains(dep)),
            WorkItemKind::FunctionCluster => item
                .cluster_members
                .as_ref()
                .map(|members| members.iter().all(|m| !stubbed.contains(m)))
                .unwrap_or(true),
            _ => false,
        }
    }

    fn priority(&self, item: &WorkItemContext, stubbed: &HashSet<String>) -> (usize, usize) {
        match item.kind {
            WorkItemKind::Function => {
                let stub_count = item
                    .dependencies
                    .iter()
                    .filter(|d| stubbed.contains(*d))
                    .count();
                (0, stub_count)
            }
            WorkItemKind::FunctionCluster => {
                let member_count = item.cluster_members.as_ref().map(|m| m.len()).unwrap_or(0);
                (1, member_count)
            }
            _ => (2, 0),
        }
    }

    async fn generate_and_build(&self, item: &WorkItemContext) -> Result<PatchOutcome> {
        let ctx = GenerationContext {
            work_item_id: item.work_item_id.clone(),
            subject_entity: item.subject_entity,
        };

        let response = match item.kind {
            WorkItemKind::Function => self.model.generate_function(&ctx).await,
            WorkItemKind::FunctionCluster => self.model.generate_cluster(&ctx).await,
            _ => {
                return Err(Error::Validation(format!(
                    "unsupported work item kind for generation: {:?}",
                    item.kind
                )));
            }
        }
        .map_err(|e| Error::Validation(e.to_string()))?;

        let prior = self.read_prior_content(&response.relative_path).await?;
        let candidate = CandidatePatch {
            relative_path: response.relative_path.clone(),
            new_content_bytes: response.candidate_bytes,
            prior_content_bytes: prior,
            source_evidence_refs: Vec::new(),
        };

        let mut declared = HashSet::new();
        declared.insert(candidate.relative_path.clone());

        let entity = item.subject_entity.unwrap_or_default();
        let pipeline = PatchPipeline::new(
            self.output_root.clone(),
            self.project_id,
            self.build_provider,
            self.client,
        );

        pipeline
            .apply(vec![candidate], &declared, entity)
            .await
            .map_err(|e| Error::Validation(e.to_string()))
    }

    async fn repair_loop(
        &mut self,
        item: WorkItemContext,
        mut last_outcome: PatchOutcome,
        mut llm_diagnostics: Vec<BuildDiagnostic>,
    ) -> Result<WorkItemOutcome> {
        loop {
            let max_occ = self.attempt_log.max_occurrences(&item.work_item_id);
            if max_occ >= self.config.repeated_equivalent_failure_threshold {
                self.client
                    .execute(ApplicationCommand::BlockWorkItem(BlockWorkItemRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                        reason: "RepeatedEquivalentFailure".into(),
                    }))?;
                return Ok(WorkItemOutcome::Blocked);
            }

            let attempts = self.attempt_log.llm_attempts(&item.work_item_id);
            if attempts >= self.config.max_repair_attempts {
                self.client
                    .execute(ApplicationCommand::BlockWorkItem(BlockWorkItemRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                        reason: "MaxRepairAttempts".into(),
                    }))?;
                return Ok(WorkItemOutcome::Blocked);
            }

            self.attempt_log.record_llm_attempt(&item.work_item_id);
            self.client
                .execute(ApplicationCommand::RecordRepairAttempt(
                    RecordRepairAttemptRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                    },
                ))?;

            let analysis = self
                .model
                .analyze_failure(&FailureAnalysisContext {
                    work_item_id: item.work_item_id.clone(),
                    subject_entity: item.subject_entity,
                    diagnostics: llm_diagnostics.clone(),
                })
                .await
                .map_err(|e| Error::Validation(e.to_string()))?;

            let repair = self
                .model
                .generate_repair(&RepairGenerationContext {
                    work_item_id: item.work_item_id.clone(),
                    subject_entity: item.subject_entity,
                    prior_candidate_path: last_outcome
                        .staging_dir
                        .as_ref()
                        .map(|d| d.join("repair.cpp"))
                        .unwrap_or_else(|| PathBuf::from("repair.cpp")),
                    prior_candidate_bytes: Vec::new(),
                    analysis,
                    diagnostics: llm_diagnostics.clone(),
                })
                .await
                .map_err(|e| Error::Validation(e.to_string()))?;

            let prior = self.read_prior_content(&repair.relative_path).await?;
            let candidate = CandidatePatch {
                relative_path: repair.relative_path,
                new_content_bytes: repair.candidate_bytes,
                prior_content_bytes: prior,
                source_evidence_refs: Vec::new(),
            };

            let mut declared = HashSet::new();
            declared.insert(candidate.relative_path.clone());

            let entity = item.subject_entity.unwrap_or_default();
            let pipeline = PatchPipeline::new(
                self.output_root.clone(),
                self.project_id,
                self.build_provider,
                self.client,
            );

            last_outcome = pipeline
                .apply(vec![candidate], &declared, entity)
                .await
                .map_err(|e| Error::Validation(e.to_string()))?;

            if last_outcome.build_success {
                self.client.execute(ApplicationCommand::CompleteWorkItem(
                    CompleteWorkItemRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                    },
                ))?;
                self.attempt_log.attempts.remove(&item.work_item_id);
                self.attempt_log.occurrences.remove(&item.work_item_id);
                return Ok(WorkItemOutcome::Completed);
            }

            self.attempt_log
                .record_diagnostics(&item.work_item_id, &last_outcome.diagnostics);

            llm_diagnostics.clear();
            for diagnostic in &last_outcome.diagnostics {
                let kind = classify(diagnostic);
                let strategy = select_repair_strategy(kind, diagnostic);
                if let RepairStrategy::RequestLlmAnalysis { .. } = strategy {
                    llm_diagnostics.push(diagnostic.clone());
                }
            }

            if llm_diagnostics.is_empty() {
                self.client
                    .execute(ApplicationCommand::BlockWorkItem(BlockWorkItemRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                        reason: "repair produced non-LLM failure".into(),
                    }))?;
                return Ok(WorkItemOutcome::Blocked);
            }
        }
    }

    async fn read_prior_content(&self, relative_path: &Path) -> Result<Vec<u8>> {
        let path = self.output_root.join(relative_path);
        if path.exists() {
            tokio::fs::read(&path).await.map_err(Error::Io)
        } else {
            Ok(Vec::new())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use autore_app::application_service::requests::{
        RecordBuildAttemptResponse, RecordRepairAttemptResponse,
    };
    use autore_app::{
        ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, QueryResult,
    };
    use autore_core::Result;
    use autore_events::project_event_service::ProjectEventSubscription;
    use autore_schema::domain::records::{ProjectEvent, WorkItemKind};
    use autore_schema::ids::{EntityId, ProjectId};

    use crate::build::types::{
        BuildDiagnostic, BuildLogs, CompileUnit, DiagnosticSeverity, GeneratorManifest,
        LinkResult as LinkResultType,
    };
    use crate::build::{
        BuildConfigured, BuildProviderTrait, BuildResult, CompileResult, RunTestResult,
    };
    use crate::generation::orchestrator::{
        FailureAnalysisContext, FailureAnalysisResponse, GenerationContext, GenerationModel,
        GenerationModelError, GenerationOrchestrator, GenerationResponse, OrchestratorConfig,
        RepairGenerationContext, WorkItemContext, WorkItemOutcome,
    };
    use crate::tests_support::RecordingAutoReClient;

    /// Test client that wraps [`RecordingAutoReClient`] and supplies the
    /// additional command handlers the shared recording client does not yet
    /// implement (`RecordBuildAttempt`, `RecordRepairAttempt`, `CompleteWorkItem`,
    /// `BlockWorkItem`). All commands are recorded for assertions.
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
                ApplicationCommand::RecordBuildAttempt(_) => Ok(
                    CommandResult::BuildAttemptRecorded(RecordBuildAttemptResponse {
                        attempt_id: uuid::Uuid::now_v7().to_string(),
                    }),
                ),
                ApplicationCommand::RecordRepairAttempt(_) => Ok(
                    CommandResult::RepairAttemptRecorded(RecordRepairAttemptResponse {
                        repair_id: uuid::Uuid::now_v7().to_string(),
                    }),
                ),
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

        fn subscribe_events(
            &self,
            project: ProjectId,
            after: u64,
        ) -> Result<ProjectEventSubscription> {
            self.inner.subscribe_events(project, after)
        }
    }

    fn diag(code: &str, line: u32, column: u32, message: &str) -> BuildDiagnostic {
        BuildDiagnostic {
            diagnostic_code: code.into(),
            severity: DiagnosticSeverity::Error,
            file_path: PathBuf::from("test.cpp"),
            line,
            column,
            message: message.into(),
            candidate_cause: String::new(),
            suggested_work_kind: crate::build::types::SuggestedWorkKind::Unknown,
        }
    }

    fn entity_relpath(entity_id: EntityId) -> PathBuf {
        let hex = entity_id.as_uuid().as_simple().to_string();
        PathBuf::from("src/generated")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex[4..6])
            .join(&hex)
            .with_extension("cpp")
    }

    fn write_prior_file(root: &std::path::Path, entity_id: EntityId, content: &[u8]) -> PathBuf {
        let rel = entity_relpath(entity_id);
        let path = root.join(&rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        rel
    }

    // -----------------------------------------------------------------------
    // Mock build provider
    // -----------------------------------------------------------------------

    struct MockBuildProvider {
        outcomes: Mutex<Vec<MockBuildOutcome>>,
        calls: Mutex<u32>,
        current_diagnostics: Mutex<Vec<BuildDiagnostic>>,
    }

    #[derive(Debug, Clone)]
    struct MockBuildOutcome {
        success: bool,
        diagnostics: Vec<BuildDiagnostic>,
    }

    impl MockBuildProvider {
        fn new(outcomes: Vec<MockBuildOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes),
                calls: Mutex::new(0),
                current_diagnostics: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> u32 {
            *self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl BuildProviderTrait for MockBuildProvider {
        async fn configure_project(
            &self,
            _manifest: &GeneratorManifest,
            _project_root: &std::path::Path,
        ) -> BuildResult<BuildConfigured> {
            Ok(BuildConfigured {
                build_dir: PathBuf::from("build"),
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }

        async fn compile_units(&self, _units: &[CompileUnit]) -> BuildResult<CompileResult> {
            let mut calls = self.calls.lock().unwrap();
            let mut outcomes = self.outcomes.lock().unwrap();
            let outcome = outcomes.remove(0);
            *self.current_diagnostics.lock().unwrap() = outcome.diagnostics.clone();
            *calls += 1;
            Ok(CompileResult {
                objects: Vec::new(),
                success: outcome.success,
                stdout: String::new(),
                stderr: String::new(),
            })
        }

        async fn link_target(&self, _target_artifacts: &[PathBuf]) -> BuildResult<LinkResultType> {
            Ok(LinkResultType {
                executable: PathBuf::from("build/output.exe"),
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

    // -----------------------------------------------------------------------
    // Mock generation model
    // -----------------------------------------------------------------------

    #[derive(Debug, Default)]
    struct MockGenerationModel {
        function_responses: Mutex<HashMap<String, GenerationResponse>>,
        repair_responses: Mutex<Vec<GenerationResponse>>,
        invocations: Mutex<Vec<String>>,
    }

    impl MockGenerationModel {
        fn with_function(id: &str, response: GenerationResponse) -> Self {
            let mut map = HashMap::new();
            map.insert(id.to_string(), response);
            Self {
                function_responses: Mutex::new(map),
                repair_responses: Mutex::new(Vec::new()),
                invocations: Mutex::new(Vec::new()),
            }
        }

        fn with_repair_chain(mut self, responses: Vec<GenerationResponse>) -> Self {
            self.repair_responses = Mutex::new(responses);
            self
        }

        fn invocation_count(&self, needle: &str) -> usize {
            self.invocations
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.contains(needle))
                .count()
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
                .push("generate_function".into());
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
            self.invocations
                .lock()
                .unwrap()
                .push("generate_cluster".into());
            Err(GenerationModelError::Other("cluster not mocked".into()))
        }

        async fn analyze_failure(
            &self,
            ctx: &FailureAnalysisContext,
        ) -> std::result::Result<FailureAnalysisResponse, GenerationModelError> {
            self.invocations
                .lock()
                .unwrap()
                .push("analyze_failure".into());
            Ok(FailureAnalysisResponse {
                diagnosis: format!("analyzed {} diagnostics", ctx.diagnostics.len()),
            })
        }

        async fn generate_repair(
            &self,
            _ctx: &RepairGenerationContext,
        ) -> std::result::Result<GenerationResponse, GenerationModelError> {
            self.invocations
                .lock()
                .unwrap()
                .push("generate_repair".into());
            let mut guard = self.repair_responses.lock().unwrap();
            if guard.is_empty() {
                Err(GenerationModelError::Other("no repair response".into()))
            } else {
                Ok(guard.remove(0))
            }
        }
    }

    fn response_for(entity_id: EntityId, content: &[u8]) -> GenerationResponse {
        GenerationResponse {
            relative_path: entity_relpath(entity_id),
            candidate_bytes: content.to_vec(),
        }
    }

    fn build_orchestrator<'a>(
        tmp: &'a tempfile::TempDir,
        client: &'a TestClient,
        model: &'a MockGenerationModel,
        provider: &'a MockBuildProvider,
        config: OrchestratorConfig,
    ) -> GenerationOrchestrator<'a> {
        GenerationOrchestrator::new(
            tmp.path().to_path_buf(),
            ProjectId::new(),
            "campaign-1".into(),
            provider,
            client,
            model,
            config,
        )
    }

    #[tokio::test]
    async fn leaf_first_ordering_priority_max_when_no_stubs_remaining_in_callees() {
        let tmp = tempfile::tempdir().unwrap();
        let client = TestClient::new();

        let leaf_entity = EntityId::new();
        let leaf_rel = entity_relpath(leaf_entity);
        std::fs::create_dir_all(tmp.path().join(leaf_rel.parent().unwrap())).unwrap();

        let caller_entity = EntityId::new();
        let caller_rel = entity_relpath(caller_entity);
        std::fs::create_dir_all(tmp.path().join(caller_rel.parent().unwrap())).unwrap();

        let leaf_id = "leaf-func".to_string();
        let caller_id = "caller-func".to_string();

        let model = MockGenerationModel::with_function(
            &leaf_id,
            response_for(leaf_entity, b"int leaf() { return 1; }\n"),
        );
        let provider = MockBuildProvider::new(vec![MockBuildOutcome {
            success: true,
            diagnostics: Vec::new(),
        }]);

        let mut orchestrator = build_orchestrator(
            &tmp,
            &client,
            &model,
            &provider,
            OrchestratorConfig::default(),
        );

        let work_items = vec![
            WorkItemContext {
                work_item_id: caller_id.clone(),
                kind: WorkItemKind::Function,
                subject_entity: Some(caller_entity),
                dependencies: vec![leaf_id.clone()],
                cluster_members: None,
            },
            WorkItemContext {
                work_item_id: leaf_id.clone(),
                kind: WorkItemKind::Function,
                subject_entity: Some(leaf_entity),
                dependencies: Vec::new(),
                cluster_members: None,
            },
        ];

        let mut stubbed: HashSet<String> = HashSet::new();
        stubbed.insert(leaf_id.clone());
        stubbed.insert(caller_id.clone());

        let outcome = orchestrator
            .process_next_work_item(&work_items, &stubbed)
            .await
            .unwrap();
        assert_eq!(outcome, WorkItemOutcome::Completed);

        let completed = client
            .commands()
            .iter()
            .filter_map(|c| match c {
                ApplicationCommand::CompleteWorkItem(req) => Some(req.work_item_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(completed, vec![leaf_id]);
    }

    #[tokio::test]
    async fn deterministic_repair_paths_covered_before_llm_repair_invoked() {
        let tmp = tempfile::tempdir().unwrap();
        let client = TestClient::new();

        let entity = EntityId::new();
        write_prior_file(tmp.path(), entity, b"int f() { return 0; }\n");

        let work_item_id = "func-a".to_string();
        let model = MockGenerationModel::with_function(
            &work_item_id,
            response_for(entity, b"int f() { return unknown(); }\n"),
        );

        let provider = MockBuildProvider::new(vec![MockBuildOutcome {
            success: false,
            diagnostics: vec![diag("C2065", 1, 0, "'unknown' : undeclared identifier")],
        }]);

        let mut orchestrator = build_orchestrator(
            &tmp,
            &client,
            &model,
            &provider,
            OrchestratorConfig::default(),
        );

        let work_items = vec![WorkItemContext {
            work_item_id: work_item_id.clone(),
            kind: WorkItemKind::Function,
            subject_entity: Some(entity),
            dependencies: Vec::new(),
            cluster_members: None,
        }];

        let stubbed: HashSet<String> = HashSet::new();
        let outcome = orchestrator
            .process_next_work_item(&work_items, &stubbed)
            .await
            .unwrap();
        assert_eq!(outcome, WorkItemOutcome::RepairDeferred);

        let create_count = client.count(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)));
        assert_eq!(
            create_count, 1,
            "deterministic repair must create one work item"
        );

        let llm_count =
            model.invocation_count("analyze_failure") + model.invocation_count("generate_repair");
        assert_eq!(
            llm_count, 0,
            "no LLM repair should be invoked for deterministic failure"
        );
    }

    #[tokio::test]
    async fn llm_repair_invoked_on_generated_code_defect_after_deterministic_exhaustion() {
        let tmp = tempfile::tempdir().unwrap();
        let client = TestClient::new();

        let entity = EntityId::new();
        write_prior_file(tmp.path(), entity, b"int f() { return 0; }\n");

        let work_item_id = "func-b".to_string();
        let model = MockGenerationModel::with_function(
            &work_item_id,
            response_for(entity, b"int f() { return 0; }\n"),
        )
        .with_repair_chain(vec![response_for(entity, b"int f() { return 1; }\n")]);

        let provider = MockBuildProvider::new(vec![
            MockBuildOutcome {
                success: false,
                diagnostics: vec![
                    diag("C2065", 1, 0, "'unknown' : undeclared identifier"),
                    diag("C1010", 1, 0, "unexpected end of file"),
                ],
            },
            MockBuildOutcome {
                success: true,
                diagnostics: Vec::new(),
            },
        ]);

        let mut orchestrator = build_orchestrator(
            &tmp,
            &client,
            &model,
            &provider,
            OrchestratorConfig::default(),
        );

        let work_items = vec![WorkItemContext {
            work_item_id: work_item_id.clone(),
            kind: WorkItemKind::Function,
            subject_entity: Some(entity),
            dependencies: Vec::new(),
            cluster_members: None,
        }];

        let stubbed: HashSet<String> = HashSet::new();
        let outcome = orchestrator
            .process_next_work_item(&work_items, &stubbed)
            .await
            .unwrap();
        assert_eq!(outcome, WorkItemOutcome::Completed);

        assert!(
            model.invocation_count("analyze_failure") >= 1,
            "LLM failure analysis must be invoked"
        );
        assert!(
            model.invocation_count("generate_repair") >= 1,
            "LLM repair generation must be invoked"
        );

        let completed = client
            .commands()
            .iter()
            .filter_map(|c| match c {
                ApplicationCommand::CompleteWorkItem(req) => Some(req.work_item_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(completed, vec![work_item_id]);
    }

    #[tokio::test]
    async fn bounded_retry_max_3_creates_blocked_work() {
        let tmp = tempfile::tempdir().unwrap();
        let client = TestClient::new();

        let entity = EntityId::new();
        write_prior_file(tmp.path(), entity, b"int f() { return 0; }\n");

        let work_item_id = "func-c".to_string();
        let model = MockGenerationModel::with_function(
            &work_item_id,
            response_for(entity, b"int f() { return 0; }\n"),
        )
        .with_repair_chain(vec![
            response_for(entity, b"int f() { return 1; }\n"),
            response_for(entity, b"int f() { return 2; }\n"),
            response_for(entity, b"int f() { return 3; }\n"),
        ]);

        let repeated = diag("C1010", 10, 5, "unexpected end of file");
        let provider = MockBuildProvider::new(vec![
            MockBuildOutcome {
                success: false,
                diagnostics: vec![repeated.clone()],
            },
            MockBuildOutcome {
                success: false,
                diagnostics: vec![repeated.clone()],
            },
            MockBuildOutcome {
                success: false,
                diagnostics: vec![repeated.clone()],
            },
            MockBuildOutcome {
                success: false,
                diagnostics: vec![repeated.clone()],
            },
        ]);

        let config = OrchestratorConfig {
            max_repair_attempts: 3,
            repeated_equivalent_failure_threshold: 4,
        };
        let mut orchestrator = build_orchestrator(&tmp, &client, &model, &provider, config);

        let work_items = vec![WorkItemContext {
            work_item_id: work_item_id.clone(),
            kind: WorkItemKind::Function,
            subject_entity: Some(entity),
            dependencies: Vec::new(),
            cluster_members: None,
        }];

        let stubbed: HashSet<String> = HashSet::new();
        let outcome = orchestrator
            .process_next_work_item(&work_items, &stubbed)
            .await
            .unwrap();
        assert_eq!(outcome, WorkItemOutcome::Blocked);

        let blocks = client
            .commands()
            .iter()
            .filter_map(|c| match c {
                ApplicationCommand::BlockWorkItem(req) => Some(req.reason.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(blocks.len(), 1, "exactly one BlockWorkItem expected");
        assert!(
            blocks[0].contains("RepeatedEquivalentFailure"),
            "expected RepeatedEquivalentFailure block, got: {}",
            blocks[0]
        );

        assert_eq!(provider.call_count(), 4, "expected four build attempts");
    }
}
