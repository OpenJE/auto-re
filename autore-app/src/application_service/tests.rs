use std::sync::Arc;

use autore_events::project_event_service::{EventBroadcaster, LocalProjectEventService};
use autore_core::operation::OperationState;
use autore_schema::domain::records::{
    Contradiction, EvidenceRecord, HypothesisStatus, Provider, ProviderRun, ProviderRunStatus,
    VerificationRecord, VerificationSubject,
};
use autore_schema::domain::{ContentHash, Derivation, DerivationMethod, EnvironmentIdentity, EvidenceValue, NamespacedId, Timestamp};
use autore_schema::ids::{
    EntityId, EvidenceRecordId, HypothesisId, ProjectId, ProviderRunId,
};
use autore_store::Database;

use crate::application_service::requests::*;
use crate::application_service::ApplicationService;

fn test_service() -> (ApplicationService, tempfile::TempDir) {
    let db = Arc::new(Database::open_in_memory().unwrap());
    let broadcaster = Arc::new(EventBroadcaster::new());
    let events = Arc::new(LocalProjectEventService::new(db.clone(), broadcaster));
    let temp_dir = tempfile::TempDir::new().unwrap();
    let service = ApplicationService::new(db, events, temp_dir.path());
    (service, temp_dir)
}

fn create_project(service: &ApplicationService, name: &str) -> ProjectId {
    let result = service
        .execute(ApplicationCommand::CreateProject(CreateProjectRequest {
            name: name.into(),
        }))
        .unwrap();
    match result {
        CommandResult::ProjectCreated(resp) => resp.project.id,
        _ => panic!("expected ProjectCreated"),
    }
}

fn register_entity(service: &ApplicationService, project: ProjectId) -> autore_schema::ids::EntityId {
    let result = service
        .execute(ApplicationCommand::RegisterEntity(RegisterEntityRequest {
            project,
            kind: "core.function".into(),
            stable_key: None,
            display_name: Some("test-entity".into()),
        }))
        .unwrap();
    match result {
        CommandResult::EntityRegistered(resp) => resp.entity.id,
        _ => panic!("expected EntityRegistered"),
    }
}

#[test]
fn create_project_emits_event() {
    let (service, _tmp) = test_service();
    let project_id = create_project(&service, "event-test");

    let events = service
        .events
        .events_after(project_id, 0, 10)
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].kind,
        autore_schema::domain::records::EVENT_KIND_PROJECT_CREATED.clone()
    );
}

#[test]
fn register_artifact_emits_event() {
    let (service, tmp) = test_service();
    let project_id = create_project(&service, "artifact-test");

    let source_path = tmp.path().join("artifact.txt");
    std::fs::write(&source_path, "hello artifact").unwrap();

    let result = service
        .execute(ApplicationCommand::RegisterArtifact(RegisterArtifactRequest {
            project: project_id,
            source_path: source_path.clone(),
            kind: "core.binary".into(),
        }))
        .unwrap();
    let artifact_id = match result {
        CommandResult::ArtifactRegistered(resp) => resp.artifact.id,
        _ => panic!("expected ArtifactRegistered"),
    };

    let events = service
        .events
        .events_after(project_id, 0, 10)
        .unwrap();
    let artifact_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == autore_schema::domain::records::EVENT_KIND_ARTIFACT_REGISTERED.clone())
        .collect();
    assert_eq!(artifact_events.len(), 1);
    assert_eq!(
        artifact_events[0].subject,
        Some(autore_schema::domain::records::EventSubject::Artifact(artifact_id))
    );
}

#[test]
fn hypothesis_accept_emits_event_and_keeps_competitors() {
    let (service, _tmp) = test_service();
    let project_id = create_project(&service, "hypothesis-test");
    let entity_id = register_entity(&service, project_id);

    let h1 = add_hypothesis(&service, project_id, entity_id, "candidate-a", 0.6);
    let h2 = add_hypothesis(&service, project_id, entity_id, "candidate-b", 0.7);

    service
        .execute(ApplicationCommand::ChangeHypothesisStatus(
            ChangeHypothesisStatusRequest {
                project: project_id,
                id: h1,
                status: HypothesisStatus::Accepted,
            },
        ))
        .unwrap();

    let events = service
        .events
        .events_after(project_id, 0, 10)
        .unwrap();
    let accept_events: Vec<_> = events
        .iter()
        .filter(|e| {
            e.kind == autore_schema::domain::records::EVENT_KIND_HYPOTHESIS_ACCEPTED.clone()
        })
        .collect();
    assert_eq!(accept_events.len(), 1);
    assert_eq!(
        accept_events[0].subject,
        Some(autore_schema::domain::records::EventSubject::Hypothesis(h1))
    );

    let h2_result = service
        .query(ApplicationQuery::GetHypothesis(GetHypothesisQuery { id: h2 }))
        .unwrap();
    match h2_result {
        QueryResult::Hypothesis(resp) => {
            assert_eq!(
                resp.hypothesis.status,
                HypothesisStatus::UnderInvestigation,
                "competitor hypothesis must remain unchanged"
            );
        }
        _ => panic!("expected Hypothesis"),
    }
}

