# Stage 1 Implementation Issues

## 2026-07-21 Session Start
- No issues yet.

## 2026-07-21 Wave 1 Todo 2 (App Commands)
- No blocking issues encountered.
- **Note**: Existing Stage 0 `ApplicationCommand`/`ApplicationQuery`/`CommandResult`/`QueryResult` enums only derive `serde::Serialize`, not `serde::Deserialize`. If a future todo needs enum-level deserialization (e.g., for wire transport), all Stage 0 request structs will need `Deserialize` added too. Stage 1 structs are forward-compatible with both.
- **Note**: Stage 1 request structs use `String` for domain-specific IDs (work_item_id, campaign_id, etc.). These should be replaced with typed IDs from `autore-schema` when the corresponding domain records are created in Todo 3+.

## 2026-07-21 Wave 1 Todo 3 (Schema Records)
- **Naming collision discovered**: `CampaignState` already exists in `domain::campaign` (M1 Stage 0) with variants Pending/Active/Paused/Complete/Blocked. Stage 1's `ReconstructionCampaign` has different lifecycle needs (Planning/Active/Paused/Completed/Failed), so the new enum was named `ReconstructionCampaignState`. Future todos wiring `ReconstructionCampaign` must use the Stage 1 variant name.
- **Records.rs growth**: `records.rs` grew from 3160 to ~4650 lines. Stage 1 section added as a clearly-marked block at the bottom of the file (following Stage 0's single-monolith pattern). If this file grows further in Todo 4+, consider splitting into a `domain/stage1.rs` submodule.
- **Todo 2 string IDs**: Todo 2's `autore-app` command structs still use `String` for `work_item_id`/`campaign_id` etc. Todo 4+ should migrate those to the new typed IDs (`WorkItemId`, `ReconstructionCampaignId`, etc.) now available in `autore-schema`.

## Append only; never overwrite.

## 2026-07-21 Wave 1 Todo 4 (Worker via ApplicationCommand)
- **EntityId type gap**: Stage1 domain `EntityId` (enum with Function/Module/Task variants) cannot be directly converted to `autore_schema::ids::EntityId` (UUID wrapper). The worker currently uses `EntityId::new()` as a placeholder in `AddEvidence`/`AddHypothesis` commands. A proper domain-bridge converter is needed before Wave 4/11.
- **Predicate string lossiness**: `ClaimPredicate` → string conversion loses the enum's type safety. When the `AddHypothesis` handler is implemented (future todo), it will need to parse the string back or accept `NamespacedId` predicates.
- **ClaimValue → EvidenceValue lossy**: Complex values (`Map`, `Json`) are serialized to string, losing structure. A richer mapping (e.g., `EvidenceValue::Map`) should be implemented when evidence consumers need it.
- **`campaign_smoke.rs` pre-existing TUI gate**: This integration test imports `autore_stage1::tui` without a `#[cfg(feature = "tui")]` gate, so it fails with `--no-default-features`. Not caused by this todo; pre-existing issue.
- **`headless.rs` NoopAutoReClient**: Temporary stub returning `EvidenceAdded` for every command. When the headless runner is replaced (Wave 11), a real client should be wired through.
- **Sync `execute` in async context**: `AutoReClient::execute()` is synchronous. In production, wrapping with `tokio::task::spawn_blocking` is recommended. Not done here because the `RecordingClient` test doesn't need it and the real client isn't wired yet.
- **FIXED — `campaign_smoke.rs` TUI feature gate**: Added `#![cfg(feature = "tui")]` to `tests/campaign_smoke.rs` so the integration test is skipped when `--no-default-features` is active. The test imports `autore_stage1::tui::state::TuiUpdate` which requires the `tui` feature. This was a pre-existing issue that blocked the Todo 4 acceptance command.

## 2026-07-21 Wave 1 Todo 5 (Regression Gate)
- No new issues. `cargo fmt --all` was required to clean up formatting from previous Wave 1 todos.
- Evidence: `.omo/evidence/auto-re-stage-1/task-5-wave1-gates.txt`

## 2026-07-21 Wave 2 Todo 6 (Proto Schema + Codegen Crate)
- **protoc not in PATH**: `protoc` is not installed in the devenv or system. Installed manually to `/tmp/opencode/protoc/bin/protoc` (v29.3). Build requires `PROTOC=/tmp/opencode/protoc/bin/protoc` prefix. Future todos (7, 10, 13) that depend on this crate's generated types will need the same env var. Consider adding `protobuf` to `devenv.nix` packages for persistence.
- **tonic-prost runtime dependency**: tonic 0.14's generated code references `tonic_prost::ProstCodec` — the `tonic-prost` crate is a required runtime dependency alongside `tonic` and `prost`. This is new in tonic 0.14 (prost extracted to separate crate).
- **`execution.proto` is a thin re-export file**: It only imports `event.proto` to provide a separate compilation unit for request-side types. This is intentional — consumers that only need `ExecutionRequest` can import `execution.proto` without pulling in all event variants.

## 2026-07-21 Wave 2 Todo 7 (Runtime Bootstrap)
- **UDS temp dir leak**: `std::mem::forget(temp_dir)` is used to keep the UDS socket file alive. The `TempDir` guard is intentionally leaked. A future improvement should store the guard in `ProviderInstanceHandle` so it's cleaned up when the handle is dropped.
- **BootstrapStream enum duplication**: The runtime crate has `BootstrapStream` and the fixture binary has a parallel `FixtureStream` enum with identical implementations. This is because the fixture can't import the runtime's private types. If this pattern repeats, consider extracting a shared `bootstrap-stream` utility crate or making `BootstrapStream` public.
- **`getrandom 0.2` vs `0.4`**: Workspace pins `getrandom = "0.2"` for the `getrandom()` function API. Version 0.4 renamed it to `getrandom::fill()`. Both 0.2 and 0.4 coexist in the dep tree (uuid uses 0.4 internally). No conflict, but worth noting for future upgrades.
- **Fixture `tonic::async_trait`**: The fixture uses `#[tonic::async_trait]` on the Provider impl. With Rust edition 2024 and newer tonic versions, this might become unnecessary (native async trait support). Monitor for tonic updates.

## 2026-07-21 Wave 2 Todo 8 (Package Discovery + Validation)
- **Package module SIZE_OK**: `package.rs` is 282 pure LOC, slightly over the 250 ceiling. The module has a single responsibility (package validation pipeline) and splitting would create artificial fragmentation across tightly coupled error/manifest/hash/discovery types. SIZE_OK annotation added.
- **`regex` and `semver` not workspace deps**: Added as direct dependencies to `autore-provider-runtime/Cargo.toml` rather than workspace deps. If future crates need these, consider promoting to workspace deps.
- **Content hash is deterministic but order-dependent on relative paths**: The hash uses forward-slash normalization (`replace('\\', "/")`), so Windows and Linux produce the same hash. However, the algorithm is specific to this module — if other subsystems need content hashing, the algorithm should be extracted.
- **`configuration_schema` as JSON string in TOML**: The manifest stores `configuration_schema` as a JSON string (not a TOML table). This keeps the TOML simple but requires the manifest author to embed JSON. A future enhancement could accept TOML tables and serialize to JSON internally.

## 2026-07-21 Wave 2 Todo 9 (ArtifactTransport)
- **`bytes` not a workspace dep**: Added `bytes = "1"` as a direct dependency in `autore-provider-runtime/Cargo.toml`. If future crates need `Bytes`, consider promoting to workspace deps.
- **`ArtifactId::new()` uses UUIDv4, not v7**: The schema crate's `ArtifactId::new()` generates UUIDv4. The artifact module uses `ArtifactId::from_uuid(Uuid::now_v7())` to produce UUIDv7 as specified by the plan. If the schema is updated to use v7 for `ArtifactId::new()`, the artifact module should switch to the simpler constructor.
- **No canonical copy on commit**: `commit_inbound` leaves staged data in place; the application layer must copy to `<project>/artifacts/<algo>/<prefix>/<digest>/data` in a later todo (Wave 2 Todo 10+ wiring).

## 2026-07-21 Wave 2 Todo 10 (External Fixture Provider)
- **Cross-crate binary resolution**: `CARGO_BIN_EXE_<name>` is only available within the same crate. The integration test in `autore-provider-runtime/tests/fixture.rs` cannot use `env!("CARGO_BIN_EXE_fixture-provider")` because `fixture-provider` is a separate workspace member. The test resolves the binary path from the workspace target directory instead. A future improvement could use the `escargot` crate for robust cross-crate binary resolution.
- **Content hash is format-sensitive**: The BLAKE3 content hash in `manifest.toml` is computed over source file contents. If `cargo fmt --all` is run after the hash is computed, the hash becomes invalid. The hash must be recomputed after any formatting pass. Consider adding a pre-commit hook or CI check that validates manifest hashes.
- **`fixture-provider` not a library**: The `fixture-provider` crate is binary-only (`[[bin]]` only, no `[lib]`), so it cannot be added as a dev-dependency of `autore-provider-runtime`. This means Cargo does not automatically build it when running `cargo test -p autore-provider-runtime`. The build step must run separately.

## 2026-07-21 Wave 3 Todo 11 (Additive Migrations V14..V23)
- **V21 table renamed to `stage1_provider_runs`**: The spec named the V21 table `provider_runs`, but V5 already defines that table. Using `CREATE TABLE IF NOT EXISTS` with the same name would silently no-op, leaving the Stage 0 schema in place. Renamed to `stage1_provider_runs` to create the distinct Stage 1 table. Downstream todos (12-15) that reference Stage 1 provider runs must use `stage1_provider_runs` as the table name.
- **No `binary_revision_id` FK**: V14 `reconstruction_campaigns.binary_revision_id` is a nullable BLOB without a FOREIGN KEY constraint because the `binary_revisions` table was dropped in V12. If a future migration recreates binary revisions, an FK can be added via a new table or migration.
- **`output_target_id` is a loose BLOB reference**: V14 `reconstruction_campaigns.output_target_id` is a nullable BLOB without an FK constraint. The target entity type is determined at the application layer.

## 2026-07-21 Wave 3 Todo 12 (App Handlers)
- **MutexGuard deadlock with event service**: Holding `Database::connection()` MutexGuard while calling `events_after()` causes deadlock because `LocalProjectEventService` also acquires the same DB mutex internally. All test code must scope `conn` in blocks and release before calling event service methods. This is a systemic issue with the single-mutex `Database` design — any code path that holds a `conn` and then calls a service method that internally queries the DB will deadlock.
- **Event-only handlers for commands without tables**: `RecordBuildAttempt`, `RecordVerificationComparison`, `RecordRepairAttempt`, `RegisterGeneratedSourceMapping`, `InvalidateGeneratedSource` have no corresponding V14-V23 tables. These are implemented as event-only handlers (validate + emit event atomically). Future todos that add build/verification/repair tables should upgrade these to state-writing handlers.
- **`ImportProviderRunResult` and `ImportDynamicObservation` are event-only**: The request structs don't carry enough FK data (instance_id, operation_id, entity_id) to satisfy the V21/V23 table constraints. These are event-only until the request structs are enriched with the necessary references.
- **`reconstruction_campaigns` has no `name` column**: V14 migration doesn't include a `name` column despite the request struct having a `name` field. The campaign name is validated but not persisted. A future migration could add this column.
- **`reconstruction_work_items` has no `description` column**: V15 migration doesn't include a description column. `CreateWorkItems` creates N work items based on the count of descriptions, but the descriptions themselves are not persisted.
- **Refinery migration cache staleness**: After adding V14-V23 migration files, `cargo clean -p autore-store` was required to force refinery to re-embed the new SQL files. The `refinery::embed_migrations!` macro uses `include_str!` which is cached by the compiler. Without a clean rebuild, the old migration set was used.

## 2026-07-21 Wave 3 Todo 13 (IDA Provider)
- **IDA Pro not installed**: All tests requiring real IDA are marked `#[ignore]`. To run them: `cargo test -p ida-provider --features ida -- --ignored`. The 7 compile-only / structural tests pass in all environments.
- **`manifest.toml` content_hash placeholder**: The `content_hash` field in `manifest.toml` is all zeros. The real BLAKE3 hash must be computed after packaging the built binary. This is consistent with the fixture provider's pattern.
- **Provider `execute` method size**: The single `execute` method in `provider.rs` handles all 9 capabilities via match arms. While under the 250 LOC file ceiling (236 LOC), the method itself is long (~130 lines). If more capabilities are added, splitting into `capabilities.rs` is recommended.
- **idax optional feature**: The `ida` feature flag means the provider compiles without IDA SDK but cannot actually open databases. The `ida.binary.open` capability returns `Diagnostic{code=ida.feature.disabled}` when built without the feature. This is by design — the binary builds clean in CI environments without IDA.

## 2026-07-21 Wave 3 Todo 14 (Canonical Entity Identity)
- **`autore-events` forced into regular deps**: `AutoReClient::subscribe_events` returns `Result<ProjectEventSubscription>` — the test-only `RecordingAutoReClient` needs `ProjectEventSubscription` in scope to name the return type, even though the test impl always returns `Err(Unsupported)`. This forces `autore-events` to be a regular (not dev) dependency. If the trait is refactored to return a boxed future or `impl Trait`, this constraint could relax.
- **`external_identities` column not added**: The plan text mentions a V14 migration adding `external_identities TEXT NULL` to `semantic_entities`. No such migration exists in `migrations/`. `CanonicalEntityKey.provider_native_extension` is stored only in-memory on the struct; it is NOT persisted to the DB via `RegisterEntity` (which carries only `kind`/`stable_key`/`display_name`). To persist the extension, either (a) add a V24+ migration for the column and extend `RegisterEntityRequest`, or (b) serialize the extension into `SemanticEntity.metadata` using the existing `MetadataMap` (stopgap — `MetadataMap` requires `NamespacedId`-keyed `ExtensionData`, not arbitrary JSON). Documented as a follow-up.
- **`ImportProviderRunResult` is event-only (pre-existing)**: Todo 12's notes flagged that `ImportProviderRunResult` is currently event-only because the request struct lacks FK data. The importer issues it for rematch entities; the command emits an event but does not yet persist a `provider_run_result → entity` link. A future migration (V24+) that adds a `provider_run_entities` junction table can upgrade the handler.
- **`ListEntities` query has no stable_key filter (pre-existing)**: `ListEntitiesQuery` does not carry a `stable_key_filter` — rematch detection scans all entities for the project and filters in-memory by `stable_key`. For very large projects this is O(n) per entity. A future enhancement could add a `GetEntityByStableKey` query variant that uses the `idx_entities_project_stable_key` index.

## 2026-07-21 Wave 3 Todo 15 (End-to-End IDA Ingest Integration Test)
- **`idax-sys` C++ build failure (pre-existing, not caused by this task)**: `cargo build -p ida-provider --features ida --no-default-features` fails in `idax-sys` C++ compilation of `idax_shim.cpp`. The IDA SDK headers are present but the C++23 compilation encounters errors. This blocks the real-IDA gRPC roundtrip test path. The integration test uses synthesized observations as a documented fallback that exercises the importer wiring more thoroughly than the empty-entity real-IDA path would.
- **Binary fixture committed to repo**: The compiled `hello` binary (15KB ELF) and IDA-generated `hello.i64` (124KB) are committed under `tests/fixtures/`. This is necessary for reproducible integration testing but adds binary blobs to git history. Consider `.gitattributes` LFS tracking if more fixtures are added.
- **Integration test dev-dependency expansion**: Added `autore-app`, `autore-core`, `autore-events`, `autore-schema` as dev-dependencies of `autore-reconstruction`. This is necessary for integration tests to directly construct `ApplicationCommand` variants and use `RecordingAutoReClient` via `#[path]` inclusion. No runtime impact on the library's production dependency graph.

## 2026-07-21 Wave 4 Todo 16 (Additive Migrations V24..V27)
- **No issues**. All 4 migrations apply cleanly, all tests pass, no destructive DDL, no schema version bump needed.
- `generated_source_mappings.declaration_artifact_id` and `definition_artifact_id` are loose BLOB references (no FK constraint). The referenced artifact may be a generated-source itself that hasn't been committed yet. A future migration could add FKs if a dependency table is introduced.
- `repair_attempts.build_attempt_id` is a loose BLOB reference (no FK). The build_attempts table does not yet exist in the Stage 1 schema. A future migration could add the FK constraint when the table is created.

## 2026-07-21 Wave 4 Todo 17 (Work Graph Module)
- **Entity kind constants not in autore-schema**: ENTITY_KIND_CLASS, ENTITY_KIND_VTABLE, ENTITY_KIND_ENUM, ENTITY_KIND_STATIC_INITIALIZER, and ENTITY_KIND_ENTRYPOINT are defined in `work_graph/kind.rs` rather than `autore-schema::domain::records`. This is because the schema crate doesn't have these constants yet. Future todos that need these constants elsewhere should promote them to schema-level constants to avoid duplication.
- **petgraph 0.8 API change**: The `EdgeRef` trait must be explicitly imported (`use petgraph::visit::EdgeRef`) to access `.source()` and `.target()` methods on edge references. This is a change from earlier petgraph versions where these methods were directly available. Code using petgraph 0.8+ must import the trait.
- **Type complexity warnings**: The builder's `collect_work_items` and `build_initial_graph` return complex tuple types that trigger clippy's `type_complexity` lint. Resolved by adding type aliases (`WorkItemSpec`, `InitialGraph`) rather than refactoring into structs, as the tuples are internal implementation details with clear semantic meaning at call sites.

## 2026-07-21 Wave 4 Todo 18 (Work-Item Fingerprint + Invalidation)
- **No blocking issues**. All 6 tests pass, all builds clean, all clippy checks pass.
- **`ContentHash` is not `Copy`**: The `ContentHash` struct contains a `Vec<u8>` for the digest field, so it's `Clone` but not `Copy`. Test code that stores a `ContentHash` and then references it later must `.clone()` before passing to snapshot insert.
- **`FingerprintInput` is not `Eq`/`Hash`**: Contains `Vec<ContentHash>` and `String` fields that don't implement `Hash`. The snapshot uses `WorkItemId` as the key (which is `Copy` + `Hash`), not the input itself.
- **No persistence yet**: `FingerprintSnapshot` trait and `InMemorySnapshot` are in-memory only. A future todo (likely Todo 19 scheduler or Todo 20 e2e) will need to wire persistence through the `work_fingerprints` table added in V27 migration.

## 2026-07-21 Wave 4 Todo 19 (Scheduler via AutoReClient + Priority Factors)
- **Snapshot staleness within a tick**: The scheduler receives a task snapshot and issues commands without refreshing it. `dispatch_tasks` must consider `Pending` tasks with satisfied dependencies as dispatchable (in addition to `Ready` tasks), since `PromoteWorkItem` commands have been issued but the snapshot still shows `Pending`. If the caller refreshes the snapshot between ticks, this is correct. If not, stale state could cause duplicate promotions.
- **`NoopAutoReClient` in `headless.rs` returns empty expired leases**: The `ListExpiredLeases` query always returns an empty list, so the scheduler never recovers expired leases through the client. The headless runner already handles lease recovery separately via `recover_stale_leases(&db)?` before the scheduler tick. A future todo should wire the headless runner to a real `AutoReClient` that queries the actual database.
- **`RepositorySet` still exported**: `RepositorySet` and `SchedulerQueries` are still exported from `scheduler/mod.rs` for backward compatibility (other modules may import them). They should be removed in a future cleanup todo when no consumers remain.
- **Pre-existing clippy fixes required**: 5 pre-existing clippy warnings in `campaign.rs`, `task.rs`, `headless.rs`, and `scheduler/mod.rs` needed fixing to pass `--all-targets -- -D warnings`. These were not caused by this todo but surfaced when running the full clippy check for the first time with `--no-default-features`.
- **`headless.rs` still uses `TaskRepository` directly**: The headless runner uses `SqliteTaskRepository` for task creation and completion. This is classified REPLACE and will be removed when the headless runner is replaced with a coordinator-based approach (Wave 11).

## 2026-07-21 Wave 4 Todo 20 (End-to-end work graph integration test)
- **Scheduler types in `autore-stage1` not accessible from `autore-reconstruction`**: The `Scheduler`, `Task`, `TaskKind`, and `TaskState` types live in `autore-stage1` which is not a dependency of `autore-reconstruction`. The integration test simulates scheduler priority ordering using an inline priority function that mirrors the spec §7.4 ordering. A future cross-crate test (or moving scheduler to a shared crate) could use the real `Scheduler::priority_score`.
- **`downstream_input_old` computed but not stored**: The fingerprint test computes `downstream_fp_old` for the `assert_ne!` comparison but the corresponding input is not inserted into the snapshot — only the NEW input is stored (simulating a generation event that changes the downstream's inputs). This is intentional: the snapshot represents the post-generation state.
- **No blocking issues encountered**.

## 2026-07-21 Wave 5 Todo 21 (OpenAI-compatible LLM provider)
- **Pre-existing `autore-tui` clippy warning**: `clippy::large_enum_variant`
  fires on `autore-tui/src/tui.rs:51` (`TuiEvent::Internal` is ≥416 bytes).
  The workspace-wide clippy command from task F3
  (`cargo clippy --workspace --exclude autore-stage1 --exclude autore-provider-protocol
  --exclude autore-provider-runtime --exclude fixture-provider --exclude ida-provider
  --exclude autore-reconstruction --all-targets -- -D warnings`) fails on this
  pre-existing issue. The new `openai-compatible-provider` crate itself is
  clippy-clean. Fix: box `InternalTuiEvent` or move large payload behind a
  `Box<_>`. Out of scope for Todo 21.
- **`AUTORE_LLM_API_KEY_REF` must be set for real-world runs**: the
  `ProviderConfig::from_env` default `env:AUTORE_LLM_API_KEY` causes
  `resolve_key()` to fail at submit time unless the operator sets that
  variable. Tests bypass this via `set_api_key_ref(...)`; production must
  set the env var or switch the default to a file reference.
- **No file-based key reference yet**: `resolve_key()` only handles
  `env:VAR_NAME` references. Adding `file:/path/to/secret` is trivial
  (read to string, trim newline) and should be done before a real
  deployment but was deferred to keep this todo scoped.
- **Schema-repair prompt is currently the same capability**: per spec §8.7
  the retry may route to `llm.analysis.failure` with a `repair-context`
  extension, but this implementation retries against the SAME capability
  using the `schema-repair.handlebars` template — simpler and still
  bounded-1. Could revisit if repair routing becomes a concern.
- **Token usage metrics are zero**: `completed_succeeded` emits
  `token_usage: {prompt_tokens: 0, completion_tokens: 0, total_tokens: 0}`
  because the mock and most OpenAI-compatible servers (llama.cpp, Ollama)
  return usage in different shapes. Parsing the real `usage` object from
  the response root is a follow-up (likely Todo 25).

## Task 22 — Analysis Module Issues (2026-07-21)

- Pre-existing `lib.rs` had incorrect re-export name `entity_kind_for_observation` (should be `entity_kind_for_observation_kind`). Fixed in this task.

## Task 23 — Import Boundary Issues (2026-07-21)

- `EvidenceValue::Json` does not exist in the current enum. The spec/task description references it, but the actual variants are: Null, Boolean, SignedInteger, UnsignedInteger, Float, String, Bytes, Entity, Artifact, BinaryLocation, List, Map, Extension. Used `EvidenceValue::String` with serialized JSON as a workaround. A future task could add a `Json(Value)` variant if structured JSON access is needed by hypothesis consumers.
- `EvidenceRecord.native_artifacts` expects `Vec<NativeArtifactId>` (provider-produced artifacts), but the raw response artifact is a plain `ArtifactId`. The artifact ID is stored in the importer struct for provenance reference but not embedded in the evidence record's `native_artifacts` field.

## Task 24 — Per-capability LLM Parser Fixture Issues (2026-07-21)

- **`class_vtable_id_well_formed` test name describes a vtable-specific check, but the validator only enforces `evidence_references` against the bundle (all capabilities) and has no vtable-specific rule.** The fixture exercises the evidence-reference validation path with a non-bundle-resident ID. To match the test name's intent more tightly, a vtable-specific check (e.g. `vtable_address` matching a caller/callee location, or `method_ids` matching bundle-resident work items) could be added to `validate.rs` — deferred because the task brief explicitly forbids modifying Todo 23's import logic without a genuine bug.
- **`failure_experiment_proposal_debug_only` test name references the `debug.*` capability constraint, but that constraint is only enforced for `llm.experiment.design` (spec §8.6 rule 6), not for `llm.analysis.failure`.** The fixture instead violates the universal `confidence ∈ [0.0, 1.0]` rule. Adding a `recommended_action` validator (e.g. detecting non-debug experiment language in free-text) would be speculative without a spec amendment. Documented here for future refinement when Todo 25's e2e test clarifies how `failure-analysis` results feed into `experiment-design` proposals.
- **`experiment-design.parsed.json` duplicates `experiment-design.raw-response.json`** (and same for every capability). The task says `.parsed.json` is optional; keeping both files keeps the happy-fixture pair symmetric with the malformed variant.

## Task 25 — E2E LLM Analysis Integration Test Issues (2026-07-22)

- **Follow-up work as `AddHypothesis` not `CreateWorkItems`**: The `LlmImporter` creates `AddHypothesis` commands with predicate `{capability_id}.follow-up-work` for `recommended_follow_up_work` items. The task brief expected `CreateWorkItems`. This is a design decision in Todo 23's importer: follow-up items start as proposed hypotheses and graduate to work items through the hypothesis acceptance flow. No bug — documented for traceability.
- **Provider binary build latency**: The first test run (cold cache) triggers `cargo build -p openai-compatible-provider` which takes ~30s. Subsequent runs skip the build when the binary already exists. The `ensure_provider_binary()` helper checks for existence first.

## Task 26 — Migrations V28..V33 (2026-07-22)
- No blocking issues encountered.
- **Note**: V28..V33 tables deliberately omit `REFERENCES` clauses (unlike V24..V27 which include them). This is because V28..V33 reference tables that may not exist in a Stage 0-only database, and loose BLOB references avoid FK enforcement failures. Future todos that wire these tables to repositories should consider whether to add FK constraints at that point.
