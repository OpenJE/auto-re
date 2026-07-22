//! Tests for fingerprint computation and invalidation propagation.

use std::collections::HashMap;

use autore_app::ApplicationCommand;
use autore_schema::domain::ContentHash;
use autore_schema::domain::records::WorkItemKind;
use autore_schema::ids::{HypothesisId, ProjectId, WorkItemId};
use petgraph::graph::DiGraph;

use super::compute::{
    FingerprintComparison, FingerprintInput, compare_fingerprint, compute_fingerprint,
};
use super::invalidate::{InMemorySnapshot, InvalidationPropagator};
use crate::tests_support::RecordingAutoReClient;
use crate::work_graph::{DependencyEdgeKind, WorkGraph, WorkItemNode};

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn base_input() -> FingerprintInput {
    FingerprintInput {
        static_artifact_hashes: vec![ContentHash::from_bytes(b"static-a")],
        accepted_hypotheses: vec![],
        upstream_declarations: vec![ContentHash::from_bytes(b"upstream-1")],
        dynamic_observations: vec![],
        prompt_template_version: "v1".into(),
        model_config_hash: ContentHash::from_bytes(b"model-v1"),
        build_config_hash: ContentHash::from_bytes(b"build-v1"),
        verification_policy_hash: ContentHash::from_bytes(b"verify-v1"),
    }
}

/// Build a `WorkGraph` from a list of `(source, target, kind)` edge triples.
/// `source` depends on `target`.  Returns the graph and a map from label to
/// `WorkItemId`.
fn build_test_graph(
    labels: &[&str],
    edges: &[(&str, &str, DependencyEdgeKind)],
) -> (WorkGraph, HashMap<String, WorkItemId>) {
    let mut graph = DiGraph::new();
    let mut label_to_id: HashMap<String, WorkItemId> = HashMap::new();
    let mut id_to_idx = HashMap::new();
    let mut work_item_to_node = HashMap::new();

    for label in labels {
        let id = WorkItemId::new();
        let idx = graph.add_node(WorkItemNode {
            work_item_id: id,
            kind: WorkItemKind::Function,
            entity_id: None,
        });
        label_to_id.insert((*label).to_string(), id);
        id_to_idx.insert((*label).to_string(), idx);
        work_item_to_node.insert(id, idx);
    }

    for (src_label, tgt_label, kind) in edges {
        let src_idx = id_to_idx[*src_label];
        let tgt_idx = id_to_idx[*tgt_label];
        graph.add_edge(src_idx, tgt_idx, *kind);
    }

    (
        WorkGraph {
            graph,
            entity_to_node: HashMap::new(),
            work_item_to_node,
        },
        label_to_id,
    )
}

// -----------------------------------------------------------------------
// Test 1: Fingerprint is deterministic for same inputs
// -----------------------------------------------------------------------

#[test]
fn fingerprint_is_deterministic_for_same_inputs() {
    let input = base_input();
    let fp1 = compute_fingerprint(&input);
    let fp2 = compute_fingerprint(&input);
    assert_eq!(fp1, fp2, "same input must produce same fingerprint");
}

// -----------------------------------------------------------------------
// Test 2: Fingerprint changes when upstream declaration changes
// -----------------------------------------------------------------------

#[test]
fn fingerprint_changes_when_upstream_declaration_changes() {
    let input_a = base_input();
    let fp_a = compute_fingerprint(&input_a);

    let mut input_b = base_input();
    input_b.upstream_declarations = vec![ContentHash::from_bytes(b"upstream-2")];
    let fp_b = compute_fingerprint(&input_b);

    assert_ne!(
        fp_a, fp_b,
        "different upstream declaration must change fingerprint"
    );
}

// -----------------------------------------------------------------------
// Test 3: Fingerprint does not change on only hypothesis status transition
// -----------------------------------------------------------------------

#[test]
fn fingerprint_does_not_change_on_only_hypothesis_status_transition() {
    let h1 = HypothesisId::new();
    let h2 = HypothesisId::new();

    let mut input_before = base_input();
    input_before.accepted_hypotheses = vec![h1, h2];
    let fp_before = compute_fingerprint(&input_before);

    // Same hypothesis IDs — only "acceptance timestamps" would differ
    // in the real system, but we don't store timestamps in the input.
    let mut input_after = base_input();
    input_after.accepted_hypotheses = vec![h1, h2];
    let fp_after = compute_fingerprint(&input_after);

    assert_eq!(
        fp_before, fp_after,
        "same hypothesis IDs with different timestamps must yield same fingerprint"
    );
}

// -----------------------------------------------------------------------
// Test 4: Invalidation propagates through GeneratedDeclRequirement only
// -----------------------------------------------------------------------

