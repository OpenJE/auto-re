//! Verification-driven repair and LLM failure analysis (spec §13.4).
//!
//! On `Different`, `Inconclusive`, or `ExecutionFailed` comparison results, the
//! driver orchestrates an 8-step repair loop: record the comparison, associate
//! it with the affected entity, classify the likely cause, create investigation
//! work, run bounded LLM failure analysis, generate a repaired candidate,
//! rebuild through the controlled patch pipeline, and re-run the failed scenario
//! plus regression scenarios.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use autore_app::application_service::requests::{
    AddEvidenceRequest, CreateWorkItemsRequest, RecordRepairAttemptRequest,
    RecordVerificationComparisonRequest,
};
use autore_app::{ApplicationCommand, AutoReClient};
use autore_core::{Error, Result};
use autore_schema::domain::records::EvidenceRecord;
use autore_schema::domain::{Derivation, DerivationMethod, EvidenceValue, NamespacedId, Timestamp};
use autore_schema::ids::{EntityId, EvidenceRecordId, NativeArtifactId, ProjectId};

use crate::build::{BuildDiagnostic, BuildProviderTrait, DiagnosticSeverity, SuggestedWorkKind};
use crate::generation::orchestrator::{
    FailureAnalysisContext, FailureAnalysisResponse, GenerationModel, RepairGenerationContext,
};
use crate::generation::patch::{CandidatePatch, PatchPipeline};

use super::executor::ScenarioExecutor;
use super::types::{
    ComparisonLevel, ComparisonResult, ObservationSet, Scenario, VerificationComparison,
};

// ---------------------------------------------------------------------------
// Cause category
// ---------------------------------------------------------------------------

/// Likely root-cause category for a verification mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CauseCategory {
    /// Generated implementation does not match the original behavior.
    Implementation,
    /// Type/layout hypothesis is wrong.
    Type,
    /// Build or runtime environment issue.
    Environment,
    /// Scenario design, nondeterminism, or normalization problem.
    Scenario,
}

impl std::fmt::Display for CauseCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Implementation => f.write_str("Implementation"),
            Self::Type => f.write_str("Type"),
            Self::Environment => f.write_str("Environment"),
            Self::Scenario => f.write_str("Scenario"),
        }
    }
}

/// Deterministic policy that classifies a comparison result into a cause.
///
/// `diagnostics` are build diagnostics from a prior build attempt (Wave 6);
/// `accepted_types` is a placeholder list of type names that are already
/// accepted and therefore unlikely to be the root cause.
pub fn determine_cause(
    comparison_level: ComparisonLevel,
    comparison: &VerificationComparison,
    diagnostics: &[BuildDiagnostic],
    _accepted_types: &[String],
) -> CauseCategory {
    if comparison.overall == ComparisonResult::ExecutionFailed {
        if diagnostics
            .iter()
            .any(|d| d.diagnostic_code.starts_with("ENV"))
        {
            return CauseCategory::Environment;
        }
        return CauseCategory::Scenario;
    }

    if comparison.overall == ComparisonResult::Inconclusive {
        return CauseCategory::Scenario;
    }

    // Wave-8 layout mismatch in build diagnostics -> Type.
    if diagnostics.iter().any(|d| {
        let text = format!("{} {}", d.message, d.candidate_cause).to_ascii_lowercase();
        text.contains("layout") || text.contains("size mismatch") || text.contains("offset")
    }) {
        return CauseCategory::Type;
    }

    // Wave-6 dialect / build errors -> Environment if the build was broken.
    if diagnostics
        .iter()
        .any(|d| d.diagnostic_code.starts_with("C") || d.diagnostic_code.starts_with("LNK"))
    {
        return CauseCategory::Environment;
    }

    match comparison_level {
        ComparisonLevel::Function => CauseCategory::Implementation,
        ComparisonLevel::Cluster | ComparisonLevel::WholeProgram => {
            // Cluster/whole-program mismatches default to Implementation unless
            // a stronger signal was present above.
            CauseCategory::Implementation
        }
    }
}

// ---------------------------------------------------------------------------
// Bounded diff
// ---------------------------------------------------------------------------

