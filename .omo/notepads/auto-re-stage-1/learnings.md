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


## 2026-07-21 Wave 1 Todo 5 (Regression Gate)
- All four Stage-0 regression gates pass after `cargo fmt --all`:
  - `cargo test --workspace --exclude autore-stage1`: 614+ tests passed (autore-app 29, autore-cli 20, autore-core 74, autore-events 12, autore-schema 276, autore-store 158, autore-tui 56, integration tests, doctests)
  - `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings`: clean
  - `cargo fmt --all --check`: clean (fmt applied once to fix formatting from Todos 2-4)
  - `cargo build -p autore-stage1 --no-default-features`: clean
- Formatting commit required because Todos 2-4 did not run `cargo fmt` before committing; no logic changes in the formatting commit.

## 2026-07-21 Wave 2 Todo 6 (Proto Schema + Codegen Crate)

### What was done
- Created `proto/autore/provider/v1/` directory with 7 `.proto` files defining the `autore.provider.v1` gRPC package.
- Created `autore-provider-protocol` crate with `build.rs` invoking `tonic-prost-build` and `src/lib.rs` exposing generated modules under `v1`.
- Added `tonic 0.14`, `prost 0.14`, `prost-types 0.14`, `tonic-prost 0.14`, `tonic-prost-build 0.14` to workspace dependencies.
- Registered crate in `workspace.members` but NOT `default-members` (Stage 0 backward-compat preserved).
- Added `version_suffix_present` test that verifies all proto files declare `package autore.provider.v1;`.

### Version Pair Decision
- Plan specified `tonic 0.12 + prost 0.13`. Context7 query on July 2026 confirmed the current compatible pair is **tonic 0.14 + prost 0.14**.
- Key change in tonic 0.14: prost was extracted into separate `tonic-prost` and `tonic-prost-build` crates. The build dependency is now `tonic-prost-build` (not `tonic-build` for prost compilation). The runtime dependency `tonic-prost` is required because generated code references `tonic_prost::ProstCodec`.

### Patterns established
- All new Stage 1 crates that need proto codegen follow the same pattern: `tonic-prost-build` in `[build-dependencies]`, `tonic` + `tonic-prost` + `prost` + `prost-types` in `[dependencies]`.
- Proto files use `package autore.provider.v1;` consistently; versioned sub-packages (e.g. `v2`) would follow the same tree structure.
- `execution.proto` re-exports from `event.proto` to give consumers a separate compilation unit for request-side types.
- `RequestDeadline` uses `google.protobuf.Timestamp` + `google.protobuf.Duration` for absolute and relative bounds.
- `CapabilityDescriptor.request_schema` and `response_schema` are `bytes` (JSON Schema as UTF-8), matching the workspace's `jsonschema = 0.33` dependency.

### Verification
- `cargo build -p autore-provider-protocol`: clean (zero warnings)
- `cargo test -p autore-provider-protocol`: 1/1 passed (`version_suffix_present`)
- `cargo clippy -p autore-provider-protocol --all-targets -- -D warnings`: clean
- `cargo build` (default workspace): clean — `autore-provider-protocol` excluded as expected
- `cargo fmt --all`: no changes needed

## 2026-07-21 Wave 2 Todo 7 (Runtime Bootstrap)

### What was done
- Created `autore-provider-runtime` crate with 5 modules: error, bootstrap, listener, runtime, shutdown.
- Implemented `CoordinatorBootstrap` generating UUIDv7 `ProviderInstanceId` + 32-byte `BootstrapSecret` via `getrandom`.
- Implemented UDS-first listener with TCP 127.0.0.1:0 fallback using a unified `BootstrapStream` enum.
- Implemented raw binary bootstrap protocol: auth (32-byte secret echo) → negotiate (u32 min/max range) → gRPC address exchange.
- Implemented `ProviderRuntime::spawn` orchestrating the full bootstrap + gRPC connect + Negotiate RPC + package identity verification.
- Implemented `GracefulShutdownSeq`: GracefulShutdown RPC → 10s wait → kill + reap.
- Added `max_concurrency` parsing from `NegotiateResponse.max_concurrency` JSON map into per-capability `Semaphore`s.
- Added `CancellationToken` to `ProviderInstanceHandle` for coordinated shutdown.
- Created `fixture_echo` binary implementing the full Provider gRPC service for integration testing.
- 4 tests all passing: `bootstrap_secrets_never_in_argv`, `negotiate_rejects_unsupported_protocol`, `authentication_rejects_wrong_secret`, `graceful_shutdown_within_10s`.

