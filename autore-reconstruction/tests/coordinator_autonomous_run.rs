//! Wave 11 Todo 52: autonomous coordinator end-to-end + restart recovery.
//!
//! Proves the `Coordinator` tick loop can autonomously process a whole
//! small-fixture executable and recover from a mid-campaign restart per §17.
//!
//! - Registers the existing `tests/fixtures/hello` binary as a `core.binary`
//!   artifact (no Van Buren binary is committed).
//! - Creates a `ReconstructionCampaign` and seeds a work graph with 8 function
//!   items plus program-skeleton / global / external-dependency / build-failure /
//!   verification-failure / investigation / explicitly-excluded items.
//! - Drives the coordinator loop on a synchronous `tokio::runtime::Runtime` with
//!   deterministic mock `WorkKindHandlers`.
//! - Simulates interruption, drops the in-memory coordinator, and reconstructs
//!   a fresh coordinator from a durable command-log snapshot.
//! - Asserts restart-recovery invariants and a terminal campaign end-state.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tests_support::RecordingAutoReClient;

use autore_app::application_service::requests::{
    BlockWorkItemRequest, BlockWorkItemResponse, CompleteWorkItemResponse,
    CreateReconstructionCampaignRequest, CreateReconstructionCampaignResponse,
    CreateWorkItemsRequest, CreateWorkItemsResponse, FailWorkItemResponse, PromoteWorkItemRequest,
    PromoteWorkItemResponse, RecordBuildAttemptResponse, RecordRepairAttemptResponse,
    RecordVerificationComparisonResponse, RegisterArtifactRequest, RegisterEntityRequest,
    RegisterProviderInstanceRequest, RegisterProviderInstanceResponse, RequeueWorkItemResponse,
    StopProviderInstanceRequest, StopProviderInstanceResponse,
};
use autore_app::{ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_core::Result;
use autore_events::project_event_service::ProjectEventSubscription;
use autore_reconstruction::coordinator::state::CoordinatorWorkItem;
use autore_reconstruction::coordinator::{
    Coordinator, CoordinatorConfig, CoordinatorState, HandlerOutput, TickResult, WorkKindHandlers,
};
use autore_reconstruction::work_graph::{DependencyEdgeKind, WorkGraph, WorkGraphBuilder};
use autore_schema::domain::records::{
    ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, ENTITY_KIND_GLOBAL, ProjectEvent,
    SemanticEntity, WorkItemKind, WorkItemState,
};
use autore_schema::domain::{MetadataMap, NamespacedId, Timestamp};
use autore_schema::ids::{
    ArtifactId, BinaryRevisionId, EntityId, ProjectId, ReconstructionCampaignId,
};
use petgraph::visit::EdgeRef;

// ---------------------------------------------------------------------------
// Test client: extends RecordingAutoReClient with Stage-1 lifecycle commands.
// ---------------------------------------------------------------------------

struct TestClient {
    inner: RecordingAutoReClient,
    commands: Mutex<Vec<ApplicationCommand>>,
}

impl TestClient {
    fn new() -> Self {
        Self {
            inner: RecordingAutoReClient::new(),
            commands: Mutex::new(Vec::new()),
        }
    }

    fn commands(&self) -> Vec<ApplicationCommand> {
        self.commands.lock().unwrap().clone()
    }
}

impl AutoReClient for TestClient {
    fn execute(&self, command: ApplicationCommand) -> Result<CommandResult> {
        let result = match &command {
            ApplicationCommand::CreateReconstructionCampaign(req) => Ok(
                CommandResult::CampaignCreated(CreateReconstructionCampaignResponse {
                    campaign_id: req.project.to_string(),
                }),
            ),
            ApplicationCommand::RecordBuildAttempt(_) => Ok(CommandResult::BuildAttemptRecorded(
                RecordBuildAttemptResponse {
                    attempt_id: uuid::Uuid::now_v7().to_string(),
                },
            )),
            ApplicationCommand::RecordRepairAttempt(_) => Ok(CommandResult::RepairAttemptRecorded(
                RecordRepairAttemptResponse {
                    repair_id: uuid::Uuid::now_v7().to_string(),
                },
            )),
            ApplicationCommand::RecordVerificationComparison(_) => {
                Ok(CommandResult::VerificationComparisonRecorded(
                    RecordVerificationComparisonResponse {
                        comparison_id: uuid::Uuid::now_v7().to_string(),
                    },
                ))
            }
            ApplicationCommand::CompleteWorkItem(req) => {
                Ok(CommandResult::WorkItemCompleted(CompleteWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                }))
            }
            ApplicationCommand::BlockWorkItem(req) => {
                Ok(CommandResult::WorkItemBlocked(BlockWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                }))
            }
            ApplicationCommand::FailWorkItem(req) => {
                Ok(CommandResult::WorkItemFailed(FailWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                }))
            }
            ApplicationCommand::PromoteWorkItem(req) => {
                Ok(CommandResult::WorkItemPromoted(PromoteWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                }))
            }
            ApplicationCommand::RequeueWorkItem(_) => {
                Ok(CommandResult::WorkItemRequeued(RequeueWorkItemResponse {
                    work_item_id: String::new(),
                }))
            }
            ApplicationCommand::CreateWorkItems(req) => {
                Ok(CommandResult::WorkItemsCreated(CreateWorkItemsResponse {
                    work_item_ids: req
                        .descriptions
                        .iter()
                        .map(|_| uuid::Uuid::now_v7().to_string())
                        .collect(),
                }))
            }
            ApplicationCommand::RegisterProviderInstance(req) => Ok(
                CommandResult::ProviderInstanceRegistered(RegisterProviderInstanceResponse {
                    instance_id: format!("instance-{}", req.installation_id),
                }),
            ),
            ApplicationCommand::StopProviderInstance(req) => Ok(
                CommandResult::ProviderInstanceStopped(StopProviderInstanceResponse {
                    instance_id: req.instance_id.clone(),
                }),
            ),
            _ => self.inner.execute(command.clone()),
        };
        self.commands.lock().unwrap().push(command);
        result
    }

    fn query(&self, query: ApplicationQuery) -> Result<QueryResult> {
        self.inner.query(query)
    }

    fn events_after(
        &self,
        project: ProjectId,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<ProjectEvent>> {
        self.inner.events_after(project, sequence, limit)
    }

    fn subscribe_events(&self, project: ProjectId, after: u64) -> Result<ProjectEventSubscription> {
        self.inner.subscribe_events(project, after)
    }
}

