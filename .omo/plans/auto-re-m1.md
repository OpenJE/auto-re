# auto-re-m1 - Work Plan

## TL;DR (For humans)

**What you'll get:** A self-contained Rust campaign engine with a real-time TUI that shows campaign status, task progress, claims, and evidence as a synthetic 10-function "binary" is analyzed by deterministic mock backends. Everything is persisted in SQLite with atomic task leasing, the process survives a kill and resumes without duplicating accepted claims, and basic CLI status commands remain available for headless use.

**Why this approach:** The spec (§39) says Milestone 1 must first prove the Rust state engine is durable and correct *without* requiring IDA, GDB, or llama.cpp. The user also wants a TUI so people can see the process run. So this plan feature-gates the optional backends behind default-off Cargo features, builds the domain/scheduler/storage/worker core with in-memory mocks, and adds a TUI that reads campaign state from the same storage — no IDA required for the default TUI experience. The IDA adapter (via `idax`) is available when the `ida` feature is enabled.

**What it will NOT do:**
- No real IDA/GDB/llama.cpp integration in the default build (feature-gated; those are Milestones 2–3 and 7).
- No actual C++ code generation or behavioral validation (Milestones 8–10).
- No network model provider, no multi-backend campaign, no debugger, no transaction writeback to IDA.
- No BLAKE3 artifact store in M1; artifacts are metadata-only in SQLite (§16 deferred).
- No claim dependency cycle detection or full DAG topology; linear dependency recording only.

**Effort:** Large
**Risk:** Medium - the kill→resume proof and atomic SQLite leasing are the load-bearing correctness claims; the TUI adds a real-time surface but does not own authoritative state.
**Decisions to sanity-check:**
1. TUI is a first-class default surface; CLI remains for headless use.
2. `idax` (not `idalib`) is the IDA adapter when the `ida` feature is enabled.
3. TUI is decoupled from IDA and works with mock backends.
4. `rusqlite(bundled)` + `refinery` for SQLite migrations.

Your next move: execution continues after this plan update. Full execution detail follows below.

---

> TL;DR (machine): Large effort, medium risk; deliver a feature-gated, mock-backend Rust campaign engine with real-time TUI, SQLite leasing, deterministic scheduler/worker, CLI, and a kill→resume proof.

## Scope
### Must have
1. Feature-aware `build.rs` and `Cargo.toml` so `cargo build` succeeds without IDA/GDB/llama.cpp installed.
2. Correct `Error` enum (`thiserror`-derived) and `Result<T>` alias; `idax` error behind the `ida` feature.
3. TUI as a default surface: ratatui + crossterm + tokio event loop, displaying campaign status, task list, claim summary, and progress. Works without IDA.
4. Typed ID macro (`define_id!`) for all spec §8 IDs.
5. Domain primitives: `Address`, `AddressSpace`, `ContentHash`, `Provenance`, `Confidence` (0.0–1.0), `SymbolName`.
6. Domain entities from §42: `Function`, `Campaign`, `Task` + `TaskState`/`TaskKind`, `Claim` + `ClaimState`/`ClaimPredicate`, `Evidence` + `EvidenceKind`.
7. `AnalysisBackend` trait with `AnalysisCapability` enum and a deterministic `MockAnalysisBackend` that returns a 10-function fixture.
8. `ModelProvider` trait with `ModelDescriptor`/`ModelCapabilities`/`ModelClass` and a deterministic `MockModelProvider` that returns schema-valid JSON.
9. Worker packet builder (`FunctionAnalysisPacket`) and schema-validated worker output (`FunctionAnalysisOutput`).
10. SQLite setup via `rusqlite(bundled)`, `refinery` migrations under `migrations/`, repository traits, and a SQLite `TaskRepository`.
11. Atomic task leasing with `TaskLease`, lease expiry, and deterministic recovery of expired leases.
12. Deterministic scheduler with priority factors, model routing, campaign loop, task dispatch, and stale-work invalidation.
13. Worker runner with cancellation token, timeout, schema validation, and claim/evidence creation.
14. clap CLI: `auto-re campaign status [id]`, `auto-re task list`, `auto-re task status <id>`, plus a TUI mode.
15. End-to-end kill→resume test proving no duplicate accepted claims after restart.
16. §43 required tests for IDs, task/claim transitions, confidence rejection, priority calculation, capability matching, schema validation, leasing contention, lease expiry, completion idempotency, migration application, scheduler retry/escalation, worker timeout/cancellation, verification independence, and TUI rendering without IDA.

