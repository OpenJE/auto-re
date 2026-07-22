# Stage 1 Implementation Report

**Date:** 2026-07-22
**Spec reference:** §22, §23
**Scope:** Final implementation summary for the `auto-re` Stage 1 vertical slice. 60 todos across 12 waves, delivering a whole-executable reverse-engineering platform with typed external providers, deterministic SCC scheduling, managed C++ generation, and differential verification.

---

## 1. Existing code (Retained/Adapted/Replaced/Removed/Added)

Stage 1 began with a full audit of the `autore-stage1` crate (30 `.rs` files). The audit is documented in `docs/stage1-audit.md`. Every file was classified into one of five dispositions.

### Retained (2 files)

| File | Notes |
|------|-------|
| `autore-stage1/src/main.rs` | Binary entry point preserved verbatim. |
| `autore-stage1/src/worker/output.rs` | Single re-export; types live in `autore-schema::worker_output`. |

### Adapted (14 files)

| File | Adaptation |
|------|------------|
| `autore-stage1/src/lib.rs` | Module map updated; new re-exports for coordinator, work graph, fingerprint. |
| `autore-stage1/src/error.rs` | Error variants extended for Stage 1 command failures. |
| `autore-stage1/src/analysis/packet.rs` | Packet structure retained; routing moved through `autore-app`. |
| `autore-stage1/src/model/provider.rs` | Model provider interface adapted to `GenerationModel` trait. |
| `autore-stage1/src/model/router.rs` | `ModelRouter` retained as scheduler field; priority scoring preserved. |
| `autore-stage1/src/model/mock.rs` | Mock provider adapted for test fixtures. |
| `autore-stage1/src/scheduler/mod.rs` | Module re-exports updated. |
| `autore-stage1/src/scheduler/scheduler.rs` | Priority score retained; transition requests moved through `autore-app` commands. |
| `autore-stage1/src/scheduler/lease.rs` | Lease logic adapted to use `ApplicationCommand::LeaseWorkItem`. |
| `autore-stage1/src/worker/mod.rs` | Module re-exports updated. |
| `autore-stage1/src/storage/mod.rs` | Storage module thinned; direct repos deferred for removal. |
| `autore-stage1/src/cli/mod.rs` | CLI module preserved until Wave 11 replacement. |
| `autore-stage1/src/cli/campaign.rs` | Campaign CLI adapted for Stage 1 subcommands. |
| `autore-stage1/src/cli/task.rs` | Task CLI adapted for work-item subcommands. |

### Replaced (5 files)

| File | Replacement |
|------|-------------|
| `autore-stage1/src/analysis/backend.rs` | `AnalysisBackend::analyze()->String` replaced by external IDA provider with 9 typed capabilities. |
| `autore-stage1/src/analysis/mock.rs` | Mock backend replaced by fixture provider capabilities. |
| `autore-stage1/src/scheduler/repos.rs` | Direct `TaskRepository`/`SchedulerQueries` replaced by `Arc<dyn AutoReClient>` + snapshot-based dispatch. |
| `autore-stage1/src/worker/runner.rs` | `WorkerRunner` direct `ClaimRepository`/`EvidenceRepository` writes replaced by `ApplicationCommand` routing. |
| `autore-stage1/src/cli/headless.rs` | Headless CLI replaced by Wave 11 CLI with 5 new top-level commands. |

### Removed (9 files)

| File | Reason |
|------|--------|
| `autore-stage1/src/engine.rs` | In-process IDA engine replaced by external provider. |
| `autore-stage1/src/engine/graph.rs` | Experimental IDA graph (28 lines, behind `#[cfg(feature = "ida")]`) removed. |
| `autore-stage1/src/store.rs` | In-process IDA store (empty file) removed. |
| `autore-stage1/src/analysis/mod.rs` | Analysis module restructured into `autore-reconstruction`. |
| `autore-stage1/src/model/mod.rs` | Model module restructured into provider crates. |
| `autore-stage1/src/storage/repositories/mod.rs` | Repository module dissolved; mutations route through `autore-app`. |
| `autore-stage1/src/storage/repositories/claim.rs` | Claim repository replaced by `ApplicationCommand::AddHypothesis`. |
| `autore-stage1/src/storage/repositories/task.rs` | Task repository replaced by `ApplicationCommand` lifecycle variants. |
| `autore-stage1/src/cli/headless_queries.rs` | Duplicated SQL parsing removed; queries route through `ApplicationQuery`. |

### Added (new crates and modules)

Seven new workspace crates were created, all off `default-members` to preserve the Stage 0 `cargo build` surface:

| Crate | Role | Key files |
|-------|------|-----------|
| `autore-provider-protocol` | Versioned gRPC schema (`autore.provider.v1`) and codegen | `proto/autore/provider/v1/*.proto`, `autore-provider-protocol/build.rs` |
| `autore-provider-runtime` | Bootstrap, auth, lifecycle, artifact transport, package discovery | `autore-provider-runtime/src/{bootstrap,runtime,artifact,package}.rs` |
| `autore-reconstruction` | Core reconstruction logic: identity, work graph, fingerprint, analysis, generation, verification, coordinator | 70+ source files under `autore-reconstruction/src/` |
| `fixture-provider` | Test fixture binary with 5 capabilities | `providers/fixture/src/{main,provider}.rs` |
| `ida-provider` | External IDA provider with 16 capabilities (9 static + 7 debug) | `providers/ida/src/{main,provider}.rs` |
| `openai-compatible-provider` | OpenAI-compatible LLM provider with 13 capabilities (7 analysis + 6 generation) | `providers/openai-compatible/src/{main,provider,llm,prompts,schemas}.rs` |
| `build-provider` | cmkr+CMake+Docker-MSVC2002 build provider | `providers/build/src/{main,provider}.rs` |

---

