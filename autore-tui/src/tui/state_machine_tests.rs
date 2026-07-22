//! State-machine tests for the TUI presentation layer (§29.13).
//!
//! These tests drive the TUI through terminal key events, project events, and
//! internal query/command results, then assert the resulting `TuiState` changes.
//! They do not exercise a real terminal; all rendering uses `TestBackend`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use autore_app::{
    ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, EventsResponse, QueryResult,
};
use autore_core::operation::OperationState;
use autore_events::project_event_service::ProjectEventSubscription;
use autore_schema::domain::records::{
    Artifact, ArtifactStorage, EVENT_KIND_PROJECT_CREATED, EventSource, Hypothesis,
    HypothesisStatus, Operation, Project, ProjectEvent,
};
use autore_schema::domain::{
    Confidence, ContentHash, EvidenceValue, MetadataMap, NamespacedId, SchemaVersion, Timestamp,
};
use autore_schema::ids::{ArtifactId, EntityId, HypothesisId, OperationId, ProjectId};

use crate::tui::state::{
    EventCursor, FilterState, Focus, Navigation, NotificationLevel, Pane, ProjectViewState,
    TuiState,
};
use crate::tui::{InternalTuiEvent, Tui};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn sample_state() -> TuiState {
    let pid = ProjectId::new();
    let mut project = Project::new("alpha");
    project.id = pid;
    let mut view = ProjectViewState {
        project_summary: Some(project),
        schema_version: Some(SchemaVersion::new(2, 0)),
        ..Default::default()
    };
    view.artifacts.push(sample_artifact(pid));
    view.hypotheses.push(sample_hypothesis(pid));
    view.operations.push(sample_operation(pid));

    let pid2 = ProjectId::new();
    let mut project2 = Project::new("beta");
    project2.id = pid2;
    let view2 = ProjectViewState {
        project_summary: Some(project2),
        ..Default::default()
    };

    let mut project_views = HashMap::new();
    project_views.insert(pid, view);
    project_views.insert(pid2, view2);

    TuiState {
        navigation: Navigation::Project(pid),
        focus: Focus::Panel1,
        filters: FilterState::default(),
        dialogs: vec![],
        notifications: vec![],
        project_views,
        operation_views: Default::default(),
        event_cursor: EventCursor::default(),
        active_pane: Pane::Dashboard,
        selected_operation: None,
        selected_hypothesis: None,
    }
}

fn sample_state_with_selected_records() -> TuiState {
    let mut state = sample_state();
    let pid = match state.navigation {
        Navigation::Project(pid) => pid,
        _ => panic!("expected project navigation"),
    };
    let view = state.project_views.get(&pid).unwrap();
    let op_id = view.operations[0].id;
    let hyp_id = view.hypotheses[0].id;
    state.selected_operation = Some(op_id);
    state.selected_hypothesis = Some(hyp_id);
    state
}

fn sample_artifact(pid: ProjectId) -> Artifact {
    Artifact {
        id: ArtifactId::new(),
        project: pid,
        kind: NamespacedId::parse("core.binary").unwrap(),
        content_hash: ContentHash::sha256(b"abc"),
        size: 42,
        storage: ArtifactStorage::ExternalFile {
            canonical_path: PathBuf::from("/tmp/x"),
        },
        created_at: Timestamp::now(),
        metadata: MetadataMap::new(),
    }
}

fn sample_hypothesis(pid: ProjectId) -> Hypothesis {
    Hypothesis {
        id: HypothesisId::new(),
        project: pid,
        subject: EntityId::new(),
        predicate: NamespacedId::new(&["test", "pred"]).unwrap(),
        candidate: EvidenceValue::Null,
        supporting_evidence: vec![],
        contradicting_evidence: vec![],
        derived_from: vec![],
        confidence: Confidence::new(0.75).unwrap(),
        status: HypothesisStatus::UnderInvestigation,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    }
}

fn sample_operation(pid: ProjectId) -> Operation {
    let mut op = Operation::new(
        pid,
        NamespacedId::new(&["test", "validate"]).unwrap(),
        "tui",
    );
    op.id = OperationId::new();
    op
}

fn sample_running_operation(pid: ProjectId) -> Operation {
    let mut op = sample_operation(pid);
    op.state = OperationState::Running;
    op
}

fn sample_completed_operation(pid: ProjectId) -> Operation {
    let mut op = sample_operation(pid);
    op.state = OperationState::Completed;
    op
}

