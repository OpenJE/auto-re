---
slug: auto-re-stage-1
status: awaiting-approval
intent: clear
review_required: false
pending-action: write .omo/plans/auto-re-stage-1.md
approach: Treat the user-supplied Stage 1 Vertical Slice Specification as the source of truth. Map it onto the existing Stage 0 + scaffold-stage1 codebase (8-crate workspace, 13 refinery migrations V1..V13, atomic ApplicationCommand+ProjectEvent transactions, 614 default-member tests, Ratatui 7-pane TUI, autore-stage1 AnalysisBackend/WorkerRunner/ModelRouter/Scheduler). Replace AnalysisBackend::analyze()->String, in-process llama_cpp, and gdbstub with typed external providers over a versioned tonic+prost gRPC substrate; route every canonical mutation through autore-app; expand Campaign/Task and the work graph into a whole-program reconstruction campaign with SCC-aware deterministic scheduler; generate one managed C++ tree with explicit stubs; close the loop with build/dynamic/verification feedback; expose via existing CLI + TUI. Adopt the spec's 12-phase implementation sequence verbatim as execution waves. TDD for new code; migrations carry forward-then-rollback tests; TUI PTY test stays isolated; full spec §19 completion-criteria enforced by the final verification wave.
---

# Draft: auto-re-stage-1

## Components (topology ledger)
> Per-cut independence: any one vertical cut can succeed or fail without invalidating the others' interface boundary. 13 components (12 spec phases + cross-cutting recovery). Hardening (Phase 12) is the final verification wave, not a topology component.

| id | outcome (one line) | status | evidence path |
| --- | --- | --- | --- |
| A-provider-substrate | Versioned tonic+prost gRPC provider protocol, runtime bootstrap/sealed-auth/limits/cancellation, local package discovery+validation, fixture provider; one ArtifactTransport abstraction | active | Cut A / Phase 2 |
| B-ida-provider | External IDA-only provider with idax 0.3.0, whole-binary ingestion, native-artifact snapshots, deterministic refresh | active | Cut B / Phase 3 |
| C-work-graph | Whole-program ReconstructionCampaign + expanded work-item kinds + SCC-collapsed dependencies + per-item input fingerprints + scheduler through autore-app commands | active | Cut C / Phase 4 |
| D-llm-provider | External OpenAI-compatible provider, capability-specific typed schemas, bounded investigation bundles, 3-level import boundary (raw → schema-validated → canonical hypotheses), bounded schema-repair | active | Cut D / Phase 5 |
| E-dynamic-investigation | Typed debugger scenario language, IDA debugger → GDB backend execution, typed observation import, target runner abstraction | active | Cut E / Phase 7 |
| F-type-recovery | Shared canonical type/class hypotheses, deterministic constraint model, conflict records + LLM arbitration, per-field-vs-per-layout verification | active | Cut F / Phase 8 |
| G-cpp-generation | One managed `generated/openvb/` source tree, immediate skeleton+stubs, entity↔artifact↔operation↔build-result mappings, controlled staged patching, explicit stub policy | active | Cut G / Phase 6 + 9 |
| H-build-toolchain | ConfigureProject/CompileUnits/LinkTarget/RunTest/CollectDiagnostics abstraction, one toolchain path, structured diagnostic parsing, bounded repair loop | active | Cut H / Phase 6 + 9 |
| I-differential-verification | 3-level (function/cluster/whole-program) scenario capture+replay, typed difference records, repair-tree, regression selection | active | Cut I / Phase 10 |
| J-coordinator | Durable autonomous loop, no-progress detection, completion policy, atomic import per iteration | active | Cut J / Phase 11 |
| K-cli | `auto-re reconstruct § provider § work § generated § build § verification` verbs on stable JSON output via existing ApplicationCommand/Query | active | §15 / Phase 11 |
| L-tui | Extend existing 7-pane dashboard with campaign coverage, work queue, providers, diagnostics, verification diffs; provider/storage ops off the render thread | active | §16 / Phase 11 |
| M-persistence-recovery | Provider installations/instances/runs, snapshots/observations, raw+parsed LLM results, mappings, build/verify attempts persisted; restart reconciles interrupted ops, expires leases, drops uncommitted staging, preserves committed artifacts | active | §17 / Phases 11 + 12 |
| Hdn-hardening | Provider-crash + coordinator-restart + invalid-LLM-output + repair-loop-failure + corrupted-artifact + debugger-timeout + stale-work-invalidation coverage; PTY TUI tests; fmt+clippy(-D warnings) | active | Phase 12 |

