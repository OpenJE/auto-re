# Stage 1 Architectural Test (Spec §23)

This document answers the 10 architectural-test questions from spec §23. Each answer must be YES with a code-path citation proving the capability is pluggable through a stable interface.

---

## Q1. Can a future Ghidra provider implement the analysis capabilities without changing canonical storage?

YES. The `ObservationImporter` in `autore-reconstruction/src/identity/importer.rs:30-88` accepts observation payloads from any provider and issues `ApplicationCommand::RegisterEntity` + `ImportProviderRunResult` through the `AutoReClient` trait. Storage routes only through `autore-app`; no provider touches SQLite directly. The canonical entity key (`autore-reconstruction/src/identity/key.rs:20-82`) is structural (`binary_revision_id` + `address_space` + `entry_address` + `entity_kind`), not IDA-specific. A Ghidra implementation needs only to implement the same gRPC `Provider` service with `ida.binary.ingest`-equivalent capabilities declared in a `manifest.toml` and produce observations conforming to the same schema.

---

## Q2. Can a future direct GDB provider implement the debugger scenarios without changing the coordinator?

YES. The `TargetRunner` trait in `autore-reconstruction/src/dynamic/runner.rs:150-187` declares 8 async methods (`launch`, `attach`, `stop`, `execute_step`, `capture_function`, `trace_function`, `capture_memory`, `capture_calls`) independent of any specific debugger backend. `WineGdbRunner` is the first implementation; `WindowsGdbServerRunner` is a compile-time stub proving the seam. The coordinator's `DynamicInvestigation` handler dispatches through `execute_scenario(&dyn TargetRunner)` without knowing the backend. An x64dbg or native GDB implementation needs only `impl TargetRunner for X64dbgRunner` or `impl TargetRunner for NativeGdbRunner`.

---

## Q3. Can a future non-OpenAI model provider implement the LLM capabilities without changing the scheduler?

YES. The `openai-compatible-provider` crate communicates over HTTP with any OpenAI-compatible endpoint (vLLM, Ollama, local servers). The import boundary (`autore-reconstruction/src/analysis/import/mod.rs`) validates responses against committed JSON schemas (`autore-reconstruction/schemas/analysis/*.schema.json`) and issues `ApplicationCommand` variants. The `GenerationModel` trait in `autore-reconstruction/src/generation/orchestrator.rs:121-141` abstracts the LLM call with 4 async methods (`generate_function`, `generate_cluster`, `analyze_failure`, `generate_repair`). The scheduler in `autore-stage1/src/scheduler/scheduler.rs:245-260` reasons about capabilities via `CapabilityDescriptor` (declared in `manifest.toml`), not provider identities. A vLLM or Anthropic provider needs only to produce responses conforming to the same schemas and implement the same gRPC `Provider` service with capabilities like `llm.analysis.function` declared in its manifest.

---

## Q4. Can remote artifact transfer replace local staging without changing capability semantics?

YES. The `ArtifactTransport` trait in `autore-provider-runtime/src/artifact.rs:120-149` declares 4 methods (`stage_inbound`, `stage_outbound`, `commit_inbound`, `discard`). `ArtifactLocation` is an enum over `Local(PathBuf)` and `Remote(String)` at `autore-provider-runtime/src/artifact.rs:59-64`, explicitly admitting remote transports. `LocalStagingTransport` is the first implementation. Providers receive opaque `ArtifactHandle` values and never see canonical paths. A future S3/GCS transport needs only `impl ArtifactTransport for RemoteTransport` returning `ArtifactLocation::Remote(uri)` from `stage_outbound`. The capability semantics (what the provider does) remain unchanged; only the transport mechanism (where bytes live) differs.

---

## Q5. Can a second build backend be added without changing generated-entity ownership?

YES. The `BuildProviderTrait` in `autore-reconstruction/src/build/trait_def.rs:53-75` declares 5 async methods (`configure_project`, `compile_units`, `link_target`, `run_test`, `collect_diagnostics`) generic over build toolchains. `DockerMsvc2002BuildProvider` is the first implementation. The `BuildFailureKind` enum (13 variants) and `classify()` function in `autore-reconstruction/src/build/classification.rs:20-140` parse structured `BuildDiagnostic` records. Generated-entity ownership is tracked via `RegisterGeneratedSourceMapping` commands issued by `ProjectSkeletonBuilder` (`autore-reconstruction/src/generation/skeleton.rs:58-281`), which is build-backend agnostic. A clang-cl or native MSVC implementation needs only to emit diagnostics in the same structured `BuildDiagnostic` format; the classifier routes them identically, and entity ownership remains with the generator that produced the source.

---

## Q6. Can symbolic execution later produce observations through the same import boundary?

YES. The `ObservationBackend` trait in `autore-reconstruction/src/verification/executor.rs:171-291` accepts any backend that implements `capture(scenario, target_artifact_id) -> ObservationSet`. The `ScenarioExecutor` drives original and candidate executions through this backend and records observations via `ApplicationCommand::ImportDynamicObservation`. Symbolic execution would implement `impl ObservationBackend for SymbolicBackend` producing `ObservationSet` records with the same `Observation` structure (`kind`, `address`, `timestamp`, `data`). The import boundary does not care whether observations came from concrete execution, symbolic execution, or static analysis; it only validates the schema and issues canonical commands.

---

