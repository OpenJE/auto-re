# auto-re Stage 0 — pre-approval draft

**Slug:** `auto-re-stage-0`
**Intent:** CLEAR — user supplied a complete decision-complete spec (architectural principles, ID shapes, CLI verbs, TUI rules, exit criteria).
**Review required:** false (no high-accuracy modifier requested).
**Scale:** Architecture (5+ modules, long-term impact, multi-phase 0A–0K).
**Routing ref:** `references/intent-clear.md` + `references/full-workflow.md`.

## Repo grounding (Phase 0A audit, evidence-based)

Existing code (from M1, all 20 todos complete):

- **Cargo**: single crate `auto-re`, lib + bin. `default = ["tui"]`. Optional `ida`/`gdb`/`llama` features. `idax` behind feature. `rusqlite(bundled)`, `refinery`, `clap`, `ratatui`/`crossterm`, `uuid(v4)`, `time`, `blake3`, `tracing`, `jsonschema`, `schemars`, `petgraph`, `tokio(full)`.
- **src/ids.rs**: `define_id!` macro over `uuid::Uuid` (UUIDv4). 13 IDs: ProjectId, BinaryId, BinaryRevisionId, ModuleId, FunctionId, TaskId, ClaimId, EvidenceId, CampaignId, WorkerRunId, TransactionId, ImplementationTargetId, ValidationRunId.
- **src/domain/**: `mod.rs` (Address, AddressSpace, ContentHash as BLAKE3-hex string, SymbolName, Provenance enum, Confidence f32 validated 0..=1), `campaign.rs` (Campaign + CampaignState machine), `claim.rs` (Claim + ClaimPredicate/Value/State), `evidence.rs` (Evidence, EvidenceKind, EvidenceLocation, EntityId closed enum, ArtifactId), `function.rs` (Function, lock/revision), `task/` (Task, TaskKind/State/Subject/Priority/RequiredCapabilities).
- **src/storage/**: `database.rs` (Database wrapping `Mutex<rusqlite::Connection>`, WAL+FK on, refinery `embed_migrations!`), `repositories/` (TaskRepository SQLite, ClaimRepository SQLite, plus repository traits + Noop stubs in headless_queries.rs).
- **src/scheduler/**: lease.rs, mod.rs, repos.rs (RepositorySet), scheduler.rs (Scheduler, run_campaign, CampaignEvaluation, ModelRouter integration).
- **src/worker/**: runner.rs (WorkerRunner, CancellationToken, timeout), output.rs (FunctionAnalysisOutput, ProposedClaim, ProposedEvidence).
- **src/analysis/**: backend.rs (AnalysisBackend trait + AnalysisCapability), mock.rs (10-function fixture), packet.rs (FunctionAnalysisPacket).
- **src/model/**: provider.rs (ModelProvider trait, ModelDescriptor), mock.rs, router.rs (ModelRouter).
- **src/engine.rs** + **src/engine/graph.rs**: experimental, `#[cfg(feature="ida")]` Engine + RETaskGraph.
- **src/tui.rs**: 4-panel Ratatui dashboard (Campaigns list | Campaign Status | Tasks table | Claims Progress gauge). `Tui` struct, `DashboardState`, j/k navigation + q quit, `TestBackend` render tests. `runtime.rs` has `run_tui(Some(receiver))` consuming `TuiUpdate`.
- **src/event.rs**: minimal `Event` enum (Render/KeyDown/MouseClick/WidgetStateChanged).
- **src/cli/**: mod.rs (Cli with Campaign/Task subcommands, no-subcommand -> runtime::run() TUI), campaign.rs (status), task.rs (list/status), headless.rs (run_headless with stale-lease recovery), headless_queries.rs (SqliteQueries).
- **migrations/V1__initial_schema.sql**: campaigns, binary_revisions, modules, functions, tasks, claims, evidences, leases, artifacts tables + 6 indexes.
- **tests/**: campaign_smoke.rs, kill_resume.rs.
- **.omo/plans/auto-re-m1.md**: fully checked complete; references the spec from this repo (the OLD M1 spec).

## Retain / Adapt / Move / Defer / Remove classification (draft, to ratify in plan)

- **RETAIN UNCHANGED**: `tracing`/`tracing-subscriber` wiring, `clap` parse dispatch skeleton, `ratatui::init()/restore()` terminal lifecycle, TestBackend render-test pattern, `Confidence` validation (f32, finite, 0..=1), `Address`/`AddressSpace` round-trip serde design.
- **RETAIN WITH ADAPTATION**: `define_id!` macro -> make the inner UUIDv7 (sortable) and add the new ~16 IDs. `ContentHash` -> add typed `HashAlgorithm` field. `Provenance` (closed enum) -> becomes `Derivation` (NamespacedId operation + DerivationMethod). The 4-panel TUI layout/widgets/keybindings -> remap Campaigns→Projects, Campaign Status→Project Summary (schema version, validation status, counts), Tasks→Operations, Claims Progress→Hypothesis/Evidence progress. `Database` open pattern (WAL, FK, parent dirs, refinery) -> retain but add transactional state+event commits, migration backups, no DB-generated IDs. `Application loop` -> split into shared ApplicationService (commands/queries) consumed by both CLI and TUI.
- **MOVE BEHIND SHARED SERVICES**: headless CLI's direct SQL (headless.rs, campaign.rs `status()`, task.rs list/status raw queries) -> behind ApplicationQuery layer. TUI's `TuiUpdate`-> durable `ProjectEvent` subscription + EventCursor. Scheduler's campaign loop -> generalized Operation execution (deferred execution to Stage 1; only the durable Operation *record* + state transitions are Stage 0).
- **DEFER TO STAGE 1+**: `src/analysis/*` (AnalysisBackend, mock, packet builder), `src/model/*` (ModelProvider, mock, router), `src/scheduler/scheduler.rs` campaign loop + ModelRouter, `src/worker/*` (WorkerRunner), `src/engine/*` (IDA engine), `run_headless` worker dispatch. These are NOT Stage 0 — Stage 0 has no RE analysis execution. They are kept (feature-gated or under a `stage1/` mod) but not on the default build path.
- **REMOVE**: `Campaign`/`CampaignState`, `Task`/`TaskKind`/`TaskState`/`TaskSubject` (replaced by `Project` + `Operation`+OperationState + namespaced kind). `Claim`/`ClaimState`/`ClaimPredicate`/`ClaimValue` (replaced by `Evidence`+`Hypothesis`+`Contradiction`). M1 `Evidence`/`EvidenceKind` shape (replaced by append-only `EvidenceRecord`+`EvidenceValue`+`Derivation`). Old V1 migration `campaigns`/`tasks`/`claims`/`evidences`/`leases`/`functions`/`modules` tables (replaced by Stage 0 schema, with a migration *from* a committed V1 fixture to V2). `EntityId` closed enum (replaced by `EntityId` opaque newtype + namespaced `kind`). M1 `campaign_smoke.rs`/`kill_resume.rs` tests (rewritten as Stage 0 Operation/Event + migration rollback + sequence-gap persistence round trip tests).

## DECISIONS (resolved)

### Owner-decisions (user-answered)

- **Q1 — Crate split: FULL 7-CRATE WORKSPACE NOW.**
  Adopt spec §5 layout exactly:
  - `autore-schema` (IDs, `NamespacedId`, `ContentHash`, `SchemaVersion`, `ExtensionData`, `EvidenceValue`, `BinaryLocation`, `StableEntityKey`, timestamps, validation primitives, committed serialization fixtures)
  - `autore-core` (domain rules, state transitions, validation rules — no SQLite)
  - `autore-store` (storage traits + SQLite impl + migrations, V1->V2 migration with backup)
  - `autore-app` (shared use-case commands + queries + `ApplicationService` + `LocalAutoReClient`)
  - `autore-events` (durable events, replay, broadcast subscriptions, `ProjectEventService`)
  - `autore-cli` (`auto-re` command-line parsing/presentation)
  - `autore-tui` (Ratatui application + presentation state)
  One root binary `auto-re` glues `cli` + `tui`. Existing `Cargo.toml` becomes a workspace `Cargo.toml`. This is the strictest §3.9/§9 enforcement path: compile-time boundary that CLI+TUI cannot reach into SQLite.

- **Q2 — ID format: UUIDv7.**
  Keep the `uuid` crate; enable the `v7` feature. `define_id!` macro: change `Uuid::new_v4()` -> `Uuid::now_v7()` (sortable, RFC 9562). Existing 36-char canonical text format and BLOB/TEXT storage preserved. Existing ID tests update from v4 to v7 semantics (still round-trip; Display unchanged).

- **Q3 — M1 disposition: AGGRESSIVE — replace domain, defer engine, port the guarantee.**
  REMOVE the M1 `Campaign`/`CampaignState`/`Task`/`TaskKind`/`TaskState`/`TaskSubject`/`Claim`/`ClaimState`/`ClaimPredicate`/`ClaimValue`/M1 `Evidence`/`EvidenceKind`/`EvidenceLocation` types and the V1 schema tables (campaigns/tasks/claims/evidences/leases/functions/modules/binary_revisions) from the default build.
  REPLACE with the Stage 0 record types (§6–§17) and a new V2 schema.
  DEFER `src/analysis/*`, `src/model/*`, `src/scheduler/scheduler.rs` campaign loop + `ModelRouter`, `src/worker/*`, `src/engine/*` to Stage 1+ by relocating them under `autore-stage1` (a non-default member crate, not compiled by `cargo build` / not on the default workspace test path, kept in-tree for Stage 1 reuse).
  REPLACE `tests/campaign_smoke.rs` and `tests/kill_resume.rs` with Stage 0 equivalents that PORT the kill→resume guarantee: durable `Operation` state-transition tests, atomic state-plus-event commit tests, sequence-gap recovery tests, V1->V2 migration rollback + backup tests, pseudo-terminal TUI restart test.
  The V1 migration SQL is preserved as a committed fixture (`migrations/V1__initial_schema.sql` stays; `tests/fixtures/v1_project.sqlite3` is committed for migration tests).

### Defaulted (recorded here; open to user override in approval reply)

- **Hash**: add typed `HashAlgorithm { Blake3, Sha256 }` to `ContentHash`; default **SHA-256** (matches spec §8 `artifacts/sha256/`); BLAKE3 retained for backward-compatible V1 artifact hashes. Layout: `<project>/artifacts/<algo>/<digest>`.
- **Live subscription**: in-process `tokio::sync::broadcast` for Stage 0 live event delivery (spec §17 "sufficient"); durable `EventStore` remain authoritative.
- **Storage**: SQLite via `rusqlite(bundled)` + `refinery` retained (§19 permits existing deliberate decision). V1 fixture committed for forward-migration tests.
- **Test framework**: `cargo test` + `ratatui::backend::TestBackend` retained; pseudo-terminal integration test (§29.15) via `expectrl` (or portable-pty); committed fixtures under `tests/fixtures/`.
- **Workspace lints**: target `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` (Stage 0 §46).
- **Migration numbering**: Stage 0 schema becomes V2; refinery embedded migrations grow `V2__stage0_schema.sql`, `V3__stage0_indexes.sql` etc. The "older fixture" required by §24/§29.8 is the V1 M1 schema.
- **Errors**: `autore-core::Error` uses `thiserror` with the §27 categories (Io, Database, Serialization, Validation, NotFound, Conflict, HashMismatch, SchemaMismatch, Migration, InvalidStateTransition, Subscription, Operation, Unsupported). `IDAError` is NOT a core variant (lives in Stage 1 `autore-stage1`).
- **JSON output**: All read CLI commands gain `--output json` producing stable versioned schemas (§22/§26).

## Phase mapping (spec §30)
- 0A: Repository+TUI audit (produce retain/adapt/move/defer/remove report in plan `## Scope`; add regression tests FIRST for current useful TUI behavior before refactor)
- 0B: Core schema primitives in `autore-schema`
- 0C: Project + artifact persistence in `autore-store`
- 0D: Canonical entities + providers in `autore-store` + `autore-core`
- 0E: Evidence/Hypotheses/Contradictions/Verification in `autore-store` + `autore-core`
- 0F: Operations + durable events in `autore-events` + `autore-store` (atomic state+event)
- 0G: Shared application services in `autore-app`
- 0H: CLI completion in `autore-cli`
- 0I: Existing Ratatui adaptation in `autore-tui` (keep 4-panel layout, remap Campaigns→Projects, Campaign Status→Project Summary+counts+validation, Tasks→Operations, Claims Progress→Hypothesis/Evidence progress; durable ProjectEvent subscription + EventCursor; generic fallback inspector)
- 0J: Migrations, validation, derived-state rebuild in `autore-app` + `autore-cli`/`autore-tui`
- 0K: End-to-end fixture + hardening (committed V2 fixture, all §32 completion criteria)

## Status: AWAITING-APPROVAL
Pending action: RUN `scaffold-plan.mjs auto-re-stage-0 --clear`, run mandatory Metis gap review, APPEND todos to `## Todos`, fill `## TL;DR (For humans)` last, then present summary + ask high-accuracy review opt-in. NO implementation by the planner.