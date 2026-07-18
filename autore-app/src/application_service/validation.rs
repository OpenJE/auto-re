use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use autore_core::validation::{
    validate_confidence_range, validate_namespaced_id, validate_no_cycle,
};
use autore_core::{Error, Result};
use autore_events::project_event_service::ProjectEventService;
use autore_schema::domain::records::{
    Artifact, Contradiction, EventSubject, EvidenceRecord, Hypothesis, NativeArtifact, Operation,
    OperationFailure, Project, ProjectEvent, ProviderRun, VerificationRecord, VerificationSubject,
};
use autore_schema::domain::{NamespacedId, SchemaVersion};
use autore_schema::ids::{
    ArtifactId, EntityId, EvidenceRecordId, HypothesisId, NativeArtifactId, OperationId, ProjectId,
    ProviderRunId, VerificationRecordId,
};
use autore_store::{
    ArtifactStore, ContradictionStore, Database, EntityColumn, EntityPage, EntityStore,
    EvidenceStore, HypothesisStore, NativeArtifactStore, OperationStore, ProjectStore,
    ProviderAliasStore, ProviderStore, RunQuery, VerificationStore,
};

use crate::application_service::requests::{
    ValidationFinding, ValidationReport, ValidationResult, ValidationSeverity,
};

/// Validates a raw namespaced-ID string and parses it into a typed [`NamespacedId`].
pub fn parse_namespaced_id(id: &str) -> Result<NamespacedId> {
    validate_namespaced_id(id)?;
    NamespacedId::parse(id).map_err(|e| Error::Validation(e.to_string()))
}

/// Validates a hypothesis confidence score is finite and within [0.0, 1.0].
pub fn validate_confidence(score: f64) -> Result<()> {
    validate_confidence_range(score, "hypothesis confidence")
}

/// Validates that a sub-record's project matches the command's project.
pub fn ensure_same_project(
    label: &str,
    command_project: ProjectId,
    record_project: ProjectId,
) -> Result<()> {
    if command_project != record_project {
        return Err(Error::Validation(format!(
            "{label} references a different project than the command"
        )));
    }
    Ok(())
}

/// Validates that a value is not empty after trimming.
pub fn validate_not_empty(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::Validation(format!("{label} must not be empty")));
    }
    Ok(())
}

/// Project-wide validation service.
///
/// Performs all 18 spec §25 checks and returns a stable, versioned
/// [`ValidationReport`]. The service is stateless: it loads the project
/// records from the backing stores on each call and never mutates data
/// itself.
pub struct ValidationService {
    pub(crate) db: Arc<Database>,
    pub(crate) project_store: Arc<dyn ProjectStore + Send + Sync>,
    pub(crate) artifact_store: Arc<dyn ArtifactStore + Send + Sync>,
    pub(crate) entity_store: Arc<dyn EntityStore + Send + Sync>,
    pub(crate) provider_store: Arc<dyn ProviderStore + Send + Sync>,
    pub(crate) evidence_store: Arc<dyn EvidenceStore + Send + Sync>,
    pub(crate) hypothesis_store: Arc<dyn HypothesisStore + Send + Sync>,
    pub(crate) contradiction_store: Arc<dyn ContradictionStore + Send + Sync>,
    pub(crate) verification_store: Arc<dyn VerificationStore + Send + Sync>,
    pub(crate) operation_store: Arc<dyn OperationStore + Send + Sync>,
    pub(crate) native_artifact_store: Arc<dyn NativeArtifactStore + Send + Sync>,
    pub(crate) alias_store: Arc<dyn ProviderAliasStore + Send + Sync>,
    pub(crate) events: Arc<dyn ProjectEventService + Send + Sync>,
}