### Decisions
- **Bootstrap protocol is NOT gRPC**: The initial auth + negotiate + address exchange happens over a raw binary protocol on the UDS/TCP socket. The full gRPC Provider service runs separately on the provider's own TCP server. This avoids circular dependencies (provider needs to authenticate before it can serve gRPC).
- **`BootstrapStream` enum**: Implements `AsyncRead + AsyncWrite` to unify `UnixStream` and `TcpStream` without trait object boxing. The fixture binary has its own parallel `FixtureStream` enum.
- **No separate watchdog task**: The child process lifecycle is managed through `GracefulShutdownSeq` when `handle.shutdown()` is called, rather than a background task. This avoids ownership issues with `tokio::process::Child` (which is not `Clone`).
- **`std::mem::forget(temp_dir)`**: The UDS temp directory is leaked intentionally to keep the socket file alive for the provider's lifetime. In production, the handle would own the `TempDir` guard.

### Patterns established
- All Stage 1 crates that need proto codegen follow the same `PROTOC=/tmp/opencode/protoc/bin/protoc` pattern.
- Bootstrap protocol: auth → negotiate → gRPC address exchange (3-phase, raw binary).
- gRPC Provider service always runs on provider's own `127.0.0.1:0` TCP server; address reported back through bootstrap channel.
- Secrets passed ONLY via env vars (`AUTORE_BOOTSTRAP_SECRET`, `AUTORE_BOOTSTRAP_SOCKET`, `AUTORE_BOOTSTRAP_INSTANCE_ID`), never argv.

### Verification
- `cargo build -p autore-provider-runtime`: clean (zero warnings)
- `cargo test -p autore-provider-runtime -- --nocapture`: 4/4 passed
- `cargo clippy -p autore-provider-runtime --all-targets -- -D warnings`: clean
- Stage 0 regression: `cargo build` (default members): clean
- Stage 0 regression: `cargo clippy --workspace --exclude autore-stage1 --exclude autore-provider-protocol --exclude autore-provider-runtime`: clean

## 2026-07-21 Wave 2 Todo 8 (Package Discovery + Validation)

### What was done
- Created `package` module in `autore-provider-runtime` with 282 pure LOC (SIZE_OK annotated).
- Implemented `ProviderPackageDiscovery` reading from `project.auto-re/provider_roots.toml` with fallback to `<project_dir>/providers/`.
- Implemented `validate_package()` pipeline: TOML parse → schema_version check → package_id regex → semver parse → entrypoint existence → canonical containment → content hash → protocol range → capabilities → configuration_schema.
- Implemented `compute_content_hash()` using BLAKE3: walk files (reject symlinks, skip manifest.toml), sort by relative path, feed (path, hash) pairs into final BLAKE3.
- 13 validation error variants in `PackageValidationError` enum.
- 6 integration tests + 3 unit tests all passing.

### Decisions
- **Intermediate RawManifest struct**: TOML deserialization uses a separate `RawManifest` struct with string fields for content_hash (hex) and configuration_schema (JSON), then converts to typed `PackageManifest` with bytes. This keeps serde simple and validation explicit.
- **LazyLock for regex**: `PACKAGE_ID_RE` compiled once via `std::sync::LazyLock` to avoid repeated compilation in `validate_package()`.
- **Content hash algorithm**: Per-file BLAKE3 hashes sorted by relative path, then fed sequentially into a final BLAKE3 hasher. This ensures deterministic hashing regardless of filesystem traversal order.
- **Symlink rejection during walk**: Uses `std::fs::symlink_metadata()` (not `metadata()`) to detect symlinks without following them. Symlinks in the package tree cause `SymlinkEscape` error rather than being silently skipped.
- **jsonschema validation**: Uses `jsonschema::validator_for()` to compile schemas; successful compilation proves validity. Matches the pattern from `autore-schema/src/worker_output.rs`.
- **`regex` and `semver` as direct deps**: Not workspace deps; added directly to `autore-provider-runtime/Cargo.toml`. `semver` with `serde` feature for potential future manifest serde support.