### Must NOT have (guardrails, anti-slop, scope boundaries)
- Do NOT write real IDA database mutations (Milestone 6).
- Do NOT make the TUI control the scheduler or authoritative state; it only observes and displays.
- Do NOT integrate a real LLM inference engine (Milestone 2).
- Do NOT integrate a real debugger or `gdbstub` target (Milestone 7).
- Do NOT generate C++/Rust/C source code (Milestones 8–10).
- Do NOT split into a Cargo workspace; keep one package.
- Do NOT add network clients/servers or distributed workers.
- Do NOT implement full BLAKE3 artifact storage; artifact metadata only in M1.
- Do NOT implement claim dependency cycle detection or full DAG topology; linear dependency recording only.
- Domain layer must not import `idax`, `gdbstub`, `llama_cpp`, `rusqlite`, `tokio`, `reqwest`, `std::fs`, or any adapter-specific crate.

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD** - write a failing-first test (unit test at a seam, or failing Manual-QA scenario) before production changes for every todo.
- Framework: built-in `cargo test` for unit/integration tests; `cargo clippy` for lint; `cargo build` and `cargo build --features ida` for compilation gates.
- Evidence path: `.omo/evidence/task-<N>-auto-re-m1.<ext>` for screenshots, logs, and CLI output captures.
- Manual-QA channels:
  - CLI: run `cargo run -- campaign status` and `cargo run -- task list` against a fresh SQLite DB.
  - TUI: run `cargo run -- tui` (or default `cargo run`) and capture the rendered terminal state via `interactive_bash` or terminal screenshot; verify it shows campaign/task status without IDA.
- Every todo's acceptance criteria include the exact `cargo test` command or CLI/TUI invocation that must pass.

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means under-splitting.

- **Wave 1: Foundation** - build feature-gating, dependencies, error types, TUI scaffolding, IDs, primitives, domain entities. Blocks all other waves.
- **Wave 2: Ports & Adapters** - `AnalysisBackend`/`ModelProvider` traits + deterministic mocks, worker packet builder, schema validation, claim/evidence conversion. Depends on Wave 1.
- **Wave 3: Persistence & Scheduler** - SQLite, migrations, repository traits, `TaskRepository`, atomic leasing, scheduler priorities/model routing/campaign loop. Depends on Waves 1–2.
- **Wave 4: Worker, CLI, TUI & Proof** - worker runner, CLI commands, TUI dashboard + real-time updates, campaign smoke test, kill→resume recovery test. Depends on Waves 1–3.

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | - | 2,3,4 | - |
| 2 | 1 | 3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20 | - |
| 3 | 1,2 | 4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20 | - |
| 4 | 1,2,3 | 5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20 | - |
| 5 | 1–4 | 7,12,14,17,18,19,20 | 6,8 |
| 6 | 1–4 | 8,12,13,14,15,17,18,19,20 | 5,7 |
| 7 | 1–5 | 14,15,17,18,19,20 | 6,8 |
| 8 | 1–6 | 14,15,17,18,19,20 | 5,7,9 |
| 9 | 1–4 | 10,11,12,13,14,15,16,17,18,19,20 | 5,6,7,8 |
| 10 | 1–9 | 11,12,13,14,15,16,17,18,19,20 | - |
| 11 | 1–10 | 12,13,14,15,16,17,18,19,20 | - |
| 12 | 1–11 | 13,14,15,16,17,18,19,20 | - |
| 13 | 1–12 | 14,15,16,17,18,19,20 | - |
| 14 | 1–13 | 15,16,17,18,19,20 | - |
| 15 | 1–14 | 16,17,18,19,20 | - |
| 16 | 1–15 | 17,18,19,20 | - |
| 17 | 1–16 | 18,19,20 | - |
| 18 | 1–17 | 19,20 | - |
| 19 | 1–18 | 20 | - |
| 20 | 1–19 | - | - |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

