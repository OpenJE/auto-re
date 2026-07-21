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
    CancellationRequest, EVENT_KIND_ARTIFACT_REGISTERED, EVENT_KIND_CONTRADICTION_CREATED,
    EVENT_KIND_ENTITY_CREATED, EVENT_KIND_EVIDENCE_ADDED, EVENT_KIND_HYPOTHESIS_ACCEPTED,
    EVENT_KIND_HYPOTHESIS_PROPOSED, EVENT_KIND_HYPOTHESIS_REJECTED,
    EVENT_KIND_OPERATION_CANCELLING, EVENT_KIND_OPERATION_QUEUED, EVENT_KIND_PROJECT_CREATED,
    EVENT_KIND_PROJECT_INDEXES_REBUILT, EVENT_KIND_PROJECT_VALIDATION_FAILED,
    EVENT_KIND_VERIFICATION_RECORDED, EventSource, EventSubject, Hypothesis, HypothesisStatus,
    OPERATION_KIND_PROJECT_MIGRATION, OPERATION_KIND_PROJECT_REBUILD_INDEXES, Operation, Project,
    ProviderRun, SemanticEntity,
};
use autore_schema::domain::{Confidence, ExtensionData, NamespacedId, Timestamp};
use autore_schema::ids::{ArtifactId, HypothesisId, ProjectId, ProviderRunId};
use autore_store::{
    ArtifactStore, ContradictionStore, Database, EntityColumn, EntityPage, EntityStore,
    EvidenceStore, HypothesisStore, NativeArtifactStore, OperationStore, ProjectStore,
    ProviderAliasStore, ProviderStore, RunQuery, VerificationStore, build_derived_state_in_tx,
    with_event,
};

