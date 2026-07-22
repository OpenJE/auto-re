//! Coordinator unit tests.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use autore_app::application_service::requests::{
    ApplicationCommand, BlockWorkItemRequest, BlockWorkItemResponse, CreateWorkItemsRequest,
    ImportProviderRunResultResponse, InvalidateWorkItemResponse, PromoteWorkItemResponse,
    RecordWorkDependencyResponse, RequeueWorkItemRequest, RequeueWorkItemResponse,
};
use autore_app::{ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_core::{Error, Result};
use autore_events::project_event_service::ProjectEventSubscription;
use autore_schema::domain::records::{ProjectEvent, WorkItemKind, WorkItemState};
use autore_schema::ids::{EntityId, ProjectId};
use tokio_util::sync::CancellationToken;

use crate::coordinator::handlers::{
    DispatchKind, HandlerOutput, WorkKindHandlers, classify_work_item,
};
use crate::coordinator::policy::CompletionPolicy;
use crate::coordinator::state::{CoordinatorState, CoordinatorWorkItem};
use crate::coordinator::{Coordinator, CoordinatorConfig, TickResult};
use crate::tests_support::RecordingAutoReClient;
use std::sync::Arc;

/// Test client that wraps [`RecordingAutoReClient`] and supplies handlers for
/// the commands the coordinator issues directly.
#[derive(Debug, Default)]
struct TestClient {
    inner: RecordingAutoReClient,
    commands: Mutex<Vec<ApplicationCommand>>,
}

impl TestClient {
    fn new() -> Self {
        Self::default()
    }

    fn commands(&self) -> Vec<ApplicationCommand> {
        self.commands.lock().unwrap().clone()
    }
}

impl AutoReClient for TestClient {
    fn execute(&self, command: ApplicationCommand) -> Result<CommandResult> {
        let result = match &command {
            ApplicationCommand::RequeueWorkItem(_) => {
                CommandResult::WorkItemRequeued(RequeueWorkItemResponse {
                    work_item_id: String::new(),
                })
            }
            ApplicationCommand::ImportProviderRunResult(req) => {
                CommandResult::ProviderRunResultImported(ImportProviderRunResultResponse {
                    run_id: req.run_id,
                })
            }
            ApplicationCommand::RecordWorkDependency(req) => {
                CommandResult::WorkDependencyRecorded(RecordWorkDependencyResponse {
                    work_item_id: req.work_item_id.clone(),
                })
            }
            ApplicationCommand::InvalidateWorkItem(req) => {
                CommandResult::WorkItemInvalidated(InvalidateWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                })
            }
            ApplicationCommand::PromoteWorkItem(req) => {
                CommandResult::WorkItemPromoted(PromoteWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                })
            }
            ApplicationCommand::BlockWorkItem(req) => {
                CommandResult::WorkItemBlocked(BlockWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                })
            }
            _ => self.inner.execute(command.clone())?,
        };
        self.commands.lock().unwrap().push(command);
        Ok(result)
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

/// Mock handler set that records invocations and returns configurable outputs.
#[derive(Debug, Default, Clone)]
struct RecordingHandlers {
    invocations: Arc<Mutex<Vec<DispatchKind>>>,
    outputs: Arc<Mutex<HashMap<DispatchKind, HandlerOutput>>>,
}

impl RecordingHandlers {
    fn with_output(kind: DispatchKind, output: HandlerOutput) -> Self {
        let mut map = HashMap::new();
        map.insert(kind, output);
        Self {
            invocations: Arc::new(Mutex::new(Vec::new())),
            outputs: Arc::new(Mutex::new(map)),
        }
    }

    fn invocation_counts(&self) -> HashMap<DispatchKind, usize> {
        let mut counts = HashMap::new();
        for k in self.invocations.lock().unwrap().iter() {
            *counts.entry(*k).or_insert(0) += 1;
        }
        counts
    }

    fn output_for(&self, kind: DispatchKind) -> Result<HandlerOutput> {
        self.outputs
            .lock()
            .unwrap()
            .get(&kind)
            .cloned()
            .ok_or_else(|| Error::Validation(format!("no mock output for {kind:?}")))
    }
}

