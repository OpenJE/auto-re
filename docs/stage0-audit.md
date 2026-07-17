# Stage 0 Audit: M1 Module Classification

**Date:** 2026-07-17
**Spec reference:** Plan §33
**Scope:** Every `*.rs` file under the original single-crate `src/` tree (44 source files + 2 integration tests = 46 total)

---

## Summary

Stage 0 splits the monolithic `auto-re` crate into an 8-crate Cargo workspace (7 shared + 1 deferred). This audit classifies every M1 source file into one of five categories:

| Classification | Meaning | Count |
|---|---|---|
| **RETAIN UNCHANGED** | File moves to its Stage 0 crate with no semantic changes | 4 |
| **RETAIN WITH ADAPTATION** | File moves and requires targeted modifications (ID versioning, type renames, atomic transactions, layout remapping) | 11 |
| **MOVE BEHIND SHARED SERVICES** | File stays in `autore-stage1` but its public types are re-exported through shared crates | 1 |
| **DEFER TO LATER STAGE** | File moves to `autore-stage1` unchanged; not part of Stage 0 shared surface | 22 |
| **REMOVE** | M1 type or test is replaced by a Stage 0+ construct | 8 |

Total: 46 files classified, 0 unclassified.

---

## Decisions and Rationale

### User Decision Q1: Full 7-Crate Workspace + autore-stage1 Deferral

**Decision:** Split into `autore-schema`, `autore-core`, `autore-store`, `autore-events`, `autore-tui`, `autore-app`, `autore-cli` as shared Stage 0 crates, with `autore-stage1` holding all M1 operational code (analysis, model, scheduler, worker, engine, headless CLI) behind a feature gate and excluded from `default-members`.

**Rationale:** Stage 0 delivers the domain model, storage, events, and TUI skeleton without pulling in IDA SDK dependencies, model providers, or the scheduler. The `autore-stage1` crate compiles and tests independently via `cargo build -p autore-stage1` and `cargo test -p autore-stage1`. Default `cargo build` skips it entirely.

### User Decision Q2: UUIDv7

**Decision:** All `define_id!` generated IDs switch from `Uuid::new_v4()` to `Uuid::now_v7()` for time-ordered identifiers.

**Rationale:** UUIDv7 provides monotonic ordering for free, which matters for append-only event logs and database indexing. The `define_id!` macro in `autore-schema/src/ids.rs` currently calls `Uuid::new_v4()`; Stage 0 adaptation swaps this to `Uuid::now_v7()` and adds the `v7` feature to the `uuid` dependency.

### User Decision Q3: Aggressive M1 Replacement

**Decision:** M1 domain types that conflict with the spec's final ontology are marked REMOVE, not RETAIN. Specifically: `Campaign`/`Task`/`Claim` are replaced by `Project`/`Operation`/`Hypothesis`; `Provenance` is replaced by `Derivation` + `DerivationMethod`; M1 `Evidence`/`EvidenceKind` is replaced by append-only `EvidenceRecord` + `EvidenceValue`; the closed `EntityId` enum is replaced by an opaque `EntityId` with namespaced kind strings.

**Rationale:** Keeping M1 names in the shared crates creates migration debt. The spec defines the final ontology; Stage 0 builds the scaffolding for it, even if the M1 types temporarily coexist in `autore-schema` for compilation compatibility.

### Default Crate Boundaries (7 Shared + 1 Deferred)

| Crate | Responsibility | Key Contents |
|---|---|---|
| `autore-schema` | Domain types, typed IDs, worker output schema | `domain/`, `ids.rs`, `worker_output.rs` |
| `autore-core` | Error/Result definitions | `Error` enum, `Result` alias |
| `autore-store` | SQLite storage, migrations, repository traits | `storage/database.rs`, `storage/repositories/` |
| `autore-events` | TUI event types | `Event` enum (crossterm wrapper) |
| `autore-tui` | Terminal UI, runtime orchestration | `tui.rs`, `runtime.rs`, `tui/state.rs` |
| `autore-app` | Re-export facade | Re-exports from schema/core/store/events/tui |
| `autore-cli` | Stage 0 placeholder binary | Prints build instructions |
| `autore-stage1` | Deferred M1 operational code | analysis, model, scheduler, worker, engine, cli, store |

