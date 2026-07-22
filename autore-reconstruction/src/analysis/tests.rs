//! Tests for the `analysis` module.

use std::collections::HashMap;

use autore_schema::domain::records::WorkItemKind;
use autore_schema::ids::{ArtifactId, ConflictRecordId, EntityId, HypothesisId, WorkItemId};
use petgraph::graph::DiGraph;
use serde_json::{Value, json};

use crate::analysis::builder::{BundleBuilder, BundleStore, StaticArtifactSet};
use crate::analysis::bundle::{BUNDLE_MAX_BYTES, InvestigationBundle, StringSnippet};
use crate::analysis::schemas::{
    CAPABILITIES, request_payload_for, request_schema, response_schema_for,
    validate_request_payload, validate_response_payload,
};
use crate::work_graph::graph::{WorkGraph, WorkItemNode};
use crate::work_graph::kind::DependencyEdgeKind;

struct StubStore {
    entities: HashMap<WorkItemId, EntityId>,
    static_arts: HashMap<EntityId, StaticArtifactSet>,
    snippets: HashMap<EntityId, Vec<StringSnippet>>,
    accepted: HashMap<EntityId, Vec<HypothesisId>>,
    conflicts: HashMap<EntityId, Vec<ConflictRecordId>>,
    prior: HashMap<EntityId, Option<ArtifactId>>,
    types: HashMap<EntityId, Vec<EntityId>>,
    globals: HashMap<EntityId, Vec<EntityId>>,
}

impl StubStore {
    fn new() -> Self {
        Self {
            entities: HashMap::new(),
            static_arts: HashMap::new(),
            snippets: HashMap::new(),
            accepted: HashMap::new(),
            conflicts: HashMap::new(),
            prior: HashMap::new(),
            types: HashMap::new(),
            globals: HashMap::new(),
        }
    }
}

impl BundleStore for StubStore {
    fn entity_for_work_item(&self, work_item_id: WorkItemId) -> Option<EntityId> {
        self.entities.get(&work_item_id).copied()
    }

    fn static_artifacts(&self, entity_id: EntityId) -> StaticArtifactSet {
        self.static_arts
            .get(&entity_id)
            .cloned()
            .unwrap_or_default()
    }

    fn string_snippets(&self, entity_id: EntityId) -> Vec<StringSnippet> {
        self.snippets.get(&entity_id).cloned().unwrap_or_default()
    }

    fn accepted_hypotheses(&self, entity_id: EntityId) -> Vec<HypothesisId> {
        self.accepted.get(&entity_id).cloned().unwrap_or_default()
    }

    fn unresolved_conflicts(&self, entity_id: EntityId) -> Vec<ConflictRecordId> {
        self.conflicts.get(&entity_id).cloned().unwrap_or_default()
    }

    fn prior_generated_candidate(&self, entity_id: EntityId) -> Option<ArtifactId> {
        self.prior.get(&entity_id).copied().flatten()
    }

    fn relevant_types(&self, entity_id: EntityId) -> Vec<EntityId> {
        self.types.get(&entity_id).cloned().unwrap_or_default()
    }

    fn relevant_globals(&self, entity_id: EntityId) -> Vec<EntityId> {
        self.globals.get(&entity_id).cloned().unwrap_or_default()
    }
}

// -- helpers --

/// Builds a simple work graph: f_b → f_a, f_c → f_a.
fn test_graph() -> (WorkGraph, WorkItemId, WorkItemId, WorkItemId) {
    let wid_a = WorkItemId::new();
    let wid_b = WorkItemId::new();
    let wid_c = WorkItemId::new();
    let eid_a = EntityId::new();

    let mut graph = DiGraph::new();
    let na = graph.add_node(WorkItemNode {
        work_item_id: wid_a,
        kind: WorkItemKind::Function,
        entity_id: Some(eid_a),
    });
    let nb = graph.add_node(WorkItemNode {
        work_item_id: wid_b,
        kind: WorkItemKind::Function,
        entity_id: Some(EntityId::new()),
    });
    let nc = graph.add_node(WorkItemNode {
        work_item_id: wid_c,
        kind: WorkItemKind::Function,
        entity_id: Some(EntityId::new()),
    });

    // f_b calls f_a (incoming edge to a).
    graph.add_edge(nb, na, DependencyEdgeKind::DirectCall);
    // f_c calls f_a (incoming edge to a).
    graph.add_edge(nc, na, DependencyEdgeKind::DirectCall);

    let mut entity_to_node = HashMap::new();
    entity_to_node.insert(eid_a, na);

    let mut work_item_to_node = HashMap::new();
    work_item_to_node.insert(wid_a, na);
    work_item_to_node.insert(wid_b, nb);
    work_item_to_node.insert(wid_c, nc);

    let wg = WorkGraph {
        graph,
        entity_to_node,
        work_item_to_node,
    };
    (wg, wid_a, wid_b, wid_c)
}

fn test_store(wid_a: WorkItemId, eid_a: EntityId) -> StubStore {
    let mut store = StubStore::new();
    store.entities.insert(wid_a, eid_a);
    store.static_arts.insert(
        eid_a,
        StaticArtifactSet {
            structural_snapshot: Some(ArtifactId::new()),
            decompilation: Some(ArtifactId::new()),
            disassembly: Some(ArtifactId::new()),
            cfg_summary: None,
        },
    );
    store.snippets.insert(
        eid_a,
        vec![StringSnippet {
            value: "hello".into(),
            context: "string ref at 0x401000".into(),
        }],
    );
    store.accepted.insert(eid_a, vec![HypothesisId::new()]);
    store.types.insert(eid_a, vec![EntityId::new()]);
    store.globals.insert(eid_a, vec![EntityId::new()]);
    store
}