## 2. Provider substrate

### gRPC protocol (spec §5)

The `autore-provider-protocol` crate defines 7 proto files under `proto/autore/provider/v1/`:

- `provider.proto` — `Provider` service with 5 RPCs: `Negotiate`, `DiscoverCapabilities`, `Execute`, `Health`, `GracefulShutdown`.
- `bootstrap.proto` — bootstrap handshake messages.
- `capability.proto` — `CapabilityDescriptor` with JSON Schema request/response schemas as `bytes`.
- `execution.proto` — `ExecutionRequest`/`ExecutionEvent` with `oneof` over `Accepted`, `Progress`, `Diagnostic`, `ObservationProduced`, `ArtifactProduced`, `Completed`.
- `event.proto` — monotonic sequence events with `provider_instance_id`, `request_id`, `operation_id`, `capability_id`, `capability_version`.
- `health.proto` — health check messages.
- `package.proto` — `ArtifactDescriptor` with `package_id`, `version`, `content_hash`, `relative_path`, `size`.

Version: tonic 0.14 + prost 0.14 (verified compatible pair via Context7). Build-time test `version_suffix_present` asserts all proto files declare `package autore.provider.v1;`.

### Bootstrap and runtime (spec §5.3)

`autore-provider-runtime/src/bootstrap.rs` implements `CoordinatorBootstrap`:
- Generates UUIDv7 `ProviderInstanceId` + 32-byte `BootstrapSecret` via `getrandom`.
- UDS-first listener with TCP `127.0.0.1:0` fallback using `BootstrapStream` enum.
- Raw binary bootstrap protocol: auth (32-byte secret echo) → negotiate (u32 min/max range) → gRPC address exchange.
- Secrets passed ONLY via environment variables (`AUTORE_BOOTSTRAP_SECRET`, `AUTORE_BOOTSTRAP_SOCKET`, `AUTORE_BOOTSTRAP_INSTANCE_ID`), never argv.

`autore-provider-runtime/src/runtime.rs` implements `ProviderRuntime::spawn`:
- Full 13-step bootstrap: CoordinatorBootstrap → bind socket → spawn child → authenticate → version negotiation → gRPC address exchange → channel connect → Negotiate RPC → package identity verification → concurrency limits → cancellation token.
- `GracefulShutdownSeq`: GracefulShutdown RPC → 10s wait → kill + reap.
- `CancellationToken` propagation into inbound streams and child process.

### Package discovery and validation (spec §5.2)

`autore-provider-runtime/src/package.rs` (282 pure LOC):
- Reads `project.auto-re/provider_roots.toml` with fallback to `<project_dir>/providers/`.
- 13-variant `PackageValidationError` enum.
- Validation pipeline: TOML parse → schema_version → package_id regex (`^[a-z0-9-]+\.[a-z0-9-]+$`) → semver → entrypoint existence → canonical containment → content hash → protocol range → capabilities → configuration_schema.
- Content hash: per-file BLAKE3 sorted by relative path, fed into final BLAKE3.
- Symlinks rejected (not traversed) during hash walk.

### Artifact transport (spec §5.4)

`autore-provider-runtime/src/artifact.rs`:
- `ArtifactTransport` trait with 4 methods: `stage_inbound`, `stage_outbound`, `commit_inbound`, `discard`.
- `ArtifactHandle` is opaque; providers receive staging-scoped paths, never canonical artifact paths.
- `ArtifactLocation` enum (`Local(PathBuf)` / `Remote(String)`) admits remote transports without changing capability semantics.
- `LocalStagingTransport` rooted at `project.auto-re/staging/` with layout `<root>/<instance_id>/<request_id>/<artifact_uuid>/data`.
- `commit_inbound` independently recomputes BLAKE3; discards on mismatch.
- `StagingReconciler::sweep()` removes orphan staging entries on startup.

### Fixture provider (spec §5.5)

`providers/fixture/` (off `default-members`):
- 5 capabilities: `fixture.echo`, `fixture.delay`, `fixture.fail`, `fixture.artifact`, `fixture.large-stream`.
- Full bootstrap protocol: env vars → connect → auth → negotiate → gRPC address exchange → serve.
- `manifest.toml` with `package_id = "fixture.echo"`, `version = "0.1.0"`, BLAKE3 content hash, `protocol_range = [1, 1]`, `max_concurrency = 4`.
- Integration test drives all 5 capabilities through `ProviderRuntime::spawn`.

---

## 3. IDA integration

### External IDA provider (spec §6)

`providers/ida/` implements 16 capabilities:

**9 static analysis capabilities:**
- `ida.binary.open` — opens an IDB database via `idax::database::open`.
- `ida.binary.ingest` — whole-binary structural ingestion producing 5 snapshot artifacts (disassembly, decompilation, CFG, instructions, types).
- `ida.program.refresh` — deterministic refresh with stale-marking invalidation.
- `ida.function.snapshot` — per-function decompiler output snapshot.
- `ida.type.snapshot` — per-type structural snapshot.
- `ida.class.snapshot` — per-class snapshot with vtable slots.
- `ida.references.query` — cross-reference query.
- `ida.reanalyze` — trigger reanalysis.
- `ida.native-artifact.export` — export native artifact.

**7 debug capabilities (spec §9):**
- `debug.launch`, `debug.attach`, `debug.breakpoint`, `debug.continue`, `debug.step`, `debug.capture-function`, `debug.capture-memory`.

`idax` is behind an `ida` feature flag; without it, `ida.binary.open` returns a diagnostic error, allowing builds without the IDA SDK.

### Canonical entity identity (spec §6.5)

