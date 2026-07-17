pub mod requests;
pub mod stores;

mod mutations;
#[cfg(test)]
mod tests;
mod validation;

use std::path::PathBuf;
use std::sync::Arc;

pub use requests::*;
use stores::*;

use autore_core::operation::OperationState;
use autore_core::{Error, Result};
use autore_events::project_event_service::ProjectEventService;
use autore_schema::domain::records::{
    CancellationRequest, EventSource, EventSubject, Hypothesis, HypothesisStatus, Operation,
    Project, ProviderRun, SemanticEntity, EVENT_KIND_ARTIFACT_REGISTERED,
    EVENT_KIND_CONTRADICTION_CREATED, EVENT_KIND_EVIDENCE_ADDED, EVENT_KIND_ENTITY_CREATED,
    EVENT_KIND_HYPOTHESIS_ACCEPTED, EVENT_KIND_HYPOTHESIS_PROPOSED, EVENT_KIND_HYPOTHESIS_REJECTED,
    EVENT_KIND_OPERATION_CANCELLING, EVENT_KIND_OPERATION_QUEUED,
    EVENT_KIND_PROJECT_CREATED, EVENT_KIND_VERIFICATION_RECORDED,
    OPERATION_KIND_PROJECT_MIGRATION, OPERATION_KIND_PROJECT_REBUILD_INDEXES,
    OPERATION_KIND_PROJECT_VALIDATION,
};
use autore_schema::domain::{Confidence, NamespacedId, Timestamp};
use autore_schema::ids::{ArtifactId, HypothesisId, ProjectId, ProviderRunId};
use autore_store::{
    ArtifactStore, ContradictionStore, Database, EntityColumn, EntityPage, EntityStore,
    EvidenceStore, HypothesisStore, OperationStore, ProjectStore, ProviderStore, RunQuery,
    VerificationStore, with_event,
};

use crate::application_service::mutations as muts;
use crate::application_service::validation::{ensure_same_project, parse_namespaced_id, validate_confidence, validate_not_empty};

/// The shared application layer for all auto-re frontends (CLI, TUI, etc.).
///
/// All state mutations run through [`execute`](ApplicationService::execute), which validates
/// requests and routes them to the correct store. Every mutating command emits a
/// [`ProjectEvent`](autore_schema::domain::records::ProjectEvent) in the same SQLite transaction
/// as the state mutation via the [`with_event`](autore_store::with_event) helper.
///
/// Queries are read-only and are routed through the same store layer.
pub struct ApplicationService {
    pub(crate) db: Arc<Database>,
    pub(crate) events: Arc<dyn ProjectEventService + Send + Sync>,
    pub(crate) base_dir: PathBuf,
    pub(crate) project_store: Arc<dyn ProjectStore + Send + Sync>,
    pub(crate) artifact_store: Arc<dyn ArtifactStore + Send + Sync>,
    pub(crate) entity_store: Arc<dyn EntityStore + Send + Sync>,
    pub(crate) provider_store: Arc<dyn ProviderStore + Send + Sync>,
    pub(crate) evidence_store: Arc<dyn EvidenceStore + Send + Sync>,
    pub(crate) hypothesis_store: Arc<dyn HypothesisStore + Send + Sync>,
    pub(crate) contradiction_store: Arc<dyn ContradictionStore + Send + Sync>,
    pub(crate) verification_store: Arc<dyn VerificationStore + Send + Sync>,
    pub(crate) operation_store: Arc<dyn OperationStore + Send + Sync>,
}

impl ApplicationService {
    /// Creates a new application service backed by the given database and event service.
    ///
    /// `base_dir` is used for managed artifact storage.
    pub fn new(
        db: Arc<Database>,
        events: Arc<dyn ProjectEventService + Send + Sync>,
        base_dir: impl Into<PathBuf>,
    ) -> Self {
        let base_dir = base_dir.into();
        Self {
            db: Arc::clone(&db),
            events,
            project_store: Arc::new(ProjectStoreImpl::new(Arc::clone(&db))),
            artifact_store: Arc::new(ArtifactStoreImpl::new(Arc::clone(&db), base_dir.clone())),
            entity_store: Arc::new(EntityStoreImpl::new(Arc::clone(&db))),
            provider_store: Arc::new(ProviderStoreImpl::new(Arc::clone(&db))),
            evidence_store: Arc::new(EvidenceStoreImpl::new(Arc::clone(&db))),
            hypothesis_store: Arc::new(HypothesisStoreImpl::new(Arc::clone(&db))),
            contradiction_store: Arc::new(ContradictionStoreImpl::new(Arc::clone(&db))),
            verification_store: Arc::new(VerificationStoreImpl::new(Arc::clone(&db))),
            operation_store: Arc::new(OperationStoreImpl::new(Arc::clone(&db))),
            base_dir,
        }
    }

