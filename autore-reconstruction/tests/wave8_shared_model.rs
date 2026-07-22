//! Wave 8 exit-criterion integration test: shared type/class coherent
//! evolving model end-to-end.
//!
//! Exercises the deterministic layout reconciler, LLM-driven conflict
//! arbitration, declaration generation, and mock build verification for
//! shared canonical type recovery.
//!
//! Flow:
//!   1. Register two type entities (A, B) and one function entity.
//!   2. Build a skeleton with `ProjectSkeletonBuilder` (declarations + stubs).
//!   3. Issue `AddEvidence` commands carrying `LayoutConstraint` JSON.
//!   4. Run `Reconciler::reconcile`:
//!        - Type A (compatible layout) -> `AddHypothesis`.
//!        - Type B (conflicting sizes) -> `CreateWorkItems` with
//!          `ConflictResolution:` description.
//!   5. Dispatch `llm.analysis.conflict` via a mock `ConflictLlm` to resolve
//!      the Type B conflict with a `supersede` decision.
//!   6. Assert `AcceptHypothesisPolicyDriven` + `InvalidateGeneratedSource`
//!      commands are emitted.
//!   7. Mark the superseding Type B hypothesis as `Accepted` and run
//!      `DeclarationGenerator::generate_accepted_types` for Type A and the
//!      accepted Type B layout.
//!   8. Assert the generated `include/recovered/<entity>.hpp` files reflect
//!      the accepted layouts (fields + padding).
//!   9. Run the mock `DockerMsvc2002BuildProvider` to confirm the skeleton
//!      still builds green (only declarations changed, function body stubs
//!      remain tamper-not-modified).
//!  10. Audit that every canonical mutation flowed through an
//!      `ApplicationCommand` variant.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::path::PathBuf;
use std::sync::Mutex;

use tests_support::RecordingAutoReClient;

use autore_app::application_service::requests::{
    AddEvidenceRequest, RecordBuildAttemptRequest, RecordBuildAttemptResponse,
    RegisterEntityRequest,
};
use autore_app::{ApplicationCommand, AutoReClient, CommandResult};
use autore_core::Result;
use autore_events::project_event_service::ProjectEventSubscription;
use autore_reconstruction::build::{
    BuildProviderTrait, CompileUnit, DockerMsvc2002BuildProvider, DockerMsvc2002Config,
    GeneratorManifest,
};
use autore_reconstruction::generation::{ProjectSkeletonBuilder, StubPolicy};
use autore_reconstruction::types::conflict::{ConflictArbitrator, ConflictLlm};
use autore_reconstruction::types::constraint::LayoutConstraintStore;
use autore_reconstruction::types::declaration_gen::{DeclarationGenerator, entity_to_source_path};
use autore_reconstruction::types::{
    CONFLICT_RESOLUTION_PREFIX, LAYOUT_HYPOTHESIS_PREDICATE, LayoutConstraint,
    LayoutConstraintKind, ReconciledLayout, Reconciler,
};
use autore_schema::domain::records::{
    CanonicalTypeHypothesis, ENTITY_KIND_FUNCTION, ENTITY_KIND_TYPE, GeneratedSourceMapping,
    Hypothesis, HypothesisStatus, PolicyDecision,
};
use autore_schema::domain::records::{ProjectEvent, SemanticEntity};
use autore_schema::domain::{Confidence, EvidenceValue, NamespacedId, Timestamp};
use autore_schema::ids::{
    ArtifactId, EntityId, GeneratedSourceMappingId, HypothesisId, ProjectId,
    ReconstructionCampaignId, WorkItemId,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mock_docker_success_path() -> String {
    std::env::var("AUTORE_TEST_MOCK_DOCKER").unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mock-docker-success.sh")
            .to_string_lossy()
            .into_owned()
    })
}

fn register_entity(
    client: &dyn AutoReClient,
    project: ProjectId,
    kind: NamespacedId,
    display_name: &str,
) -> EntityId {
    let req = RegisterEntityRequest {
        project,
        kind: kind.to_string(),
        stable_key: None,
        display_name: Some(display_name.into()),
    };
    match client
        .execute(ApplicationCommand::RegisterEntity(req))
        .expect("RegisterEntity must succeed")
    {
        CommandResult::EntityRegistered(resp) => resp.entity.id,
        other => panic!("expected EntityRegistered, got {other:?}"),
    }
}