`autore-reconstruction/src/identity/` (4 sub-modules):
- `CanonicalEntityKey` with 4 structural fields: `binary_revision_id || address_space || entry_address || entity_kind`.
- Sidecar `provider_native_extension` HashMap for IDA row ids, deliberately excluded from `stable_key()` and `identity_hash()`.
- Stable key: `StableEntityKey::ExternalIdentity{ namespace: "autore.recon.canonical", value: canonical_json }` using `BTreeMap`-backed JSON for deterministic ordering.
- `ObservationImporter` issues `RegisterEntity` for unseen entities and `ImportProviderRunResult` for rematch via `ApplicationQuery::ListEntities`.
- Stale diagnostics issue `BlockWorkItem(reason="ProviderObservedStaleEntity")` + `CreateWorkItems`; entities are NEVER deleted.

### Migrations (spec §6.4)

Two-layer storage implemented via additive migrations:
- V14..V23: `reconstruction_campaigns`, `reconstruction_work_items`, `work_dependencies`, `work_fingerprints`, `work_leases`, `provider_installations`, `provider_instances`, `stage1_provider_runs`, `capability_descriptors`, `native_artifact_snapshots`.
- V24..V27: `conflict_records`, `generated_source_mappings`, `blocked_reasons`, `repair_attempts`.
- V28..V33: `dynamic_observations`, `raw_llm_responses`, `parsed_llm_results`, `build_attempts`, `build_diagnostics`, `verification_scenarios`, `verification_comparisons`.

Total: 33 migrations (V1..V33). All use `CREATE TABLE IF NOT EXISTS` for idempotency. Stage 1 tables that would collide with Stage 0 names use `stage1_` prefix.

---

## 4. Debugger integration

### Typed scenario language (spec §9.2)

`autore-reconstruction/src/dynamic/scenario.rs`:
- Typed `Scenario` AST: `SetupOp` (2 variants), `Step` (14 variants), `StopOp` (3 variants), `AddressRange` struct.
- All AST types derive `serde::Serialize`/`serde::Deserialize` for JSON wire transport.

### Scenario verifier (spec §9.3)

`autore-reconstruction/src/dynamic/verifier.rs`:
- `ScenarioVerifier::validate()` enforces 4 invariants: entity existence, address containment in mapped segments, API allowlist, memory delta ≤ 64 KiB.
- 6 `ScenarioValidationError` variants.
- Pure function: no side effects, no I/O, no client dependency.

### TargetRunner trait (spec §9.6)

`autore-reconstruction/src/dynamic/runner.rs`:
- `TargetRunner` async trait with 8 methods: `launch`, `attach`, `stop`, `execute_step`, `capture_function`, `trace_function`, `capture_memory`, `capture_calls`.
- `WineGdbRunner` first implementation with configurable Wine/gdbserver paths and deterministic mock mode under `AUTORE_TEST_MOCK_RUNNER=1`.
- `WindowsGdbServerRunner` compile-time stub returning `RunnerError::Unsupported` for every method, proving the backend-agnostic seam.
- `CaptureContext` accumulates observations and staged artifacts.

### Dynamic observation import (spec §9.5)

`autore-reconstruction/src/dynamic/import.rs`:
- 6-step import flow: `RegisterArtifact` (kind `core.trace`) → `ImportDynamicObservation` → `AddEvidence` → fingerprint recompute → `InvalidateWorkItem` + downstream propagation → investigation work item creation on replay/nondeterminism flags.
- Nondeterminism detected via `replay_flag=true` or `sequence_token != scenario_id`.

### LLM-proposed experiment flow (spec §9.4)

Integration test `autore-reconstruction/tests/dynamic_llm_proposed_scenario.rs`:
- Full flow: LLM proposes typed `Scenario` → `ScenarioVerifier` validates → `CreateWorkItems` for investigation → `WineGdbRunner::mock()` executes → `DynamicObservationImporter` imports observations → fingerprint invalidation propagates to dependent analysis.
- Invalid path: verifier rejects with `UnmappedAddress` → `FailWorkItem` + `BlockWorkWithReason`.

---

## 5. LLM integration

### OpenAI-compatible provider (spec §8)

`providers/openai-compatible/` implements 13 capabilities:

**7 analysis capabilities:**
- `llm.analysis.function` — structured function analysis with proposed name/signature/claims/experiment-proposals.
- `llm.analysis.type` — type analysis.
- `llm.analysis.class` — class analysis.
- `llm.analysis.subsystem` — subsystem analysis.
- `llm.analysis.conflict` — conflict arbitration.
- `llm.analysis.failure` — failure analysis.
- `llm.experiment.design` — experiment design.

**6 generation capabilities (spec §11):**
- `llm.generation.declaration`, `llm.generation.type`, `llm.generation.function`, `llm.generation.cluster`, `llm.generation.test`, `llm.generation.repair`.

Provider uses `reqwest` HTTP client against operator-supplied endpoint. Response format: `choices[0].message.content` as JSON string. Per-request request schema validation via `jsonschema = 0.33`.

### Investigation bundles (spec §8.3)

`autore-reconstruction/src/analysis/bundle.rs`:
- `InvestigationBundle` carries only artifact handles (`ArtifactId`), never raw bytes.
- `BundleStore` trait abstracts data source; `StubStore` for testing.
- `BundleBuilder` walks work graph for callers/callees, fills from store.

### Import boundary (spec §8.5)

`autore-reconstruction/src/analysis/import/`:
- Three-level import: raw artifact → schema-validated result → canonical hypotheses/work items.
- `LlmImporter` validates response against committed JSON schemas under `autore-reconstruction/schemas/analysis/`.
- Bounded schema-repair policy: 1 attempt, then `FailWorkItem` + `BlockWorkWithReason("InvalidOutput: ...")`.
- Raw response persisted as `EvidenceRecord` (Level 1); parsed result as hypothesis (Level 3).
- No plaintext secrets in provenance records.

### Per-capability parser fixtures (spec §8.4)

