# Stage 1 Implementation Learnings

## 2026-07-21 Session Start
- Plan: `.omo/plans/auto-re-stage-1.md` (60 todos + F1-F4 final wave).
- First-implementation toolchain: cmkr + CMake + Docker-hosted MSVC 2002 compiler.
- Debug backend: Wine + GDB via IDA debugger bridge; abstract to allow x64dbg later.
- Operator supplies binary path; only content-hash committed to event store.
- All 7 new Stage 1 crates are off `default-members` to keep `cargo build` protoc-free.

## Conventions
- TDD per todo; per-todo happy + failure QA.
- All mutations go through `autore-app` ApplicationCommand/Query variants.
- External providers communicate via gRPC; never link directly into core.
- Generated code is imported as a patch, not written to working tree.
- Append only; never overwrite this file.

## 2026-07-21 Wave 1 Todo 1 (Audit)

### Module Classification Summary
- **RETAIN** (2): `main.rs`, `worker/output.rs`
- **ADAPT** (14): `lib.rs`, `error.rs`, `analysis/packet.rs`, `model/provider.rs`, `model/router.rs`, `model/mock.rs`, `scheduler/mod.rs`, `scheduler/scheduler.rs`, `scheduler/lease.rs`, `worker/mod.rs`, `storage/mod.rs`, `cli/mod.rs`, `cli/campaign.rs`, `cli/task.rs`
- **REPLACE** (5): `analysis/backend.rs`, `analysis/mock.rs`, `scheduler/repos.rs`, `worker/runner.rs`, `cli/headless.rs`
- **REMOVE** (9): `engine.rs`, `engine/graph.rs`, `store.rs`, `analysis/mod.rs`, `model/mod.rs`, `storage/repositories/mod.rs`, `storage/repositories/claim.rs`, `storage/repositories/task.rs`, `cli/headless_queries.rs`

### Key Findings
1. **30 `.rs` files** found under `autore-stage1/src/`, all accounted for in the audit.
2. **`store.rs` is empty** (1 line, blank) — confirms the planned REMOVE.
3. **`worker/output.rs` is a single re-export** — actual types live in `autore-schema::worker_output`.
4. **`engine/graph.rs` is experimental** — 28 lines, behind `#[cfg(feature = "ida")]`, almost empty stub.
5. **`scheduler/scheduler.rs` has extensive tests** (869 lines total, ~400 lines of test code) with an in-memory `MockStore` that implements `TaskRepository` + `SchedulerQueries`. These test patterns are valuable for the new coordinator.
6. **`storage/repositories/task.rs` has concurrent-safety test** — `concurrent_lease_exactly_one_wins` uses `tokio::sync::Barrier` + `BEGIN IMMEDIATE` to prove atomic leasing. Reference for new `LeaseWorkItem` command handler.
7. **`cli/headless.rs` duplicates SQL parsing logic** — `task_from_row` function appears in both `storage/repositories/task.rs` and `cli/headless_queries.rs`.

### Ambiguities
- `analysis/packet.rs` destination crate unclear: `autore-coordinator` vs `autore-provider`.
- `error.rs` umbrella fate: may dissolve or become thin re-export crate.
- `cli/mod.rs` must persist until Wave 11 for testing.

### Deliverable
- `docs/stage1-audit.md` created with full audit table (30 rows).
- No source files modified (verified via `git diff --name-only`).

## 2026-07-21 Wave 1 Todo 2 (App Commands)

### What was done
- Extended `ApplicationCommand` enum with 29 Stage 1 command variants and `ApplicationQuery` with 16 Stage 1 query variants.
- Added 58 command request/response structs and 32 query request/response structs in `requests.rs`.
- All new structs derive `Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize` (Stage 0 structs only have `Serialize`).
- Added stub handler arms in `application_service.rs` returning `Err(Error::Validation("not yet implemented: ..."))`.
- Roundtrip test `application_command_stage1_variants_roundtrip` covers 9 work-item lifecycle variants.