impl ValidationService {
    /// Validates a single project, returning a [`ValidationResult`] that
    /// carries the full report.
    pub fn validate_project(&self, project_id: ProjectId) -> Result<ValidationResult> {
        let mut findings = Vec::new();

        let project = match self.project_store.get_project(project_id)? {
            Some(p) => p,
            None => {
                findings.push(ValidationFinding {
                    check: "project-exists".to_string(),
                    severity: ValidationSeverity::Error,
                    message: format!("project {} does not exist", project_id),
                    record_id: Some(project_id.to_string()),
                });
                return Ok(ValidationResult::Failed(ValidationReport::failed(
                    project_id, findings,
                )));
            }
        };

        let artifacts = self.artifact_store.list_by_project(project_id)?;
        let entities = self.list_all_entities(project_id)?;
        let providers = self.provider_store.list_providers()?;
        let runs = self.provider_store.list_runs(RunQuery {
            project_id,
            status_filter: None,
            provider_filter: None,
            offset: 0,
            limit: 10_000,
        })?;
        let evidence_records = self.evidence_store.list_by_project(project_id)?;
        let hypotheses = self.hypothesis_store.list_by_project(project_id)?;
        let contradictions = self.contradiction_store.list_by_project(project_id)?;
        let verifications = self.verification_store.list_by_project(project_id)?;
        let operations = self.operation_store.list_by_project(project_id)?;
        let native_artifacts = self.list_native_artifacts_for_project(&runs)?;
        let aliases = self.list_aliases_for_project(&runs)?;
        let events = self.events.events_after(project_id, 0, 10_000)?;

        let artifact_ids: HashSet<ArtifactId> = artifacts.iter().map(|a| a.id).collect();
        let entity_ids: HashSet<EntityId> = entities.iter().map(|e| e.id).collect();
        let provider_ids: HashSet<autore_schema::ids::ProviderId> =
            providers.iter().map(|p| p.id).collect();
        let run_ids: HashSet<ProviderRunId> = runs.iter().map(|r| r.id).collect();
        let evidence_ids: HashSet<EvidenceRecordId> =
            evidence_records.iter().map(|e| e.id).collect();
        let hypothesis_ids: HashSet<HypothesisId> = hypotheses.iter().map(|h| h.id).collect();
        let contradiction_ids: HashSet<autore_schema::ids::ContradictionId> =
            contradictions.iter().map(|c| c.id).collect();
        let verification_ids: HashSet<VerificationRecordId> =
            verifications.iter().map(|v| v.id).collect();
        let operation_ids: HashSet<OperationId> = operations.iter().map(|o| o.id).collect();
        let native_artifact_ids: HashSet<NativeArtifactId> =
            native_artifacts.iter().map(|n| n.id).collect();

        self.check_project_scope(project_id, &artifacts, &native_artifacts, &mut findings);
        self.check_namespaced_ids(
            &artifacts,
            &entities,
            &runs,
            &native_artifacts,
            &evidence_records,
            &hypotheses,
            &contradictions,
            &verifications,
            &operations,
            &mut findings,
        );
        self.check_artifact_integrity(project_id, &artifacts, &mut findings);
        self.check_provider_runs(&runs, &provider_ids, &artifact_ids, &mut findings);
        self.check_native_artifacts(
            &native_artifacts,
            &run_ids,
            &artifact_ids,
            &entity_ids,
            &mut findings,
        );
        self.check_evidence_records(
            &evidence_records,
            &entity_ids,
            &run_ids,
            &native_artifact_ids,
            &evidence_ids,
            &mut findings,
        );
        self.check_hypotheses(
            &hypotheses,
            &entity_ids,
            &evidence_ids,
            &hypothesis_ids,
            &mut findings,
        );
        self.check_hypothesis_supersession_cycles(&hypotheses, &mut findings);
        self.check_contradictions(
            &contradictions,
            &entity_ids,
            &evidence_ids,
            &hypothesis_ids,
            &mut findings,
        );
        self.check_verifications(
            &verifications,
            &entity_ids,
            &hypothesis_ids,
            &artifact_ids,
            &run_ids,
            &evidence_ids,
            &mut findings,
        );
        self.check_operation_parent_cycles(&operations, &mut findings);
        self.check_events(
            project_id,
            &events,
            &operation_ids,
            &artifact_ids,
            &entity_ids,
            &evidence_ids,
            &hypothesis_ids,
            &contradiction_ids,
            &verification_ids,
            &mut findings,
        );
        self.check_schema_version(&project, &mut findings);
        self.check_derived_indexes(&aliases, &run_ids, &entity_ids, &mut findings);

        let has_errors = findings
            .iter()
            .any(|f| f.severity == ValidationSeverity::Error);
        let report = if has_errors {
            ValidationReport::failed(project_id, findings)
        } else {
            ValidationReport::passed(project_id)
        };

        Ok(if has_errors {
            ValidationResult::Failed(report)
        } else {
            ValidationResult::Passed(report)
        })
    }