fn add_hypothesis(
    service: &ApplicationService,
    project: ProjectId,
    subject: EntityId,
    candidate: &str,
    confidence: f64,
) -> HypothesisId {
    let result = service
        .execute(ApplicationCommand::AddHypothesis(AddHypothesisRequest {
            project,
            subject,
            predicate: "hypothesis.test".into(),
            candidate: EvidenceValue::String(candidate.into()),
            confidence_score: confidence,
            confidence_rationale: None,
            supporting_evidence: vec![],
            contradicting_evidence: vec![],
            derived_from: vec![],
            status: HypothesisStatus::UnderInvestigation,
        }))
        .unwrap();
    match result {
        CommandResult::HypothesisAdded(resp) => resp.id,
        _ => panic!("expected HypothesisAdded"),
    }
}

#[test]
fn cancel_operation_is_cooperative() {
    let (service, _tmp) = test_service();
    let project_id = create_project(&service, "cancel-test");

    let result = service
        .execute(ApplicationCommand::ValidateProject(ValidateProjectRequest {
            project: project_id,
        }))
        .unwrap();
    let op_id = match result {
        CommandResult::ProjectValidated(resp) => resp.operation.id,
        _ => panic!("expected ProjectValidated"),
    };

    service
        .operation_store
        .transition(op_id, OperationState::Running, None)
        .unwrap();

    service
        .execute(ApplicationCommand::CancelOperation(CancelOperationRequest {
            project: project_id,
            id: op_id,
            requested_by: "user".into(),
            reason: Some("user requested stop".into()),
        }))
        .unwrap();

    let requests = service
        .operation_store
        .list_cancellation_requests(op_id)
        .unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].requested_by, "user");

    let op_result = service
        .query(ApplicationQuery::GetOperation(GetOperationQuery { id: op_id }))
        .unwrap();
    match op_result {
        QueryResult::Operation(resp) => {
            assert_eq!(resp.operation.state, OperationState::Cancelling);
        }
        _ => panic!("expected Operation"),
    }
}

#[test]
fn command_rejects_invalid_request() {
    let (service, tmp) = test_service();
    let project_id = create_project(&service, "reject-test");

    let source_path = tmp.path().join("artifact.txt");
    std::fs::write(&source_path, "x").unwrap();

    let result = service.execute(ApplicationCommand::RegisterArtifact(
        RegisterArtifactRequest {
            project: project_id,
            source_path,
            kind: "invalid-kind".into(),
        },
    ));
    assert!(result.is_err(), "invalid namespaced ID must be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("namespaced ID") || err.contains("Validation"),
        "unexpected error: {err}"
    );

    let result = service.execute(ApplicationCommand::AddHypothesis(AddHypothesisRequest {
        project: project_id,
        subject: autore_schema::ids::EntityId::new(),
        predicate: "hypothesis.test".into(),
        candidate: EvidenceValue::String("x".into()),
        confidence_score: 1.5,
        confidence_rationale: None,
        supporting_evidence: vec![],
        contradicting_evidence: vec![],
        derived_from: vec![],
        status: HypothesisStatus::Proposed,
    }));
    assert!(result.is_err(), "out-of-range confidence must be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("confidence") || err.contains("Validation"),
        "unexpected error: {err}"
    );
}

#[test]
fn register_provider_round_trips() {
    let (service, _tmp) = test_service();
    let project_id = create_project(&service, "provider-test");

    let provider = Provider::new(
        "test-provider",
        NamespacedId::parse("provider.disassembler").unwrap(),
        "1.0.0",
    );
    let result = service
        .execute(ApplicationCommand::RegisterProvider(RegisterProviderRequest {
            project: project_id,
            provider: provider.clone(),
        }))
        .unwrap();
    let registered = match result {
        CommandResult::ProviderRegistered(resp) => resp.provider,
        _ => panic!("expected ProviderRegistered"),
    };
    assert_eq!(registered.id, provider.id);

    let fetched = service
        .query(ApplicationQuery::GetProvider(GetProviderQuery { id: provider.id }))
        .unwrap();
    match fetched {
        QueryResult::Provider(resp) => assert_eq!(resp.provider.id, provider.id),
        _ => panic!("expected Provider"),
    }
}