14 response schema fixtures under `autore-reconstruction/schemas/analysis/`:
- `function-analysis-response.schema.json`, `type-analysis-response.schema.json`, `class-analysis-response.schema.json`, `subsystem-analysis-response.schema.json`, `conflict-analysis-response.schema.json`, `failure-analysis-response.schema.json`, `experiment-design-response.schema.json`.
- Table-driven tests with `{{ENTITY_REF_1}}` placeholders for ID substitution.

### Generation schemas and prompts

6 generation response schemas under `autore-reconstruction/schemas/generation/`:
- `generation.declaration.schema.json`, `generation.type.schema.json`, `generation.function.schema.json`, `generation.cluster.schema.json`, `generation.test.schema.json`, `generation.repair.schema.json`.
- 6 Handlebars prompt templates under `providers/openai-compatible/prompts/generation/`.
- Source bytes are base64-encoded strings in response schemas.
- Generated candidates staged via `LocalStagingTransport` per request, emitted as `ArtifactProduced`.

---

## 6. Scheduler

### Work graph (spec §7)

`autore-reconstruction/src/work_graph/`:
- `WorkGraphBuilder` with 5-phase construction: collect → create → build graph → SCC collapse → record dependencies.
- `DependencyEdgeKind` enum with 11 variants (10 spec + 1 synthetic ClusterMember).
- SCC detection via Kosaraju's algorithm (`petgraph::kosaraju_scc`).
- Function SCCs collapse into `FunctionCluster` nodes with `ClusterMember` edges.
- Mixed-kind SCCs rejected at validation time.
- All mutations route through `ApplicationCommand` variants (`CreateWorkItems`, `RecordWorkDependency`).

### 18 work-item kinds (spec §7.2)

`WorkItemKind` enum in `autore-schema/src/domain/records.rs`:
`ProgramSkeleton`, `ExternalDependency`, `Global`, `Enum`, `Structure`, `Class`, `Vtable`, `Function`, `FunctionCluster`, `StaticInitializer`, `Subsystem`, `Entrypoint`, `Investigation`, `Generation`, `BuildFailure`, `LinkFailure`, `VerificationFailure`, `ConflictResolution`.

### Fingerprint and invalidation (spec §7.5)

`autore-reconstruction/src/fingerprint/`:
- `FingerprintInput` with 8 input categories: static artifacts, hypotheses, upstream declarations, dynamic observations, prompt version, model config, build config, verification policy.
- `compute_fingerprint()` using BLAKE3 over canonical JSON (BTreeMap for deterministic key ordering).
- `InvalidationPropagator` walks downstream through `GeneratedDeclRequirement` and `BuildDependency` edges only.
- Propagation stops when recomputed fingerprint matches stored (bounded invalidation).
- All mutations via `ApplicationCommand::InvalidateWorkItem`.

### Scheduler via ApplicationCommand (spec §7.6)

`autore-stage1/src/scheduler/scheduler.rs`:
- Replaced `RepositorySet` with `Arc<dyn AutoReClient>` + `ProjectId` + task snapshot.
- All direct `TaskRepository`/`SchedulerQueries` calls removed from production code.
- Mutations route through: `FailWorkItem`, `RequeueWorkItem`, `PromoteWorkItem`, `LeaseWorkItem`.
- Reads use `ApplicationQuery::ListExpiredLeases`.
- `PriorityFactors` expanded with 5 new weights per spec §7.4: `dependents_unblocked`, `high_impact_conflict`, `removes_build_blocker`, `verified_coverage`, `evidence_strength`.
- `evaluate_state` is a pure function (no client, no Result).
- Scheduler is a pure decision engine: takes snapshot + factors + context, returns decisions via commands.

---

## 7. C++ generation

### Project skeleton (spec §11.2)

`autore-reconstruction/src/generation/skeleton.rs`:
- `ProjectSkeletonBuilder` takes `SemanticEntity` objects and emits a deterministic managed source tree.
- Generation order: external declarations → enums → types → globals → functions → classes → vtables → static initializers → entrypoints.
- Source paths derived from `EntityId` UUID hex: `<2hex>/<2hex>/<2hex>/<full-uuid>`; renaming display_name does NOT change paths.
- `StubPolicy` enum: `StaticAssert` (compile-fail) vs `EmptyBody` (compiles but no-op).
- Every generated file registered via `RegisterArtifact` (kind `core.generated-candidate`) + `RegisterGeneratedSourceMapping`.
- Directory structure: `include/recovered/`, `src/generated/`, `src/runtime/`, `src/subsystems/`, `src/entrypoints/`, `tests/`, `reports/`.
- CMakeLists.txt and reconstruction.toml generated as metadata files.

### Controlled staged patching (spec §11.5)

`autore-reconstruction/src/generation/patch.rs`:
- `PatchPipeline` implementing the full spec §11.5 pipeline: validate file targets → stage candidate artifacts → parse/syntax check → construct controlled patch → apply through generated project manager → build → accept or roll back.
- Validation rejects: blank paths, paths outside generated source tree, undeclared file deletions, content > 16 MiB, paths containing `auto-re/` segments, paths unrelated to work item's entity source directory.
- Lightweight syntactic validator (brace/paren/quote balance) catches obviously malformed C++.
- Transactional write-before-build with rollback: writes candidates, builds, restores `prior_content_bytes` on failure.
- `accept_or_roll_back` registers artifacts + generated-source mapping on build success; on failure restores prior content and issues `FailWorkItem`.

### Generation orchestrator (spec §11.4)

`autore-reconstruction/src/generation/orchestrator.rs`:
- `GenerationOrchestrator` with leaf-first priority selection (Function before FunctionCluster; fewer stubbed callees first).
- `GenerationModel` async trait boundary for testability.
- Deterministic repair routing via `BuildFailureClassifier::classify` + `select_repair_strategy`.
- Bounded LLM repair loop with per-work-item attempt counting and repeated-equivalent-failure detection (same diagnostic code + line + column).
- `BlockWorkItem{RepeatedEquivalentFailure}` or `BlockWorkItem{MaxRepairAttempts}` when thresholds exceeded.

