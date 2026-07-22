//! Dynamic observation canonical importer.
//!
//! Routes a provider-emitted `debug.observation` artifact into the canonical
//! store through [`ApplicationCommand`] only.  After registering the raw bytes
//! as an artifact, recording the observation metadata, and attaching the
//! artifact as verification evidence, the importer recomputes the owning work
//! item's fingerprint and propagates invalidation to downstream work.

use std::path::PathBuf;

use autore_app::application_service::requests::{
    AddEvidenceRequest, CreateWorkItemsRequest, ImportDynamicObservationRequest,
    InvalidateWorkItemRequest, RegisterArtifactRequest,
};
use autore_app::{ApplicationCommand, AutoReClient, CommandResult};
use autore_core::{Error, Result};
use autore_schema::domain::records::{ARTIFACT_KIND_TRACE, EvidenceRecord};
use autore_schema::domain::{
    ContentHash, Derivation, DerivationMethod, EvidenceValue, NamespacedId, Timestamp,
};
use autore_schema::ids::{
    ArtifactId, EntityId, EvidenceRecordId, ProjectId, ProviderRunId, ReconstructionCampaignId,
    WorkItemId,
};

use crate::fingerprint::invalidate::InvalidationPropagator;
use crate::fingerprint::{
    FingerprintComparison, FingerprintSnapshot, compare_fingerprint, compute_fingerprint,
};
use crate::work_graph::WorkGraph;

// ---------------------------------------------------------------------------
// Observation payload types
// ---------------------------------------------------------------------------

/// A contiguous timestamp range `[start, end)` describing when an observation
/// was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimestampRange {
    /// Inclusive start of the observation window.
    pub start: Timestamp,
    /// Exclusive end of the observation window.
    pub end: Timestamp,
}

/// Metadata describing a captured dynamic observation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DynamicObservation {
    /// Namespaced kind of the observation (e.g. `debug.syscall`).
    pub observation_kind: NamespacedId,
    /// The artifact that was captured for this observation.
    pub captured_artifact_id: ArtifactId,
    /// The canonical entity the observation describes.
    pub target_entity_id: EntityId,
    /// Identifier of the scenario that produced the observation.
    pub scenario_id: String,
    /// Time window during which the observation was captured.
    pub timestamp_range: TimestampRange,
    /// Moment the observation was recorded.
    pub recorded_at: Timestamp,
}

/// Input to [`DynamicObservationImporter::import`].
#[derive(Debug, Clone)]
pub struct ObservationImport {
    /// Structured observation metadata.
    pub observation: DynamicObservation,
    /// Raw observation bytes to register as a canonical artifact.
    pub bytes: Vec<u8>,
    /// Optional sequence token provided by the provider. A mismatch against
    /// the expected token is a replay/nondeterminism signal.
    pub sequence_token: Option<String>,
    /// Explicit replay flag from the provider. When `true`, the importer
    /// spawns an investigation work item.
    pub replay_flag: bool,
}