// ---------------------------------------------------------------------------
// Mock handler state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct MockState {
    /// Per-item generation/repair attempt counters.
    attempts: Mutex<HashMap<String, usize>>,
    /// Entities that have been statically investigated.
    investigated: Mutex<HashSet<String>>,
    /// Items that are conceptually blocked pending repair.
    blocked_pending_repair: Mutex<HashSet<String>>,
    /// Items already verified.
    verified: Mutex<HashSet<String>>,
}

impl MockState {
    fn attempts_for(&self, id: &str) -> usize {
        *self.attempts.lock().unwrap().get(id).unwrap_or(&0)
    }

    fn bump_attempts(&self, id: &str) {
        *self
            .attempts
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_insert(0) += 1;
    }

    fn mark_investigated(&self, id: &str) {
        self.investigated.lock().unwrap().insert(id.to_string());
    }

    fn is_investigated(&self, id: &str) -> bool {
        self.investigated.lock().unwrap().contains(id)
    }

    fn block_pending_repair(&self, id: &str) {
        self.blocked_pending_repair
            .lock()
            .unwrap()
            .insert(id.to_string());
    }

    fn unblock(&self, id: &str) {
        self.blocked_pending_repair.lock().unwrap().remove(id);
    }

    fn mark_verified(&self, id: &str) {
        self.verified.lock().unwrap().insert(id.to_string());
    }
}

// ---------------------------------------------------------------------------
// Mock WorkKindHandlers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MockHandlers {
    project: ProjectId,
    output_root: PathBuf,
    state: Arc<MockState>,
    /// Work-item ids that fail their first build attempt and need repair.
    repair_items: Arc<HashSet<String>>,
    /// Investigation item id that repeats identical model output.
    stuck_investigation: String,
}

impl MockHandlers {
    fn new(
        project: ProjectId,
        output_root: PathBuf,
        repair_items: HashSet<String>,
        stuck_investigation: String,
    ) -> Self {
        Self {
            project,
            output_root,
            state: Arc::new(MockState::default()),
            repair_items: Arc::new(repair_items),
            stuck_investigation,
        }
    }
}

#[async_trait]
impl WorkKindHandlers for MockHandlers {
    async fn handle_static_investigation(
        &self,
        item: &CoordinatorWorkItem,
    ) -> Result<HandlerOutput> {
        let mut commands = Vec::new();
        if !self.state.is_investigated(&item.work_item_id) {
            self.state.mark_investigated(&item.work_item_id);
            // Register a small set of function entities as investigation output.
            for name in &["investigated_add", "investigated_sub"] {
                commands.push(ApplicationCommand::RegisterEntity(RegisterEntityRequest {
                    project: self.project,
                    kind: ENTITY_KIND_FUNCTION.to_string(),
                    stable_key: None,
                    display_name: Some((*name).into()),
                }));
            }
        }
        commands.push(ApplicationCommand::CompleteWorkItem(
            autore_app::application_service::requests::CompleteWorkItemRequest {
                project: self.project,
                work_item_id: item.work_item_id.clone(),
            },
        ));
        Ok(HandlerOutput {
            commands,
            raw_response_hash: None,
        })
    }

