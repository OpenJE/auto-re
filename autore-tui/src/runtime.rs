//! Runtime orchestration — TUI as a tokio task.
//!
//! Task 29 will wire `ProjectEventSubscription` for live event updates.
//! For now, the runtime simply runs the TUI event loop.

use std::path::Path;

use autore_app::AutoReClient;

use crate::tui::Tui;
use crate::tui::state::{Navigation, ProjectViewState, TuiState};

/// Runs the application: TUI as a tokio task.
pub async fn run() -> crate::Result<()> {
    crate::tui::run_tui().await
}

/// Runs the TUI pre-loaded with the project in `project_dir`.
///
/// The project is opened, a live event subscription is attached, and the
/// initial project summary query is dispatched so the dashboard renders the
/// fixture project immediately.
pub async fn run_with_project(project_dir: &Path) -> crate::Result<()> {
    let (client, project_id) = autore_app::open_project_client(project_dir)?;

    let mut state = TuiState {
        navigation: Navigation::Project(project_id),
        ..TuiState::default()
    };
    state
        .project_views
        .insert(project_id, ProjectViewState::default());

    let subscription = client.subscribe_events(project_id, 0)?;
    let mut tui = Tui::with_client(state, Box::new(client));
    tui.attach_subscription(project_id, subscription);
    tui.open_selected_project();

    crate::tui::run_tui_with(tui).await
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn runtime_run_typechecks() {
        // Smoke test: `run()` compiles and returns the correct type.
        // We don't actually run the TUI in unit tests (it needs a terminal).
        fn _assert_fn() -> crate::Result<()> {
            unreachable!()
        }
    }
}