## Open assumptions (announced defaults)
> Intent is CLEAR: these are defaults I am adopting instead of asking. Each is reversible unless flagged; the user can veto any at the approval gate.

| assumption | adopted default | rationale | reversible? |
| --- | --- | --- | --- |
| IDA Rust binding | `idax = 0.3.0` already in autore-stage1/Cargo.toml:36 | Spec §6.1 allows `idax or current supported IDA binding`; idax is the binding Stage 0 deliberately chose | LOW — external dep + license-bearing |
| Provider implementation language | Each provider is a separate Rust binary under `providers/{fixture,ida,openai-compatible,build}/` workspace member, off `default-members` so cargo build (no flags) stays clean | Spec §5 mandates external child processes; pure-Rust workspace already; avoids polyglot toolchain boot | LOW — directory move only |
| gRPC framework | `tonic` (server+client) + `prost` (codegen) + `proto/` schema tree; protobuf versioned via package option `autore.provider.v1` | Spec §5.1 mandates "versioned Protobuf and gRPC"; tonic+prost is THE Rust standard | LOW |
| Generated project build system | CMake + Ninja first build adapter (spec §11.1 shows `CMakeLists.txt`) | Spec literally demands CMakeLists.txt | LOW — abstraction admits other adapters later |
| Local LLM endpoint | `llama-server` (from llama.cpp) over loopback OpenAI-compatible REST, started/managed by the openai-compatible provider run; provider config admits any OpenAI-compatible URL+key so Ollama/vLLM/LM Studio also work | Spec §5 requires external OpenAI-compatible; spec §6.2 mandates local; project already chose llama.cpp via `llama_cpp = 0.3.2` (being replaced) | medium — provider config-drives it |
| Identity scheme | `binary_revision || address_space || entry_address || entity_kind || provider_native_ext` — no IDA row ids as canonical ids | Spec §6.5 explicit | LOW |
| Detailed-artifact storage | Two-layer: canonical structural rows + immutable native-artifact snapshots hashed+referenced | Spec §6.4 explicit | LOW |
| Schema migration policy | Additive refinery migrations V14..VN for Stage 1 records; never rewrite V1..V13; V2 → Stage 1 schema is forward-only (Stage 0 already migrated to V2 via V13) | Stage 0 established refinery + additive-only migrations | LOW |
| Application mutation principle | Every Stage 1 canonical mutation routes through NEW ApplicationCommand variants added in Phase 1; autore-stage1's direct WorkerRunner → Claim/EvidenceRepository writes are removed in Phase 1 (replaced with work-item-bound commands) | Spec §3.1 + §4.3 explicit | LOW |
| Test strategy | TDD for new code (providers, schemas, coordinator scenarios, generator); tests-alongside for additive migrations (rollback+idempotency); PTY integration test stays separate (`cargo test -p autore-tui --test pty_integration -- --ignored`); agent-executed QA per todo with happy + failure scenarios + evidence path; spec §19 38-criterion completion gate is the FINAL verification wave's F4 scope-fidelity rubric | Stage 0 plan established this; spec §19 inventory matches | LOW |
| Phase→wave mapping | Spec §18 phases 1..12 verbatim become execution waves 1..12 (one wave each, except Phase 6+9 share Cut G+H todos, and Phase 11 touches all cuts) | Spec ordering already validated by the spec author; not something to re-architect | LOW |
| Cancellation tokens + budgets | Existing tokio-util CancellationToken pattern carried forward; deadlines propagate as tonic per-call timeouts | Stage 0 plan §3.10 / spec §5.5 explicit | LOW |