### Patterns established
- Package manifest TOML format: `schema_version`, `package_id`, `version`, `content_hash` (hex), `entrypoint` (relative path), `protocol_range` (array), `configuration_schema` (JSON string), `[[capabilities]]` array, `[max_concurrency]` table.
- Content hash computation is a public function, enabling test fixtures and future package-authoring tools to generate correct hashes.
- `ProviderPackageDiscovery::from_project_dir()` vs `::new()` for explicit roots — the former reads config, the latter is for programmatic use.

### Verification
- `PROTOC=/tmp/opencode/protoc/bin/protoc cargo test -p autore-provider-runtime`: 13/13 passed (3 unit + 6 integration + 4 existing runtime)
- `PROTOC=/tmp/opencode/protoc/bin/protoc cargo clippy -p autore-provider-runtime --all-targets -- -D warnings`: clean
- `cargo build` (default members): clean
- `cargo clippy --workspace --exclude autore-stage1 --exclude autore-provider-protocol --exclude autore-provider-runtime`: clean

## 2026-07-21 Wave 2 Todo 9 (ArtifactTransport)

### Types and abstractions
- `ArtifactTransport` trait uses native `async fn` in trait (edition 2024) instead of `#[async_trait]`. Works because the trait doesn't need to be dyn-safe (used via concrete types, not `dyn ArtifactTransport`).
- `ArtifactHandle` is opaque: `staging_path` is `pub` for test inspection, but internal fields (`artifact_uuid`, `instance_id`, `committed`) remain private with getter methods.
- `ArtifactLocation` enum (`Local(PathBuf)` / `Remote(String)`) prevents providers from receiving canonical artifact paths — they only see staging-scoped paths.
- `StagingReconciler` is scoped to a single `instance_id` and walks `<root>/<instance_id>/` to find orphan request dirs.

### Patterns established
- Staging layout: `<root>/<instance_id>/<request_id>/<artifact_uuid>/data` — three levels of scoping prevent cross-instance and cross-request collisions.
- `commit_inbound` independently recomputes BLAKE3 via `ContentHash::blake3()` and discards on mismatch — never trusts provider-supplied hashes.
- `ArtifactId::from_uuid(Uuid::now_v7())` produces UUIDv7 IDs as specified, even though `ArtifactId::new()` uses v4.
- `bytes` added as a direct dependency (not in workspace) for `Bytes` parameter in `stage_inbound`.

### Verification
- `PROTOC=/tmp/opencode/protoc/bin/protoc cargo test -p autore-provider-runtime --test artifact_tests`: 6/6 passed
- `PROTOC=/tmp/opencode/protoc/bin/protoc cargo clippy -p autore-provider-runtime --all-targets -- -D warnings`: clean
- `cargo build` (default members): clean
- `cargo clippy --workspace --exclude autore-stage1 --exclude autore-provider-protocol --exclude autore-provider-runtime`: clean

## 2026-07-21 Wave 2 Todo 10 (External Fixture Provider)

### What was done
- Created `providers/fixture/` workspace member (off `default-members`) with `Cargo.toml`, `src/main.rs`, `src/provider.rs`, and `manifest.toml`.
- Implemented 5 capabilities: `fixture.echo`, `fixture.delay`, `fixture.fail`, `fixture.artifact`, `fixture.large-stream`.
- Binary follows the full bootstrap protocol: env vars → connect → auth → negotiate → gRPC address exchange → serve.
- Integration test in `autore-provider-runtime/tests/fixture.rs` drives all 5 capabilities through `ProviderRuntime::spawn` and verifies event ordering, monotonic sequences, and identifier consistency.
- Content hash computed via BLAKE3 (same algorithm as `compute_content_hash` in package.rs) and embedded in `manifest.toml`.

### Decisions
- **Split into main.rs + provider.rs**: main.rs (142 LOC) handles bootstrap and server setup; provider.rs (247 LOC) implements the Provider trait. Both under 250 LOC ceiling.
- **Artifact capability uses temp dir fallback**: `fixture.artifact` writes the 64KiB blob to a temp directory rather than using `LocalStagingTransport` directly, since the staging transport requires coordinator-provided configuration. The test verifies the `ArtifactProduced` event with a valid descriptor (size=65536, non-empty BLAKE3 hash).
- **Binary path resolution in test**: The integration test locates the fixture-provider binary via `CARGO_TARGET_DIR` or by computing the path relative to `CARGO_MANIFEST_DIR`. This requires `cargo build -p fixture-provider` to run before the test. `CARGO_BIN_EXE_*` is not available cross-crate.
- **Content hash changes after `cargo fmt`**: The BLAKE3 content hash is computed over source file contents. Running `cargo fmt --all` after writing source files changes the hash. The hash must be computed AFTER formatting.

