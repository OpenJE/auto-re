//! Work-graph data structure: nodes are work-item IDs, edges carry
//! [`DependencyEdgeKind`].

use std::collections::HashMap;

use autore_schema::domain::records::WorkItemKind;
use autore_schema::ids::{EntityId, WorkItemId};
use petgraph::graph::DiGraph;

use super::kind::DependencyEdgeKind;

/// A directed dependency graph of work items for a single reconstruction
/// campaign.
///
/// Nodes are `WorkItemId`s annotated with their `WorkItemKind`. Edges carry
/// a `DependencyEdgeKind` describing why the source depends on the target.
#[derive(Debug, Clone)]
pub struct WorkGraph {
    pub graph: DiGraph<WorkItemNode, DependencyEdgeKind>,
    /// Maps entity IDs to the graph node index for the corresponding work item.
    pub entity_to_node: HashMap<EntityId, petgraph::graph::NodeIndex>,
    /// Maps work-item IDs to graph node indices.
    pub work_item_to_node: HashMap<WorkItemId, petgraph::graph::NodeIndex>,
}

/// A node in the work graph.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkItemNode {
    pub work_item_id: WorkItemId,
    pub kind: WorkItemKind,
    /// The entity this work item was created from, if any.
    /// `None` for synthetic nodes like `ProgramSkeleton`.
    pub entity_id: Option<EntityId>,
}

impl WorkGraph {
    /// Returns the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Returns the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Returns all nodes of the given kind.
    pub fn nodes_of_kind(&self, kind: &WorkItemKind) -> Vec<&WorkItemNode> {
        self.graph
            .node_weights()
            .filter(|n| &n.kind == kind)
            .collect()
    }
}