#[test]
fn start_provider_run_round_trips() {
    let (service, _tmp) = test_service();
    let project_id = create_project(&service, "run-test");

    let provider = Provider::new(
        "run-provider",
        NamespacedId::parse("provider.disassembler").unwrap(),
        "1.0.0",
    );
    service
        .execute(ApplicationCommand::RegisterProvider(RegisterProviderRequest {
            project: project_id,
            provider: provider.clone(),
        }))
        .unwrap();

    let run = ProviderRun {
        id: ProviderRunId::new(),
        project: project_id,
        provider: provider.id,
        operation: NamespacedId::parse("core.operation.disassemble").unwrap(),
        input_artifacts: vec![],
        configuration_artifact: None,
        configuration_hash: ContentHash::sha256(b"config"),
        environment: EnvironmentIdentity {
            operating_system: NamespacedId::parse("os.linux").unwrap(),
            architecture: NamespacedId::parse("arch.x86-64").unwrap(),
            isolation_backend: None,
            image_digest: None,
            extension: None,
        },
        started_at: Timestamp::now(),
        completed_at: None,
        status: ProviderRunStatus::Running,
    };

    service
        .provider_store
        .start_run(&run)
        .unwrap();

    let fetched = service
        .query(ApplicationQuery::GetProviderRun(GetProviderRunQuery { id: run.id }))
        .unwrap();
    match fetched {
        QueryResult::ProviderRun(resp) => assert_eq!(resp.run.id, run.id),
        _ => panic!("expected ProviderRun"),
    }
}

#[test]
fn add_evidence_records_event() {
    let (service, _tmp) = test_service();
    let project_id = create_project(&service, "evidence-test");
    let entity_id = register_entity(&service, project_id);

    let record = EvidenceRecord {
        id: EvidenceRecordId::new(),
        project: project_id,
        subject: entity_id,
        predicate: NamespacedId::parse("evidence.predicate.test").unwrap(),
        value: EvidenceValue::String("observed".into()),
        derivation: autore_schema::domain::Derivation::new(
            autore_schema::domain::DerivationMethod::DirectObservation,
            NamespacedId::parse("core.observe").unwrap(),
            vec![],
            vec![],
        ),
        provider_run: None,
        native_artifacts: vec![],
        assumptions: vec![],
        created_at: Timestamp::now(),
    };

    let result = service
        .execute(ApplicationCommand::AddEvidence(AddEvidenceRequest {
            project: project_id,
            record: record.clone(),
        }))
        .unwrap();
    let id = match result {
        CommandResult::EvidenceAdded(resp) => resp.id,
        _ => panic!("expected EvidenceAdded"),
    };
    assert_eq!(id, record.id);

    let events = service
        .events
        .events_after(project_id, 0, 10)
        .unwrap();
    let evidence_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == autore_schema::domain::records::EVENT_KIND_EVIDENCE_ADDED.clone())
        .collect();
    assert_eq!(evidence_events.len(), 1);
}

#[test]
fn record_contradiction_emits_event() {
    let (service, _tmp) = test_service();
    let project_id = create_project(&service, "contradiction-test");
    let entity_id = register_entity(&service, project_id);

    let contradiction = Contradiction::new(
        project_id,
        entity_id,
        NamespacedId::parse("hypothesis.test").unwrap(),
        vec![],
        vec![],
    );

    let result = service
        .execute(ApplicationCommand::RecordContradiction(
            RecordContradictionRequest {
                project: project_id,
                contradiction: contradiction.clone(),
            },
        ))
        .unwrap();
    let id = match result {
        CommandResult::ContradictionRecorded(resp) => resp.id,
        _ => panic!("expected ContradictionRecorded"),
    };
    assert_eq!(id, contradiction.id);

    let events = service
        .events
        .events_after(project_id, 0, 10)
        .unwrap();
    let contradiction_events: Vec<_> = events
        .iter()
        .filter(|e| {
            e.kind == autore_schema::domain::records::EVENT_KIND_CONTRADICTION_CREATED.clone()
        })
        .collect();
    assert_eq!(contradiction_events.len(), 1);
}

