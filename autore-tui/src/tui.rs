//! TUI dashboard — campaign overview with task and claim summaries.
//!
//! Renders a four-panel layout:
//! - Left: campaign list with selection highlight
//! - Top-right: selected campaign status
//! - Middle-right: task list with per-task state
//! - Bottom-right: claim summary and progress gauge
//!
//! The TUI is read-only: it displays state from repositories but never
//! mutates campaigns, tasks, or claims.

pub mod state;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, Paragraph, Row, Table};

use crate::tui::state::{
    ClaimSummary, DashboardState, TaskSummary, TuiUpdate, format_campaign_state, format_task_state,
};

/// TUI application state.
pub struct Tui {
    state: DashboardState,
}

impl Tui {
    /// Creates a new TUI with the given dashboard state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: DashboardState::default(),
        }
    }

    /// Creates a TUI pre-loaded with the given dashboard state.
    #[must_use]
    pub fn with_state(state: DashboardState) -> Self {
        Self { state }
    }

    /// Returns a reference to the current dashboard state.
    #[must_use]
    pub fn state(&self) -> &DashboardState {
        &self.state
    }

    /// Applies a `TuiUpdate` to the internal dashboard state.
    pub fn apply_update(&mut self, update: TuiUpdate) {
        self.state.apply_update(update);
    }

    /// Handles a key event. Returns `true` if the app should quit.
    fn handle_key_event(&mut self, key_event: KeyEvent) -> io::Result<bool> {
        match key_event.kind {
            KeyEventKind::Press => match key_event.code {
                KeyCode::Char('q') => Ok(true),
                KeyCode::Char('j') | KeyCode::Down => {
                    self.state.select_next();
                    Ok(false)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.state.select_previous();
                    Ok(false)
                }
                _ => Ok(false),
            },
            _ => Ok(false),
        }
    }

    /// Renders the full dashboard into the given frame.
    fn render(&self, frame: &mut Frame) {
        let outer = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(frame.area());

        // Left panel: campaign list
        self.render_campaign_list(frame, outer[0]);

        // Right panel: split vertically into 3 sections
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),
                Constraint::Min(5),
                Constraint::Length(5),
            ])
            .split(outer[1]);

        self.render_campaign_status(frame, right[0]);
        self.render_task_list(frame, right[1]);
        self.render_claim_summary(frame, right[2]);
    }

    fn render_campaign_list(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let items: Vec<ListItem> = self
            .state
            .campaigns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let state_str = format_campaign_state(c.state);
                let marker = if i == self.state.selected_campaign {
                    "▶ "
                } else {
                    "  "
                };
                let content = Line::from(vec![
                    Span::raw(marker),
                    Span::raw(&c.name),
                    Span::raw(" ["),
                    Span::raw(state_str),
                    Span::raw("]"),
                ]);
                ListItem::new(content)
            })
            .collect();

        let title = format!("Campaigns ({})", self.state.campaigns.len());
        let list = List::new(items).block(Block::bordered().title(title));
        frame.render_widget(list, area);
    }

    fn render_campaign_status(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let content = match self.state.selected() {
            Some(campaign) => {
                let tasks = self.state.selected_tasks();
                let task_summary = TaskSummary::from_tasks(&tasks);
                let lines = vec![
                    Line::from(vec![Span::raw("Name: ").bold(), Span::raw(&campaign.name)]),
                    Line::from(vec![
                        Span::raw("State: ").bold(),
                        Span::raw(format_campaign_state(campaign.state)),
                    ]),
                    Line::from(vec![
                        Span::raw("Tasks: ").bold(),
                        Span::raw(format!(
                            "{} total ({} completed, {} running, {} pending)",
                            task_summary.total(),
                            task_summary.completed,
                            task_summary.running,
                            task_summary.pending
                        )),
                    ]),
                ];
                Paragraph::new(lines)
            }
            None => Paragraph::new("No campaign selected.").italic(),
        };
        let block = Block::bordered().title("Campaign Status");
        frame.render_widget(content.block(block), area);
    }

    fn render_task_list(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let tasks = self.state.selected_tasks();
        if tasks.is_empty() {
            let msg = Paragraph::new("No tasks for this campaign.")
                .italic()
                .block(Block::bordered().title("Tasks"));
            frame.render_widget(msg, area);
            return;
        }

        let header = Row::new(vec!["ID", "Kind", "State", "Priority"]);
        let rows: Vec<Row> = tasks
            .iter()
            .map(|t| {
                let id_short = &t.id.to_string()[..8];
                Row::new(vec![
                    id_short.to_string(),
                    format!("{:?}", t.kind),
                    format_task_state(t.state).to_string(),
                    t.priority.score().to_string(),
                ])
            })
            .collect();

        let title = format!("Tasks ({})", tasks.len());
        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Percentage(40),
                Constraint::Length(12),
                Constraint::Length(8),
            ],
        )
        .header(header.bold())
        .block(Block::bordered().title(title));
        frame.render_widget(table, area);
    }

    fn render_claim_summary(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let claims = self.state.selected_claims();
        let summary = ClaimSummary::from_claims(&claims);

        let progress = summary.progress();
        let label = format!(
            "{}/{} accepted ({:.0}%)",
            summary.accepted,
            summary.total(),
            progress * 100.0
        );

        let gauge = Gauge::default()
            .block(Block::bordered().title("Claims Progress"))
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

/// Entry point for the TUI, called from main when the `tui` feature is enabled.
///
/// Accepts an optional `mpsc::Receiver<TuiUpdate>` for real-time updates
/// from the scheduler. When `None`, the TUI renders a static snapshot.
pub async fn run_tui(
    mut receiver: Option<tokio::sync::mpsc::Receiver<TuiUpdate>>,
) -> crate::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = Tui::new();

    loop {
        if let Some(ref mut rx) = receiver {
            while let Ok(update) = rx.try_recv() {
                app.apply_update(update);
            }
        }

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
    use crate::domain::{
        Campaign, CampaignState, Claim, ClaimPredicate, ClaimState, ClaimValue, Confidence,
        Provenance, RequiredCapabilities, Task, TaskKind, TaskPriority, TaskState, TaskSubject,
    };
    use crate::ids::{CampaignId, ClaimId, FunctionId, TaskId};

    fn make_campaign(name: &str, state: CampaignState) -> Campaign {
        let mut c = Campaign::new(CampaignId::new(), name);
        c.state = state;
        c
    }

    fn make_task(campaign_id: CampaignId, kind: TaskKind, state: TaskState) -> Task {
        let mut t = Task::new(
            TaskId::new(),
            campaign_id,
            kind,
            TaskSubject::Binary,
            TaskPriority::new(100),
            RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        );
        t.state = state;
        t
    }

    fn make_claim(state: ClaimState) -> Claim {
        let mut c = Claim::new(
            ClaimId::new(),
            crate::domain::EntityId::Function(FunctionId::new()),
            ClaimPredicate::FunctionName,
            ClaimValue::String("test_fn".into()),
            Confidence::new(0.9).unwrap(),
            Provenance::StaticAnalysis,
        );
        c.state = state;
        c
    }

    fn sample_state() -> DashboardState {
        let c1 = make_campaign("alpha", CampaignState::Active);
        let c2 = make_campaign("beta", CampaignState::Pending);
        let t1 = make_task(c1.id, TaskKind::AnalyzeFunction, TaskState::Running);
        let t2 = make_task(c1.id, TaskKind::DecompileFunction, TaskState::Completed);
        let cl1 = make_claim(ClaimState::Accepted);
        let cl2 = make_claim(ClaimState::Proposed);

        DashboardState {
            campaigns: vec![c1, c2],
            tasks: vec![t1, t2],
            claims: vec![cl1, cl2],
            selected_campaign: 0,
        }
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

    #[test]
    fn tui_dashboard_renders() {
        let tui = Tui::with_state(sample_state());
        let output = render_to_string(&tui, 80, 24);
        // Dashboard must contain the panel titles.
        assert!(
            output.contains("Campaigns"),
            "missing Campaigns panel: {output}"
        );
        assert!(
            output.contains("Campaign Status"),
            "missing Campaign Status panel: {output}"
        );
        assert!(output.contains("Tasks"), "missing Tasks panel: {output}");
        assert!(
            output.contains("Claims Progress"),
            "missing Claims Progress panel: {output}"
        );
    }

    #[test]
    fn tui_dashboard_shows_campaigns() {
        let tui = Tui::with_state(sample_state());
        let output = render_to_string(&tui, 80, 24);
        // Both campaign names must appear in the output.
        assert!(
            output.contains("alpha"),
            "campaign 'alpha' not rendered: {output}"
        );
        assert!(
            output.contains("beta"),
            "campaign 'beta' not rendered: {output}"
        );
        // The selected campaign marker must be present.
        assert!(output.contains("▶"), "selection marker missing: {output}");
        // Campaign states must be shown.
        assert!(output.contains("Active"), "Active state missing: {output}");
        assert!(
            output.contains("Pending"),
            "Pending state missing: {output}"
        );
    }

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

    #[test]
    fn tui_dashboard_empty_state() {
        let tui = Tui::new();
        let output = render_to_string(&tui, 80, 24);
        assert!(
            output.contains("No campaign selected"),
            "empty state message missing: {output}"
        );
    }

    #[test]
    fn tui_dashboard_navigation() {
        let mut tui = Tui::with_state(sample_state());
        assert_eq!(tui.state().selected_campaign, 0);

        // Move down
        let down = KeyEvent::new(KeyCode::Char('j'), crossterm::event::KeyModifiers::NONE);
        tui.handle_key_event(down).unwrap();
        assert_eq!(tui.state().selected_campaign, 1);

        // Move down again (wraps)
        tui.handle_key_event(down).unwrap();
        assert_eq!(tui.state().selected_campaign, 0);

        // Move up (wraps to end)
        let up = KeyEvent::new(KeyCode::Char('k'), crossterm::event::KeyModifiers::NONE);
        tui.handle_key_event(up).unwrap();
        assert_eq!(tui.state().selected_campaign, 1);
    }

    // -----------------------------------------------------------------------
    // Pre-refactor regression tests (Stage 0 remap baseline)
    // -----------------------------------------------------------------------
    //
    // The following tests pin the current M1 dashboard behavior so the
    // upcoming 4-panel → Stage 0 remap preserves useful functionality.

    /// Startup with empty state renders without panicking and displays the
    /// empty-state help message.
    #[test]
    fn tui_startup_renders_empty() {
        let tui = Tui::new();
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal creation must not fail");
        // Must not panic — this is the primary assertion for a cold start.
        terminal
            .draw(|frame| tui.render(frame))
            .expect("initial render must succeed");

        let output = render_to_string(&tui, 80, 24);

        // Empty state must communicate that nothing is loaded.
        assert!(
            output.contains("No campaign selected"),
            "empty-state message missing from startup render: {output}"
        );
        // The campaign list must show a count of zero.
        assert!(
            output.contains("Campaigns (0)"),
            "empty campaign list title missing: {output}"
        );
        // The task panel must indicate no tasks.
        assert!(
            output.contains("No tasks"),
            "empty task panel message missing: {output}"
        );
    }

    /// Primary 4-panel screen renders with the panel titles
    /// `Campaigns` / `Campaign Status` / `Tasks` / `Claims Progress`.
    #[test]
    fn tui_primary_panels_present() {
        let tui = Tui::with_state(sample_state());
        let output = render_to_string(&tui, 80, 24);

        // All four panel titles must be present in a single render pass.
        let expected_titles = [
            "Campaigns",
            "Campaign Status",
            "Tasks",
            "Claims Progress",
        ];
        for title in expected_titles {
            assert!(
                output.contains(title),
                "panel title {title:?} missing from dashboard render: {output}"
            );
        }
    }

    /// `j`/`Down` and `k`/`Up` navigation moves selection and wraps around
    /// both ends of the campaign list.
    #[test]
    fn tui_navigation_jk_up_down_wraps() {
        let mut tui = Tui::with_state(sample_state());
        // sample_state has 2 campaigns (alpha, beta); selection starts at 0.
        assert_eq!(tui.state().selected_campaign, 0);

        let j = KeyEvent::new(KeyCode::Char('j'), crossterm::event::KeyModifiers::NONE);
        let k = KeyEvent::new(KeyCode::Char('k'), crossterm::event::KeyModifiers::NONE);
        let down = KeyEvent::new(KeyCode::Down, crossterm::event::KeyModifiers::NONE);
        let up = KeyEvent::new(KeyCode::Up, crossterm::event::KeyModifiers::NONE);

        // 'j' moves selection down (0 → 1).
        tui.handle_key_event(j).unwrap();
        assert_eq!(tui.state().selected_campaign, 1, "'j' must move selection down");

        // Down arrow wraps around (1 → 0, since there are 2 campaigns).
        tui.handle_key_event(down).unwrap();
        assert_eq!(
            tui.state().selected_campaign, 0,
            "Down arrow must wrap from last to first"
        );

        // 'k' moves selection up, wrapping to the end (0 → 1).
        tui.handle_key_event(k).unwrap();
        assert_eq!(
            tui.state().selected_campaign, 1,
            "'k' must wrap from first to last"
        );

        // Up arrow moves selection up (1 → 0).
        tui.handle_key_event(up).unwrap();
        assert_eq!(tui.state().selected_campaign, 0, "Up arrow must move selection up");
    }

    /// `q` quits; no other common key should signal quit.
    #[test]
    fn tui_q_quits_cleanly() {
        let mut tui = Tui::new();

        // 'q' must signal quit.
        let q = KeyEvent::new(KeyCode::Char('q'), crossterm::event::KeyModifiers::NONE);
        assert!(
            tui.handle_key_event(q).unwrap(),
            "'q' must signal quit (return true)"
        );

        // Navigation keys must NOT signal quit.
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
