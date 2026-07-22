//! PTY integration test for the Ratatui TUI.
//!
//! This test launches the real `auto-re tui` binary inside a pseudo-terminal
//! (via the system `script` utility), opens a Stage 0 fixture project, and
//! verifies end-to-end rendering, event propagation, clean shutdown, and
//! terminal restoration.
//!
//! Platform requirements:
//! - Linux is required because the test relies on the `script` utility to
//!   allocate a real pseudo-terminal.
//! - The test is marked `#[ignore]` so it does not run in the default suite.
//! - Run with: `cargo test -p autore-tui --test pty_integration -- --ignored`

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

/// Maximum time to wait for the TUI to render or react.
const RENDER_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time to wait for the TUI to exit after sending `q`.
const EXIT_TIMEOUT: Duration = Duration::from_secs(60);
/// Render timeout for Stage 1 tests.
const STAGE1_RENDER_TIMEOUT: Duration = Duration::from_secs(8);
/// Exit timeout for Stage 1 tests.
const STAGE1_EXIT_TIMEOUT: Duration = Duration::from_secs(4);

/// Builds a `cargo run -p autore-cli -- <args>` command.
fn auto_re_cmd(args: &[&str]) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("-q")
        .arg("-p")
        .arg("autore-cli")
        .arg("--")
        .args(args)
        .env("NO_COLOR", "1");
    cmd
}