/// Build a compact textual diff for LLM consumption.
///
/// Returns a summary of counts and the first `max_differences` differing
/// observations.
pub fn bounded_diff_for_llm(
    original: &ObservationSet,
    candidate: &ObservationSet,
    max_differences: usize,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "original: {} observations, candidate: {} observations\n",
        original.observations.len(),
        candidate.observations.len()
    ));

    if original.execution_failed {
        out.push_str("original execution failed\n");
        if let Some(d) = &original.execution_failure_diagnostic {
            out.push_str(&format!("  {}: {}\n", d.code, d.message));
        }
    }
    if candidate.execution_failed {
        out.push_str("candidate execution failed\n");
        if let Some(d) = &candidate.execution_failure_diagnostic {
            out.push_str(&format!("  {}: {}\n", d.code, d.message));
        }
    }

    let mut diff_count = 0;
    let max_len = original
        .observations
        .len()
        .max(candidate.observations.len());
    for i in 0..max_len {
        match (original.observations.get(i), candidate.observations.get(i)) {
            (Some(a), Some(b)) if a != b && diff_count < max_differences => {
                out.push_str(&format!("- [{i}] {a:?}\n"));
                out.push_str(&format!("+ [{i}] {b:?}\n"));
                diff_count += 1;
            }
            (Some(a), None) if diff_count < max_differences => {
                out.push_str(&format!("- [{i}] {a:?} (missing in candidate)\n"));
                diff_count += 1;
            }
            (None, Some(b)) if diff_count < max_differences => {
                out.push_str(&format!("+ [{i}] {b:?} (missing in original)\n"));
                diff_count += 1;
            }
            _ => {}
        }
    }

    if diff_count >= max_differences {
        let total_diffs = count_differences(original, candidate);
        let omitted = total_diffs.saturating_sub(max_differences);
        out.push_str(&format!("... and {omitted} more differences omitted\n"));
    }

    out
}

fn count_differences(original: &ObservationSet, candidate: &ObservationSet) -> usize {
    let mut count = 0;
    let max_len = original
        .observations
        .len()
        .max(candidate.observations.len());
    for i in 0..max_len {
        match (original.observations.get(i), candidate.observations.get(i)) {
            (Some(a), Some(b)) if a != b => count += 1,
            (Some(_), None) | (None, Some(_)) => count += 1,
            _ => {}
        }
    }
    count
}

// ---------------------------------------------------------------------------
// LLM request wrappers
// ---------------------------------------------------------------------------

/// Wrapper for LLM failure-analysis input.
#[derive(Debug, Clone)]
pub struct FailureAnalysisRequest {
    pub work_item_id: String,
    pub subject_entity: Option<EntityId>,
    pub comparison: VerificationComparison,
    pub bounded_diff: String,
}

impl FailureAnalysisRequest {
    /// Convert this request into a [`FailureAnalysisContext`] for the
    /// [`GenerationModel`] trait.
    pub fn into_generation_context(self) -> FailureAnalysisContext {
        FailureAnalysisContext {
            work_item_id: self.work_item_id,
            subject_entity: self.subject_entity,
            diagnostics: vec![comparison_to_diagnostic(
                &self.comparison,
                &self.bounded_diff,
            )],
        }
    }
}

/// Wrapper for LLM repair-generation input.
#[derive(Debug, Clone)]
pub struct RepairGenerationRequest {
    pub work_item_id: String,
    pub subject_entity: Option<EntityId>,
    pub prior_candidate_path: PathBuf,
    pub prior_candidate_bytes: Vec<u8>,
    pub analysis: FailureAnalysisResponse,
    pub bounded_diff: String,
}

impl RepairGenerationRequest {
    /// Convert this request into a [`RepairGenerationContext`] for the
    /// [`GenerationModel`] trait.
    pub fn into_generation_context(self) -> RepairGenerationContext {
        RepairGenerationContext {
            work_item_id: self.work_item_id,
            subject_entity: self.subject_entity,
            prior_candidate_path: self.prior_candidate_path,
            prior_candidate_bytes: self.prior_candidate_bytes,
            analysis: self.analysis,
            diagnostics: vec![BuildDiagnostic {
                diagnostic_code: "VERIFY_DIFF".to_string(),
                severity: DiagnosticSeverity::Error,
                file_path: PathBuf::new(),
                line: 0,
                column: 0,
                message: format!("verification bounded diff: {}", self.bounded_diff),
                candidate_cause: "verification mismatch".to_string(),
                suggested_work_kind: SuggestedWorkKind::Unknown,
            }],
        }
    }
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Outcome of a verification repair attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairResult {
    pub cause: CauseCategory,
    pub investigation_created: bool,
    pub analysis_invoked: bool,
    pub repair_applied: bool,
    pub build_success: bool,
    pub original_rerun_matches: bool,
    pub regression_runs_passed: bool,
}

/// Configuration for the repair driver.
#[derive(Debug, Clone, Copy)]
pub struct RepairConfig {
    /// Maximum number of differing observations to include in the LLM diff.
    pub max_diff_observations: usize,
}

impl Default for RepairConfig {
    fn default() -> Self {
        Self {
            max_diff_observations: 10,
        }
    }
}

/// Orchestrates the §13.4 8-step verification-driven repair flow.
pub struct VerificationRepairDriver<'a> {
    project_id: ProjectId,
    output_root: PathBuf,
    campaign_id: String,
    client: &'a dyn AutoReClient,
    build_provider: &'a dyn BuildProviderTrait,
    model: &'a dyn GenerationModel,
    scenario_executor: &'a ScenarioExecutor,
    config: RepairConfig,
}

