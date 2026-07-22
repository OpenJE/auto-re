use std::path::PathBuf;

use autore_schema::domain::records::{
    Artifact, Contradiction, EvidenceRecord, Hypothesis, Operation, Project, Provider, ProviderRun,
    SemanticEntity, VerificationRecord,
};
use autore_schema::domain::{
    ContentHash, EnvironmentIdentity, EvidenceValue, NamespacedId, StableEntityKey,
};
use autore_schema::ids::{
    ArtifactId, ContradictionId, EntityId, EvidenceRecordId, HypothesisId, OperationId, ProjectId,
    ProviderId, ProviderRunId, VerificationRecordId,
};

// ---------------------------------------------------------------------------
// Project commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CreateProjectRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CreateProjectResponse {
    pub project: Project,
}

// ---------------------------------------------------------------------------
// Artifact commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RegisterArtifactRequest {
    pub project: ProjectId,
    pub source_path: PathBuf,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RegisterArtifactResponse {
    pub artifact: Artifact,
}

// ---------------------------------------------------------------------------
// Entity commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RegisterEntityRequest {
    pub project: ProjectId,
    pub kind: String,
    pub stable_key: Option<StableEntityKey>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RegisterEntityResponse {
    pub entity: SemanticEntity,
}

// ---------------------------------------------------------------------------
// Provider commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RegisterProviderRequest {
    pub project: ProjectId,
    pub provider: Provider,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RegisterProviderResponse {
    pub provider: Provider,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StartProviderRunRequest {
    pub project: ProjectId,
    pub provider: ProviderId,
    pub operation: String,
    pub input_artifacts: Vec<ArtifactId>,
    pub configuration_artifact: Option<ArtifactId>,
    pub configuration_hash: ContentHash,
    pub environment: EnvironmentIdentity,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StartProviderRunResponse {
    pub run: ProviderRun,
}

// ---------------------------------------------------------------------------
// Evidence / Hypothesis / Contradiction / Verification commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AddEvidenceRequest {
    pub project: ProjectId,
    pub record: EvidenceRecord,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AddEvidenceResponse {
    pub id: EvidenceRecordId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AddHypothesisRequest {
    pub project: ProjectId,
    pub subject: EntityId,
    pub predicate: String,
    pub candidate: EvidenceValue,
    pub confidence_score: f64,
    pub confidence_rationale: Option<String>,
    pub supporting_evidence: Vec<EvidenceRecordId>,
    pub contradicting_evidence: Vec<EvidenceRecordId>,
    pub derived_from: Vec<HypothesisId>,
    pub status: autore_schema::domain::records::HypothesisStatus,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AddHypothesisResponse {
    pub id: HypothesisId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ChangeHypothesisStatusRequest {
    pub project: ProjectId,
    pub id: HypothesisId,
    pub status: autore_schema::domain::records::HypothesisStatus,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ChangeHypothesisStatusResponse {
    pub hypothesis: Hypothesis,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RecordContradictionRequest {
    pub project: ProjectId,
    pub contradiction: Contradiction,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RecordContradictionResponse {
    pub id: ContradictionId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AddVerificationRequest {
    pub project: ProjectId,
    pub record: VerificationRecord,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AddVerificationResponse {
    pub id: VerificationRecordId,
}

// ---------------------------------------------------------------------------
// Operation commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CancelOperationRequest {
    pub project: ProjectId,
    pub id: OperationId,
    pub requested_by: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CancelOperationResponse {
    pub operation: Operation,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidateProjectRequest {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidateProjectResponse {
    pub result: ValidationResult,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MigrateProjectRequest {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MigrateProjectResponse {
    pub operation: Operation,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RebuildIndexesRequest {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RebuildIndexesResponse {
    pub operation: Operation,
}

// ---------------------------------------------------------------------------
// Stage 1 – Reconstruction campaign & work-item lifecycle commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreateReconstructionCampaignRequest {
    pub project: ProjectId,
    pub name: String,
    pub binary_artifact_id: ArtifactId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreateReconstructionCampaignResponse {
    pub campaign_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreateWorkItemsRequest {
    pub project: ProjectId,
    pub campaign_id: String,
    pub descriptions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreateWorkItemsResponse {
    pub work_item_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordWorkDependencyRequest {
    pub project: ProjectId,
    pub work_item_id: String,
    pub depends_on: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordWorkDependencyResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromoteWorkItemRequest {
    pub project: ProjectId,
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromoteWorkItemResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LeaseWorkItemRequest {
    pub project: ProjectId,
    pub work_item_id: String,
    pub worker_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LeaseWorkItemResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RenewWorkLeaseRequest {
    pub project: ProjectId,
    pub work_item_id: String,
    pub worker_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RenewWorkLeaseResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompleteWorkItemRequest {
    pub project: ProjectId,
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompleteWorkItemResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FailWorkItemRequest {
    pub project: ProjectId,
    pub work_item_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FailWorkItemResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockWorkItemRequest {
    pub project: ProjectId,
    pub work_item_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockWorkItemResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InvalidateWorkItemRequest {
    pub project: ProjectId,
    pub work_item_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InvalidateWorkItemResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequeueWorkItemRequest {
    pub project: ProjectId,
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequeueWorkItemResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockWorkWithReasonRequest {
    pub project: ProjectId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockWorkWithReasonResponse {
    pub blocked_count: u32,
}

// ---------------------------------------------------------------------------
// Stage 1 – Provider installation & instance commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RegisterProviderInstallationRequest {
    pub project: ProjectId,
    pub provider_id: ProviderId,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RegisterProviderInstallationResponse {
    pub installation_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RegisterProviderInstanceRequest {
    pub project: ProjectId,
    pub installation_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RegisterProviderInstanceResponse {
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StopProviderInstanceRequest {
    pub project: ProjectId,
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StopProviderInstanceResponse {
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportProviderRunResultRequest {
    pub project: ProjectId,
    pub run_id: ProviderRunId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportProviderRunResultResponse {
    pub run_id: ProviderRunId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportDynamicObservationRequest {
    pub project: ProjectId,
    pub observation: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportDynamicObservationResponse {
    pub observation_id: String,
}

// ---------------------------------------------------------------------------
// Stage 1 – Build, verification, and generated-source commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordBuildAttemptRequest {
    pub project: ProjectId,
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordBuildAttemptResponse {
    pub attempt_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunBuildRequest {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunBuildResponse {
    pub build_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordVerificationComparisonRequest {
    pub project: ProjectId,
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordVerificationComparisonResponse {
    pub comparison_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RegisterGeneratedSourceMappingRequest {
    pub project: ProjectId,
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RegisterGeneratedSourceMappingResponse {
    pub mapping_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InvalidateGeneratedSourceRequest {
    pub project: ProjectId,
    pub mapping_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InvalidateGeneratedSourceResponse {
    pub mapping_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportGeneratedSourceCandidatesRequest {
    pub project: ProjectId,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportGeneratedSourceCandidatesResponse {
    pub imported_count: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScheduleVerificationRegressionRequest {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScheduleVerificationRegressionResponse {
    pub regression_id: String,
}

// ---------------------------------------------------------------------------
// Stage 1 – Repair, hypothesis policy, and coordinator commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordRepairAttemptRequest {
    pub project: ProjectId,
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordRepairAttemptResponse {
    pub repair_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AcceptHypothesisPolicyDrivenRequest {
    pub project: ProjectId,
    pub hypothesis_id: HypothesisId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AcceptHypothesisPolicyDrivenResponse {
    pub hypothesis_id: HypothesisId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PauseCoordinatorRequest {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PauseCoordinatorResponse {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResumeCoordinatorRequest {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResumeCoordinatorResponse {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StopCoordinatorRequest {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StopCoordinatorResponse {
    pub project: ProjectId,
}

// ---------------------------------------------------------------------------
// Validation report types
// ---------------------------------------------------------------------------

/// Severity of a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

/// A single finding from one of the project-wide validation checks.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ValidationFinding {
    /// Human-readable identifier for the check that produced this finding.
    pub check: String,
    /// Whether this finding causes validation to fail.
    pub severity: ValidationSeverity,
    /// Human-readable description of the problem.
    pub message: String,
    /// Optional identifier of the record involved, if applicable.
    pub record_id: Option<String>,
}

/// Stable, versioned report produced by project-wide validation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ValidationReport {
    /// Schema version of this report format.
    pub schema_version: String,
    /// Project that was validated.
    pub project_id: ProjectId,
    /// `true` when no error findings were found.
    pub passed: bool,
    /// All findings from every check, ordered by check then record.
    pub findings: Vec<ValidationFinding>,
}

impl ValidationReport {
    /// Current report format version.
    pub const SCHEMA_VERSION: &str = "1.0.0";

    /// Creates an empty passed report for the given project.
    pub fn passed(project_id: ProjectId) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION.to_string(),
            project_id,
            passed: true,
            findings: vec![],
        }
    }

    /// Creates a failed report with the supplied findings.
    pub fn failed(project_id: ProjectId, findings: Vec<ValidationFinding>) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION.to_string(),
            project_id,
            passed: false,
            findings,
        }
    }
}

/// Result of running project-wide validation.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ValidationResult {
    /// Validation passed with no error findings.
    Passed(ValidationReport),
    /// Validation failed; the report contains one or more error findings.
    Failed(ValidationReport),
}

impl ValidationResult {
    /// Returns the contained validation report.
    pub fn report(&self) -> &ValidationReport {
        match self {
            ValidationResult::Passed(r) | ValidationResult::Failed(r) => r,
        }
    }
}

// ---------------------------------------------------------------------------
// Command enum and result enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ApplicationCommand {
    CreateProject(CreateProjectRequest),
    RegisterArtifact(RegisterArtifactRequest),
    RegisterEntity(RegisterEntityRequest),
    RegisterProvider(RegisterProviderRequest),
    StartProviderRun(StartProviderRunRequest),
    AddEvidence(AddEvidenceRequest),
    AddHypothesis(AddHypothesisRequest),
    ChangeHypothesisStatus(ChangeHypothesisStatusRequest),
    RecordContradiction(RecordContradictionRequest),
    AddVerification(AddVerificationRequest),
    CancelOperation(CancelOperationRequest),
    ValidateProject(ValidateProjectRequest),
    MigrateProject(MigrateProjectRequest),
    RebuildIndexes(RebuildIndexesRequest),
    // Stage 1 variants
    CreateReconstructionCampaign(CreateReconstructionCampaignRequest),
    CreateWorkItems(CreateWorkItemsRequest),
    RecordWorkDependency(RecordWorkDependencyRequest),
    PromoteWorkItem(PromoteWorkItemRequest),
    LeaseWorkItem(LeaseWorkItemRequest),
    RenewWorkLease(RenewWorkLeaseRequest),
    CompleteWorkItem(CompleteWorkItemRequest),
    FailWorkItem(FailWorkItemRequest),
    BlockWorkItem(BlockWorkItemRequest),
    InvalidateWorkItem(InvalidateWorkItemRequest),
    RequeueWorkItem(RequeueWorkItemRequest),
    BlockWorkWithReason(BlockWorkWithReasonRequest),
    RegisterProviderInstallation(RegisterProviderInstallationRequest),
    RegisterProviderInstance(RegisterProviderInstanceRequest),
    StopProviderInstance(StopProviderInstanceRequest),
    ImportProviderRunResult(ImportProviderRunResultRequest),
    ImportDynamicObservation(ImportDynamicObservationRequest),
    RecordBuildAttempt(RecordBuildAttemptRequest),
    RunBuild(RunBuildRequest),
    RecordVerificationComparison(RecordVerificationComparisonRequest),
    RegisterGeneratedSourceMapping(RegisterGeneratedSourceMappingRequest),
    InvalidateGeneratedSource(InvalidateGeneratedSourceRequest),
    ImportGeneratedSourceCandidates(ImportGeneratedSourceCandidatesRequest),
    ScheduleVerificationRegression(ScheduleVerificationRegressionRequest),
    RecordRepairAttempt(RecordRepairAttemptRequest),
    AcceptHypothesisPolicyDriven(AcceptHypothesisPolicyDrivenRequest),
    PauseCoordinator(PauseCoordinatorRequest),
    ResumeCoordinator(ResumeCoordinatorRequest),
    StopCoordinator(StopCoordinatorRequest),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum CommandResult {
    ProjectCreated(CreateProjectResponse),
    ArtifactRegistered(RegisterArtifactResponse),
    EntityRegistered(RegisterEntityResponse),
    ProviderRegistered(RegisterProviderResponse),
    ProviderRunStarted(StartProviderRunResponse),
    EvidenceAdded(AddEvidenceResponse),
    HypothesisAdded(AddHypothesisResponse),
    HypothesisStatusChanged(ChangeHypothesisStatusResponse),
    ContradictionRecorded(RecordContradictionResponse),
    VerificationAdded(AddVerificationResponse),
    OperationCancelled(CancelOperationResponse),
    ProjectValidated(ValidateProjectResponse),
    ProjectMigrated(MigrateProjectResponse),
    IndexesRebuilt(RebuildIndexesResponse),
    // Stage 1 variants
    CampaignCreated(CreateReconstructionCampaignResponse),
    WorkItemsCreated(CreateWorkItemsResponse),
    WorkDependencyRecorded(RecordWorkDependencyResponse),
    WorkItemPromoted(PromoteWorkItemResponse),
    WorkItemLeased(LeaseWorkItemResponse),
    WorkLeaseRenewed(RenewWorkLeaseResponse),
    WorkItemCompleted(CompleteWorkItemResponse),
    WorkItemFailed(FailWorkItemResponse),
    WorkItemBlocked(BlockWorkItemResponse),
    WorkItemInvalidated(InvalidateWorkItemResponse),
    WorkItemRequeued(RequeueWorkItemResponse),
    WorkBlocked(BlockWorkWithReasonResponse),
    ProviderInstallationRegistered(RegisterProviderInstallationResponse),
    ProviderInstanceRegistered(RegisterProviderInstanceResponse),
    ProviderInstanceStopped(StopProviderInstanceResponse),
    ProviderRunResultImported(ImportProviderRunResultResponse),
    DynamicObservationImported(ImportDynamicObservationResponse),
    BuildAttemptRecorded(RecordBuildAttemptResponse),
    BuildRun(RunBuildResponse),
    VerificationComparisonRecorded(RecordVerificationComparisonResponse),
    GeneratedSourceMappingRegistered(RegisterGeneratedSourceMappingResponse),
    GeneratedSourceInvalidated(InvalidateGeneratedSourceResponse),
    GeneratedSourceCandidatesImported(ImportGeneratedSourceCandidatesResponse),
    VerificationRegressionScheduled(ScheduleVerificationRegressionResponse),
    RepairAttemptRecorded(RecordRepairAttemptResponse),
    HypothesisAcceptedPolicyDriven(AcceptHypothesisPolicyDrivenResponse),
    CoordinatorPaused(PauseCoordinatorResponse),
    CoordinatorResumed(ResumeCoordinatorResponse),
    CoordinatorStopped(StopCoordinatorResponse),
}

// ---------------------------------------------------------------------------
// Query request / response structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetProjectSummaryQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProjectSummaryResponse {
    pub project: Project,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetArtifactQuery {
    pub id: ArtifactId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ArtifactResponse {
    pub artifact: Artifact,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListArtifactsQuery {
    pub project: ProjectId,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ArtifactsResponse {
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetEntityQuery {
    pub id: EntityId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EntityResponse {
    pub entity: SemanticEntity,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListEntitiesQuery {
    pub project: ProjectId,
    pub offset: u32,
    pub limit: u32,
    pub kind_filter: Option<NamespacedId>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EntitiesResponse {
    pub entities: Vec<SemanticEntity>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetProviderQuery {
    pub id: ProviderId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProviderResponse {
    pub provider: Provider,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListProvidersQuery;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProvidersResponse {
    pub providers: Vec<Provider>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetProviderRunQuery {
    pub id: ProviderRunId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProviderRunResponse {
    pub run: ProviderRun,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListProviderRunsQuery {
    pub project: ProjectId,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProviderRunsResponse {
    pub runs: Vec<ProviderRun>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetEvidenceQuery {
    pub id: EvidenceRecordId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EvidenceResponse {
    pub record: EvidenceRecord,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListEvidenceQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EvidenceListResponse {
    pub records: Vec<EvidenceRecord>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetHypothesisQuery {
    pub id: HypothesisId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HypothesisResponse {
    pub hypothesis: Hypothesis,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListHypothesesQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HypothesesResponse {
    pub hypotheses: Vec<Hypothesis>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetContradictionQuery {
    pub id: ContradictionId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ContradictionResponse {
    pub contradiction: Contradiction,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListContradictionsQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ContradictionsResponse {
    pub contradictions: Vec<Contradiction>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetVerificationQuery {
    pub id: VerificationRecordId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VerificationResponse {
    pub record: VerificationRecord,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListVerificationsQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VerificationsResponse {
    pub records: Vec<VerificationRecord>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetOperationQuery {
    pub id: OperationId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct OperationResponse {
    pub operation: Operation,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListOperationsQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct OperationsResponse {
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ListEventsQuery {
    pub project: ProjectId,
    pub after_sequence: u64,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EventsResponse {
    pub events: Vec<autore_schema::domain::records::ProjectEvent>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GetValidationReportQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidationReportResponse {
    pub report: ValidationReport,
}

// ---------------------------------------------------------------------------
// Stage 1 – Campaign & work-item queries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GetCampaignQuery {
    pub campaign_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CampaignResponse {
    pub campaign_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListWorkItemsQuery {
    pub project: ProjectId,
    pub campaign_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkItemsResponse {
    pub work_items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GetWorkItemQuery {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkItemResponse {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListWorkItemDependenciesQuery {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkItemDependenciesResponse {
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListWorkItemBlockersQuery {
    pub work_item_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkItemBlockersResponse {
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListExpiredLeasesQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExpiredLeasesResponse {
    pub expired: Vec<String>,
}

// ---------------------------------------------------------------------------
// Stage 1 – Provider installation & instance queries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GetProviderInstallationQuery {
    pub installation_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderInstallationResponse {
    pub installation_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListProviderInstallationsQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderInstallationsResponse {
    pub installations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListProviderInstancesQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderInstancesResponse {
    pub instances: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GetProviderInstanceQuery {
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderInstanceResponse {
    pub instance_id: String,
}

// ---------------------------------------------------------------------------
// Stage 1 – Build, verification, and source-mapping queries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GetBuildStatusQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BuildStatusResponse {
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListBuildDiagnosticsQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BuildDiagnosticsResponse {
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GetVerificationCoverageQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VerificationCoverageResponse {
    pub covered: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListGeneratedSourceMappingsQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeneratedSourceMappingsResponse {
    pub mappings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListConflictsQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConflictsResponse {
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ListBlockedReasonsQuery {
    pub project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockedReasonsResponse {
    pub reasons: Vec<String>,
}

// ---------------------------------------------------------------------------
// Query enum and result enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ApplicationQuery {
    GetProjectSummary(GetProjectSummaryQuery),
    GetArtifact(GetArtifactQuery),
    ListArtifacts(ListArtifactsQuery),
    GetEntity(GetEntityQuery),
    ListEntities(ListEntitiesQuery),
    GetProvider(GetProviderQuery),
    ListProviders(ListProvidersQuery),
    GetProviderRun(GetProviderRunQuery),
    ListProviderRuns(ListProviderRunsQuery),
    GetEvidence(GetEvidenceQuery),
    ListEvidence(ListEvidenceQuery),
    GetHypothesis(GetHypothesisQuery),
    ListHypotheses(ListHypothesesQuery),
    GetContradiction(GetContradictionQuery),
    ListContradictions(ListContradictionsQuery),
    GetVerification(GetVerificationQuery),
    ListVerifications(ListVerificationsQuery),
    GetOperation(GetOperationQuery),
    ListOperations(ListOperationsQuery),
    ListEvents(ListEventsQuery),
    GetValidationReport(GetValidationReportQuery),
    // Stage 1 variants
    GetCampaign(GetCampaignQuery),
    ListWorkItems(ListWorkItemsQuery),
    GetWorkItem(GetWorkItemQuery),
    ListWorkItemDependencies(ListWorkItemDependenciesQuery),
    ListWorkItemBlockers(ListWorkItemBlockersQuery),
    ListExpiredLeases(ListExpiredLeasesQuery),
    GetProviderInstallation(GetProviderInstallationQuery),
    ListProviderInstallations(ListProviderInstallationsQuery),
    ListProviderInstances(ListProviderInstancesQuery),
    GetProviderInstance(GetProviderInstanceQuery),
    GetBuildStatus(GetBuildStatusQuery),
    ListBuildDiagnostics(ListBuildDiagnosticsQuery),
    GetVerificationCoverage(GetVerificationCoverageQuery),
    ListGeneratedSourceMappings(ListGeneratedSourceMappingsQuery),
    ListConflicts(ListConflictsQuery),
    ListBlockedReasons(ListBlockedReasonsQuery),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum QueryResult {
    ProjectSummary(ProjectSummaryResponse),
    Artifact(ArtifactResponse),
    Artifacts(ArtifactsResponse),
    Entity(EntityResponse),
    Entities(EntitiesResponse),
    Provider(ProviderResponse),
    Providers(ProvidersResponse),
    ProviderRun(ProviderRunResponse),
    ProviderRuns(ProviderRunsResponse),
    Evidence(EvidenceResponse),
    EvidenceList(EvidenceListResponse),
    Hypothesis(HypothesisResponse),
    Hypotheses(HypothesesResponse),
    Contradiction(ContradictionResponse),
    Contradictions(ContradictionsResponse),
    Verification(VerificationResponse),
    Verifications(VerificationsResponse),
    Operation(OperationResponse),
    Operations(OperationsResponse),
    Events(EventsResponse),
    ValidationReport(ValidationReportResponse),
    // Stage 1 variants
    Campaign(CampaignResponse),
    WorkItems(WorkItemsResponse),
    WorkItem(WorkItemResponse),
    WorkItemDependencies(WorkItemDependenciesResponse),
    WorkItemBlockers(WorkItemBlockersResponse),
    ExpiredLeases(ExpiredLeasesResponse),
    ProviderInstallation(ProviderInstallationResponse),
    ProviderInstallations(ProviderInstallationsResponse),
    ProviderInstances(ProviderInstancesResponse),
    ProviderInstance(ProviderInstanceResponse),
    BuildStatus(BuildStatusResponse),
    BuildDiagnostics(BuildDiagnosticsResponse),
    VerificationCoverage(VerificationCoverageResponse),
    GeneratedSourceMappings(GeneratedSourceMappingsResponse),
    Conflicts(ConflictsResponse),
    BlockedReasons(BlockedReasonsResponse),
}

// ---------------------------------------------------------------------------
// Client trait (Task 25)
// ---------------------------------------------------------------------------

use autore_events::project_event_service::ProjectEventSubscription;
use autore_schema::domain::records::ProjectEvent;

pub trait AutoReClient: Send + Sync {
    fn execute(&self, command: ApplicationCommand) -> autore_core::Result<CommandResult>;
    fn query(&self, query: ApplicationQuery) -> autore_core::Result<QueryResult>;
    fn events_after(
        &self,
        project: ProjectId,
        sequence: u64,
        limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>>;
    fn subscribe_events(
        &self,
        project: ProjectId,
        after: u64,
    ) -> autore_core::Result<ProjectEventSubscription>;
}

/// In-process [`AutoReClient`] that delegates directly to an
/// [`ApplicationService`]. No network transport (spec §21).
pub struct LocalAutoReClient {
    application: std::sync::Arc<super::ApplicationService>,
}

impl LocalAutoReClient {
    pub fn new(application: std::sync::Arc<super::ApplicationService>) -> Self {
        Self { application }
    }
}

impl AutoReClient for LocalAutoReClient {
    fn execute(&self, command: ApplicationCommand) -> autore_core::Result<CommandResult> {
        self.application.execute(command)
    }

    fn query(&self, query: ApplicationQuery) -> autore_core::Result<QueryResult> {
        self.application.query(query)
    }

    fn events_after(
        &self,
        project: ProjectId,
        sequence: u64,
        limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>> {
        self.application
            .events
            .events_after(project, sequence, limit)
    }

    fn subscribe_events(
        &self,
        project: ProjectId,
        after: u64,
    ) -> autore_core::Result<ProjectEventSubscription> {
        self.application.events.subscribe(project, after)
    }
}