- [x] 1. Feature-aware build.rs and Cargo.toml for no-IDA core build
  What to do / Must NOT do: Rewrite `build.rs` so IDA build calls are feature-gated. Add all M1 dependencies to `Cargo.toml`. Keep `idax`/`gdbstub`/`llama_cpp` behind default-off features. Do NOT make idax a default dependency.
  Parallelization: Wave 1 | Blocked by: - | Blocks: 2,3,4
  References: spec §3, §4, §5, §6, §39; existing `Cargo.toml`, `build.rs`.
  Acceptance criteria: `cargo build` exits 0 with no features enabled; `cargo build --features ida` compiles when prerequisites are available; `cargo check --all-features` exits 0.
  QA scenarios: happy: `cargo build` succeeds; Evidence `.omo/evidence/task-1-auto-re-m1-build.log`.
  Commit: Y | build: feature-gate optional backends and add Milestone 1 dependencies

- [x] 2. Error enum, Result alias, and TUI scaffolding decoupled from IDA
  What to do / Must NOT do: Implement the spec §3 `#[derive(Debug, Error)]` `Error` enum with variants `Configuration`, `Database`, `ModelProvider`, `AnalysisBackend`, `Worker`, `Validation`, `Io`, and `Ida(#[from] idax::Error)` behind `#[cfg(feature = "ida")]`. Add `pub type Result<T, E = Error> = std::result::Result<T, E>;`. Update `Cargo.toml` so `default = ["tui"]`, `tui = ["dep:crossterm", "dep:ratatui"]`, and `tui` does NOT imply `ida`. Remove `smol` dependency. Make the existing `src/event.rs`, `src/tui.rs`, `src/tui/state.rs` modules compile by default. Replace the `smol::block_on` TUI call in `src/main.rs` with a tokio-based entry. Do NOT let the TUI import `idax` or require IDA. Do NOT make the TUI control the scheduler.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20
  References: spec §3, §6; remote `src/event.rs`, `src/tui.rs`, `src/main.rs`; `Cargo.toml:14-19`.
  Acceptance criteria: `cargo build` (default features, includes TUI) exits 0 with no IDA installed; `cargo build --no-default-features` exits 0; `cargo test error_enum` passes; `cargo test tui_compiles` passes.
  QA scenarios: happy: `cargo run` (default TUI) starts and shows a frame; exit with `q`; Evidence `.omo/evidence/task-2-auto-re-m1-tui.log`.
  Commit: Y | feat(tui, error): spec-aligned Error enum and TUI decoupled from IDA

- [x] 3. Typed ID macro and domain primitives
  What to do / Must NOT do: Create `src/ids.rs` with `macro_rules! define_id` over `uuid::Uuid` for all spec §8 IDs. Create domain primitives: `Address`, `AddressSpace`, `ContentHash`, `SymbolName`, `Provenance`, `Confidence` (0.0–1.0). Do NOT use raw UUIDs/strings as IDs.
  Parallelization: Wave 1 | Blocked by: 1,2 | Blocks: 4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20
  References: spec §8, §9.2, §9.3.
  Acceptance criteria: `cargo test ids_serialize_and_roundtrip` passes; `cargo test confidence_rejects_out_of_range` passes; `cargo test ids_are_not_interchangeable` passes.
  QA scenarios: happy: `TaskId::new()` unique; `Confidence::new(0.5)` OK; failure: `Confidence::new(1.5)` returns `Error::Validation`; Evidence `.omo/evidence/task-3-auto-re-m1-id-tests.log`.
  Commit: Y | feat(ids, domain): typed IDs and domain primitives