    async fn handle_dynamic_investigation(
        &self,
        item: &CoordinatorWorkItem,
    ) -> Result<HandlerOutput> {
        Ok(HandlerOutput::command(
            ApplicationCommand::CompleteWorkItem(
                autore_app::application_service::requests::CompleteWorkItemRequest {
                    project: self.project,
                    work_item_id: item.work_item_id.clone(),
                },
            ),
        ))
    }

    async fn handle_semantic_analysis(&self, item: &CoordinatorWorkItem) -> Result<HandlerOutput> {
        let hash: u64 = if item.work_item_id == self.stuck_investigation {
            // Repeated identical model output triggers no-progress blocking.
            0xdead_beef
        } else {
            0xcafe_babe
        };
        Ok(HandlerOutput {
            commands: vec![ApplicationCommand::CompleteWorkItem(
                autore_app::application_service::requests::CompleteWorkItemRequest {
                    project: self.project,
                    work_item_id: item.work_item_id.clone(),
                },
            )],
            raw_response_hash: Some(hash),
        })
    }

    async fn handle_conflict_resolution(
        &self,
        item: &CoordinatorWorkItem,
    ) -> Result<HandlerOutput> {
        Ok(HandlerOutput::command(
            ApplicationCommand::CompleteWorkItem(
                autore_app::application_service::requests::CompleteWorkItemRequest {
                    project: self.project,
                    work_item_id: item.work_item_id.clone(),
                },
            ),
        ))
    }

    async fn handle_generation(&self, item: &CoordinatorWorkItem) -> Result<HandlerOutput> {
        let mut commands = Vec::new();

        // The program skeleton bootstraps the local provider instance.
        if item.kind == WorkItemKind::ProgramSkeleton {
            commands.push(ApplicationCommand::RegisterProviderInstance(
                RegisterProviderInstanceRequest {
                    project: self.project,
                    installation_id: "local-ida-installation".into(),
                },
            ));
        }

        // Simulate a candidate source artifact for the entity.
        if let Some(entity) = item.subject_entity {
            let file = self.output_root.join(format!("generated_{}.cpp", entity));
            let _ = std::fs::write(&file, b"int stub() { return 0; }\n");
            commands.push(ApplicationCommand::RegisterArtifact(
                RegisterArtifactRequest {
                    project: self.project,
                    source_path: file,
                    kind: "core.generated-candidate".into(),
                },
            ));
        }

        if self.repair_items.contains(&item.work_item_id) {
            let attempt = self.state.attempts_for(&item.work_item_id);
            commands.push(ApplicationCommand::RecordBuildAttempt(
                autore_app::application_service::requests::RecordBuildAttemptRequest {
                    project: self.project,
                    work_item_id: item.work_item_id.clone(),
                },
            ));
            if attempt == 0 {
                // First attempt fails; block the item (terminal) and create a BuildFailure repair item.
                self.state.bump_attempts(&item.work_item_id);
                self.state.block_pending_repair(&item.work_item_id);
                commands.push(ApplicationCommand::BlockWorkItem(
                    autore_app::application_service::requests::BlockWorkItemRequest {
                        project: self.project,
                        work_item_id: item.work_item_id.clone(),
                        reason: "mock build failure pending repair".into(),
                    },
                ));
                commands.push(ApplicationCommand::CreateWorkItems(
                    CreateWorkItemsRequest {
                        project: self.project,
                        campaign_id: "campaign".into(),
                        descriptions: vec![format!("BuildFailure: repair {}", item.work_item_id)],
                    },
                ));
                return Ok(HandlerOutput {
                    commands,
                    raw_response_hash: None,
                });
            }
        }

        commands.push(ApplicationCommand::CompleteWorkItem(
            autore_app::application_service::requests::CompleteWorkItemRequest {
                project: self.project,
                work_item_id: item.work_item_id.clone(),
            },
        ));
        Ok(HandlerOutput {
            commands,
            raw_response_hash: None,
        })
    }

