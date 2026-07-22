//! Regression selection on dependency change (spec §13.5).
//!
//! For every accepted implementation we retain a bounded regression set:
//! the scenarios that verified it, the fingerprints of its transitive
//! accepted hypotheses, the supplied types it uses, and the build profiles
//! under which it was verified.  When a dependency changes we walk the
//! work-dependency graph and issue [`ApplicationCommand::ScheduleVerificationRegression`]
//! for each affected entity.

use std::collections::{HashMap, HashSet};

use autore_app::application_service::requests::ScheduleVerificationRegressionRequest;
use autore_app::{ApplicationCommand, AutoReClient, CommandResult};
use autore_core::{Error, Result};
use autore_schema::domain::ContentHash;
use autore_schema::ids::{EntityId, ProjectId};
use petgraph::Direction;
use petgraph::visit::EdgeRef;

use crate::work_graph::{DependencyEdgeKind, WorkGraph};

/// Default cap on the number of scenarios retained per entity.
pub const DEFAULT_MAX_REGRESSION_SCENARIOS: usize = 100;

/// Regression set retained for a single verified entity.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RegressionSet {
    /// Scenario IDs that verified this entity.
    pub scenarios: Vec<String>,
    /// Fingerprint of each transitive accepted hypothesis dependency keyed
    /// by dependency identifier (e.g. hypothesis id or entity id).
    pub dependency_fingerprints: HashMap<String, ContentHash>,
    /// Type names/identifiers used by this entity.
    pub affected_types: Vec<String>,
    /// Build profile identifiers under which this entity was verified.
    pub build_profiles: Vec<String>,
}

impl RegressionSet {
    /// Creates an empty regression set.
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Tracks regression sets for verified entities and schedules re-verification
/// when their dependencies change.
#[derive(Debug, Clone)]
pub struct RegressionTracker {
    sets: HashMap<EntityId, RegressionSet>,
    max_scenarios: usize,
}

impl RegressionTracker {
    /// Creates a tracker with the default scenario cap.
    pub fn new() -> Self {
        Self::with_max_scenarios(DEFAULT_MAX_REGRESSION_SCENARIOS)
    }

    /// Creates a tracker with a custom scenario cap.
    pub fn with_max_scenarios(max_scenarios: usize) -> Self {
        Self {
            sets: HashMap::new(),
            max_scenarios,
        }
    }

    /// Returns the number of tracked regression sets.
    pub fn len(&self) -> usize {
        self.sets.len()
    }

    /// Returns `true` if no regression sets are tracked.
    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// Returns the regression set for an entity, if any.
    pub fn get(&self, entity_id: &EntityId) -> Option<&RegressionSet> {
        self.sets.get(entity_id)
    }

    /// Records or replaces the regression set for an entity that has just
    /// been verified.
    ///
    /// `scenario_ids` are capped to `max_scenarios` to enforce the explicit
    /// cost bound from spec §13.5.
    pub fn register_verification(
        &mut self,
        entity_id: EntityId,
        scenario_ids: Vec<String>,
        dependency_fingerprints: HashMap<String, ContentHash>,
        affected_types: Vec<String>,
        build_profile: String,
    ) {
        let mut scenarios = scenario_ids;
        scenarios.truncate(self.max_scenarios);

        let build_profiles = if build_profile.is_empty() {
            Vec::new()
        } else {
            vec![build_profile]
        };

        self.sets.insert(
            entity_id,
            RegressionSet {
                scenarios,
                dependency_fingerprints,
                affected_types,
                build_profiles,
            },
        );
    }

    /// Computes the set of tracked entities that need regression when
    /// `changed_entity_id` changes.
    ///
    /// Walks `work_dependencies` edges of kind [`DependencyEdgeKind::BuildDependency`]
    /// or [`DependencyEdgeKind::VerificationDependency`] from the changed entity
    /// to its dependents.  Only dependents with a recorded regression set are
    /// returned, because only those have scenarios that can be re-run.
    pub fn compute_affected_entities(
        &self,
        changed_entity_id: EntityId,
        dependency_graph: &WorkGraph,
    ) -> Vec<EntityId> {
        let node_index = match dependency_graph.entity_to_node.get(&changed_entity_id) {
            Some(idx) => *idx,
            None => return Vec::new(),
        };

        let mut affected = HashSet::new();
        for edge in dependency_graph
            .graph
            .edges_directed(node_index, Direction::Incoming)
        {
            if !is_regression_edge_kind(*edge.weight()) {
                continue;
            }
            let source = edge.source();
            if let Some(entity_id) = dependency_graph.graph[source].entity_id
                && self.sets.contains_key(&entity_id)
            {
                affected.insert(entity_id);
            }
        }

        affected.into_iter().collect()
    }