- [x] 4. Domain entities: Function, Campaign, Task, Claim, Evidence
  What to do / Must NOT do: Implement `src/domain/function.rs`, `src/domain/campaign.rs`, `src/domain/task.rs`, `src/domain/claim.rs`, `src/domain/evidence.rs` matching spec §9, §17, §24. Include constructors and state-transition methods. Do NOT import adapter crates.
  Parallelization: Wave 1 | Blocked by: 1,2,3 | Blocks: 5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20
  References: spec §7.1, §9, §17, §24.
  Acceptance criteria: `cargo test task_state_transitions` passes; `cargo test claim_state_transitions` passes; `cargo test task_dependencies` passes; `cargo test claim_evidence_link` passes.
  QA scenarios: happy: `Task` moves `Pending → Ready → Leased → Running → Completed`; failure: completing `Pending` returns `Error::Validation`; Evidence `.omo/evidence/task-4-auto-re-m1-domain-tests.log`.
  Commit: Y | feat(domain): Function, Campaign, Task, Claim, Evidence entities

- [x] 5. AnalysisBackend trait and MockAnalysisBackend with 10-function fixture
  What to do / Must NOT do: Create `src/analysis/backend.rs` with `AnalysisCapability` enum (spec §10) and `#[async_trait] trait AnalysisBackend`. Create `src/analysis/mock.rs` with `MockAnalysisBackend` returning a deterministic 10-function fixture. Do NOT import `idax` here; that belongs in `src/analysis/ida/` (Milestone 3).
  Parallelization: Wave 2 | Blocked by: 1–4 | Blocks: 7,12,14,17,18,19,20
  References: spec §7.2, §10, §11.2, §21, §39 proof.
  Acceptance criteria: `cargo test mock_backend_inventory_returns_ten_functions` passes; `cargo test mock_backend_capabilities` passes; `cargo test mock_backend_analyze_is_deterministic` passes; `cargo test unsupported_capability_returns_error` passes.
  QA scenarios: happy: `mock.inventory().await` returns 10 functions; failure: `decompile` returns capability error; Evidence `.omo/evidence/task-5-auto-re-m1-mock-analysis.log`.
  Commit: Y | feat(analysis): AnalysisBackend trait and deterministic mock fixture

- [x] 6. ModelProvider trait and MockModelProvider with deterministic responses
  What to do / Must NOT do: Create `src/model/provider.rs` with `ModelProvider` trait, `ModelDescriptor`, `ModelCapabilities`, `ModelClass`, `ModelRequest`, `ModelResponse`. Use `tokio_util::sync::CancellationToken`. Create `src/model/mock.rs` with deterministic schema-bound responses. Do NOT import `llama_cpp` here.
  Parallelization: Wave 2 | Blocked by: 1–4 | Blocks: 8,12,13,14,15,17,18,19,20
  References: spec §7.4, §13, §14.
  Acceptance criteria: `cargo test mock_provider_lists_models` passes; `cargo test mock_provider_complete_returns_valid_json` passes; `cargo test mock_provider_cancels_on_token` passes; `cargo test mock_provider_descriptor_capabilities` passes.
  QA scenarios: happy: `provider.complete(request, token).await` returns valid JSON; failure: cancel token triggers early return; Evidence `.omo/evidence/task-6-auto-re-m1-mock-model.log`.
  Commit: Y | feat(model): ModelProvider trait and deterministic mock provider

- [x] 7. Worker packet builder and FunctionAnalysisPacket
  What to do / Must NOT do: Create `src/analysis/packet.rs` with `FunctionAnalysisPacket` (spec §21) and `PacketBuilder` trait. Packets must be `Serialize`, bounded, deterministic, hashable. Do NOT include raw backend types.
  Parallelization: Wave 2 | Blocked by: 1–5 | Blocks: 14,15,17,18,19,20
  References: spec §21.
  Acceptance criteria: `cargo test packet_is_serializable` passes; `cargo test packet_is_deterministic` passes; `cargo test packet_builder_uses_mock_backend` passes; `cargo test packet_hashes_equal_for_equal_input` passes.
  QA scenarios: happy: packet for function 0 has expected callers/callees; failure: unknown function returns error; Evidence `.omo/evidence/task-7-auto-re-m1-packet.log`.
  Commit: Y | feat(analysis): FunctionAnalysisPacket and PacketBuilder

