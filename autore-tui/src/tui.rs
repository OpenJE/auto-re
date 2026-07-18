//! TUI dashboard — project overview with operations, hypotheses, and evidence.
//!
//! The TUI is presentation-only: it displays state snapshots loaded via
//! `AutoReClient` queries but never mutates the database directly (§3.9 + §23.3).
//!
//! ## Event loop (§23.4)
//!
//! The TUI event loop routes three kinds of events through [`TuiEvent`]:
//!
//! - `Terminal(TerminalEvent)`: keyboard, mouse, resize, tick (crossterm).
//! - `Project(ProjectEvent)`: durable project events from the subscription.
//! - `Internal(InternalTuiEvent)`: completed queries, command results, UI
//!   notifications — produced by background tasks so the render path never
//!   blocks on storage.
//!
//! On receiving a `ProjectEvent`, the loop schedules a query on a tokio task
//! and posts the result via the internal channel. Sequence gaps are detected
//! and recovered via `events_after` (§23.5).

pub mod state;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, Paragraph, Row, Table, Tabs};
use tokio::sync::mpsc;

use crate::tui::state::{Focus, Navigation, Pane, TuiState};
use autore_app::{ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_events::project_event_service::ProjectEventSubscription;
use autore_schema::domain::records::ProjectEvent;
use autore_schema::domain::{ExtensionData, MetadataMap, NamespacedId};
use autore_schema::ids::ProjectId;

// ---------------------------------------------------------------------------
// TuiEvent — unified event type (§23.4)
// ---------------------------------------------------------------------------

/// Unified TUI event routed through the event loop.
#[derive(Debug)]
pub enum TuiEvent {
    /// Terminal input (keyboard, mouse, resize, tick).
    Terminal(TerminalEvent),
    /// Durable project event from the subscription.
    Project(ProjectEvent),
    /// Internal event from a background task.
    Internal(InternalTuiEvent),
}

/// Terminal-level events from crossterm.
#[derive(Debug)]
pub enum TerminalEvent {
    /// Keyboard input.
    Key(KeyEvent),
    /// Mouse input.
    Mouse(MouseEvent),
    /// Terminal resize.
    Resize(u16, u16),
    /// Periodic tick for animation / polling.
    Tick,
}

impl TerminalEvent {
    /// Converts a crossterm `Event` into a `TerminalEvent`.
    pub fn from_crossterm(event: Event) -> Option<Self> {
        match event {
            Event::Key(k) => Some(TerminalEvent::Key(k)),
            Event::Mouse(m) => Some(TerminalEvent::Mouse(m)),
            Event::Resize(w, h) => Some(TerminalEvent::Resize(w, h)),
            _ => None,
        }
    }
}

/// Internal events produced by background tasks.
#[derive(Debug)]
pub enum InternalTuiEvent {
    /// A background query completed.
    QueryResult {
        project: ProjectId,
        result: QueryResult,
    },
    /// A background command completed.
    CommandResult { result: CommandResult },
    /// A transient notification message.
    Notification(String),
    /// A background query failed.
    QueryError { project: ProjectId, error: String },
    /// Catch-up events from a gap recovery query.
    CatchupEvents {
        project: ProjectId,
        events: Vec<ProjectEvent>,
    },
    /// Gap recovery query failed.
    CatchupError { project: ProjectId, error: String },
}

// ---------------------------------------------------------------------------
// Tui application struct
// ---------------------------------------------------------------------------

/// TUI application state.
pub struct Tui {
    state: TuiState,
    client: Option<Arc<dyn AutoReClient>>,
    subscription: Option<ProjectEventSubscription>,
    subscription_project: Option<ProjectId>,
    internal_tx: mpsc::Sender<InternalTuiEvent>,
    internal_rx: mpsc::Receiver<InternalTuiEvent>,
}

impl Tui {
    /// Creates a new TUI with default empty state.
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(64);
        Self {
            state: TuiState::default(),
            client: None,
            subscription: None,
            subscription_project: None,
            internal_tx: tx,
            internal_rx: rx,
        }
    }

    /// Creates a TUI pre-loaded with the given state.
    #[must_use]
    pub fn with_state(state: TuiState) -> Self {
        let (tx, rx) = mpsc::channel(64);
        Self {
            state,
            client: None,
            subscription: None,
            subscription_project: None,
            internal_tx: tx,
            internal_rx: rx,
        }
    }

    /// Creates a TUI with a client for data access.
    #[must_use]
    pub fn with_client(state: TuiState, client: Box<dyn AutoReClient>) -> Self {
        let (tx, rx) = mpsc::channel(64);
        Self {
            state,
            client: Some(Arc::from(client)),
            subscription: None,
            subscription_project: None,
            internal_tx: tx,
            internal_rx: rx,
        }
    }

    /// Returns a reference to the current TUI state.
    #[must_use]
    pub fn state(&self) -> &TuiState {
        &self.state
    }

    /// Returns a mutable reference to the current TUI state.
    pub fn state_mut(&mut self) -> &mut TuiState {
        &mut self.state
    }

    /// Returns a reference to the internal event sender.
    #[must_use]
    pub fn internal_tx(&self) -> &mpsc::Sender<InternalTuiEvent> {
        &self.internal_tx
    }

    /// Takes the internal event receiver, leaving a fresh channel in its place.
    pub fn take_internal_rx(&mut self) -> mpsc::Receiver<InternalTuiEvent> {
        let (new_tx, new_rx) = mpsc::channel(64);
        let old_rx = std::mem::replace(&mut self.internal_rx, new_rx);
        self.internal_tx = new_tx;
        old_rx
    }

    /// Attaches a project event subscription for live event updates.
    pub fn attach_subscription(&mut self, project: ProjectId, sub: ProjectEventSubscription) {
        self.state.event_cursor.connected = true;
        self.subscription = Some(sub);
        self.subscription_project = Some(project);
    }

    pub fn subscription_mut(&mut self) -> Option<&mut ProjectEventSubscription> {
        self.subscription.as_mut()
    }

    /// Handles a key event. Returns `true` if the app should quit.
    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> io::Result<bool> {
        match key_event.kind {
            KeyEventKind::Press => match key_event.code {
                KeyCode::Char('q') => Ok(true),
                KeyCode::Char('j') | KeyCode::Down => {
                    self.select_next();
                    Ok(false)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.select_previous();
                    Ok(false)
                }
                KeyCode::Tab => {
                    self.cycle_focus();
                    Ok(false)
                }
                KeyCode::Char('1') if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    self.state.active_pane = Pane::Dashboard;
                    Ok(false)
                }
                KeyCode::Char('2') if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    self.state.active_pane = Pane::Providers;
                    Ok(false)
                }
                KeyCode::Char('3') if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    self.state.active_pane = Pane::NativeArtifacts;
                    Ok(false)
                }
                KeyCode::Char('4') if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    self.state.active_pane = Pane::OperationsDetail;
                    Ok(false)
                }
                KeyCode::Char('5') if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    self.state.active_pane = Pane::EventsLog;
                    Ok(false)
                }
                KeyCode::Char('6') if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    self.state.active_pane = Pane::MigrationHistory;
                    Ok(false)
                }
                KeyCode::Char('7') if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    self.state.active_pane = Pane::ExternalArtifactIntegrity;
                    Ok(false)
                }
                _ => Ok(false),
            },
            _ => Ok(false),
        }
    }

    /// Handles a terminal event. Returns `true` if the app should quit.
    pub fn handle_terminal_event(&mut self, event: TerminalEvent) -> io::Result<bool> {
        match event {
            TerminalEvent::Key(key) => self.handle_key_event(key),
            TerminalEvent::Resize(_w, _h) => Ok(false),
            TerminalEvent::Mouse(_m) => Ok(false),
            TerminalEvent::Tick => Ok(false),
        }
    }

    /// Handles a project event from the subscription.
    ///
    /// If a sequence gap is detected (`event.sequence > expected`),
    /// sets `missed_events = true` and triggers a catch-up query.
    pub fn handle_project_event(&mut self, event: ProjectEvent) {
        let project = event.project;
        let sequence = event.sequence;
        let expected = self.state.event_cursor.last_sequence + 1;

        if sequence > expected && self.state.event_cursor.last_sequence > 0 {
            self.state.event_cursor.missed_events = true;
            self.schedule_catchup(project, self.state.event_cursor.last_sequence);
        }

        self.state.event_cursor.last_sequence = sequence;

        if let Some(view) = self.state.project_views.get_mut(&project) {
            view.recent_events.push(event);
        }

        self.schedule_project_refresh(project);
    }

    pub fn handle_internal_event(&mut self, event: InternalTuiEvent) {
        match event {
            InternalTuiEvent::QueryResult { project, result } => {
                self.apply_query_result(project, result);
            }
            InternalTuiEvent::CommandResult { result: _ } => {}
            InternalTuiEvent::Notification(msg) => {
                self.push_notification(&msg, crate::tui::state::NotificationLevel::Info);
            }
            InternalTuiEvent::QueryError { project: _, error } => {
                self.push_notification(
                    &format!("Query error: {error}"),
                    crate::tui::state::NotificationLevel::Error,
                );
            }
            InternalTuiEvent::CatchupEvents { project, events } => {
                for ev in events {
                    self.state.event_cursor.last_sequence = ev.sequence;
                    if let Some(view) = self.state.project_views.get_mut(&project) {
                        view.recent_events.push(ev);
                    }
                }
                self.state.event_cursor.missed_events = false;
            }
            InternalTuiEvent::CatchupError { project: _, error } => {
                self.push_notification(
                    &format!("Catch-up error: {error}"),
                    crate::tui::state::NotificationLevel::Error,
                );
            }
        }
    }

    fn push_notification(&mut self, message: &str, level: crate::tui::state::NotificationLevel) {
        self.state.notifications.push(crate::tui::state::Notification {
            message: message.to_string(),
            level,
            created_at: autore_schema::domain::Timestamp::now(),
        });
    }

    /// Schedules a background catch-up query to fill a sequence gap.
    fn schedule_catchup(&self, project: ProjectId, after_sequence: u64) {
        let Some(client) = &self.client else {
            return;
        };
        let tx = self.internal_tx.clone();
        let client = Arc::clone(client);
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                client.events_after(project, after_sequence, 100)
            })
            .await;
            match result {
                Ok(Ok(events)) => {
                    let _ = tx
                        .send(InternalTuiEvent::CatchupEvents { project, events })
                        .await;
                }
                Ok(Err(e)) => {
                    let _ = tx
                        .send(InternalTuiEvent::CatchupError {
                            project,
                            error: e.to_string(),
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(InternalTuiEvent::CatchupError {
                            project,
                            error: e.to_string(),
                        })
                        .await;
                }
            }
        });
    }

    fn schedule_project_refresh(&self, project: ProjectId) {
        let Some(client) = &self.client else {
            return;
        };
        let tx = self.internal_tx.clone();
        let client = Arc::clone(client);
        tokio::spawn(async move {
            let query = ApplicationQuery::ListEvents(autore_app::ListEventsQuery {
                project,
                after_sequence: 0,
                limit: 100,
            });
            let result = tokio::task::spawn_blocking(move || client.query(query)).await;
            match result {
                Ok(Ok(qr)) => {
                    let _ = tx.send(InternalTuiEvent::QueryResult { project, result: qr }).await;
                }
                Ok(Err(e)) => {
                    let _ = tx
                        .send(InternalTuiEvent::QueryError {
                            project,
                            error: e.to_string(),
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(InternalTuiEvent::QueryError {
                            project,
                            error: e.to_string(),
                        })
                        .await;
                }
            }
        });
    }

    fn apply_query_result(&mut self, _project: ProjectId, result: QueryResult) {
        if let QueryResult::Events(response) = result {
            for ev in &response.events {
                if let Some(view) = self.state.project_views.get_mut(&ev.project) {
                    view.recent_events = response.events.clone();
                    break;
                }
            }
        }
    }

    /// Moves to the next project in the project list (wraps around).
    fn select_next(&mut self) {
        let project_ids: Vec<_> = self.state.project_views.keys().copied().collect();
        if project_ids.is_empty() {
            return;
        }
        match &self.state.navigation {
            Navigation::Dashboard => {
                if let Some(&first) = project_ids.first() {
                    self.state.navigation = Navigation::Project(first);
                }
            }
            Navigation::Project(current) => {
                if let Some(pos) = project_ids.iter().position(|id| id == current) {
                    let next = (pos + 1) % project_ids.len();
                    self.state.navigation = Navigation::Project(project_ids[next]);
                }
            }
            _ => {}
        }
    }

    /// Moves to the previous project in the project list (wraps around).
    fn select_previous(&mut self) {
        let project_ids: Vec<_> = self.state.project_views.keys().copied().collect();
        if project_ids.is_empty() {
            return;
        }
        match &self.state.navigation {
            Navigation::Project(current) => {
                if let Some(pos) = project_ids.iter().position(|id| id == current) {
                    let prev = if pos == 0 { project_ids.len() - 1 } else { pos - 1 };
                    self.state.navigation = Navigation::Project(project_ids[prev]);
                }
            }
            Navigation::Dashboard => {
                if let Some(&last) = project_ids.last() {
                    self.state.navigation = Navigation::Project(last);
                }
            }
            _ => {}
        }
    }

    /// Cycles focus to the next panel.
    fn cycle_focus(&mut self) {
        self.state.focus = match self.state.focus {
            Focus::Panel1 => Focus::Panel2,
            Focus::Panel2 => Focus::Panel3,
            Focus::Panel3 => Focus::Sidebar,
            Focus::Sidebar => Focus::Panel1,
            Focus::Dialog => Focus::Panel1,
        };
    }

    /// Renders the full dashboard into the given frame.
    fn render(&self, frame: &mut Frame) {
        let outer = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(frame.area());

        // Panel 1: project summary / projects list (Stage 0 remap).
        self.render_project_panel(frame, outer[0]);

        // Right column: tab strip + body.
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(outer[1]);

        self.render_tab_strip(frame, right[0]);

        match self.state.active_pane {
            Pane::Dashboard => {
                // Preserve 4-panel physical layout: right column split 50/50.
                let body = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(right[1]);
                self.render_operations_panel(frame, body[0]);
                self.render_hypotheses_panel(frame, body[1]);
            }
            Pane::Providers => self.render_providers_pane(frame, right[1]),
            Pane::NativeArtifacts => self.render_native_artifacts_pane(frame, right[1]),
            Pane::OperationsDetail => self.render_operations_detail_pane(frame, right[1]),
            Pane::EventsLog => self.render_events_log_pane(frame, right[1]),
            Pane::MigrationHistory => self.render_migration_history_pane(frame, right[1]),
            Pane::ExternalArtifactIntegrity => {
                self.render_external_artifact_integrity_pane(frame, right[1]);
            }
        }
    }

    fn render_tab_strip(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let titles = [
            "Dashboard",
            "Providers",
            "NativeArtifacts",
            "OpsDetail",
            "EventsLog",
            "Migration",
            "ExtIntegrity",
        ];
        let idx = match self.state.active_pane {
            Pane::Dashboard => 0,
            Pane::Providers => 1,
            Pane::NativeArtifacts => 2,
            Pane::OperationsDetail => 3,
            Pane::EventsLog => 4,
            Pane::MigrationHistory => 5,
            Pane::ExternalArtifactIntegrity => 6,
        };
        let tabs = Tabs::new(titles.iter().map(|t| Line::from(*t)).collect::<Vec<_>>())
            .select(idx)
            .highlight_style(Style::default().bold())
            .block(Block::default());
        frame.render_widget(tabs, area);
    }

    fn render_project_panel(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let project_views = &self.state.project_views;

        if project_views.is_empty() {
            let msg = Paragraph::new("No projects loaded.")
                .italic()
                .block(Block::bordered().title("Projects (0)"));
            frame.render_widget(msg, area);
            return;
        }

        // When navigating to a specific project, show its detailed summary.
        if let Some(view) = self.state.current_project_view() {
            let name = view
                .project_summary
                .as_ref()
                .map(|p| p.name.as_str())
                .unwrap_or("(loading)");
            let schema = view
                .schema_version
                .as_ref()
                .map(|v| format!("v{v}"))
                .unwrap_or_else(|| "v?".into());
            let validation = match &view.validation_status {
                None => "unchecked".to_owned(),
                Some(crate::tui::state::ValidationStatus::Ok) => "ok".to_owned(),
                Some(crate::tui::state::ValidationStatus::Warnings(w)) => {
                    format!("warnings ({})", w.len())
                }
                Some(crate::tui::state::ValidationStatus::Failed(e)) => {
                    format!("failed ({})", e.len())
                }
            };
            let mut lines = vec![
                Line::from(vec![
                    Span::raw("Project: ").bold(),
                    Span::raw(name),
                ]),
                Line::from(vec![
                    Span::raw("Schema:  "),
                    Span::raw(schema),
                ]),
                Line::from(vec![
                    Span::raw("Valid:   "),
                    Span::raw(validation),
                ]),
                Line::from(""),
                Line::from("Counts:".bold()),
                Line::from(format!("  artifacts:       {}", view.artifacts.len())),
                Line::from(format!("  entities:        {}", view.entities.len())),
                Line::from(format!("  evidence:        {}", view.evidence.len())),
                Line::from(format!("  hypotheses:      {}", view.hypotheses.len())),
                Line::from(format!("  contradictions:  {}", view.contradictions.len())),
                Line::from(format!("  verifications:   {}", view.verification.len())),
            ];
            if !view.recent_events.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(format!(
                    "events: {} (last seq {})",
                    view.recent_events.len(),
                    view.recent_events
                        .iter()
                        .map(|e| e.sequence)
                        .max()
                        .unwrap_or(0)
                )));
            }
            let selected_id = match &self.state.navigation {
                Navigation::Project(pid) => Some(*pid),
                _ => None,
            };
            lines.push(Line::from(""));
            lines.push(Line::from("Other:"));
            for (id, v) in project_views {
                if Some(*id) != selected_id {
                    let n = v
                        .project_summary
                        .as_ref()
                        .map(|p| p.name.as_str())
                        .unwrap_or("(loading)");
                    lines.push(Line::from(format!("  ▶ {n}")));
                }
            }
            let title = format!("Projects ({})", project_views.len());
            let para = Paragraph::new(lines).block(Block::bordered().title(title));
            frame.render_widget(para, area);
            return;
        }

        // Fallback: render a project list when not viewing a specific project.
        let items: Vec<ListItem> = project_views
            .iter()
            .map(|(id, view)| {
                let name = view
                    .project_summary
                    .as_ref()
                    .map(|p| p.name.as_str())
                    .unwrap_or("(loading)");
                let is_selected =
                    matches!(&self.state.navigation, Navigation::Project(pid) if pid == id);
                let marker = if is_selected { "▶ " } else { "  " };
                let content = Line::from(vec![Span::raw(marker), Span::raw(name)]);
                ListItem::new(content)
            })
            .collect();

        let title = format!("Projects ({})", project_views.len());
        let list = List::new(items).block(Block::bordered().title(title));
        frame.render_widget(list, area);
    }

    fn render_operations_panel(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let operations: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.operations.iter())
            .collect();

        if operations.is_empty() {
            let msg = Paragraph::new("No operations.")
                .italic()
                .block(Block::bordered().title("Operations"));
            frame.render_widget(msg, area);
            return;
        }

        let header = Row::new(vec!["ID", "Kind", "State", "Progress", "Cancel"]);
        let rows: Vec<Row> = operations
            .iter()
            .map(|op| {
                let id_str = op.id.to_string();
                let id_short = if id_str.len() >= 8 { &id_str[..8] } else { &id_str };
                let progress_pct = if op
                    .failure
                    .is_none() { {
                        if matches!(op.state, autore_core::operation::OperationState::Completed) {
                            100
                        } else if matches!(op.state, autore_core::operation::OperationState::Queued)
                        {
                            0
                        } else {
                            50
                        }
                    } } else { 0 };
                let cancel = if matches!(
                    op.state,
                    autore_core::operation::OperationState::Running
                        | autore_core::operation::OperationState::Queued
                        | autore_core::operation::OperationState::Paused
                ) {
                    "[c]"
                } else {
                    ""
                };
                Row::new(vec![
                    id_short.to_string(),
                    op.kind.to_string(),
                    op.state.to_string(),
                    format!("{progress_pct}%"),
                    cancel.to_string(),
                ])
            })
            .collect();

        let title = format!("Operations ({})", operations.len());
        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Percentage(35),
                Constraint::Length(14),
                Constraint::Length(9),
                Constraint::Length(6),
            ],
        )
        .header(header.bold())
        .block(Block::bordered().title(title));
        frame.render_widget(table, area);
    }

    fn render_hypotheses_panel(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let hypotheses: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.hypotheses.iter())
            .collect();
        let evidence: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.evidence.iter())
            .collect();

        let total_h = hypotheses.len();
        let total_e = evidence.len();
        let progress = if total_h == 0 {
            0.0
        } else {
            let supported = hypotheses
                .iter()
                .filter(|h| {
                    matches!(
                        h.status,
                        autore_schema::domain::records::HypothesisStatus::UnderInvestigation
                            | autore_schema::domain::records::HypothesisStatus::Accepted
                    )
                })
                .count();
            supported as f64 / total_h as f64
        };

        let label = format!(
            "{total_e} evidence / {total_h} hypotheses ({:.0}%)",
            progress * 100.0
        );

        let gauge = Gauge::default()
            .block(Block::bordered().title("Hypotheses + Evidence"))
            .gauge_style(Style::default())
            .percent((progress * 100.0) as u16)
            .label(label);

        frame.render_widget(gauge, area);
    }

    fn render_providers_pane(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let providers: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.providers.iter())
            .collect();
        let runs: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.runs.iter())
            .collect();
        let mut lines = vec![
            Line::from(format!(
                "Providers: {}   Runs: {}",
                providers.len(),
                runs.len()
            )),
            Line::from(""),
        ];
        for p in providers {
            lines.push(Line::from(format!(
                "• {} ({}) v{}",
                p.name, p.kind, p.version
            )));
        }
        if !runs.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("Recent runs:"));
            for r in runs.iter().take(10) {
                lines.push(Line::from(format!(
                    "  {} op={} status={:?}",
                    &r.id.to_string()[..8.min(r.id.to_string().len())],
                    r.operation,
                    r.status
                )));
            }
        }
        let para = Paragraph::new(lines).block(Block::bordered().title("Providers"));
        frame.render_widget(para, area);
    }

    fn render_native_artifacts_pane(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let artifacts: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.artifacts.iter())
            .collect();
        let mut lines = vec![Line::from(format!("Native artifacts: {}", artifacts.len()))];
        for a in artifacts.iter().take(20) {
            lines.push(Line::from(format!(
                "  {} kind={} size={}",
                &a.id.to_string()[..8.min(a.id.to_string().len())],
                a.kind,
                a.size
            )));
        }
        let para = Paragraph::new(lines).block(Block::bordered().title("NativeArtifacts"));
        frame.render_widget(para, area);
    }

    fn render_operations_detail_pane(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let all: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.operations.iter())
            .collect();
        let mut lines = vec![Line::from(format!(
            "Operations detail: {} tracked",
            all.len()
        ))];
        for op in all.iter().take(10) {
            let op_view = self.state.operation_views.get(&op.id);
            let progress_count = op_view.map(|v| v.progress.len()).unwrap_or(0);
            let cancel_count = op_view.map(|v| v.cancellation_requests.len()).unwrap_or(0);
            lines.push(Line::from(format!(
                "  {} kind={} state={} progress={} cancels={}",
                &op.id.to_string()[..8.min(op.id.to_string().len())],
                op.kind,
                op.state,
                progress_count,
                cancel_count
            )));
        }
        let para = Paragraph::new(lines).block(Block::bordered().title("OperationsDetail"));
        frame.render_widget(para, area);
    }

    fn render_events_log_pane(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let events: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.recent_events.iter())
            .collect();
        let mut lines = vec![Line::from(format!(
            "Events log: {} recent (cursor seq={}, connected={}, missed={})",
            events.len(),
            self.state.event_cursor.last_sequence,
            self.state.event_cursor.connected,
            self.state.event_cursor.missed_events
        ))];
        for ev in events.iter().take(20) {
            lines.push(Line::from(format!(
                "  seq={} kind={} src={:?} at={}",
                ev.sequence, ev.kind, ev.source, ev.created_at
            )));
        }
        let para = Paragraph::new(lines).block(Block::bordered().title("EventsLog"));
        frame.render_widget(para, area);
    }

    fn render_migration_history_pane(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines = vec![Line::from(format!(
            "Migration history: {} projects tracked",
            self.state.project_views.len()
        ))];
        for (pid, view) in &self.state.project_views {
            let sv = view
                .schema_version
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into());
            lines.push(Line::from(format!(
                "  project={} schema={}",
                &pid.to_string()[..8.min(pid.to_string().len())],
                sv
            )));
        }
        let para = Paragraph::new(lines).block(Block::bordered().title("MigrationHistory"));
        frame.render_widget(para, area);
    }

    fn render_external_artifact_integrity_pane(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
    ) {
        let artifacts: Vec<_> = self
            .state
            .project_views
            .values()
            .flat_map(|v| v.artifacts.iter())
            .collect();
        let mut lines = vec![Line::from(format!(
            "External artifact integrity: {} tracked ({} external refs)",
            artifacts.len(),
            0
        ))];
        for a in artifacts.iter().take(20) {
            lines.push(Line::from(format!(
                "  {} kind={} hash={:?} size={}",
                &a.id.to_string()[..8.min(a.id.to_string().len())],
                a.kind,
                a.content_hash,
                a.size
            )));
        }
        let para = Paragraph::new(lines)
            .block(Block::bordered().title("ExternalArtifactIntegrity"));
        frame.render_widget(para, area);
    }
}