---

## Classification Matrix

| # | M1 Source Path | Stage 0 Destination | Classification |
|---|---|---|---|
| 1 | `src/ids.rs` | `autore-schema/src/ids.rs` | RETAIN WITH ADAPTATION |
| 2 | `src/domain/mod.rs` | `autore-schema/src/domain/mod.rs` | RETAIN WITH ADAPTATION |
| 3 | `src/domain/campaign.rs` | `autore-schema/src/domain/campaign.rs` | REMOVE |
| 4 | `src/domain/claim.rs` | `autore-schema/src/domain/claim.rs` | REMOVE |
| 5 | `src/domain/evidence.rs` | `autore-schema/src/domain/evidence.rs` | REMOVE |
| 6 | `src/domain/function.rs` | `autore-schema/src/domain/function.rs` | RETAIN UNCHANGED |
| 7 | `src/domain/task/mod.rs` | `autore-schema/src/domain/task/mod.rs` | REMOVE |
| 8 | `src/domain/task/kind.rs` | `autore-schema/src/domain/task/kind.rs` | REMOVE |
| 9 | `src/domain/task/types.rs` | `autore-schema/src/domain/task/types.rs` | REMOVE |
| 10 | `src/event.rs` | `autore-events/src/lib.rs` | RETAIN UNCHANGED |
| 11 | `src/lib.rs` | `autore-app/src/lib.rs` + `autore-core/src/lib.rs` + `autore-schema/src/lib.rs` + `autore-store/src/lib.rs` + `autore-tui/src/lib.rs` | RETAIN WITH ADAPTATION |
| 12 | `src/main.rs` | `autore-stage1/src/main.rs` + `autore-cli/src/main.rs` | RETAIN WITH ADAPTATION |
| 13 | `src/storage/database.rs` | `autore-store/src/storage/database.rs` | RETAIN WITH ADAPTATION |
| 14 | `src/storage/mod.rs` | `autore-store/src/storage/mod.rs` | RETAIN UNCHANGED |
| 15 | `src/storage/repositories/mod.rs` | `autore-store/src/storage/repositories/mod.rs` | RETAIN WITH ADAPTATION |
| 16 | `src/storage/repositories/claim.rs` | `autore-store/src/storage/repositories/claim.rs` | RETAIN WITH ADAPTATION |
| 17 | `src/storage/repositories/task.rs` | `autore-store/src/storage/repositories/task.rs` | RETAIN WITH ADAPTATION |
| 18 | `src/tui.rs` | `autore-tui/src/tui.rs` | RETAIN WITH ADAPTATION |
| 19 | `src/tui/state.rs` | `autore-tui/src/tui/state.rs` | RETAIN WITH ADAPTATION |
| 20 | `src/tui/state/home.rs` | `autore-tui/src/tui/state/home.rs` | RETAIN UNCHANGED |
| 21 | `src/runtime.rs` | `autore-tui/src/runtime.rs` | RETAIN WITH ADAPTATION |
| 22 | `src/analysis/mod.rs` | `autore-stage1/src/analysis/mod.rs` | DEFER TO LATER STAGE |
| 23 | `src/analysis/backend.rs` | `autore-stage1/src/analysis/backend.rs` | DEFER TO LATER STAGE |
| 24 | `src/analysis/mock.rs` | `autore-stage1/src/analysis/mock.rs` | DEFER TO LATER STAGE |
| 25 | `src/analysis/packet.rs` | `autore-stage1/src/analysis/packet.rs` | DEFER TO LATER STAGE |
| 26 | `src/cli/mod.rs` | `autore-stage1/src/cli/mod.rs` | DEFER TO LATER STAGE |
| 27 | `src/cli/campaign.rs` | `autore-stage1/src/cli/campaign.rs` | DEFER TO LATER STAGE |
| 28 | `src/cli/headless.rs` | `autore-stage1/src/cli/headless.rs` | DEFER TO LATER STAGE |
| 29 | `src/cli/headless_queries.rs` | `autore-stage1/src/cli/headless_queries.rs` | DEFER TO LATER STAGE |
| 30 | `src/cli/task.rs` | `autore-stage1/src/cli/task.rs` | DEFER TO LATER STAGE |
| 31 | `src/engine.rs` | `autore-stage1/src/engine.rs` | DEFER TO LATER STAGE |
| 32 | `src/engine/graph.rs` | `autore-stage1/src/engine/graph.rs` | DEFER TO LATER STAGE |
| 33 | `src/model/mod.rs` | `autore-stage1/src/model/mod.rs` | DEFER TO LATER STAGE |
| 34 | `src/model/mock.rs` | `autore-stage1/src/model/mock.rs` | DEFER TO LATER STAGE |
| 35 | `src/model/provider.rs` | `autore-stage1/src/model/provider.rs` | DEFER TO LATER STAGE |
| 36 | `src/model/router.rs` | `autore-stage1/src/model/router.rs` | DEFER TO LATER STAGE |
| 37 | `src/scheduler/mod.rs` | `autore-stage1/src/scheduler/mod.rs` | DEFER TO LATER STAGE |
| 38 | `src/scheduler/lease.rs` | `autore-stage1/src/scheduler/lease.rs` | DEFER TO LATER STAGE |
| 39 | `src/scheduler/repos.rs` | `autore-stage1/src/scheduler/repos.rs` | DEFER TO LATER STAGE |
| 40 | `src/scheduler/scheduler.rs` | `autore-stage1/src/scheduler/scheduler.rs` | DEFER TO LATER STAGE |
| 41 | `src/store.rs` | `autore-stage1/src/store.rs` | DEFER TO LATER STAGE |
| 42 | `src/worker/mod.rs` | `autore-stage1/src/worker/mod.rs` | DEFER TO LATER STAGE |
| 43 | `src/worker/output.rs` | `autore-stage1/src/worker/output.rs` | MOVE BEHIND SHARED SERVICES |
| 44 | `src/worker/runner.rs` | `autore-stage1/src/worker/runner.rs` | DEFER TO LATER STAGE |
| 45 | `tests/campaign_smoke.rs` | `autore-stage1/tests/campaign_smoke.rs` | REMOVE |
| 46 | `tests/kill_resume.rs` | `autore-stage1/tests/kill_resume.rs` | REMOVE |