- [x] 8. Schema validation and worker output types
  What to do / Must NOT do: Create `src/worker/output.rs` with `FunctionAnalysisOutput` (spec §22). Implement JSON schema validation using `jsonschema` + `schemars`. Return `Error::Validation` on mismatch. Do NOT silently truncate invalid output.
  Parallelization: Wave 2 | Blocked by: 1–6 | Blocks: 14,15,17,18,19,20
  References: spec §7.6, §22.
  Acceptance criteria: `cargo test valid_output_passes_schema` passes; `cargo test malformed_output_fails_schema` passes; `cargo test schema_error_includes_pointer` passes; `cargo test output_roundtrips_via_json` passes.
  QA scenarios: happy: valid JSON passes; failure: missing `confidence` fails with path; Evidence `.omo/evidence/task-8-auto-re-m1-schema.log`.
  Commit: Y | feat(worker): schema validation and worker output types

- [x] 9. Claim and evidence conversion from worker output
  What to do / Must NOT do: In `src/domain/claim.rs` and `src/domain/evidence.rs`, add conversion from `FunctionAnalysisOutput` to proposed `Claim`/`Evidence` with `ClaimState::Proposed`. Implement `Provenance::Agent { worker_run_id }`. Record linear dependencies. Do NOT auto-accept claims.
  Parallelization: Wave 2 | Blocked by: 1–8 | Blocks: 14,15,17,18,19,20
  References: spec §24, §25.
  Acceptance criteria: `cargo test worker_output_to_proposed_claims` passes; `cargo test claims_start_in_proposed_state` passes; `cargo test evidence_links_to_claims` passes; `cargo test claim_dependencies_recorded` passes.
  QA scenarios: happy: worker output produces one `Claim` in `Proposed` with linked `Evidence`; failure: confidence > 1 rejected; Evidence `.omo/evidence/task-9-auto-re-m1-conversion.log`.
  Commit: Y | feat(domain): convert worker output to proposed claims and evidence

- [x] 10. SQLite connection, refinery migrations, and repository traits
  What to do / Must NOT do: Create `src/storage/database.rs` with `Database` struct. Create `migrations/V1__initial_schema.sql` with tables: `campaigns`, `binary_revisions`, `modules`, `functions`, `tasks`, `claims`, `evidences`, `leases`, `artifacts`. Integrate `refinery`. Define repository traits. Only `TaskRepository` gets SQLite impl in M1; others in-memory stubs. Do NOT make domain depend on `rusqlite`.
  Parallelization: Wave 3 | Blocked by: 1–4 | Blocks: 10,11,12,13,14,15,16,17,18,19,20
  References: spec §7.7, §15, §16.
  Acceptance criteria: `cargo test migrations_apply_cleanly` passes; `cargo test database_opens_new_file` passes; `cargo test repository_traits_compile` passes; `cargo test domain_has_no_external_imports` passes.
  QA scenarios: happy: `Database::open(".auto-re/state.sqlite3")` applies migrations; failure: malformed path returns `Error::Database`; Evidence `.omo/evidence/task-10-auto-re-m1-storage.log`.
  Commit: Y | feat(storage): SQLite database, refinery migrations, repository traits

- [x] 11. TaskRepository: create, lease, complete, fail
  What to do / Must NOT do: Implement `src/storage/repositories/task.rs` with `TaskRepository` trait: `create`, `lease_next`, `renew_lease`, `complete`, `fail`. `lease_next` must be a single SQLite transaction. Do NOT allow leasing non-ready tasks or duplicate active leases.
  Parallelization: Wave 3 | Blocked by: 1–10 | Blocks: 11,12,13,14,15,16,17,18,19,20
  References: spec §15, §18.
  Acceptance criteria: `cargo test task_repository_create_and_fetch` passes; `cargo test lease_next_returns_ready_task` passes; `cargo test lease_next_respects_dependencies` passes; `cargo test complete_updates_state_and_claims` passes; `cargo test fail_increments_attempt_count` passes.
  QA scenarios: happy: create 3 tasks, lease one, state becomes `Leased`; failure: leasing `Blocked` returns `None`; Evidence `.omo/evidence/task-11-auto-re-m1-taskrepo.log`.
  Commit: Y | feat(storage): SQLite TaskRepository with atomic leasing