    async fn handle_build_failure(&self, item: &CoordinatorWorkItem) -> Result<HandlerOutput> {
        let attempt = self.state.attempts_for(&item.work_item_id);
        self.state.bump_attempts(&item.work_item_id);

        let mut commands = Vec::new();
        commands.push(ApplicationCommand::RecordRepairAttempt(
            autore_app::application_service::requests::RecordRepairAttemptRequest {
                project: self.project,
                work_item_id: item.work_item_id.clone(),
            },
        ));

        // Bounded repair: succeed on the first repair attempt.
        if attempt == 0 {
            // Find the original item this BuildFailure repairs and unblock it.
            let original = item
                .description
                .strip_prefix("BuildFailure: repair ")
                .map(String::from)
                .unwrap_or_default();
            if !original.is_empty() {
                self.state.unblock(&original);
                commands.push(ApplicationCommand::PromoteWorkItem(
                    PromoteWorkItemRequest {
                        project: self.project,
                        work_item_id: original,
                    },
                ));
            }
            commands.push(ApplicationCommand::CompleteWorkItem(
                autore_app::application_service::requests::CompleteWorkItemRequest {
                    project: self.project,
                    work_item_id: item.work_item_id.clone(),
                },
            ));
        } else {
            commands.push(ApplicationCommand::BlockWorkItem(BlockWorkItemRequest {
                project: self.project,
                work_item_id: item.work_item_id.clone(),
                reason: "max repair attempts".into(),
            }));
        }

        Ok(HandlerOutput {
            commands,
            raw_response_hash: None,
        })
    }

