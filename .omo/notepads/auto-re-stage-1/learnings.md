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