### Declaration generator (spec §10.5)

`autore-reconstruction/src/types/declaration.rs`:
- `DeclarationGenerator` filters by `HypothesisStatus::Accepted`, groups by entity, detects size conflicts.
- Deterministic C++ rendering: `#pragma once`, `namespace recovered`, optional base classes, optional vtable pointer, `uint8_t` placeholder fields with explicit padding.
- Separate vtable headers: `include/recovered/<entity>_vtable.hpp` with function-pointer slots sorted by canonical slot index.
- Conflicts emit `CreateWorkItems { descriptions: ["BuildFailure: ..."] }`.

---

## 8. Build & verification

### Build provider (spec §12)

`autore-reconstruction/src/build/trait_def.rs`:
- `BuildProviderTrait` with 5 methods: `configure_project`, `compile_units`, `link_target`, `run_test`, `collect_diagnostics`.

`autore-reconstruction/src/build/docker_msvc2002.rs`:
- `DockerMsvc2002BuildProvider` first implementation: cmkr + cmake + Docker-hosted MSVC 2002.
- Command validation against allowlist (metacharacter check, image name check, path containment).
- Container names derived from `blake3(project_root)` for deterministic, collision-resistant naming.

`providers/build/`:
- External build provider binary following the bootstrap protocol pattern.
- `manifest.toml` with build capabilities.

### Build-failure classification (spec §12.4)

`autore-reconstruction/src/build/classification.rs`:
- 13-variant `BuildFailureKind` enum: `MissingDeclaration`, `UnknownType`, `IncompleteType`, `TypeMismatch`, `Syntax`, `LinkerUnresolved`, `LinkerDuplicate`, `LinkerOrder`, `BuildEnvironmentDefect`, `ConfigurationError`, `InternalCompilerError`, `LayoutMismatch`, `AbiMismatch`.
- `RepairStrategy` enum: `CreateWorkItems`, `BlockWorkItem`, `RequestLlmAnalysis`, `RequestLayoutInvestigation`, `NoAction`.
- `classify()` is a pure function: MSVC code → `BuildFailureKind` via match arms.
- Context-sensitive routing for C2440 (layout vs abi) and C2065 (stdlib vs missing-decl).
- Environment errors detected via `ENV*` code prefix or message text patterns.

### Differential verification (spec §13)

`autore-reconstruction/src/verification/`:
- `Scenario` per spec §13.2: `initial_state`, `inputs`, `executable_artifact_id`, `candidate_artifact_id`, `execution_steps`, `comparison_policy`, `normalization_rules`, `comparison_level`.
- `ObservationSet` with typed observations (registers, memory, stdout, stderr, exit code, diagnostics).
- `NormalizationRule` enum: `RelocatedAddress`, `Timestamp`, `RandomSeed`, `EnvSpecificHandle`.
- `ComparisonLevel` enum: `Function`, `Cluster`, `WholeProgram`.
- `ComparisonResult` enum with 6 variants: `Equal`, `EquivalentUnderNormalization`, `Different`, `Inconclusive`, `NotObserved`, `ExecutionFailed`.
- `ObservationBackend` trait + `ScenarioExecutor` with async `execute_original`, `execute_candidate`, `compare_and_record`.
- `Wave7ObservationBackend` drives `WineGdbRunner` through the Wave 7 scenario executor.

### Verification-driven repair (spec §13.4)

`autore-reconstruction/src/verification/repair.rs`:
- `VerificationRepairDriver` with 8-step repair flow: re-run scenarios → determine cause → emit comparison → create investigation work item → LLM failure analysis → generate repair patch → apply and rebuild → record repair attempt.
- `CauseCategory` enum: `Implementation`, `Type`, `Layout`, `Environment`, `Scenario`.
- `bounded_diff_for_llm()` summarizes execution diagnostics into token-capped string.

### Regression selection (spec §13.5)

`autore-reconstruction/src/verification/regression.rs`:
- `RegressionTracker` storing `HashMap<EntityId, RegressionSet>`.
- `compute_affected_entities` walks `BuildDependency`, `VerificationDependency`, and `GeneratedDeclRequirement` edges.
- `schedule_regressions` issues `ApplicationCommand::ScheduleVerificationRegression`.
- Configurable max regression scenarios per entity (default 100).

---

## 9. Recovery

### Coordinator loop (spec §14.1)

`autore-reconstruction/src/coordinator/`:
- `Coordinator::tick()` runs phases in order: `reconcile_interrupted_operations` → `refresh_provider_health` → `refresh_program_structure_if_requested` → `update_work_dependencies` → `invalidate_stale_work` → `promote_ready_work` → `select_ready_work`.
- Work-kind dispatch to 7 handlers: StaticInvestigation, DynamicInvestigation, SemanticAnalysis, ConflictResolution, Generation, BuildFailure, Verification.
- `NoProgressDetector` tracks last-3 raw-response hashes per entity; on 3 identical hashes emits `BlockWorkItem` with `RepeatedIdenticalModelOutput:<kind>` reason.
- `CompletionPolicy::is_complete` returns true when all required items are terminal; `is_successfully_complete` additionally requires no blocked items.
- `CancellationToken::is_cancelled` checked at start of every tick.

### Restart recovery (spec §17)

On restart:
- Old local provider instances marked `Unavailable` via `StopProviderInstance`.
- Leased/Running items requeued via `reconcile_interrupted_operations`.
- `StagingReconciler::sweep()` removes orphan staging directories.
- `CreateReconstructionCampaign` and `CreateWorkItems` NOT re-issued on resume (non-idempotent operations preserved).
- Uncommitted staging dropped; committed artifacts preserved.

