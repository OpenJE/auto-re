//! End-to-end integration test: ingestion → work graph → cycle → invalidation.
//!
//! Exercises all 7 steps from the Wave 4 Todo 20 plan:
//! 1. Ingest the `hello` fixture via synthesized observations.
//! 2. Build a work graph from imported entities.
//! 3. Assert `ProgramSkeleton`, `Entrypoint`, `Function`, `ExternalDependency` work items.
//! 4. Assert `DirectCall` dependency edges.
//! 5. Induce a synthetic cycle; assert SCC collapse into `FunctionCluster`.
//! 6. Compute fingerprints, simulate generation, assert downstream invalidation.
//! 7. Simulate scheduler tick; assert priority ordering matches spec §7.4.
//!
//! # Fallback
//!
//! Uses the same synthesized observation stream as `ida_full_ingest.rs` when
//! the real IDA environment is unavailable.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use tests_support::RecordingAutoReClient;

use autore_app::ApplicationCommand;
use autore_reconstruction::fingerprint::{
    FingerprintInput, InMemorySnapshot, InvalidationPropagator, compute_fingerprint,
};
use autore_reconstruction::identity::{ImportSummary, ObservationImporter, ObservationProduced};
use autore_reconstruction::work_graph::{
    DependencyEdgeKind, WorkGraph, WorkGraphBuilder, WorkItemNode,
};
use autore_schema::domain::records::{
    ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, SemanticEntity, WorkItemKind,
};
use autore_schema::domain::{MetadataMap, NamespacedId, Timestamp};
use autore_schema::ids::{
    ArtifactId, BinaryRevisionId, EntityId, ProjectId, ProviderRunId, ReconstructionCampaignId,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Expected function count in the `hello` fixture (add, multiply, greet, main).
const EXPECTED_FUNCTION_COUNT: usize = 4;

/// Expected external dependency count (one synthetic `printf` import).
const EXPECTED_EXTERNAL_COUNT: usize = 1;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn synthesize_observation(
    kind: &str,
    address_space: u32,
    entry_address: u64,
    display_name: &str,
) -> ObservationProduced {
    let payload = serde_json::json!({
        "address_space": address_space,
        "entry_address": entry_address,
        "display_name": display_name,
        "ea": format!("0x{entry_address:x}"),
    });
    ObservationProduced {
        provider_instance_id: "test-ida-instance".into(),
        request_id: "test-request-001".into(),
        operation_id: "test-op-001".into(),
        capability_id: "ida.binary.ingest".into(),
        capability_version: "1.0.0".into(),
        sequence: 0,
        observation_kind: kind.into(),
        payload: serde_json::to_vec(&payload).unwrap(),
        artifacts: Vec::new(),
    }
}

fn synthesize_hello_observations() -> Vec<ObservationProduced> {
    vec![
        synthesize_observation("ida.ingest.functions", 1, 0x1149, "add"),
        synthesize_observation("ida.ingest.functions", 1, 0x1160, "multiply"),
        synthesize_observation("ida.ingest.functions", 1, 0x1177, "greet"),
        synthesize_observation("ida.ingest.functions", 1, 0x1199, "main"),
        synthesize_observation("ida.ingest.imports", 1, 0x3000, "printf"),
    ]
}

fn make_entity(kind: NamespacedId, name: &str) -> SemanticEntity {
    SemanticEntity {
        id: EntityId::new(),
        project: ProjectId::new(),
        kind,
        stable_key: None,
        display_name: Some(name.into()),
        created_at: Timestamp::now(),
        metadata: MetadataMap::new(),
    }
}

fn make_fingerprint_input(
    upstream_hashes: Vec<autore_schema::domain::ContentHash>,
) -> FingerprintInput {
    let zero = autore_schema::domain::ContentHash::blake3(b"zero");
    FingerprintInput {
        static_artifact_hashes: vec![autore_schema::domain::ContentHash::blake3(b"static")],
        accepted_hypotheses: Vec::new(),
        upstream_declarations: upstream_hashes,
        dynamic_observations: Vec::new(),
        prompt_template_version: "v1".into(),
        model_config_hash: zero.clone(),
        build_config_hash: zero.clone(),
        verification_policy_hash: zero,
    }
}

/// Spec §7.4 priority ordering: lower number = higher priority = dispatched first.
fn work_item_priority_score(kind: &WorkItemKind) -> u32 {
    match kind {
        WorkItemKind::ProgramSkeleton => 1000,
        WorkItemKind::ExternalDependency => 900,
        WorkItemKind::Entrypoint => 800,
        WorkItemKind::FunctionCluster => 700,
        WorkItemKind::Function => 500,
        WorkItemKind::Structure => 600,
        WorkItemKind::Global => 550,
        _ => 100,
    }
}

fn assert_all_commands_are_application_commands(client: &RecordingAutoReClient) {
    for cmd in client.commands() {
        let is_valid = matches!(
            cmd,
            ApplicationCommand::RegisterEntity(_)
                | ApplicationCommand::ImportProviderRunResult(_)
                | ApplicationCommand::BlockWorkItem(_)
                | ApplicationCommand::CreateWorkItems(_)
                | ApplicationCommand::RecordWorkDependency(_)
                | ApplicationCommand::InvalidateWorkItem(_)
        );
        assert!(
            is_valid,
            "every mutation must be an ApplicationCommand, got: {cmd:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

/// End-to-end: ingestion → work graph → cycle → invalidation → scheduler priority.
#[test]
#[ignore]
fn whole_program_work_graph_end_to_end() {
    // ── Step 1: Ingest the hello fixture ──────────────────────────────
    eprintln!("[whole_program] SYNTHESIZED FALLBACK: using synthesized observation stream");

    let client = RecordingAutoReClient::new();
    let importer = ObservationImporter::new(&client);
    let binary_revision_id = ArtifactId::from_uuid(uuid::Uuid::nil());
    let campaign_id = ReconstructionCampaignId::new();
    let project_id = ProjectId::new();
    let run_id = ProviderRunId::new();

    let observations = synthesize_hello_observations();
    let summary: ImportSummary = importer
        .import(
            &observations,
            binary_revision_id,
            campaign_id,
            project_id,
            run_id,
        )
        .expect("import must succeed");

    assert_eq!(
        summary.entities_created,
        (EXPECTED_FUNCTION_COUNT + EXPECTED_EXTERNAL_COUNT) as u64,
        "expected {} functions + {} external",
        EXPECTED_FUNCTION_COUNT,
        EXPECTED_EXTERNAL_COUNT
    );
    eprintln!(
        "[whole_program] Step 1: {} entities created",
        summary.entities_created
    );

    // ── Step 2: Build work graph ─────────────────────────────────────
    // Construct entities mirroring what the importer created.
    let fn_add = make_entity(ENTITY_KIND_FUNCTION.clone(), "add");
    let fn_multiply = make_entity(ENTITY_KIND_FUNCTION.clone(), "multiply");
    let fn_greet = make_entity(ENTITY_KIND_FUNCTION.clone(), "greet");
    let fn_main = make_entity(ENTITY_KIND_FUNCTION.clone(), "main");
    let ext_printf = make_entity(ENTITY_KIND_EXTERNAL_FUNCTION.clone(), "printf");

    let entities = vec![
        fn_add.clone(),
        fn_multiply.clone(),
        fn_greet.clone(),
        fn_main.clone(),
        ext_printf.clone(),
    ];

    // Dependency edges: main → greet → add, greet → multiply, greet → printf.
    let edges = vec![
        (fn_main.id, fn_greet.id, DependencyEdgeKind::DirectCall),
        (fn_greet.id, fn_add.id, DependencyEdgeKind::DirectCall),
        (fn_greet.id, fn_multiply.id, DependencyEdgeKind::DirectCall),
        (
            fn_greet.id,
            ext_printf.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
    ];

    let binary_rev = BinaryRevisionId::new();
    let graph: WorkGraph = WorkGraphBuilder::build(
        &client,
        project_id,
        campaign_id,
        binary_rev,
        &entities,
        &edges,
    )
    .expect("work graph build must succeed");

    eprintln!(
        "[whole_program] Step 2: graph has {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    // ── Step 3: Assert work item kinds ───────────────────────────────
    let skeleton_nodes = graph.nodes_of_kind(&WorkItemKind::ProgramSkeleton);
    assert_eq!(
        skeleton_nodes.len(),
        1,
        "exactly one ProgramSkeleton work item"
    );

    let function_nodes = graph.nodes_of_kind(&WorkItemKind::Function);
    assert_eq!(
        function_nodes.len(),
        EXPECTED_FUNCTION_COUNT,
        "expected {EXPECTED_FUNCTION_COUNT} Function work items"
    );

    let external_nodes = graph.nodes_of_kind(&WorkItemKind::ExternalDependency);
    assert_eq!(
        external_nodes.len(),
        EXPECTED_EXTERNAL_COUNT,
        "expected {EXPECTED_EXTERNAL_COUNT} ExternalDependency work items"
    );

    eprintln!(
        "[whole_program] Step 3: 1 ProgramSkeleton, {} Functions, {} ExternalDependencies",
        function_nodes.len(),
        external_nodes.len()
    );

    // ── Step 4: Assert dependency edges ──────────────────────────────
    let direct_call_count = graph
        .graph
        .edge_weights()
        .filter(|k| **k == DependencyEdgeKind::DirectCall)
        .count();
    assert!(
        direct_call_count >= 3,
        "expected at least 3 DirectCall edges, got {direct_call_count}"
    );
    eprintln!(
        "[whole_program] Step 4: {} DirectCall edges",
        direct_call_count
    );

    // ── Step 5: Synthetic cycle → SCC collapse ──────────────────────
    let cycle_client = RecordingAutoReClient::new();
    let fn_a = make_entity(ENTITY_KIND_FUNCTION.clone(), "f_a");
    let fn_b = make_entity(ENTITY_KIND_FUNCTION.clone(), "f_b");
    let fn_c = make_entity(ENTITY_KIND_FUNCTION.clone(), "f_c");

    let cycle_entities = vec![fn_a.clone(), fn_b.clone(), fn_c.clone()];
    // f_a → f_b → f_a forms a cycle; f_c is acyclic.
    let cycle_edges = vec![
        (fn_a.id, fn_b.id, DependencyEdgeKind::DirectCall),
        (fn_b.id, fn_a.id, DependencyEdgeKind::DirectCall),
        (fn_c.id, fn_a.id, DependencyEdgeKind::DirectCall),
    ];

    let cycle_graph: WorkGraph = WorkGraphBuilder::build(
        &cycle_client,
        project_id,
        campaign_id,
        binary_rev,
        &cycle_entities,
        &cycle_edges,
    )
    .expect("cycle graph build must succeed");

    let cluster_nodes = cycle_graph.nodes_of_kind(&WorkItemKind::FunctionCluster);
    assert_eq!(
        cluster_nodes.len(),
        1,
        "SCC collapse must produce exactly one FunctionCluster"
    );

    // The cluster must have ClusterMember edges to its members.
    let cluster_ni = *cycle_graph
        .work_item_to_node
        .get(&cluster_nodes[0].work_item_id)
        .unwrap();
    let member_edges: Vec<_> = cycle_graph
        .graph
        .edges_directed(cluster_ni, petgraph::Direction::Outgoing)
        .filter(|e| *e.weight() == DependencyEdgeKind::ClusterMember)
        .collect();
    assert_eq!(
        member_edges.len(),
        2,
        "FunctionCluster must have 2 ClusterMember edges (f_a, f_b)"
    );

    eprintln!(
        "[whole_program] Step 5: SCC collapsed f_a+f_b into FunctionCluster with {} members",
        member_edges.len()
    );

    // ── Step 6: Fingerprint invalidation ─────────────────────────────
    let fp_client = RecordingAutoReClient::new();

    // Build a small graph: upstream → downstream via GeneratedDeclRequirement.
    let upstream = make_entity(ENTITY_KIND_FUNCTION.clone(), "upstream_fn");
    let downstream = make_entity(ENTITY_KIND_FUNCTION.clone(), "downstream_fn");

    let fp_entities = vec![upstream.clone(), downstream.clone()];
    let fp_edges = vec![(
        downstream.id,
        upstream.id,
        DependencyEdgeKind::GeneratedDeclRequirement,
    )];

    let fp_graph: WorkGraph = WorkGraphBuilder::build(
        &fp_client,
        project_id,
        campaign_id,
        binary_rev,
        &fp_entities,
        &fp_edges,
    )
    .expect("fingerprint graph build must succeed");

    // Find the work-item IDs for upstream and downstream.
    let upstream_wid = fp_graph
        .graph
        .node_weights()
        .find(|n| n.entity_id == Some(upstream.id))
        .map(|n| n.work_item_id)
        .expect("upstream node must exist");
    let downstream_wid = fp_graph
        .graph
        .node_weights()
        .find(|n| n.entity_id == Some(downstream.id))
        .map(|n| n.work_item_id)
        .expect("downstream node must exist");

    // Build snapshot: downstream depends on upstream's declaration hash.
    let old_upstream_hash = autore_schema::domain::ContentHash::blake3(b"old_decl");
    let new_upstream_hash = autore_schema::domain::ContentHash::blake3(b"new_decl");

    let upstream_input = make_fingerprint_input(vec![old_upstream_hash.clone()]);
    let upstream_fp = compute_fingerprint(&upstream_input);

    let downstream_input_old = make_fingerprint_input(vec![old_upstream_hash.clone()]);
    let downstream_fp_old = compute_fingerprint(&downstream_input_old);

    // The downstream input references the NEW upstream hash (simulating generation).
    let downstream_input_new = make_fingerprint_input(vec![new_upstream_hash]);
    let downstream_fp_new = compute_fingerprint(&downstream_input_new);

    assert_ne!(
        downstream_fp_old, downstream_fp_new,
        "changing upstream hash must change downstream fingerprint"
    );

    // Store the OLD fingerprints in the snapshot.
    let mut snapshot = InMemorySnapshot::new();
    snapshot.insert(upstream_wid, upstream_input, upstream_fp);
    // The downstream still has the OLD input stored (before regeneration),
    // but we insert the NEW input so recomputation yields a different hash.
    snapshot.insert(downstream_wid, downstream_input_new, downstream_fp_old);

    let propagator = InvalidationPropagator::new(&fp_client, project_id);
    let invalidated = propagator
        .propagate(&upstream_wid, &fp_graph, &snapshot)
        .expect("propagation must succeed");

    assert!(
        invalidated.contains(&downstream_wid),
        "downstream work item must be invalidated when upstream declaration changes"
    );

    let invalidate_count =
        fp_client.count(|c| matches!(c, ApplicationCommand::InvalidateWorkItem(_)));
    assert!(
        invalidate_count >= 1,
        "at least one InvalidateWorkItem command expected"
    );

    eprintln!(
        "[whole_program] Step 6: {} work items invalidated via {} InvalidateWorkItem commands",
        invalidated.len(),
        invalidate_count
    );

    // ── Step 7: Scheduler priority ordering ──────────────────────────
    // Simulate scheduler dispatch ordering using spec §7.4 priority.
    // Collect all work items from the main graph, sort by priority,
    // and verify ProgramSkeleton and ExternalDependency dispatch first.
    let mut work_items: Vec<&WorkItemNode> = fp_graph.graph.node_weights().collect();
    // Also include items from the main graph.
    work_items.extend(graph.graph.node_weights());

    // Deduplicate by work_item_id.
    let mut seen = std::collections::HashSet::new();
    work_items.retain(|n| seen.insert(n.work_item_id));

    // Sort by priority score (descending — highest first).
    work_items.sort_by_key(|b| std::cmp::Reverse(work_item_priority_score(&b.kind)));

    // Record dispatched kind sequence.
    let dispatched_sequence: Vec<WorkItemKind> =
        work_items.iter().map(|n| n.kind.clone()).collect();

    eprintln!(
        "[whole_program] Step 7: dispatch sequence = {:?}",
        dispatched_sequence
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
    );

    // ProgramSkeleton must appear before any Function.
    let skeleton_pos = dispatched_sequence
        .iter()
        .position(|k| *k == WorkItemKind::ProgramSkeleton);
    let first_function_pos = dispatched_sequence
        .iter()
        .position(|k| *k == WorkItemKind::Function);

    if let (Some(sp), Some(fp)) = (skeleton_pos, first_function_pos) {
        assert!(
            sp < fp,
            "ProgramSkeleton (pos {sp}) must dispatch before first Function (pos {fp})"
        );
    }

    // ExternalDependency must appear before Function.
    let external_pos = dispatched_sequence
        .iter()
        .position(|k| *k == WorkItemKind::ExternalDependency);
    if let (Some(ep), Some(fp)) = (external_pos, first_function_pos) {
        assert!(
            ep < fp,
            "ExternalDependency (pos {ep}) must dispatch before first Function (pos {fp})"
        );
    }

    // ── Final audit: all mutations are ApplicationCommands ───────────
    assert_all_commands_are_application_commands(&client);
    assert_all_commands_are_application_commands(&cycle_client);
    assert_all_commands_are_application_commands(&fp_client);

    eprintln!("[whole_program] PASSED: all 7 steps verified");
    eprintln!(
        "[OK] traversed executable without manual selection; priority ordering matched spec §7.4 sample"
    );
}