#[async_trait]
impl WorkKindHandlers for RecordingHandlers {
    async fn handle_static_investigation(
        &self,
        _item: &CoordinatorWorkItem,
    ) -> Result<HandlerOutput> {
        self.invocations
            .lock()
            .unwrap()
            .push(DispatchKind::StaticInvestigation);
        self.output_for(DispatchKind::StaticInvestigation)
    }

    async fn handle_dynamic_investigation(
        &self,
        _item: &CoordinatorWorkItem,
    ) -> Result<HandlerOutput> {
        self.invocations
            .lock()
            .unwrap()
            .push(DispatchKind::DynamicInvestigation);
        self.output_for(DispatchKind::DynamicInvestigation)
    }

    async fn handle_semantic_analysis(&self, _item: &CoordinatorWorkItem) -> Result<HandlerOutput> {
        self.invocations
            .lock()
            .unwrap()
            .push(DispatchKind::SemanticAnalysis);
        self.output_for(DispatchKind::SemanticAnalysis)
    }

    async fn handle_conflict_resolution(
        &self,
        _item: &CoordinatorWorkItem,
    ) -> Result<HandlerOutput> {
        self.invocations
            .lock()
            .unwrap()
            .push(DispatchKind::ConflictResolution);
        self.output_for(DispatchKind::ConflictResolution)
    }

    async fn handle_generation(&self, _item: &CoordinatorWorkItem) -> Result<HandlerOutput> {
        self.invocations
            .lock()
            .unwrap()
            .push(DispatchKind::Generation);
        self.output_for(DispatchKind::Generation)
    }

    async fn handle_build_failure(&self, _item: &CoordinatorWorkItem) -> Result<HandlerOutput> {
        self.invocations
            .lock()
            .unwrap()
            .push(DispatchKind::BuildFailure);
        self.output_for(DispatchKind::BuildFailure)
    }

    async fn handle_verification(&self, _item: &CoordinatorWorkItem) -> Result<HandlerOutput> {
        self.invocations
            .lock()
            .unwrap()
            .push(DispatchKind::Verification);
        self.output_for(DispatchKind::Verification)
    }
}

fn work_item(
    id: &str,
    kind: WorkItemKind,
    description: &str,
    state: WorkItemState,
) -> CoordinatorWorkItem {
    CoordinatorWorkItem {
        work_item_id: id.to_string(),
        kind,
        description: description.to_string(),
        state,
        subject_entity: Some(EntityId::new()),
        dependencies: Vec::new(),
        required: true,
    }
}

#[tokio::test]
async fn coordinator_tick_executes_expected_handlers_per_work_kind() {
    let mut outputs = HashMap::new();
    for kind in [
        DispatchKind::StaticInvestigation,
        DispatchKind::DynamicInvestigation,
        DispatchKind::SemanticAnalysis,
        DispatchKind::ConflictResolution,
        DispatchKind::Generation,
        DispatchKind::BuildFailure,
        DispatchKind::Verification,
    ] {
        outputs.insert(
            kind,
            HandlerOutput {
                commands: vec![ApplicationCommand::CreateWorkItems(
                    CreateWorkItemsRequest {
                        project: ProjectId::new(),
                        campaign_id: "c".into(),
                        descriptions: vec![format!("{kind:?}")],
                    },
                )],
                raw_response_hash: Some(1),
            },
        );
    }

    let handlers = RecordingHandlers {
        invocations: Arc::new(Mutex::new(Vec::new())),
        outputs: Arc::new(Mutex::new(outputs)),
    };

    let cases: Vec<(WorkItemKind, &str, DispatchKind)> = vec![
        (
            WorkItemKind::Investigation,
            "static: x",
            DispatchKind::StaticInvestigation,
        ),
        (
            WorkItemKind::Investigation,
            "dynamic: x",
            DispatchKind::DynamicInvestigation,
        ),
        (
            WorkItemKind::Investigation,
            "semantic: x",
            DispatchKind::SemanticAnalysis,
        ),
        (
            WorkItemKind::ConflictResolution,
            "",
            DispatchKind::ConflictResolution,
        ),
        (WorkItemKind::Function, "", DispatchKind::Generation),
        (WorkItemKind::BuildFailure, "", DispatchKind::BuildFailure),
        (
            WorkItemKind::VerificationFailure,
            "",
            DispatchKind::Verification,
        ),
    ];

    for (kind, description, expected_dispatch) in cases {
        let client = Arc::new(TestClient::new());
        let mut state = CoordinatorState::default();
        state.work_items.push(work_item(
            &format!("wi-{expected_dispatch:?}"),
            kind.clone(),
            description,
            WorkItemState::Ready,
        ));

        let mut coordinator = Coordinator::new(
            ProjectId::new(),
            "campaign".into(),
            Arc::clone(&client) as Arc<dyn AutoReClient>,
            handlers.clone(),
            CancellationToken::new(),
        );
        coordinator.state = state;

        let result = coordinator.tick().await.unwrap();
        assert!(
            matches!(result, TickResult::Processed(_)),
            "expected Processed for {kind:?}"
        );

        let counts = handlers.invocation_counts();
        assert_eq!(
            counts.get(&expected_dispatch).copied().unwrap_or(0),
            1,
            "expected exactly one {expected_dispatch:?} invocation for {kind:?}"
        );
    }
}

