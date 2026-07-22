//! [`WorkGraphBuilder`] — constructs a [`WorkGraph`] from semantic entities
//! and dependency edges, collapsing recursive call cycles into
//! `FunctionCluster` nodes via Tarjan/Kosaraju SCC detection.

use std::collections::{HashMap, HashSet};

use autore_app::application_service::requests::{
    CreateWorkItemsRequest, RecordWorkDependencyRequest,
};
use autore_app::{ApplicationCommand, AutoReClient, CommandResult};
use autore_core::Error;
use autore_schema::domain::records::{SemanticEntity, WorkItemKind};
use autore_schema::ids::{
    BinaryRevisionId, EntityId, ProjectId, ReconstructionCampaignId, WorkItemId,
};
use petgraph::Direction;
use petgraph::algo::kosaraju_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use super::graph::{WorkGraph, WorkItemNode};
use super::kind::{DependencyEdgeKind, work_item_kind_for_entity_kind};

/// A work-item specification collected before creation.
type WorkItemSpec = (Option<EntityId>, WorkItemKind, String);

/// The initial graph triple returned by `build_initial_graph`.
type InitialGraph = (
    DiGraph<WorkItemNode, DependencyEdgeKind>,
    HashMap<EntityId, NodeIndex>,
    HashMap<WorkItemId, NodeIndex>,
);

/// Builds a [`WorkGraph`] from a set of semantic entities and their
/// dependency edges.
///
/// All mutations (work-item creation, dependency recording) go through
/// the supplied [`AutoReClient`] as [`ApplicationCommand`]s — no direct
/// storage access.
pub struct WorkGraphBuilder;