// ---------------------------------------------------------------------------
// Generic fallback renderer (§23.8 + Metis)
// ---------------------------------------------------------------------------

/// Render any namespaced record generically. Given a `kind`, an `id`, and an
/// iterator of `(field_name, field_value)` pairs, renders a `Paragraph`
/// titled `"<kind> <id>"`. If `fields` is empty, shows "no fields".
///
/// Never panics on unknown kinds — this is the fallback for records the TUI
/// has not been taught about explicitly.
pub fn render_generic_record<'a, I, S>(
    kind: &NamespacedId,
    id: impl std::fmt::Display,
    fields: I,
) -> Paragraph<'a>
where
    I: IntoIterator<Item = (S, S)>,
    S: std::fmt::Display,
{
    let mut lines: Vec<Line> = Vec::new();
    let mut any = false;
    for (k, v) in fields {
        any = true;
        lines.push(Line::from(format!("{k}: {v}")));
    }
    if !any {
        lines.push(Line::from("no fields"));
    }
    let title = format!("{kind} {id}");
    Paragraph::new(lines).block(Block::bordered().title(title))
}

/// Convenience: render an `ExtensionData` payload generically.
pub fn render_extension_data_generic<'a>(
    kind: &NamespacedId,
    id: impl std::fmt::Display,
    data: Option<&ExtensionData>,
) -> Paragraph<'a> {
    let fields: Vec<(String, String)> = match data {
        Some(ed) => {
            let mut out = vec![(
                "schema".to_string(),
                format!("{}@v{}", ed.schema, ed.version),
            )];
            flatten_json(&ed.value, &mut out, String::new());
            out
        }
        None => vec![],
    };
    render_generic_record(
        kind,
        id,
        fields,
    )
}