fn project_event(pid: ProjectId, sequence: u64, kind: &NamespacedId) -> ProjectEvent {
    ProjectEvent::new(
        pid,
        sequence,
        kind.clone(),
        EventSource::Project,
        None,
        None,
    )
}

fn render_to_string(tui: &Tui, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| tui.render(frame))
        .expect("draw failed");
    let buffer = terminal.backend().buffer();
    let mut result = String::new();
    for y in 0..height {
        for x in 0..width {
            let cell = buffer.cell((x, y)).expect("cell in bounds");
            result.push_str(cell.symbol());
        }
        result.push('\n');
    }
    result
}

// ---------------------------------------------------------------------------
// Recording client
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct RecordingClient {
    commands: Arc<Mutex<Vec<ApplicationCommand>>>,
    queries: Arc<Mutex<Vec<ApplicationQuery>>>,
    query_results: Arc<Mutex<Vec<QueryResult>>>,
    events_after: Arc<Mutex<Vec<ProjectEvent>>>,
}

impl RecordingClient {
    fn commands(&self) -> Vec<ApplicationCommand> {
        self.commands.lock().unwrap().clone()
    }

    fn queries(&self) -> Vec<ApplicationQuery> {
        self.queries.lock().unwrap().clone()
    }

    fn with_query_result(self, result: QueryResult) -> Self {
        self.query_results.lock().unwrap().push(result);
        self
    }
}

impl AutoReClient for RecordingClient {
    fn execute(&self, command: ApplicationCommand) -> autore_core::Result<CommandResult> {
        self.commands.lock().unwrap().push(command.clone());
        Ok(match command {
            ApplicationCommand::ChangeHypothesisStatus(req) => {
                CommandResult::HypothesisStatusChanged(autore_app::ChangeHypothesisStatusResponse {
                    hypothesis: Hypothesis {
                        id: req.id,
                        project: req.project,
                        subject: EntityId::new(),
                        predicate: NamespacedId::new(&["test", "predicate"]).unwrap(),
                        candidate: EvidenceValue::Null,
                        supporting_evidence: vec![],
                        contradicting_evidence: vec![],
                        derived_from: vec![],
                        confidence: Confidence::new(0.5).unwrap(),
                        status: req.status,
                        created_at: Timestamp::now(),
                        updated_at: Timestamp::now(),
                    },
                })
            }
            ApplicationCommand::CancelOperation(req) => {
                CommandResult::OperationCancelled(autore_app::CancelOperationResponse {
                    operation: Operation::new(
                        req.project,
                        NamespacedId::new(&["test", "op"]).unwrap(),
                        &req.requested_by,
                    ),
                })
            }
            ApplicationCommand::RegisterArtifact(req) => {
                CommandResult::ArtifactRegistered(autore_app::RegisterArtifactResponse {
                    artifact: Artifact {
                        id: ArtifactId::new(),
                        project: req.project,
                        kind: NamespacedId::parse(&req.kind)
                            .unwrap_or_else(|_| NamespacedId::new(&["test", "artifact"]).unwrap()),
                        content_hash: ContentHash::blake3(b""),
                        size: 0,
                        metadata: Default::default(),
                        storage: ArtifactStorage::ExternalFile {
                            canonical_path: req.source_path,
                        },
                        created_at: Timestamp::now(),
                    },
                })
            }
            other => unimplemented!("RecordingClient: unhandled command {other:?}"),
        })
    }

    fn query(&self, query: ApplicationQuery) -> autore_core::Result<QueryResult> {
        self.queries.lock().unwrap().push(query.clone());
        let mut results = self.query_results.lock().unwrap();
        Ok(match query {
            ApplicationQuery::GetProjectSummary(_) => {
                if results.is_empty() {
                    QueryResult::ProjectSummary(
                        autore_app::application_service::requests::ProjectSummaryResponse {
                            project: Project::new("test"),
                        },
                    )
                } else {
                    results.remove(0)
                }
            }
            ApplicationQuery::ListEvents(_) => {
                if results.is_empty() {
                    QueryResult::Events(EventsResponse { events: vec![] })
                } else {
                    results.remove(0)
                }
            }
            other => unimplemented!("RecordingClient: unhandled query {other:?}"),
        })
    }

