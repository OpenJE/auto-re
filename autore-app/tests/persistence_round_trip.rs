//! Task 36: Full Stage 0 persistence round-trip integration test.
//!
//! Creates a project containing every Stage 0 record type per §29.9, closes it,
//! reopens it via `autore_app::lifecycle::open_project`, and asserts semantic
//! equality for every record type by comparing serialize+deserialize round-trips.

use std::collections::BTreeMap;
use std::sync::Arc;

use autore_app::application_service::requests::*;
use autore_app::application_service::{
    ApplicationCommand, ApplicationQuery, ApplicationService, CommandResult, QueryResult,
};
use autore_app::{close_project, create_project, open_project};
use autore_core::operation::OperationState;
use autore_events::project_event_service::{EventBroadcaster, LocalProjectEventService};
use autore_schema::domain::Timestamp;
use autore_schema::domain::records::*;
use autore_schema::domain::{
    BinaryLocation, ContentHash, Derivation, DerivationMethod, EnvironmentIdentity, EvidenceValue,
    ExtensionData, ModuleIdentity, NamespacedId, StableEntityKey,
};
use autore_schema::ids::{
    BinaryArtifactId, EvidenceRecordId, NativeArtifactId, VerificationRecordId,
};
use autore_schema::manifest::ProjectManifest;
use autore_store::{
    Database, NativeArtifactStore, OperationStore, ProviderAliasStore, SqliteAliasStore,
    SqliteOperationStore,
};

/// Snapshot helper: serialize a record to JSON and return the string.
fn snapshot<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).expect("serialize snapshot")
}

/// Assert that two JSON strings are equal, providing a semantic round-trip check.
fn assert_json_eq(original: &str, reloaded: &str, label: &str) {
    assert_eq!(
        original, reloaded,
        "semantic round-trip failed for {label}\noriginal:\n{original}\nreloaded:\n{reloaded}"
    );
}

/// Build an `ApplicationService` backed by the database at `project_dir/project.sqlite3`.
fn service_for(project_parent: &std::path::Path) -> (ApplicationService, Arc<Database>) {
    let project_dir = project_parent.join("project.auto-re");
    let db =
        Arc::new(Database::open(project_dir.join("project.sqlite3")).expect("open project db"));
    let broadcaster = Arc::new(EventBroadcaster::new());
    let events = Arc::new(LocalProjectEventService::new(db.clone(), broadcaster));
    let service = ApplicationService::new(db.clone(), events, project_parent);
    (service, db)
}

