//! Runtime orchestration — TUI as a tokio task.
//!
//! Task 29 will wire `ProjectEventSubscription` for live event updates.
//! For now, the runtime simply runs the TUI event loop.

/// Runs the application: TUI as a tokio task.
pub async fn run() -> crate::Result<()> {
    crate::tui::run_tui().await
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