## Q7. Can package dependency resolution later wrap the existing local provider-package identity?

YES. Provider identity is declared via `ProviderManifest` in `autore-provider-runtime/src/runtime.rs:24-33` with fields `package_id`, `package_version`, `executable_path`, `content_hash`. The `verify_package_identity()` function at `autore-provider-runtime/src/runtime.rs:224-241` checks that the provider's negotiated `package_id` matches the manifest. Capabilities are declared in `manifest.toml` (see `providers/openai-compatible/manifest.toml`) and advertised via `CapabilityDescriptor` during gRPC negotiation. A future package manager would wrap the local manifest with dependency metadata (e.g., `dependencies = ["ida-sdk-7.7", "python-3.10"]`) and verify them before spawning the provider. The core identity (`package_id` + `version` + `capabilities`) remains unchanged; the package manager is an orthogonal layer that validates prerequisites before the runtime loads the manifest.

---

## Q8. Can stronger sandboxing later wrap provider and target execution without changing provider capabilities?

YES. Provider execution is orchestrated by `ProviderRuntime` in `autore-provider-runtime/src/runtime.rs`, which spawns provider processes, manages gRPC clients, and enforces per-capability concurrency limits via semaphores. Target execution is driven by `TargetRunner` implementations (`WineGdbRunner`, `WindowsGdbServerRunner`) in `autore-reconstruction/src/dynamic/runner.rs`. Both are trait-based seams. A future sandboxing layer (gVisor, Firecracker, SELinux) would wrap the process spawn in `ProviderRuntime::spawn_provider()` and the target launch in `TargetRunner::launch()` without changing the trait signatures. Provider capabilities (what the provider can do) are declared in `manifest.toml` and negotiated via gRPC; sandboxing (how the provider is isolated) is an orthogonal concern. The `ArtifactTransport` trait already abstracts staging directory access, so sandboxed providers would use the same transport interface with a sandbox-aware `LocalStagingTransport` implementation.

---

## Q9. Can additional verification techniques produce records through the existing verification model?

YES. The `VerificationRecord` struct in `autore-schema/src/domain/records.rs:1123-1150` has fields `subject: VerificationSubject`, `check: NamespacedId`, `state: VerificationState`, `evidence: Vec<EvidenceRecordId>`, `details: Option<ExtensionData>`. The `VerificationSubject` enum admits `Entity`, `Hypothesis`, `Artifact`, `GenerationTarget`, and other subject types. The `check` field is a `NamespacedId` (e.g., `core.artifact.hash`, `verification.build`, `verification.abi.layout`), allowing arbitrary verification techniques to register new check kinds. The `ComparisonLevel` enum in `autore-reconstruction/src/verification/types.rs:73-80` has 3 variants (`Function`, `Cluster`, `WholeProgram`), and `NormalizationRule` (4 variants) is level-agnostic. The `ScenarioExecutor` in `autore-reconstruction/src/verification/executor.rs:171-291` accepts any level and drives the `ObservationBackend` trait. A future symbolic-verification or fuzzing technique would issue `ApplicationCommand::AddVerification(AddVerificationRequest { record: VerificationRecord { check: NamespacedId::parse("verification.symbolic").unwrap(), ... } })` and store results through the same model.

---

## Q10. Can every current shortcut be identified as an implementation behind a stable interface rather than a semantic assumption embedded in the core?

YES. Every operational capability routes through a trait or command variant:

- **Analysis**: `AnalysisBackend` trait (`autore-stage1/src/analysis/backend.rs:33-48`) with `capabilities()`, `inventory()`, `analyze()` methods.
- **Debugging**: `TargetRunner` trait (`autore-reconstruction/src/dynamic/runner.rs:150-187`) with 8 async methods.
- **Generation**: `GenerationModel` trait (`autore-reconstruction/src/generation/orchestrator.rs:121-141`) with 4 async methods.
- **Build**: `BuildProviderTrait` (`autore-reconstruction/src/build/trait_def.rs:53-75`) with 5 async methods.
- **Verification**: `ObservationBackend` trait (`autore-reconstruction/src/verification/executor.rs`) with `capture()` method.
- **Artifact transport**: `ArtifactTransport` trait (`autore-provider-runtime/src/artifact.rs:120-149`) with 4 methods.
- **Mutations**: All route through `ApplicationCommand` enum (`autore-app/src/application_service/requests.rs:637-682`) with 30+ variants.
- **Scheduler**: Pure decision engine in `autore-stage1/src/scheduler/scheduler.rs:245-411` that issues `ApplicationCommand` variants (`FailWorkItem`, `RequeueWorkItem`, `PromoteWorkItem`, `LeaseWorkItem`) through `AutoReClient` trait with zero direct storage access.
- **Coordinator**: `WorkKindHandlers` trait (`autore-reconstruction/src/coordinator/handlers.rs:103-131`) with 7 handler methods; the coordinator dispatches through `dispatch()` without knowing implementation details.

No semantic assumption is embedded in the core; every capability is an implementation behind a stable interface (trait, command variant, or canonical record). The core (`autore-app`, `autore-schema`, `autore-store`) knows only about commands, queries, events, and records; it does not know about IDA, GDB, OpenAI, Docker, or any specific provider technology.

---

**End of Architectural Test.**

Ultraworked with [Sisyphus](https://github.com/code-yeongyu/oh-my-openagent)