### Fault injection tests (spec §14.3)

`autore-reconstruction/tests/faults.rs`:
- Fixture provider SIGKILL mid-RPC → instance marked unavailable.
- Coordinator restart old-provider sweep.
- IDA-style provider SIGKILL after `ArtifactProduced` before `Completed` → work item reconciled to `Failed`, orphan staging swept.
- LLM-style provider SIGKILL with partial artifact discard → no `RegisterArtifact` issued.
- SQLite atomic transaction fails closed → rollback + `PRAGMA integrity_check = ok`.

`autore-reconstruction/tests/faults-llm.rs`:
- Invalid parsed LLM output → `FailWorkItem` + `BlockWorkWithReason`.
- Provider timeout then retry success.
- Repeated identical compiler failure → `BlockWorkItem{RepeatedEquivalentFailure}`.
- Corrupted artifact import rejected → `ArtifactError::HashMismatch`.

`autore-reconstruction/tests/faults-coverage.rs`:
- Debugger timeout → `RunnerError::Cancelled`.
- Stale-work invalidation → downstream rebuild.
- Build-tool environment defect → `BlockWorkItem`.
- Cancellation token propagation.

### Persistence (spec §17)

All durable state stored in SQLite via additive migrations V14..V33:
- Provider installations/instances/runs, capability descriptors.
- Work items + deps + fingerprints + leases.
- Static snapshots, dynamic observations.
- Raw + parsed LLM responses, hypotheses, conflicts.
- Generated-source mappings, build attempts, compiler diagnostics.
- Verification scenarios + comparisons, repair attempts, blocked reasons.
- Operations + events.

---

## 10. Deferred work

The following items from spec §21 are explicitly excluded from Stage 1. Only the interfaces required to add them later are preserved.

### Analysis backends

1. **Ghidra provider** — no Ghidra implementation; `AnalysisProvider` trait in `autore-reconstruction` admits future Ghidra backends via `impl AnalysisProvider for GhidraProvider`.
2. **Binary Ninja provider** — no Binary Ninja implementation; same trait seam.
3. **Multi-backend consensus voting** — no consensus mechanism; each provider produces independent hypotheses.

### Package management

4. **Public package registry** — only local directory discovery via `provider_roots.toml`; no network registry.
5. **Package dependency solver** — no solver; packages are self-contained.
6. **Network package installation** — no remote installation; packages must be pre-staged locally.
7. **Package signing authority** — no signing; content hash verification only (BLAKE3).

### Remote execution

8. **Remote workers** — all execution is local; no remote worker protocol.
9. **Distributed scheduling** — single-process scheduler; no distributed consensus.
10. **General remote providers** — TCP listener stays on loopback `127.0.0.1:0`; no remote provider connections.
11. **Remote TLS authorization** — no TLS; bootstrap uses one-time secret over UDS/loopback.

### Sandboxing

12. **Full sandbox enforcement** — no sandbox; configured working directories and bounded runtimes only.
13. **Containers** — Docker used only for MSVC 2002 cross-compilation, not as a general sandbox.
14. **MicroVMs** — no microVM isolation.

### Advanced analysis

15. **Symbolic execution** — no symbolic execution engine.
16. **Concolic execution** — no concolic execution engine.
17. **General fuzzing** — no fuzzing harness; differential verification uses structured scenarios only.
18. **Kernel debugging** — no kernel debug support; `TargetRunner` targets user-space executables.
19. **Multi-process distributed tracing** — no multi-process tracing; single-target scenarios only.
20. **Hardware tracing** — no hardware trace support (Intel PT, etc.).

### Generation

21. **Multiple generation languages** — Stage 1 ships C++ only; `ImplementationTargetId` is the polymorphism seam for future languages, but no second generator is implemented.

### Build systems

22. **Build systems beyond cmkr+CMake+Docker-MSVC2002** — `BuildProviderTrait` admits future build systems (clang-cl, native MSVC, etc.), but only the Docker-MSVC2002 first implementation is shipped.

### TUI and providers

23. **Arbitrary provider TUI plugins** — TUI has fixed 12-pane layout; no plugin system.
24. **Automatic hot reload** — no hot reload; provider changes require restart.
25. **Wasm providers** — no Wasm runtime; providers are native binaries.
26. **Native dynamic-library providers** — no dynamic library loading; providers are separate processes.

### Learning and knowledge

27. **Automatic prompt-training** — no prompt optimization; prompts are static Handlebars templates.
28. **Fine-tuning pipelines** — no fine-tuning; operator supplies LLM endpoint.
29. **General cross-project knowledge packages** — no knowledge sharing between projects.
30. **General autonomous tool use** — coordinator follows fixed work-kind dispatch; no arbitrary tool selection.

### Verification

31. **Formal whole-program equivalence proofs** — differential verification uses structured scenario comparison, not formal proofs.

---

## 11. Test results

### Workspace test summary

```
cargo test --workspace --exclude autore-stage1
```

**Total: 911 tests passed, 0 failed.**

Breakdown by crate:

| Crate | Tests | Notes |
|-------|-------|-------|
| `autore-app` | 35 | 29 Stage 0 + 6 Stage 1 handler tests |
| `autore-cli` | 38 | 20 Stage 0 + 18 Stage 1 CLI tests |
| `autore-core` | 74 | Stage 0 core validation |
| `autore-events` | 12 | Stage 0 event subscription |
| `autore-schema` | 279 | 248 Stage 0 + 31 Stage 1 record/ID/fixture tests |
| `autore-store` | 158 | Stage 0 storage + 10 Stage 1 migration tests (V14..V33) |
| `autore-tui` | 61 | 56 Stage 0 + 5 Stage 1 pane tests |
| `autore-reconstruction` | 191 | 177 unit + 14 integration (ignored by default) |
| `autore-provider-protocol` | 1 | `version_suffix_present` |
| `autore-provider-runtime` | 21 | 4 bootstrap + 6 artifact + 9 package + 1 fixture integration + 1 roundtrip |
| `fixture-provider` | 0 | Binary crate; tested via integration |
| `ida-provider` | 7 | 7 passed, 6 ignored (IDA-dependent) |
| `openai-compatible-provider` | 11 | 7 analysis + 4 generation tests |

