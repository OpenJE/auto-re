# F2 Code Quality Review — Final Verification Wave

**Date:** 2026-07-18
**Reviewer:** Oracle (code quality)
**Scope:** 7 default crates + autore-stage1 (deferred)

---

## Checklist Results

### ✅ 1. `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings`
- **Result:** PASSED (exit 0, zero warnings)
- All 7 default crates are clippy-clean with strict `-D warnings`.

### ✅ 2. `cargo fmt --all --check`
- **Result:** PASSED (exit 0)
- All workspace files are properly formatted.

### ⚠️ 3. `cargo clippy --workspace --all-targets --all-features`
- **Result:** FAILED (exit 101)
- **Three distinct failure modes:**

  **a) `llama_cpp_sys` build failure (ENVIRONMENT)**
  - The optional `llama` feature of `autore-stage1` depends on `llama_cpp_sys`, which requires `libclang` for bindgen.
  - `libclang` is not installed in this build environment.
  - This is a system dependency issue, not a Rust code quality issue.

  **b) `idax-sys` C++ compilation failure (EXTERNAL DEPENDENCY)**
  - The optional `ida` feature of `autore-stage1` depends on `idax-sys`, whose C++ shim (`idax_shim.cpp`) has API mismatches with the bundled IDA SDK headers (e.g., `register_class` → `register_name`, `structure_ids` → `structure_name`, `CommentPosition` constructor changes).
  - These are upstream C++ binding errors in `idax-sys v0.3.0`, not Rust code defects.

  **c) `autore-stage1/tests/campaign_smoke.rs:26` broken import (CODE DEFECT)**
  - `use autore_stage1::tui::state::TuiUpdate;` — `TuiUpdate` was removed in Task 28/29 when the TUI update channel was replaced by `ProjectEventSubscription`.
  - This is a real code defect in `autore-stage1`'s integration test.
  - **Impact:** `campaign_smoke.rs` cannot compile. This test was already broken before the verification wave.

  **Note:** Without `--all-features`, `cargo clippy --workspace --all-targets` also fails due to (c), plus 6 clippy warnings in `autore-stage1` lib (print_literal, collapsible_if, needless_return, module_inception). None of these affect the 7 default crates.

### ✅ 4. No TODO/FIXME/unimplemented!/unwrap/panic! in production code
- **TODO/FIXME:** Zero matches across all 7 default crates.
- **unimplemented!:** Zero in production code. All instances are in `#[cfg(test)]` test helpers (`SlowClient`, `RecordingClient` in `autore-tui`).
- **unwrap() in production code — 3 sites, all justified:**
  1. `autore-core/src/validation.rs:227` — `path.iter().position(|&x| x == next).unwrap()` inside DFS cycle detection. Mathematically guaranteed: `next` was just found with `colors[next] == 1` (in-progress), so it must exist in `path`.
  2. `autore-core/src/logging.rs:45` — `lock.lock().unwrap()` on a global Mutex during panic hook installation. Standard Rust pattern for one-time global initialization.
  3. `autore-schema/src/domain/records.rs` — ~60 `LazyLock` constants using `NamespacedId::parse("literal").unwrap()`. These are hardcoded string literals validated at first access; failure would indicate a programming error at startup.
- **panic! in production code:** Zero instances.

### ✅ 5. No scope creep
- No `scheduler`, `AnalysisBackend`, `WorkerPool`, `JobQueue` implementation code in any default crate.
- "worker" references in `autore-schema` are M1 domain types only (`WorkerRunId`, `worker_output`, `Provenance::Agent`) — data model definitions, not execution infrastructure.
- No `idax`, `ghidra_sys`, `llama_cpp`, `gdbstub`, or `EngineError` references in any default crate.
- Stage 1 analysis/scheduler/worker code is properly isolated in `autore-stage1`.

---

## Findings Summary

| # | Severity | Crate | File | Description |
|---|----------|-------|------|-------------|
| F2-1 | LOW | autore-stage1 | tests/campaign_smoke.rs:26 | Broken import: `TuiUpdate` no longer exists. Test cannot compile. |
| F2-2 | INFO | autore-stage1 | src/cli/campaign.rs, src/cli/task.rs, src/cli/headless.rs, src/cli/mod.rs, src/scheduler/mod.rs | 6 clippy warnings (print_literal ×3, collapsible_if, needless_return, module_inception) |
| F2-3 | INFO | autore-stage1 | Cargo.toml | `--all-features` requires system dependencies (libclang, IDA SDK C++ headers) not available in build environment |

---

## VERDICT: APPROVE (with findings)

**Rationale:**
- All 7 default crates (`autore-schema`, `autore-core`, `autore-store`, `autore-app`, `autore-events`, `autore-cli`, `autore-tui`) pass every quality check with zero defects.
- Production code contains no unjustified `unwrap()`, `panic!`, `TODO`, `FIXME`, or `unimplemented!()`.
- No scope creep: Stage 1 code is properly isolated in `autore-stage1`.
- The 3 findings are all in `autore-stage1` (the deferred crate with acknowledged leniency):
  - F2-1 is a broken integration test that should be fixed when Stage 1 work resumes.
  - F2-2 are minor clippy warnings in deferred code.
  - F2-3 is an environment constraint, not a code defect.
- None of these findings affect the Stage 0 deliverables or the 7 default crates.

**Recommended follow-up (not blocking):**
- Fix `campaign_smoke.rs` import when Stage 1 work resumes (replace `TuiUpdate` with appropriate type or remove the test).
- Address the 6 clippy warnings in `autore-stage1` lib.
- Install `libclang` in CI environment for `--all-features` validation.
