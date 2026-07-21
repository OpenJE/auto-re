# F4 Scope Fidelity Re-Run Findings

**Date:** 2026-07-18
**Verifier:** Oracle (strategic-technical-advisor)
**Context:** Re-run after fix moved `Database::open` from `autore-tui` to `autore-app::open_project_client` in `autore-app/src/lifecycle.rs`.

## Check Results

### 1. Provider-specific code (IDAError / idax:: / gdbstub / llama_cpp)
- **Result:** ✅ PASS
- **Detail:** Zero matches across all 7 default crates.

### 2. autore-stage1 excluded from default-members
- **Result:** ✅ PASS
- **Detail:** `Cargo.toml` lines 12–20: `default-members` lists exactly 7 crates. `autore-stage1` is in `members` (line 10) only.

### 3. TUI does not touch rusqlite / Database
- **Result:** ✅ PASS
- **Detail:** `grep -rn 'rusqlite\|Database' autore-tui/src` — zero matches. `runtime.rs` now calls `autore_app::open_project_client(project_dir)?` (line 24). No `use autore_store` import anywhere.

### 4. autore-tui/Cargo.toml has no autore-store dependency
- **Result:** ✅ PASS
- **Detail:** Dependencies are: `autore-core`, `autore-schema`, `autore-events`, `autore-app`, `ratatui`, `crossterm`, `tokio`, `serde_json`. No `autore-store`.

### 5. No disassembly/decompilation/CFG/SCC/symbolic exec/sandbox exec implementation code
- **Result:** ✅ PASS
- **Detail:** Zero implementation functions found. Schema crate retains domain vocabulary (enum variants, provider kind constants, capability flags) — this is metadata, not executable RE logic.

### 6. No network transport code (TCP/HTTP/WebSocket/gRPC)
- **Result:** ✅ PASS
- **Detail:** Zero matches across all 7 default crates.

### 7. No LLM/model provider implementation code
- **Result:** ✅ PASS
- **Detail:** Only match is `tui_state_machine_query_completion_updates_project_summary` — a test function name where "completion" refers to query dispatch completion, not LLM inference. False positive.

## Pre-existing Verification (from orchestrator)
- `cargo fmt --all --check` — exit 0
- `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` — exit 0
- `cargo test --workspace --exclude autore-stage1` — all tests pass
- `cargo test -p autore-tui --test pty_integration -- --ignored --nocapture` — pass

---

## VERDICT: APPROVE

All 7 scope-fidelity checks pass. The previous blocking issue (TUI directly importing `Database`) is resolved. The TUI now depends only on `autore-app`'s public API via `open_project_client()`, and `autore-store` has been removed from its dependency list.