    /// Issues [`ApplicationCommand::ScheduleVerificationRegression`] for each
    /// affected entity.
    pub fn schedule_regressions(
        &self,
        client: &dyn AutoReClient,
        project: ProjectId,
        affected_entities: &[EntityId],
    ) -> Result<Vec<String>> {
        let mut regression_ids = Vec::with_capacity(affected_entities.len());
        for entity_id in affected_entities {
            let req = ScheduleVerificationRegressionRequest {
                project,
                entity_id: entity_id.to_string(),
            };
            let result = client.execute(ApplicationCommand::ScheduleVerificationRegression(req))?;
            let id = match result {
                CommandResult::VerificationRegressionScheduled(resp) => resp.regression_id,
                other => {
                    return Err(Error::Validation(format!(
                        "unexpected result for ScheduleVerificationRegression: {other:?}"
                    )));
                }
            };
            regression_ids.push(id);
        }
        Ok(regression_ids)
    }
}

impl Default for RegressionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` if the given edge kind should be considered for regression
/// selection of affected dependents.
///
/// Per spec §13.5 only build and verification dependency edges trigger
/// regression of dependents.
pub fn is_regression_edge_kind(kind: DependencyEdgeKind) -> bool {
    matches!(
        kind,
        DependencyEdgeKind::BuildDependency | DependencyEdgeKind::VerificationDependency
    )
}

/// Returns `true` if the given edge kind contributes to the regression
/// fingerprint/type dependency set.
///
/// Per spec §13.5 the explicit cost-bound limits regression dependency
/// tracking to declaration-requirement and verification edges.
pub fn is_regression_fingerprint_edge_kind(kind: DependencyEdgeKind) -> bool {
    matches!(
        kind,
        DependencyEdgeKind::GeneratedDeclRequirement | DependencyEdgeKind::VerificationDependency
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use autore_app::ApplicationCommand;
    use autore_schema::domain::ContentHash;
    use autore_schema::domain::records::WorkItemKind;
    use autore_schema::ids::{EntityId, ProjectId, WorkItemId};
    use petgraph::graph::DiGraph;

    use crate::tests_support::RecordingAutoReClient;
    use crate::work_graph::{DependencyEdgeKind, WorkGraph, WorkItemNode};

    use super::{
        DEFAULT_MAX_REGRESSION_SCENARIOS, RegressionTracker, is_regression_edge_kind,
        is_regression_fingerprint_edge_kind,
    };

    fn work_item_node(id: WorkItemId, entity_id: Option<EntityId>) -> WorkItemNode {
        WorkItemNode {
            work_item_id: id,
            kind: WorkItemKind::Function,
            entity_id,
        }
    }

    /// Build a `WorkGraph` from a list of entities and `(source_index,
    /// target_index, kind)` edge triples.  Edges point from dependent to
    /// dependency.
    fn build_graph_with_entities(
        entities: &[EntityId],
        edges: &[(usize, usize, DependencyEdgeKind)],
    ) -> WorkGraph {
        let mut graph = DiGraph::new();
        let mut entity_to_node = HashMap::new();
        let mut work_item_to_node = HashMap::new();

        for entity_id in entities {
            let work_item_id = WorkItemId::new();
            let idx = graph.add_node(work_item_node(work_item_id, Some(*entity_id)));
            entity_to_node.insert(*entity_id, idx);
            work_item_to_node.insert(work_item_id, idx);
        }

        for (src, tgt, kind) in edges {
            let src_idx = entity_to_node[&entities[*src]];
            let tgt_idx = entity_to_node[&entities[*tgt]];
            graph.add_edge(src_idx, tgt_idx, *kind);
        }

        WorkGraph {
            graph,
            entity_to_node,
            work_item_to_node,
        }
    }

    #[test]
    fn change_to_shared_type_modifies_regression_set_of_dependent_entities() {
        let shared_type = EntityId::new();
        let dependent = EntityId::new();

        let graph = build_graph_with_entities(
            &[shared_type, dependent],
            &[(1, 0, DependencyEdgeKind::VerificationDependency)],
        );

        let mut tracker = RegressionTracker::new();
        tracker.register_verification(
            dependent,
            vec!["s1".into()],
            HashMap::new(),
            vec!["shared_type".into()],
            "debug".into(),
        );

        let affected = tracker.compute_affected_entities(shared_type, &graph);
        assert_eq!(affected.len(), 1);
        assert!(affected.contains(&dependent));
    }

    #[test]
    fn stale_fp_of_dependencies_triggers_regression() {
        let callee = EntityId::new();
        let caller = EntityId::new();

        let graph = build_graph_with_entities(
            &[callee, caller],
            &[(1, 0, DependencyEdgeKind::BuildDependency)],
        );

        let mut tracker = RegressionTracker::new();
        let mut fingerprints = HashMap::new();
        fingerprints.insert(callee.to_string(), ContentHash::from_bytes(b"old-fp"));
        tracker.register_verification(
            caller,
            vec!["s1".into()],
            fingerprints,
            vec![],
            "release".into(),
        );

        // When the callee's fingerprint becomes stale (detected elsewhere via
        // invalidation), the caller is reachable through the BuildDependency
        // edge and should be regressed.
        let affected = tracker.compute_affected_entities(callee, &graph);
        assert_eq!(affected.len(), 1);
        assert!(affected.contains(&caller));
    }

    #[test]
    fn regression_contains_only_specific_callee_scenarios() {
        let entity_id = EntityId::new();
        let mut tracker = RegressionTracker::new();
        tracker.register_verification(
            entity_id,
            vec!["scenario-a".into(), "scenario-b".into()],
            HashMap::new(),
            vec![],
            "debug".into(),
        );

        let set = tracker.get(&entity_id).expect("regression set present");
        assert_eq!(set.scenarios, vec!["scenario-a", "scenario-b"]);
        assert_eq!(set.build_profiles, vec!["debug"]);
    }

    #[test]
    fn non_build_verification_dep_not_in_regression_set() {
        let shared = EntityId::new();
        let dependent = EntityId::new();

        let graph = build_graph_with_entities(
            &[shared, dependent],
            &[(1, 0, DependencyEdgeKind::DirectCall)],
        );

        let mut tracker = RegressionTracker::new();
        tracker.register_verification(
            dependent,
            vec!["s1".into()],
            HashMap::new(),
            vec![],
            "debug".into(),
        );

        let affected = tracker.compute_affected_entities(shared, &graph);
        assert!(
            affected.is_empty(),
            "DirectCall must not trigger regression selection"
        );
    }

    #[test]
    fn schedule_regressions_issues_commands() {
        let entity_id = EntityId::new();
        let client = RecordingAutoReClient::new();
        let tracker = RegressionTracker::new();

        let ids = tracker
            .schedule_regressions(&client, ProjectId::new(), &[entity_id])
            .expect("schedule succeeds");

        assert_eq!(ids.len(), 1);
        let commands = client.commands();
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            ApplicationCommand::ScheduleVerificationRegression(req) => {
                assert_eq!(req.entity_id, entity_id.to_string());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn max_scenarios_cost_bound_is_enforced() {
        let entity_id = EntityId::new();
        let mut tracker = RegressionTracker::with_max_scenarios(2);
        let scenarios: Vec<String> = (0..5).map(|i| format!("s{i}")).collect();
        tracker.register_verification(entity_id, scenarios, HashMap::new(), vec![], "debug".into());

        let set = tracker.get(&entity_id).expect("regression set present");
        assert_eq!(set.scenarios.len(), 2);
    }

    #[test]
    fn default_max_scenarios_is_100() {
        assert_eq!(DEFAULT_MAX_REGRESSION_SCENARIOS, 100);
    }

    #[test]
    fn regression_edge_kind_filter() {
        assert!(is_regression_edge_kind(DependencyEdgeKind::BuildDependency));
        assert!(is_regression_edge_kind(
            DependencyEdgeKind::VerificationDependency
        ));
        assert!(!is_regression_edge_kind(DependencyEdgeKind::DirectCall));
        assert!(!is_regression_edge_kind(
            DependencyEdgeKind::GeneratedDeclRequirement
        ));
    }

    #[test]
    fn regression_fingerprint_edge_kind_filter() {
        assert!(is_regression_fingerprint_edge_kind(
            DependencyEdgeKind::GeneratedDeclRequirement
        ));
        assert!(is_regression_fingerprint_edge_kind(
            DependencyEdgeKind::VerificationDependency
        ));
        assert!(!is_regression_fingerprint_edge_kind(
            DependencyEdgeKind::BuildDependency
        ));
        assert!(!is_regression_fingerprint_edge_kind(
            DependencyEdgeKind::DirectCall
        ));
    }
}