impl WorkGraphBuilder {
    /// Constructs a work graph for the given campaign.
    ///
    /// # Errors
    ///
    /// Returns `ValidationError` if a strongly-connected component contains
    /// mixed work-item kinds (e.g., Function + Vtable in the same cycle).
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        client: &dyn AutoReClient,
        project_id: ProjectId,
        campaign_id: ReconstructionCampaignId,
        _binary_revision_id: BinaryRevisionId,
        entities: &[SemanticEntity],
        edges: &[(EntityId, EntityId, DependencyEdgeKind)],
    ) -> autore_core::Result<WorkGraph> {
        let (items, _entity_to_kind) = Self::collect_work_items(entities);
        let ids = Self::issue_create(client, project_id, &campaign_id, &items)?;
        let (mut graph, entity_to_node, mut wid_to_node) =
            Self::build_initial_graph(&items, &ids, edges)?;

        Self::collapse_sccs(
            client,
            project_id,
            &campaign_id,
            &mut graph,
            &mut wid_to_node,
        )?;

        Self::record_dependencies(client, project_id, &graph)?;

        Ok(WorkGraph {
            graph,
            entity_to_node,
            work_item_to_node: wid_to_node,
        })
    }

    // -- Phase 1: collect work items to create --

    fn collect_work_items(
        entities: &[SemanticEntity],
    ) -> (Vec<WorkItemSpec>, HashMap<EntityId, WorkItemKind>) {
        let mut items: Vec<WorkItemSpec> = Vec::new();
        let mut entity_kind_map: HashMap<EntityId, WorkItemKind> = HashMap::new();

        // ProgramSkeleton singleton
        items.push((
            None,
            WorkItemKind::ProgramSkeleton,
            "program-skeleton".into(),
        ));

        for entity in entities {
            if let Some(kind) = work_item_kind_for_entity_kind(&entity.kind) {
                let desc = format!(
                    "{}:{}",
                    kind,
                    entity
                        .display_name
                        .as_deref()
                        .unwrap_or(&entity.id.to_string())
                );
                entity_kind_map.insert(entity.id, kind.clone());
                items.push((Some(entity.id), kind, desc));
            }
        }

        (items, entity_kind_map)
    }

    // -- Phase 1b: issue CreateWorkItems command --

    fn issue_create(
        client: &dyn AutoReClient,
        project_id: ProjectId,
        campaign_id: &ReconstructionCampaignId,
        items: &[WorkItemSpec],
    ) -> autore_core::Result<Vec<WorkItemId>> {
        let descriptions: Vec<String> = items.iter().map(|(_, _, d)| d.clone()).collect();
        let cmd = ApplicationCommand::CreateWorkItems(CreateWorkItemsRequest {
            project: project_id,
            campaign_id: campaign_id.to_string(),
            descriptions,
        });
        let result = client.execute(cmd)?;
        let string_ids = match result {
            CommandResult::WorkItemsCreated(resp) => resp.work_item_ids,
            _ => {
                return Err(Error::Validation(
                    "unexpected CreateWorkItems result".into(),
                ));
            }
        };
        string_ids
            .iter()
            .map(|s| {
                let uuid =
                    uuid::Uuid::parse_str(s).map_err(|e| Error::Validation(e.to_string()))?;
                Ok(WorkItemId::from_uuid(uuid))
            })
            .collect()
    }

    // -- Phase 2: build petgraph from created work items --

    fn build_initial_graph(
        items: &[WorkItemSpec],
        ids: &[WorkItemId],
        edges: &[(EntityId, EntityId, DependencyEdgeKind)],
    ) -> autore_core::Result<InitialGraph> {
        let mut graph = DiGraph::new();
        let mut entity_to_node: HashMap<EntityId, NodeIndex> = HashMap::new();
        let mut wid_to_node: HashMap<WorkItemId, NodeIndex> = HashMap::new();

        for (i, (entity_id, kind, _)) in items.iter().enumerate() {
            let wid = ids[i];
            let node = graph.add_node(WorkItemNode {
                work_item_id: wid,
                kind: kind.clone(),
                entity_id: *entity_id,
            });
            wid_to_node.insert(wid, node);
            if let Some(eid) = entity_id {
                entity_to_node.insert(*eid, node);
            }
        }

        for (from_eid, to_eid, edge_kind) in edges {
            if let (Some(&from_ni), Some(&to_ni)) =
                (entity_to_node.get(from_eid), entity_to_node.get(to_eid))
            {
                graph.add_edge(from_ni, to_ni, *edge_kind);
            }
        }

        Ok((graph, entity_to_node, wid_to_node))
    }

    // -- Phase 3+4: SCC detection, validation, and FunctionCluster collapse --

    fn collapse_sccs(
        client: &dyn AutoReClient,
        project_id: ProjectId,
        campaign_id: &ReconstructionCampaignId,
        graph: &mut DiGraph<WorkItemNode, DependencyEdgeKind>,
        wid_to_node: &mut HashMap<WorkItemId, NodeIndex>,
    ) -> autore_core::Result<()> {
        let sccs = kosaraju_scc(&*graph);
        let mut to_collapse: Vec<Vec<NodeIndex>> = Vec::new();

        for scc in &sccs {
            if scc.len() < 2 {
                let dominated_by_self_loop = scc.len() == 1
                    && graph
                        .edges_directed(scc[0], Direction::Outgoing)
                        .any(|e| e.target() == scc[0]);
                if !dominated_by_self_loop {
                    continue;
                }
            }

            let kinds: HashSet<WorkItemKind> =
                scc.iter().map(|&ni| graph[ni].kind.clone()).collect();
            if kinds.len() > 1 {
                return Err(Error::Validation(format!("mixed-kind SCC: {kinds:?}")));
            }

            let kind = kinds.into_iter().next().unwrap();
            if kind == WorkItemKind::Function {
                to_collapse.push(scc.clone());
            }
        }

        if to_collapse.is_empty() {
            return Ok(());
        }

        let cluster_descs: Vec<String> = to_collapse
            .iter()
            .map(|members| format!("function-cluster-{}-members", members.len()))
            .collect();

        let cluster_ids =
            Self::issue_create_from_descs(client, project_id, campaign_id, &cluster_descs)?;

        for (i, member_indices) in to_collapse.iter().enumerate() {
            let cluster_wid = cluster_ids[i];
            let cluster_ni = graph.add_node(WorkItemNode {
                work_item_id: cluster_wid,
                kind: WorkItemKind::FunctionCluster,
                entity_id: None,
            });
            wid_to_node.insert(cluster_wid, cluster_ni);

            let member_set: HashSet<NodeIndex> = member_indices.iter().copied().collect();
            let mut outgoing: Vec<(NodeIndex, DependencyEdgeKind)> = Vec::new();

            for &member_ni in member_indices {
                graph.add_edge(cluster_ni, member_ni, DependencyEdgeKind::ClusterMember);
                for edge in graph.edges_directed(member_ni, Direction::Outgoing) {
                    if !member_set.contains(&edge.target()) {
                        outgoing.push((edge.target(), *edge.weight()));
                    }
                }
            }

            let mut seen: HashSet<(NodeIndex, DependencyEdgeKind)> = HashSet::new();
            for (target, kind) in outgoing {
                if seen.insert((target, kind)) {
                    graph.add_edge(cluster_ni, target, kind);
                }
            }
        }

        Ok(())
    }

    /// Issues a `CreateWorkItems` command and parses the response IDs.
    fn issue_create_from_descs(
        client: &dyn AutoReClient,
        project_id: ProjectId,
        campaign_id: &ReconstructionCampaignId,
        descriptions: &[String],
    ) -> autore_core::Result<Vec<WorkItemId>> {
        let cmd = ApplicationCommand::CreateWorkItems(CreateWorkItemsRequest {
            project: project_id,
            campaign_id: campaign_id.to_string(),
            descriptions: descriptions.to_vec(),
        });
        let result = client.execute(cmd)?;
        let string_ids = match result {
            CommandResult::WorkItemsCreated(resp) => resp.work_item_ids,
            _ => {
                return Err(Error::Validation(
                    "unexpected CreateWorkItems result".into(),
                ));
            }
        };
        string_ids
            .iter()
            .map(|s| {
                let uuid =
                    uuid::Uuid::parse_str(s).map_err(|e| Error::Validation(e.to_string()))?;
                Ok(WorkItemId::from_uuid(uuid))
            })
            .collect()
    }

    // -- Phase 5: record all dependency edges via commands --

    fn record_dependencies(
        client: &dyn AutoReClient,
        project_id: ProjectId,
        graph: &DiGraph<WorkItemNode, DependencyEdgeKind>,
    ) -> autore_core::Result<()> {
        for edge in graph.edge_references() {
            let source_wid = graph[edge.source()].work_item_id;
            let target_wid = graph[edge.target()].work_item_id;
            let cmd = ApplicationCommand::RecordWorkDependency(RecordWorkDependencyRequest {
                project: project_id,
                work_item_id: source_wid.to_string(),
                depends_on: target_wid.to_string(),
            });
            client.execute(cmd)?;
        }
        Ok(())
    }
}