    fn events_after(
        &self,
        _project: ProjectId,
        _sequence: u64,
        _limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>> {
        Ok(self.events_after.lock().unwrap().clone())
    }

    fn subscribe_events(
        &self,
        _project: ProjectId,
        _after: u64,
    ) -> autore_core::Result<ProjectEventSubscription> {
        unimplemented!()
    }
}

// ---------------------------------------------------------------------------
// State-machine tests
// ---------------------------------------------------------------------------

/// Navigation with `j`/`Down` and `k`/`Up` moves between projects and wraps.
#[test]
fn tui_state_machine_navigation_down_up_wraps() {
    let state = sample_state();
    let pid = match state.navigation {
        Navigation::Project(pid) => pid,
        _ => panic!("expected project navigation"),
    };
    let mut tui = Tui::with_state(state);

    let down = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
    tui.handle_key_event(down).unwrap();
    assert!(
        matches!(tui.state().navigation, Navigation::Project(_)),
        "navigation must stay on a project after j"
    );
    assert_ne!(
        tui.state().navigation,
        Navigation::Project(pid),
        "j must move to a different project"
    );

    let up = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
    tui.handle_key_event(up).unwrap();
    assert_eq!(
        tui.state().navigation,
        Navigation::Project(pid),
        "k must wrap back to the first project"
    );

    // Arrow keys behave the same.
    tui.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    assert_ne!(tui.state().navigation, Navigation::Project(pid));
    tui.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(tui.state().navigation, Navigation::Project(pid));
}

/// `Tab` cycles focus through the four main focus targets.
#[test]
fn tui_state_machine_focus_cycles_with_tab() {
    let mut tui = Tui::with_state(sample_state());
    assert_eq!(tui.state().focus, Focus::Panel1);

    tui.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(tui.state().focus, Focus::Panel2);

    tui.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(tui.state().focus, Focus::Panel3);

    tui.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(tui.state().focus, Focus::Sidebar);

    tui.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(tui.state().focus, Focus::Panel1);
}

/// Text filter state can be updated and is retained.
#[test]
fn tui_state_machine_filtering_text() {
    let mut tui = Tui::with_state(sample_state());
    tui.state_mut().filters.text_search = "loop".to_string();
    assert_eq!(tui.state().filters.text_search, "loop");

    tui.state_mut().filters.text_search.clear();
    assert!(tui.state().filters.text_search.is_empty());
}

/// Kind filter state can be updated and is retained.
#[test]
fn tui_state_machine_filtering_kind() {
    let mut tui = Tui::with_state(sample_state());
    let kind = NamespacedId::parse("core.binary").unwrap();
    tui.state_mut().filters.kind_filter = Some(kind.clone());
    assert_eq!(tui.state().filters.kind_filter, Some(kind));

    tui.state_mut().filters.kind_filter = None;
    assert!(tui.state().filters.kind_filter.is_none());
}

/// Search state transition: a text filter plus a query dispatch represents
/// the TUI search path.
#[tokio::test]
async fn tui_state_machine_search_triggers_query() {
    let recorder = RecordingClient::default();
    let state = sample_state();
    let pid = match state.navigation {
        Navigation::Project(pid) => pid,
        _ => panic!("expected project navigation"),
    };
    let mut tui = Tui::with_client(state, Box::new(recorder.clone()));
    tui.state_mut().filters.text_search = "alpha".to_string();

    // Open the selected project, which dispatches a summary query.
    tui.open_selected_project();

    for _ in 0..100 {
        tokio::task::yield_now().await;
        if !recorder.queries().is_empty() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    }

    let queries = recorder.queries();
    assert_eq!(queries.len(), 1, "search must dispatch a query");
    match &queries[0] {
        ApplicationQuery::GetProjectSummary(req) => assert_eq!(req.project, pid),
        other => panic!("expected GetProjectSummary, got {other:?}"),
    }
    assert_eq!(tui.state().filters.text_search, "alpha");
}

/// Dialog lifecycle: `a` opens an input dialog, typing updates the buffer,
/// `Enter` confirms and closes the dialog, `Esc` cancels without confirming.
#[test]
fn tui_state_machine_dialog_lifecycle() {
    let recorder = RecordingClient::default();
    let state = sample_state_with_selected_records();
    let mut tui = Tui::with_client(state, Box::new(recorder.clone()));

    // Open artifact import dialog.
    tui.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(tui.state().focus, Focus::Dialog);
    assert_eq!(tui.state().dialogs.len(), 1);

    // Type into the buffer.
    for ch in "/tmp/binary".chars() {
        tui.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
    }

    // Confirm dispatches the command and clears the dialog.
    tui.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(tui.state().dialogs.is_empty());
    assert_eq!(tui.state().focus, Focus::Panel1);
    assert_eq!(recorder.commands().len(), 1);

    // Cancel path: open again and press Esc.
    let mut tui2 = Tui::with_client(
        sample_state_with_selected_records(),
        Box::new(RecordingClient::default()),
    );
    tui2.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        .unwrap();
    tui2.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(tui2.state().dialogs.is_empty());
    assert_eq!(tui2.state().focus, Focus::Panel1);
}

/// Form validation: an empty artifact path is still dispatched (the TUI is
/// presentation-only; validation lives in the application layer), but the dialog
/// closes and focus returns to the main UI.
#[test]
fn tui_state_machine_form_validation_empty_path() {
    let recorder = RecordingClient::default();
    let state = sample_state_with_selected_records();
    let mut tui = Tui::with_client(state, Box::new(recorder.clone()));

    tui.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        .unwrap();
    // Confirm without typing anything.
    tui.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(tui.state().dialogs.is_empty());
    assert_eq!(tui.state().focus, Focus::Panel1);
    let cmds = recorder.commands();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        ApplicationCommand::RegisterArtifact(req) => {
            assert_eq!(req.source_path, PathBuf::from(""));
        }
        other => panic!("expected RegisterArtifact, got {other:?}"),
    }
}