    fn list_all_entities(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<autore_schema::domain::records::SemanticEntity>> {
        let page = EntityPage {
            offset: 0,
            limit: 10_000,
            order_by: EntityColumn::CreatedAt,
        };
        self.entity_store.list_by_project(project_id, page, None)
    }

    fn list_native_artifacts_for_project(
        &self,
        runs: &[ProviderRun],
    ) -> Result<Vec<NativeArtifact>> {
        let mut out = Vec::new();
        let run_ids: HashSet<ProviderRunId> = runs.iter().map(|r| r.id).collect();
        for run in runs {
            for na in self.native_artifact_store.list_by_run(run.id)? {
                if run_ids.contains(&na.provider_run) {
                    out.push(na);
                }
            }
        }
        Ok(out)
    }

    fn list_aliases_for_project(
        &self,
        runs: &[ProviderRun],
    ) -> Result<Vec<autore_schema::domain::records::ProviderEntityAlias>> {
        let mut out = Vec::new();
        let run_ids: HashSet<ProviderRunId> = runs.iter().map(|r| r.id).collect();
        for run in runs {
            for alias in self.alias_store.list_aliases_for_run(run.id)? {
                if run_ids.contains(&alias.provider_run) {
                    out.push(alias);
                }
            }
        }
        Ok(out)
    }

    fn add_error(
        &self,
        findings: &mut Vec<ValidationFinding>,
        check: &str,
        message: String,
        record_id: Option<String>,
    ) {
        findings.push(ValidationFinding {
            check: check.to_string(),
            severity: ValidationSeverity::Error,
            message,
            record_id,
        });
    }

    fn check_project_scope(
        &self,
        project_id: ProjectId,
        artifacts: &[Artifact],
        native_artifacts: &[NativeArtifact],
        findings: &mut Vec<ValidationFinding>,
    ) {
        for a in artifacts {
            if a.project != project_id {
                self.add_error(
                    findings,
                    "cross-project-reference",
                    format!("artifact {} belongs to project {}", a.id, a.project),
                    Some(a.id.to_string()),
                );
            }
        }
        let artifact_id_to_project: HashMap<ArtifactId, ProjectId> =
            artifacts.iter().map(|a| (a.id, a.project)).collect();
        for na in native_artifacts {
            if let Some(&proj) = artifact_id_to_project.get(&na.artifact)
                && proj != project_id
            {
                self.add_error(
                    findings,
                    "cross-project-reference",
                    format!(
                        "native artifact {} points to artifact in another project",
                        na.id
                    ),
                    Some(na.id.to_string()),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check_namespaced_ids(
        &self,
        artifacts: &[Artifact],
        entities: &[autore_schema::domain::records::SemanticEntity],
        runs: &[ProviderRun],
        native_artifacts: &[NativeArtifact],
        evidence_records: &[EvidenceRecord],
        hypotheses: &[Hypothesis],
        contradictions: &[Contradiction],
        verifications: &[VerificationRecord],
        operations: &[Operation],
        findings: &mut Vec<ValidationFinding>,
    ) {
        let mut check = |value: &NamespacedId, check_name: &str, record_id: &str| {
            if validate_namespaced_id(value.as_str()).is_err() {
                self.add_error(
                    findings,
                    check_name,
                    format!("invalid namespaced id: {value}"),
                    Some(record_id.to_string()),
                );
            }
        };

        for a in artifacts {
            check(&a.kind, "namespaced-id", &a.id.to_string());
        }
        for e in entities {
            check(&e.kind, "namespaced-id", &e.id.to_string());
        }
        for r in runs {
            check(&r.operation, "namespaced-id", &r.id.to_string());
            check(
                &r.environment.operating_system,
                "namespaced-id",
                &r.id.to_string(),
            );
            check(
                &r.environment.architecture,
                "namespaced-id",
                &r.id.to_string(),
            );
            if let Some(isolation) = &r.environment.isolation_backend {
                check(isolation, "namespaced-id", &r.id.to_string());
            }
        }
        for na in native_artifacts {
            check(&na.format, "namespaced-id", &na.id.to_string());
        }
        for e in evidence_records {
            check(&e.predicate, "namespaced-id", &e.id.to_string());
        }
        for h in hypotheses {
            check(&h.predicate, "namespaced-id", &h.id.to_string());
        }
        for c in contradictions {
            check(&c.predicate, "namespaced-id", &c.id.to_string());
        }
        for v in verifications {
            check(&v.check, "namespaced-id", &v.id.to_string());
        }
        for o in operations {
            check(&o.kind, "namespaced-id", &o.id.to_string());
            if let Some(OperationFailure { code, .. }) = &o.failure {
                check(code, "namespaced-id", &o.id.to_string());
            }
        }
    }

    fn check_artifact_integrity(
        &self,
        project_id: ProjectId,
        artifacts: &[Artifact],
        findings: &mut Vec<ValidationFinding>,
    ) {
        for a in artifacts {
            match self.artifact_store.verify_artifact(project_id, a) {
                Ok(_) => {}
                Err(Error::HashMismatch) => {
                    let kind = match &a.storage {
                        autore_schema::domain::ArtifactStorage::ManagedBlob { .. } => {
                            "managed artifact hash mismatch"
                        }
                        autore_schema::domain::ArtifactStorage::ExternalFile { .. } => {
                            "modified external artifact"
                        }
                    };
                    self.add_error(
                        findings,
                        "artifact-integrity",
                        format!("{kind}: {}", a.id),
                        Some(a.id.to_string()),
                    );
                }
                Err(Error::NotFound(_)) => {
                    let kind = match &a.storage {
                        autore_schema::domain::ArtifactStorage::ManagedBlob { .. } => {
                            "managed blob missing"
                        }
                        autore_schema::domain::ArtifactStorage::ExternalFile { .. } => {
                            "external artifact missing"
                        }
                    };
                    self.add_error(
                        findings,
                        "artifact-integrity",
                        format!("{kind}: {}", a.id),
                        Some(a.id.to_string()),
                    );
                }
                Err(e) => {
                    self.add_error(
                        findings,
                        "artifact-integrity",
                        format!("artifact {} verification failed: {e}", a.id),
                        Some(a.id.to_string()),
                    );
                }
            }
        }
    }

    fn check_provider_runs(
        &self,
        runs: &[ProviderRun],
        provider_ids: &HashSet<autore_schema::ids::ProviderId>,
        artifact_ids: &HashSet<ArtifactId>,
        findings: &mut Vec<ValidationFinding>,
    ) {
        for r in runs {
            if !provider_ids.contains(&r.provider) {
                self.add_error(
                    findings,
                    "provider-run-reference",
                    format!(
                        "provider run {} references missing provider {}",
                        r.id, r.provider
                    ),
                    Some(r.id.to_string()),
                );
            }
            for aid in &r.input_artifacts {
                if !artifact_ids.contains(aid) {
                    self.add_error(
                        findings,
                        "provider-run-reference",
                        format!("provider run {} references missing artifact {}", r.id, aid),
                        Some(r.id.to_string()),
                    );
                }
            }
            if let Some(aid) = r.configuration_artifact
                && !artifact_ids.contains(&aid)
            {
                self.add_error(
                    findings,
                    "provider-run-reference",
                    format!(
                        "provider run {} references missing configuration artifact {}",
                        r.id, aid
                    ),
                    Some(r.id.to_string()),
                );
            }
        }
    }

    fn check_native_artifacts(
        &self,
        native_artifacts: &[NativeArtifact],
        run_ids: &HashSet<ProviderRunId>,
        artifact_ids: &HashSet<ArtifactId>,
        entity_ids: &HashSet<EntityId>,
        findings: &mut Vec<ValidationFinding>,
    ) {
        for na in native_artifacts {
            if !run_ids.contains(&na.provider_run) {
                self.add_error(
                    findings,
                    "native-artifact-reference",
                    format!(
                        "native artifact {} references missing provider run {}",
                        na.id, na.provider_run
                    ),
                    Some(na.id.to_string()),
                );
            }
            if !artifact_ids.contains(&na.artifact) {
                self.add_error(
                    findings,
                    "native-artifact-reference",
                    format!(
                        "native artifact {} references missing artifact {}",
                        na.id, na.artifact
                    ),
                    Some(na.id.to_string()),
                );
            }
            for eid in &na.subject_entities {
                if !entity_ids.contains(eid) {
                    self.add_error(
                        findings,
                        "native-artifact-reference",
                        format!(
                            "native artifact {} references missing subject entity {}",
                            na.id, eid
                        ),
                        Some(na.id.to_string()),
                    );
                }
            }
        }
    }

    fn check_evidence_records(
        &self,
        evidence_records: &[EvidenceRecord],
        entity_ids: &HashSet<EntityId>,
        run_ids: &HashSet<ProviderRunId>,
        native_artifact_ids: &HashSet<NativeArtifactId>,
        evidence_ids: &HashSet<EvidenceRecordId>,
        findings: &mut Vec<ValidationFinding>,
    ) {
        for e in evidence_records {
            if !entity_ids.contains(&e.subject) {
                self.add_error(
                    findings,
                    "evidence-reference",
                    format!(
                        "evidence {} references missing subject entity {}",
                        e.id, e.subject
                    ),
                    Some(e.id.to_string()),
                );
            }
            if let Some(run_id) = e.provider_run
                && !run_ids.contains(&run_id)
            {
                self.add_error(
                    findings,
                    "evidence-reference",
                    format!(
                        "evidence {} references missing provider run {}",
                        e.id, run_id
                    ),
                    Some(e.id.to_string()),
                );
            }
            for na_id in &e.native_artifacts {
                if !native_artifact_ids.contains(na_id) {
                    self.add_error(
                        findings,
                        "evidence-reference",
                        format!(
                            "evidence {} references missing native artifact {}",
                            e.id, na_id
                        ),
                        Some(e.id.to_string()),
                    );
                }
            }
            for assumption in &e.assumptions {
                if let Some(evid) = assumption.evidence
                    && !evidence_ids.contains(&evid)
                {
                    self.add_error(
                        findings,
                        "evidence-reference",
                        format!(
                            "evidence {} assumption references missing evidence {}",
                            e.id, evid
                        ),
                        Some(e.id.to_string()),
                    );
                }
            }
        }
    }

    fn check_hypotheses(
        &self,
        hypotheses: &[Hypothesis],
        entity_ids: &HashSet<EntityId>,
        evidence_ids: &HashSet<EvidenceRecordId>,
        hypothesis_ids: &HashSet<HypothesisId>,
        findings: &mut Vec<ValidationFinding>,
    ) {
        for h in hypotheses {
            if !entity_ids.contains(&h.subject) {
                self.add_error(
                    findings,
                    "hypothesis-reference",
                    format!(
                        "hypothesis {} references missing subject entity {}",
                        h.id, h.subject
                    ),
                    Some(h.id.to_string()),
                );
            }
            for evid in &h.supporting_evidence {
                if !evidence_ids.contains(evid) {
                    self.add_error(
                        findings,
                        "hypothesis-reference",
                        format!(
                            "hypothesis {} references missing supporting evidence {}",
                            h.id, evid
                        ),
                        Some(h.id.to_string()),
                    );
                }
            }
            for evid in &h.contradicting_evidence {
                if !evidence_ids.contains(evid) {
                    self.add_error(
                        findings,
                        "hypothesis-reference",
                        format!(
                            "hypothesis {} references missing contradicting evidence {}",
                            h.id, evid
                        ),
                        Some(h.id.to_string()),
                    );
                }
            }
            for hid in &h.derived_from {
                if !hypothesis_ids.contains(hid) {
                    self.add_error(
                        findings,
                        "hypothesis-reference",
                        format!(
                            "hypothesis {} references missing parent hypothesis {}",
                            h.id, hid
                        ),
                        Some(h.id.to_string()),
                    );
                }
            }
            let score = h.confidence.score() as f64;
            if validate_confidence_range(score, "hypothesis confidence").is_err() {
                self.add_error(
                    findings,
                    "confidence-range",
                    format!("hypothesis {} has invalid confidence {score}", h.id),
                    Some(h.id.to_string()),
                );
            }
        }
    }

    fn check_hypothesis_supersession_cycles(
        &self,
        hypotheses: &[Hypothesis],
        findings: &mut Vec<ValidationFinding>,
    ) {
        let ids: Vec<String> = hypotheses.iter().map(|h| h.id.to_string()).collect();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let index_of: HashMap<&str, usize> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        for h in hypotheses {
            if let autore_schema::domain::records::HypothesisStatus::Superseded { by } = h.status
                && let Some(&from) = index_of.get(h.id.to_string().as_str())
                && let Some(&to) = index_of.get(by.to_string().as_str())
            {
                edges.push((from, to));
            }
        }
        if let Err(e) = validate_no_cycle(&ids, &edges) {
            self.add_error(
                findings,
                "hypothesis-supersession-cycle",
                e.to_string(),
                None,
            );
        }
    }

    fn check_contradictions(
        &self,
        contradictions: &[Contradiction],
        entity_ids: &HashSet<EntityId>,
        evidence_ids: &HashSet<EvidenceRecordId>,
        hypothesis_ids: &HashSet<HypothesisId>,
        findings: &mut Vec<ValidationFinding>,
    ) {
        for c in contradictions {
            if !entity_ids.contains(&c.subject) {
                self.add_error(
                    findings,
                    "contradiction-reference",
                    format!(
                        "contradiction {} references missing subject entity {}",
                        c.id, c.subject
                    ),
                    Some(c.id.to_string()),
                );
            }
            for evid in &c.evidence {
                if !evidence_ids.contains(evid) {
                    self.add_error(
                        findings,
                        "contradiction-reference",
                        format!(
                            "contradiction {} references missing evidence {}",
                            c.id, evid
                        ),
                        Some(c.id.to_string()),
                    );
                }
            }
            for hid in &c.hypotheses {
                if !hypothesis_ids.contains(hid) {
                    self.add_error(
                        findings,
                        "contradiction-reference",
                        format!(
                            "contradiction {} references missing hypothesis {}",
                            c.id, hid
                        ),
                        Some(c.id.to_string()),
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check_verifications(
        &self,
        verifications: &[VerificationRecord],
        entity_ids: &HashSet<EntityId>,
        hypothesis_ids: &HashSet<HypothesisId>,
        artifact_ids: &HashSet<ArtifactId>,
        run_ids: &HashSet<ProviderRunId>,
        evidence_ids: &HashSet<EvidenceRecordId>,
        findings: &mut Vec<ValidationFinding>,
    ) {
        for v in verifications {
            let subject_ok = match v.subject {
                VerificationSubject::Entity(id) => entity_ids.contains(&id),
                VerificationSubject::Hypothesis(id) => hypothesis_ids.contains(&id),
                VerificationSubject::Artifact(id) => artifact_ids.contains(&id),
                VerificationSubject::GenerationTarget(_) => true,
            };
            if !subject_ok {
                self.add_error(
                    findings,
                    "verification-reference",
                    format!(
                        "verification {} references missing subject {:?}",
                        v.id, v.subject
                    ),
                    Some(v.id.to_string()),
                );
            }
            if let Some(run_id) = v.provider_run
                && !run_ids.contains(&run_id)
            {
                self.add_error(
                    findings,
                    "verification-reference",
                    format!(
                        "verification {} references missing provider run {}",
                        v.id, run_id
                    ),
                    Some(v.id.to_string()),
                );
            }
            for evid in &v.evidence {
                if !evidence_ids.contains(evid) {
                    self.add_error(
                        findings,
                        "verification-reference",
                        format!("verification {} references missing evidence {}", v.id, evid),
                        Some(v.id.to_string()),
                    );
                }
            }
        }
    }

    fn check_operation_parent_cycles(
        &self,
        operations: &[Operation],
        findings: &mut Vec<ValidationFinding>,
    ) {
        let ids: Vec<String> = operations.iter().map(|o| o.id.to_string()).collect();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let index_of: HashMap<&str, usize> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        for o in operations {
            if let Some(parent_id) = o.parent
                && let Some(&from) = index_of.get(o.id.to_string().as_str())
                && let Some(&to) = index_of.get(parent_id.to_string().as_str())
            {
                edges.push((from, to));
            }
        }
        if let Err(e) = validate_no_cycle(&ids, &edges) {
            self.add_error(findings, "operation-parent-cycle", e.to_string(), None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check_events(
        &self,
        project_id: ProjectId,
        events: &[ProjectEvent],
        operation_ids: &HashSet<OperationId>,
        artifact_ids: &HashSet<ArtifactId>,
        entity_ids: &HashSet<EntityId>,
        evidence_ids: &HashSet<EvidenceRecordId>,
        hypothesis_ids: &HashSet<HypothesisId>,
        contradiction_ids: &HashSet<autore_schema::ids::ContradictionId>,
        verification_ids: &HashSet<VerificationRecordId>,
        findings: &mut Vec<ValidationFinding>,
    ) {
        let mut sorted: Vec<&ProjectEvent> = events
            .iter()
            .filter(|ev| ev.project == project_id)
            .collect();
        sorted.sort_by(|a, b| {
            a.created_at
                .as_offset_datetime()
                .cmp(b.created_at.as_offset_datetime())
        });
        let mut last: Option<u64> = None;
        let mut seen = HashSet::new();
        for ev in sorted {
            if let Some(prev) = last
                && ev.sequence <= prev
            {
                self.add_error(
                    findings,
                    "event-sequence",
                    format!(
                        "event sequence {} is not strictly greater than previous {}",
                        ev.sequence, prev
                    ),
                    Some(ev.id.to_string()),
                );
            }
            if !seen.insert(ev.sequence) {
                self.add_error(
                    findings,
                    "event-sequence",
                    format!("duplicate event sequence {}", ev.sequence),
                    Some(ev.id.to_string()),
                );
            }
            last = Some(ev.sequence);

            if let Some(subject) = &ev.subject {
                let subject_ok = match subject {
                    EventSubject::Operation(id) => operation_ids.contains(id),
                    EventSubject::Project(_) => true,
                    EventSubject::Artifact(id) => artifact_ids.contains(id),
                    EventSubject::Entity(id) => entity_ids.contains(id),
                    EventSubject::Evidence(id) => evidence_ids.contains(id),
                    EventSubject::Hypothesis(id) => hypothesis_ids.contains(id),
                    EventSubject::Contradiction(id) => contradiction_ids.contains(id),
                    EventSubject::Verification(id) => verification_ids.contains(id),
                };
                if !subject_ok {
                    self.add_error(
                        findings,
                        "event-subject-reference",
                        format!("event {} references missing subject {:?}", ev.id, subject),
                        Some(ev.id.to_string()),
                    );
                }
            }
        }
    }

    fn check_schema_version(&self, project: &Project, findings: &mut Vec<ValidationFinding>) {
        let expected = SchemaVersion::new(2, 0);
        if project.schema_version != expected {
            self.add_error(
                findings,
                "schema-version",
                format!(
                    "project schema version {} does not match expected {}",
                    project.schema_version, expected
                ),
                Some(project.id.to_string()),
            );
        }

        let conn = match self.db.connection() {
            Ok(c) => c,
            Err(e) => {
                self.add_error(
                    findings,
                    "schema-version",
                    format!("failed to connect to database for schema check: {e}"),
                    Some(project.id.to_string()),
                );
                return;
            }
        };
        let expected_tables: HashSet<&str> = [
            "projects",
            "stage0_artifacts",
            "semantic_entities",
            "providers",
            "provider_runs",
            "provider_entity_aliases",
            "native_artifacts",
            "evidence_records",
            "evidence_lifecycle_events",
            "hypotheses",
            "contradictions",
            "verification_records",
            "operations",
            "progress_updates",
            "cancellation_requests",
            "project_events",
        ]
        .iter()
        .copied()
        .collect();
        let actual: HashSet<String> = match conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        ) {
            Ok(mut stmt) => match stmt.query_map([], |row| row.get::<_, String>(0)) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    self.add_error(
                        findings,
                        "schema-version",
                        format!("failed to list database tables: {e}"),
                        Some(project.id.to_string()),
                    );
                    return;
                }
            },
            Err(e) => {
                self.add_error(
                    findings,
                    "schema-version",
                    format!("failed to list database tables: {e}"),
                    Some(project.id.to_string()),
                );
                return;
            }
        };
        for table in &expected_tables {
            if !actual.contains(*table) {
                self.add_error(
                    findings,
                    "schema-version",
                    format!("required V2 table '{table}' is missing"),
                    Some(project.id.to_string()),
                );
            }
        }
    }

    fn check_derived_indexes(
        &self,
        aliases: &[autore_schema::domain::records::ProviderEntityAlias],
        run_ids: &HashSet<ProviderRunId>,
        entity_ids: &HashSet<EntityId>,
        findings: &mut Vec<ValidationFinding>,
    ) {
        for alias in aliases {
            if !run_ids.contains(&alias.provider_run) {
                self.add_error(
                    findings,
                    "derived-index",
                    format!(
                        "provider alias references missing provider run {}",
                        alias.provider_run
                    ),
                    Some(alias.provider_run.to_string()),
                );
            }
            if !entity_ids.contains(&alias.entity) {
                self.add_error(
                    findings,
                    "derived-index",
                    format!("provider alias references missing entity {}", alias.entity),
                    Some(alias.entity.to_string()),
                );
            }
        }
    }
}
