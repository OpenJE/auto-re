# auto-re-stage-0 Learnings Notepad

## 2026-07-17 Atlas: initial orchestration
- Plan approved by Momus + Oracle on round 3.
- 7-crate workspace + autore-stage1 deferral confirmed by user.
- UUIDv7, SHA-256 default + BLAKE3 retained, in-process broadcast events confirmed.
- No M1 code changes in task 1 — only relocation.

## 2026-07-17 Sisyphus: workspace split completed
- Moved domain/ids to `autore-schema`, storage to `autore-store`, events to `autore-events`, tui/runtime to `autore-tui`, core Error/Result to `autore-core`, app re-exports to `autore-app`, and all M1 code (analysis/model/scheduler/worker/engine/store + headless CLI) to `autore-stage1`.
- The `auto-re` binary lives in `autore-cli` and delegates to `autore_stage1::cli::run`.
- `FunctionAnalysisOutput`, `ProposedClaim`, and `ProposedEvidence` had to move from `worker::output` to `autore-schema::worker_output` to prevent `autore-schema` (domain/ids) from depending on `autore-stage1` (worker); `autore-stage1::worker::output` re-exports them to preserve the public path.
- `refinery::embed_migrations!("../migrations")` in `autore-store/src/storage/database.rs` keeps the repo-root `migrations/` directory working.
- Kept a `tui` feature in `autore-stage1` (default on) so the no-subcommand CLI test path remains unchanged.
- Verified: `cargo build`, `cargo build -p autore-stage1`, `cargo build --workspace`, `cargo test`, `cargo test -p autore-stage1` all pass; grep for `idax|gdbstub|llama_cpp` in default crates returns empty.
- Evidence log: `.omo/evidence/task-1-auto-re-stage-0-workspace-split.log`.