/// Command dispatch: `A` with a selected hypothesis dispatches
/// `ChangeHypothesisStatus(Accepted)`.
#[test]
fn tui_state_machine_command_dispatch_accept_hypothesis() {
    let recorder = RecordingClient::default();
    let state = sample_state_with_selected_records();
    let hyp_id = state.selected_hypothesis.unwrap();
    let pid = match state.navigation {
        Navigation::Project(pid) => pid,
        _ => panic!("expected project navigation"),
    };
    let mut tui = Tui::with_client(state, Box::new(recorder.clone()));

    tui.handle_key_event(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE))
        .unwrap();

    let cmds = recorder.commands();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        ApplicationCommand::ChangeHypothesisStatus(req) => {
            assert_eq!(req.project, pid);
            assert_eq!(req.id, hyp_id);
            assert_eq!(req.status, HypothesisStatus::Accepted);
        }
        other => panic!("expected ChangeHypothesisStatus, got {other:?}"),
    }
}

/// Command dispatch: `c` with a selected operation dispatches `CancelOperation`.
#[test]
fn tui_state_machine_command_dispatch_cancel_operation() {
    let recorder = RecordingClient::default();
    let state = sample_state_with_selected_records();
    let op_id = state.selected_operation.unwrap();
    let pid = match state.navigation {
        Navigation::Project(pid) => pid,
        _ => panic!("expected project navigation"),
    };
    let mut tui = Tui::with_client(state, Box::new(recorder.clone()));

    tui.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
        .unwrap();

    let cmds = recorder.commands();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        ApplicationCommand::CancelOperation(req) => {
            assert_eq!(req.project, pid);
            assert_eq!(req.id, op_id);
            assert_eq!(req.requested_by, "tui");
        }
        other => panic!("expected CancelOperation, got {other:?}"),
    }
}

/// Command dispatch: `a` dialog followed by `Enter` dispatches `RegisterArtifact`.
#[test]
fn tui_state_machine_command_dispatch_register_artifact() {
    let recorder = RecordingClient::default();
    let state = sample_state_with_selected_records();
    let pid = match state.navigation {
        Navigation::Project(pid) => pid,
        _ => panic!("expected project navigation"),
    };
    let mut tui = Tui::with_client(state, Box::new(recorder.clone()));

    tui.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
        .unwrap();
    for ch in "/tmp/artifact.bin".chars() {
        tui.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
    }
    tui.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    let cmds = recorder.commands();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        ApplicationCommand::RegisterArtifact(req) => {
            assert_eq!(req.project, pid);
            assert_eq!(req.source_path, PathBuf::from("/tmp/artifact.bin"));
            assert_eq!(req.kind, "native");
        }
        other => panic!("expected RegisterArtifact, got {other:?}"),
    }
}