fn test_bundle() -> InvestigationBundle {
    let (graph, wid_a, _wid_b, _wid_c) = test_graph();
    let eid_a = graph.graph[graph.work_item_to_node[&wid_a]]
        .entity_id
        .unwrap();
    let store = test_store(wid_a, eid_a);
    let builder = BundleBuilder::new(&graph, &store);
    let schema = response_schema_for("llm.analysis.function").unwrap();
    builder.build(wid_a, schema)
}

// -- tests --

#[test]
fn bundle_serializes_via_serde_json() {
    let bundle = test_bundle();
    let json_bytes = serde_json::to_vec(&bundle).expect("serialize");
    let deserialized: InvestigationBundle =
        serde_json::from_slice(&json_bytes).expect("deserialize");
    assert_eq!(bundle, deserialized);
}

#[test]
fn bundle_excludes_artifact_bytes() {
    let bundle = test_bundle();
    let value: Value = serde_json::to_value(&bundle).unwrap();

    // Each artifact field must be a string (UUID), not bytes/array/object.
    for field in &[
        "static_structural_snapshot",
        "decompilation_artifact",
        "disassembly_artifact",
        "cfg_summary",
        "prior_generated_candidate",
    ] {
        let v = value.get(field).expect("field present");
        // Must be string or null — never an array of bytes.
        assert!(
            v.is_string() || v.is_null(),
            "{field} must be ArtifactId string or null, got {v}"
        );
    }

    // dynamic_observations is an array of ArtifactId strings.
    let dyn_obs = value.get("dynamic_observations").unwrap();
    assert!(dyn_obs.is_array());
    for item in dyn_obs.as_array().unwrap() {
        assert!(
            item.is_string(),
            "dynamic_observations items must be ArtifactId strings"
        );
    }
}

#[test]
fn bundle_only_relevant_calls_in_callers_list() {
    let (graph, wid_a, wid_b, wid_c) = test_graph();
    let eid_a = graph.graph[graph.work_item_to_node[&wid_a]]
        .entity_id
        .unwrap();
    let store = test_store(wid_a, eid_a);
    let builder = BundleBuilder::new(&graph, &store);
    let schema = response_schema_for("llm.analysis.function").unwrap();
    let bundle = builder.build(wid_a, schema);

    // f_a has callers f_b and f_c (incoming edges).
    let caller_ids: Vec<WorkItemId> = bundle
        .callers_and_callees
        .iter()
        .filter(|cs| cs.brief.starts_with("caller"))
        .map(|cs| cs.work_item_id)
        .collect();
    assert!(caller_ids.contains(&wid_b), "f_b must be a caller of f_a");
    assert!(caller_ids.contains(&wid_c), "f_c must be a caller of f_a");

    // No irrelevant callees should appear — there are no outgoing edges from f_a.
    let callee_ids: Vec<WorkItemId> = bundle
        .callers_and_callees
        .iter()
        .filter(|cs| cs.brief.starts_with("callee"))
        .map(|cs| cs.work_item_id)
        .collect();
    assert!(
        callee_ids.is_empty(),
        "f_a has no callees in this graph; got {callee_ids:?}"
    );
}

#[test]
fn function_analysis_schema_rejects_missing_required_field() {
    let schema = response_schema_for("llm.analysis.function").unwrap();

    // Missing required "proposed_name" field.
    let bad_response = json!({
        "behavior_claims": ["does something"],
        "evidence_references": ["ref1"],
        "confidence": 0.8,
        "recommended_follow_up_work": ["more analysis"]
    });

    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    let result = validator.validate(&bad_response);
    assert!(
        result.is_err(),
        "schema must reject response missing required 'proposed_name'"
    );
}

#[test]
fn bundle_size_bounded() {
    let bundle = test_bundle();
    let size = bundle.byte_size_estimate();
    assert!(
        size <= BUNDLE_MAX_BYTES,
        "bundle size {size} exceeds bound {BUNDLE_MAX_BYTES}"
    );
}

#[test]
fn request_payload_matches_capability_schema() {
    let bundle = test_bundle();
    let payload_bytes = request_payload_for(&bundle);
    let payload: Value = serde_json::from_slice(&payload_bytes).expect("valid JSON");

    // Validate the payload against the request schema.
    validate_request_payload(&payload).expect("payload must validate against request schema");

    // The requested_output_schema must be a valid JSON Schema object.
    let output_schema = payload.get("requested_output_schema").unwrap();
    assert!(output_schema.is_object(), "output schema must be an object");

    // Verify the embedded schema itself compiles.
    jsonschema::validator_for(output_schema).expect("embedded output schema compiles");
}

#[test]
fn all_schemas_are_valid_json_schema_2020_12() {
    for cap_id in CAPABILITIES {
        let schema = response_schema_for(cap_id).expect("schema defined");
        jsonschema::validator_for(&schema)
            .unwrap_or_else(|e| panic!("schema for {cap_id} invalid: {e}"));
    }

    let req_schema = request_schema();
    jsonschema::validator_for(&req_schema).expect("request schema is valid");
}

#[test]
fn validate_response_payload_round_trip() {
    let good = json!({
        "proposed_name": "parse_input",
        "behavior_claims": ["parses a buffer"],
        "evidence_references": ["ref1"],
        "confidence": 0.9,
        "recommended_follow_up_work": ["check callers"]
    });
    validate_response_payload("llm.analysis.function", &good).expect("valid response passes");
    let bad = json!({ "behavior_claims": ["no proposed_name"] });
    let result = validate_response_payload("llm.analysis.function", &bad);
    assert!(result.is_err(), "missing required fields must be rejected");
}