fn entity_relpath(entity_id: EntityId) -> PathBuf {
    let hex = entity_id.as_uuid().as_simple().to_string();
    PathBuf::from(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex[4..6])
        .join(&hex)
}

fn cpp_relpath(entity_id: EntityId) -> PathBuf {
    PathBuf::from("src/generated")
        .join(entity_relpath(entity_id))
        .with_extension("cpp")
}

fn add_evidence_for_constraint(
    client: &dyn AutoReClient,
    project: ProjectId,
    subject: EntityId,
    constraint: LayoutConstraint,
) {
    let mut store = LayoutConstraintStore::new();
    store.add(constraint);
    let record = store.to_evidence_record(project, subject);
    client
        .execute(ApplicationCommand::AddEvidence(AddEvidenceRequest {
            project,
            record,
        }))
        .expect("AddEvidence must succeed");
}

fn make_hypothesis(
    id: HypothesisId,
    project: ProjectId,
    subject: EntityId,
    layout: &ReconciledLayout,
) -> Hypothesis {
    Hypothesis {
        id,
        project,
        subject,
        predicate: NamespacedId::parse(LAYOUT_HYPOTHESIS_PREDICATE).unwrap(),
        candidate: EvidenceValue::String(serde_json::to_string(layout).expect("layout serializes")),
        supporting_evidence: vec![],
        contradicting_evidence: vec![],
        derived_from: vec![],
        confidence: Confidence::new(1.0).expect("1.0 is valid"),
        status: HypothesisStatus::Proposed,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    }
}

fn make_canonical_hypothesis(
    project: ProjectId,
    entity_id: EntityId,
    layout: &ReconciledLayout,
) -> CanonicalTypeHypothesis {
    let mut h = CanonicalTypeHypothesis::new(
        project,
        entity_id,
        serde_json::to_string(layout).expect("layout serializes"),
    );
    h.confidence = 1.0;
    h.verified_size = true;
    h.verified_field_offsets.insert("0".into(), true);
    h.status = HypothesisStatus::Accepted;
    h
}

fn make_generated_source_mapping(
    campaign: ReconstructionCampaignId,
    target_entity: EntityId,
) -> GeneratedSourceMapping {
    GeneratedSourceMapping {
        id: GeneratedSourceMappingId::new(),
        campaign,
        generated_artifact: ArtifactId::new(),
        target_entity,
        produced_by: WorkItemId::new(),
        mapping_kind: NamespacedId::parse("mapping.type").unwrap(),
        created_at: Timestamp::now(),
    }
}

fn success_build_provider() -> DockerMsvc2002BuildProvider {
    DockerMsvc2002BuildProvider::new(DockerMsvc2002Config {
        image_name: "msvc2002-build:test".into(),
        cmake_generator: "NMake Makefiles".into(),
        toolchain_path: PathBuf::from("/opt/msvc2002"),
        docker_binary: Some(mock_docker_success_path()),
    })
}

// Wrapper that extends RecordingAutoReClient with RecordBuildAttempt handling.
struct BuildAwareClient {
    inner: RecordingAutoReClient,
    build_commands: Mutex<Vec<ApplicationCommand>>,
}

impl BuildAwareClient {
    fn new() -> Self {
        Self {
            inner: RecordingAutoReClient::new(),
            build_commands: Mutex::new(Vec::new()),
        }
    }

    fn commands(&self) -> Vec<ApplicationCommand> {
        let mut cmds = self.inner.commands();
        cmds.extend(self.build_commands.lock().unwrap().iter().cloned());
        cmds
    }
}

impl AutoReClient for BuildAwareClient {
    fn execute(&self, command: ApplicationCommand) -> Result<CommandResult> {
        if let ApplicationCommand::RecordBuildAttempt(_req) = &command {
            let result = CommandResult::BuildAttemptRecorded(RecordBuildAttemptResponse {
                attempt_id: uuid::Uuid::now_v7().to_string(),
            });
            self.build_commands.lock().unwrap().push(command);
            return Ok(result);
        }
        self.inner.execute(command)
    }