### Decisions
- **Test scope**: Roundtrip test exercises individual request structs (not `ApplicationCommand` enum wrapping them), because the Stage 0 enum only derives `Serialize` and adding `Deserialize` would require modifying all Stage 0 request structs. This is a clean boundary — Stage 0 types are untouched.
- **Placeholder IDs**: Used `String` for Stage 1 IDs (work_item_id, campaign_id, etc.) since typed IDs don't exist in `autore-schema` yet. Future todos will introduce typed IDs when domain records are created.
- **Section organization**: Followed existing file pattern of section-separator comments for Stage 1 groupings.

### Patterns established
- All Stage 1 request/response structs get both `Serialize` + `Deserialize` (forward-looking for future persistence/wire use).
- Stub arms use uniform `Err(Error::Validation(format!("not yet implemented: {variant_name}")))` pattern.
- `LocalAutoReClient` left completely untouched — it routes through `ApplicationService` which handles the new variants via the stub arms.

### Verification
- `cargo build -p autore-app`: clean
- `cargo test -p autore-app -- --nocapture`: 29/29 passed (including new roundtrip test)
- `cargo clippy -p autore-app --all-targets -- -D warnings`: clean
- Full workspace: 614 tests passed, 0 failed

## 2026-07-21 Wave 1 Todo 3 (Schema Records)

### What was done
- Added 15 new typed IDs to `autore-schema/src/ids.rs` via the existing `define_id!` macro (total now 43, up from 28).
- Added 21 new Stage 1 domain records to `autore-schema/src/domain/records.rs` in a clearly-marked Stage 1 section.
- Added `WorkItemKind` enum with all 18 variants (§7.2) and `as_namespaced_kind()` returning `recon.work.*` strings.
- Added `WorkItemState` enum mirroring the existing `Task` state machine with identical transition rules.
- Added `ReconstructionCampaignState` enum (renamed from initial `CampaignState` to avoid clash with existing `domain::campaign::CampaignState`).
- Added `DependencyKind`, `DiagnosticSeverity`, `RepairTarget` enums.
- Added 31 namespaced-kind `LazyLock<NamespacedId>` constants (`WORK_ITEM_KIND_*`, `RECON_KIND_*`, `PROVIDER_KIND_*`, `DEBUG_KIND_*`, `LLM_KIND_*`, `BUILD_KIND_*`, `VERIFY_KIND_*`, `RECON_KIND_MAPPING`).
- `ReconstructionWorkItem` composes the existing `Task` type by value without modifying `Task`; includes `state()` method that derives `WorkItemState` from `Task::state`.
- Added 28 fixture/roundtrip tests in the existing `records::tests` module covering every new record plus state transitions and kind-constant registrations.
- Re-exported all new types from `domain/mod.rs` and all new IDs from `lib.rs`.