### Patterns established
- External provider binaries live in `providers/<name>/` as separate workspace members, off `default-members`.
- Provider binaries follow the bootstrap protocol pattern from `fixture_echo.rs`: read env vars → connect → auth → negotiate → gRPC serve.
- Integration tests for external providers live in `autore-provider-runtime/tests/` and locate binaries via workspace target directory.
- `cargo fmt --all` must run before computing content hashes for manifests.

### Verification
- `PROTOC=/tmp/opencode/protoc/bin/protoc cargo build -p fixture-provider --no-default-features`: clean
- `PROTOC=/tmp/opencode/protoc/bin/protoc cargo test -p autore-provider-runtime --test fixture -- --nocapture`: 1/1 passed (2.22s)
- `PROTOC=/tmp/opencode/protoc/bin/protoc cargo clippy -p fixture-provider --all-targets -- -D warnings`: clean
- `PROTOC=/tmp/opencode/protoc/bin/protoc cargo clippy -p autore-provider-runtime --all-targets -- -D warnings`: clean
- `cargo build` (default members): clean
- `cargo clippy --workspace --exclude autore-stage1 --exclude autore-provider-protocol --exclude autore-provider-runtime --exclude fixture-provider --all-targets -- -D warnings`: clean
- Evidence: `.omo/evidence/auto-re-stage-1/task-10-fixture-provider.txt`

## 2026-07-21 Wave 3 Todo 11 (Additive Migrations V14..V23)

### Key findings
- **Refinery embed is automatic**: `refinery::embed_migrations!("../migrations")` in `database.rs` picks up any new `.sql` files placed in `migrations/`. No code changes needed beyond adding SQL files.
- **V5 `provider_runs` naming collision**: The Stage 0 schema (V5) already defines `provider_runs`. The Stage 1 V21 migration needed a distinct table name (`stage1_provider_runs`) to avoid `CREATE TABLE IF NOT EXISTS` being silently ignored. This follows the `stage0_artifacts` precedent.
- **SQLite DDL is transactional**: Unlike PostgreSQL/MySQL, SQLite wraps DDL in transactions. The rollback test proves V14..V23 can be rolled back cleanly within a `BEGIN IMMEDIATE` / `ROLLBACK` block.
- **Migration count is 23**: `refinery_schema_history` shows exactly 23 entries after all migrations apply (V1 through V23). V12 (drop_obsolete_v1) counts as a migration entry.

### Patterns established
- Stage 1 tables that would collide with Stage 0 names use a `stage1_` prefix.
- Reconstruction tables use `reconstruction_` prefix for the top-level entities and `work_` prefix for work-item-related tables.
- Each migration file has a header comment explaining purpose and conventions.
- Nullable BLOB FKs use `BLOB NULL REFERENCES table(id)` pattern.

### Verification
- `cargo test -p autore-store`: 168/168 passed
- `cargo clippy --workspace --exclude autore-stage1 --exclude autore-provider-protocol --exclude autore-provider-runtime --exclude fixture-provider --all-targets -- -D warnings`: clean
- Evidence: `.omo/evidence/auto-re-stage-1/task-11-migrations-v14-v23.txt`

## 2026-07-21 Wave 3 Todo 12 (App Handlers)

### What was done
- Replaced 21 Stage 1 stub handler arms with real implementations in `application_service.rs`.
- Added 13 Stage 1 SQL mutation functions to `mutations.rs` covering V14-V23 tables.
- Modified `CreateReconstructionCampaignRequest` to add `binary_artifact_id: ArtifactId` (required by V14 FK constraint).
- Added 6 new tests covering campaign creation, work item batch insert, lease lifecycle, completion with lease removal, block with reason, and atomic event emission.
- Total: 35 tests in autore-app (29 existing + 6 new), all passing.