    fn query(&self, query: autore_app::ApplicationQuery) -> Result<autore_app::QueryResult> {
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

    fn subscribe_events(&self, project: ProjectId, after: u64) -> Result<ProjectEventSubscription> {
        self.inner.subscribe_events(project, after)
    }
}

struct MockConflictLlm {
    response: serde_json::Value,
}

impl ConflictLlm for MockConflictLlm {
    fn analyze_conflict(
        &self,
        _bundle: &autore_reconstruction::analysis::bundle::InvestigationBundle,
    ) -> Result<serde_json::Value, autore_reconstruction::types::conflict::ConflictError> {
        Ok(self.response.clone())
    }
}

// ---------------------------------------------------------------------------
// Wave 8 shared-model end-to-end test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn wave8_shared_model_end_to_end() {
    eprintln!("[wave8_shared_model] bootstrapping project + campaign + entities");

    let tmp = tempfile::tempdir().expect("temp dir");
    let project = ProjectId::new();
    let campaign = ReconstructionCampaignId::new();
    let campaign_id_str = campaign.to_string();
    let client = BuildAwareClient::new();

    // 1. Register canonical entities through ApplicationCommand.
    let entity_type_a = register_entity(&client, project, ENTITY_KIND_TYPE.clone(), "TypeA");
    let entity_type_b = register_entity(&client, project, ENTITY_KIND_TYPE.clone(), "TypeB");
    let entity_function =
        register_entity(&client, project, ENTITY_KIND_FUNCTION.clone(), "use_types");

    eprintln!(
        "[wave8_shared_model] entities: A={entity_type_a}, B={entity_type_b}, fn={entity_function}"
    );

    // 2. Build deterministic project skeleton (declarations + function stubs).
    let mut skeleton_builder =
        ProjectSkeletonBuilder::new(tmp.path().to_path_buf(), project, &client)
            .with_policy(StubPolicy::EmptyBody);
    let entity_a_stub = SemanticEntity::new(
        project,
        ENTITY_KIND_TYPE.clone(),
        None,
        Some("TypeA".into()),
    );
    let entity_b_stub = SemanticEntity::new(
        project,
        ENTITY_KIND_TYPE.clone(),
        None,
        Some("TypeB".into()),
    );
    let entity_fn_stub = SemanticEntity::new(
        project,
        ENTITY_KIND_FUNCTION.clone(),
        None,
        Some("use_types".into()),
    );

    // Use the registered IDs for skeleton generation so paths line up.
    let mut entity_a_stub = entity_a_stub;
    let mut entity_b_stub = entity_b_stub;
    let mut entity_fn_stub = entity_fn_stub;
    entity_a_stub.id = entity_type_a;
    entity_b_stub.id = entity_type_b;
    entity_fn_stub.id = entity_function;

    skeleton_builder.add_entity(&entity_a_stub);
    skeleton_builder.add_entity(&entity_b_stub);
    skeleton_builder.add_entity(&entity_fn_stub);

    let manifest = skeleton_builder
        .build()
        .expect("skeleton build must succeed");
    assert_eq!(manifest.entity_count, 3);

    // Capture the function stub content before declaration generation so we
    // can assert it is tamper-not-modified later.
    let fn_cpp_path = tmp.path().join(cpp_relpath(entity_function));
    let fn_cpp_original = std::fs::read_to_string(&fn_cpp_path)
        .expect("function cpp stub must exist after skeleton build");

    // 3. Build layout constraints and issue AddEvidence commands for each.
    let constraints_a = vec![
        LayoutConstraint::new(LayoutConstraintKind::ObjectAllocationSize {
            entity: entity_type_a,
            size_bytes: 16,
        }),
        LayoutConstraint::new(LayoutConstraintKind::FieldObservedAtOffset {
            entity: entity_type_a,
            offset: 0,
        }),
        LayoutConstraint::new(LayoutConstraintKind::ReadWidth {
            entity: entity_type_a,
            offset: 0,
            width_bytes: 4,
        }),
    ];