impl ObservationImport {
    /// Creates an observation import with no replay flag.
    pub fn new(observation: DynamicObservation, bytes: Vec<u8>) -> Self {
        Self {
            observation,
            bytes,
            sequence_token: None,
            replay_flag: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Import summary
// ---------------------------------------------------------------------------

/// Result of a single dynamic observation import.
#[derive(Debug, Clone)]
pub struct ImportSummary {
    /// ID of the registered trace artifact.
    pub artifact_id: ArtifactId,
    /// ID returned by the `ImportDynamicObservation` command.
    pub observation_id: String,
    /// ID of the verification evidence record.
    pub evidence_id: EvidenceRecordId,
    /// Outcome of comparing the recomputed target fingerprint with the stored one.
    pub fingerprint_comparison: FingerprintComparison,
    /// Work items invalidated because their fingerprint changed.
    pub invalidated_work_items: Vec<WorkItemId>,
    /// Investigation work items created for nondeterminism/replay anomalies.
    pub investigations_created: u64,
}

// ---------------------------------------------------------------------------
// Importer
// ---------------------------------------------------------------------------

/// Canonical importer for `debug.observation` artifacts.
pub struct DynamicObservationImporter<'a> {
    snapshot: &'a dyn FingerprintSnapshot,
    graph: &'a WorkGraph,
}

impl<'a> DynamicObservationImporter<'a> {
    /// Creates an importer bound to a fingerprint snapshot and work graph.
    pub fn new(snapshot: &'a dyn FingerprintSnapshot, graph: &'a WorkGraph) -> Self {
        Self { snapshot, graph }
    }

    /// Imports a dynamic observation into the canonical store.
    ///
    /// All mutations are issued as [`ApplicationCommand`]s through `client`.
    /// The import is conceptually atomic: commands are issued in order and
    /// real atomicity is enforced by the `autore-app` command handlers.
    pub fn import(
        &self,
        observation: &ObservationImport,
        client: &dyn AutoReClient,
        project_id: ProjectId,
        campaign_id: ReconstructionCampaignId,
        run_id: ProviderRunId,
    ) -> Result<ImportSummary> {
        let mut summary = ImportSummary {
            artifact_id: ArtifactId::new(),
            observation_id: String::new(),
            evidence_id: EvidenceRecordId::new(),
            fingerprint_comparison: FingerprintComparison::FirstTime,
            invalidated_work_items: Vec::new(),
            investigations_created: 0,
        };

        // 1. Register the observation bytes as a canonical trace artifact.
        let (artifact_id, artifact_hash) =
            self.register_artifact(observation, client, project_id)?;
        summary.artifact_id = artifact_id;

        // 2. Import the observation metadata.  This emits the
        //    `debug.observation-imported` project event via the command event path.
        let observation_id = self.import_observation(observation, client, project_id)?;
        summary.observation_id = observation_id;

        // 3. Add the artifact as verification evidence for the target entity.
        let evidence_id =
            self.add_evidence(observation, client, project_id, run_id, artifact_id)?;
        summary.evidence_id = evidence_id;

        // 4. Recompute the target work item's fingerprint including the new
        //    observation artifact hash.
        if let Some(target_work_item) = self.target_work_item(observation)
            && let Some(base_input) = self.snapshot.get_input(&target_work_item)
        {
            let mut new_input = base_input.clone();
            new_input.dynamic_observations.push(artifact_hash.clone());
            let new_fingerprint = compute_fingerprint(&new_input);
            let stored = self.snapshot.get_fingerprint(&target_work_item);
            let comparison = compare_fingerprint(&new_fingerprint, stored.as_ref());
            summary.fingerprint_comparison = comparison;

            if comparison == FingerprintComparison::Changed {
                // 5. The target work item is stale.
                let req = InvalidateWorkItemRequest {
                    project: project_id,
                    work_item_id: target_work_item.to_string(),
                    reason: "dynamic_observation_changed_fingerprint".into(),
                };
                client.execute(ApplicationCommand::InvalidateWorkItem(req))?;
                summary.invalidated_work_items.push(target_work_item);

                // 6. Propagate invalidation to downstream dependents.
                let propagator = InvalidationPropagator::new(client, project_id);
                let downstream =
                    propagator.propagate(&target_work_item, self.graph, self.snapshot)?;
                summary.invalidated_work_items.extend(downstream);
            }
        }

        // 7. Nondeterminism / replay anomalies spawn an investigation work item.
        if self.is_nondeterministic(observation) {
            self.create_investigation(client, project_id, campaign_id, run_id)?;
            summary.investigations_created += 1;
        }

        Ok(summary)
    }

    fn target_work_item(&self, observation: &ObservationImport) -> Option<WorkItemId> {
        self.graph
            .entity_to_node
            .get(&observation.observation.target_entity_id)
            .map(|idx| self.graph.graph[*idx].work_item_id)
    }

    fn register_artifact(
        &self,
        observation: &ObservationImport,
        client: &dyn AutoReClient,
        project_id: ProjectId,
    ) -> Result<(ArtifactId, ContentHash)> {
        let staging_path = self.write_staging_bytes(&observation.bytes)?;
        let req = RegisterArtifactRequest {
            project: project_id,
            source_path: staging_path,
            kind: ARTIFACT_KIND_TRACE.as_str().to_string(),
        };
        let result = client.execute(ApplicationCommand::RegisterArtifact(req))?;
        match result {
            CommandResult::ArtifactRegistered(resp) => {
                Ok((resp.artifact.id, resp.artifact.content_hash.clone()))
            }
            _ => Err(Error::Validation(
                "unexpected RegisterArtifact result".into(),
            )),
        }
    }

    fn write_staging_bytes(&self, bytes: &[u8]) -> Result<PathBuf> {
        let name = format!("autore-observation-{}.trace", uuid::Uuid::new_v4());
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, bytes).map_err(Error::Io)?;
        Ok(path)
    }

    fn import_observation(
        &self,
        observation: &ObservationImport,
        client: &dyn AutoReClient,
        project_id: ProjectId,
    ) -> Result<String> {
        let payload = serde_json::to_string(&observation.observation)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        let req = ImportDynamicObservationRequest {
            project: project_id,
            observation: payload,
        };
        let result = client.execute(ApplicationCommand::ImportDynamicObservation(req))?;
        match result {
            CommandResult::DynamicObservationImported(resp) => Ok(resp.observation_id),
            _ => Err(Error::Validation(
                "unexpected ImportDynamicObservation result".into(),
            )),
        }
    }

    fn add_evidence(
        &self,
        observation: &ObservationImport,
        client: &dyn AutoReClient,
        project_id: ProjectId,
        run_id: ProviderRunId,
        artifact_id: ArtifactId,
    ) -> Result<EvidenceRecordId> {
        let record = EvidenceRecord {
            id: EvidenceRecordId::new(),
            project: project_id,
            subject: observation.observation.target_entity_id,
            predicate: NamespacedId::parse("evidence.predicate.verification")
                .expect("verification predicate is valid"),
            value: EvidenceValue::Artifact(artifact_id),
            derivation: Derivation::new(
                DerivationMethod::DirectObservation,
                NamespacedId::parse("debug.observation").expect("debug.observation is valid"),
                Vec::new(),
                Vec::new(),
            ),
            provider_run: Some(run_id),
            native_artifacts: Vec::new(),
            assumptions: Vec::new(),
            created_at: Timestamp::now(),
        };
        let req = AddEvidenceRequest {
            project: project_id,
            record,
        };
        let result = client.execute(ApplicationCommand::AddEvidence(req))?;
        match result {
            CommandResult::EvidenceAdded(resp) => Ok(resp.id),
            _ => Err(Error::Validation("unexpected AddEvidence result".into())),
        }
    }

    fn is_nondeterministic(&self, observation: &ObservationImport) -> bool {
        if observation.replay_flag {
            return true;
        }
        if let Some(token) = &observation.sequence_token {
            // A sequence token that differs from the scenario id is treated as
            // a replay / nondeterminism signal.
            return token != &observation.observation.scenario_id;
        }
        false
    }

    fn create_investigation(
        &self,
        client: &dyn AutoReClient,
        project_id: ProjectId,
        campaign_id: ReconstructionCampaignId,
        run_id: ProviderRunId,
    ) -> Result<()> {
        let req = CreateWorkItemsRequest {
            project: project_id,
            campaign_id: campaign_id.to_string(),
            descriptions: vec![format!(
                "investigate nondeterministic dynamic observation from run {run_id}"
            )],
        };
        let result = client.execute(ApplicationCommand::CreateWorkItems(req))?;
        match result {
            CommandResult::WorkItemsCreated(_) => Ok(()),
            _ => Err(Error::Validation(
                "unexpected CreateWorkItems result".into(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use autore_app::ApplicationCommand;
    use autore_schema::domain::records::WorkItemKind;
    use autore_schema::domain::{ContentHash, NamespacedId};
    use autore_schema::ids::{
        EntityId, ProjectId, ProviderRunId, ReconstructionCampaignId, WorkItemId,
    };
    use petgraph::graph::DiGraph;

    use super::{
        DynamicObservation, DynamicObservationImporter, ObservationImport, TimestampRange,
    };
    use crate::fingerprint::{
        FingerprintComparison, FingerprintInput, InMemorySnapshot, compute_fingerprint,
    };
    use crate::tests_support::RecordingAutoReClient;
    use crate::work_graph::{DependencyEdgeKind, WorkGraph, WorkItemNode};

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

    fn build_test_graph(
        labels: &[&str],
        edges: &[(usize, usize, DependencyEdgeKind)],
    ) -> (WorkGraph, HashMap<String, EntityId>) {
        let mut graph = DiGraph::new();
        let mut label_to_entity: HashMap<String, EntityId> = HashMap::new();
        let mut entity_to_node: HashMap<EntityId, petgraph::graph::NodeIndex> = HashMap::new();
        let mut work_item_to_node: HashMap<WorkItemId, petgraph::graph::NodeIndex> = HashMap::new();

        for label in labels {
            let entity_id = EntityId::new();
            let work_item_id = WorkItemId::new();
            let idx = graph.add_node(WorkItemNode {
                work_item_id,
                kind: WorkItemKind::Function,
                entity_id: Some(entity_id),
            });
            label_to_entity.insert((*label).to_string(), entity_id);
            entity_to_node.insert(entity_id, idx);
            work_item_to_node.insert(work_item_id, idx);
        }

        for (src_label_idx, tgt_label_idx, kind) in edges {
            let src_entity = label_to_entity[labels[*src_label_idx]];
            let tgt_entity = label_to_entity[labels[*tgt_label_idx]];
            let src_idx = entity_to_node[&src_entity];
            let tgt_idx = entity_to_node[&tgt_entity];
            graph.add_edge(src_idx, tgt_idx, *kind);
        }

        (
            WorkGraph {
                graph,
                entity_to_node,
                work_item_to_node,
            },
            label_to_entity,
        )
    }

    fn make_observation_import(target_entity_id: EntityId) -> ObservationImport {
        ObservationImport::new(
            DynamicObservation {
                observation_kind: NamespacedId::parse("debug.syscall").unwrap(),
                captured_artifact_id: autore_schema::ids::ArtifactId::new(),
                target_entity_id,
                scenario_id: "scenario-1".into(),
                timestamp_range: TimestampRange {
                    start: autore_schema::domain::Timestamp::now(),
                    end: autore_schema::domain::Timestamp::now(),
                },
                recorded_at: autore_schema::domain::Timestamp::now(),
            },
            b"mov rax, [rbx]; ret".to_vec(),
        )
    }

    #[test]
    fn observation_importer_emits_three_commands_in_atomic_transaction() {
        let (graph, entities) = build_test_graph(&["target"], &[]);
        let snapshot = InMemorySnapshot::new();
        let client = RecordingAutoReClient::new();
        let importer = DynamicObservationImporter::new(&snapshot, &graph);
        let obs = make_observation_import(entities["target"]);

        let summary = importer
            .import(
                &obs,
                &client,
                ProjectId::new(),
                ReconstructionCampaignId::new(),
                ProviderRunId::new(),
            )
            .unwrap();

        let commands = client.commands();
        assert_eq!(commands.len(), 3);
        assert!(matches!(
            commands[0],
            ApplicationCommand::RegisterArtifact(_)
        ));
        assert!(matches!(
            commands[1],
            ApplicationCommand::ImportDynamicObservation(_)
        ));
        assert!(matches!(commands[2], ApplicationCommand::AddEvidence(_)));

        assert_eq!(
            summary.fingerprint_comparison,
            FingerprintComparison::FirstTime
        );
        assert!(summary.invalidated_work_items.is_empty());
        assert_eq!(summary.investigations_created, 0);
    }

    #[test]
    fn importer_recomputes_target_fingerprint() {
        let (graph, entities) = build_test_graph(&["target"], &[]);
        let target_entity = entities["target"];
        let target_idx = graph.entity_to_node[&target_entity];
        let target_work_item = graph.graph[target_idx].work_item_id;

        let mut snapshot = InMemorySnapshot::new();
        let base = base_input();
        let stored_fp = compute_fingerprint(&base);
        snapshot.insert(target_work_item, base, stored_fp);

        let client = RecordingAutoReClient::new();
        let importer = DynamicObservationImporter::new(&snapshot, &graph);
        let obs = make_observation_import(target_entity);

        let summary = importer
            .import(
                &obs,
                &client,
                ProjectId::new(),
                ReconstructionCampaignId::new(),
                ProviderRunId::new(),
            )
            .unwrap();

        assert_eq!(
            summary.fingerprint_comparison,
            FingerprintComparison::Changed
        );
        assert_eq!(summary.invalidated_work_items.len(), 1);
        assert_eq!(summary.invalidated_work_items[0], target_work_item);

        let commands = client.commands();
        assert!(commands.iter().any(|c| matches!(
            c,
            ApplicationCommand::InvalidateWorkItem(req) if req.work_item_id == target_work_item.to_string()
        )));
    }

    #[test]
    fn importer_propagates_invalidation_to_downstream_work() {
        // downstream depends on target via GeneratedDeclRequirement.
        let (graph, entities) = build_test_graph(
            &["target", "downstream"],
            &[(1, 0, DependencyEdgeKind::GeneratedDeclRequirement)],
        );
        let target_entity = entities["target"];
        let downstream_entity = entities["downstream"];
        let target_idx = graph.entity_to_node[&target_entity];
        let downstream_idx = graph.entity_to_node[&downstream_entity];
        let target_work_item = graph.graph[target_idx].work_item_id;
        let downstream_work_item = graph.graph[downstream_idx].work_item_id;

        let mut snapshot = InMemorySnapshot::new();
        let target_input = base_input();
        let target_fp = compute_fingerprint(&target_input);
        snapshot.insert(target_work_item, target_input, target_fp);

        // Downstream's stored fingerprint is stale: the input has no dynamic
        // observations, but the stored fingerprint was computed with an old one.
        let downstream_input = base_input();
        let stale_downstream_fp = ContentHash::from_bytes(b"stale-fp");
        snapshot.insert(downstream_work_item, downstream_input, stale_downstream_fp);

        let client = RecordingAutoReClient::new();
        let importer = DynamicObservationImporter::new(&snapshot, &graph);
        let obs = make_observation_import(target_entity);

        let summary = importer
            .import(
                &obs,
                &client,
                ProjectId::new(),
                ReconstructionCampaignId::new(),
                ProviderRunId::new(),
            )
            .unwrap();

        assert_eq!(
            summary.fingerprint_comparison,
            FingerprintComparison::Changed
        );
        assert!(summary.invalidated_work_items.contains(&target_work_item));
        assert!(
            summary
                .invalidated_work_items
                .contains(&downstream_work_item)
        );

        let invalidate_count =
            client.count(|c| matches!(c, ApplicationCommand::InvalidateWorkItem(_)));
        assert_eq!(invalidate_count, 2);
    }

    #[test]
    fn nondeterministic_observation_flags_create_investigation_work_item() {
        let (graph, entities) = build_test_graph(&["target"], &[]);
        let snapshot = InMemorySnapshot::new();
        let client = RecordingAutoReClient::new();
        let importer = DynamicObservationImporter::new(&snapshot, &graph);
        let mut obs = make_observation_import(entities["target"]);
        obs.replay_flag = true;

        let summary = importer
            .import(
                &obs,
                &client,
                ProjectId::new(),
                ReconstructionCampaignId::new(),
                ProviderRunId::new(),
            )
            .unwrap();

        assert_eq!(summary.investigations_created, 1);
        let create_count = client.count(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)));
        assert_eq!(create_count, 1);
    }
}
