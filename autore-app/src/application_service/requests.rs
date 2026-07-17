use std::path::PathBuf;

use autore_schema::domain::records::{
    Artifact, Contradiction, EvidenceRecord, Hypothesis, Operation, Project, Provider, ProviderRun,
    SemanticEntity, VerificationRecord,
};
use autore_schema::domain::{ContentHash, EnvironmentIdentity, EvidenceValue, NamespacedId, StableEntityKey};
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
    pub operation: Operation,
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
        self.application.events.events_after(project, sequence, limit)
    }

    fn subscribe_events(
        &self,
        project: ProjectId,
        after: u64,
    ) -> autore_core::Result<ProjectEventSubscription> {
        self.application.events.subscribe(project, after)
    }
}