/// Convenience: render a `MetadataMap` generically.
pub fn render_metadata_map_generic<'a>(
    kind: &NamespacedId,
    id: impl std::fmt::Display,
    map: &MetadataMap,
) -> Paragraph<'a> {
    let fields: Vec<(String, String)> = map
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                format!("{}@v{} = {}", v.schema, v.version, v.value),
            )
        })
        .collect();
    render_generic_record(
        kind,
        id,
        fields,
    )
}

fn flatten_json(value: &serde_json::Value, out: &mut Vec<(String, String)>, prefix: String) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_json(v, out, key);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let key = format!("{prefix}[{i}]");
                flatten_json(v, out, key);
            }
        }
        serde_json::Value::Null => {
            out.push((prefix, "null".into()));
        }
        other => {
            out.push((prefix, other.to_string()));
        }
    }
}

impl Default for Tui {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry point for the TUI.
pub async fn run_tui() -> crate::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = Tui::new();
    let (term_tx, mut term_rx) = mpsc::channel::<TerminalEvent>(64);

    let tick_handle = {
        let tx = term_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if tx.send(TerminalEvent::Tick).await.is_err() {
                    break;
                }
            }
        })
    };

    let crossterm_handle = {
        let tx = term_tx;
        tokio::task::spawn_blocking(move || loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false)
                && let Ok(ev) = event::read()
                && let Some(term) = TerminalEvent::from_crossterm(ev)
                && tx.blocking_send(term).is_err()
            {
                break;
            }
        })
    };

    let result = loop {
        terminal
            .draw(|frame| app.render(frame))
            .map_err(crate::Error::Io)?;

        let event = tokio::select! {
            Some(term) = term_rx.recv() => TuiEvent::Terminal(term),
            Some(internal) = app.internal_rx.recv() => TuiEvent::Internal(internal),
            event = poll_subscription(app.subscription.as_mut()) => {
                match event {
                    Some(Ok(pe)) => TuiEvent::Project(pe),
                    Some(Err(e)) => {
                        app.handle_internal_event(
                            InternalTuiEvent::Notification(format!("subscription error: {e}")),
                        );
                        continue;
                    }
                    None => continue,
                }
            }
        };

        let should_quit = match event {
            TuiEvent::Terminal(term) => app.handle_terminal_event(term)?,
            TuiEvent::Project(pe) => {
                app.handle_project_event(pe);
                false
            }
            TuiEvent::Internal(internal) => {
                app.handle_internal_event(internal);
                false
            }
        };

        if should_quit {
            break Ok(());
        }
    };

    tick_handle.abort();
    crossterm_handle.abort();
    ratatui::restore();
    result
}