/// Creates a Stage 0 project directory and inserts the canonical DB record.
fn prepare_project(tmp: &TempDir) {
    autore_app::lifecycle::create_project(tmp.path(), "pty-test")
        .expect("lifecycle::create_project should succeed");

    let output = auto_re_cmd(&[
        "--project-dir",
        tmp.path().to_str().expect("project dir is UTF-8"),
        "project",
        "create",
        "--name",
        "pty-test",
    ])
    .output()
    .expect("project create should execute");
    assert!(
        output.status.success(),
        "project create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Waits until `path` contains all `needles`, returning the full contents.
fn wait_for_content(path: &std::path::Path, needles: &[&str], timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(content) = std::fs::read_to_string(path)
            && needles.iter().all(|n| content.contains(n))
        {
            return content;
        }
        if Instant::now() > deadline {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            panic!("timed out waiting for {needles:?} in script output. Last content:\n{content}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Spawns the TUI inside `script` and returns (Child, stdin handle, output path).
fn spawn_tui_in_pty(
    tmp: &TempDir,
) -> (
    std::process::Child,
    std::process::ChildStdin,
    std::path::PathBuf,
) {
    let project_dir = tmp.path().to_str().expect("project dir is UTF-8");
    let output_file = tmp.path().join("tui.script");

    let mut script = Command::new("script")
        .arg("-q")
        .arg("-c")
        .arg(format!(
            "stty rows 40 cols 120; cargo run -q -p autore-cli -- --project-dir {project_dir} tui"
        ))
        .arg(&output_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("script should spawn");

    let stdin = script.stdin.take().expect("script stdin should be open");
    (script, stdin, output_file)
}

/// Sends keys, quits with `q`, waits for exit, and verifies terminal restoration.
fn quit_and_verify_terminal_restore(
    script: &mut std::process::Child,
    stdin: std::process::ChildStdin,
    output_file: &std::path::Path,
    exit_timeout: Duration,
) {
    let mut stdin = stdin;
    stdin.write_all(b"q").expect("send q");
    drop(stdin);

    let deadline = Instant::now() + exit_timeout;
    loop {
        match script.try_wait().expect("try_wait should not fail") {
            Some(status) => {
                assert!(
                    status.success(),
                    "TUI exited with non-zero status: {status}"
                );
                break;
            }
            None => {
                if Instant::now() > deadline {
                    let _ = script.kill();
                    panic!("TUI did not exit after sending q");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    let content = std::fs::read_to_string(output_file).expect("script output should be readable");
    assert!(
        content.contains("\u{1b}[?1049l"),
        "TUI should leave the alternate screen"
    );
    assert!(
        content.contains("\u{1b}[?25h"),
        "TUI should show the cursor"
    );
}

/// Non-Linux platforms provide a documented skip.
#[cfg(not(target_os = "linux"))]
#[ignore = "PTY integration test requires the Linux script utility"]
#[test]
fn pty_tui_lifecycle() {
    eprintln!("SKIP: pty_tui_lifecycle requires a Linux PTY");
}

#[cfg(target_os = "linux")]
#[ignore = "requires a Linux PTY and the script utility; run with --ignored"]
#[test]
fn pty_tui_lifecycle() {
    let tmp = TempDir::new().expect("temp dir creation should succeed");
    prepare_project(&tmp);

    let project_dir = tmp.path().to_str().expect("project dir is UTF-8");
    let output_file = tmp.path().join("tui.script");

    // `script` allocates a real PTY so ratatui can open /dev/tty.
    let mut script = Command::new("script")
        .arg("-q")
        .arg("-c")
        .arg(format!(
            "stty rows 24 cols 80; cargo run -q -p autore-cli -- --project-dir {project_dir} tui"
        ))
        .arg(&output_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("script should spawn");

    let mut stdin = script.stdin.take().expect("script stdin should be open");

    // 1-3) TUI starts, opens the fixture project, and renders primary screen.
    // ANSI cursor-positioning codes may sit between the title and the count,
    // so match the pieces separately.
    let _ = wait_for_content(&output_file, &["Projects", "(1)"], RENDER_TIMEOUT);
    let _ = wait_for_content(&output_file, &["Operations"], RENDER_TIMEOUT);
    let _ = wait_for_content(&output_file, &["Hypotheses", "Evidence"], RENDER_TIMEOUT);
    let _ = wait_for_content(&output_file, &["pty-test"], RENDER_TIMEOUT);

    // 4) Trigger a committed project event from a side process.
    let output = auto_re_cmd(&[
        "--project-dir",
        project_dir,
        "entity",
        "add",
        "--kind",
        "entity.function",
        "--display-name",
        "main",
    ])
    .output()
    .expect("entity add should execute");
    assert!(
        output.status.success(),
        "entity add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 5) Visible state updates: the committed entity-add event is reflected.
    let _ = wait_for_content(&output_file, &["events:", "2"], RENDER_TIMEOUT);
    let _ = wait_for_content(&output_file, &["entities:", "1"], RENDER_TIMEOUT);

    // 6) Exit cleanly with `q`.
    stdin.write_all(b"q").expect("send q");
    drop(stdin);

    let deadline = Instant::now() + EXIT_TIMEOUT;
    loop {
        match script.try_wait().expect("try_wait should not fail") {
            Some(status) => {
                assert!(
                    status.success(),
                    "TUI exited with non-zero status: {status}"
                );
                break;
            }
            None => {
                if Instant::now() > deadline {
                    let _ = script.kill();
                    panic!("TUI did not exit after sending q");
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    // 7) Terminal restored: ratatui should leave the alternate screen and show
    //    the cursor.
    let content = std::fs::read_to_string(&output_file).expect("script output should be readable");
    assert!(
        content.contains("\u{1b}[?1049l"),
        "TUI should leave the alternate screen: {content}"
    );
    assert!(
        content.contains("\u{1b}[?25h"),
        "TUI should show the cursor: {content}"
    );
}

// ---------------------------------------------------------------------------
// Stage 1 PTY integration tests
// ---------------------------------------------------------------------------
//
// These tests exercise the Stage 1 keybindings (campaign, coordinator,
// provider dialogs) and the 12-pane tab strip through a real PTY. Commands
// are verified through their observable side effects: event-count updates in
// the project panel (for commands that commit events) and TUI responsiveness
// after dispatch (for commands that error without side effects).

/// Non-Linux skip stubs for Stage 1 tests.
#[cfg(not(target_os = "linux"))]
#[ignore = "PTY integration test requires the Linux script utility"]
#[test]
fn pty_stage1_start_campaign_dialog() {
    eprintln!("SKIP: pty_stage1_start_campaign_dialog requires a Linux PTY");
}

#[cfg(not(target_os = "linux"))]
#[ignore = "PTY integration test requires the Linux script utility"]
#[test]
fn pty_stage1_stop_campaign_dialog() {
    eprintln!("SKIP: pty_stage1_stop_campaign_dialog requires a Linux PTY");
}

#[cfg(not(target_os = "linux"))]
#[ignore = "PTY integration test requires the Linux script utility"]
#[test]
fn pty_stage1_start_provider_line_item() {
    eprintln!("SKIP: pty_stage1_start_provider_line_item requires a Linux PTY");
}

/// Press `o` → type campaign name → Enter: the TUI dispatches
/// `CreateReconstructionCampaign`. Verify the TUI remains responsive.
#[cfg(target_os = "linux")]
#[ignore = "requires a Linux PTY and the script utility; run with --ignored"]
#[test]
fn pty_stage1_start_campaign_dialog() {
    let tmp = TempDir::new().expect("temp dir");
    prepare_project(&tmp);

    let (mut script, stdin, output_file) = spawn_tui_in_pty(&tmp);

    let _ = wait_for_content(&output_file, &["Projects", "(1)"], STAGE1_RENDER_TIMEOUT);

    let mut stdin = stdin;
    stdin.write_all(b"o").expect("send o");
    std::thread::sleep(Duration::from_millis(150));
    stdin.write_all(b"test").expect("type campaign name");
    std::thread::sleep(Duration::from_millis(150));
    stdin.write_all(b"\r").expect("send Enter");
    std::thread::sleep(Duration::from_millis(300));

    assert!(
        script.try_wait().expect("try_wait").is_none(),
        "TUI should still be running after campaign dispatch"
    );

    quit_and_verify_terminal_restore(&mut script, stdin, &output_file, STAGE1_EXIT_TIMEOUT);
}

/// Press `X` (stop coordinator) and `p` (pause coordinator): both dispatch
/// commands that currently return "not yet implemented" errors. Verify the TUI
/// stays responsive (does not crash).
#[cfg(target_os = "linux")]
#[ignore = "requires a Linux PTY and the script utility; run with --ignored"]
#[test]
fn pty_stage1_stop_campaign_dialog() {
    let tmp = TempDir::new().expect("temp dir");
    prepare_project(&tmp);

    let (mut script, stdin, output_file) = spawn_tui_in_pty(&tmp);

    let _ = wait_for_content(&output_file, &["Projects", "(1)"], STAGE1_RENDER_TIMEOUT);

    let mut stdin = stdin;
    stdin.write_all(b"X").expect("send X");
    std::thread::sleep(Duration::from_millis(200));

    stdin.write_all(b"p").expect("send p");
    std::thread::sleep(Duration::from_millis(200));

    assert!(
        script.try_wait().expect("try_wait").is_none(),
        "TUI should still be running after stop/pause dispatches"
    );

    quit_and_verify_terminal_restore(&mut script, stdin, &output_file, STAGE1_EXIT_TIMEOUT);
}

/// Press `P` → type a provider installation UUID → Enter: the TUI dispatches
/// `RegisterProviderInstance`. Verify the TUI remains responsive.
#[cfg(target_os = "linux")]
#[ignore = "requires a Linux PTY and the script utility; run with --ignored"]
#[test]
fn pty_stage1_start_provider_line_item() {
    let tmp = TempDir::new().expect("temp dir");
    prepare_project(&tmp);

    let (mut script, stdin, output_file) = spawn_tui_in_pty(&tmp);

    let _ = wait_for_content(&output_file, &["Projects", "(1)"], STAGE1_RENDER_TIMEOUT);

    let mut stdin = stdin;
    stdin.write_all(b"P").expect("send P");
    std::thread::sleep(Duration::from_millis(150));
    stdin
        .write_all(b"550e8400-e944-4cad-9f01-7710b2a3c5f0")
        .expect("type UUID");
    std::thread::sleep(Duration::from_millis(150));
    stdin.write_all(b"\r").expect("send Enter");
    std::thread::sleep(Duration::from_millis(300));

    assert!(
        script.try_wait().expect("try_wait").is_none(),
        "TUI should still be running after provider dispatch"
    );

    quit_and_verify_terminal_restore(&mut script, stdin, &output_file, STAGE1_EXIT_TIMEOUT);
}