---

## Per-Module Classification

### 1. `src/ids.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-schema/src/ids.rs`

**Current state:** The `define_id!` macro generates newtype IDs over `uuid::Uuid` using `Uuid::new_v4()`. Defines 13 ID types: `ProjectId`, `BinaryId`, `BinaryRevisionId`, `ModuleId`, `FunctionId`, `TaskId`, `ClaimId`, `EvidenceId`, `CampaignId`, `WorkerRunId`, `TransactionId`, `ImplementationTargetId`, `ValidationRunId`.

**Required adaptations:**
- Swap `Uuid::new_v4()` → `Uuid::now_v7()` in the `define_id!` macro's `new()` method.
- Add `v7` feature to the `uuid` dependency in `autore-schema/Cargo.toml`.
- The `EntityId` closed enum (defined in `domain/evidence.rs` but conceptually part of the ID system) is marked REMOVE; the Stage 0 replacement is an opaque `EntityId` struct with a namespaced kind string (e.g., `"function:<uuid>"`).

**Tests:** Existing tests in `ids.rs` (serialize roundtrip, type distinctness, copy, default, from_uuid) remain valid after the v4→v7 swap.

---

### 2. `src/domain/mod.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-schema/src/domain/mod.rs`

**Current state:** Defines `Address`, `AddressSpace`, `ContentHash`, `SymbolName`, `Provenance`, `Confidence`, and re-exports entity sub-modules.

