//! Tests for [`WorkGraphBuilder`] — verifies entity-to-work-item mapping,
//! SCC collapse, mixed-kind rejection, and command routing.

use crate::tests_support::RecordingAutoReClient;
use crate::work_graph::builder::WorkGraphBuilder;
use crate::work_graph::kind::{DependencyEdgeKind, ENTITY_KIND_ENTRYPOINT, ENTITY_KIND_VTABLE};
use autore_app::ApplicationCommand;
use autore_schema::domain::NamespacedId;
use autore_schema::domain::records::{
    ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, ENTITY_KIND_GLOBAL, SemanticEntity,
    WorkItemKind,
};
use autore_schema::ids::{BinaryRevisionId, ProjectId, ReconstructionCampaignId};

fn make_entity(kind: NamespacedId, name: &str) -> SemanticEntity {
    SemanticEntity::new(ProjectId::new(), kind, None, Some(name.into()))
}

fn project_and_campaign() -> (ProjectId, ReconstructionCampaignId, BinaryRevisionId) {
    (
        ProjectId::new(),
        ReconstructionCampaignId::new(),
        BinaryRevisionId::new(),
    )
}

// -----------------------------------------------------------------------
// Test 1: N function entities → N Function work items + 1 ProgramSkeleton
// -----------------------------------------------------------------------

#[test]
fn work_graph_builder_creates_function_per_entity() {
    let client = RecordingAutoReClient::new();
    let (pid, cid, bid) = project_and_campaign();

    let entities: Vec<SemanticEntity> = (0..5)
        .map(|i| make_entity(ENTITY_KIND_FUNCTION.clone(), &format!("fn_{i}")))
        .collect();

    let graph = WorkGraphBuilder::build(&client, pid, cid, bid, &entities, &[]).unwrap();

    // 5 functions + 1 ProgramSkeleton = 6 nodes
    assert_eq!(graph.nodes_of_kind(&WorkItemKind::Function).len(), 5);
    assert_eq!(graph.nodes_of_kind(&WorkItemKind::ProgramSkeleton).len(), 1);
    assert_eq!(graph.node_count(), 6);

    // Verify CreateWorkItems command was issued
    let create_count = client.count(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)));
    assert_eq!(create_count, 1, "exactly one CreateWorkItems command");
}

// -----------------------------------------------------------------------
// Test 2: f_a → f_b → f_a collapses to one FunctionCluster
// -----------------------------------------------------------------------

#[test]
fn work_graph_builder_collapses_recursive_cycles_to_cluster() {
    let client = RecordingAutoReClient::new();
    let (pid, cid, bid) = project_and_campaign();

    let f_a = make_entity(ENTITY_KIND_FUNCTION.clone(), "f_a");
    let f_b = make_entity(ENTITY_KIND_FUNCTION.clone(), "f_b");
    let entities = vec![f_a.clone(), f_b.clone()];

    let edges = vec![
        (f_a.id, f_b.id, DependencyEdgeKind::DirectCall),
        (f_b.id, f_a.id, DependencyEdgeKind::DirectCall),
    ];

    let graph = WorkGraphBuilder::build(&client, pid, cid, bid, &entities, &edges).unwrap();

    // Should have: 1 ProgramSkeleton + 2 Function + 1 FunctionCluster = 4 nodes
    let clusters = graph.nodes_of_kind(&WorkItemKind::FunctionCluster);
    assert_eq!(clusters.len(), 1, "one FunctionCluster created");

    // The cluster should have ClusterMember edges to both functions
    let cluster_ni = graph.work_item_to_node[&clusters[0].work_item_id];
    let member_edges: Vec<_> = graph
        .graph
        .edges_directed(cluster_ni, petgraph::Direction::Outgoing)
        .filter(|e| *e.weight() == DependencyEdgeKind::ClusterMember)
        .collect();
    assert_eq!(member_edges.len(), 2, "cluster has 2 ClusterMember edges");

    // Two CreateWorkItems commands: one for initial items, one for cluster
    let create_count = client.count(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)));
    assert_eq!(create_count, 2);
}

// -----------------------------------------------------------------------
// Test 3: single function with no cycle → remains plain Function
// -----------------------------------------------------------------------