    /// Executes a mutating command and returns the typed result.
    ///
    /// Validation runs before any store call. Mutations are wrapped in a single SQLite transaction
    /// together with the emitted event.
    pub fn execute(&self, command: ApplicationCommand) -> Result<CommandResult> {
        match command {
            ApplicationCommand::CreateProject(req) => self.create_project(req),
            ApplicationCommand::RegisterArtifact(req) => self.register_artifact(req),
            ApplicationCommand::RegisterEntity(req) => self.register_entity(req),
            ApplicationCommand::RegisterProvider(req) => self.register_provider(req),
            ApplicationCommand::StartProviderRun(req) => self.start_provider_run(req),
            ApplicationCommand::AddEvidence(req) => self.add_evidence(req),
            ApplicationCommand::AddHypothesis(req) => self.add_hypothesis(req),
            ApplicationCommand::ChangeHypothesisStatus(req) => self.change_hypothesis_status(req),
            ApplicationCommand::RecordContradiction(req) => self.record_contradiction(req),
            ApplicationCommand::AddVerification(req) => self.add_verification(req),
            ApplicationCommand::CancelOperation(req) => self.cancel_operation(req),
            ApplicationCommand::ValidateProject(req) => self.validate_project(req),
            ApplicationCommand::MigrateProject(req) => self.migrate_project(req),
            ApplicationCommand::RebuildIndexes(req) => self.rebuild_indexes(req),
        }
    }

    /// Executes a read-only query and returns the typed result.
    pub fn query(&self, query: ApplicationQuery) -> Result<QueryResult> {
        match query {
            ApplicationQuery::GetProjectSummary(q) => self.get_project_summary(q),
            ApplicationQuery::GetArtifact(q) => self.get_artifact(q),
            ApplicationQuery::ListArtifacts(q) => self.list_artifacts(q),
            ApplicationQuery::GetEntity(q) => self.get_entity(q),
            ApplicationQuery::ListEntities(q) => self.list_entities(q),
            ApplicationQuery::GetProvider(q) => self.get_provider(q),
            ApplicationQuery::ListProviders(q) => self.list_providers(q),
            ApplicationQuery::GetProviderRun(q) => self.get_provider_run(q),
            ApplicationQuery::ListProviderRuns(q) => self.list_provider_runs(q),
            ApplicationQuery::GetEvidence(q) => self.get_evidence(q),
            ApplicationQuery::ListEvidence(q) => self.list_evidence(q),
            ApplicationQuery::GetHypothesis(q) => self.get_hypothesis(q),
            ApplicationQuery::ListHypotheses(q) => self.list_hypotheses(q),
            ApplicationQuery::GetContradiction(q) => self.get_contradiction(q),
            ApplicationQuery::ListContradictions(q) => self.list_contradictions(q),
            ApplicationQuery::GetVerification(q) => self.get_verification(q),
            ApplicationQuery::ListVerifications(q) => self.list_verifications(q),
            ApplicationQuery::GetOperation(q) => self.get_operation(q),
            ApplicationQuery::ListOperations(q) => self.list_operations(q),
            ApplicationQuery::ListEvents(q) => self.list_events(q),
        }
    }

    fn create_project(&self, req: CreateProjectRequest) -> Result<CommandResult> {
        validate_not_empty(&req.name, "project name")?;
        let project = Project::new(req.name);
        let project_id = project.id;
        with_event(
            &self.db,
            project_id,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            Some(EventSubject::Project(project_id)),
            None,
            |txn| muts::insert_project(txn, &project),
        )?;
        Ok(CommandResult::ProjectCreated(CreateProjectResponse { project }))
    }