## Findings (cited - path:lines)
- Cargo.toml:1-51 — 8-crate workspace, resolver=3, edition=2024; workspace deps already include: thiserror, tokio(full), rusqlite(bundled), refinery(rusqlite-bundled), uuid(v4,v7,serde), sha2, blake3, time, schemars(1), jsonschema(0.33), petgraph, crossterm, ratatui(0.30), tempfile, assert_cmd, predicates. Missing for Stage 1: tonic, prost, hyper (for openai-compatible REST client), reqwest (alt), tower.
- autore-stage1/Cargo.toml:6-11 — features `default=["tui"]`, optional `ida` (idax 0.3.0), `gdb` (gdbstub 0.7.8), `llama` (llama_cpp 0.3.2), `tui`. Spec replaces gdbstub+llama_cpp; idax stays as the binding inside the external IDA provider. Workspace member; off default-members.
- autore-stage1/src/lib.rs:1-35 — exports Stage-0 schema symbols (Campaign, Task, Claim, Evidence, etc.) via autore-schema re-export; modules analysis/cli/model/scheduler/worker; engine + store behind `#[cfg(feature="ida")]`. So `autore-schema/src/domain/task/mod.rs` already defines Task state machine (Pending→Ready→Leased→Running→Completed/Failed/Cancelled/Blocked/Stale).
- autore-schema/src/domain/task/mod.rs:79-301 — Task already has dependencies: Vec, required_capabilities, preferred_worker, preferred_model_class, maximum_attempts, attempt_count, input_revision. State machine already implements lease/start/complete/fail/cancel/block/unblock/mark_stale/requeue with proper transition guarding. Spec §4.2 wants this "adapted" into a generalized reconstruction work item — most of the scaffold survives.
- autore-stage1/src/analysis/backend.rs:1-49 — `AnalysisBackend` trait with `async fn analyze(&self, function_id, capability) -> crate::Result<String>`; THIS IS THE `analyze()->String` interface the spec §4.3 marks REPLACE. Capabilities: InventoryFunctions/Disassemble/Decompile/RecoverTypes/ControlFlowGraph/CallGraph — only 6, spec §6.2 needs ~9 capabilities.
- autore-stage1/src/worker/runner.rs:42-80 — WorkerRunner holds Arc<dyn ModelProvider>, Arc<dyn TaskRepository>, Arc<dyn ClaimRepository>, Arc<dyn EvidenceRepository>; runs a single analysis task. THIS is the "direct worker writes through claim/evidence repositories" pattern spec §4.3 marks REPLACE.
- autore-stage1/src/scheduler/scheduler.rs:1-23+ — Scheduler holds ModelRouter; priority_score, score_tasks, run_campaign, evaluate, recover_expired_leases, promote_ready_tasks already present. Spec §4.2 marks this ADAPT: scheduler may decide transitions but request them through autore-app.
- autore-stage1/src/model/router.rs:23 — ModelRouter already exists. Will be ADAPTED to capability+policy-based model selection per spec §4.2.
- autore-app/src/application_service/requests.rs:290-308 — existing 14-variant ApplicationCommand (CreateProject, RegisterArtifact, RegisterEntity, RegisterProvider, StartProviderRun, AddEvidence, AddHypothesis, ChangeHypothesisStatus, RecordContradiction, AddVerification, CancelOperation, ValidateProject, MigrateProject, RebuildIndexes). Stage 1 will add: CreateReconstructionCampaign, CreateWorkItems, PromoteWorkItem, LeaseWorkItem, RenewWorkLease, CompleteWorkItem, FailWorkItem, BlockWorkItem, InvalidateWorkItem, RequeueWorkItem, RegisterProviderInstallation, StartProviderInstance, StopProviderInstance, ImportProviderRunResult, ImportDynamicObservation, RecordBuildAttempt, RecordVerificationComparison, BlockWorkWithReason, etc. (spec §7.6 + §17).
- autore-app/src/application_service/requests.rs:610-648 — AutoReClient trait (execute/query/events_after/subscribe_events) + LocalAutoReClient; TuiState uses RecordingClient test double (TUI doesn't mutate ApplicationService directly). Stage 0 invariant spec §3.1 sits cleanly on top of.
- migrations/V1..V13 — Stage 0 schema complete at V13 (derived_indexes). Stage 1 additive migrations start at V14: `V14__reconstruction_campaigns.sql`, `V15__work_items.sql`, `V16__work_dependencies.sql`, `V17__work_fingerprints.sql`, `V18__work_leases.sql`, `V19__provider_installations.sql`, `V20__provider_instances.sql`, `V21__provider_runs.sql`, `V22__capability_descriptors.sql`, `V23__native_artifacts.sql`, `V24__dynamic_observations.sql`, `V25__llm_raw_responses.sql`, `V26__llm_parsed_results.sql`, `V27__conflict_records.sql`, `V28__generated_source_mappings.sql`, `V29__build_attempts.sql`, `V30__build_diagnostics.sql`, `V31__verification_scenarios.sql`, `V32__verification_comparisons.sql`, `V33__repair_attempts.sql`, `V34__blocked_reasons.sql`. (Allocation may consolidate dependent on Metis review.)
- README.md:1-254 — current schema version 2.0; project.sqlite3 + project.toml + artifacts/<algo>/<prefix>/<digest>/data + packages.lock stub. Artifact kinds: core.binary, core.source-tree, core.native-provider-output, core.configuration, core.log, core.trace, core.generated-candidate. Entity kinds: core.function, core.type, core.global, core.string, core.external-function, core.source-symbol. Stage 1 needs new artifact kinds: provider.protocol-message, raw-llm-response, parsed-llm-result, disassembly-snapshot, decompilation-snapshot, cfg-snapshot, ida-native-snapshot, generated-declaration, generated-definition, generated-test, differential-trace, build-log, debugger-trace.
- docs/stage0-report.md, docs/stage0-audit.md — Stage 0 closure documents already in place; spec §22 requires equivalent Stage 1 report at completion.
- .omo/plans/auto-re-stage-0.md — the Stage 0 plan, completion receipts include dual-review round-3 OKAY (Momus ses_08f76a6a4ffeI6yxvQN5m37lGs + Oracle ses_08f7651d3ffewhmq6z2NOE7PPm). The Stage 0 review cycle is a template for Stage 1's final verification wave.

## Decisions (with rationale)
- Treat the user-supplied Stage 1 Vertical Slice Specification (sections 1-23) as the source of truth; do not re-derive scope or phases. Map spec → plan todos one-to-one.
- Use the spec's 12-phase sequence verbatim as execution waves. Each phase = one wave (5-8 todos); Phase 11 is integration-cross-cutting and may be one wave with sub-batches per Cut-K/L.
- All retained Stage 0 modules keep their public API. All Stage 1 mutations pass through NEW ApplicationCommand variants.
- Replace (per spec §4.3): `AnalysisBackend::analyze()->String` → typed per-capability results over gRPC; in-process `llama_cpp` → external openai-compatible provider against a local LLM server; in-process `gdbstub` → IDA debugger with GDB backend (spec §9.1); placeholder task-graph construction → whole-program ingestion-derived graph (spec §4.3 + §7.3); fixture-only completion → §19 38-criterion gate + §20 Van Buren completion criteria.
- Add crates (per spec §4.4): proto tree for `autore.provider.v1`; `autore-provider-protocol` (generated stubs + capability marker traits); `autore-provider-runtime` (bootstrap, sealed auth, lifecycle, cancellation, limits, package hashing + discovery); `autore-reconstruction` (coordinator, fingerprint, SCC, conflict, scenario, patch-control, mapping, recovery). Provider binaries live under `providers/{fixture,ida,openai-compatible,build}/` as off-default-members workspace members. May begin as modules inside one crate to avoid early churn (spec §4.4 explicitly permits).

## Scope IN
The 23 sections of the user-supplied Stage 1 Vertical Slice Specification. Specifically the §2 vertical-slice matrix (11 future-stage aspects, narrow cut each), the §19 38-item implementation-completion gate, and the §20 11-item Van Buren reconstruction completion criteria (the latter being a campaign-success bar, not an implementation success bar per §20).

## Scope OUT (Must NOT have)
Spec §21 explicit exclusions — do NOT implement: Ghidra / Binary Ninja / multi-backend consensus; public package registry or solver; network package installation; package signing authority; remote workers / distributed scheduling / general remote providers; full sandbox enforcement / containers / microVMs; symbolic execution / concolic execution / general fuzzing; multiple generation languages; build systems beyond the required CMake+Ninja abstraction; arbitrary provider TUI plugins; automatic prompt-training infra; general cross-project knowledge packages; formal whole-program equivalence proofs.
Plus ulw-plan guardrails: no speculative implementations of any deferred item, only the interfaces that admit them later; no human-intervention verification; no manual log interpretation in the inner loop.

## Open questions
Resolved (user answered 2026-07-21):

1. **Build toolchain target** — User: "The system should be able to take any executable, and produce an implementation in any target language. I currently use cmake and cmake with a docker container hosting msvc 2002, but I do not want to force generation into a specific workflow."
   - RESOLUTION: Cut H ships a generic `BuildProvider` capability trait; FIRST impl = external provider wrapping the operator's existing Docker+MSVC+cmake workflow (the conductor launches the docker container, cmake + MSVC compile + link). The trait admits other toolchains (clang-cl cross, MinGW, native MSVC host, future non-CMake systems) without changing generated-entity ownership. Stage 1 product surface is still only the operator's existing flow; abstractions are deliberate. Spec §23 #5 ("Can a second build backend be added without changing generated-entity ownership?") is the exit test.
   - Side-effect on Cut G: "implementation target" is similarly generic. Stage 1 ship = C++ generator only; the `llm.generation.*` capability set is C++-oriented but the framing `ImplementationTargetId` already exists in autore-schema/ids.rs and stays as the polymorphism seam.

2. **Target runner env** — User: "Wine+gdb via IDA's debugger Wine bridge is what I will be doing, but it should be allowed to later use other debugging backends like x64dbg."
   - RESOLUTION: Cut E ships a generic `DebuggerBackend` abstraction; FIRST impl = IDA debugger with GDB backend running on Linux+Wine. The `debug.scenario.execute` capability surface is backend-agnostic; the scenario interpreter validates against the typed scenario language (spec §9.2), and the concrete execution backend is pluggable. Future x64dbg / WinDbg backends add a new provider without changing the coordinator or scenario language.

3. **Van Buren executable acquisition** — User: "Operator supplies path; only content-hash committed".
   - RESOLUTION: `auto-re reconstruct start --binary <path>` registers a `core.binary` artifact from the local file; the file path and bytes stay out of the auto-re repo; only `ArtifactId` + `ContentHash` flow into the campaign. UUID directory `019f7dcf-1cf6-7540-85cc-7b9e8ee18228/` is operator-managed staging area, NOT a van-buren fixture.

4. **Test strategy** — User confirmed recommended default: TDD per todo + per-todo happy/failure QA + spec §19 38-criterion gate as final wave F4.

## Approval gate
status: approved (user resolved all 4 forks; intent CLEAR; review_required=false)
Native Momus / independent Oracle dual review: NOT run (CLEAR + review_required=false; user did not opt in).
Mandatory Metis gap review: TO RUN before delivery; findings folded into plan silently.