    let constraints_b = vec![
        LayoutConstraint::new(LayoutConstraintKind::ObjectAllocationSize {
            entity: entity_type_b,
            size_bytes: 16,
        }),
        LayoutConstraint::new(LayoutConstraintKind::ObjectAllocationSize {
            entity: entity_type_b,
            size_bytes: 32,
        }),
        LayoutConstraint::new(LayoutConstraintKind::FieldObservedAtOffset {
            entity: entity_type_b,
            offset: 0,
        }),
        LayoutConstraint::new(LayoutConstraintKind::ReadWidth {
            entity: entity_type_b,
            offset: 0,
            width_bytes: 4,
        }),
    ];

    for c in &constraints_a {
        add_evidence_for_constraint(&client, project, entity_type_a, c.clone());
    }
    for c in &constraints_b {
        add_evidence_for_constraint(&client, project, entity_type_b, c.clone());
    }

    eprintln!("[wave8_shared_model] issued AddEvidence for all layout constraints");

    // 4. Run the deterministic reconciler.
    let all_constraints: Vec<LayoutConstraint> = constraints_a
        .iter()
        .chain(constraints_b.iter())
        .cloned()
        .collect();
    let reconciler = Reconciler::new(&client, project, campaign_id_str.clone());
    let reconciled_layouts = reconciler.reconcile(&all_constraints).expect("reconcile");

    // Assert Type A produced an accepted-layout hypothesis and Type B produced
    // a conflict-resolution work item.
    let add_hypotheses: Vec<_> = client
        .commands()
        .iter()
        .filter(|c| matches!(c, ApplicationCommand::AddHypothesis(_)))
        .cloned()
        .collect();
    let conflict_work_items: Vec<_> = client
        .commands()
        .iter()
        .filter(|c| {
            matches!(c, ApplicationCommand::CreateWorkItems(req)
            if req.descriptions.iter().any(|d| d.starts_with(CONFLICT_RESOLUTION_PREFIX)))
        })
        .cloned()
        .collect();

    assert_eq!(
        add_hypotheses.len(),
        1,
        "exactly one AddHypothesis expected for Type A"
    );
    assert_eq!(
        conflict_work_items.len(),
        1,
        "exactly one ConflictResolution CreateWorkItems expected for Type B"
    );

    let type_a_layout = reconciled_layouts
        .into_iter()
        .find(|l| l.entity_id == entity_type_a)
        .expect("Type A layout must be reconciled");
    assert_eq!(type_a_layout.computed_size_bytes, Some(16));
    assert!(
        type_a_layout
            .fields
            .iter()
            .any(|f| f.offset == 0 && f.width_bytes == Some(4))
    );

    if let ApplicationCommand::AddHypothesis(req) = &add_hypotheses[0] {
        assert_eq!(req.subject, entity_type_a);
        assert_eq!(req.predicate, LAYOUT_HYPOTHESIS_PREDICATE);
        assert_eq!(req.confidence_score, 1.0);
    } else {
        panic!("expected AddHypothesis");
    }

    if let ApplicationCommand::CreateWorkItems(req) = &conflict_work_items[0] {
        assert!(req.descriptions[0].contains(&entity_type_b.to_string()));
    } else {
        panic!("expected CreateWorkItems");
    }

    eprintln!(
        "[wave8_shared_model] reconciler: Type A hypothesis accepted, Type B conflict queued"
    );

    // 5. Construct conflicting Type B hypotheses and a generated-source mapping.
    let layout_b_small = ReconciledLayout {
        entity_id: entity_type_b,
        computed_size_bytes: Some(16),
        computed_alignment: None,
        fields: vec![autore_reconstruction::types::ReconciledField {
            offset: 0,
            width_bytes: Some(4),
        }],
        vtable_slot_targets: vec![],
        base_adjustments: vec![],
        array_stride: None,
        parameter_usages: vec![],
        return_value_use: None,
        source_constraints: vec![],
    };
    let layout_b_large = ReconciledLayout {
        entity_id: entity_type_b,
        computed_size_bytes: Some(32),
        computed_alignment: None,
        fields: vec![autore_reconstruction::types::ReconciledField {
            offset: 0,
            width_bytes: Some(4),
        }],
        vtable_slot_targets: vec![],
        base_adjustments: vec![],
        array_stride: None,
        parameter_usages: vec![],
        return_value_use: None,
        source_constraints: vec![],
    };

