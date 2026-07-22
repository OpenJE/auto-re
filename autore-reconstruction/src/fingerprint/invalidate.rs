//! Bounded downstream invalidation through `GeneratedDeclRequirement` and
//! `BuildDependency` edges only.
//!
//! When a work item's fingerprint changes, the propagator walks downstream
//! dependents reachable via propagation edges, recomputes their
//! fingerprints, and issues [`ApplicationCommand::InvalidateWorkItem`] for
//! every downstream item whose fingerprint also changed.  Propagation
//! stops as soon as a downstream item's fingerprint is unchanged.

use std::collections::{HashMap, HashSet, VecDeque};

use autore_app::application_service::requests::InvalidateWorkItemRequest;
use autore_app::{ApplicationCommand, AutoReClient};
use autore_schema::domain::ContentHash;
use autore_schema::ids::{ProjectId, WorkItemId};
use petgraph::Direction;
use petgraph::visit::EdgeRef;

use super::compute::{FingerprintInput, compute_fingerprint};
use crate::work_graph::{DependencyEdgeKind, WorkGraph};

/// Snapshot of stored fingerprints for a set of work items.
pub trait FingerprintSnapshot {
    fn get_fingerprint(&self, work_item_id: &WorkItemId) -> Option<ContentHash>;
    fn get_input(&self, work_item_id: &WorkItemId) -> Option<FingerprintInput>;
}

/// In-memory `HashMap`-backed fingerprint snapshot.
#[derive(Debug, Default)]
pub struct InMemorySnapshot {
    fingerprints: HashMap<WorkItemId, ContentHash>,
    inputs: HashMap<WorkItemId, FingerprintInput>,
}

impl InMemorySnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a fingerprint together with its input for later lookup.
    pub fn insert(
        &mut self,
        work_item_id: WorkItemId,
        input: FingerprintInput,
        fingerprint: ContentHash,
    ) {
        self.fingerprints.insert(work_item_id, fingerprint);
        self.inputs.insert(work_item_id, input);
    }
}

impl FingerprintSnapshot for InMemorySnapshot {
    fn get_fingerprint(&self, work_item_id: &WorkItemId) -> Option<ContentHash> {
        self.fingerprints.get(work_item_id).cloned()
    }

    fn get_input(&self, work_item_id: &WorkItemId) -> Option<FingerprintInput> {
        self.inputs.get(work_item_id).cloned()
    }
}

/// Propagates invalidation through `GeneratedDeclRequirement` and
/// `BuildDependency` edges only.
pub struct InvalidationPropagator<'a> {
    client: &'a dyn AutoReClient,
    project: ProjectId,
}

impl<'a> InvalidationPropagator<'a> {
    pub fn new(client: &'a dyn AutoReClient, project: ProjectId) -> Self {
        Self { client, project }
    }

    /// Propagates downstream invalidation starting from `changed_work_item_id`.
    ///
    /// Returns the list of work-item IDs that were invalidated.
    pub fn propagate(
        &self,
        changed_work_item_id: &WorkItemId,
        graph: &WorkGraph,
        snapshot: &dyn FingerprintSnapshot,
    ) -> autore_core::Result<Vec<WorkItemId>> {
        let mut invalidated = Vec::new();
        let mut visited: HashSet<WorkItemId> = HashSet::new();
        let mut queue = VecDeque::new();

        for id in downstream_via_propagation_edges(changed_work_item_id, graph) {
            if visited.insert(id) {
                queue.push_back(id);
            }
        }

        while let Some(current) = queue.pop_front() {
            let input = match snapshot.get_input(&current) {
                Some(input) => input,
                None => continue,
            };
            let recomputed = compute_fingerprint(&input);
            let stored = snapshot.get_fingerprint(&current);

            let changed = match stored {
                Some(ref s) => *s != recomputed,
                None => true,
            };

            if changed {
                issue_invalidate(self.client, self.project, &current)?;
                invalidated.push(current);

                for id in downstream_via_propagation_edges(&current, graph) {
                    if visited.insert(id) {
                        queue.push_back(id);
                    }
                }
            }
        }

        Ok(invalidated)
    }
}

fn downstream_via_propagation_edges(
    work_item_id: &WorkItemId,
    graph: &WorkGraph,
) -> Vec<WorkItemId> {
    let node_index = match graph.work_item_to_node.get(work_item_id) {
        Some(idx) => *idx,
        None => return Vec::new(),
    };

    graph
        .graph
        .edges_directed(node_index, Direction::Incoming)
        .filter(|e| {
            matches!(
                e.weight(),
                DependencyEdgeKind::GeneratedDeclRequirement | DependencyEdgeKind::BuildDependency
            )
        })
        .map(|e| graph.graph[e.source()].work_item_id)
        .collect()
}

fn issue_invalidate(
    client: &dyn AutoReClient,
    project: ProjectId,
    work_item_id: &WorkItemId,
) -> autore_core::Result<()> {
    let req = InvalidateWorkItemRequest {
        project,
        work_item_id: work_item_id.to_string(),
        reason: "upstream_fingerprint_changed".into(),
    };
    client.execute(ApplicationCommand::InvalidateWorkItem(req))?;
    Ok(())
}