    fn register_artifact(&self, req: RegisterArtifactRequest) -> Result<CommandResult> {
        let kind = parse_namespaced_id(&req.kind)?;
        let project = self.project_store.get_project(req.project)?;
        if project.is_none() {
            return Err(Error::NotFound(format!("project {} not found", req.project)));
        }
        let project_dir = self.base_dir.join(req.project.to_string());
        let artifact_id = ArtifactId::new();
        let artifact = with_event(
            &self.db,
            req.project,
            EVENT_KIND_ARTIFACT_REGISTERED.clone(),
            EventSource::Artifact,
            Some(EventSubject::Artifact(artifact_id)),
            None,
            |txn| {
                muts::insert_artifact_managed(
                    txn,
                    &project_dir,
                    req.project,
                    &req.source_path,
                    kind,
                    artifact_id,
                )
            },
        )?;
        Ok(CommandResult::ArtifactRegistered(RegisterArtifactResponse {
            artifact,
        }))
    }

    fn register_entity(&self, req: RegisterEntityRequest) -> Result<CommandResult> {
        let kind = parse_namespaced_id(&req.kind)?;
        let _project = self.require_project(req.project)?;
        let entity = SemanticEntity::new(req.project, kind, req.stable_key, req.display_name);
        let entity_id = entity.id;
        with_event(
            &self.db,
            req.project,
            EVENT_KIND_ENTITY_CREATED.clone(),
            EventSource::Entity,
            Some(EventSubject::Entity(entity_id)),
            None,
            |txn| muts::insert_entity(txn, &entity),
        )?;
        Ok(CommandResult::EntityRegistered(RegisterEntityResponse { entity }))
    }

    fn register_provider(&self, req: RegisterProviderRequest) -> Result<CommandResult> {
        let _project = self.require_project(req.project)?;
        self.provider_store.insert_provider(&req.provider)?;
        Ok(CommandResult::ProviderRegistered(RegisterProviderResponse {
            provider: req.provider,
        }))
    }

    fn start_provider_run(&self, req: StartProviderRunRequest) -> Result<CommandResult> {
        let _project = self.require_project(req.project)?;
        let operation = parse_namespaced_id(&req.operation)?;
        let _provider = self
            .provider_store
            .get_provider(req.provider)?
            .ok_or_else(|| Error::NotFound(format!("provider {} not found", req.provider)))?;
        let run = ProviderRun {
            id: ProviderRunId::new(),
            project: req.project,
            provider: req.provider,
            operation,
            input_artifacts: req.input_artifacts,
            configuration_artifact: req.configuration_artifact,
            configuration_hash: req.configuration_hash,
            environment: req.environment,
            started_at: Timestamp::now(),
            completed_at: None,
            status: autore_schema::domain::records::ProviderRunStatus::Running,
        };
        self.provider_store.start_run(&run)?;
        Ok(CommandResult::ProviderRunStarted(StartProviderRunResponse { run }))
    }

    fn add_evidence(&self, req: AddEvidenceRequest) -> Result<CommandResult> {
        ensure_same_project("evidence record", req.project, req.record.project)?;
        let _project = self.require_project(req.project)?;
        let record = req.record;
        let record_id = record.id;
        with_event(
            &self.db,
            req.project,
            EVENT_KIND_EVIDENCE_ADDED.clone(),
            EventSource::Evidence,
            Some(EventSubject::Evidence(record_id)),
            None,
            |txn| muts::insert_evidence(txn, &record),
        )?;
        Ok(CommandResult::EvidenceAdded(AddEvidenceResponse { id: record_id }))
    }