use crate::application_service::mutations as muts;
use crate::application_service::validation::{
    ValidationService, ensure_same_project, parse_namespaced_id, validate_confidence,
    validate_not_empty,
};

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
    pub(crate) validation_service: ValidationService,
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
        let project_store: Arc<dyn ProjectStore + Send + Sync> =
            Arc::new(ProjectStoreImpl::new(Arc::clone(&db)));
        let artifact_store: Arc<dyn ArtifactStore + Send + Sync> =
            Arc::new(ArtifactStoreImpl::new(Arc::clone(&db), base_dir.clone()));
        let entity_store: Arc<dyn EntityStore + Send + Sync> =
            Arc::new(EntityStoreImpl::new(Arc::clone(&db)));
        let provider_store: Arc<dyn ProviderStore + Send + Sync> =
            Arc::new(ProviderStoreImpl::new(Arc::clone(&db)));
        let evidence_store: Arc<dyn EvidenceStore + Send + Sync> =
            Arc::new(EvidenceStoreImpl::new(Arc::clone(&db)));
        let hypothesis_store: Arc<dyn HypothesisStore + Send + Sync> =
            Arc::new(HypothesisStoreImpl::new(Arc::clone(&db)));
        let contradiction_store: Arc<dyn ContradictionStore + Send + Sync> =
            Arc::new(ContradictionStoreImpl::new(Arc::clone(&db)));
        let verification_store: Arc<dyn VerificationStore + Send + Sync> =
            Arc::new(VerificationStoreImpl::new(Arc::clone(&db)));
        let operation_store: Arc<dyn OperationStore + Send + Sync> =
            Arc::new(OperationStoreImpl::new(Arc::clone(&db)));
        let native_artifact_store: Arc<dyn NativeArtifactStore + Send + Sync> =
            Arc::new(NativeArtifactStoreImpl::new(Arc::clone(&db)));
        let alias_store: Arc<dyn ProviderAliasStore + Send + Sync> =
            Arc::new(ProviderAliasStoreImpl::new(Arc::clone(&db)));
        let validation_service = ValidationService {
            db: Arc::clone(&db),
            project_store: Arc::clone(&project_store),
            artifact_store: Arc::clone(&artifact_store),
            entity_store: Arc::clone(&entity_store),
            provider_store: Arc::clone(&provider_store),
            evidence_store: Arc::clone(&evidence_store),
            hypothesis_store: Arc::clone(&hypothesis_store),
            contradiction_store: Arc::clone(&contradiction_store),
            verification_store: Arc::clone(&verification_store),
            operation_store: Arc::clone(&operation_store),
            native_artifact_store: Arc::clone(&native_artifact_store),
            alias_store: Arc::clone(&alias_store),
            events: Arc::clone(&events),
        };
        Self {
            db: Arc::clone(&db),
            events,
            project_store,
            artifact_store,
            entity_store,
            provider_store,
            evidence_store,
            hypothesis_store,
            contradiction_store,
            verification_store,
            operation_store,
            validation_service,
            base_dir,
        }
    }

    fn validation_report_payload(
        report: &crate::application_service::requests::ValidationReport,
    ) -> Result<ExtensionData> {
        let value = serde_json::to_value(report)
            .map_err(|e| Error::Serialization(format!("validation report: {e}")))?;
        Ok(ExtensionData::new(
            NamespacedId::parse("core.project.validation-report").map_err(|e| {
                Error::Validation(format!("invalid validation report schema id: {e}"))
            })?,
            1,
            value,
        ))
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
            // Stage 1 stubs
            ApplicationCommand::CreateReconstructionCampaign(_) => Err(Error::Validation(
                "not yet implemented: CreateReconstructionCampaign".into(),
            )),
            ApplicationCommand::CreateWorkItems(_) => Err(Error::Validation(
                "not yet implemented: CreateWorkItems".into(),
            )),
            ApplicationCommand::RecordWorkDependency(_) => Err(Error::Validation(
                "not yet implemented: RecordWorkDependency".into(),
            )),
            ApplicationCommand::PromoteWorkItem(_) => Err(Error::Validation(
                "not yet implemented: PromoteWorkItem".into(),
            )),
            ApplicationCommand::LeaseWorkItem(_) => Err(Error::Validation(
                "not yet implemented: LeaseWorkItem".into(),
            )),
            ApplicationCommand::RenewWorkLease(_) => Err(Error::Validation(
                "not yet implemented: RenewWorkLease".into(),
            )),
            ApplicationCommand::CompleteWorkItem(_) => Err(Error::Validation(
                "not yet implemented: CompleteWorkItem".into(),
            )),
            ApplicationCommand::FailWorkItem(_) => Err(Error::Validation(
                "not yet implemented: FailWorkItem".into(),
            )),
            ApplicationCommand::BlockWorkItem(_) => Err(Error::Validation(
                "not yet implemented: BlockWorkItem".into(),
            )),
            ApplicationCommand::InvalidateWorkItem(_) => Err(Error::Validation(
                "not yet implemented: InvalidateWorkItem".into(),
            )),
            ApplicationCommand::RequeueWorkItem(_) => Err(Error::Validation(
                "not yet implemented: RequeueWorkItem".into(),
            )),
            ApplicationCommand::BlockWorkWithReason(_) => Err(Error::Validation(
                "not yet implemented: BlockWorkWithReason".into(),
            )),
            ApplicationCommand::RegisterProviderInstallation(_) => Err(Error::Validation(
                "not yet implemented: RegisterProviderInstallation".into(),
            )),
            ApplicationCommand::RegisterProviderInstance(_) => Err(Error::Validation(
                "not yet implemented: RegisterProviderInstance".into(),
            )),
            ApplicationCommand::StopProviderInstance(_) => Err(Error::Validation(
                "not yet implemented: StopProviderInstance".into(),
            )),
            ApplicationCommand::ImportProviderRunResult(_) => Err(Error::Validation(
                "not yet implemented: ImportProviderRunResult".into(),
            )),
            ApplicationCommand::ImportDynamicObservation(_) => Err(Error::Validation(
                "not yet implemented: ImportDynamicObservation".into(),
            )),
            ApplicationCommand::RecordBuildAttempt(_) => Err(Error::Validation(
                "not yet implemented: RecordBuildAttempt".into(),
            )),
            ApplicationCommand::RunBuild(_) => {
                Err(Error::Validation("not yet implemented: RunBuild".into()))
            }
            ApplicationCommand::RecordVerificationComparison(_) => Err(Error::Validation(
                "not yet implemented: RecordVerificationComparison".into(),
            )),
            ApplicationCommand::RegisterGeneratedSourceMapping(_) => Err(Error::Validation(
                "not yet implemented: RegisterGeneratedSourceMapping".into(),
            )),
            ApplicationCommand::InvalidateGeneratedSource(_) => Err(Error::Validation(
                "not yet implemented: InvalidateGeneratedSource".into(),
            )),
            ApplicationCommand::ImportGeneratedSourceCandidates(_) => Err(Error::Validation(
                "not yet implemented: ImportGeneratedSourceCandidates".into(),
            )),
            ApplicationCommand::ScheduleVerificationRegression(_) => Err(Error::Validation(
                "not yet implemented: ScheduleVerificationRegression".into(),
            )),
            ApplicationCommand::RecordRepairAttempt(_) => Err(Error::Validation(
                "not yet implemented: RecordRepairAttempt".into(),
            )),
            ApplicationCommand::AcceptHypothesisPolicyDriven(_) => Err(Error::Validation(
                "not yet implemented: AcceptHypothesisPolicyDriven".into(),
            )),
            ApplicationCommand::PauseCoordinator(_) => Err(Error::Validation(
                "not yet implemented: PauseCoordinator".into(),
            )),
            ApplicationCommand::ResumeCoordinator(_) => Err(Error::Validation(
                "not yet implemented: ResumeCoordinator".into(),
            )),
            ApplicationCommand::StopCoordinator(_) => Err(Error::Validation(
                "not yet implemented: StopCoordinator".into(),
            )),
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
            ApplicationQuery::GetValidationReport(q) => self.get_validation_report(q),
            // Stage 1 stubs
            ApplicationQuery::GetCampaign(_) => {
                Err(Error::Validation("not yet implemented: GetCampaign".into()))
            }
            ApplicationQuery::ListWorkItems(_) => Err(Error::Validation(
                "not yet implemented: ListWorkItems".into(),
            )),
            ApplicationQuery::GetWorkItem(_) => {
                Err(Error::Validation("not yet implemented: GetWorkItem".into()))
            }
            ApplicationQuery::ListWorkItemDependencies(_) => Err(Error::Validation(
                "not yet implemented: ListWorkItemDependencies".into(),
            )),
            ApplicationQuery::ListWorkItemBlockers(_) => Err(Error::Validation(
                "not yet implemented: ListWorkItemBlockers".into(),
            )),
            ApplicationQuery::ListExpiredLeases(_) => Err(Error::Validation(
                "not yet implemented: ListExpiredLeases".into(),
            )),
            ApplicationQuery::GetProviderInstallation(_) => Err(Error::Validation(
                "not yet implemented: GetProviderInstallation".into(),
            )),
            ApplicationQuery::ListProviderInstallations(_) => Err(Error::Validation(
                "not yet implemented: ListProviderInstallations".into(),
            )),
            ApplicationQuery::ListProviderInstances(_) => Err(Error::Validation(
                "not yet implemented: ListProviderInstances".into(),
            )),
            ApplicationQuery::GetProviderInstance(_) => Err(Error::Validation(
                "not yet implemented: GetProviderInstance".into(),
            )),
            ApplicationQuery::GetBuildStatus(_) => Err(Error::Validation(
                "not yet implemented: GetBuildStatus".into(),
            )),
            ApplicationQuery::ListBuildDiagnostics(_) => Err(Error::Validation(
                "not yet implemented: ListBuildDiagnostics".into(),
            )),
            ApplicationQuery::GetVerificationCoverage(_) => Err(Error::Validation(
                "not yet implemented: GetVerificationCoverage".into(),
            )),
            ApplicationQuery::ListGeneratedSourceMappings(_) => Err(Error::Validation(
                "not yet implemented: ListGeneratedSourceMappings".into(),
            )),
            ApplicationQuery::ListConflicts(_) => Err(Error::Validation(
                "not yet implemented: ListConflicts".into(),
            )),
            ApplicationQuery::ListBlockedReasons(_) => Err(Error::Validation(
                "not yet implemented: ListBlockedReasons".into(),
            )),
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
        Ok(CommandResult::ProjectCreated(CreateProjectResponse {
            project,
        }))
    }

    fn register_artifact(&self, req: RegisterArtifactRequest) -> Result<CommandResult> {
        let kind = parse_namespaced_id(&req.kind)?;
        let project = self.project_store.get_project(req.project)?;
        if project.is_none() {
            return Err(Error::NotFound(format!(
                "project {} not found",
                req.project
            )));
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
        Ok(CommandResult::ArtifactRegistered(
            RegisterArtifactResponse { artifact },
        ))
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
        Ok(CommandResult::EntityRegistered(RegisterEntityResponse {
            entity,
        }))
    }

    fn register_provider(&self, req: RegisterProviderRequest) -> Result<CommandResult> {
        let _project = self.require_project(req.project)?;
        self.provider_store.insert_provider(&req.provider)?;
        Ok(CommandResult::ProviderRegistered(
            RegisterProviderResponse {
                provider: req.provider,
            },
        ))
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
        Ok(CommandResult::ProviderRunStarted(
            StartProviderRunResponse { run },
        ))
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
        Ok(CommandResult::EvidenceAdded(AddEvidenceResponse {
            id: record_id,
        }))
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
            ChangeHypothesisStatusResponse {
                hypothesis: updated,
            },
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
            RecordContradictionResponse {
                id: contradiction_id,
            },
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
        let result = self.validation_service.validate_project(req.project)?;
        let report = result.report().clone();
        if let ValidationResult::Failed(_) = &result {
            let payload = Self::validation_report_payload(&report)?;
            with_event(
                &self.db,
                req.project,
                EVENT_KIND_PROJECT_VALIDATION_FAILED.clone(),
                EventSource::Project,
                Some(EventSubject::Project(req.project)),
                Some(payload),
                |_txn| Ok(()),
            )?;
        }
        Ok(CommandResult::ProjectValidated(ValidateProjectResponse {
            result,
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
        let _project = self.require_project(req.project)?;
        let operation = Operation::new(
            req.project,
            OPERATION_KIND_PROJECT_REBUILD_INDEXES.clone(),
            "rebuild-indexes",
        );
        let op_id = operation.id;
        with_event(
            &self.db,
            req.project,
            EVENT_KIND_PROJECT_INDEXES_REBUILT.clone(),
            EventSource::Project,
            Some(EventSubject::Project(req.project)),
            None,
            |txn| {
                build_derived_state_in_tx(txn, req.project)?;
                muts::insert_operation(txn, &operation)?;
                Ok(op_id)
            },
        )?;
        let updated = self
            .operation_store
            .get(op_id)?
            .ok_or_else(|| Error::NotFound(format!("operation {op_id} not found")))?;
        Ok(CommandResult::IndexesRebuilt(RebuildIndexesResponse {
            operation: updated,
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

    fn get_validation_report(&self, q: GetValidationReportQuery) -> Result<QueryResult> {
        let result = self.validation_service.validate_project(q.project)?;
        Ok(QueryResult::ValidationReport(ValidationReportResponse {
            report: result.report().clone(),
        }))
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
        let entities =
            self.entity_store
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
        Ok(QueryResult::Verifications(VerificationsResponse {
            records,
        }))
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