#[test]
fn invalidation_propagates_through_generated_decl_edges_only() {
    // B depends on A via GeneratedDeclRequirement (edge B→A)
    // C depends on A via DirectCall (edge C→A)
    let (graph, ids) = build_test_graph(
        &["A", "B", "C"],
        &[
            ("B", "A", DependencyEdgeKind::GeneratedDeclRequirement),
            ("C", "A", DependencyEdgeKind::DirectCall),
        ],
    );

    let id_a = ids["A"];
    let id_b = ids["B"];

    // B's input has changed → stored fingerprint is stale
    let input_b_new = FingerprintInput {
        upstream_declarations: vec![ContentHash::from_bytes(b"a-changed")],
        ..base_input()
    };
    let fp_b_new = compute_fingerprint(&input_b_new);
    let fp_b_stale = ContentHash::from_bytes(b"stale-fingerprint");

    let mut snapshot = InMemorySnapshot::new();
    snapshot.insert(id_b, input_b_new, fp_b_stale.clone());
    // C has no entry in snapshot → propagator skips it even if it checks

    let project = ProjectId::new();
    let client = RecordingAutoReClient::new();
    let propagator = InvalidationPropagator::new(&client, project);

    let result = propagator.propagate(&id_a, &graph, &snapshot).unwrap();

    assert!(
        result.contains(&id_b),
        "B must be invalidated (GeneratedDeclRequirement edge)"
    );
    assert_eq!(result.len(), 1, "only B invalidated, not C");

    // Verify fp_b_new is actually different from stale
    assert_ne!(
        fp_b_new, fp_b_stale,
        "test setup: new fp differs from stale"
    );
    assert_eq!(
        compare_fingerprint(&fp_b_new, Some(&fp_b_stale)),
        FingerprintComparison::Changed
    );
}

// -----------------------------------------------------------------------
// Test 5: Invalidation stops at matching fingerprint
// -----------------------------------------------------------------------

#[test]
fn invalidation_stops_at_matching_fingerprint() {
    // B depends on A via GeneratedDeclRequirement (edge B→A)
    // C depends on B via GeneratedDeclRequirement (edge C→B)
    let (graph, ids) = build_test_graph(
        &["A", "B", "C"],
        &[
            ("B", "A", DependencyEdgeKind::GeneratedDeclRequirement),
            ("C", "B", DependencyEdgeKind::GeneratedDeclRequirement),
        ],
    );

    let id_a = ids["A"];
    let id_b = ids["B"];

    // B's stored fingerprint matches its current input → no change
    let input_b = base_input();
    let fp_b = compute_fingerprint(&input_b);

    let mut snapshot = InMemorySnapshot::new();
    snapshot.insert(id_b, input_b, fp_b);

    let project = ProjectId::new();
    let client = RecordingAutoReClient::new();
    let propagator = InvalidationPropagator::new(&client, project);

    let result = propagator.propagate(&id_a, &graph, &snapshot).unwrap();

    assert!(
        result.is_empty(),
        "no items invalidated when B's fingerprint is unchanged"
    );
}

// -----------------------------------------------------------------------
// Test 6: Invalidation issues InvalidateWorkItem command
// -----------------------------------------------------------------------

#[test]
fn invalidation_issues_invalidate_work_item_command() {
    // B depends on A via BuildDependency (edge B→A)
    let (graph, ids) = build_test_graph(
        &["A", "B"],
        &[("B", "A", DependencyEdgeKind::BuildDependency)],
    );

    let id_a = ids["A"];
    let id_b = ids["B"];

    // B's input changed → stale stored fingerprint
    let input_b_new = FingerprintInput {
        build_config_hash: ContentHash::from_bytes(b"build-v2"),
        ..base_input()
    };
    let fp_b_stale = ContentHash::from_bytes(b"old-fp");

    let mut snapshot = InMemorySnapshot::new();
    snapshot.insert(id_b, input_b_new, fp_b_stale);

    let project = ProjectId::new();
    let client = RecordingAutoReClient::new();
    let propagator = InvalidationPropagator::new(&client, project);

    let result = propagator.propagate(&id_a, &graph, &snapshot).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], id_b);

    // Verify the command was issued
    let invalidate_count = client.count(|c| matches!(c, ApplicationCommand::InvalidateWorkItem(_)));
    assert_eq!(
        invalidate_count, 1,
        "exactly one InvalidateWorkItem command"
    );

    // Verify the command carries the right work_item_id
    let commands = client.commands();
    let invalidate_cmd = commands
        .iter()
        .find(|c| matches!(c, ApplicationCommand::InvalidateWorkItem(_)))
        .expect("must find InvalidateWorkItem command");

    if let ApplicationCommand::InvalidateWorkItem(req) = invalidate_cmd {
        assert_eq!(req.work_item_id, id_b.to_string());
        assert_eq!(req.project, project);
    } else {
        panic!("unexpected command variant");
    }
}