    let hyp_b_small_id = HypothesisId::new();
    let hyp_b_large_id = HypothesisId::new();
    let hyp_b_small = make_hypothesis(hyp_b_small_id, project, entity_type_b, &layout_b_small);
    let hyp_b_large = make_hypothesis(hyp_b_large_id, project, entity_type_b, &layout_b_large);
    let hypotheses_b = vec![hyp_b_small, hyp_b_large];

    let mapping_b = make_generated_source_mapping(campaign, entity_type_b);
    let mappings = vec![mapping_b];

    // 6. Dispatch mock LLM conflict analysis with a "supersede" resolution.
    let mock_response = serde_json::json!({
        "resolution_kind": "supersede",
        "accepted_hypothesis_id": hyp_b_large_id.to_string(),
        "rejected_hypothesis_ids": [hyp_b_small_id.to_string()],
        "rationale": "larger layout explains observed 32-byte allocations",
        "evidence_references": ["evidence-b-size-32"],
        "confidence": 0.92
    });
    let llm = MockConflictLlm {
        response: mock_response,
    };

    let arbitrator = ConflictArbitrator::new();
    let resolution_commands = arbitrator
        .arbitrate(
            project,
            entity_type_b,
            &hypotheses_b,
            &constraints_b,
            &[],
            &mappings,
            &llm,
        )
        .expect("conflict arbitration must succeed");

    // 7. Assert policy-driven acceptance + invalidation of affected sources.
    assert_eq!(
        resolution_commands.len(),
        2,
        "supersede must emit AcceptHypothesisPolicyDriven + InvalidateGeneratedSource"
    );
    let ApplicationCommand::AcceptHypothesisPolicyDriven(accept_req) = &resolution_commands[0]
    else {
        panic!(
            "expected AcceptHypothesisPolicyDriven, got {:?}",
            resolution_commands[0]
        );
    };
    assert_eq!(accept_req.hypothesis_id, hyp_b_small_id);
    assert_eq!(accept_req.policy_decision, PolicyDecision::Supersede);
    assert_eq!(accept_req.superseding_hypothesis_id, Some(hyp_b_large_id));
    assert!(!accept_req.justification.is_empty());

    let ApplicationCommand::InvalidateGeneratedSource(inv_req) = &resolution_commands[1] else {
        panic!(
            "expected InvalidateGeneratedSource, got {:?}",
            resolution_commands[1]
        );
    };
    assert_eq!(inv_req.mapping_id, mappings[0].id.to_string());

    // Execute the arbitrator commands through the client so they are recorded
    // as canonical mutations.
    for cmd in resolution_commands {
        client
            .execute(cmd)
            .expect("resolution command must succeed");
    }

    eprintln!("[wave8_shared_model] conflict arbitration: supersede Type B small -> large");

    // 8. Mark the superseding Type B hypothesis as Accepted (policy-driven
    //    acceptance handler simulation) and generate declarations.
    let accepted_type_b_hyp = make_canonical_hypothesis(project, entity_type_b, &layout_b_large);
    let accepted_type_a_hyp = make_canonical_hypothesis(project, entity_type_a, &type_a_layout);

    let generator = DeclarationGenerator::new(
        project,
        campaign_id_str.clone(),
        tmp.path().to_path_buf(),
        &client,
    );
    let outputs = generator
        .generate_accepted_types(&[accepted_type_a_hyp, accepted_type_b_hyp])
        .expect("generate_accepted_types must succeed");

    assert_eq!(outputs.len(), 2, "one declaration output per accepted type");
    for output in &outputs {
        assert!(output.file_path.exists(), "generated hpp must exist");
    }