**Required adaptations:**
- `ContentHash(String)`: Add a `HashAlgorithm` field (enum: `Blake3`, `Sha256`) so the hash is self-describing. Default remains BLAKE3; SHA-256 is added per spec.
- `Provenance`: Marked REMOVE. Replaced by `Derivation` (which entity produced this) + `DerivationMethod` (how: human, static analysis, dynamic analysis, agent, imported, etc.). The `Provenance::Agent { worker_run_id }` variant maps to `DerivationMethod::AgentRun(WorkerRunId)`.
- `Confidence`: Retained unchanged (parse-don't-validate f32 in [0.0, 1.0]).
- `Address`, `AddressSpace`, `SymbolName`: Retained unchanged.

---

### 3. `src/domain/campaign.rs` → REMOVE

**Stage 0 destination:** Replaced by `Project` entity (not yet implemented).

**Rationale:** The spec replaces `Campaign` with `Project` as the top-level container. `CampaignState` (Pending/Active/Paused/Complete/Blocked) maps to `ProjectState` with additional variants. The M1 file is retained in `autore-schema` only for compilation compatibility with `autore-stage1`; it will be deleted when `Project` is implemented.

---

### 4. `src/domain/claim.rs` → REMOVE

**Stage 0 destination:** Replaced by `Hypothesis` entity (not yet implemented).

**Rationale:** The spec replaces `Claim` with `Hypothesis`. `ClaimState` transitions (Proposed → UnderReview → Accepted/Rejected → Superseded/Invalidated) map to `HypothesisState`. `ClaimPredicate`, `ClaimValue` are subsumed by the Hypothesis type system. Retained in `autore-schema` for M1 compatibility.

---

### 5. `src/domain/evidence.rs` → REMOVE

**Stage 0 destination:** Replaced by append-only `EvidenceRecord` + `EvidenceValue` (not yet implemented).

**Rationale:** M1 `Evidence` is a mutable entity with an ID, kind, artifact link, entity link, location, and provenance. The spec replaces this with an append-only log: `EvidenceRecord` (immutable, time-ordered) containing `EvidenceValue` (the actual data). `EvidenceKind` (18 variants including Decompilation, Disassembly, CFG, etc.) is subsumed by a more general value type system. `EntityId` (closed enum with 7 variants) is replaced by an opaque ID with namespaced kind. Retained in `autore-schema` for M1 compatibility.

---

### 6. `src/domain/function.rs` → RETAIN UNCHANGED

**Stage 0 destination:** `autore-schema/src/domain/function.rs`

**Current state:** Defines `Function` with fields: `id`, `binary_revision_id`, `module_id`, `entry_address`, `current_name`, `backend_name`, `content_hash`, optional `cfg_hash`, `provenance`. Immutable after construction.

**No adaptations required.** The `Function` entity aligns with the spec's ontology. The `provenance: Provenance` field will need updating when `Provenance` is replaced by `Derivation`, but that is a downstream effect of the `domain/mod.rs` adaptation, not a change to this file's structure.

---

### 7–9. `src/domain/task/{mod.rs, kind.rs, types.rs}` → REMOVE

**Stage 0 destination:** Replaced by `Operation` entity (not yet implemented).

**Rationale:** The spec replaces `Task` with `Operation`. `TaskKind` (AnalyzeFunction, DecompileFunction, etc.), `TaskState` (Pending/Ready/Leased/Running/Complete/Failed/Cancelled), `TaskSubject`, `TaskPriority`, `RequiredCapabilities` all map to Operation equivalents. Retained in `autore-schema` for M1 compatibility.

---

### 10. `src/event.rs` → RETAIN UNCHANGED

**Stage 0 destination:** `autore-events/src/lib.rs`

**Current state:** Defines `Event` enum with 4 variants: `Render`, `KeyDown(KeyCode)`, `MouseClick(MouseEventKind)`, `WidgetStateChanged`. Pure crossterm wrapper with convenience constructors.

**No adaptations required.** This is a leaf type with no domain dependencies.

---

### 11. `src/lib.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** Split across 5 crate roots:
- `autore-app/src/lib.rs` (re-export facade)
- `autore-core/src/lib.rs` (Error/Result)
- `autore-schema/src/lib.rs` (domain + ids + worker_output)
- `autore-store/src/lib.rs` (storage re-exports)
- `autore-tui/src/lib.rs` (runtime + tui)

**Required adaptations:** Each crate root re-exports only its own module surface. `autore-app` provides the unified facade (`pub use autore_core::{Error, Result}`, `pub use autore_schema::{domain, ids}`, etc.). No semantic changes to the types themselves.

---

### 12. `src/main.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** Split into two binaries:
- `autore-stage1/src/main.rs` (real M1 binary: `autore_stage1::cli::run().await`)
- `autore-cli/src/main.rs` (Stage 0 placeholder: prints build instructions)

**Required adaptations:** The real `#[tokio::main] async fn main` moves to `autore-stage1`. `autore-cli` gets a stub `fn main()` that prints a message. This is a structural split, not a semantic change.

---

### 13. `src/storage/database.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-store/src/storage/database.rs`

**Current state:** `Database` wraps `rusqlite::Connection` in a `Mutex`. Opens SQLite files, enables WAL mode, applies `refinery` migrations. Provides `connection()` returning `MutexGuard<Connection>`.

**Required adaptations:**
- Add atomic state + event transactions: a single SQLite transaction that writes both the aggregate state change and the corresponding domain event. Currently, state mutations and event emissions are separate operations.
- No DB-generated IDs: all IDs are generated client-side (already the case with UUID newtypes), but this must be enforced as an invariant (no `AUTOINCREMENT` primary keys, no `DEFAULT` UUID generation in SQL).
- Migration backups: before applying migrations, copy the database file to a `.bak` timestamped sibling. This protects against migration corruption.
- The `refinery::embed_migrations!("../migrations")` path already works from `autore-store` (verified during workspace split).

---

### 14. `src/storage/mod.rs` → RETAIN UNCHANGED

**Stage 0 destination:** `autore-store/src/storage/mod.rs`

**Current state:** Module declarations and re-exports for `database` and `repositories`.

**No adaptations required.**

---

### 15. `src/storage/repositories/mod.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-store/src/storage/repositories/mod.rs`

**Current state:** Defines repository traits (`CampaignRepository`, `BinaryRevisionRepository`, `ModuleRepository`, `FunctionRepository`, `TaskRepository`, `ClaimRepository`, `EvidenceRepository`, `ArtifactRepository`) and re-exports `SqliteClaimRepository`, `SqliteTaskRepository`.

**Required adaptations:**
- Trait method signatures will need updating when `Campaign` → `Project`, `Task` → `Operation`, `Claim` → `Hypothesis`, `Evidence` → `EvidenceRecord`.
- Add event-emitting repository wrappers: each mutation method should also append a domain event to the event log within the same transaction.
- Retained as-is for M1 compatibility.

---

### 16. `src/storage/repositories/claim.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-store/src/storage/repositories/claim.rs`

**Current state:** `SqliteClaimRepository` implementing `ClaimRepository` trait with SQLite CRUD.

**Required adaptations:** Same as repositories/mod.rs. Will be replaced when `Claim` → `Hypothesis`.

---

### 17. `src/storage/repositories/task.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-store/src/storage/repositories/task.rs`

**Current state:** `SqliteTaskRepository` implementing `TaskRepository` trait with SQLite CRUD.

**Required adaptations:** Same as repositories/mod.rs. Will be replaced when `Task` → `Operation`.

---

### 18. `src/tui.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-tui/src/tui.rs`

**Current state:** 426-line TUI dashboard with 4-panel layout (left: campaign list, top-right: campaign status, middle-right: task list, bottom-right: claim summary + gauge). Uses `ratatui::init()`/`ratatui::restore()` lifecycle. Read-only rendering from `DashboardState`.

**Required adaptations:**
- `ratatui::init`/`ratatui::restore` lifecycle: RETAIN UNCHANGED (no adaptation needed for the terminal lifecycle).
- 4-panel layout: RETAIN WITH ADAPTATION. The panel contents will be remapped in Stage 0 iteration 0I (campaign → project, task → operation, claim → hypothesis). The layout structure (left sidebar + right stacked panels) is retained.
- The TUI never mutates state; it receives `DashboardState` via a channel. This read-only property is retained.

---

### 19. `src/tui/state.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-tui/src/tui/state.rs`

**Current state:** `DashboardState` struct with `campaigns: Vec<Campaign>`, `tasks: Vec<Task>`, `claims: Vec<Claim>`, `selected_campaign: usize`. Provides `TuiUpdate` enum for channel messages, formatting helpers for state display.

**Required adaptations:** Field types will change when domain entities are renamed. `DashboardState` will hold `Vec<Project>`, `Vec<Operation>`, `Vec<Hypothesis>`. The `TuiUpdate` channel protocol is retained.

---

### 20. `src/tui/state/home.rs` → RETAIN UNCHANGED

**Stage 0 destination:** `autore-tui/src/tui/state/home.rs`

**Current state:** Empty `Home` struct (placeholder for future home panel state).

**No adaptations required.**

---

### 21. `src/runtime.rs` → RETAIN WITH ADAPTATION

**Stage 0 destination:** `autore-tui/src/runtime.rs`

**Current state:** 276-line runtime orchestrator. Creates a bounded `mpsc` channel for `TuiUpdate` events, spawns scheduler loop and TUI as concurrent tokio tasks, coordinates graceful shutdown. M1 version uses a mock scheduler that simulates campaign progress.

**Required adaptations:**
- The mock scheduler loop will be replaced by the real scheduler integration (from `autore-stage1`).
- The channel-based TUI update protocol is retained.
- The `run()` / `run_with_tick_interval()` API surface is retained.

---

### 22–25. `src/analysis/{mod.rs, backend.rs, mock.rs, packet.rs}` → DEFER TO LATER STAGE

**Stage 0 destination:** `autore-stage1/src/analysis/`

**Rationale:** The analysis subsystem (backend trait, mock implementation, packet types) is M1 operational code. It depends on IDA SDK types (behind the `ida` feature) and is not part of the Stage 0 shared surface. Deferred entirely to `autore-stage1`.

---

### 26–30. `src/cli/{mod.rs, campaign.rs, headless.rs, headless_queries.rs, task.rs}` → DEFER TO LATER STAGE

**Stage 0 destination:** `autore-stage1/src/cli/`

**Rationale:** All CLI dispatch logic, including headless mode and campaign/task subcommands, depends on M1 operational types. The Stage 0 `autore-cli` crate has only a placeholder `main()`. The real CLI lives in `autore-stage1` and is deferred.

---

### 31–32. `src/engine.rs`, `src/engine/graph.rs` → DEFER TO LATER STAGE

**Stage 0 destination:** `autore-stage1/src/engine.rs`, `autore-stage1/src/engine/graph.rs`

**Rationale:** The RE engine module (IDA integration, RETaskGraph) is experimental and behind the `ida` feature gate. Not part of Stage 0.

---

### 33–36. `src/model/{mod.rs, mock.rs, provider.rs, router.rs}` → DEFER TO LATER STAGE

**Stage 0 destination:** `autore-stage1/src/model/`

**Rationale:** Model provider abstraction (LLM routing, mock provider) is M1 operational code. Depends on external model APIs. Deferred to `autore-stage1`.

---

### 37–40. `src/scheduler/{mod.rs, lease.rs, repos.rs, scheduler.rs}` → DEFER TO LATER STAGE

**Stage 0 destination:** `autore-stage1/src/scheduler/`

**Rationale:** The scheduler (lease-based task dispatch, dependency resolution, campaign execution) is the core M1 operational loop. It depends on analysis backends, model providers, and the worker runner. Deferred to `autore-stage1`.

---

### 41. `src/store.rs` → DEFER TO LATER STAGE

**Stage 0 destination:** `autore-stage1/src/store.rs`

**Current state:** Empty file (1 line, blank).

**Rationale:** Placeholder for a higher-level store abstraction that composes repositories. Deferred.

---

### 42. `src/worker/mod.rs` → DEFER TO LATER STAGE

**Stage 0 destination:** `autore-stage1/src/worker/mod.rs`

**Rationale:** Worker module declarations. Deferred with the rest of the worker subsystem.

---

### 43. `src/worker/output.rs` → MOVE BEHIND SHARED SERVICES

**Stage 0 destination:** `autore-stage1/src/worker/output.rs` (re-exports from `autore-schema/src/worker_output.rs`)

**Rationale:** The types `FunctionAnalysisOutput`, `ProposedClaim`, and `ProposedEvidence` were moved from `worker::output` to `autore-schema::worker_output` during the workspace split to prevent a circular dependency (`autore-schema` cannot depend on `autore-stage1`). `autore-stage1::worker::output` re-exports them to preserve the original public path. This is a "move behind shared services" pattern: the types live in the shared schema crate, but the worker module re-exports them for backward compatibility.

---

### 44. `src/worker/runner.rs` → DEFER TO LATER STAGE

**Stage 0 destination:** `autore-stage1/src/worker/runner.rs`

**Rationale:** Worker execution engine (runs analysis backends, collects output, produces claims/evidence). Core M1 operational code. Deferred.

---

### 45. `tests/campaign_smoke.rs` → REMOVE

**Stage 0 destination:** Replaced by Stage 0 integration tests (not yet implemented).

**Rationale:** The M1 campaign smoke test exercises the full stack (mock backends + SQLite + scheduler + worker + TUI channel). In Stage 0, the equivalent guarantee (campaign completion with claims and TUI updates) is tested through `Operation` state transitions + atomic event transactions. The M1 test is retained in `autore-stage1/tests/` for now but is classified REMOVE because it will not be ported to the shared crates.

---

### 46. `tests/kill_resume.rs` → REMOVE

**Stage 0 destination:** Replaced by Stage 0 crash recovery tests (not yet implemented).

**Rationale:** The M1 kill/resume test proves that the campaign engine recovers from ungraceful process death without duplicate accepted claims. In Stage 0, this guarantee is ported via `Operation` state + atomic events (the event log is the source of truth; replaying events after crash produces the same state). The M1 test is retained in `autore-stage1/tests/` for now but is classified REMOVE.

---

## Deferred Capabilities

The following M1 capabilities are deferred to `autore-stage1` and are not part of the Stage 0 shared surface:

| Capability | M1 Source | Stage 0 Status |
|---|---|---|
| Analysis backends (IDA, mock) | `src/analysis/` | Deferred; behind `ida` feature |
| Model providers (LLM routing) | `src/model/` | Deferred; external API deps |
| Scheduler (lease-based dispatch) | `src/scheduler/` | Deferred; core operational loop |
| Worker runner | `src/worker/runner.rs` | Deferred; execution engine |
| RE engine (IDAGraph) | `src/engine/` | Deferred; experimental |
| Headless CLI | `src/cli/headless*.rs` | Deferred; depends on scheduler |
| Campaign/task CLI subcommands | `src/cli/{campaign,task}.rs` | Deferred; depends on domain ops |
| Integration tests | `tests/{campaign_smoke,kill_resume}.rs` | Deferred; to be replaced |

---

## Notes

### Verification

- `git ls-files 'src/**/*.rs'` returns empty: all source moved out of old `src/`.
- `cargo build` (default members) succeeds without `autore-stage1`.
- `cargo build -p autore-stage1` succeeds with M1 code.
- `cargo test` and `cargo test -p autore-stage1` both pass.

### Open Questions

1. **`Provenance` → `Derivation` + `DerivationMethod` mapping:** The M1 `Provenance::Agent { worker_run_id }` variant carries a `WorkerRunId`. The Stage 0 `Derivation` type needs to preserve this linkage. Proposed: `Derivation { produced_by: EntityId, method: DerivationMethod }` where `DerivationMethod::AgentRun(WorkerRunId)`.

2. **`ContentHash` algorithm negotiation:** Adding `HashAlgorithm` to `ContentHash` changes the serialized format. Migration plan: existing BLAKE3-only hashes get `HashAlgorithm::Blake3` on deserialization (default). New hashes can specify the algorithm.

3. **Atomic state + event transactions:** The `database.rs` adaptation requires a `transactional` wrapper that takes a closure receiving both a state writer and an event appender, committing both in a single SQLite transaction. Design TBD in Stage 0 implementation.

4. **`EntityId` opaque replacement:** The closed enum `EntityId` (7 variants) becomes `struct EntityId { kind: String, id: Uuid }`. Display format: `"{kind}:{uuid}"`. Deserialization parses the colon-separated format. Migration: existing serialized `EntityId` values need a one-time conversion script.

### File Count Summary

- Original M1 source files: 44
- Original M1 test files: 2
- Total classified: 46
- RETAIN UNCHANGED: 4 (9%)
- RETAIN WITH ADAPTATION: 11 (24%)
- MOVE BEHIND SHARED SERVICES: 1 (2%)
- DEFER TO LATER STAGE: 22 (48%)
- REMOVE: 8 (17%)