- [x] 12. Atomic leasing contention and expired-lease recovery tests
  What to do / Must NOT do: Add concurrency tests: two threads racing `lease_next` → exactly one wins. Expired lease with past `expires_at` → reclaimed. Completion idempotency. Artifact reference integrity. Do NOT use sleeps; use injected `OffsetDateTime`.
  Parallelization: Wave 3 | Blocked by: 1–11 | Blocks: 13,14,15,16,17,18,19,20
  References: spec §18, §43.
  Acceptance criteria: `cargo test concurrent_lease_exactly_one_wins` passes; `cargo test expired_lease_is_reclaimed` passes; `cargo test complete_is_idempotent` passes; `cargo test artifact_reference_integrity` passes.
  QA scenarios: happy: 2 concurrent leases → 1 `Some`, 1 `None`; failure: renewing expired lease returns `Error::Validation`; Evidence `.omo/evidence/task-12-auto-re-m1-lease-tests.log`.
  Commit: Y | test(storage): atomic leasing, expiry, and idempotency tests

- [x] 13. Scheduler: deterministic priorities and model routing
  What to do / Must NOT do: Create `src/scheduler/scheduler.rs` with `Scheduler` struct, `PriorityFactors` (spec §20), stable priority score, `ModelRouter` in `src/model/router.rs`. Do NOT make scheduler call models directly.
  Parallelization: Wave 3 | Blocked by: 1–12 | Blocks: 14,15,16,17,18,19,20
  References: spec §7.5, §14, §19, §20.
  Acceptance criteria: `cargo test priority_score_is_stable` passes; `cargo test priority_factors_are_inspectable` passes; `cargo test model_router_selects_by_class` passes; `cargo test model_router_enforces_concurrency_limits` passes.
  QA scenarios: happy: `VerifyClaim` routes to `Verifier`; failure: unavailable class returns `Error::ModelProvider`; Evidence `.omo/evidence/task-13-auto-re-m1-scheduler-priority.log`.
  Commit: Y | feat(scheduler): deterministic priorities and model routing

- [x] 14. Scheduler campaign loop, task dispatch, and invalidation
  What to do / Must NOT do: Implement `run_campaign` loop (spec §37): recover leases, refresh inventory, invalidate stale work, create ready tasks, launch work, evaluate state. Create `src/scheduler/lease.rs`. Return `CampaignEvaluation { Complete, Blocked, Idle, Active }`. Do NOT block Tokio core threads on SQLite.
  Parallelization: Wave 3 | Blocked by: 1–13 | Blocks: 15,16,17,18,19,20
  References: spec §7.5, §18, §19, §25, §37.
  Acceptance criteria: `cargo test scheduler_evaluates_complete` passes; `cargo test scheduler_recovers_expired_lease` passes; `cargo test scheduler_invalidates_stale_work` passes; `cargo test scheduler_respects_dependencies` passes; `cargo test scheduler_idle_sleeps` passes.
  QA scenarios: happy: all tasks complete → `Complete`; failure: dependency chain blocks until predecessor completes; Evidence `.omo/evidence/task-14-auto-re-m1-scheduler-loop.log`.
  Commit: Y | feat(scheduler): campaign loop, dispatch, and invalidation

