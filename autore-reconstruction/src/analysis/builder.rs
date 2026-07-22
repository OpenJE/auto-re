//! `BundleBuilder` — assembles an [`InvestigationBundle`] from a
//! [`WorkItemId`], a [`WorkGraph`], and trait-based lightweight stores.
//!
//! The [`BundleStore`] trait abstracts the data source so tests can use
//! in-memory stubs while production code plugs into the real application
//! service.

use autore_schema::ids::{ArtifactId, ConflictRecordId, EntityId, HypothesisId, WorkItemId};
use petgraph::Direction;
use petgraph::visit::EdgeRef;

use crate::analysis::bundle::{CallSiteSummary, InvestigationBundle, StringSnippet};
use crate::work_graph::WorkGraph;

// ---------------------------------------------------------------------------
// BundleStore trait
// ---------------------------------------------------------------------------

/// Read-only data source for bundle assembly.
///
/// Production implementations query the application service; tests provide
/// an in-memory stub. All methods return owned vectors so the builder does
/// not borrow across async boundaries.
pub trait BundleStore {
    /// Returns the `EntityId` associated with a work item, if any.
    fn entity_for_work_item(&self, work_item_id: WorkItemId) -> Option<EntityId>;

    /// Returns artifact handles for static observations of the given entity.
    fn static_artifacts(&self, entity_id: EntityId) -> StaticArtifactSet;

    /// Returns string snippets observed in the entity's scope.
    fn string_snippets(&self, entity_id: EntityId) -> Vec<StringSnippet>;

    /// Returns accepted hypothesis IDs for the entity.
    fn accepted_hypotheses(&self, entity_id: EntityId) -> Vec<HypothesisId>;

    /// Returns unresolved conflict record IDs for the entity.
    fn unresolved_conflicts(&self, entity_id: EntityId) -> Vec<ConflictRecordId>;

    /// Returns the prior generated candidate artifact, if any.
    fn prior_generated_candidate(&self, entity_id: EntityId) -> Option<ArtifactId>;

    /// Returns relevant type entity IDs for the subject.
    fn relevant_types(&self, entity_id: EntityId) -> Vec<EntityId>;

    /// Returns relevant global entity IDs for the subject.
    fn relevant_globals(&self, entity_id: EntityId) -> Vec<EntityId>;
}

/// The set of static-artifact handles for a single entity.
#[derive(Debug, Clone, Default)]
pub struct StaticArtifactSet {
    pub structural_snapshot: Option<ArtifactId>,
    pub decompilation: Option<ArtifactId>,
    pub disassembly: Option<ArtifactId>,
    pub cfg_summary: Option<ArtifactId>,
}

// ---------------------------------------------------------------------------
// BundleBuilder
// ---------------------------------------------------------------------------

/// Assembles an [`InvestigationBundle`] from the work graph and a store.
pub struct BundleBuilder<'a, S: BundleStore> {
    graph: &'a WorkGraph,
    store: &'a S,
}

impl<'a, S: BundleStore> BundleBuilder<'a, S> {
    /// Creates a new builder.
    pub fn new(graph: &'a WorkGraph, store: &'a S) -> Self {
        Self { graph, store }
    }

    /// Builds a bundle for the given subject work item and capability
    /// output schema.
    ///
    /// Walks the work graph to find direct callers and callees of the
    /// subject, then fills the remaining fields from the store.
    pub fn build(
        &self,
        subject: WorkItemId,
        requested_output_schema: serde_json::Value,
    ) -> InvestigationBundle {
        let subject_entity = self.store.entity_for_work_item(subject);
        let callers_and_callees = self.collect_call_sites(subject);

        let (static_arts, snippets, accepted_hyps, conflicts, prior, types, globals) =
            match subject_entity {
                Some(eid) => (
                    self.store.static_artifacts(eid),
                    self.store.string_snippets(eid),
                    self.store.accepted_hypotheses(eid),
                    self.store.unresolved_conflicts(eid),
                    self.store.prior_generated_candidate(eid),
                    self.store.relevant_types(eid),
                    self.store.relevant_globals(eid),
                ),
                None => (
                    StaticArtifactSet::default(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    None,
                    Vec::new(),
                    Vec::new(),
                ),
            };

        InvestigationBundle {
            subject_identity: subject,
            subject_entity_id: subject_entity,
            static_structural_snapshot: static_arts.structural_snapshot,
            decompilation_artifact: static_arts.decompilation,
            disassembly_artifact: static_arts.disassembly,
            cfg_summary: static_arts.cfg_summary,
            callers_and_callees,
            relevant_types: types,
            relevant_globals: globals,
            strings_and_constants: snippets,
            dynamic_observations: Vec::new(),
            accepted_hypotheses: accepted_hyps,
            unresolved_conflicts: conflicts,
            prior_generated_candidate: prior,
            compiler_diagnostics: Vec::new(),
            verification_failures: Vec::new(),
            requested_output_schema,
        }
    }

    /// Collects direct callers and callees of the subject node from the
    /// work graph, producing a brief for each neighbor.
    fn collect_call_sites(&self, subject: WorkItemId) -> Vec<CallSiteSummary> {
        let Some(&node_idx) = self.graph.work_item_to_node.get(&subject) else {
            return Vec::new();
        };

        let mut sites = Vec::new();

        // Outgoing edges = callees (subject depends on them).
        for edge_ref in self
            .graph
            .graph
            .edges_directed(node_idx, Direction::Outgoing)
        {
            let target = self.graph.graph[edge_ref.target()].clone();
            sites.push(CallSiteSummary {
                work_item_id: target.work_item_id,
                brief: format!("callee {}", target.work_item_id),
                edge_kind: *edge_ref.weight(),
            });
        }

        // Incoming edges = callers (they depend on subject).
        for edge_ref in self
            .graph
            .graph
            .edges_directed(node_idx, Direction::Incoming)
        {
            let source = self.graph.graph[edge_ref.source()].clone();
            sites.push(CallSiteSummary {
                work_item_id: source.work_item_id,
                brief: format!("caller {}", source.work_item_id),
                edge_kind: *edge_ref.weight(),
            });
        }

        sites
    }
}