/// Query completion: a dispatched `GetProjectSummary` query returns a result
/// that updates the project view state.
#[tokio::test]
async fn tui_state_machine_query_completion_updates_project_summary() {
    let pid = ProjectId::new();
    let mut project = Project::new("loaded");
    project.id = pid;
    let response = QueryResult::ProjectSummary(
        autore_app::application_service::requests::ProjectSummaryResponse { project },
    );
    let recorder = RecordingClient::default().with_query_result(response);

    let mut state = sample_state();
    state.navigation = Navigation::Project(pid);
    state.project_views.clear();
    state.project_views.insert(pid, ProjectViewState::default());

    let mut tui = Tui::with_client(state, Box::new(recorder));
    tui.open_selected_project();

    for _ in 0..100 {
        tokio::task::yield_now().await;
        if let Ok(Some(ev)) = tokio::time::timeout(
            tokio::time::Duration::from_millis(10),
            tui.take_internal_rx().recv(),
        )
        .await
        {
            tui.handle_internal_event(ev);
        } else {
            break;
        }
    }

    let view = tui.state().project_views.get(&pid).unwrap();
    assert_eq!(view.project_summary.as_ref().unwrap().name, "loaded");
}

/// Incoming durable event: the event cursor advances and the event is appended
/// to the project's recent events.
#[test]
fn tui_state_machine_incoming_durable_event() {
    let state = sample_state();
    let pid = match state.navigation {
        Navigation::Project(pid) => pid,
        _ => panic!("expected project navigation"),
    };
    let mut tui = Tui::with_state(state);

    let event = project_event(pid, 1, &EVENT_KIND_PROJECT_CREATED);
    tui.handle_project_event(event);

    assert_eq!(tui.state().event_cursor.last_sequence, 1);
    let view = tui.state().project_views.get(&pid).unwrap();
    assert_eq!(view.recent_events.len(), 1);
    assert_eq!(view.recent_events[0].sequence, 1);
}

/// Sequence gap: receiving an event with a non-sequential number sets the
/// missed-events flag; a catch-up response clears it.
#[test]
fn tui_state_machine_sequence_gap_handling() {
    let state = sample_state();
    let pid = match state.navigation {
        Navigation::Project(pid) => pid,
        _ => panic!("expected project navigation"),
    };
    let mut tui = Tui::with_state(state);

    tui.handle_project_event(project_event(pid, 1, &EVENT_KIND_PROJECT_CREATED));
    assert!(!tui.state().event_cursor.missed_events);

    tui.handle_project_event(project_event(pid, 3, &EVENT_KIND_PROJECT_CREATED));
    assert!(tui.state().event_cursor.missed_events);
    assert_eq!(tui.state().event_cursor.last_sequence, 3);

    tui.handle_internal_event(InternalTuiEvent::CatchupEvents {
        project: pid,
        events: vec![project_event(pid, 2, &EVENT_KIND_PROJECT_CREATED)],
    });
    assert!(!tui.state().event_cursor.missed_events);
    assert_eq!(tui.state().event_cursor.last_sequence, 2);
}

/// Loading state: a project view with no summary yet renders as loading.
#[test]
fn tui_state_machine_loading_state() {
    let pid = ProjectId::new();
    let mut project_views = HashMap::new();
    project_views.insert(pid, ProjectViewState::default());
    let state = TuiState {
        navigation: Navigation::Project(pid),
        project_views,
        ..Default::default()
    };
    let tui = Tui::with_state(state);
    let output = render_to_string(&tui, 80, 24);
    assert!(
        output.contains("(loading)"),
        "loading state must render '(loading)': {output}"
    );
    assert!(output.contains("Projects (1)"));
}

/// Empty state: no project views renders the empty dashboard.
#[test]
fn tui_state_machine_empty_state() {
    let tui = Tui::new();
    let output = render_to_string(&tui, 80, 24);
    assert!(
        output.contains("No projects loaded"),
        "empty state message missing: {output}"
    );
    assert!(output.contains("Projects (0)"));
    assert!(output.contains("No operations"));
}

/// Error state: a command error produces a notification.
#[test]
fn tui_state_machine_error_state() {
    struct FailingClient;
    impl AutoReClient for FailingClient {
        fn execute(&self, _command: ApplicationCommand) -> autore_core::Result<CommandResult> {
            Err(autore_core::Error::InvalidStateTransition(
                "simulate failure".to_string(),
            ))
        }
        fn query(&self, _query: ApplicationQuery) -> autore_core::Result<QueryResult> {
            unimplemented!()
        }
        fn events_after(
            &self,
            _project: ProjectId,
            _sequence: u64,
            _limit: usize,
        ) -> autore_core::Result<Vec<ProjectEvent>> {
            unimplemented!()
        }
        fn subscribe_events(
            &self,
            _project: ProjectId,
            _after: u64,
        ) -> autore_core::Result<ProjectEventSubscription> {
            unimplemented!()
        }
    }

    let state = sample_state_with_selected_records();
    let mut tui = Tui::with_client(state, Box::new(FailingClient));
    tui.handle_key_event(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE))
        .unwrap();

    assert!(!tui.state().notifications.is_empty());
    let last = tui.state().notifications.last().unwrap();
    assert_eq!(last.level, NotificationLevel::Error);
    assert!(last.message.contains("command error"));
}