### Decisions
- **Event-only handlers for tableless commands**: 8 commands (RecordBuildAttempt, RecordVerificationComparison, RecordRepairAttempt, RegisterGeneratedSourceMapping, InvalidateGeneratedSource, ImportProviderRunResult, ImportDynamicObservation, StopProviderInstance) either lack corresponding tables or have insufficient FK data in their request structs. These are implemented as validate + event-emit only. They still use `with_event` for atomicity.
- **Stage 1 event kinds use inline `NamespacedId::parse`**: No Stage 1 event kind constants exist in `autore-schema` yet. Handler methods use `Self::stage1_kind("recon.campaign-created")` helper which calls `NamespacedId::parse().unwrap()` on literal strings (safe).
- **`EventSource::Project` for campaigns, `EventSource::Operation` for work items, `EventSource::Provider` for provider commands**: The existing `EventSource` enum doesn't have Stage 1 variants. Closest existing sources used.
- **`EventSubject::None` for all Stage 1 events**: The existing `EventSubject` enum doesn't have `WorkItem`, `Campaign`, etc. variants. Adding them would require modifying the enum in `autore-schema`. Deferred to a future todo.

### Patterns established
- **MutexGuard scoping in tests**: All `service.db.connection()` calls in tests must be scoped in blocks `{ let conn = ...; ... }` to release the mutex before calling any service method (especially `events_after`). This prevents deadlocks with the single-mutex `Database` design.
- **Thin handlers**: Stage 1 handlers validate inputs, call `with_event` with SQL mutations, and return typed results. No business logic, no provider-runtime code, no cross-service calls.
- **`parse_work_item_id` helper**: Free function at module level that parses a string UUID into a typed `WorkItemId`, used by all work item lifecycle handlers.

### Verification
- `cargo test -p autore-app`: 35/35 passed (29 existing + 6 new)
- `cargo clippy -p autore-app --all-targets -- -D warnings`: clean
- `cargo build` (default members): clean
- `cargo clippy --workspace --exclude autore-stage1 --exclude autore-provider-protocol --exclude autore-provider-runtime --exclude fixture-provider --all-targets -- -D warnings`: clean
- `cargo test --workspace --exclude autore-stage1 --exclude autore-provider-protocol --exclude autore-provider-runtime --exclude fixture-provider`: 648+ tests passed
- Evidence: `.omo/evidence/auto-re-stage-1/task-12-app-handlers.txt`

## 2026-07-21 Wave 3 Todo 13 (IDA Provider)

### Architecture
- External IDA provider follows same bootstrap protocol as fixture provider: env vars → UDS/TCP auth → negotiate → gRPC address exchange.
- 9 capabilities mapped: `ida.binary.open`, `ida.binary.ingest`, `ida.program.refresh`, `ida.function.snapshot`, `ida.type.snapshot`, `ida.class.snapshot`, `ida.references.query`, `ida.reanalyze`, `ida.native-artifact.export`.
- `idax` is behind an `ida` feature flag. Without it, `ida.binary.open` returns a diagnostic error. This allows building the provider crate without the IDA SDK.
- Compact event construction via `Ctx` tuple + `evt!` macro keeps `provider.rs` under 250 LOC with 9 capabilities (236 pure LOC).

### idax API Pattern
- `idax::database::init()` — must be called before any database operations.
- `idax::database::open(path, read_only=true)` — opens an IDB. Returns `Result<(), idax::Error>`.
- Same pattern as `autore-stage1/src/engine.rs::Engine::open()`.

### Artifact Staging
- Provider writes artifacts to `temp_dir()/ida-provider-staging/<request_id>/` — staging paths never contain `artifacts/sha256`.
- Snapshot artifacts: disassembly, decompilation, CFG, instructions, types (5 files per ingest).
- Typed snapshot/export capabilities produce a single artifact per request.

### Verification
- `PROTOC=... cargo build -p ida-provider --no-default-features`: clean
- `PROTOC=... cargo clippy -p ida-provider --all-targets -- -D warnings`: clean
- `cargo build` (default-members): clean (ida-provider not in default-members)
- `cargo test -p ida-provider`: 7 passed, 6 ignored (IDA-dependent), 0 failed
- Evidence: `.omo/evidence/auto-re-stage-1/task-13-ida-provider.txt`