### Decisions
- **Single-file placement**: All Stage 1 records added to the existing `records.rs` file (following Stage 0's single-monolith pattern) rather than a separate module, with a clear `// Stage 1 Records — §7 persistence list` boundary marker.
- **Renamed `CampaignState`** → `ReconstructionCampaignState` because `CampaignState` already exists in `domain::campaign` for M1 Stage 0 campaigns and is re-exported at `domain::` scope. The Stage 1 campaign has different variants (Planning/Active/Paused/Completed/Failed vs Pending/Active/Paused/Complete/Blocked).
- **`ReconstructionWorkItem.state()`** derives `WorkItemState` from the composed `Task::state` rather than maintaining a duplicate field — single source of truth.
- **`RepairTarget` enum** uses a tagged enum with 3 variants (BuildAttempt, VerificationComparison, ConflictRecord) following the existing `VerificationSubject` pattern.
- **`WorkItemKind::as_namespaced_kind()`** returns `NamespacedId::parse("recon.work.<snake>").unwrap()` — safe because the string is a literal.

### Patterns established
- Stage 1 constructors use `#[allow(clippy::too_many_arguments)]` only where Stage 0 already uses it (e.g., `Task::new`).
- New types added to the `records::tests` `use crate::ids::{...}` block to avoid polluting the top-level `use` list.
- Every new record gets a `*_fixture` roundtrip test that both exercises serde and verifies constructor defaults.

### Verification
- `cargo build -p autore-schema`: clean
- `cargo test -p autore-schema -- --nocapture`: 276/276 passed (28 new Stage 1 tests + 248 existing)
- `cargo clippy -p autore-schema --all-targets -- -D warnings`: clean
- `grep -c '^define_id!' autore-schema/src/ids.rs`: 43 (15 new + 28 pre-existing)
- `autore-schema/src/domain/task/` fully untouched (verified via `git diff --stat HEAD -- autore-schema/src/domain/task/`)

## 2026-07-21 Wave 1 Todo 4 (Worker via ApplicationCommand)

### What was done
- Refactored `WorkerRunner` to route all durable writes through `Arc<dyn AutoReClient>` instead of direct `ClaimRepository`/`EvidenceRepository` calls.
- `WorkerRunner` struct now holds `client: Arc<dyn AutoReClient>` (replacing `claims`/`evidence` fields).
- `WorkerInput` gained a `project_id: ProjectId` field for command construction.
- `issue_commands()` helper issues `AddEvidence` (per evidence item), `AddHypothesis` (with supporting evidence IDs and confidence from analysis), and `CompleteWorkItem`.
- `WorkerOutput` struct preserved unchanged (claims, evidence, analysis) for in-memory return.
- Added `RecordingClient` test stub that records all `ApplicationCommand`s and asserts exactly 1 `AddEvidence` + 1 `AddHypothesis` + 1 `CompleteWorkItem` in order.
- Added `autore-app` as a path dependency in `autore-stage1/Cargo.toml`.
- `ClaimRepository` and `EvidenceRepository` traits NOT deleted (still in `storage/repositories/` for other consumers; removal in Wave 11).

### Decisions
- **`tasks` field kept**: `WorkerRunner` still holds `Arc<dyn TaskRepository>` for internal stage1 task lifecycle (`complete`/`fail`). The app-level `CompleteWorkItem` is a separate concern (stage0 work-item state). Task repo removal deferred to Wave 4/11 when the scheduler moves to commands.
- **EntityId type mismatch**: Stage1 domain `EntityId` (enum: Function, Module, etc.) vs Stage0 `ids::EntityId` (UUID wrapper) — used `EntityId::new()` placeholder for command requests. Proper mapping layer deferred to a future todo when the domain bridge is designed.
- **Predicate mapping**: `ClaimPredicate` → string via match function (e.g., `FunctionName` → `"function-name"`). Used as the hypothesis predicate string.
- **ClaimValue → EvidenceValue**: Simple conversion function mapping each variant to the closest `EvidenceValue` variant. Complex/map/json values serialized to string.
- **`headless.rs` minimal fix**: Added `NoopAutoReClient` stub (classified REPLACE in Wave 11). Also fixed `campaign_smoke.rs` with `SmokeAutoReClient`.
- **Float precision**: `Confidence::score()` returns `f32`, cast to `f64` for `AddHypothesisRequest.confidence_score`. Test uses approximate comparison (`abs() < 0.001`).

### Patterns established
- Command routing via `AutoReClient::execute(ApplicationCommand::*)` from worker subsystem.
- `RecordingClient` pattern for testing command issuance: records commands in `Mutex<Vec<ApplicationCommand>>`, returns plausible `CommandResult` stubs.
- Evidence-first ordering: `AddEvidence` commands issued before `AddHypothesis` so the hypothesis can reference evidence record IDs.

### Verification
- `cargo build -p autore-stage1 --no-default-features`: clean (zero warnings)
- `cargo test -p autore-stage1 --no-default-features --lib worker::runner`: 5/5 passed
- `cargo build` (workspace): clean
- `grep -n 'ClaimRepository' runner.rs`: nothing
- `grep -n 'EvidenceRepository' runner.rs`: nothing