#[test]
fn add_verification_emits_event() {
    let (service, _tmp) = test_service();
    let project_id = create_project(&service, "verification-test");
    let entity_id = register_entity(&service, project_id);

    let record = VerificationRecord::new(
        project_id,
        VerificationSubject::Entity(entity_id),
        NamespacedId::parse("core.artifact.hash").unwrap(),
    );

    let result = service
        .execute(ApplicationCommand::AddVerification(AddVerificationRequest {
            project: project_id,
            record: record.clone(),
        }))
        .unwrap();
    let id = match result {
        CommandResult::VerificationAdded(resp) => resp.id,
        _ => panic!("expected VerificationAdded"),
    };
    assert_eq!(id, record.id);

    let events = service
        .events
        .events_after(project_id, 0, 10)
        .unwrap();
    let verification_events: Vec<_> = events
        .iter()
        .filter(|e| {
            e.kind == autore_schema::domain::records::EVENT_KIND_VERIFICATION_RECORDED.clone()
        })
        .collect();
    assert_eq!(verification_events.len(), 1);
}

fn test_client() -> (LocalAutoReClient, Arc<ApplicationService>, tempfile::TempDir) {
    let (service, tmp) = test_service();
    let service = Arc::new(service);
    let client = LocalAutoReClient::new(Arc::clone(&service));
    (client, service, tmp)
}

#[test]
fn local_client_routes_command() {
    let (client, _service, _tmp) = test_client();
    let result = client
        .execute(ApplicationCommand::CreateProject(CreateProjectRequest {
            name: "client-cmd-test".into(),
        }))
        .unwrap();
    match result {
        CommandResult::ProjectCreated(resp) => {
            assert_eq!(resp.project.name, "client-cmd-test");
        }
        _ => panic!("expected ProjectCreated"),
    }
}

#[test]
fn local_client_routes_query() {
    let (client, _service, _tmp) = test_client();
    let result = client
        .execute(ApplicationCommand::CreateProject(CreateProjectRequest {
            name: "client-query-test".into(),
        }))
        .unwrap();
    let project_id = match result {
        CommandResult::ProjectCreated(resp) => resp.project.id,
        _ => panic!("expected ProjectCreated"),
    };

    let query_result = client
        .query(ApplicationQuery::GetProjectSummary(GetProjectSummaryQuery {
            project: project_id,
        }))
        .unwrap();
    match query_result {
        QueryResult::ProjectSummary(resp) => {
            assert_eq!(resp.project.name, "client-query-test");
            assert_eq!(resp.project.id, project_id);
        }
        _ => panic!("expected ProjectSummary"),
    }
}

#[tokio::test]
async fn local_client_routes_subscription() {
    let (client, _service, _tmp) = test_client();
    let result = client
        .execute(ApplicationCommand::CreateProject(CreateProjectRequest {
            name: "client-sub-test".into(),
        }))
        .unwrap();
    let project_id = match result {
        CommandResult::ProjectCreated(resp) => resp.project.id,
        _ => panic!("expected ProjectCreated"),
    };

    let mut sub = client.subscribe_events(project_id, 0).unwrap();
    let event = sub.next().await.unwrap().unwrap();
    assert_eq!(event.project, project_id);
    assert_eq!(
        event.kind,
        autore_schema::domain::records::EVENT_KIND_PROJECT_CREATED.clone()
    );
    assert_eq!(event.sequence, 1);
}

#[test]
fn cross_project_reference_rejected() {
    let (client, _service, _tmp) = test_client();

    let project_a = match client
        .execute(ApplicationCommand::CreateProject(CreateProjectRequest {
            name: "project-a".into(),
        }))
        .unwrap()
    {
        CommandResult::ProjectCreated(resp) => resp.project.id,
        _ => panic!("expected ProjectCreated"),
    };

    let project_b = match client
        .execute(ApplicationCommand::CreateProject(CreateProjectRequest {
            name: "project-b".into(),
        }))
        .unwrap()
    {
        CommandResult::ProjectCreated(resp) => resp.project.id,
        _ => panic!("expected ProjectCreated"),
    };

    let record = EvidenceRecord {
        id: EvidenceRecordId::new(),
        project: project_b,
        subject: EntityId::new(),
        predicate: NamespacedId::parse("evidence.test").unwrap(),
        value: EvidenceValue::String("cross-project".into()),
        derivation: Derivation::new(
            DerivationMethod::DirectObservation,
            NamespacedId::parse("core.observe").unwrap(),
            vec![],
            vec![],
        ),
        provider_run: None,
        native_artifacts: vec![],
        assumptions: vec![],
        created_at: Timestamp::now(),
    };

    let result = client.execute(ApplicationCommand::AddEvidence(AddEvidenceRequest {
        project: project_a,
        record,
    }));

    assert!(result.is_err(), "cross-project evidence must be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("different project") || err.contains("Validation"),
        "unexpected error: {err}"
    );
}
