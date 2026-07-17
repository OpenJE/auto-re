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

## 2026-07-17 Sisyphus: task 3 stage0-audit.md produced
- Wrote `docs/stage0-audit.md` classifying all 46 M1 files (44 source + 2 tests).
- Breakdown: 4 RETAIN UNCHANGED (9%), 11 RETAIN WITH ADAPTATION (24%), 1 MOVE BEHIND SHARED SERVICES (2%), 22 DEFER TO LATER STAGE (48%), 8 REMOVE (17%).
- Key REMOVE classifications: Campaign/Task/Claim (→ Project/Operation/Hypothesis), Provenance (→ Derivation+DerivationMethod), Evidence/EvidenceKind (→ EvidenceRecord+EvidenceValue), EntityId closed enum (→ opaque EntityId+namespaced kind), campaign_smoke/kill_resume tests (→ Stage 0 replacements).
- Key RETAIN ADAPTED: `define_id!` macro (v4→v7), `ContentHash` (add HashAlgorithm), `database.rs` (atomic state+event transactions, no DB-generated IDs, migration backups), 4-panel TUI layout (remapped in 0I), repository traits (event-emitting wrappers).
- Open questions documented: Provenance→Derivation mapping, ContentHash algorithm negotiation, atomic transaction wrapper design, EntityId opaque migration.
- All 3 user decisions (Q1: 7-crate+stage1 deferral, Q2: UUIDv7, Q3: aggressive M1 replacement) and 7 default crate boundaries documented in the audit.