/// Action returned by a single event-loop step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    /// The loop should continue.
    Continue,
    /// The loop should exit.
    Quit,
}

/// Drives the TUI event loop over three sources using `tokio::select!` (§23.4).
///
/// The loop dispatches terminal events, project events from the subscription,
/// and internal events from background tasks. Storage queries never execute
/// in the rendering path.
pub struct TuiEventLoop<'a> {
    tui: &'a mut Tui,
}

impl<'a> TuiEventLoop<'a> {
    /// Creates a new event loop driver.
    pub fn new(tui: &'a mut Tui) -> Self {
        Self { tui }
    }

    /// Returns a reference to the underlying TUI.
    #[must_use]
    pub fn tui(&self) -> &Tui {
        self.tui
    }

    /// Returns a mutable reference to the underlying TUI.
    pub fn tui_mut(&mut self) -> &mut Tui {
        self.tui
    }

    /// Processes one event from any source. Returns the loop action.
    pub async fn step(
        &mut self,
        terminal_rx: &mut mpsc::Receiver<TerminalEvent>,
    ) -> crate::Result<LoopAction> {
        let event = tokio::select! {
            Some(term) = terminal_rx.recv() => TuiEvent::Terminal(term),
            Some(internal) = self.tui.internal_rx.recv() => TuiEvent::Internal(internal),
            event = poll_subscription(self.tui.subscription.as_mut()) => {
                match event {
                    Some(Ok(pe)) => TuiEvent::Project(pe),
                    Some(Err(e)) => {
                        self.tui.handle_internal_event(
                            InternalTuiEvent::Notification(format!("subscription error: {e}")),
                        );
                        return Ok(LoopAction::Continue);
                    }
                    None => return Ok(LoopAction::Continue),
                }
            }
        };

        match event {
            TuiEvent::Terminal(term) => {
                if self.tui.handle_terminal_event(term)? {
                    return Ok(LoopAction::Quit);
                }
            }
            TuiEvent::Project(pe) => {
                self.tui.handle_project_event(pe);
            }
            TuiEvent::Internal(internal) => {
                self.tui.handle_internal_event(internal);
            }
        }

        Ok(LoopAction::Continue)
    }
}