impl<'a> VerificationRepairDriver<'a> {
    /// Creates a new repair driver.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: ProjectId,
        output_root: PathBuf,
        campaign_id: String,
        client: &'a dyn AutoReClient,
        build_provider: &'a dyn BuildProviderTrait,
        model: &'a dyn GenerationModel,
        scenario_executor: &'a ScenarioExecutor,
        config: RepairConfig,
    ) -> Self {
        Self {
            project_id,
            output_root,
            campaign_id,
            client,
            build_provider,
            model,
            scenario_executor,
            config,
        }
    }

    /// Orchestrates the 8-step repair flow for a verification mismatch.
    ///
    /// `regression_scenarios` are additional scenarios that must pass after the
    /// repair is applied (step 8 regression guard).
    #[allow(clippy::too_many_arguments)]
    pub async fn repair_on_mismatch(
        &self,
        scenario: &Scenario,
        comparison: &VerificationComparison,
        subject_entity: EntityId,
        original_observations: &ObservationSet,
        candidate_observations: &ObservationSet,
        regression_scenarios: &[Scenario],
        prior_build_diagnostics: &[BuildDiagnostic],
    ) -> Result<RepairResult> {
        // Step 1: Record the comparison.
        self.client
            .execute(ApplicationCommand::RecordVerificationComparison(
                RecordVerificationComparisonRequest {
                    project: self.project_id,
                    work_item_id: scenario.work_item_id.clone(),
                },
            ))?;

        // Step 2: Associate with affected entity via evidence.
        self.associate_comparison_with_entity(comparison, subject_entity)?;

        // Step 3: Determine cause.
        let cause = determine_cause(
            scenario.comparison_level,
            comparison,
            prior_build_diagnostics,
            &[],
        );

        // Step 4: Create investigation work item.
        let investigation_kind = match cause {
            CauseCategory::Environment | CauseCategory::Scenario => "dynamic",
            CauseCategory::Implementation | CauseCategory::Type => "static",
        };
        let description = format!(
            "Investigation: {investigation_kind} verification mismatch (cause={cause}) for entity {subject_entity}"
        );
        self.client.execute(ApplicationCommand::CreateWorkItems(
            CreateWorkItemsRequest {
                project: self.project_id,
                campaign_id: self.campaign_id.clone(),
                descriptions: vec![description],
            },
        ))?;

        // Step 5: LLM failure analysis on bounded diff.
        let bounded_diff = bounded_diff_for_llm(
            original_observations,
            candidate_observations,
            self.config.max_diff_observations,
        );
        let analysis_request = FailureAnalysisRequest {
            work_item_id: scenario.work_item_id.clone(),
            subject_entity: Some(subject_entity),
            comparison: comparison.clone(),
            bounded_diff: bounded_diff.clone(),
        };
        let analysis = self
            .model
            .analyze_failure(&analysis_request.into_generation_context())
            .await
            .map_err(|e| Error::Validation(e.to_string()))?;

        // Step 6: Generate repair candidate.
        let relative_path = entity_source_path(&subject_entity);
        let prior_candidate_bytes = self.read_prior_content(&relative_path).await?;
        let repair_request = RepairGenerationRequest {
            work_item_id: scenario.work_item_id.clone(),
            subject_entity: Some(subject_entity),
            prior_candidate_path: relative_path.clone(),
            prior_candidate_bytes: prior_candidate_bytes.clone(),
            analysis: analysis.clone(),
            bounded_diff,
        };
        let repair = self
            .model
            .generate_repair(&repair_request.into_generation_context())
            .await
            .map_err(|e| Error::Validation(e.to_string()))?;

        // Step 7: Rebuild via controlled patch pipeline.
        self.client
            .execute(ApplicationCommand::RecordRepairAttempt(
                RecordRepairAttemptRequest {
                    project: self.project_id,
                    work_item_id: scenario.work_item_id.clone(),
                },
            ))?;

        let candidate = CandidatePatch {
            relative_path: repair.relative_path,
            new_content_bytes: repair.candidate_bytes,
            prior_content_bytes: prior_candidate_bytes,
            source_evidence_refs: Vec::new(),
        };
        let mut declared = HashSet::new();
        declared.insert(candidate.relative_path.clone());
        let pipeline = PatchPipeline::new(
            self.output_root.clone(),
            self.project_id,
            self.build_provider,
            self.client,
        );
        let patch_outcome = pipeline
            .apply(vec![candidate], &declared, subject_entity)
            .await
            .map_err(|e| Error::Validation(e.to_string()))?;

        // Step 8: Re-run failed scenario and regression scenarios.
        let original_rerun = self.rerun_scenario(scenario).await?;
        let mut regression_runs_passed = true;
        for reg in regression_scenarios {
            let reg_result = self.rerun_scenario(reg).await?;
            if !reg_result.matches {
                regression_runs_passed = false;
                break;
            }
        }

        Ok(RepairResult {
            cause,
            investigation_created: true,
            analysis_invoked: true,
            repair_applied: patch_outcome.accepted,
            build_success: patch_outcome.build_success,
            original_rerun_matches: original_rerun.matches,
            regression_runs_passed,
        })
    }

    fn associate_comparison_with_entity(
        &self,
        comparison: &VerificationComparison,
        subject_entity: EntityId,
    ) -> Result<()> {
        let value = EvidenceValue::String(
            serde_json::to_string(comparison).unwrap_or_else(|_| "{}".to_string()),
        );
        let record = EvidenceRecord {
            id: EvidenceRecordId::new(),
            project: self.project_id,
            subject: subject_entity,
            predicate: NamespacedId::parse("verify.comparison")
                .map_err(|e| Error::Validation(e.0))?,
            value,
            derivation: Derivation::new(
                DerivationMethod::DeterministicAnalysis,
                NamespacedId::parse("verify.repair").map_err(|e| Error::Validation(e.0))?,
                Vec::new(),
                Vec::new(),
            ),
            provider_run: None,
            native_artifacts: Vec::<NativeArtifactId>::new(),
            assumptions: Vec::new(),
            created_at: Timestamp::now(),
        };
        self.client
            .execute(ApplicationCommand::AddEvidence(AddEvidenceRequest {
                project: self.project_id,
                record,
            }))?;
        Ok(())
    }

    async fn rerun_scenario(&self, scenario: &Scenario) -> Result<RerunResult> {
        let original = self.scenario_executor.execute_original(scenario).await?;
        let candidate = self
            .scenario_executor
            .execute_candidate(scenario, scenario.candidate_artifact_id)
            .await?;
        let comparison = self
            .scenario_executor
            .compare_and_record(scenario, &original, &candidate)
            .await?;
        Ok(RerunResult {
            matches: comparison.matches,
            comparison,
        })
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

#[derive(Debug, Clone)]
struct RerunResult {
    matches: bool,
    #[allow(dead_code)]
    comparison: VerificationComparison,
}

fn comparison_to_diagnostic(
    comparison: &VerificationComparison,
    bounded_diff: &str,
) -> BuildDiagnostic {
    BuildDiagnostic {
        diagnostic_code: "VERIFY_DIFF".to_string(),
        severity: DiagnosticSeverity::Error,
        file_path: PathBuf::new(),
        line: 0,
        column: 0,
        message: format!(
            "verification mismatch (overall={:?}): {bounded_diff}",
            comparison.overall
        ),
        candidate_cause: format!("comparison counts: {:?}", comparison.counts),
        suggested_work_kind: SuggestedWorkKind::Unknown,
    }
}

fn entity_source_path(entity_id: &EntityId) -> PathBuf {
    let hex = entity_id.as_uuid().as_simple().to_string();
    PathBuf::from("src/generated")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex[4..6])
        .join(&hex)
        .with_extension("cpp")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use autore_app::application_service::requests::{
        ApplicationCommand, CommandResult, CreateWorkItemsResponse, QueryResult,
        RecordRepairAttemptResponse, RecordVerificationComparisonResponse,
    };
    use autore_app::{ApplicationQuery, AutoReClient};
    use autore_core::Result;
    use autore_events::project_event_service::ProjectEventSubscription;
    use autore_schema::domain::{EvidenceValue, NamespacedId};
    use autore_schema::ids::{ArtifactId, EntityId, ProjectId, WorkItemId};

    use crate::build::types::{
        BuildConfigured, BuildDiagnostic, BuildLogs, CompileResult, CompileUnit,
        DiagnosticSeverity, GeneratorManifest, LinkResult as LinkResultType, RunTestResult,
    };
    use crate::build::{BuildProviderTrait, BuildResult};
    use crate::generation::orchestrator::{
        FailureAnalysisContext, FailureAnalysisResponse, GenerationContext, GenerationModel,
        GenerationModelError, GenerationResponse, RepairGenerationContext,
    };
    use crate::tests_support::RecordingAutoReClient;
    use crate::verification::types::{
        ComparisonCounts, ComparisonLevel, ComparisonResult, InitialState, Observation,
        ObservationSet, Scenario, VerificationComparison,
    };

    use super::super::{ObservationBackend, ObservationError, ScenarioExecutor};
    use super::{CauseCategory, RepairConfig, VerificationRepairDriver, determine_cause};

    fn debug_kind(name: &str) -> NamespacedId {
        NamespacedId::parse(&format!("debug.{name}")).unwrap()
    }

    fn base_observation_set(scenario_id: &str) -> ObservationSet {
        ObservationSet::new(scenario_id, ArtifactId::new())
    }

    fn make_scenario(id: &str, candidate_artifact_id: ArtifactId) -> Scenario {
        Scenario::new(
            id,
            WorkItemId::new().to_string(),
            EntityId::new(),
            InitialState::new(HashMap::new(), vec![], PathBuf::from("/tmp")),
            ArtifactId::new(),
            candidate_artifact_id,
            vec![],
            ComparisonLevel::Function,
        )
    }

    // -----------------------------------------------------------------------
    // Test client
    // -----------------------------------------------------------------------

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
            self.commands.lock().unwrap().push(command.clone());
            match &command {
                ApplicationCommand::RecordVerificationComparison(_) => {
                    Ok(CommandResult::VerificationComparisonRecorded(
                        RecordVerificationComparisonResponse {
                            comparison_id: autore_schema::ids::VerificationComparisonId::new()
                                .to_string(),
                        },
                    ))
                }
                ApplicationCommand::RecordRepairAttempt(_) => Ok(
                    CommandResult::RepairAttemptRecorded(RecordRepairAttemptResponse {
                        repair_id: uuid::Uuid::now_v7().to_string(),
                    }),
                ),
                ApplicationCommand::CreateWorkItems(req) => {
                    let ids: Vec<WorkItemId> =
                        req.descriptions.iter().map(|_| WorkItemId::new()).collect();
                    Ok(CommandResult::WorkItemsCreated(CreateWorkItemsResponse {
                        work_item_ids: ids.iter().map(|id| id.to_string()).collect(),
                    }))
                }
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
                ApplicationCommand::FailWorkItem(req) => Ok(CommandResult::WorkItemFailed(
                    autore_app::application_service::requests::FailWorkItemResponse {
                        work_item_id: req.work_item_id.clone(),
                    },
                )),
                _ => self.inner.execute(command),
            }
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

        fn subscribe_events(
            &self,
            project: ProjectId,
            after: u64,
        ) -> Result<ProjectEventSubscription> {
            self.inner.subscribe_events(project, after)
        }
    }

    // -----------------------------------------------------------------------
    // Mock build provider
    // -----------------------------------------------------------------------

    struct MockBuildProvider {
        success: bool,
        calls: Mutex<u32>,
    }

    impl MockBuildProvider {
        fn success() -> Self {
            Self {
                success: true,
                calls: Mutex::new(0),
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
            *self.calls.lock().unwrap() += 1;
            Ok(CompileResult {
                objects: Vec::new(),
                success: self.success,
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
            Ok(Vec::new())
        }
    }

    // -----------------------------------------------------------------------
    // Mock generation model
    // -----------------------------------------------------------------------

    #[derive(Debug, Default)]
    struct MockGenerationModel {
        analysis_responses: Mutex<Vec<FailureAnalysisResponse>>,
        repair_responses: Mutex<Vec<GenerationResponse>>,
        analysis_inputs: Mutex<Vec<FailureAnalysisContext>>,
    }

    impl MockGenerationModel {
        fn with_repair(relative_path: PathBuf, candidate_bytes: Vec<u8>) -> Self {
            let model = Self::default();
            *model.analysis_responses.lock().unwrap() = vec![FailureAnalysisResponse {
                diagnosis: "candidate returns wrong value".to_string(),
            }];
            *model.repair_responses.lock().unwrap() = vec![GenerationResponse {
                relative_path,
                candidate_bytes,
            }];
            model
        }
    }

    #[async_trait]
    impl GenerationModel for MockGenerationModel {
        async fn generate_function(
            &self,
            _ctx: &GenerationContext,
        ) -> std::result::Result<GenerationResponse, GenerationModelError> {
            Err(GenerationModelError::Other("not mocked".to_string()))
        }

        async fn generate_cluster(
            &self,
            _ctx: &GenerationContext,
        ) -> std::result::Result<GenerationResponse, GenerationModelError> {
            Err(GenerationModelError::Other("not mocked".to_string()))
        }

        async fn analyze_failure(
            &self,
            ctx: &FailureAnalysisContext,
        ) -> std::result::Result<FailureAnalysisResponse, GenerationModelError> {
            self.analysis_inputs.lock().unwrap().push(ctx.clone());
            let mut guard = self.analysis_responses.lock().unwrap();
            if guard.is_empty() {
                Err(GenerationModelError::Other(
                    "no analysis response".to_string(),
                ))
            } else {
                Ok(guard.remove(0))
            }
        }

        async fn generate_repair(
            &self,
            _ctx: &RepairGenerationContext,
        ) -> std::result::Result<GenerationResponse, GenerationModelError> {
            let mut guard = self.repair_responses.lock().unwrap();
            if guard.is_empty() {
                Err(GenerationModelError::Other(
                    "no repair response".to_string(),
                ))
            } else {
                Ok(guard.remove(0))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Mock observation backend
    // -----------------------------------------------------------------------

    struct MockObservationBackend {
        captures: Mutex<Vec<String>>,
        originals: HashMap<String, ObservationSet>,
        candidates: HashMap<String, ObservationSet>,
    }

    impl MockObservationBackend {
        fn new(
            originals: HashMap<String, ObservationSet>,
            candidates: HashMap<String, ObservationSet>,
        ) -> Self {
            Self {
                captures: Mutex::new(Vec::new()),
                originals,
                candidates,
            }
        }

        fn captured_scenarios(&self) -> Vec<String> {
            self.captures.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ObservationBackend for MockObservationBackend {
        async fn capture(
            &self,
            scenario: &Scenario,
            target_artifact_id: ArtifactId,
        ) -> std::result::Result<ObservationSet, ObservationError> {
            self.captures.lock().unwrap().push(scenario.id.clone());
            if target_artifact_id == scenario.executable_artifact_id {
                Ok(self
                    .originals
                    .get(&scenario.id)
                    .cloned()
                    .unwrap_or_else(|| base_observation_set(&scenario.id)))
            } else {
                Ok(self
                    .candidates
                    .get(&scenario.id)
                    .cloned()
                    .unwrap_or_else(|| base_observation_set(&scenario.id)))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Cause classification tests
    // -----------------------------------------------------------------------

    #[test]
    fn determine_cause_execution_failed_with_env_diagnostic_is_environment() {
        let comparison = VerificationComparison::new(
            "s1",
            EvidenceValue::String("{}".to_string()),
            EvidenceValue::String("{}".to_string()),
            vec![ComparisonResult::ExecutionFailed],
            ComparisonCounts::zero(),
            ComparisonResult::ExecutionFailed,
        );
        let diagnostics = vec![BuildDiagnostic {
            diagnostic_code: "ENV_DOCKER".to_string(),
            severity: DiagnosticSeverity::Error,
            file_path: PathBuf::new(),
            line: 0,
            column: 0,
            message: "docker daemon down".to_string(),
            candidate_cause: String::new(),
            suggested_work_kind: crate::build::SuggestedWorkKind::Unknown,
        }];
        assert_eq!(
            determine_cause(ComparisonLevel::Function, &comparison, &diagnostics, &[]),
            CauseCategory::Environment
        );
    }

    #[test]
    fn determine_cause_layout_mismatch_is_type() {
        let comparison = VerificationComparison::new(
            "s1",
            EvidenceValue::String("{}".to_string()),
            EvidenceValue::String("{}".to_string()),
            vec![ComparisonResult::Different],
            ComparisonCounts::zero(),
            ComparisonResult::Different,
        );
        let diagnostics = vec![BuildDiagnostic {
            diagnostic_code: "C2440".to_string(),
            severity: DiagnosticSeverity::Error,
            file_path: PathBuf::new(),
            line: 0,
            column: 0,
            message: "struct size mismatch".to_string(),
            candidate_cause: "layout offset does not match".to_string(),
            suggested_work_kind: crate::build::SuggestedWorkKind::Unknown,
        }];
        assert_eq!(
            determine_cause(ComparisonLevel::Function, &comparison, &diagnostics, &[]),
            CauseCategory::Type
        );
    }

    #[test]
    fn determine_cause_function_level_different_is_implementation() {
        let comparison = VerificationComparison::new(
            "s1",
            EvidenceValue::String("{}".to_string()),
            EvidenceValue::String("{}".to_string()),
            vec![ComparisonResult::Different],
            ComparisonCounts::zero(),
            ComparisonResult::Different,
        );
        assert_eq!(
            determine_cause(ComparisonLevel::Function, &comparison, &[], &[]),
            CauseCategory::Implementation
        );
    }

    // -----------------------------------------------------------------------
    // Bounded diff test
    // -----------------------------------------------------------------------

    #[test]
    fn bounded_diff_summarizes_and_limits_observations() {
        let original = base_observation_set("s1").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let candidate = base_observation_set("s1").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 2}),
        ));
        let diff = super::bounded_diff_for_llm(&original, &candidate, 5);
        assert!(diff.contains("original: 1 observations"));
        assert!(diff.contains("rax"));
        assert!(diff.contains("Number(1)"));
        assert!(diff.contains("Number(2)"));
    }

    // -----------------------------------------------------------------------
    // Repair driver tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn mismatch_creates_investigation_work() {
        let tmp = tempfile::tempdir().unwrap();
        let client = Arc::new(TestClient::new());
        let build_provider = MockBuildProvider::success();
        let candidate_artifact = ArtifactId::new();
        let scenario = make_scenario("s1", candidate_artifact);
        let subject_entity = EntityId::new();

        let original = base_observation_set("s1").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let candidate = base_observation_set("s1").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 2}),
        ));

        let backend = Arc::new(MockObservationBackend::new(
            [("s1".to_string(), original.clone())].into_iter().collect(),
            [("s1".to_string(), candidate.clone())]
                .into_iter()
                .collect(),
        ));
        let executor = ScenarioExecutor::new(ProjectId::new(), client.clone(), backend);

        let comparison = executor
            .compare_and_record(&scenario, &original, &candidate)
            .await
            .unwrap();

        let rel_path = super::entity_source_path(&subject_entity);
        let model = MockGenerationModel::with_repair(rel_path, b"int f() { return 1; }\n".to_vec());

        let driver = VerificationRepairDriver::new(
            ProjectId::new(),
            tmp.path().to_path_buf(),
            "campaign-1".to_string(),
            &*client,
            &build_provider,
            &model,
            &executor,
            RepairConfig::default(),
        );

        let _result = driver
            .repair_on_mismatch(
                &scenario,
                &comparison,
                subject_entity,
                &original,
                &candidate,
                &[],
                &[],
            )
            .await
            .unwrap();

        let create_count = client.count(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)));
        assert_eq!(create_count, 1, "investigation work item must be created");

        let investigation_desc = client
            .commands()
            .iter()
            .find_map(|c| match c {
                ApplicationCommand::CreateWorkItems(req) => req.descriptions.first().cloned(),
                _ => None,
            })
            .expect("description present");
        assert!(investigation_desc.contains("Investigation:"));
        assert!(investigation_desc.contains("static"));
        assert!(investigation_desc.contains("Implementation"));
    }

    #[tokio::test]
    async fn llm_analysis_failure_invoked_with_bounded_diff() {
        let tmp = tempfile::tempdir().unwrap();
        let client = Arc::new(TestClient::new());
        let build_provider = MockBuildProvider::success();
        let candidate_artifact = ArtifactId::new();
        let scenario = make_scenario("s2", candidate_artifact);
        let subject_entity = EntityId::new();

        let original = base_observation_set("s2").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let candidate = base_observation_set("s2").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 2}),
        ));

        let backend = Arc::new(MockObservationBackend::new(
            [("s2".to_string(), original.clone())].into_iter().collect(),
            [("s2".to_string(), candidate.clone())]
                .into_iter()
                .collect(),
        ));
        let executor = ScenarioExecutor::new(ProjectId::new(), client.clone(), backend);

        let comparison = executor
            .compare_and_record(&scenario, &original, &candidate)
            .await
            .unwrap();

        let rel_path = super::entity_source_path(&subject_entity);
        let model = MockGenerationModel::with_repair(rel_path, b"int f() { return 1; }\n".to_vec());

        let driver = VerificationRepairDriver::new(
            ProjectId::new(),
            tmp.path().to_path_buf(),
            "campaign-1".to_string(),
            &*client,
            &build_provider,
            &model,
            &executor,
            RepairConfig::default(),
        );

        let _result = driver
            .repair_on_mismatch(
                &scenario,
                &comparison,
                subject_entity,
                &original,
                &candidate,
                &[],
                &[],
            )
            .await
            .unwrap();

        let inputs = model.analysis_inputs.lock().unwrap().clone();
        assert_eq!(inputs.len(), 1, "analyze_failure must be invoked once");
        let diagnostic_message = &inputs[0].diagnostics[0].message;
        assert!(
            diagnostic_message.contains("rax"),
            "bounded diff must mention the differing observation: {diagnostic_message}"
        );
    }

    #[tokio::test]
    async fn repair_candidate_rebuilds() {
        let tmp = tempfile::tempdir().unwrap();
        let client = Arc::new(TestClient::new());
        let build_provider = MockBuildProvider::success();
        let candidate_artifact = ArtifactId::new();
        let scenario = make_scenario("s3", candidate_artifact);
        let subject_entity = EntityId::new();

        let original = base_observation_set("s3").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let candidate = base_observation_set("s3").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 2}),
        ));

        let backend = Arc::new(MockObservationBackend::new(
            [("s3".to_string(), original.clone())].into_iter().collect(),
            [("s3".to_string(), candidate.clone())]
                .into_iter()
                .collect(),
        ));
        let executor = ScenarioExecutor::new(ProjectId::new(), client.clone(), backend);

        let comparison = executor
            .compare_and_record(&scenario, &original, &candidate)
            .await
            .unwrap();

        let rel_path = super::entity_source_path(&subject_entity);
        let model =
            MockGenerationModel::with_repair(rel_path.clone(), b"int f() { return 1; }\n".to_vec());

        let driver = VerificationRepairDriver::new(
            ProjectId::new(),
            tmp.path().to_path_buf(),
            "campaign-1".to_string(),
            &*client,
            &build_provider,
            &model,
            &executor,
            RepairConfig::default(),
        );

        let result = driver
            .repair_on_mismatch(
                &scenario,
                &comparison,
                subject_entity,
                &original,
                &candidate,
                &[],
                &[],
            )
            .await
            .unwrap();

        assert!(result.repair_applied, "patch must be accepted");
        assert!(result.build_success, "build must succeed");
        assert_eq!(
            build_provider.call_count(),
            1,
            "build provider must be invoked"
        );

        let repair_attempts =
            client.count(|c| matches!(c, ApplicationCommand::RecordRepairAttempt(_)));
        assert_eq!(repair_attempts, 1, "repair attempt must be recorded");
    }

    #[tokio::test]
    async fn invocation_re_runs_scenarios_and_regression() {
        let tmp = tempfile::tempdir().unwrap();
        let client = Arc::new(TestClient::new());
        let build_provider = MockBuildProvider::success();
        let candidate_artifact = ArtifactId::new();
        let scenario = make_scenario("s4", candidate_artifact);
        let subject_entity = EntityId::new();

        let original = base_observation_set("s4").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let candidate = base_observation_set("s4").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 2}),
        ));

        // Regression scenarios: one passes, one fails.
        let reg1 = make_scenario("reg1", candidate_artifact);
        let reg2 = make_scenario("reg2", candidate_artifact);
        let reg1_candidate = base_observation_set("reg1").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let reg2_candidate = base_observation_set("reg2").add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 99}),
        ));

        let mut originals = HashMap::new();
        originals.insert("s4".to_string(), original.clone());
        originals.insert("reg1".to_string(), original.clone());
        originals.insert("reg2".to_string(), original.clone());

        // The mock always returns the repaired candidate; the initial mismatch is
        // supplied directly to compare_and_record below.
        let mut candidates = HashMap::new();
        candidates.insert("s4".to_string(), original.clone());
        candidates.insert("reg1".to_string(), reg1_candidate);
        candidates.insert("reg2".to_string(), reg2_candidate);

        let backend = Arc::new(MockObservationBackend::new(originals, candidates));
        let executor = ScenarioExecutor::new(ProjectId::new(), client.clone(), backend.clone());

        let comparison = executor
            .compare_and_record(&scenario, &original, &candidate)
            .await
            .unwrap();

        let rel_path = super::entity_source_path(&subject_entity);
        let model = MockGenerationModel::with_repair(rel_path, b"int f() { return 1; }\n".to_vec());

        let driver = VerificationRepairDriver::new(
            ProjectId::new(),
            tmp.path().to_path_buf(),
            "campaign-1".to_string(),
            &*client,
            &build_provider,
            &model,
            &executor,
            RepairConfig::default(),
        );

        let result = driver
            .repair_on_mismatch(
                &scenario,
                &comparison,
                subject_entity,
                &original,
                &candidate,
                &[reg1, reg2],
                &[],
            )
            .await
            .unwrap();

        assert!(
            result.original_rerun_matches,
            "original rerun must match: {result:?}"
        );
        assert!(
            !result.regression_runs_passed,
            "regression guard must fail when a regression scenario mismatches"
        );

        let captured = backend.captured_scenarios();
        assert!(captured.contains(&"s4".to_string()));
        assert!(captured.contains(&"reg1".to_string()));
        assert!(captured.contains(&"reg2".to_string()));
    }
}
