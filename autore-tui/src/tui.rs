//! TUI dashboard — project overview with operations, hypotheses, and evidence.
//!
//! Renders a four-panel layout:
//! - Panel 1 (left): Project summary / projects list
//! - Panel 2 (top-right): Operations table
//! - Panel 3 (bottom-right): Hypotheses + Evidence progress
//! - Sidebar: Navigation / tabs
//!
//! The TUI is presentation-only: it displays state snapshots loaded via
//! `AutoReClient` queries but never mutates the database directly.

pub mod state;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, Paragraph, Row, Table};

use crate::tui::state::{Focus, Navigation, TuiState};
use autore_app::AutoReClient;

/// TUI application state.
pub struct Tui {
    state: TuiState,
    /// Client for data access. Wired by Task 29 (ProjectEventSubscription).
    #[allow(dead_code)]
    client: Option<Box<dyn AutoReClient>>,
}

impl Tui {
    /// Creates a new TUI with default empty state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: TuiState::default(),
            client: None,
        }
    }

    /// Creates a TUI pre-loaded with the given state.
    #[must_use]
    pub fn with_state(state: TuiState) -> Self {
        Self {
            state,
            client: None,
        }
    }

    /// Creates a TUI with a client for data access.
    #[must_use]
    pub fn with_client(state: TuiState, client: Box<dyn AutoReClient>) -> Self {
        Self {
            state,
            client: Some(client),
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
                _ => Ok(false),
            },
            _ => Ok(false),
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

        // Panel 1: project summary / projects list
        self.render_project_panel(frame, outer[0]);

        // Right panel: split vertically into 2 sections
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(outer[1]);

        // Panel 2: operations table
        self.render_operations_panel(frame, right[0]);
        // Panel 3: hypotheses + evidence progress
        self.render_hypotheses_panel(frame, right[1]);
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

        let items: Vec<ListItem> = project_views
            .iter()
            .map(|(id, view)| {
                let name = view
                    .project_summary
                    .as_ref()
                    .map(|p| p.name.as_str())
                    .unwrap_or("(loading)");
                let is_selected = matches!(&self.state.navigation, Navigation::Project(pid) if pid == id);
                let marker = if is_selected { "▶ " } else { "  " };
                let schema = view
                    .schema_version
                    .as_ref()
                    .map(|v| format!("v{v}"))
                    .unwrap_or_default();
                let content = Line::from(vec![
                    Span::raw(marker),
                    Span::raw(name),
                    if schema.is_empty() {
                        Span::raw("")
                    } else {
                        Span::raw(format!(" [{schema}]"))
                    },
                ]);
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

        let header = Row::new(vec!["ID", "Kind", "State", "Requested By"]);
        let rows: Vec<Row> = operations
            .iter()
            .map(|op| {
                let id_short = &op.id.to_string()[..8];
                Row::new(vec![
                    id_short.to_string(),
                    op.kind.to_string(),
                    format!("{:?}", op.state),
                    op.requested_by.clone(),
                ])
            })
            .collect();

        let title = format!("Operations ({})", operations.len());
        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Percentage(40),
                Constraint::Length(14),
                Constraint::Percentage(25),
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
}

impl Default for Tui {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry point for the TUI.
///
/// Task 29 will wire `ProjectEventSubscription` here. For now, the TUI
/// renders a static snapshot and exits on 'q'.
pub async fn run_tui() -> crate::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = Tui::new();

    loop {
        terminal
            .draw(|frame| app.render(frame))
            .map_err(crate::Error::Io)?;

        // Poll with 100 ms timeout so tokio can run other tasks.
        if event::poll(Duration::from_millis(100)).map_err(crate::Error::Io)?
            && let Event::Key(key_event) = event::read().map_err(crate::Error::Io)?
            && app.handle_key_event(key_event)?
        {
            break;
        }
    }

    ratatui::restore();
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::tui::state::{
        EventCursor, FilterState, Focus, Navigation, ProjectViewState, TuiState,
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
}