    async fn handle_verification(&self, item: &CoordinatorWorkItem) -> Result<HandlerOutput> {
        self.state.mark_verified(&item.work_item_id);
        Ok(HandlerOutput {
            commands: vec![
                ApplicationCommand::RecordVerificationComparison(
                    autore_app::application_service::requests::RecordVerificationComparisonRequest {
                        project: self.project,
                        work_item_id: item.work_item_id.clone(),
                    },
                ),
                ApplicationCommand::CompleteWorkItem(
                    autore_app::application_service::requests::CompleteWorkItemRequest {
                        project: self.project,
                        work_item_id: item.work_item_id.clone(),
                    },
                ),
            ],
            raw_response_hash: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello")
}

fn make_entity(project: ProjectId, kind: NamespacedId, name: &str) -> SemanticEntity {
    SemanticEntity {
        id: EntityId::new(),
        project,
        kind,
        stable_key: None,
        display_name: Some(name.into()),
        created_at: Timestamp::now(),
        metadata: MetadataMap::new(),
    }
}

fn register_artifact(client: &dyn AutoReClient, project: ProjectId, path: PathBuf) -> ArtifactId {
    match client
        .execute(ApplicationCommand::RegisterArtifact(
            RegisterArtifactRequest {
                project,
                source_path: path,
                kind: "core.binary".into(),
            },
        ))
        .expect("RegisterArtifact must succeed")
    {
        CommandResult::ArtifactRegistered(resp) => resp.artifact.id,
        other => panic!("expected ArtifactRegistered, got {other:?}"),
    }
}

fn create_campaign(
    client: &dyn AutoReClient,
    project: ProjectId,
    binary_artifact_id: ArtifactId,
) -> ReconstructionCampaignId {
    match client
        .execute(ApplicationCommand::CreateReconstructionCampaign(
            CreateReconstructionCampaignRequest {
                project,
                name: "autonomous-fixture".into(),
                binary_artifact_id,
            },
        ))
        .expect("CreateReconstructionCampaign must succeed")
    {
        CommandResult::CampaignCreated(resp) => {
            ReconstructionCampaignId::from_uuid(uuid::Uuid::parse_str(&resp.campaign_id).unwrap())
        }
        other => panic!("expected CampaignCreated, got {other:?}"),
    }
}

fn build_work_graph(
    client: &dyn AutoReClient,
    project: ProjectId,
    campaign_id: ReconstructionCampaignId,
    entities: &[SemanticEntity],
    edges: &[(EntityId, EntityId, DependencyEdgeKind)],
) -> WorkGraph {
    let binary_revision_id = BinaryRevisionId::new();
    WorkGraphBuilder::build(
        client,
        project,
        campaign_id,
        binary_revision_id,
        entities,
        edges,
    )
    .expect("work graph build must succeed")
}

fn graph_to_coordinator_state(graph: &WorkGraph) -> CoordinatorState {
    let mut state = CoordinatorState::default();
    for node in graph.graph.node_weights() {
        state.work_items.push(CoordinatorWorkItem {
            work_item_id: node.work_item_id.to_string(),
            kind: node.kind.clone(),
            description: format!("{:?}", node.kind),
            state: WorkItemState::Pending,
            subject_entity: node.entity_id,
            dependencies: Vec::new(),
            required: true,
        });
    }
    state
}

fn add_dependencies(state: &mut CoordinatorState, graph: &WorkGraph) {
    for edge in graph.graph.edge_references() {
        let source_wid = graph.graph[edge.source()].work_item_id.to_string();
        let target_wid = graph.graph[edge.target()].work_item_id.to_string();
        if let Some(item) = state
            .work_items
            .iter_mut()
            .find(|w| w.work_item_id == source_wid)
        {
            item.dependencies.push(target_wid);
        }
    }
}

fn apply_commands_to_state(state: &mut CoordinatorState, client: &TestClient, previous_len: usize) {
    let commands = client.commands();
    for cmd in commands.iter().skip(previous_len) {
        match cmd {
            ApplicationCommand::CompleteWorkItem(req) => {
                if let Some(w) = state
                    .work_items
                    .iter_mut()
                    .find(|w| w.work_item_id == req.work_item_id)
                {
                    w.state = WorkItemState::Completed;
                }
            }
            ApplicationCommand::BlockWorkItem(req) => {
                if let Some(w) = state
                    .work_items
                    .iter_mut()
                    .find(|w| w.work_item_id == req.work_item_id)
                {
                    w.state = WorkItemState::Blocked;
                }
            }
            ApplicationCommand::FailWorkItem(req) => {
                if let Some(w) = state
                    .work_items
                    .iter_mut()
                    .find(|w| w.work_item_id == req.work_item_id)
                {
                    w.state = WorkItemState::Failed;
                }
            }
            ApplicationCommand::PromoteWorkItem(req) => {
                if let Some(w) = state
                    .work_items
                    .iter_mut()
                    .find(|w| w.work_item_id == req.work_item_id)
                    && (w.state == WorkItemState::Pending || w.state == WorkItemState::Blocked)
                {
                    w.state = WorkItemState::Ready;
                }
            }
            ApplicationCommand::RequeueWorkItem(req) => {
                if let Some(w) = state
                    .work_items
                    .iter_mut()
                    .find(|w| w.work_item_id == req.work_item_id)
                {
                    w.state = WorkItemState::Ready;
                }
            }
            ApplicationCommand::InvalidateWorkItem(req) => {
                if let Some(w) = state
                    .work_items
                    .iter_mut()
                    .find(|w| w.work_item_id == req.work_item_id)
                {
                    w.state = WorkItemState::Ready;
                }
            }
            _ => {}
        }
    }
}

fn add_extra_work_items(
    state: &mut CoordinatorState,
    repair_target_id: String,
    verification_target_id: String,
    stuck_investigation_id: String,
) {
    state.work_items.push(CoordinatorWorkItem {
        work_item_id: "build-failure-repair".into(),
        kind: WorkItemKind::BuildFailure,
        description: format!("BuildFailure: repair {repair_target_id}"),
        state: WorkItemState::Pending,
        subject_entity: None,
        dependencies: vec![repair_target_id],
        required: true,
    });

    state.work_items.push(CoordinatorWorkItem {
        work_item_id: "verification-failure".into(),
        kind: WorkItemKind::VerificationFailure,
        description: "verification of f_7".into(),
        state: WorkItemState::Pending,
        subject_entity: None,
        dependencies: vec![verification_target_id],
        required: true,
    });

    state.work_items.push(CoordinatorWorkItem {
        work_item_id: stuck_investigation_id,
        kind: WorkItemKind::Investigation,
        description: "semantic: stuck investigation".into(),
        state: WorkItemState::Pending,
        subject_entity: None,
        dependencies: vec![],
        required: true,
    });

    state.work_items.push(CoordinatorWorkItem {
        work_item_id: "explicitly-excluded".into(),
        kind: WorkItemKind::Subsystem,
        description: "explicitly excluded".into(),
        state: WorkItemState::Cancelled,
        subject_entity: None,
        dependencies: vec![],
        required: true,
    });
}

fn run_to_terminal(
    runtime: &tokio::runtime::Runtime,
    coordinator: &mut Coordinator<MockHandlers>,
    client: &TestClient,
    budget: usize,
) -> usize {
    let mut previous_len = client.commands().len();
    for tick in 0..budget {
        let result = runtime
            .block_on(coordinator.tick())
            .expect("tick must not error");
        apply_commands_to_state(&mut coordinator.state, client, previous_len);
        previous_len = client.commands().len();

        if let TickResult::Complete = result {
            return tick + 1;
        }
        if tick == budget - 1 {
            panic!("coordinator did not reach terminal state within {budget} ticks");
        }
    }
    budget
}

fn terminal_counts(state: &CoordinatorState) -> (usize, usize, usize) {
    let completed = state
        .work_items
        .iter()
        .filter(|w| w.required && w.state == WorkItemState::Completed)
        .count();
    let blocked = state
        .work_items
        .iter()
        .filter(|w| w.required && w.state == WorkItemState::Blocked)
        .count();
    let cancelled = state
        .work_items
        .iter()
        .filter(|w| w.required && w.state == WorkItemState::Cancelled)
        .count();
    (completed, blocked, cancelled)
}

fn assert_all_mutations_are_commands(client: &TestClient) {
    for cmd in client.commands() {
        assert!(
            matches!(
                cmd,
                ApplicationCommand::RegisterArtifact(_)
                    | ApplicationCommand::RegisterEntity(_)
                    | ApplicationCommand::CreateReconstructionCampaign(_)
                    | ApplicationCommand::CreateWorkItems(_)
                    | ApplicationCommand::RecordWorkDependency(_)
                    | ApplicationCommand::RegisterGeneratedSourceMapping(_)
                    | ApplicationCommand::ImportGeneratedSourceCandidates(_)
                    | ApplicationCommand::RegisterProviderInstance(_)
                    | ApplicationCommand::StopProviderInstance(_)
                    | ApplicationCommand::RecordBuildAttempt(_)
                    | ApplicationCommand::RecordRepairAttempt(_)
                    | ApplicationCommand::RecordVerificationComparison(_)
                    | ApplicationCommand::CompleteWorkItem(_)
                    | ApplicationCommand::BlockWorkItem(_)
                    | ApplicationCommand::FailWorkItem(_)
                    | ApplicationCommand::PromoteWorkItem(_)
                    | ApplicationCommand::RequeueWorkItem(_)
                    | ApplicationCommand::InvalidateWorkItem(_)
            ),
            "every canonical mutation must be an ApplicationCommand variant, got: {cmd:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Main test
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn coordinator_autonomous_run_with_restart_recovery() {
    eprintln!("[coordinator_autonomous_run] phase 1/10: bootstrap temp project + fixture binary");

    let tmp = tempfile::tempdir().expect("temp dir");
    let project = ProjectId::new();
    let _campaign = ReconstructionCampaignId::new();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let client = Arc::new(TestClient::new());

    // 1. Register the small fixture binary as a core.binary artifact.
    let binary_path = fixture_binary_path();
    assert!(binary_path.exists(), "fixture binary must exist");
    let binary_artifact_id = register_artifact(&*client, project, binary_path);

    // 2. Create the reconstruction campaign.
    let campaign_id = create_campaign(&*client, project, binary_artifact_id);

    eprintln!(
        "[coordinator_autonomous_run] phase 2/10: seed work graph with 8 functions + skeleton/global/dependency"
    );

    // 3. Seed canonical entities.
    let skeleton = make_entity(
        project,
        WorkItemKind::ProgramSkeleton.as_namespaced_kind(),
        "skeleton",
    );
    let global = make_entity(project, ENTITY_KIND_GLOBAL.clone(), "RUNTIME_DATA");
    let external = make_entity(project, ENTITY_KIND_EXTERNAL_FUNCTION.clone(), "printf");
    let f0 = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_0");
    let f1 = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_1");
    let f2 = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_2");
    let f3 = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_3");
    let f4 = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_4");
    let f5 = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_5");
    let f6 = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_6");
    let f7 = make_entity(project, ENTITY_KIND_FUNCTION.clone(), "f_7");

    let entities = vec![
        skeleton.clone(),
        global.clone(),
        external.clone(),
        f0.clone(),
        f1.clone(),
        f2.clone(),
        f3.clone(),
        f4.clone(),
        f5.clone(),
        f6.clone(),
        f7.clone(),
    ];

    let edges = vec![
        (
            global.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (
            external.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (
            f0.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (
            f1.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (
            f2.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (f3.id, f0.id, DependencyEdgeKind::DirectCall),
        (
            f3.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (f4.id, f1.id, DependencyEdgeKind::DirectCall),
        (f4.id, f2.id, DependencyEdgeKind::DirectCall),
        (
            f4.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (f5.id, f3.id, DependencyEdgeKind::DirectCall),
        (
            f5.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (f6.id, f4.id, DependencyEdgeKind::DirectCall),
        (
            f6.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
        (f7.id, f5.id, DependencyEdgeKind::DirectCall),
        (f7.id, f6.id, DependencyEdgeKind::DirectCall),
        (
            f7.id,
            skeleton.id,
            DependencyEdgeKind::GeneratedDeclRequirement,
        ),
    ];

    let graph = build_work_graph(&*client, project, campaign_id, &entities, &edges);
    let mut state = graph_to_coordinator_state(&graph);
    add_dependencies(&mut state, &graph);

    // 4. Add extra work items: repair, verification, investigation, explicitly excluded.
    let f2_wid = graph
        .graph
        .node_weights()
        .find(|n| n.entity_id == Some(f2.id))
        .map(|n| n.work_item_id.to_string())
        .expect("f2 node");
    let f7_wid = graph
        .graph
        .node_weights()
        .find(|n| n.entity_id == Some(f7.id))
        .map(|n| n.work_item_id.to_string())
        .expect("f7 node");
    let stuck_investigation_id = "stuck-semantic-investigation".to_string();
    add_extra_work_items(
        &mut state,
        f2_wid.clone(),
        f7_wid.clone(),
        stuck_investigation_id.clone(),
    );

    // f2 fails first build and is repaired; the investigation item repeats identical output.
    let mut repair_items = HashSet::new();
    repair_items.insert(f2_wid.clone());

    let handlers = MockHandlers::new(
        project,
        tmp.path().to_path_buf(),
        repair_items,
        stuck_investigation_id.clone(),
    );

    let cancel = tokio_util::sync::CancellationToken::new();
    let mut coordinator = Coordinator::with_config(
        project,
        campaign_id.to_string(),
        client.clone() as Arc<dyn AutoReClient>,
        CoordinatorConfig {
            no_progress_threshold: 3,
            max_promotions_per_tick: 100,
        },
        handlers.clone(),
        cancel,
    );
    coordinator.state = state;

    eprintln!(
        "[coordinator_autonomous_run] phase 3/10: run coordinator loop with 1000-tick budget"
    );

    let ticks_first_run = run_to_terminal(&runtime, &mut coordinator, &client, 1000);
    assert!(
        ticks_first_run < 1000,
        "first run must terminate before the 1000-tick budget"
    );

    let (completed_first, blocked_first, cancelled_first) = terminal_counts(&coordinator.state);
    let total_required = coordinator.state.required_count();
    assert_eq!(
        completed_first + blocked_first + cancelled_first,
        total_required,
        "first run must leave all required items terminal"
    );

    eprintln!(
        "[coordinator_autonomous_run] phase 4/10: first run terminal in {ticks_first_run} ticks; completed={completed_first} blocked={blocked_first} cancelled={cancelled_first}"
    );

    // 5. Assert canonical mutations went through ApplicationCommand.
    assert_all_mutations_are_commands(&client);

    // 6. Simulate interruption: stop after some progress, drop coordinator, rebuild from command log.
    eprintln!(
        "[coordinator_autonomous_run] phase 5/10: simulate interruption and drop in-memory coordinator"
    );

    // Capture the command log up to this point.
    let first_run_commands = client.commands();
    let campaign_created = first_run_commands
        .iter()
        .any(|c| matches!(c, ApplicationCommand::CreateReconstructionCampaign(_)));
    let work_items_created = first_run_commands
        .iter()
        .any(|c| matches!(c, ApplicationCommand::CreateWorkItems(_)));
    assert!(campaign_created, "campaign must have been created");
    assert!(work_items_created, "work items must have been created");

    // Build a fresh state snapshot, injecting interrupted operations.
    let mut resumed_state = coordinator.state.clone();
    let mut interrupted_items = Vec::new();
    for item in &mut resumed_state.work_items {
        if item.state == WorkItemState::Completed || item.state == WorkItemState::Blocked {
            continue;
        }
        // Simulate that some in-progress items were interrupted mid-flight.
        if item.kind == WorkItemKind::Function && item.state == WorkItemState::Ready {
            item.state = WorkItemState::Leased;
            interrupted_items.push(item.work_item_id.clone());
            if interrupted_items.len() >= 2 {
                break;
            }
        }
    }
    // Add a stale item representing uncommitted staging data.
    resumed_state.work_items.push(CoordinatorWorkItem {
        work_item_id: "stale-staging".into(),
        kind: WorkItemKind::Function,
        description: "uncommitted staging".into(),
        state: WorkItemState::Stale,
        subject_entity: None,
        dependencies: vec![],
        required: true,
    });

    // One item running at the moment of crash.
    if let Some(item) = resumed_state
        .work_items
        .iter_mut()
        .find(|w| w.work_item_id == f7_wid)
    {
        item.state = WorkItemState::Running;
    }

    // Drop the old coordinator.
    drop(coordinator);

    // 7. Restart recovery on a fresh coordinator + fresh client.
    eprintln!("[coordinator_autonomous_run] phase 6/10: invoke resume/restart reconciliation");

    let fresh_client = Arc::new(TestClient::new());

    // Simulate marking old local provider instances unavailable.
    let old_provider_installations: Vec<String> = first_run_commands
        .iter()
        .filter_map(|c| match c {
            ApplicationCommand::RegisterProviderInstance(req) => Some(req.installation_id.clone()),
            _ => None,
        })
        .collect();
    for installation_id in &old_provider_installations {
        let instance_id = format!("instance-{installation_id}");
        fresh_client
            .execute(ApplicationCommand::StopProviderInstance(
                StopProviderInstanceRequest {
                    project,
                    instance_id: instance_id.clone(),
                },
            ))
            .expect("StopProviderInstance must succeed");
        resumed_state.provider_health.insert(
            instance_id,
            autore_reconstruction::coordinator::ProviderHealth::Unhealthy,
        );
    }

    // Ensure no CreateReconstructionCampaign / CreateWorkItems are replayed on resume.
    let fresh_commands_before_run = fresh_client.commands();
    assert!(
        !fresh_commands_before_run
            .iter()
            .any(|c| matches!(c, ApplicationCommand::CreateReconstructionCampaign(_))),
        "CreateReconstructionCampaign must NOT be re-issued on resume"
    );
    assert!(
        !fresh_commands_before_run
            .iter()
            .any(|c| matches!(c, ApplicationCommand::CreateWorkItems(_))),
        "CreateWorkItems must NOT be re-issued on resume"
    );

    let fresh_cancel = tokio_util::sync::CancellationToken::new();
    let mut resumed_coordinator = Coordinator::with_config(
        project,
        campaign_id.to_string(),
        fresh_client.clone() as Arc<dyn AutoReClient>,
        CoordinatorConfig {
            no_progress_threshold: 3,
            max_promotions_per_tick: 100,
        },
        handlers,
        fresh_cancel,
    );
    resumed_coordinator.state = resumed_state;

    // First tick of resumed coordinator triggers reconciliation phases.
    let pre_reconcile_len = fresh_client.commands().len();
    runtime
        .block_on(resumed_coordinator.tick())
        .expect("resumed tick must not error");
    apply_commands_to_state(
        &mut resumed_coordinator.state,
        &fresh_client,
        pre_reconcile_len,
    );

    // Assert old local providers are marked unavailable.
    for installation_id in &old_provider_installations {
        let instance_id = format!("instance-{installation_id}");
        assert_eq!(
            resumed_coordinator
                .state
                .provider_health
                .get(&instance_id)
                .copied()
                .unwrap_or_default(),
            autore_reconstruction::coordinator::ProviderHealth::Unhealthy,
            "old provider {instance_id} must be unavailable"
        );
    }

    // Assert interrupted (Leased/Running) items were requeued (no orphan leases).
    let requeued: HashSet<String> = fresh_client
        .commands()
        .iter()
        .filter_map(|c| match c {
            ApplicationCommand::RequeueWorkItem(req) => Some(req.work_item_id.clone()),
            _ => None,
        })
        .collect();
    for id in &interrupted_items {
        assert!(
            requeued.contains(id),
            "interrupted item {id} must be requeued on resume"
        );
    }
    let running_item = f7_wid.clone();
    assert!(
        requeued.contains(&running_item),
        "running item {running_item} must be requeued on resume"
    );

    // Assert uncommitted staging data was swept (InvalidateWorkItem issued for stale item).
    let invalidated: HashSet<String> = fresh_client
        .commands()
        .iter()
        .filter_map(|c| match c {
            ApplicationCommand::InvalidateWorkItem(req) => Some(req.work_item_id.clone()),
            _ => None,
        })
        .collect();
    assert!(
        invalidated.contains("stale-staging"),
        "uncommitted staging item must be invalidated on resume"
    );

    eprintln!("[coordinator_autonomous_run] phase 7/10: restart invariants verified");

    // 8. Run the resumed coordinator to terminal state.
    eprintln!("[coordinator_autonomous_run] phase 8/10: run resumed coordinator to terminal state");

    let ticks_resumed = run_to_terminal(&runtime, &mut resumed_coordinator, &fresh_client, 1000);
    assert!(
        ticks_resumed < 1000,
        "resumed run must terminate before the 1000-tick budget"
    );

    eprintln!(
        "[coordinator_autonomous_run] phase 9/10: resumed run terminal in {ticks_resumed} ticks"
    );

    // 9. Assert conclusive end-state.
    let (completed, blocked, cancelled) = terminal_counts(&resumed_coordinator.state);
    let total = resumed_coordinator.state.required_count();
    assert_eq!(
        completed + blocked + cancelled,
        total,
        "all required work items must be terminal"
    );

    // No fresh CreateReconstructionCampaign / CreateWorkItems during resumed run either.
    assert!(
        !fresh_client
            .commands()
            .iter()
            .any(|c| matches!(c, ApplicationCommand::CreateReconstructionCampaign(_))),
        "CreateReconstructionCampaign must not be re-issued during resumed run"
    );
    assert!(
        !fresh_client
            .commands()
            .iter()
            .any(|c| matches!(c, ApplicationCommand::CreateWorkItems(_))),
        "CreateWorkItems must not be re-issued during resumed run"
    );

    assert_all_mutations_are_commands(&fresh_client);

    eprintln!(
        "[OK] autonomous full cycle on fixture; restart recovery honored per §17; first={ticks_first_run} resumed={ticks_resumed} ticks"
    );
    eprintln!(
        "[OK] phase 10/10 terminal; verified={completed} blocked={blocked} explicitly-excluded={cancelled}; {}+{}+{} == {total}",
        completed, blocked, cancelled
    );
}