- [x] 15. Worker runner: dispatch, cancellation, timeout, and schema validation
  What to do / Must NOT do: Create `src/worker/runner.rs` with `WorkerRunner` that receives `WorkerInput`, calls `ModelProvider`, validates schema, converts to proposed claims/evidence. Respect `CancellationToken` and `time_budget`. On timeout/cancellation, fail task. Do NOT commit claims directly.
  Parallelization: Wave 4 | Blocked by: 1–14 | Blocks: 16,17,18,19,20
  References: spec §7.6, §22, §23.
  Acceptance criteria: `cargo test worker_runs_valid_output_to_claims` passes; `cargo test worker_rejects_malformed_schema` passes; `cargo test worker_cancels_on_token` passes; `cargo test worker_times_out` passes.
  QA scenarios: happy: valid JSON → proposed claims; failure: invalid JSON → `Error::Validation`; Evidence `.omo/evidence/task-15-auto-re-m1-worker.log`.
  Commit: Y | feat(worker): runner with cancellation, timeout, and schema validation

- [x] 16. CLI commands and tokio main wiring
  What to do / Must NOT do: Create `src/cli/mod.rs`, `src/cli/campaign.rs`, `src/cli/task.rs` with clap subcommands: `campaign status`, `task list`, `task status`. Wire `src/main.rs` to `#[tokio::main] async fn main() -> auto_re::Result<()> { auto_re::cli::run().await }` for headless mode and launch TUI for interactive mode. Default to TUI when no subcommand is given and `tui` feature is enabled. Do NOT implement create/start/stop/resume commands in M1.
  Parallelization: Wave 4 | Blocked by: 1–15 | Blocks: 17,18,19,20
  References: spec §7.17, §35, §36.
  Acceptance criteria: `cargo run -- campaign status` exits 0; `cargo run -- task list` exits 0; `cargo run -- task status nonexistent-id` returns error; `cargo clippy` clean.
  QA scenarios: happy: `cargo run -- campaign status` shows empty table; failure: `cargo run -- task status` without id prints clap error; Evidence `.omo/evidence/task-16-auto-re-m1-cli.log`.
  Commit: Y | feat(cli): campaign and task status commands with tokio main

- [x] 17. TUI campaign dashboard view
  What to do / Must NOT do: In `src/tui.rs`, implement a `Tui` dashboard that displays: campaign list, selected campaign status, task list with states, claim summary, and progress. Use `ratatui` widgets (Table, List, Gauge, Paragraph). The TUI reads from `CampaignRepository`/`TaskRepository`/`ClaimRepository` traits (in-memory stubs in M1) and does NOT import `idax`. Support 'q' to quit. Do NOT mutate state.
  Parallelization: Wave 4 | Blocked by: 1–16 | Blocks: 18,19,20
  References: remote `src/tui.rs`, `src/event.rs`; spec §35.
  Acceptance criteria: `cargo test tui_dashboard_renders` passes; `cargo test tui_dashboard_shows_campaigns` passes; `cargo test tui_dashboard_quits_on_q` passes.
  QA scenarios: happy: `cargo run` renders dashboard with empty state; failure: TUI with malformed state doesn't panic; Evidence `.omo/evidence/task-17-auto-re-m1-tui-dashboard.log` (terminal screenshot).
  Commit: Y | feat(tui): campaign dashboard view

- [x] 18. TUI real-time updates from scheduler
  What to do / Must NOT do: Wire the TUI to receive updates from the scheduler via a tokio channel or by polling the repositories on a timer. The TUI should refresh the dashboard as tasks move through states and claims are produced. The TUI runs in a separate tokio task; the scheduler runs in another. Do NOT let the TUI call the scheduler directly.
  Parallelization: Wave 4 | Blocked by: 1–17 | Blocks: 19,20
  References: spec §37; remote `src/event.rs`.
  Acceptance criteria: `cargo test tui_updates_on_task_state_change` passes; `cargo test tui_updates_on_new_claim` passes; `cargo test tui_does_not_block_scheduler` passes.
  QA scenarios: happy: TUI shows task progress during a mock campaign; failure: killing TUI task does not stop scheduler; Evidence `.omo/evidence/task-18-auto-re-m1-tui-updates.log` (terminal screenshot).
  Commit: Y | feat(tui): real-time updates from scheduler