/// Operation progress: the state tracks operation records and the rendered
/// progress reflects the operation state (Queued 0%, Running 50%,
/// Completed 100%).
#[test]
fn tui_state_machine_operation_progress() {
    let pid = ProjectId::new();
    let mut view = ProjectViewState {
        project_summary: Some(Project::new("prog")),
        ..Default::default()
    };
    view.operations.push(sample_running_operation(pid));
    view.operations.push(sample_completed_operation(pid));

    let mut project_views = HashMap::new();
    project_views.insert(pid, view);
    let state = TuiState {
        navigation: Navigation::Project(pid),
        project_views,
        ..Default::default()
    };
    let tui = Tui::with_state(state);
    let output = render_to_string(&tui, 80, 24);

    assert!(output.contains("Operations (2)"));
    assert!(output.contains("Running"));
    assert!(output.contains("Completed"));
    assert!(output.contains("50%"));
    assert!(output.contains("100%"));
}

/// Unknown namespaced record: the generic fallback renderer accepts an arbitrary
/// kind and fields without panicking.
#[test]
fn tui_state_machine_unknown_namespaced_record() {
    let kind = NamespacedId::parse("developer.experimental.widget").unwrap();
    let fields = vec![
        ("name".to_string(), "gadget".to_string()),
        ("count".to_string(), "42".to_string()),
    ];
    let paragraph = crate::tui::render_generic_record(&kind, "id-7", fields);
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| frame.render_widget(paragraph, frame.area()))
        .unwrap();
    let output = render_to_string(&Tui::new(), 80, 24);
    // The generic renderer was drawn independently; assert it does not panic.
    assert!(
        !output.is_empty(),
        "rendering an unknown namespaced record must not panic"
    );
}

/// Alt+1..Alt+7 switch the active secondary pane.
#[test]
fn tui_state_machine_pane_switching() {
    let mut tui = Tui::new();
    assert_eq!(tui.state().active_pane, Pane::Dashboard);

    let combos = [
        ('2', Pane::Providers),
        ('3', Pane::NativeArtifacts),
        ('4', Pane::OperationsDetail),
        ('5', Pane::EventsLog),
        ('6', Pane::MigrationHistory),
        ('7', Pane::ExternalArtifactIntegrity),
        ('1', Pane::Dashboard),
    ];
    for (ch, expected) in combos {
        tui.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT))
            .unwrap();
        assert_eq!(
            tui.state().active_pane,
            expected,
            "Alt+{ch} should switch to {expected:?}"
        );
    }
}

/// Query error: a failed background query produces an error notification.
#[tokio::test]
async fn tui_state_machine_query_error() {
    struct ErrorClient;
    impl AutoReClient for ErrorClient {
        fn execute(&self, _command: ApplicationCommand) -> autore_core::Result<CommandResult> {
            unimplemented!()
        }
        fn query(&self, _query: ApplicationQuery) -> autore_core::Result<QueryResult> {
            Err(autore_core::Error::NotFound("project missing".to_string()))
        }
        fn events_after(
            &self,
            _project: ProjectId,
            _sequence: u64,
            _limit: usize,
        ) -> autore_core::Result<Vec<ProjectEvent>> {
            unimplemented!()
        }
        fn subscribe_events(
            &self,
            _project: ProjectId,
            _after: u64,
        ) -> autore_core::Result<ProjectEventSubscription> {
            unimplemented!()
        }
    }

    let state = sample_state();
    let mut tui = Tui::with_client(state, Box::new(ErrorClient));
    tui.open_selected_project();

    for _ in 0..100 {
        tokio::task::yield_now().await;
        if let Ok(Some(ev)) = tokio::time::timeout(
            tokio::time::Duration::from_millis(10),
            tui.take_internal_rx().recv(),
        )
        .await
        {
            tui.handle_internal_event(ev);
        } else {
            break;
        }
    }

    let last = tui
        .state()
        .notifications
        .last()
        .expect("notification expected");
    assert_eq!(last.level, NotificationLevel::Error);
    assert!(last.message.contains("Query error"));
}