#[tokio::test]
async fn no_progress_on_repeated_identical_model_output_triggers_blockwork() {
    let client = Arc::new(TestClient::new());
    let handlers = RecordingHandlers::with_output(
        DispatchKind::SemanticAnalysis,
        HandlerOutput {
            commands: vec![ApplicationCommand::CreateWorkItems(
                CreateWorkItemsRequest {
                    project: ProjectId::new(),
                    campaign_id: "c".into(),
                    descriptions: vec!["semantic result".into()],
                },
            )],
            raw_response_hash: Some(12345),
        },
    );

    let mut coordinator = Coordinator::with_config(
        ProjectId::new(),
        "campaign".into(),
        Arc::clone(&client) as Arc<dyn AutoReClient>,
        CoordinatorConfig {
            no_progress_threshold: 3,
            max_promotions_per_tick: 100,
        },
        handlers,
        CancellationToken::new(),
    );

    coordinator.state.work_items.push(work_item(
        "wi-1",
        WorkItemKind::Investigation,
        "semantic: x",
        WorkItemState::Ready,
    ));

    // First two ticks record the hash but do not block.
    assert!(matches!(
        coordinator.tick().await.unwrap(),
        TickResult::Processed(_)
    ));
    assert!(matches!(
        coordinator.tick().await.unwrap(),
        TickResult::Processed(_)
    ));

    // Third identical hash triggers BlockWorkItem.
    let result = coordinator.tick().await.unwrap();
    assert!(matches!(result, TickResult::Blocked(ref id) if id == "wi-1"));

    let block_reasons: Vec<String> = client
        .commands()
        .into_iter()
        .filter_map(|c| match c {
            ApplicationCommand::BlockWorkItem(BlockWorkItemRequest { reason, .. }) => Some(reason),
            _ => None,
        })
        .collect();
    assert_eq!(block_reasons.len(), 1);
    assert!(
        block_reasons[0].starts_with("RepeatedIdenticalModelOutput"),
        "unexpected reason: {}",
        block_reasons[0]
    );
}

#[tokio::test]
async fn coordinator_respects_cancellation_token_between_ticks() {
    let client = Arc::new(TestClient::new());
    let cancel = CancellationToken::new();
    let handlers = RecordingHandlers::default();

    let mut coordinator = Coordinator::new(
        ProjectId::new(),
        "campaign".into(),
        Arc::clone(&client) as Arc<dyn AutoReClient>,
        handlers,
        cancel.clone(),
    );
    coordinator.state.work_items.push(work_item(
        "wi-1",
        WorkItemKind::Function,
        "",
        WorkItemState::Ready,
    ));

    cancel.cancel();
    let result = coordinator.tick().await.unwrap();
    assert_eq!(result, TickResult::Cancelled);
}