#[test]
fn work_graph_builder_leaves_singleton_sccs_alone() {
    let client = RecordingAutoReClient::new();
    let (pid, cid, bid) = project_and_campaign();

    let f = make_entity(ENTITY_KIND_FUNCTION.clone(), "lonely_fn");
    let entities = vec![f];

    let graph = WorkGraphBuilder::build(&client, pid, cid, bid, &entities, &[]).unwrap();

    assert_eq!(graph.nodes_of_kind(&WorkItemKind::Function).len(), 1);
    assert_eq!(graph.nodes_of_kind(&WorkItemKind::FunctionCluster).len(), 0);

    // Only one CreateWorkItems (no cluster creation)
    let create_count = client.count(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)));
    assert_eq!(create_count, 1);
}

// -----------------------------------------------------------------------
// Test 4: function ↔ vtable cycle → ValidationError
// -----------------------------------------------------------------------

#[test]
fn work_graph_builder_rejects_mixed_kind_scc() {
    let client = RecordingAutoReClient::new();
    let (pid, cid, bid) = project_and_campaign();

    let f = make_entity(ENTITY_KIND_FUNCTION.clone(), "fn_mixed");
    let v = make_entity(ENTITY_KIND_VTABLE.clone(), "vtable_mixed");
    let entities = vec![f.clone(), v.clone()];

    let edges = vec![
        (f.id, v.id, DependencyEdgeKind::VtableMembership),
        (v.id, f.id, DependencyEdgeKind::DirectCall),
    ];

    let result = WorkGraphBuilder::build(&client, pid, cid, bid, &entities, &edges);
    assert!(result.is_err(), "mixed-kind SCC must be rejected");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("mixed-kind SCC"),
        "error message mentions mixed-kind: {err_msg}"
    );
}

// -----------------------------------------------------------------------
// Test 5: RecordWorkDependency count matches expected edge count
// -----------------------------------------------------------------------

#[test]
fn work_graph_builder_records_dependencies_via_command() {
    let client = RecordingAutoReClient::new();
    let (pid, cid, bid) = project_and_campaign();

    let f_a = make_entity(ENTITY_KIND_FUNCTION.clone(), "f_a");
    let f_b = make_entity(ENTITY_KIND_FUNCTION.clone(), "f_b");
    let g = make_entity(ENTITY_KIND_GLOBAL.clone(), "g_var");
    let entities = vec![f_a.clone(), f_b.clone(), g.clone()];

    // f_a calls f_b, f_b accesses g
    let edges = vec![
        (f_a.id, f_b.id, DependencyEdgeKind::DirectCall),
        (f_b.id, g.id, DependencyEdgeKind::GlobalAccess),
    ];

    let _graph = WorkGraphBuilder::build(&client, pid, cid, bid, &entities, &edges).unwrap();

    let dep_count = client.count(|c| matches!(c, ApplicationCommand::RecordWorkDependency(_)));
    assert_eq!(dep_count, 2, "2 dependency edges recorded via commands");
}

// -----------------------------------------------------------------------
// Test 6: ProgramSkeleton and Entrypoint work items are created
// -----------------------------------------------------------------------

#[test]
fn work_graph_builder_prioritizes_entrypoint_and_skeleton() {
    let client = RecordingAutoReClient::new();
    let (pid, cid, bid) = project_and_campaign();

    let ep = make_entity(ENTITY_KIND_ENTRYPOINT.clone(), "main");
    let ext = make_entity(ENTITY_KIND_EXTERNAL_FUNCTION.clone(), "printf");
    let entities = vec![ep, ext];

    let graph = WorkGraphBuilder::build(&client, pid, cid, bid, &entities, &[]).unwrap();

    assert_eq!(
        graph.nodes_of_kind(&WorkItemKind::ProgramSkeleton).len(),
        1,
        "ProgramSkeleton singleton created"
    );
    assert_eq!(
        graph.nodes_of_kind(&WorkItemKind::Entrypoint).len(),
        1,
        "Entrypoint created for entrypoint entity"
    );
    assert_eq!(
        graph.nodes_of_kind(&WorkItemKind::ExternalDependency).len(),
        1,
        "ExternalDependency created for external-function entity"
    );
}