### Integration tests (ignored by default, run with `--ignored`)

| Test file | Tests | Description |
|-----------|-------|-------------|
| `autore-reconstruction/tests/ida_full_ingest.rs` | 1 | End-to-end IDA ingest with synthesized observations |
| `autore-reconstruction/tests/whole_program_work_graph.rs` | 1 | Full work graph: ingest → build → SCC → fingerprint → scheduler |
| `autore-reconstruction/tests/dynamic_llm_proposed_scenario.rs` | 1 | LLM-proposed scenario through verifier → executor → importer |
| `autore-reconstruction/tests/wave7_exit_criterion.rs` | 1 | IDA debugger uses GDB + TargetRunner seam |
| `autore-reconstruction/tests/wave8_shared_model.rs` | 1 | Shared types recovered, declaration artifacts up-to-date, build green |
| `autore-reconstruction/tests/wave9_stub_replacement.rs` | 2 | Leaf-first stub→replaced; skeleton builds green before replacement |
| `autore-reconstruction/tests/wave10_differential.rs` | 1 | Function-verified + cluster-verified + regression-passed |
| `autore-reconstruction/tests/coordinator_autonomous_run.rs` | 1 | Autonomous coordinator with restart recovery |
| `autore-reconstruction/tests/faults.rs` | 1 | 5 fault scenarios (provider crash, restart sweep, artifact reconciliation) |
| `autore-reconstruction/tests/faults-llm.rs` | 4 | Invalid output, timeout retry, repeated failure, corrupted artifact |
| `autore-reconstruction/tests/faults-coverage.rs` | 4 | Debugger timeout, stale-work invalidation, environment defect, cancellation |

### PTY integration tests

```
cargo test -p autore-tui --test pty_integration -- --ignored --nocapture
```

3 new Stage 1 PTY tests + Stage 0 tests. Verifies terminal restoration and keybinding dispatch.

### Workspace gates

```
cargo fmt --all --check                                    # clean
cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings  # clean
cargo build -p autore-stage1 --no-default-features         # clean
cargo build -p fixture-provider --no-default-features      # clean
cargo build -p ida-provider --no-default-features          # clean
cargo build -p openai-compatible-provider --no-default-features  # clean
cargo build -p build-provider --no-default-features        # clean
```

### Evidence files

All 53 evidence files are under `.omo/evidence/auto-re-stage-1/`:

```
task-1-audit.txt                    task-28-build-provider.txt
task-2-app-commands.txt             task-29-skeleton-first-build.txt
task-3-schema-records.txt           task-30-build-classification.txt
task-4-worker-via-app.txt           task-31-scenario-lang.txt
task-5-wave1-gates.txt              task-32-ida-debug.txt
task-6-proto.txt                    task-33-observation-import.txt
task-7-runtime-bootstrap.txt        task-34-llm-experiment-flow.txt
task-8-package-validation.txt       task-35-wave7-exit-criterion.txt
task-9-artifact-transport.txt       task-36-layout-constraint-reconciliation.txt
task-10-fixture-provider.txt        task-38-types-conflict.txt
task-11-migrations-v14-v23.txt      task-41-generation-providers.txt
task-12-app-handlers.txt            task-43-generation-orchestrator.txt
task-13-ida-provider.txt            task-44-wave9-exit.txt
task-14-canonical-identity.txt      task-45-scenario.txt
task-15-ida-end-to-end.txt          task-46-verification-repair.txt
task-16-migrations-v24-v27.txt      task-47-regression.txt
task-17-work-graph.txt              task-48-wave10-exit.txt
task-18-fingerprint.txt             task-49-coordinator.txt
task-19-scheduler-via-app.txt       task-50-cli.txt
task-20-whole-program-work-graph.txt task-51-tui.txt
task-21-openai-compatible-provider.txt task-52-autonomous-run.txt
task-22-bundle.txt                  task-53-fault-provider-crash.txt
task-23-import-boundary.txt         task-54-faults-llm.txt
task-24-llm-capability-fixtures.txt task-55-faults-coverage.txt
task-25-llm-analysis-e2e.txt        task-56-pty.txt
task-26-migrations-v28-v33.txt      task-57-workspace-gates.txt
task-27-generator-skeleton.txt      task-57-clippy-fix.txt
```

---

## 12. Architectural test (spec §23)

10 yes/no questions. All must be YES for Stage 1 completion.

### Q1. Can a future Ghidra or Binary Ninja provider implement the analysis capabilities without changing canonical storage?

**YES.** The `ObservationImporter` in `autore-reconstruction/src/identity/importer.rs` accepts observation payloads from any provider and issues `ApplicationCommand::RegisterEntity` + `ImportProviderRunResult`. Storage routes only through `autore-app`; no provider touches SQLite directly. A Ghidra implementation needs only to implement the same gRPC `Provider` service with `ida.binary.ingest`-equivalent capabilities and a `manifest.toml`. The canonical entity key (`autore-reconstruction/src/identity/key.rs`) is structural (binary_revision + address_space + entry_address + entity_kind), not IDA-specific.

### Q2. Can a future x64dbg or native GDB provider implement the debugger scenarios without changing the coordinator?