    fn add_hypothesis(&self, req: AddHypothesisRequest) -> Result<CommandResult> {
        let predicate = parse_namespaced_id(&req.predicate)?;
        validate_confidence(req.confidence_score)?;
        let _project = self.require_project(req.project)?;
        let confidence = match req.confidence_rationale {
            Some(rationale) => Confidence::with_rationale(req.confidence_score as f32, rationale),
            None => Confidence::new(req.confidence_score as f32),
        }?;
        let hypothesis = Hypothesis {
            id: HypothesisId::new(),
            project: req.project,
            subject: req.subject,
            predicate,
            candidate: req.candidate,
            supporting_evidence: req.supporting_evidence,
            contradicting_evidence: req.contradicting_evidence,
            derived_from: req.derived_from,
            confidence,
            status: req.status,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        let hypothesis_id = hypothesis.id;
        with_event(
            &self.db,
            req.project,
            EVENT_KIND_HYPOTHESIS_PROPOSED.clone(),
            EventSource::Hypothesis,
            Some(EventSubject::Hypothesis(hypothesis_id)),
            None,
            |txn| muts::insert_hypothesis(txn, &hypothesis),
        )?;
        Ok(CommandResult::HypothesisAdded(AddHypothesisResponse {
            id: hypothesis_id,
        }))
    }

    fn change_hypothesis_status(
        &self,
        req: ChangeHypothesisStatusRequest,
    ) -> Result<CommandResult> {
        let hypothesis = self
            .hypothesis_store
            .get(req.id)?
            .ok_or_else(|| Error::NotFound(format!("hypothesis {} not found", req.id)))?;
        ensure_same_project("hypothesis", req.project, hypothesis.project)?;
        let kind = match req.status {
            HypothesisStatus::Accepted => EVENT_KIND_HYPOTHESIS_ACCEPTED.clone(),
            HypothesisStatus::Rejected => EVENT_KIND_HYPOTHESIS_REJECTED.clone(),
            _ => EVENT_KIND_HYPOTHESIS_PROPOSED.clone(),
        };
        let target_status = req.status.clone();
        with_event(
            &self.db,
            req.project,
            kind,
            EventSource::Hypothesis,
            Some(EventSubject::Hypothesis(req.id)),
            None,
            |txn| muts::update_hypothesis_status(txn, req.id, target_status),
        )?;
        let updated = self
            .hypothesis_store
            .get(req.id)?
            .ok_or_else(|| Error::NotFound(format!("hypothesis {} not found", req.id)))?;
        Ok(CommandResult::HypothesisStatusChanged(
            ChangeHypothesisStatusResponse { hypothesis: updated },
        ))
    }

    fn record_contradiction(&self, req: RecordContradictionRequest) -> Result<CommandResult> {
        ensure_same_project("contradiction", req.project, req.contradiction.project)?;
        let _project = self.require_project(req.project)?;
        let contradiction = req.contradiction;
        let contradiction_id = contradiction.id;
        with_event(
            &self.db,
            req.project,
            EVENT_KIND_CONTRADICTION_CREATED.clone(),
            EventSource::Contradiction,
            Some(EventSubject::Contradiction(contradiction_id)),
            None,
            |txn| muts::insert_contradiction(txn, &contradiction),
        )?;
        Ok(CommandResult::ContradictionRecorded(
            RecordContradictionResponse { id: contradiction_id },
        ))
    }

    fn add_verification(&self, req: AddVerificationRequest) -> Result<CommandResult> {
        ensure_same_project("verification record", req.project, req.record.project)?;
        let _project = self.require_project(req.project)?;
        let record = req.record;
        let record_id = record.id;
        with_event(
            &self.db,
            req.project,
            EVENT_KIND_VERIFICATION_RECORDED.clone(),
            EventSource::Verification,
            Some(EventSubject::Verification(record_id)),
            None,
            |txn| muts::insert_verification(txn, &record),
        )?;
        Ok(CommandResult::VerificationAdded(AddVerificationResponse {
            id: record_id,
        }))
    }

    fn cancel_operation(&self, req: CancelOperationRequest) -> Result<CommandResult> {
        let operation = self
            .operation_store
            .get(req.id)?
            .ok_or_else(|| Error::NotFound(format!("operation {} not found", req.id)))?;
        ensure_same_project("operation", req.project, operation.project)?;
        let request = CancellationRequest::new(req.id, req.requested_by, req.reason);
        let op_id = operation.id;
        with_event(
            &self.db,
            req.project,
            EVENT_KIND_OPERATION_CANCELLING.clone(),
            EventSource::Operation,
            Some(EventSubject::Operation(op_id)),
            None,
            |txn| {
                muts::insert_cancellation_request(txn, &request)?;
                muts::transition_operation(txn, op_id, OperationState::Cancelling, None)?;
                Ok(())
            },
        )?;
        let updated = self
            .operation_store
            .get(req.id)?
            .ok_or_else(|| Error::NotFound(format!("operation {} not found", req.id)))?;
        Ok(CommandResult::OperationCancelled(CancelOperationResponse {
            operation: updated,
        }))
    }

    fn validate_project(&self, req: ValidateProjectRequest) -> Result<CommandResult> {
        let operation = self.queue_operation(
            req.project,
            OPERATION_KIND_PROJECT_VALIDATION.clone(),
            "validate-project",
        )?;
        Ok(CommandResult::ProjectValidated(ValidateProjectResponse {
            operation,
        }))
    }

    fn migrate_project(&self, req: MigrateProjectRequest) -> Result<CommandResult> {
        let operation = self.queue_operation(
            req.project,
            OPERATION_KIND_PROJECT_MIGRATION.clone(),
            "migrate-project",
        )?;
        Ok(CommandResult::ProjectMigrated(MigrateProjectResponse {
            operation,
        }))
    }

    fn rebuild_indexes(&self, req: RebuildIndexesRequest) -> Result<CommandResult> {
        let operation = self.queue_operation(
            req.project,
            OPERATION_KIND_PROJECT_REBUILD_INDEXES.clone(),
            "rebuild-indexes",
        )?;
        Ok(CommandResult::IndexesRebuilt(RebuildIndexesResponse {
            operation,
        }))
    }

    fn queue_operation(
        &self,
        project: ProjectId,
        kind: NamespacedId,
        requested_by: &str,
    ) -> Result<Operation> {
        let _project = self.require_project(project)?;
        let operation = Operation::new(project, kind, requested_by);
        let op_id = operation.id;
        with_event(
            &self.db,
            project,
            EVENT_KIND_OPERATION_QUEUED.clone(),
            EventSource::Operation,
            Some(EventSubject::Operation(op_id)),
            None,
            |txn| muts::insert_operation(txn, &operation),
        )?;
        Ok(operation)
    }

    fn require_project(&self, project: ProjectId) -> Result<Project> {
        self.project_store
            .get_project(project)?
            .ok_or_else(|| Error::NotFound(format!("project {} not found", project)))
    }

    fn get_project_summary(&self, q: GetProjectSummaryQuery) -> Result<QueryResult> {
        let project = self.require_project(q.project)?;
        Ok(QueryResult::ProjectSummary(ProjectSummaryResponse {
            project,
        }))
    }

    fn get_artifact(&self, q: GetArtifactQuery) -> Result<QueryResult> {
        let artifact = self
            .artifact_store
            .get_artifact(q.id)?
            .ok_or_else(|| Error::NotFound(format!("artifact {} not found", q.id)))?;
        Ok(QueryResult::Artifact(ArtifactResponse { artifact }))
    }

    fn list_artifacts(&self, q: ListArtifactsQuery) -> Result<QueryResult> {
        let artifacts = self.artifact_store.list_by_project(q.project)?;
        Ok(QueryResult::Artifacts(ArtifactsResponse { artifacts }))
    }

    fn get_entity(&self, q: GetEntityQuery) -> Result<QueryResult> {
        let entity = self
            .entity_store
            .get(q.id)?
            .ok_or_else(|| Error::NotFound(format!("entity {} not found", q.id)))?;
        Ok(QueryResult::Entity(EntityResponse { entity }))
    }

    fn list_entities(&self, q: ListEntitiesQuery) -> Result<QueryResult> {
        let page = EntityPage {
            offset: q.offset,
            limit: q.limit,
            order_by: EntityColumn::CreatedAt,
        };
        let entities = self
            .entity_store
            .list_by_project(q.project, page, q.kind_filter.as_ref())?;
        Ok(QueryResult::Entities(EntitiesResponse { entities }))
    }

    fn get_provider(&self, q: GetProviderQuery) -> Result<QueryResult> {
        let provider = self
            .provider_store
            .get_provider(q.id)?
            .ok_or_else(|| Error::NotFound(format!("provider {} not found", q.id)))?;
        Ok(QueryResult::Provider(ProviderResponse { provider }))
    }

    fn list_providers(&self, _q: ListProvidersQuery) -> Result<QueryResult> {
        let providers = self.provider_store.list_providers()?;
        Ok(QueryResult::Providers(ProvidersResponse { providers }))
    }

    fn get_provider_run(&self, q: GetProviderRunQuery) -> Result<QueryResult> {
        let run = self
            .provider_store
            .get_run(q.id)?
            .ok_or_else(|| Error::NotFound(format!("provider run {} not found", q.id)))?;
        Ok(QueryResult::ProviderRun(ProviderRunResponse { run }))
    }

    fn list_provider_runs(&self, q: ListProviderRunsQuery) -> Result<QueryResult> {
        let query = RunQuery {
            project_id: q.project,
            status_filter: None,
            provider_filter: None,
            offset: q.offset,
            limit: q.limit,
        };
        let runs = self.provider_store.list_runs(query)?;
        Ok(QueryResult::ProviderRuns(ProviderRunsResponse { runs }))
    }

    fn get_evidence(&self, q: GetEvidenceQuery) -> Result<QueryResult> {
        let record = self
            .evidence_store
            .get_evidence(q.id)?
            .ok_or_else(|| Error::NotFound(format!("evidence {} not found", q.id)))?;
        Ok(QueryResult::Evidence(EvidenceResponse { record }))
    }

    fn list_evidence(&self, q: ListEvidenceQuery) -> Result<QueryResult> {
        let records = self.evidence_store.list_by_project(q.project)?;
        Ok(QueryResult::EvidenceList(EvidenceListResponse { records }))
    }

    fn get_hypothesis(&self, q: GetHypothesisQuery) -> Result<QueryResult> {
        let hypothesis = self
            .hypothesis_store
            .get(q.id)?
            .ok_or_else(|| Error::NotFound(format!("hypothesis {} not found", q.id)))?;
        Ok(QueryResult::Hypothesis(HypothesisResponse { hypothesis }))
    }

    fn list_hypotheses(&self, q: ListHypothesesQuery) -> Result<QueryResult> {
        let hypotheses = self.hypothesis_store.list_by_project(q.project)?;
        Ok(QueryResult::Hypotheses(HypothesesResponse { hypotheses }))
    }

    fn get_contradiction(&self, q: GetContradictionQuery) -> Result<QueryResult> {
        let contradiction = self
            .contradiction_store
            .get(q.id)?
            .ok_or_else(|| Error::NotFound(format!("contradiction {} not found", q.id)))?;
        Ok(QueryResult::Contradiction(ContradictionResponse {
            contradiction,
        }))
    }

    fn list_contradictions(&self, q: ListContradictionsQuery) -> Result<QueryResult> {
        let contradictions = self.contradiction_store.list_by_project(q.project)?;
        Ok(QueryResult::Contradictions(ContradictionsResponse {
            contradictions,
        }))
    }

    fn get_verification(&self, q: GetVerificationQuery) -> Result<QueryResult> {
        let record = self
            .verification_store
            .get(q.id)?
            .ok_or_else(|| Error::NotFound(format!("verification {} not found", q.id)))?;
        Ok(QueryResult::Verification(VerificationResponse { record }))
    }

    fn list_verifications(&self, q: ListVerificationsQuery) -> Result<QueryResult> {
        let records = self.verification_store.list_by_project(q.project)?;
        Ok(QueryResult::Verifications(VerificationsResponse { records }))
    }

    fn get_operation(&self, q: GetOperationQuery) -> Result<QueryResult> {
        let operation = self
            .operation_store
            .get(q.id)?
            .ok_or_else(|| Error::NotFound(format!("operation {} not found", q.id)))?;
        Ok(QueryResult::Operation(OperationResponse { operation }))
    }

    fn list_operations(&self, q: ListOperationsQuery) -> Result<QueryResult> {
        let operations = self.operation_store.list_by_project(q.project)?;
        Ok(QueryResult::Operations(OperationsResponse { operations }))
    }

    fn list_events(&self, q: ListEventsQuery) -> Result<QueryResult> {
        let events = self
            .events
            .events_after(q.project, q.after_sequence, q.limit)?;
        Ok(QueryResult::Events(EventsResponse { events }))
    }
}