- [x] 19. Campaign smoke test: scheduler + mocks + SQLite + TUI
  What to do / Must NOT do: Write an integration test under `tests/campaign_smoke.rs` that creates a `Runtime` with mock backends, SQLite storage, scheduler, and worker runner. Run a campaign with the 10-function fixture. Assert all 10 functions analyzed, claims produced, and TUI dashboard receives updates. Do NOT require real IDA or model.
  Parallelization: Wave 4 | Blocked by: 1–18 | Blocks: 20
  References: spec §39 proof.
  Acceptance criteria: `cargo test campaign_smoke_completes` passes; `cargo test campaign_smoke_analyzes_all_functions` passes; `cargo test campaign_smoke_produces_claims` passes; `cargo test campaign_smoke_updates_tui` passes.
  QA scenarios: happy: campaign completes and TUI shows 10 tasks completed; failure: missing mock provider causes campaign to block; Evidence `.omo/evidence/task-19-auto-re-m1-smoke.log`.
  Commit: Y | test(integration): campaign smoke test with mock backends and TUI

- [x] 20. End-to-end crash recovery: kill→resume without duplicate accepted claims
  What to do / Must NOT do: Write an integration test that starts the campaign in a child process, waits until at least one claim reaches `Accepted`, kills the process, restarts with the same SQLite file, and asserts: accepted claims persist, no duplicate accepted claims, campaign eventually completes. The TUI can be disabled in this test. Do NOT use graceful shutdown.
  Parallelization: Wave 4 | Blocked by: 1–19 | Blocks: -
  References: spec §18, §39 proof, §43.
  Acceptance criteria: `cargo test kill_resume_no_duplicate_claims` passes; `cargo test accepted_claims_persist_after_kill` passes; `cargo test campaign_completes_after_resume` passes.
  QA scenarios: happy: kill mid-campaign, restart, no duplicates, completes; failure: broken lease expiry re-runs accepted work; Evidence `.omo/evidence/task-20-auto-re-m1-recovery.log`.
  Commit: Y | test(integration): kill-resume recovery proof

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [x] F1. Plan compliance audit: verify every spec §42 file exists, every §43 required test is present, no domain file imports adapter crates, `cargo build` succeeds without optional features, and TUI runs without IDA.
- [x] F2. Code quality review: run `cargo clippy --all-targets --all-features`, `cargo fmt --check`, review for stubs/TODOs/scope creep, confirm no `unwrap`/`panic` in production paths.
- [x] F3. Real manual QA: run `cargo run` (TUI), `cargo run -- campaign status`, and `cargo run -- task list` on a fresh `.auto-re/` directory; capture stdout/terminal state; run full `cargo test`.
- [x] F4. Scope fidelity: confirm no real IDA/GDB/llama.cpp code on default path, no C++ generation, no network model provider, no workspace split, and no artifact storage beyond SQLite metadata.

## Commit strategy
- One atomic conventional commit per todo (feat/test/build/fix as appropriate).
- Commit message format: `<type>(<scope>): <imperative summary>`.
- No commit should leave the tree in a non-compiling state (`cargo build` must pass before each commit).
- Final verification wave commits (if any fixes are needed) use `fix(<scope>): ...`.
- Do not squash until the final wave passes; keep history granular for review.

## Success criteria
1. `cargo build` exits 0 with default features (includes TUI) on a machine without IDA installed.
2. `cargo build --no-default-features` exits 0.
3. `cargo build --features ida` exits 0 when `idax` prerequisites (`cmake`, IDA SDK) are available.
4. `cargo test` passes 100%, including all §43 required unit, database, scheduler, worker, TUI, and end-to-end recovery tests.
5. `cargo run` launches the TUI and shows campaign/task status on a fresh DB.
6. `cargo run -- campaign status` and `cargo run -- task list` produce correct output on a fresh DB.
7. The kill→resume integration test proves no duplicate accepted claims after process restart.
8. The mock campaign smoke test analyzes all 10 synthetic functions to completion and updates the TUI.
9. Domain layer contains zero imports of `idax`, `gdbstub`, `llama_cpp`, `rusqlite`, `tokio`, `reqwest`, or `std::fs`.
10. `cargo clippy --all-targets --all-features` and `cargo fmt --check` are clean.