#[tokio::test]
async fn coordinator_refresh_reconciles_interrupted_operations() {
    let client = Arc::new(TestClient::new());
    let handlers = RecordingHandlers::with_output(
        DispatchKind::Generation,
        HandlerOutput {
            commands: vec![ApplicationCommand::CreateWorkItems(
                CreateWorkItemsRequest {
                    project: ProjectId::new(),
                    campaign_id: "c".into(),
                    descriptions: vec!["generated".into()],
                },
            )],
            raw_response_hash: None,
        },
    );

    let mut coordinator = Coordinator::new(
        ProjectId::new(),
        "campaign".into(),
        Arc::clone(&client) as Arc<dyn AutoReClient>,
        handlers,
        CancellationToken::new(),
    );

    coordinator.state.work_items.push(CoordinatorWorkItem {
        work_item_id: "leased".into(),
        kind: WorkItemKind::Function,
        description: String::new(),
        state: WorkItemState::Leased,
        subject_entity: None,
        dependencies: Vec::new(),
        required: true,
    });
    coordinator.state.work_items.push(CoordinatorWorkItem {
        work_item_id: "running".into(),
        kind: WorkItemKind::Function,
        description: String::new(),
        state: WorkItemState::Running,
        subject_entity: None,
        dependencies: Vec::new(),
        required: true,
    });
    coordinator.state.work_items.push(CoordinatorWorkItem {
        work_item_id: "ready".into(),
        kind: WorkItemKind::Function,
        description: String::new(),
        state: WorkItemState::Ready,
        subject_entity: None,
        dependencies: Vec::new(),
        required: true,
    });

    // Ready item exists, so tick will dispatch after reconciliation.
    coordinator.tick().await.unwrap();

    let requeued: Vec<String> = client
        .commands()
        .into_iter()
        .filter_map(|c| match c {
            ApplicationCommand::RequeueWorkItem(RequeueWorkItemRequest {
                work_item_id, ..
            }) => Some(work_item_id),
            _ => None,
        })
        .collect();

    assert!(requeued.contains(&"leased".to_string()));
    assert!(requeued.contains(&"running".to_string()));
    assert!(!requeued.contains(&"ready".to_string()));
}

#[tokio::test]
async fn coordinator_completeness_does_not_pass_with_terminal_blocked() {
    let client = Arc::new(TestClient::new());
    let handlers = RecordingHandlers::default();

    let mut coordinator = Coordinator::new(
        ProjectId::new(),
        "campaign".into(),
        Arc::clone(&client) as Arc<dyn AutoReClient>,
        handlers,
        CancellationToken::new(),
    );

    coordinator.state.work_items.push(CoordinatorWorkItem {
        work_item_id: "done".into(),
        kind: WorkItemKind::Function,
        description: String::new(),
        state: WorkItemState::Completed,
        subject_entity: None,
        dependencies: Vec::new(),
        required: true,
    });
    coordinator.state.work_items.push(CoordinatorWorkItem {
        work_item_id: "blocked".into(),
        kind: WorkItemKind::Function,
        description: String::new(),
        state: WorkItemState::Blocked,
        subject_entity: None,
        dependencies: Vec::new(),
        required: true,
    });

    assert!(CompletionPolicy::is_complete(&coordinator.state));
    assert!(!CompletionPolicy::is_successfully_complete(
        &coordinator.state
    ));

    let result = coordinator.tick().await.unwrap();
    assert_eq!(result, TickResult::Complete);
}

#[test]
fn classify_investigation_covers_all_subkinds() {
    assert_eq!(
        classify_work_item(&work_item(
            "s",
            WorkItemKind::Investigation,
            "static: x",
            WorkItemState::Ready
        )),
        Some(DispatchKind::StaticInvestigation)
    );
    assert_eq!(
        classify_work_item(&work_item(
            "d",
            WorkItemKind::Investigation,
            "dynamic: x",
            WorkItemState::Ready
        )),
        Some(DispatchKind::DynamicInvestigation)
    );
    assert_eq!(
        classify_work_item(&work_item(
            "m",
            WorkItemKind::Investigation,
            "semantic: x",
            WorkItemState::Ready
        )),
        Some(DispatchKind::SemanticAnalysis)
    );
}