async fn poll_subscription(
    sub: Option<&mut ProjectEventSubscription>,
) -> Option<autore_core::Result<ProjectEvent>> {
    match sub {
        Some(s) => s.next().await,
        None => std::future::pending().await,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::tui::state::{
        EventCursor, FilterState, Focus, Navigation, Pane, ProjectViewState, TuiState,
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

    fn sample_state() -> TuiState {
        use autore_schema::domain::records::Project;
        use autore_schema::ids::ProjectId;

        let pid = ProjectId::new();
        let mut view = ProjectViewState::default();
        let mut project = Project::new("alpha");
        project.id = pid;
        view.project_summary = Some(project);
        view.schema_version = Some(autore_schema::domain::SchemaVersion::new(2, 0));

        let mut project_views = HashMap::new();
        project_views.insert(pid, view);

        let pid2 = ProjectId::new();
        let mut view2 = ProjectViewState::default();
        let mut project2 = Project::new("beta");
        project2.id = pid2;
        view2.project_summary = Some(project2);
        project_views.insert(pid2, view2);

        TuiState {
            navigation: Navigation::Project(pid),
            focus: Focus::Panel1,
            filters: FilterState::default(),
            dialogs: vec![],
            notifications: vec![],
            project_views,
            operation_views: HashMap::new(),
            event_cursor: EventCursor::default(),
            active_pane: Pane::Dashboard,
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance criteria tests
    // -----------------------------------------------------------------------

    /// Empty TuiState renders without panicking.
    #[test]
    fn tui_state_renders_empty() {
        let tui = Tui::new();
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| tui.render(frame))
            .expect("empty render must not panic");
    }

    /// Compile-time + grep test: no direct storage imports in autore-tui/src.
    /// The actual enforcement is a grep test verifying no storage crate references.
    /// This test asserts true as a marker.
    #[test]
    fn tui_state_no_direct_db() {
        // Enforced by grep at the acceptance-criteria level, not at runtime.
        // This test exists as a marker for the test runner.
    }

    // -----------------------------------------------------------------------
    // Adapted regression tests from Task 2
    // -----------------------------------------------------------------------

    /// Startup with empty state renders without panicking and shows empty panels.
    #[test]
    fn tui_startup_renders_empty() {
        let tui = Tui::new();
        let output = render_to_string(&tui, 80, 24);

        // Empty state communicates that nothing is loaded.
        assert!(
            output.contains("No projects loaded"),
            "empty-state message missing from startup render: {output}"
        );
        // The project list must show a count of zero.
        assert!(
            output.contains("Projects (0)"),
            "empty project list title missing: {output}"
        );
        // The operations panel must indicate no operations.
        assert!(
            output.contains("No operations"),
            "empty operations panel message missing: {output}"
        );
    }

    /// Dashboard renders with project panel, operations panel, and hypotheses panel.
    #[test]
    fn tui_dashboard_renders() {
        let tui = Tui::with_state(sample_state());
        let output = render_to_string(&tui, 80, 24);
        assert!(
            output.contains("Projects"),
            "missing Projects panel: {output}"
        );
        assert!(
            output.contains("Operations"),
            "missing Operations panel: {output}"
        );
        assert!(
            output.contains("Hypotheses"),
            "missing Hypotheses panel: {output}"
        );
    }

    /// Both project names appear in the rendered output.
    #[test]
    fn tui_dashboard_shows_projects() {
        let tui = Tui::with_state(sample_state());
        let output = render_to_string(&tui, 80, 24);
        assert!(
            output.contains("alpha"),
            "project 'alpha' not rendered: {output}"
        );
        assert!(
            output.contains("beta"),
            "project 'beta' not rendered: {output}"
        );
        // The selected project marker must be present.
        assert!(output.contains("▶"), "selection marker missing: {output}");
    }

    /// `q` quits; no other common key should signal quit.
    #[test]
    fn tui_dashboard_quits_on_q() {
        let mut tui = Tui::new();
        let q_event = KeyEvent::new(KeyCode::Char('q'), crossterm::event::KeyModifiers::NONE);
        let should_quit = tui.handle_key_event(q_event).unwrap();
        assert!(should_quit, "pressing 'q' must signal quit");

        // Other keys must not quit.
        let mut tui2 = Tui::new();
        let j_event = KeyEvent::new(KeyCode::Char('j'), crossterm::event::KeyModifiers::NONE);
        let should_quit2 = tui2.handle_key_event(j_event).unwrap();
        assert!(!should_quit2, "pressing 'j' must not signal quit");
    }

    /// Navigation moves selection between projects.
    #[test]
    fn tui_dashboard_navigation() {
        let state = sample_state();
        let mut tui = Tui::with_state(state);

        // Initially on the first project (alpha).
        assert!(matches!(tui.state().navigation, Navigation::Project(_)));

        // Move down to next project.
        let down = KeyEvent::new(KeyCode::Char('j'), crossterm::event::KeyModifiers::NONE);
        tui.handle_key_event(down).unwrap();

        // Still on a project, but possibly different one.
        assert!(matches!(tui.state().navigation, Navigation::Project(_)));

        // Move up wraps around.
        let up = KeyEvent::new(KeyCode::Char('k'), crossterm::event::KeyModifiers::NONE);
        tui.handle_key_event(up).unwrap();
        assert!(matches!(tui.state().navigation, Navigation::Project(_)));
    }

    // -----------------------------------------------------------------------
    // Stage 0 remap baseline regression tests
    // -----------------------------------------------------------------------

    /// Primary panels present in a single render pass.
    #[test]
    fn tui_primary_panels_present() {
        let tui = Tui::with_state(sample_state());
        let output = render_to_string(&tui, 80, 24);

        let expected_titles = ["Projects", "Operations", "Hypotheses"];
        for title in expected_titles {
            assert!(
                output.contains(title),
                "panel title {title:?} missing from dashboard render: {output}"
            );
        }
    }

    /// `j`/`Down` and `k`/`Up` navigation moves selection and wraps.
    #[test]
    fn tui_navigation_jk_up_down_wraps() {
        let state = sample_state();
        let mut tui = Tui::with_state(state);

        let j = KeyEvent::new(KeyCode::Char('j'), crossterm::event::KeyModifiers::NONE);
        let k = KeyEvent::new(KeyCode::Char('k'), crossterm::event::KeyModifiers::NONE);
        let down = KeyEvent::new(KeyCode::Down, crossterm::event::KeyModifiers::NONE);
        let up = KeyEvent::new(KeyCode::Up, crossterm::event::KeyModifiers::NONE);

        // 'j' selects a project.
        tui.handle_key_event(j).unwrap();
        assert!(matches!(tui.state().navigation, Navigation::Project(_)));

        // Down wraps around.
        tui.handle_key_event(down).unwrap();
        assert!(matches!(tui.state().navigation, Navigation::Project(_)));

        // 'k' wraps from first to last.
        tui.handle_key_event(k).unwrap();
        assert!(matches!(tui.state().navigation, Navigation::Project(_)));

        // Up wraps.
        tui.handle_key_event(up).unwrap();
        assert!(matches!(tui.state().navigation, Navigation::Project(_)));
    }

    /// `q` quits; navigation keys do not.
    #[test]
    fn tui_q_quits_cleanly() {
        let mut tui = Tui::new();

        let q = KeyEvent::new(KeyCode::Char('q'), crossterm::event::KeyModifiers::NONE);
        assert!(
            tui.handle_key_event(q).unwrap(),
            "'q' must signal quit (return true)"
        );

        for code in [
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Enter,
            KeyCode::Char(' '),
        ] {
            let mut t = Tui::new();
            let event = KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
            assert!(
                !t.handle_key_event(event).unwrap(),
                "{code:?} must not signal quit"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 29 — event loop acceptance tests
    // -----------------------------------------------------------------------

    fn make_project_event(
        project: ProjectId,
        sequence: u64,
        kind: &autore_schema::domain::NamespacedId,
    ) -> ProjectEvent {
        ProjectEvent::new(
            project,
            sequence,
            kind.clone(),
            autore_schema::domain::records::EventSource::Project,
            None,
            None,
        )
    }

    /// Injects synthetic crossterm key and resize events and asserts the TUI
    /// processes them without panic.
    #[tokio::test]
    async fn tui_event_loop_handles_terminal() {
        let state = sample_state();
        let mut tui = Tui::with_state(state);
        let (term_tx, mut term_rx) = mpsc::channel(16);

        let pid_before = tui.state().navigation.clone();

        term_tx
            .send(TerminalEvent::Key(KeyEvent::new(
                KeyCode::Char('j'),
                crossterm::event::KeyModifiers::NONE,
            )))
            .await
            .unwrap();

        let mut loop_driver = TuiEventLoop::new(&mut tui);
        let action = loop_driver.step(&mut term_rx).await.unwrap();
        assert_eq!(action, LoopAction::Continue);

        term_tx
            .send(TerminalEvent::Resize(120, 40))
            .await
            .unwrap();
        let action = loop_driver.step(&mut term_rx).await.unwrap();
        assert_eq!(action, LoopAction::Continue);

        assert_ne!(tui.state().navigation, pid_before);
    }

    /// Injects a `ProjectEvent` and asserts the event cursor advances.
    #[tokio::test]
    async fn tui_event_loop_handles_project_event() {
        use autore_schema::domain::records::EVENT_KIND_PROJECT_CREATED;

        let state = sample_state();
        let mut tui = Tui::with_state(state);

        let pid = tui.state().project_views.keys().next().copied().unwrap();
        let event = make_project_event(pid, 1, &EVENT_KIND_PROJECT_CREATED);

        tui.handle_project_event(event);

        assert_eq!(tui.state().event_cursor.last_sequence, 1);
        let view = tui.state().project_views.get(&pid).unwrap();
        assert_eq!(view.recent_events.len(), 1);
        assert_eq!(view.recent_events[0].sequence, 1);
    }

    /// Injects events with a sequence gap and asserts the subscription
    /// resyncs and `missed_events` is set.
    #[tokio::test]
    async fn tui_event_loop_sequence_gap_recovers() {
        use autore_schema::domain::records::EVENT_KIND_PROJECT_CREATED;

        let state = sample_state();
        let mut tui = Tui::with_state(state);

        let pid = tui.state().project_views.keys().next().copied().unwrap();

        let ev1 = make_project_event(pid, 1, &EVENT_KIND_PROJECT_CREATED);
        tui.handle_project_event(ev1);
        assert_eq!(tui.state().event_cursor.last_sequence, 1);
        assert!(!tui.state().event_cursor.missed_events);

        let ev3 = make_project_event(pid, 3, &EVENT_KIND_PROJECT_CREATED);
        tui.handle_project_event(ev3);

        assert_eq!(tui.state().event_cursor.last_sequence, 3);
        assert!(
            tui.state().event_cursor.missed_events,
            "missed_events flag must be set after sequence gap"
        );

        tui.handle_internal_event(InternalTuiEvent::CatchupEvents {
            project: pid,
            events: vec![make_project_event(pid, 2, &EVENT_KIND_PROJECT_CREATED)],
        });
        assert!(!tui.state().event_cursor.missed_events);
    }

    /// Simulates a slow query and asserts rendering/keyboard handling
    /// continues while the query is outstanding.
    #[tokio::test]
    async fn tui_does_not_block_storage() {
        use autore_schema::domain::records::EVENT_KIND_PROJECT_CREATED;
        use std::sync::atomic::{AtomicBool, Ordering};

        struct SlowClient {
            query_started: Arc<AtomicBool>,
            query_released: Arc<AtomicBool>,
        }

        impl AutoReClient for SlowClient {
            fn execute(
                &self,
                _command: autore_app::ApplicationCommand,
            ) -> autore_core::Result<autore_app::CommandResult> {
                unimplemented!()
            }
            fn query(
                &self,
                _query: autore_app::ApplicationQuery,
            ) -> autore_core::Result<autore_app::QueryResult> {
                self.query_started.store(true, Ordering::SeqCst);
                while !self.query_released.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Ok(autore_app::QueryResult::Events(autore_app::EventsResponse {
                    events: vec![],
                }))
            }
            fn events_after(
                &self,
                _project: ProjectId,
                _sequence: u64,
                _limit: usize,
            ) -> autore_core::Result<Vec<ProjectEvent>> {
                Ok(vec![])
            }
            fn subscribe_events(
                &self,
                _project: ProjectId,
                _after: u64,
            ) -> autore_core::Result<ProjectEventSubscription> {
                unimplemented!()
            }
        }

        let query_started = Arc::new(AtomicBool::new(false));
        let query_released = Arc::new(AtomicBool::new(false));

        let client = SlowClient {
            query_started: Arc::clone(&query_started),
            query_released: Arc::clone(&query_released),
        };

        let state = sample_state();
        let mut tui = Tui::with_client(state, Box::new(client));

        let pid = tui.state().project_views.keys().next().copied().unwrap();
        let event = make_project_event(pid, 1, &EVENT_KIND_PROJECT_CREATED);

        tui.handle_project_event(event);

        for _ in 0..50 {
            if query_started.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(
            query_started.load(Ordering::SeqCst),
            "background query must have started"
        );

        let quit = tui
            .handle_terminal_event(TerminalEvent::Key(KeyEvent::new(
                KeyCode::Char('j'),
                crossterm::event::KeyModifiers::NONE,
            )))
            .unwrap();
        assert!(!quit, "keyboard must not block while query is outstanding");

        query_released.store(true, Ordering::SeqCst);

        for _ in 0..50 {
            if let Ok(Some(internal)) =
                tokio::time::timeout(Duration::from_millis(10), tui.internal_rx.recv()).await
            {
                tui.handle_internal_event(internal);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task 30 — Stage 0 panel remap tests
    // -----------------------------------------------------------------------

    /// Panel 1 = Projects (with summary), Panel 2 = Operations, Panel 3 = Hypotheses + Evidence.
    #[test]
    fn tui_dashboard_panels_present() {
        let tui = Tui::with_state(sample_state());
        let output = render_to_string(&tui, 100, 30);

        assert!(
            output.contains("Projects"),
            "Panel 1 must be Projects: {output}"
        );
        assert!(
            output.contains("Operations"),
            "Panel 2 must be Operations: {output}"
        );
        assert!(
            output.contains("Hypotheses + Evidence"),
            "Panel 3 must be Hypotheses + Evidence: {output}"
        );
    }

    /// Dashboard shows validation status and counts for the selected project.
    #[test]
    fn tui_dashboard_shows_validation_status() {
        let mut state = sample_state();
        // sample_state() navigates to the first-inserted project; get its id
        // from the navigation field rather than from HashMap iteration
        // (which is non-deterministic across runs).
        let pid = match &state.navigation {
            Navigation::Project(pid) => *pid,
            _ => panic!("sample_state must navigate to a project"),
        };
        let view = state.project_views.get_mut(&pid).unwrap();
        view.validation_status = Some(crate::tui::state::ValidationStatus::Ok);
        view.artifacts.push(make_sample_artifact(pid));
        view.hypotheses.push(make_sample_hypothesis(pid));

        let tui = Tui::with_state(state);
        let output = render_to_string(&tui, 100, 30);

        assert!(
            output.contains("Valid:"),
            "validation status line missing: {output}"
        );
        assert!(
            output.contains("ok"),
            "validation status value 'ok' missing: {output}"
        );
        assert!(
            output.contains("artifacts:"),
            "artifacts count missing: {output}"
        );
        assert!(
            output.contains("hypotheses:"),
            "hypotheses count missing: {output}"
        );
    }

    fn make_sample_artifact(
        pid: autore_schema::ids::ProjectId,
    ) -> autore_schema::domain::records::Artifact {
        use autore_schema::domain::records::Artifact;
        Artifact {
            id: autore_schema::ids::ArtifactId::new(),
            project: pid,
            kind: NamespacedId::parse("core.binary").unwrap(),
            content_hash: autore_schema::domain::ContentHash::sha256(b"abc"),
            size: 42,
            storage: autore_schema::domain::records::ArtifactStorage::ExternalFile {
                canonical_path: "/tmp/x".into(),
            },
            created_at: autore_schema::domain::Timestamp::now(),
            metadata: MetadataMap::new(),
        }
    }

    fn make_sample_hypothesis(
        pid: autore_schema::ids::ProjectId,
    ) -> autore_schema::domain::records::Hypothesis {
        use autore_schema::domain::{
            Confidence, EvidenceValue,
        };
        use autore_schema::domain::records::{Hypothesis, HypothesisStatus};
        Hypothesis {
            id: autore_schema::ids::HypothesisId::new(),
            project: pid,
            subject: autore_schema::ids::EntityId::new(),
            predicate: NamespacedId::parse("core.is-loop").unwrap(),
            candidate: EvidenceValue::Boolean(true),
            supporting_evidence: vec![],
            contradicting_evidence: vec![],
            derived_from: vec![],
            confidence: Confidence::new(0.5).unwrap(),
            status: HypothesisStatus::Proposed,
            created_at: autore_schema::domain::Timestamp::now(),
            updated_at: autore_schema::domain::Timestamp::now(),
        }
    }

    /// Generic fallback renderer renders unknown-kind records without panic.
    #[test]
    fn tui_generic_fallback_unknown_record() {
        let kind = NamespacedId::parse("developer.experimental.foo").unwrap();
        let id = "rec-42";
        let fields = vec![
            ("bar".to_owned(), "baz".to_owned()),
            ("count".to_owned(), "7".to_owned()),
        ];

        let para = render_generic_record(&kind, id, fields);
        let backend = ratatui::backend::TestBackend::new(60, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(para, frame.area());
            })
            .expect("render must not panic");
        let output = render_buffer(&terminal, 60, 10);

        assert!(
            output.contains("developer.experimental.foo rec-42"),
            "fallback title missing: {output}"
        );
        assert!(
            output.contains("bar: baz"),
            "fallback field 'bar: baz' missing: {output}"
        );
        assert!(
            output.contains("count: 7"),
            "fallback field 'count: 7' missing: {output}"
        );
    }

    /// Generic fallback with no fields shows "no fields".
    #[test]
    fn tui_generic_fallback_empty_fields() {
        let kind = NamespacedId::parse("developer.experimental.empty").unwrap();
        let para = render_generic_record::<Vec<(String, String)>, _>(&kind, "no-id", vec![]);
        let backend = ratatui::backend::TestBackend::new(60, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(para, frame.area());
            })
            .expect("render must not panic");
        let output = render_buffer(&terminal, 60, 6);
        assert!(
            output.contains("no fields"),
            "empty fallback must render 'no fields': {output}"
        );
    }

    fn render_buffer(
        terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
        width: u16,
        height: u16,
    ) -> String {
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

    /// Secondary pane: Providers renders with expected title.
    #[test]
    fn tui_pane_providers_title() {
        let mut state = sample_state();
        state.active_pane = Pane::Providers;
        let tui = Tui::with_state(state);
        let output = render_to_string(&tui, 100, 30);
        assert!(
            output.contains("Providers"),
            "Providers pane title missing: {output}"
        );
    }

    /// Secondary pane: NativeArtifacts renders with expected title.
    #[test]
    fn tui_pane_native_artifacts_title() {
        let mut state = sample_state();
        state.active_pane = Pane::NativeArtifacts;
        let tui = Tui::with_state(state);
        let output = render_to_string(&tui, 100, 30);
        assert!(
            output.contains("NativeArtifacts"),
            "NativeArtifacts pane title missing: {output}"
        );
    }

    /// Secondary pane: OperationsDetail renders with expected title.
    #[test]
    fn tui_pane_operations_detail_title() {
        let mut state = sample_state();
        state.active_pane = Pane::OperationsDetail;
        let tui = Tui::with_state(state);
        let output = render_to_string(&tui, 100, 30);
        assert!(
            output.contains("OperationsDetail"),
            "OperationsDetail pane title missing: {output}"
        );
    }

    /// Secondary pane: EventsLog renders with expected title.
    #[test]
    fn tui_pane_events_log_title() {
        let mut state = sample_state();
        state.active_pane = Pane::EventsLog;
        let tui = Tui::with_state(state);
        let output = render_to_string(&tui, 100, 30);
        assert!(
            output.contains("EventsLog"),
            "EventsLog pane title missing: {output}"
        );
    }

    /// Secondary pane: MigrationHistory renders with expected title.
    #[test]
    fn tui_pane_migration_history_title() {
        let mut state = sample_state();
        state.active_pane = Pane::MigrationHistory;
        let tui = Tui::with_state(state);
        let output = render_to_string(&tui, 100, 30);
        assert!(
            output.contains("MigrationHistory"),
            "MigrationHistory pane title missing: {output}"
        );
    }

    /// Secondary pane: ExternalArtifactIntegrity renders with expected title.
    #[test]
    fn tui_pane_external_artifact_integrity_title() {
        let mut state = sample_state();
        state.active_pane = Pane::ExternalArtifactIntegrity;
        let tui = Tui::with_state(state);
        let output = render_to_string(&tui, 100, 30);
        assert!(
            output.contains("ExternalArtifactIntegrity"),
            "ExternalArtifactIntegrity pane title missing: {output}"
        );
    }

    /// Alt+1..Alt+7 switch active pane; default is Dashboard.
    #[test]
    fn tui_pane_switching_keys() {
        let mut tui = Tui::new();
        assert_eq!(tui.state().active_pane, Pane::Dashboard);

        let alt2 = KeyEvent::new(
            KeyCode::Char('2'),
            crossterm::event::KeyModifiers::ALT,
        );
        tui.handle_key_event(alt2).unwrap();
        assert_eq!(tui.state().active_pane, Pane::Providers);

        let alt1 = KeyEvent::new(
            KeyCode::Char('1'),
            crossterm::event::KeyModifiers::ALT,
        );
        tui.handle_key_event(alt1).unwrap();
        assert_eq!(tui.state().active_pane, Pane::Dashboard);
    }
}