#[test]
fn full_stage0_persistence_round_trip() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");

    // Step 1: create the project directory layout (manifest, empty DB, artifacts/).
    let _ = create_project(temp_dir.path(), "roundtrip-test").expect("create project layout");

    // Step 2: open the DB and create an application service.
    let (service, db) = service_for(temp_dir.path());

    // Step 3: insert the canonical project record via typed command and sync manifest.
    let project = match service
        .execute(ApplicationCommand::CreateProject(CreateProjectRequest {
            name: "roundtrip-test".into(),
        }))
        .expect("create project record")
    {
        CommandResult::ProjectCreated(resp) => resp.project,
        _ => panic!("expected ProjectCreated"),
    };
    let project_id = project.id;
    let project_dir = temp_dir.path().join("project.auto-re");
    let manifest_path = project_dir.join("project.toml");
    ProjectManifest::new(project.clone(), manifest_path.clone())
        .save(&manifest_path)
        .expect("save manifest with canonical project");
    let project_json = snapshot(&project);

    // Step 4: register one binary artifact.
    let source_path = temp_dir.path().join("binary.bin");
    std::fs::write(&source_path, b"fixture binary content").expect("write binary fixture");
    let artifact = match service
        .execute(ApplicationCommand::RegisterArtifact(
            RegisterArtifactRequest {
                project: project_id,
                source_path: source_path.clone(),
                kind: "core.binary".into(),
            },
        ))
        .expect("register artifact")
    {
        CommandResult::ArtifactRegistered(resp) => resp.artifact,
        _ => panic!("expected ArtifactRegistered"),
    };
    let artifact_id = artifact.id;
    let artifact_json = snapshot(&artifact);

    // Step 5: register two semantic entities.
    let module = ModuleIdentity::new(
        Some(".text".into()),
        ContentHash::sha256(b"module content"),
        Some(0),
    );
    let binary_location = StableEntityKey::BinaryLocation(BinaryLocation::new(
        BinaryArtifactId::new(),
        module,
        0x1000,
    ));
    let entity1 = match service
        .execute(ApplicationCommand::RegisterEntity(RegisterEntityRequest {
            project: project_id,
            kind: "core.function".into(),
            stable_key: Some(binary_location),
            display_name: Some("main".into()),
        }))
        .expect("register entity 1")
    {
        CommandResult::EntityRegistered(resp) => resp.entity,
        _ => panic!("expected EntityRegistered"),
    };
    let entity1_json = snapshot(&entity1);

    let entity2 = match service
        .execute(ApplicationCommand::RegisterEntity(RegisterEntityRequest {
            project: project_id,
            kind: "core.string".into(),
            stable_key: None,
            display_name: Some("hello world".into()),
        }))
        .expect("register entity 2")
    {
        CommandResult::EntityRegistered(resp) => resp.entity,
        _ => panic!("expected EntityRegistered"),
    };
    let entity2_json = snapshot(&entity2);

    // Step 6: register a provider and start a provider run.
    let provider = Provider::new(
        "test-disassembler",
        NamespacedId::parse("provider.disassembler").expect("provider kind"),
        "1.0.0",
    );
    let registered_provider = match service
        .execute(ApplicationCommand::RegisterProvider(
            RegisterProviderRequest {
                project: project_id,
                provider: provider.clone(),
            },
        ))
        .expect("register provider")
    {
        CommandResult::ProviderRegistered(resp) => resp.provider,
        _ => panic!("expected ProviderRegistered"),
    };
    let provider_id = registered_provider.id;
    let provider_json = snapshot(&registered_provider);

    let run = match service
        .execute(ApplicationCommand::StartProviderRun(
            StartProviderRunRequest {
                project: project_id,
                provider: provider_id,
                operation: "core.disassemble".into(),
                input_artifacts: vec![artifact_id],
                configuration_artifact: Some(artifact_id),
                configuration_hash: ContentHash::sha256(b"config"),
                environment: EnvironmentIdentity {
                    operating_system: NamespacedId::parse("core.linux").expect("os"),
                    architecture: NamespacedId::parse("core.x86-64").expect("arch"),
                    isolation_backend: Some(NamespacedId::parse("core.docker").expect("backend")),
                    image_digest: Some(ContentHash::sha256(b"image")),
                    extension: Some(ExtensionData::new(
                        NamespacedId::parse("core.env").expect("env schema"),
                        1,
                        serde_json::json!({"region": "us-east-1"}),
                    )),
                },
            },
        ))
        .expect("start provider run")
    {
        CommandResult::ProviderRunStarted(resp) => resp.run,
        _ => panic!("expected ProviderRunStarted"),
    };
    let run_id = run.id;
    let run_json = snapshot(&run);

    // Step 7: insert provider aliases directly (no typed command yet).
    let alias = ProviderEntityAlias {
        provider_run: run_id,
        provider_kind: PROVIDER_KIND_DISASSEMBLER.clone(),
        provider_identifier: "sub_401000".into(),
        entity: entity1.id,
    };
    SqliteAliasStore::new(&db)
        .insert_alias(&alias)
        .expect("insert provider alias");
    let alias_json = snapshot(&alias);

    // Step 8: insert a native artifact directly (no typed command yet).
    let native_artifact = NativeArtifact {
        id: NativeArtifactId::new(),
        provider_run: run_id,
        artifact: artifact_id,
        format: NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone(),
        subject_entities: vec![entity1.id],
        description: Some("decompiled main".into()),
    };
    SqliteAliasStore::new(&db)
        .insert(&native_artifact)
        .expect("insert native artifact");
    let native_artifact_id = native_artifact.id;
    let native_artifact_json = snapshot(&native_artifact);

    // Step 9: add several evidence records.
    let evidence1 = EvidenceRecord {
        id: EvidenceRecordId::new(),
        project: project_id,
        subject: entity1.id,
        predicate: EVIDENCE_PREDICATE_FUNCTION_NAME.clone(),
        value: EvidenceValue::String("main".into()),
        derivation: Derivation::new(
            DerivationMethod::ProviderAnalysis,
            NamespacedId::parse("core.disassemble").expect("operation"),
            vec![],
            vec![],
        ),
        provider_run: Some(run_id),
        native_artifacts: vec![native_artifact_id],
        assumptions: vec![Assumption {
            description: "binary is stripped".into(),
            evidence: None,
        }],
        created_at: Timestamp::now(),
    };
    match service
        .execute(ApplicationCommand::AddEvidence(AddEvidenceRequest {
            project: project_id,
            record: evidence1.clone(),
        }))
        .expect("add evidence 1")
    {
        CommandResult::EvidenceAdded(resp) => assert_eq!(resp.id, evidence1.id),
        _ => panic!("expected EvidenceAdded"),
    };
    let evidence1_json = snapshot(&evidence1);

    let evidence2 = EvidenceRecord {
        id: EvidenceRecordId::new(),
        project: project_id,
        subject: entity2.id,
        predicate: EVIDENCE_PREDICATE_STRING_REFERENCE.clone(),
        value: EvidenceValue::String("hello world".into()),
        derivation: Derivation::new(
            DerivationMethod::DirectObservation,
            NamespacedId::parse("core.observe").expect("operation"),
            vec![],
            vec![],
        ),
        provider_run: None,
        native_artifacts: vec![],
        assumptions: vec![],
        created_at: Timestamp::now(),
    };
    match service
        .execute(ApplicationCommand::AddEvidence(AddEvidenceRequest {
            project: project_id,
            record: evidence2.clone(),
        }))
        .expect("add evidence 2")
    {
        CommandResult::EvidenceAdded(resp) => assert_eq!(resp.id, evidence2.id),
        _ => panic!("expected EvidenceAdded"),
    };
    let evidence2_json = snapshot(&evidence2);

    // Step 10: add two competing hypotheses.
    let hypothesis1_id = match service
        .execute(ApplicationCommand::AddHypothesis(AddHypothesisRequest {
            project: project_id,
            subject: entity1.id,
            predicate: "hypothesis.name".into(),
            candidate: EvidenceValue::String("entry_point".into()),
            confidence_score: 0.75,
            confidence_rationale: Some("multiple providers agree".into()),
            supporting_evidence: vec![evidence1.id],
            contradicting_evidence: vec![],
            derived_from: vec![],
            status: HypothesisStatus::UnderInvestigation,
        }))
        .expect("add hypothesis 1")
    {
        CommandResult::HypothesisAdded(resp) => resp.id,
        _ => panic!("expected HypothesisAdded"),
    };

    // Accept hypothesis 1 so it becomes a terminal competitor.
    let hypothesis1 = match service
        .execute(ApplicationCommand::ChangeHypothesisStatus(
            ChangeHypothesisStatusRequest {
                project: project_id,
                id: hypothesis1_id,
                status: HypothesisStatus::Accepted,
            },
        ))
        .expect("accept hypothesis 1")
    {
        CommandResult::HypothesisStatusChanged(resp) => resp.hypothesis,
        _ => panic!("expected HypothesisStatusChanged"),
    };
    let hypothesis1_json = snapshot(&hypothesis1);

    let hypothesis2_id = match service
        .execute(ApplicationCommand::AddHypothesis(AddHypothesisRequest {
            project: project_id,
            subject: entity1.id,
            predicate: "hypothesis.name".into(),
            candidate: EvidenceValue::String("_start".into()),
            confidence_score: 0.4,
            confidence_rationale: Some("weaker signal".into()),
            supporting_evidence: vec![],
            contradicting_evidence: vec![evidence1.id],
            derived_from: vec![],
            status: HypothesisStatus::UnderInvestigation,
        }))
        .expect("add hypothesis 2")
    {
        CommandResult::HypothesisAdded(resp) => resp.id,
        _ => panic!("expected HypothesisAdded"),
    };
    let hypothesis2 = match service
        .query(ApplicationQuery::GetHypothesis(GetHypothesisQuery {
            id: hypothesis2_id,
        }))
        .expect("get hypothesis 2")
    {
        QueryResult::Hypothesis(resp) => resp.hypothesis,
        _ => panic!("expected Hypothesis"),
    };
    let hypothesis2_json = snapshot(&hypothesis2);

    // Step 11: record a contradiction between the competing hypotheses.
    let contradiction = Contradiction::new(
        project_id,
        entity1.id,
        NamespacedId::parse("hypothesis.name").expect("predicate"),
        vec![evidence1.id, evidence2.id],
        vec![hypothesis1_id, hypothesis2_id],
    );
    match service
        .execute(ApplicationCommand::RecordContradiction(
            RecordContradictionRequest {
                project: project_id,
                contradiction: contradiction.clone(),
            },
        ))
        .expect("record contradiction")
    {
        CommandResult::ContradictionRecorded(resp) => assert_eq!(resp.id, contradiction.id),
        _ => panic!("expected ContradictionRecorded"),
    };
    let contradiction_json = snapshot(&contradiction);

    // Step 12: add several verification records.
    let verification1 = VerificationRecord::new(
        project_id,
        VerificationSubject::Entity(entity1.id),
        VERIFICATION_CHECK_ARTIFACT_HASH.clone(),
    );
    match service
        .execute(ApplicationCommand::AddVerification(
            AddVerificationRequest {
                project: project_id,
                record: verification1.clone(),
            },
        ))
        .expect("add verification 1")
    {
        CommandResult::VerificationAdded(resp) => assert_eq!(resp.id, verification1.id),
        _ => panic!("expected VerificationAdded"),
    };
    let verification1_json = snapshot(&verification1);

    let verification2 = VerificationRecord {
        id: VerificationRecordId::new(),
        project: project_id,
        subject: VerificationSubject::Hypothesis(hypothesis1_id),
        check: VERIFICATION_CHECK_BUILD.clone(),
        state: VerificationState::Pending,
        provider_run: Some(run_id),
        evidence: vec![evidence1.id],
        details: Some(ExtensionData::new(
            NamespacedId::parse("core.verification.detail").expect("detail schema"),
            1,
            serde_json::json!({"build": "succeeded"}),
        )),
        created_at: Timestamp::now(),
    };
    match service
        .execute(ApplicationCommand::AddVerification(
            AddVerificationRequest {
                project: project_id,
                record: verification2.clone(),
            },
        ))
        .expect("add verification 2")
    {
        CommandResult::VerificationAdded(resp) => assert_eq!(resp.id, verification2.id),
        _ => panic!("expected VerificationAdded"),
    };
    let verification2_json = snapshot(&verification2);

    let verification3 = VerificationRecord {
        id: VerificationRecordId::new(),
        project: project_id,
        subject: VerificationSubject::Artifact(artifact_id),
        check: VERIFICATION_CHECK_PROJECT_INTEGRITY.clone(),
        state: VerificationState::Passed,
        provider_run: None,
        evidence: vec![],
        details: None,
        created_at: Timestamp::now(),
    };
    match service
        .execute(ApplicationCommand::AddVerification(
            AddVerificationRequest {
                project: project_id,
                record: verification3.clone(),
            },
        ))
        .expect("add verification 3")
    {
        CommandResult::VerificationAdded(resp) => assert_eq!(resp.id, verification3.id),
        _ => panic!("expected VerificationAdded"),
    };
    let verification3_json = snapshot(&verification3);

    // Step 13: create an operation and progress records, then complete it.
    let operation = match service
        .execute(ApplicationCommand::RebuildIndexes(RebuildIndexesRequest {
            project: project_id,
        }))
        .expect("rebuild indexes")
    {
        CommandResult::IndexesRebuilt(resp) => resp.operation,
        _ => panic!("expected IndexesRebuilt"),
    };
    let operation_id = operation.id;

    let progress1 = ProgressUpdate::new(operation_id, 0, "started", BTreeMap::new());
    SqliteOperationStore::new(&db)
        .record_progress(&progress1)
        .expect("record progress 1");
    let progress1_json = snapshot(&progress1);

    let mut metrics = BTreeMap::new();
    metrics.insert(
        NamespacedId::parse("core.progress.percent").expect("metric"),
        0.5,
    );
    let progress2 = ProgressUpdate::new(operation_id, 1, "halfway", metrics);
    SqliteOperationStore::new(&db)
        .record_progress(&progress2)
        .expect("record progress 2");
    let progress2_json = snapshot(&progress2);

    SqliteOperationStore::new(&db)
        .transition(operation_id, OperationState::Running, None)
        .expect("start operation");
    SqliteOperationStore::new(&db)
        .transition(operation_id, OperationState::Completed, None)
        .expect("complete operation");
    let operation = match service
        .query(ApplicationQuery::GetOperation(GetOperationQuery {
            id: operation_id,
        }))
        .expect("get operation")
    {
        QueryResult::Operation(resp) => resp.operation,
        _ => panic!("expected Operation"),
    };
    let operation_json = snapshot(&operation);

    // Step 14: snapshot project events.
    let events = match service
        .query(ApplicationQuery::ListEvents(ListEventsQuery {
            project: project_id,
            after_sequence: 0,
            limit: 100,
        }))
        .expect("list events")
    {
        QueryResult::Events(resp) => resp.events,
        _ => panic!("expected Events"),
    };
    assert!(!events.is_empty(), "expected project events");
    let events_json = snapshot(&events);

    // -------------------------------------------------------------------------
    // Step 15: close the project (drop service/DB handles) and reopen.
    // -------------------------------------------------------------------------
    drop(service);
    drop(db);
    let mut project_for_close = project.clone();
    close_project(&mut project_for_close);

    let reopened_project = open_project(temp_dir.path()).expect("reopen project");
    assert_eq!(reopened_project.id, project_id);

    // Step 16: rebuild the service against the reopened database.
    let (service, db) = service_for(temp_dir.path());

    // Step 17: reload every record type and assert semantic equality.
    let reloaded_project = match service
        .query(ApplicationQuery::GetProjectSummary(
            GetProjectSummaryQuery {
                project: project_id,
            },
        ))
        .expect("get project summary")
    {
        QueryResult::ProjectSummary(resp) => resp.project,
        _ => panic!("expected ProjectSummary"),
    };
    assert_json_eq(&project_json, &snapshot(&reloaded_project), "Project");

    let reloaded_artifact = match service
        .query(ApplicationQuery::GetArtifact(GetArtifactQuery {
            id: artifact_id,
        }))
        .expect("get artifact")
    {
        QueryResult::Artifact(resp) => resp.artifact,
        _ => panic!("expected Artifact"),
    };
    assert_json_eq(&artifact_json, &snapshot(&reloaded_artifact), "Artifact");

    let reloaded_entity1 = match service
        .query(ApplicationQuery::GetEntity(GetEntityQuery {
            id: entity1.id,
        }))
        .expect("get entity 1")
    {
        QueryResult::Entity(resp) => resp.entity,
        _ => panic!("expected Entity"),
    };
    assert_json_eq(&entity1_json, &snapshot(&reloaded_entity1), "Entity 1");

    let reloaded_entity2 = match service
        .query(ApplicationQuery::GetEntity(GetEntityQuery {
            id: entity2.id,
        }))
        .expect("get entity 2")
    {
        QueryResult::Entity(resp) => resp.entity,
        _ => panic!("expected Entity"),
    };
    assert_json_eq(&entity2_json, &snapshot(&reloaded_entity2), "Entity 2");

    let reloaded_provider = match service
        .query(ApplicationQuery::GetProvider(GetProviderQuery {
            id: provider_id,
        }))
        .expect("get provider")
    {
        QueryResult::Provider(resp) => resp.provider,
        _ => panic!("expected Provider"),
    };
    assert_json_eq(&provider_json, &snapshot(&reloaded_provider), "Provider");

    let reloaded_run = match service
        .query(ApplicationQuery::GetProviderRun(GetProviderRunQuery {
            id: run_id,
        }))
        .expect("get provider run")
    {
        QueryResult::ProviderRun(resp) => resp.run,
        _ => panic!("expected ProviderRun"),
    };
    assert_json_eq(&run_json, &snapshot(&reloaded_run), "ProviderRun");

    let reloaded_aliases = SqliteAliasStore::new(&db)
        .list_aliases_for_run(run_id)
        .expect("list aliases");
    assert_eq!(reloaded_aliases.len(), 1, "expected one provider alias");
    assert_json_eq(
        &alias_json,
        &snapshot(&reloaded_aliases[0]),
        "ProviderEntityAlias",
    );

    let reloaded_native = SqliteAliasStore::new(&db)
        .get(native_artifact_id)
        .expect("get native artifact")
        .expect("native artifact exists");
    assert_json_eq(
        &native_artifact_json,
        &snapshot(&reloaded_native),
        "NativeArtifact",
    );

    let reloaded_evidence1 = match service
        .query(ApplicationQuery::GetEvidence(GetEvidenceQuery {
            id: evidence1.id,
        }))
        .expect("get evidence 1")
    {
        QueryResult::Evidence(resp) => resp.record,
        _ => panic!("expected Evidence"),
    };
    assert_json_eq(
        &evidence1_json,
        &snapshot(&reloaded_evidence1),
        "EvidenceRecord 1",
    );

    let reloaded_evidence2 = match service
        .query(ApplicationQuery::GetEvidence(GetEvidenceQuery {
            id: evidence2.id,
        }))
        .expect("get evidence 2")
    {
        QueryResult::Evidence(resp) => resp.record,
        _ => panic!("expected Evidence"),
    };
    assert_json_eq(
        &evidence2_json,
        &snapshot(&reloaded_evidence2),
        "EvidenceRecord 2",
    );

    let reloaded_hypothesis1 = match service
        .query(ApplicationQuery::GetHypothesis(GetHypothesisQuery {
            id: hypothesis1_id,
        }))
        .expect("get hypothesis 1")
    {
        QueryResult::Hypothesis(resp) => resp.hypothesis,
        _ => panic!("expected Hypothesis"),
    };
    assert_json_eq(
        &hypothesis1_json,
        &snapshot(&reloaded_hypothesis1),
        "Hypothesis 1",
    );

    let reloaded_hypothesis2 = match service
        .query(ApplicationQuery::GetHypothesis(GetHypothesisQuery {
            id: hypothesis2_id,
        }))
        .expect("get hypothesis 2")
    {
        QueryResult::Hypothesis(resp) => resp.hypothesis,
        _ => panic!("expected Hypothesis"),
    };
    assert_json_eq(
        &hypothesis2_json,
        &snapshot(&reloaded_hypothesis2),
        "Hypothesis 2",
    );

    let reloaded_contradiction = match service
        .query(ApplicationQuery::GetContradiction(GetContradictionQuery {
            id: contradiction.id,
        }))
        .expect("get contradiction")
    {
        QueryResult::Contradiction(resp) => resp.contradiction,
        _ => panic!("expected Contradiction"),
    };
    assert_json_eq(
        &contradiction_json,
        &snapshot(&reloaded_contradiction),
        "Contradiction",
    );

    let reloaded_verification1 = match service
        .query(ApplicationQuery::GetVerification(GetVerificationQuery {
            id: verification1.id,
        }))
        .expect("get verification 1")
    {
        QueryResult::Verification(resp) => resp.record,
        _ => panic!("expected Verification"),
    };
    assert_json_eq(
        &verification1_json,
        &snapshot(&reloaded_verification1),
        "VerificationRecord 1",
    );

    let reloaded_verification2 = match service
        .query(ApplicationQuery::GetVerification(GetVerificationQuery {
            id: verification2.id,
        }))
        .expect("get verification 2")
    {
        QueryResult::Verification(resp) => resp.record,
        _ => panic!("expected Verification"),
    };
    assert_json_eq(
        &verification2_json,
        &snapshot(&reloaded_verification2),
        "VerificationRecord 2",
    );

    let reloaded_verification3 = match service
        .query(ApplicationQuery::GetVerification(GetVerificationQuery {
            id: verification3.id,
        }))
        .expect("get verification 3")
    {
        QueryResult::Verification(resp) => resp.record,
        _ => panic!("expected Verification"),
    };
    assert_json_eq(
        &verification3_json,
        &snapshot(&reloaded_verification3),
        "VerificationRecord 3",
    );

    let reloaded_operation = match service
        .query(ApplicationQuery::GetOperation(GetOperationQuery {
            id: operation_id,
        }))
        .expect("get operation")
    {
        QueryResult::Operation(resp) => resp.operation,
        _ => panic!("expected Operation"),
    };
    assert_json_eq(&operation_json, &snapshot(&reloaded_operation), "Operation");

    let reloaded_progress = SqliteOperationStore::new(&db)
        .list_progress(operation_id)
        .expect("list progress");
    assert_eq!(reloaded_progress.len(), 2, "expected two progress updates");
    assert_json_eq(
        &progress1_json,
        &snapshot(&reloaded_progress[0]),
        "ProgressUpdate 1",
    );
    assert_json_eq(
        &progress2_json,
        &snapshot(&reloaded_progress[1]),
        "ProgressUpdate 2",
    );

    let reloaded_events = match service
        .query(ApplicationQuery::ListEvents(ListEventsQuery {
            project: project_id,
            after_sequence: 0,
            limit: 100,
        }))
        .expect("list events after reopen")
    {
        QueryResult::Events(resp) => resp.events,
        _ => panic!("expected Events"),
    };
    assert_json_eq(
        &events_json,
        &snapshot(&reloaded_events),
        "ProjectEvent list",
    );
}