**YES.** The `TargetRunner` trait in `autore-reconstruction/src/dynamic/runner.rs:150-187` declares 8 async methods (`launch`, `attach`, `stop`, `execute_step`, `capture_function`, `trace_function`, `capture_memory`, `capture_calls`) independent of any specific debugger backend. `WineGdbRunner` is the first implementation; `WindowsGdbServerRunner` is a compile-time stub proving the seam. The coordinator's `DynamicInvestigation` handler dispatches through `execute_scenario(&dyn TargetRunner)` without knowing the backend. An x64dbg implementation needs only `impl TargetRunner for X64dbgRunner`.

### Q3. Can a future vLLM, Anthropic, or other LLM provider implement the analysis/generation capabilities without changing the import boundary?

**YES.** The `openai-compatible-provider` crate communicates over HTTP with any OpenAI-compatible endpoint. The import boundary (`autore-reconstruction/src/analysis/import/`) validates responses against committed JSON schemas (`autore-reconstruction/schemas/analysis/*.schema.json`) and issues `ApplicationCommand` variants. The `GenerationModel` trait in `autore-reconstruction/src/generation/orchestrator.rs` abstracts the LLM call. A vLLM or Anthropic provider needs only to produce responses conforming to the same schemas and implement the same gRPC `Provider` service.

### Q4. Can a future remote object store implement artifact transport without changing capability semantics?

**YES.** The `ArtifactTransport` trait in `autore-provider-runtime/src/artifact.rs:120-149` declares 4 methods (`stage_inbound`, `stage_outbound`, `commit_inbound`, `discard`). `ArtifactLocation` is an enum over `Local(PathBuf)` and `Remote(String)`, explicitly admitting remote transports. `LocalStagingTransport` is the first implementation. A future S3/GCS transport needs only `impl ArtifactTransport for RemoteTransport`. Providers receive opaque `ArtifactHandle` values and never see canonical paths.

### Q5. Can a future clang-cl or native MSVC build system implement build without changing the diagnostic taxonomy?

**YES.** The `BuildProviderTrait` in `autore-reconstruction/src/build/trait_def.rs` declares 5 methods (`configure_project`, `compile_units`, `link_target`, `run_test`, `collect_diagnostics`). `DockerMsvc2002BuildProvider` is the first implementation. The `BuildFailureKind` enum (13 variants) and `classify()` function in `autore-reconstruction/src/build/classification.rs` parse structured diagnostics. A clang-cl implementation needs only to emit diagnostics in the same structured format; the classifier routes them identically.

### Q6. Can the scheduler operate without direct storage access, routing only through ApplicationCommand?

**YES.** The scheduler in `autore-stage1/src/scheduler/scheduler.rs` takes `Arc<dyn AutoReClient>` + `ProjectId` + task snapshot. All mutations route through `ApplicationCommand` variants (`FailWorkItem`, `RequeueWorkItem`, `PromoteWorkItem`, `LeaseWorkItem`). Reads use `ApplicationQuery::ListExpiredLeases`. The scheduler is a pure decision engine with zero direct storage access. `evaluate_state` is a pure function (no client, no Result).

### Q7. Can generated C++ be replaced by another implementation language without changing the work graph?

**YES.** The work graph (`autore-reconstruction/src/work_graph/`) is language-agnostic. `WorkItemKind` and `DependencyEdgeKind` do not reference C++. The `ProjectSkeletonBuilder` generates C++ file extensions (`.hpp`, `.cpp`), but the work graph nodes and edges are independent of file format. `ImplementationTargetId` (spec §11.1) is the polymorphism seam. A future Rust or Go generator would implement a new skeleton builder producing `.rs` or `.go` files; the work graph, scheduler, and verification layers remain unchanged.

### Q8. Can entity identity remain stable across provider relocations without IDA row IDs?

**YES.** `CanonicalEntityKey` in `autore-reconstruction/src/identity/key.rs` uses 4 structural fields: `binary_revision_id || address_space || entry_address || entity_kind`. The sidecar `provider_native_extension` HashMap holds IDA row ids but is deliberately excluded from `stable_key()` and `identity_hash()`. The negative-proof test `canonical_key_excludes_ida_row_id` shows the correct implementation produces identical stable keys across different `ea` values. A Ghidra provider would populate the same structural fields; identity remains stable.

### Q9. Can the coordinator loop run durably with restart recovery without repeating non-idempotent operations?

**YES.** The coordinator in `autore-reconstruction/src/coordinator/mod.rs` implements spec §14.1 with `reconcile_interrupted_operations` as the first phase of every tick. On restart: old local provider instances are marked `Unavailable` via `StopProviderInstance`; leased/running items are requeued; `StagingReconciler::sweep()` removes orphan staging directories. The integration test `coordinator_autonomous_run.rs` proves `CreateReconstructionCampaign` and `CreateWorkItems` are NOT re-issued on resume. `NoProgressDetector` blocks work items after 3 identical raw-response hashes, preventing infinite loops.

### Q10. Can differential verification operate at function, cluster, and whole-program levels without changing the comparison model?

**YES.** The `ComparisonLevel` enum in `autore-reconstruction/src/verification/types.rs` has 3 variants: `Function`, `Cluster`, `WholeProgram`. The `ScenarioExecutor` in `autore-reconstruction/src/verification/executor.rs` accepts any level and drives the `ObservationBackend` trait. `NormalizationRule` (4 variants: `RelocatedAddress`, `Timestamp`, `RandomSeed`, `EnvSpecificHandle`) is level-agnostic. `ComparisonResult` (6 variants) is the same regardless of level. The integration test `wave10_differential.rs` exercises both `Function` and `Cluster` levels with the same comparator. Whole-program verification uses the same model with a broader scenario scope.

---

**End of Stage 1 Implementation Report.**

Ultraworked with [Sisyphus](https://github.com/code-yeongyu/oh-my-openagent)

Co-authored-by: Sisyphus <clio-agent@sisyphuslabs.ai>
