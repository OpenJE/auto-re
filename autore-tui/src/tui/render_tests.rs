//! Deterministic render tests for the TUI (§29.14).
//!
//! These tests create a `TuiState` with deterministic data, draw it to a
//! `TestBackend` of fixed size (80x24), and assert that the buffer contains
//! expected semantic strings (panel titles, key labels, summary counts) without
//! relying on brittle full-screen whitespace snapshots.

use std::collections::HashMap;
use std::path::PathBuf;

use autore_schema::domain::records::{
    Artifact, ArtifactStorage, EventSource, Hypothesis, HypothesisStatus, Operation, Project,
    ProjectEvent,
};
use autore_schema::domain::{
    Confidence, ContentHash, EvidenceValue, MetadataMap, NamespacedId, SchemaVersion, Timestamp,
};
use autore_schema::ids::{ArtifactId, EntityId, HypothesisId, OperationId, ProjectId};

use crate::tui::Tui;
use crate::tui::state::{
    EventCursor, FilterState, Focus, Navigation, Pane, ProjectViewState, TuiState, ValidationStatus,
};

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

fn sample_operation(pid: ProjectId, state: autore_core::operation::OperationState) -> Operation {
    let mut op = Operation::new(
        pid,
        NamespacedId::new(&["test", "validate"]).unwrap(),
        "tui",
    );
    op.id = OperationId::new();
    op.state = state;
    op
}

fn populated_state() -> TuiState {
    let pid = ProjectId::new();
    let mut project = Project::new("alpha");
    project.id = pid;

    let mut view = ProjectViewState {
        project_summary: Some(project),
        schema_version: Some(SchemaVersion::new(2, 0)),
        validation_status: Some(ValidationStatus::Ok),
        ..Default::default()
    };
    view.artifacts.push(sample_artifact(pid));
    view.hypotheses.push(sample_hypothesis(pid));
    view.operations.push(sample_operation(
        pid,
        autore_core::operation::OperationState::Running,
    ));
    view.operations.push(sample_operation(
        pid,
        autore_core::operation::OperationState::Completed,
    ));
    view.recent_events.push(ProjectEvent::new(
        pid,
        1,
        NamespacedId::parse("core.project.created").unwrap(),
        EventSource::Project,
        None,
        None,
    ));

    let mut project_views = HashMap::new();
    project_views.insert(pid, view);

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

/// Empty dashboard at 80x24 shows the expected panel titles and empty messages.
#[test]
fn tui_render_dashboard_empty_state() {
    let tui = Tui::new();
    let output = render_to_string(&tui, 80, 24);

    assert!(
        output.contains("Projects (0)"),
        "project title/count missing: {output}"
    );
    assert!(
        output.contains("No projects loaded"),
        "empty project message missing: {output}"
    );
    assert!(
        output.contains("Operations"),
        "operations panel title missing: {output}"
    );
    assert!(
        output.contains("No operations"),
        "empty operations message missing: {output}"
    );
    assert!(
        output.contains("Hypotheses + Evidence"),
        "hypotheses panel title missing: {output}"
    );
}

/// Populated dashboard at 80x24 shows project summary, validation status, and
/// semantic counts.
#[test]
fn tui_render_dashboard_project_summary() {
    let tui = Tui::with_state(populated_state());
    let output = render_to_string(&tui, 80, 24);

    assert!(
        output.contains("Project:"),
        "project label missing: {output}"
    );
    assert!(output.contains("alpha"), "project name missing: {output}");
    assert!(output.contains("Schema:"), "schema label missing: {output}");
    assert!(output.contains("v2.0"), "schema version missing: {output}");
    assert!(
        output.contains("Valid:"),
        "validation label missing: {output}"
    );
    assert!(output.contains("ok"), "validation status missing: {output}");
    assert!(
        output.contains("Counts:"),
        "counts heading missing: {output}"
    );
    assert!(
        output.contains("artifacts:"),
        "artifacts label missing: {output}"
    );
    assert!(
        output.contains("hypotheses:"),
        "hypotheses label missing: {output}"
    );
}

/// Operations panel renders progress percentages and cancel hints.
#[test]
fn tui_render_operations_progress() {
    let tui = Tui::with_state(populated_state());
    let output = render_to_string(&tui, 80, 24);

    assert!(
        output.contains("Operations (2)"),
        "operations count missing: {output}"
    );
    assert!(
        output.contains("Running"),
        "running state missing: {output}"
    );
    assert!(
        output.contains("Completed"),
        "completed state missing: {output}"
    );
    assert!(output.contains("50%"), "running progress missing: {output}");
    assert!(
        output.contains("100%"),
        "completed progress missing: {output}"
    );
    assert!(output.contains("[c]"), "cancel hint missing: {output}");
}

/// Generic fallback renderer shows an unknown namespaced record with field
/// values at fixed dimensions.
#[test]
fn tui_render_unknown_namespaced_record() {
    let kind = NamespacedId::parse("developer.experimental.foo").unwrap();
    let paragraph = crate::tui::render_generic_record(
        &kind,
        "rec-42",
        vec![
            ("bar".to_string(), "baz".to_string()),
            ("count".to_string(), "7".to_string()),
        ],
    );

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| frame.render_widget(paragraph, frame.area()))
        .expect("render must not panic");

    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for y in 0..24 {
        for x in 0..80 {
            let cell = buffer.cell((x, y)).expect("cell in bounds");
            output.push_str(cell.symbol());
        }
        output.push('\n');
    }

    assert!(
        output.contains("developer.experimental.foo rec-42"),
        "fallback title missing: {output}"
    );
    assert!(output.contains("bar: baz"), "field bar missing: {output}");
    assert!(output.contains("count: 7"), "field count missing: {output}");
}

/// Secondary pane rendering at fixed dimensions preserves semantic titles.
#[test]
fn tui_render_secondary_pane_events_log() {
    let mut state = populated_state();
    state.active_pane = Pane::EventsLog;
    let tui = Tui::with_state(state);
    let output = render_to_string(&tui, 80, 24);

    assert!(
        output.contains("EventsLog"),
        "events log title missing: {output}"
    );
    assert!(
        output.contains("Events log:"),
        "events log heading missing: {output}"
    );
    assert!(output.contains("seq=1"), "event sequence missing: {output}");
}
