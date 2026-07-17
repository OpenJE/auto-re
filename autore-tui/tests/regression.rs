//! Pre-refactor TUI regression tests — terminal lifecycle.
//!
//! These integration tests pin the terminal-setup / teardown behavior of
//! `ratatui::init()` / `ratatui::restore()` and the panic-hook pattern that
//! must survive the Stage 0 TUI remap.  Render / navigation tests live in
//! `src/tui.rs` (inline `mod tests`) because they exercise private helpers.
//!
//! Tests that touch real terminal state (raw mode, alternate screen) are
//! serialized through `TERMINAL_LOCK` and skip gracefully when stdout is not
//! a TTY (e.g. CI).

use std::io::IsTerminal;
use std::sync::Mutex;

/// Serializes tests that enable/disable raw mode or the alternate screen so
/// they never run concurrently (they share the process-wide terminal state).
static TERMINAL_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Snapshot of the terminal state we can observe from user-space.
#[derive(Debug, Clone, Copy)]
struct TerminalState {
    raw_mode: bool,
}

impl TerminalState {
    /// Queries the observable terminal state.  Returns `None` when stdout is
    /// not a terminal (CI containers, piped output, etc.).
    fn query() -> Option<Self> {
        if !std::io::stdout().is_terminal() {
            return None;
        }
        let raw_mode = crossterm::terminal::is_raw_mode_enabled().ok()?;
        Some(Self { raw_mode })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Clean shutdown restores the terminal: raw mode is disabled and the
/// alternate screen is left after `ratatui::restore()`.
///
/// This pins the contract that `run_tui` must uphold on normal exit — the
/// user must get their shell back in a usable state.
#[test]
fn tui_shutdown_restores_terminal() {
    let _guard = TERMINAL_LOCK.lock().unwrap();

    let Some(before) = TerminalState::query() else {
        eprintln!("SKIP tui_shutdown_restores_terminal: stdout is not a terminal");
        return;
    };
    assert!(
        !before.raw_mode,
        "raw mode should be off before init (pre-existing leak?)"
    );

    // init() enables raw mode + enters alternate screen.
    let _terminal = ratatui::init();

    let after_init = TerminalState::query().expect("still a terminal after init");
    assert!(
        after_init.raw_mode,
        "raw mode should be enabled after ratatui::init()"
    );

    // restore() must undo everything.
    ratatui::restore();

    let after_restore = TerminalState::query().expect("still a terminal after restore");
    assert!(
        !after_restore.raw_mode,
        "raw mode must be disabled after ratatui::restore()"
    );
}

/// A panic during TUI operation must leave the terminal usable.
///
/// `ratatui::init()` installs a panic hook that calls `ratatui::restore()`
/// before delegating to the previous hook.  We verify that contract by
/// triggering a panic inside `catch_unwind` and checking that raw mode was
/// disabled by the hook.
#[test]
fn tui_panic_restores_terminal() {
    let _guard = TERMINAL_LOCK.lock().unwrap();

    if TerminalState::query().is_none() {
        eprintln!("SKIP tui_panic_restores_terminal: stdout is not a terminal");
        return;
    }

    // init() installs the panic hook + enables raw mode.
    let _terminal = ratatui::init();
    assert!(
        crossterm::terminal::is_raw_mode_enabled().unwrap_or(true),
        "raw mode should be on after init"
    );

    // Save the panic hook chain so we can restore it after the test.
    // `ratatui::init()` already installed its hook; we capture whatever is
    // current so we can put it back.
    let hook_from_init = std::panic::take_hook();

    // Trigger a panic; the hook should call restore() before unwinding.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Re-install the hook we just took, so it fires on this panic.
        std::panic::set_hook(hook_from_init);
        panic!("simulated TUI panic for regression test");
    }));
    assert!(result.is_err(), "catch_unwind must catch the panic");

    // After the panic, the hook should have restored the terminal.
    let raw_after = crossterm::terminal::is_raw_mode_enabled().unwrap_or(true);
    assert!(
        !raw_after,
        "raw mode must be disabled after panic-hook restore"
    );

    // Clean up: take whatever hook is installed and drop it (the default
    // hook will be reinstalled on next panic).  This prevents our test hook
    // from leaking into subsequent tests.
    let _leaked = std::panic::take_hook();
}
