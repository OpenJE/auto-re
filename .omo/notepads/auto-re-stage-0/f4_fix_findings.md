# F4 Fix Findings

**Date:** 2026-07-18
**Task:** Fix F4 scope-fidelity rejection by removing `autore-store` from `autore-tui`'s dependency graph.

## Changes Made

### 1. New factory in `autore-app`

Added `autore_app::open_project_client(project_dir: &Path) -> Result<(LocalAutoReClient, ProjectId)>` in `autore-app/src/lifecycle.rs`.

This factory:
- Loads `project.auto-re/project.toml` via `ProjectManifest::load`
- Opens `project.auto-re/project.sqlite3` via `autore_store::Database::open`
- Constructs a `LocalProjectEventService` + `EventBroadcaster`
- Builds an `ApplicationService` and wraps it in a `LocalAutoReClient`
- Returns the client plus the loaded `ProjectId`

The function is re-exported from `autore-app/src/lib.rs` alongside the existing lifecycle functions.

### 2. `autore-tui/src/runtime.rs` now delegates to the factory

- Removed the local `build_client` helper.
- Removed direct imports of `autore_store::Database`, `ProjectManifest`, `EventBroadcaster`, `LocalProjectEventService`, `ApplicationService`, and `LocalAutoReClient`.
- `run_with_project` now calls `autore_app::open_project_client(project_dir)?` and keeps the `AutoReClient` trait import so `subscribe_events` is in scope.
- Public behavior of `run_with_project` is unchanged.

### 3. `autore-tui/Cargo.toml` dependency cleanup

- Removed the `autore-store = { path = "../autore-store" }` dependency.
- `autore-tui` now depends on `autore-app` only for project client construction.

## Verification Results

| Command | Result |
| --- | --- |
| `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` | PASS |
| `cargo test -p autore-tui` | PASS (56 tests) |
| `cargo test -p autore-tui --test pty_integration -- --ignored --nocapture` | PASS (`pty_tui_lifecycle` ok) |
| `grep -r 'autore_store\|Database::open\|rusqlite' autore-tui/src` | No matches |
| `cargo test --workspace --exclude autore-stage1` | PASS (all workspace tests) |

## Boundary Check

- `autore-tui/src/runtime.rs` no longer imports `autore_store::Database` or calls `Database::open`.
- The TUI does not touch SQLite/rusqlite directly; all persistence access is through the `AutoReClient` trait.
- No other TUI logic was modified.

## VERDICT: FIXED

The F4 scope-fidelity rejection is resolved. The TUI is decoupled from the store layer and uses the `autore-app` factory to obtain a pre-constructed client.