    // 9. Assert generated declarations reflect the accepted layouts.
    let type_a_hpp = tmp
        .path()
        .join("include/recovered")
        .join(entity_to_source_path(entity_type_a))
        .with_extension("hpp");
    let type_b_hpp = tmp
        .path()
        .join("include/recovered")
        .join(entity_to_source_path(entity_type_b))
        .with_extension("hpp");

    let type_a_content = std::fs::read_to_string(&type_a_hpp).expect("Type A hpp must be readable");
    let type_b_content = std::fs::read_to_string(&type_b_hpp).expect("Type B hpp must be readable");

    assert!(type_a_content.contains("namespace recovered"));
    assert!(type_a_content.contains("uint8_t field_0[4]"));
    assert!(type_a_content.contains("uint8_t pad_4[12]"));

    assert!(type_b_content.contains("namespace recovered"));
    assert!(type_b_content.contains("uint8_t field_0[4]"));
    assert!(type_b_content.contains("uint8_t pad_4[28]"));

    eprintln!("[wave8_shared_model] generated declarations reflect accepted layouts");

    // 10. Run the mock build provider to verify the skeleton still compiles.
    let provider = success_build_provider();
    let gen_manifest = GeneratorManifest {
        project_root: tmp.path().to_path_buf(),
        cmake_generator: "NMake Makefiles".into(),
        source_files: vec![cpp_relpath(entity_function)],
        executable_target: "reconstruction_skeleton".into(),
    };

    let configured = provider
        .configure_project(&gen_manifest, tmp.path())
        .await
        .expect("configure_project must succeed");
    assert!(configured.success, "configure must report success");

    let units = vec![CompileUnit {
        source_path: cpp_relpath(entity_function),
        object_path: PathBuf::from("build")
            .join(entity_function.as_uuid().as_simple().to_string())
            .with_extension("obj"),
    }];
    let compiled = provider
        .compile_units(&units)
        .await
        .expect("compile_units must succeed");
    assert!(compiled.success, "compile must report success");

    let linked = provider
        .link_target(&compiled.objects)
        .await
        .expect("link_target must succeed");
    assert!(linked.success, "link must report success");

    // Record the build attempt as a canonical mutation.
    client
        .execute(ApplicationCommand::RecordBuildAttempt(
            RecordBuildAttemptRequest {
                project,
                work_item_id: "wave8-shared-model-build".into(),
            },
        ))
        .expect("RecordBuildAttempt must succeed");

    eprintln!("[wave8_shared_model] mock build green");

    // 11. Verify the function skeleton body is tamper-not-modified; only the
    //     declaration side (type hpp files) changed.
    let fn_cpp_after = std::fs::read_to_string(&fn_cpp_path).expect("function cpp still readable");
    assert_eq!(
        fn_cpp_after, fn_cpp_original,
        "function stub body must be tamper-not-modified"
    );

    // The type hpp files should no longer contain the original stub marker.
    assert!(
        !type_a_content.contains("reconstruction_status = \"stubbed\""),
        "Type A hpp must be replaced by declaration"
    );
    assert!(
        !type_b_content.contains("reconstruction_status = \"stubbed\""),
        "Type B hpp must be replaced by declaration"
    );

    // 12. Audit: every canonical mutation went through an ApplicationCommand.
    for cmd in client.commands() {
        assert!(
            matches!(
                cmd,
                ApplicationCommand::RegisterEntity(_)
                    | ApplicationCommand::RegisterArtifact(_)
                    | ApplicationCommand::RegisterGeneratedSourceMapping(_)
                    | ApplicationCommand::AddEvidence(_)
                    | ApplicationCommand::AddHypothesis(_)
                    | ApplicationCommand::CreateWorkItems(_)
                    | ApplicationCommand::AcceptHypothesisPolicyDriven(_)
                    | ApplicationCommand::InvalidateGeneratedSource(_)
                    | ApplicationCommand::RecordBuildAttempt(_)
            ),
            "every canonical mutation must be an ApplicationCommand variant, got: {cmd:?}"
        );
    }

    eprintln!(
        "[wave8_shared_model] command audit passed: {} canonical mutation(s)",
        client.commands().len()
    );
    eprintln!("[OK] shared types recovered, declaration artifacts up-to-date; build green");
}
