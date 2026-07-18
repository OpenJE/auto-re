# Learnings — Task 5: Stage 0 Typed IDs

## 2026-07-17

- **UUIDv7 feature**: `uuid` v1 already has `v7` feature. The `now_v7()` constructor produces time-ordered UUIDs that sort chronologically by string comparison.
- **`#[serde(tag = "...")]` is for enums only**: Applying it to a struct adds an extra type tag field. For structs, normal field-level serde derives produce the expected `{"algorithm":"sha256","digest":"..."}` format.
- **`serde::Deserialize` trait must be in scope**: Using `String::deserialize(d)` requires the trait imported. Use `<String as serde::Deserialize>::deserialize(d)` or add `use serde::Deserialize;`.
- **time crate needs `parsing` + `formatting` features**: The base `serde` feature does NOT include `OffsetDateTime` serde support.
- **Ambiguous glob imports**: When two modules export the same name, consumers using both globs get warnings. Fix: explicit `use domain::EntityId;`.
- **EntityId dual existence**: M1 `domain::evidence::EntityId` is a semantic enum. Spec `ids::EntityId` is a UUIDv7 newtype. Both coexist; M1 takes precedence at crate root.
- **ContentHash backward compatibility**: `from_bytes(data)` preserved as alias for `blake3(data)`.
- **Custom hex encode/decode**: Inline helpers instead of `hex` crate dependency.

## 2026-07-17 (Task 6)

- **NamespacedId needs Ord for BTreeMap keys**: `BTreeMap<K, V>` requires `K: Ord`. Added `PartialOrd, Ord` derives to `NamespacedId(String)` — trivially valid since `String: Ord`.
- **`serde(tag = "kind", content = "value")` adjacently tagged format**: Unit variants serialize as `{"kind":"Null"}` (no content key). Newtype variants serialize as `{"kind":"Boolean","value":true}`.
- **`<f64 as serde::Deserialize>::deserialize(d)`**: When the `Deserialize` trait isn't imported in scope, use the fully-qualified syntax for custom deserialize helpers.
- **serde_json doesn't produce NaN/Inf from JSON**: Standard JSON has no NaN/Inf literal, so `serde_json` never produces them during parsing. The `deserialize_with` guard is a belt-and-suspenders measure for non-JSON deserializers.
- **`EvidenceValue::Float` cannot implement `Eq`**: `f64` doesn't implement `Eq` (NaN != NaN). The enum uses `PartialEq` only, matching the existing `ClaimValue` pattern.
- **`MetadataMap` inner map stays private**: `#[serde(transparent)]` on the newtype allows serde access without exposing the field. Public API is `get`, `insert`, `iter`, `len`, `is_empty`, `contains_key`.

## 2026-07-17 (Task 7)

- **`include_str!` requires files at compile time**: Fixture files must exist before `cargo test` compiles. Create placeholder files first, generate real content via a temporary test, then update.
- **Closed enums with `#[serde(tag = "kind")]` for unit variants**: `DerivationMethod` uses internally tagged serde (unit variants → `{"kind":"DirectObservation"}`). `StableEntityKey` uses adjacently tagged (`{"kind":"BinaryLocation","value":{...}}`). Both work cleanly with serde's derive macros.
- **Struct-level serde `#[serde(tag)]` adds unwanted type field**: `Derivation` is a struct (not an enum), so `#[serde(tag = "kind")]` would inject a spurious `"kind"` field. Structs use normal field-level derives — `Derivation` serializes as `{"method":{...},"operation":"...","supporting_evidence":[...],"source_hypotheses":[...]}`.
- **Type-system as API contract**: `BinaryLocation::new(artifact, module: ModuleIdentity, rva)` replaces the old `(artifact, module: String, rva)` signature. The type system rejects absolute addresses, filesystem paths, and tool IDs at compile time — no runtime validation needed.
- **`ArtifactId` import alias in values.rs**: `ids::ArtifactId` is imported as `Stage0ArtifactId` to avoid collision with `domain::evidence::ArtifactId` (M1 semantic enum). This alias is internal; the public type exposed through structs is still `ids::ArtifactId`.

## 2026-07-17 (Task 8)

- **Stage-split error architecture**: `autore_core::Error` holds only cross-cutting Stage 0 categories (Io, Database, Serialization, Validation, NotFound, Conflict, HashMismatch, SchemaMismatch, Migration, InvalidStateTransition, Subscription, Operation, Unsupported). Stage-specific errors (Configuration, ModelProvider, AnalysisBackend, Worker) live in `autore_stage1::Error` with `Core(#[from] autore_core::Error)` for transparent forwarding.
- **Circular dependency prevention**: `autore_schema` depends on `autore_core` (for `Error::Validation` in `From<NamespacedIdError>`), so `autore_core` cannot depend on `autore_schema`. `SchemaMismatch` uses `String` fields for version info rather than `SchemaVersion` to avoid the cycle.
- **Trait impl return types must match the defining crate**: Repository traits in `autore-store` use `autore_core::Result<T>`. Stage1 impls (test stubs, noop repos) must use `autore_core::Result<T>` explicitly — `crate::Result<T>` in stage1 now means `autore_stage1::Result<T>`, a different type.
- **`.into()` ambiguity in closures**: When `#[from]` generates `From<CoreError> for Stage1Error`, `.into()` inside a closure is ambiguous (could be `CoreError -> Stage1Error` or `CoreError -> CoreError` via `impl<T> From<T> for T`). Fix: use `crate::Error::from(autore_core::Error::...)` which is unambiguous.
- **IDAError stays in autore-stage1 only**: `idax::Error` is wrapped as `EngineError::IdaError` in `autore-stage1/src/engine.rs` behind `#[cfg(feature = "ida")]`. It never touches `autore_core`.

## 2026-07-17 (Task 9)

- **tracing-subscriber `FormatEvent` for field redaction**: Custom `FormatEvent` implementation (`RedactingFormatter`) visits each event field via `Visit` trait and replaces sensitive values with `<redacted>`. This intercepts at the formatting layer — the field data never reaches the writer.
- **`tracing::subscriber::with_default` for scoped test subscribers**: Tests can install a temporary subscriber scoped to a closure, avoiding global `set_global_default` conflicts. This allows parallel test execution with separate log files.
- **`Mutex<File>` as `MakeWriter`**: `tracing_subscriber::fmt::layer().with_writer(Mutex::new(file))` works because `Mutex<T>` implements `MakeWriter` when `T: Write`. No custom wrapper needed.
- **Panic hook via `OnceLock<Arc<Mutex<...>>>`**: Global terminal restore hook uses `OnceLock` for one-time initialization + `Arc<Mutex<Option<...>>>` for mutable access. The `install_panic_hook()` function clones the `Arc` into the hook closure, capturing the shared reference without lifetime issues.
- **Doc tests need explicit imports**: `/// ``` ` blocks don't inherit `use` statements from the enclosing module. Every type used in a doc test must be explicitly imported within the block.

## 2026-07-17 (Task 10)

- **ProjectManifest lives in autore-schema, not autore-core**: `autore-schema` depends on `autore-core` (for `Error::Validation`), so `autore-core` cannot depend on `autore-schema` without creating a circular dependency. `ProjectManifest` references `Project` and other schema types, so it must live in `autore-schema`. The task description's suggestion to put it in `autore-core` is architecturally impossible.
- **NamespacedId segments allow hyphens**: Extended validation from `[a-z0-9_]` to `[a-z0-9_-]` to support artifact kind constants like `core.source-tree` and `core.native-provider-output`.
- **`std::sync::LazyLock` for constants**: `NamespacedId::parse` is not `const`, so artifact kind constants use `LazyLock<NamespacedId>` for lazy initialization. Stabilized in Rust 1.80, available with edition 2024.
- **`include_str!` path is relative to source file**: In `autore-schema/src/domain/records.rs`, `include_str!("../../tests/fixtures/...")` resolves correctly (two levels up from `src/domain/`). Using `../tests/` would look in `src/tests/` which doesn't exist.
- **ArtifactStorage adjacently tagged**: `#[serde(tag = "kind", content = "value")]` produces `{"kind":"ManagedBlob","value":{"relative_path":"..."}}` — consistent with the pattern used for `EvidenceValue` and `StableEntityKey`.
- **Timestamp lacks PartialOrd/Ord**: `Timestamp` only derives `PartialEq, Eq, Hash`. For ordering comparisons in tests, use `timestamp.as_offset_datetime()` to access the inner `time::OffsetDateTime` which implements `Ord`.
- **TOML round-trip via flat structure**: `ProjectManifest` uses a flat `ManifestToml` intermediate type (schema_version, project_id, name, created_at, updated_at) rather than serializing the full `Project` struct. Metadata is stored in the database, not the manifest file.

## 2026-07-17 (Task 11)

- **Transaction wrapper pattern with `Mutex<Connection>`**: `rusqlite::Transaction<'conn>` borrows `&'conn mut Connection`, which conflicts with `MutexGuard` lifetimes. Solution: custom `Transaction` struct holds the `MutexGuard`, issues raw `BEGIN IMMEDIATE` / `COMMIT` / `ROLLBACK` SQL, and uses `Drop` for automatic rollback on uncommitted drop. The `committed: bool` flag prevents double-rollback.
- **`BEGIN IMMEDIATE` over `BEGIN`**: Using `BEGIN IMMEDIATE` acquires a RESERVED lock immediately, preventing `SQLITE_BUSY` errors that occur when two connections start concurrent read-then-write transactions. This is the correct default for SQLite write transactions.
- **`FromSqlConversionFailure` requires `Box<dyn Error + Send + Sync>`**: `rusqlite::Error::FromSqlConversionFailure` needs `Box<dyn std::error::Error + Send + Sync>`, not `Box<String>`. A simple `ParseError(String)` newtype with `Display + Error` impls bridges the gap.
- **UUIDv7 as BLOB in SQLite**: `ProjectId` (UUIDv7 newtype) is stored as 16-byte BLOB via `as_uuid().as_bytes().as_slice()`. Reconstruction uses `Uuid::from_slice(&bytes)` which accepts `&[u8]` of length 16.
- **ExtensionData is a struct, not an enum**: Unlike `EvidenceValue` (adjacently tagged enum), `ExtensionData` is a struct with `schema: NamespacedId`, `version: u32`, `value: serde_json::Value`. Constructed via `ExtensionData::new(schema, version, value)`.
- **`lint_schema_no_db_ids` test pattern**: Query `sqlite_master` for all table DDL, uppercase it, and assert no `AUTOINCREMENT` keyword and no `DEFAULT` + `UUID` combination. This catches both `INTEGER PRIMARY KEY AUTOINCREMENT` and `DEFAULT (uuid())` patterns at migration time.
- **`next_project_event_sequence` queries future table**: The method references `project_events` which doesn't exist in V2. It's a utility for future migrations. Tests create the table inline via `CREATE TABLE project_events (...)` to verify the logic works.

## 2026-07-17 (Task 12)

- **FK constraint requires project existence before artifact insert**: The `stage0_artifacts` table references `projects(id)` via FK. Tests must insert a project into the `projects` table before registering artifacts — `SqliteProjectStore::insert_project` must run first.
- **`ContentHash` row reconstruction must NOT re-hash**: `ContentHash::sha256(data)` computes a NEW hash from raw data. When reconstructing from a stored digest in `row_to_artifact`, use direct struct construction `ContentHash { algorithm, digest }` — NOT `ContentHash::sha256(&digest)` which would hash-the-hash. Both `algorithm` and `digest` fields are `pub`, making this safe.
- **Content-addressed dedup at filesystem level**: Two `register_managed` calls with identical content produce the same blob path but different `ArtifactId` values. The dedup check is `blob_path.exists()` — skip the `fs::write` when the blob already exists. The DB always gets a new row with a unique UUID.
- **External artifact verification returns `HashMismatch` not silent update**: When an external file changes after registration, `verify_artifact` returns `Err(Error::HashMismatch)` — the stored hash is immutable. This is the correct behavior per spec §8.
- **V3 migration uses `stage0_artifacts` table name**: The V1 migration already creates an `artifacts` table (M1 schema). The Stage 0 artifact table is named `stage0_artifacts` to avoid collision.
- **`base_dir` + `project_id` path composition**: `SqliteArtifactStore` takes a `base_dir` and composes project paths as `<base_dir>/<project_id>/`. This avoids storing project directory paths in the DB and keeps the store stateless wrt filesystem layout.

## 2026-07-17 (Task 13)

- **Lifecycle code placement in autore-app**: Task description suggested `autore-core`, but the circular dependency (`autore-schema` → `autore-core`) prevents it. `autore-app` depends on both `autore-schema` (for `ProjectManifest`) and `autore-store` (for `Database`), making it the correct location. This is a preview of the `ApplicationService` that will arrive in Wave 0G.
- **Project directory layout constants**: Using module-level constants (`PROJECT_DIR_NAME`, `MANIFEST_FILE_NAME`, etc.) for the directory structure makes the layout explicit and testable. The layout is `<parent>/project.auto-re/{project.toml,project.sqlite3,artifacts/,packages.lock}`.
- **Schema version verification on open**: `open_project` checks that the manifest's `schema_version` matches the expected `SchemaVersion::new(2, 0)`. This provides forward-migration safety — opening a project with an incompatible schema version fails early with `Error::SchemaMismatch`.
- **Database::open is idempotent**: Calling `Database::open` on an existing database file applies any pending migrations but does not fail if migrations are already applied. This makes `open_project` safe to call multiple times.
- **close_project as no-op marker**: The `Database` uses `Mutex<Connection>` which is dropped when it goes out of scope. Explicit cleanup is not required, but the `close_project` function documents the lifecycle and provides a future hook for explicit resource management if needed.
- **Project equality via PartialEq derive**: `Project` derives `PartialEq`, allowing `assert_eq!` for round-trip tests. The manifest's `Project` reconstructed from TOML matches the original `Project` field-by-field (id, name, schema_version, created_at, updated_at). Metadata is empty in both cases since it's stored in the database, not the manifest.
- **tempfile as dev-dependency**: Tests use `tempfile::TempDir` for isolated project directories. Added as `[dev-dependencies]` in `autore-app/Cargo.toml` with `tempfile.workspace = true`.

## 2026-07-17 (Task 14)

- **EntityId naming collision managed via import paths**: `ids::EntityId` (UUIDv7 newtype) and `domain::evidence::EntityId` (semantic enum) coexist. `SemanticEntity.id` uses `crate::ids::EntityId` directly in `records.rs`. Cannot add `ids::EntityId` to `lib.rs` top-level re-exports because `pub use domain::*` already brings in `domain::evidence::EntityId`. Consumers use `autore_schema::ids::EntityId` for the newtype.
- **Partial UNIQUE index in SQLite**: `CREATE UNIQUE INDEX ... WHERE stable_key IS NOT NULL` enforces uniqueness only on non-NULL stable_keys per project. Multiple entities with NULL stable_key are allowed in the same project. This is a SQLite-specific feature (partial indexes) that matches the spec requirement exactly.
- **StableEntityKey stored as JSON TEXT**: The `stable_key` column stores the serde JSON serialization of `StableEntityKey` (adjacently tagged enum). This makes cross-revision matching work by string equality on the JSON representation.
- **Conflict detection via error message matching**: SQLite returns `UNIQUE constraint failed: ...` on duplicate inserts. The `SqliteEntityStore::insert` maps this to `Error::Conflict` by checking if the error message contains `"UNIQUE constraint failed"`. This is the established pattern in the codebase.
- **Stable pagination ordering with `ORDER BY kind ASC, id ASC`**: Adding `id ASC` as a secondary sort key ensures deterministic pagination even when multiple entities share the same `kind` or `created_at`. UUIDv7's time-ordering means `id ASC` also gives temporal ordering within the same kind.
- **Entity kind constants follow artifact kind pattern**: `ENTITY_KIND_*` constants use `std::sync::LazyLock<NamespacedId>` exactly like `ARTIFACT_KIND_*` constants. The `register_kind` trait method is a no-op default implementation, allowing future implementations to track registered kinds without requiring it now.

## 2026-07-17 (Task 15)

- **`ExtensionData` contains `serde_json::Value` which does not implement `Eq`**: Any struct containing `ExtensionData` (e.g., `EnvironmentIdentity`) must drop the `Eq` derive and use only `PartialEq`. Same constraint as `EvidenceValue::Float`.
- **`ProviderRunStatus` state machine in schema, transitions in store**: The `transition(target)` method lives on `ProviderRunStatus` in `autore-schema` (returns `autore_core::Result<()>` using `Error::InvalidStateTransition`). The store's `complete_run` reads current status from DB, validates the transition, then updates — enforcing the FSM at both domain and persistence layers.
- **`complete_run` as atomic read-validate-write**: Rather than loading the full `ProviderRun` struct, `complete_run` queries just the `status` column, validates via `ProviderRunStatus::transition`, then issues `UPDATE SET status, completed_at`. This avoids unnecessary serialization overhead for a state-only change.
- **Provider kind constants use `provider.*` namespace**: `provider.disassembler`, `provider.decompiler`, `provider.debugger`, `provider.symbolic-executor`, `provider.llm`, `provider.human`. The `provider.` prefix distinguishes them from artifact and entity kinds.
- **`ContentHash` and `EnvironmentIdentity` stored as JSON TEXT in SQLite**: Complex types serialize to JSON strings in TEXT columns. `ContentHash::sha256(b"x")` stores as `{"algorithm":"sha256","digest":"..."}`. `EnvironmentIdentity` stores as a JSON object with all its nested fields.
- **`Vec<ArtifactId>` stored as JSON array TEXT**: `input_artifacts` serializes as a JSON array of UUID strings. This keeps the schema simple and avoids a junction table for a typically small list.
- **Dynamic SQL for `list_runs` query filtering**: `RunQuery` supports optional `status_filter` and `provider_filter`. The query builder constructs `WHERE` clauses dynamically with numbered parameters (`?1`, `?2`, etc.), matching the entity store's approach for optional filters.
- **`Option<Vec<u8>>` for nullable BLOB columns**: `package_id` and `configuration_artifact` are optional UUID BLOBs. `rusqlite` maps `Option<Vec<u8>>` to/from nullable BLOB columns naturally.

## 2026-07-17 (Task 16)

- **`Vec<EntityId>` serializes to JSON array TEXT**: `subject_entities` in `native_artifacts` stores as a JSON array of UUID strings. Same pattern as `Vec<ArtifactId>` in `provider_runs.input_artifacts`.
- **`list_by_subject_entity` uses application-level filter**: Since `subject_entities` is stored as a JSON array in a TEXT column, the query fetches all rows and filters in Rust by checking if any entity UUID matches. This avoids SQLite JSON extension dependency. Acceptable for the expected small dataset sizes per run.
- **`provider_entity_aliases` has no PRIMARY KEY**: The table uses a `UNIQUE INDEX` on `(provider_run, provider_identifier)` to enforce uniqueness, but no single-column PK. This is valid in SQLite (rowid tables) and matches the composite nature of the identity.
- **`SqliteAliasStore` implements both `ProviderAliasStore` and `NativeArtifactStore`**: A single struct implements both traits, sharing the `&Database` reference. This avoids redundant struct definitions while maintaining trait-object separation.
- **V6 migration uses `hash_digest` BLOB not TEXT hex**: The `stage0_artifacts` table stores `hash_digest` as BLOB (raw bytes), not hex TEXT. Test helpers must use `ch.digest.as_slice()` not `ch.digest_hex()`.
- **FK references for aliases require both `provider_runs` AND `semantic_entities`**: The `provider_entity_aliases` table has FKs to two different tables. Both must exist before an alias can be inserted — test setup needs `insert_project`, `insert_provider`, `insert_run`, AND `insert_entity`.

## 2026-07-17 (Task 17)

- **`EvidenceRecordId` is a new typed ID distinct from M1 `EvidenceId`**: The M1 `ids::EvidenceId` already exists (used by claims/hypotheses). `EvidenceRecordId` is the Stage 0 append-only evidence record ID — a separate type preventing accidental mixing.
- **`EvidenceValue` contains `f64` via `Float` variant**: Any struct containing `EvidenceValue` (including `EvidenceRecord`) cannot derive `Eq` — must use `PartialEq` only. Same constraint as `EnvironmentIdentity` with `ExtensionData`.
- **`native_artifacts` stored as JSON array TEXT with no FK**: The `evidence_records.native_artifacts` column stores `Vec<NativeArtifactId>` as a JSON array of UUID strings. No FK constraint — the IDs are opaque references. This matches the `provider_runs.input_artifacts` pattern.
- **`assumptions` stored as JSON TEXT**: `Vec<Assumption>` serializes as a JSON array of objects with `description` and optional `evidence` fields. The `evidence` field references another `EvidenceRecordId` (self-referential within the same table).
- **`EvidenceLifecycleState` uses string representation in DB**: The `state` column stores the Display representation ("Active", "Superseded", "Invalidated", "Unavailable"). Reconstruction uses a match on the string rather than serde, keeping the DB format simple and human-readable.
- **Append-only enforced at API level, not DB level**: The `EvidenceStore` trait has no `update` or `delete` methods. The DB table has no `UNIQUE` constraint beyond the PK — multiple inserts with the same ID would fail on PK collision. The immutability guarantee is the absence of mutation APIs.
- **V7 migration numbering**: Task plan says "V6__evidence.sql" but V6 is already used by `aliases_native`. V7 is the correct next migration number. No impact — refinery applies migrations in numerical order.
- **`evidence_lifecycle_events` has no PRIMARY KEY**: Like `provider_entity_aliases`, this table uses rowid. Multiple events per evidence record are expected (append-only history). The `idx_evidence_lifecycle_evidence_time` index provides efficient ordered retrieval.

## 2026-07-17 (Task 18)

- **`Confidence` evolved from transparent newtype to struct**: Changed from `Confidence(f32)` with `#[serde(transparent)]` to `Confidence { score: f32, rationale: Option<String> }`. This breaks serialization (bare number → JSON object), but no persisted data exists yet. `Confidence::new(score)` preserved for backward compat.
- **Removing `Copy` from `Confidence` requires `.clone()` at move sites**: `ProposedClaim` construction in `claim.rs` uses `.iter()` (shared ref) and needs `.clone()` instead of copy. Only one site affected.
- **`HypothesisStatus::Superseded { by }` needs two-column DB storage**: The `Superseded` variant carries a `HypothesisId`. Stored as `status TEXT` (discriminant: "Proposed", "Superseded", etc.) + `superseded_by BLOB NULL REFERENCES hypotheses(id)`. Clean SQL filtering and FK enforcement.
- **`update_status` as atomic read-validate-write**: Same pattern as ProviderStore's `complete_run`. Reads current status from DB, validates transition via `HypothesisStatus::transition(&target)`, then updates. The `&target` signature (reference) avoids moving the Superseded variant's data.
- **`JsonSchema` for `Confidence` must match serde format**: The `worker_output.rs` JsonSchema impl delegated to `f64` (bare number). Updated to a `ConfidenceRepr { score: f32, rationale: Option<String> }` struct matching the new serialization. The `jsonschema` crate validates the generated JSON against this schema.
- **Verification tests run under `-p autore-schema` not `-p autore-core`**: Plan text says `cargo test -p autore-core`, but `HypothesisStatus` lives in `autore-schema` (circular dependency: `autore-schema` → `autore-core`, so `autore-core` cannot reference `HypothesisStatus`). The `validate_no_cycle` function in `autore-core` is generic and tested independently; the hypothesis-specific cycle test uses it from `autore-schema`.
- **V8 migration numbering**: Plan says V7 for hypotheses but V7 is already used by evidence. V8 is correct.

## 2026-07-17 (Task 19)

- **`ContradictionStatus` lives in `autore-schema` (not `autore-core`)**: Follows the established `HypothesisStatus`/`ProviderRunStatus` pattern. `autore-core` cannot reference schema types due to the circular dependency constraint (`autore-schema` → `autore-core` for `Error`/`Result`). Plan text says `cargo test -p autore-core` but the correct crate is `autore-schema`.
- **`ContradictionResolution` stored as JSON TEXT**: The `resolution` column is `TEXT NULL` — NULL until the contradiction transitions to `Resolved`, at which point the whole `ContradictionResolution { resolved_at, resolution, chosen, rationale }` is serialized as JSON. Simpler than a separate table for Stage 0.
- **`Contradiction.evidence` and `hypotheses` stored as JSON arrays of UUID strings**: Matches the existing `Hypothesis.supporting_evidence` pattern. No junction tables at this stage.
- **`VerificationSubject` uses a discriminator column pattern**: `subject_kind TEXT` + `subject_id BLOB`. The discriminator values are "Entity", "Hypothesis", "Artifact", "GenerationTarget". Since each variant refers to a different FK target, no FK constraint is possible at the DB level — the application layer owns referential integrity.
- **`VerificationState` has a non-terminal `Blocked` state**: Unlike `ContradictionStatus.Resolved` which is terminal, `VerificationState.Blocked` can transition back to `Pending` (retry after unblocking). Terminal states: `Passed`, `Failed`, `Inconclusive`.
- **`Contradiction.transition(target, resolution)` bundles resolution with the Resolved transition**: When `target == Resolved`, a `ContradictionResolution` MUST be provided; for other transitions it MUST be `None`. Validation happens at the domain layer BEFORE the DB call.
- **`ContradictionStore.resolve` as atomic read-validate-write**: Same pattern as `HypothesisStore.update_status` and `ProviderStore.complete_run`. Reads current status, validates via `ContradictionStatus::transition(&target)`, then updates.
- **`VerificationStore.multi_check_per_subject_supported` returns `true`**: A capability flag. The table has no UNIQUE constraint on (subject, check), so multiple records per (subject_kind, subject_id, check_kind) are permitted. Index on `(subject_kind, subject_id, check_kind)` supports efficient queries.
- **Subject-discriminator isolation is verified by test**: `verification_subject_kind_discriminator_isolated` inserts both an `Entity(id)` and an `Artifact(same_uuid)` and asserts that `list_by_subject` returns only the matching variant. This catches regressions where a future refactor forgets to filter on `subject_kind`.
- **V9 migration numbering**: Plan text says `V8__contradictions_verification.sql` but V8 is already used by `hypotheses.sql`. V9 is the correct next migration number. Task brief correctly noted this.

## 2026-07-17 (Task 20)

- **`OperationState` placement in `autore-core`**: Since `OperationState` is unit-only (no schema dependency), it lives in `autore-core/src/operation.rs`. `autore-schema` imports it via `autore_core::operation::OperationState`. This avoids the circular dependency constraint while keeping the state machine testable independently.
- **`serde` and `serde_json` added to `autore-core`**: `OperationState` derives `Serialize`/`Deserialize` for JSON round-trip tests and future event serialization. These were already workspace dependencies.
- **`MetricMap` as type alias**: `pub type MetricMap = BTreeMap<NamespacedId, f64>` — a simple type alias rather than a newtype. `BTreeMap` provides deterministic serialization ordering; `f64` values are measurements.
- **`EventSource` and `EventSubject` designed for reuse**: These types are shareable between `Operation` events and future `ProjectEvent` records (Task 21). `EventSubject` uses adjacently tagged serde (`#[serde(tag = "kind", content = "id")]`) matching the `VerificationSubject` pattern.
- **Per-operation sequence numbers**: `ProgressUpdate.sequence` is per-operation, not global. The store uses `INSERT` with caller-provided sequence; a future `next_sequence` helper would query `MAX(sequence) + 1 WHERE operation_id = ?`.
- **Cooperative cancellation is store-level, not domain-level**: `CancellationRequest` records are inserted into the DB; the operation's state does NOT change automatically. The application layer checks for pending requests and transitions through `Cancelling → Cancelled` when the operation yields.
- **`Operation.parent` is self-referential FK**: `operations.parent BLOB NULL REFERENCES operations(id)` — SQLite supports self-referential FKs. Cycle rejection is at the application layer via `validate_no_cycle`.
- **`OperationFailure` stored as JSON TEXT**: Same pattern as `ContradictionResolution` — complex type serialized to a TEXT column. NULL until the operation transitions to `Failed`.
- **V10 migration numbering**: Plan text says `V9__operations.sql` but V9 is already used by `contradictions_verification.sql`. V10 is the correct next migration number. Task brief correctly flagged this.
- **`autore-events::operation_events` module**: Created as a bridge module with `transition_event_kind()` and `emit_transition_event()` functions. Task 21 will wire these to actual `ProjectEvent` records. The event kind format is `core.operation.<state>` (e.g., `core.operation.started` for Running).

## 2026-07-17 (Task 21)

- **`Transaction` Mutex deadlock with store methods**: `Database` wraps `Connection` in `Mutex`. `begin_transaction()` acquires the lock. Store methods like `OperationStore::transition()` call `self.db.connection()` which tries to lock the same Mutex → deadlock. Inside `with_event` closures, state mutations must use `txn.conn()` directly (raw SQL) rather than going through store methods.
- **`next_project_event_sequence` must run inside the transaction**: Computing `MAX(sequence) + 1` outside the transaction risks two concurrent writers getting the same sequence. The in-transaction version takes `&Transaction` (not `&Database`), ensuring the sequence is computed while the lock is held.
- **`V11 project_events` has FK to `projects(id)`**: Tests that insert events must first insert a project row. The `next_project_event_sequence_with_table` test in database.rs needed updating to insert a project before inserting an event.
- **`EventSource` reconstructed via `parse_event_source` match**: Rather than using serde for DB round-trip, the `EventSource` enum is reconstructed from its `Display` string via a match expression. This is simpler and avoids serde overhead for a flat enum.
- **`with_event` helper returns `Result<T>` from the closure**: The closure can return any value, which is forwarded after the event is emitted and the transaction committed. This enables patterns like `let id = with_event(db, pid, kind, source, subject, None, |txn| { insert_and_return_id(txn) })?`.
- **`autore-events` gains `autore-schema` dependency**: The `operation_events` module now references `EVENT_KIND_OPERATION_*` constants from `autore-schema`. This is a lightweight dependency (no SQLite) and does not create circular references.

## 2026-07-17 (Task 22)

- **On-disk SQLite reopen pattern for durability tests**: `Database::open(path)` on an existing file re-applies migrations (idempotent) and opens the connection. Dropping the `Database` handle closes the connection. Reopening the same path proves that committed state survives process kill.
- **`with_event` closure must use `txn.conn()` for raw SQL**: Confirmed the Task 21 lesson — calling store methods inside `with_event` would deadlock because the `Transaction` holds the `MutexGuard`. All state mutations inside the closure use raw SQL via `txn.conn()`.
- **OperationState::Queued.transition(&Running) validates in-closure**: The core state machine validation runs inside the `with_event` closure before the SQL UPDATE. This proves the transition is legal before persisting.
- **MutexGuard scoping prevents deadlock in reopen tests**: After reopening, querying IDs via `db.connection()` must release the guard (via scope block) before calling store methods that re-acquire the lock. Holding the guard across store calls causes deadlock.
- **`Transaction::Drop` auto-rollback is atomic across state+event**: When `with_event`'s closure returns `Err`, the `Transaction` drops without `commit()`, triggering `ROLLBACK`. This undoes both the state UPDATE and the event INSERT in one atomic operation — no partial writes survive.
- **`tempfile::tempdir()` for isolated on-disk tests**: Each test gets a unique temporary directory that is automatically cleaned up. The `Database::open` call creates the SQLite file inside it.
- **WAL mode does not prevent reopen consistency**: SQLite WAL mode is enabled by `Database::open`. After dropping the connection and reopening, the WAL is checkpointed and all committed data is visible. No special WAL handling needed in tests.

## 2026-07-17 (Task 23)

- **`ProjectEventSubscription` decouples from `ProjectEventService` via a closure**: Holding a trait-object reference back to the service creates a self-reference problem for `LocalProjectEventService::subscribe`. Passing an `Arc<dyn Fn(ProjectId, u64, usize) -> Result<Vec<ProjectEvent>> + Send + Sync>` closure into the subscription avoids the issue and keeps the subscription usable with mock services like `GappedSubscription`.
- **`tokio::sync::broadcast` lag is a `RecvError::Lagged(n)`**: When a slow subscriber falls behind, the receiver returns this error. The subscription must resync from the durable store rather than dropping events. `Lagged(n)` does not tell us *which* events were missed, only that `n` were dropped, so `events_after(last_known_sequence)` is the correct recovery path.
- **Replay-to-live gap detection checks `event.sequence > last_known_sequence + 1`**: This works for both replay buffers and live events. On detection, the subscription resyncs from the store and re-evaluates any pending live event.
- **Store commit before broadcast**: `LocalProjectEventService::emit_event` begins a transaction, computes the sequence inside the transaction, inserts the event, commits, and only then broadcasts. If broadcast fails (no receivers), the event is still durable.
- **In-process only**: `tokio::sync::broadcast` is the only transport. No TCP, WebSocket, HTTP, or gRPC is introduced, satisfying §21.
- **`events_after` with `limit`**: The underlying `EventStore::events_after` returns all matching events; the service layer applies `.take(limit)` to satisfy the `ProjectEventService` trait contract without changing the store API.
- **`ProjectEventSubscription::new` is synchronous but lazy**: The initial replay buffer is loaded on the first `next().await` call, so creating a subscription never blocks on the database.
- **`EventBroadcaster::with_capacity` enables testable lag**: The default capacity is `EVENT_BROADCAST_CAPACITY = 256`, but a smaller capacity can be used in tests to force `RecvError::Lagged` without emitting hundreds of events.

## 2026-07-17 (Task 24)

- **ApplicationService belongs in `autore-app`**: It needs both `autore-schema` (request/response types) and `autore-store` (database + event helpers), so `autore-app` is the natural home.
- **All mutating commands must use `with_event`**: The helper runs the mutation closure and then emits the event in the same SQLite transaction. Passing the event `subject` as `Some(EventSubject::...)` is required for tests that inspect the event.
- **Raw SQL helpers inside `with_event` avoid Task 21 deadlock**: Store methods acquire their own `Mutex<Connection>` guard, so calling them inside the `with_event` closure would deadlock. The mutation helpers use `txn.conn()` directly.
- **Store traits needed `list_by_project` for query routing**: `ArtifactStore` and `VerificationStore` lacked project-scoped list methods. Adding them lets `ApplicationService` route `ListArtifacts` and `ListVerifications` without duplicating SQL.
- **Schema constants for cancellation were missing**: `EVENT_KIND_OPERATION_CANCELLING` and `EVENT_KIND_OPERATION_CANCELLED` were added to `autore-schema` so `CancelOperation` can emit a meaningful event.
- **`NamespacedId` validation requires exactly one dot**: Test predicates like `hypothesis.predicate.test` are invalid; use `hypothesis.test` instead.
- **`MetadataMap` is private in `domain::records`**: Import it from `autore_schema::domain::values` when constructing records manually.
- **`ProjectEventService` trait does not expose `emit_event`**: Only `LocalProjectEventService` has the inherent `emit_event` method; application code should route through `with_event` for atomicity.
- **Clippy tuple-variant `map_err`**: `map_err(|e| Error::Database(e))` can be simplified to `map_err(Error::Database)`.

## 2026-07-17 (Task 25)

- **`AutoReClient` trait expands from empty placeholder to full interface**: The trait now has `execute`, `query`, `events_after`, and `subscribe_events`. `LocalAutoReClient` delegates to `Arc<ApplicationService>`, giving CLI/TUI a stable interface without exposing store fields.
- **Cross-project validation already comprehensive in Task 24**: `ensure_same_project` was already called for `AddEvidence`, `ChangeHypothesisStatus`, `RecordContradiction`, `AddVerification`, and `CancelOperation`. `RegisterProvider` and `StartProviderRun` don't carry sub-records with their own `project` field (Provider has no project field; ProviderRun is constructed from request fields), so no additional checks needed.
- **`ProjectEventSubscription::next()` is async**: The subscription test requires `#[tokio::test]`. Added `tokio` as a dev-dependency of `autore-app`. The `tokio::test` macro provides a runtime automatically.
- **`ApplicationService` no longer implements `AutoReClient` directly**: The old empty `impl AutoReClient for ApplicationService {}` was removed. `LocalAutoReClient` is now the sole in-process implementation, wrapping `Arc<ApplicationService>`. This decouples the client interface from the service implementation.
- **`Derivation` and `DerivationMethod` accessible via `autore_schema::domain`**: Used in test code to construct `EvidenceRecord` fixtures. The import path is `autore_schema::domain::{Derivation, DerivationMethod}`.

## 2026-07-17 (Task 26)

- **`lifecycle::create_project` vs `ApplicationCommand::CreateProject`**: Two separate paths exist for project creation. `lifecycle::create_project` creates the directory structure (manifest, DB, artifacts/) but does NOT insert a project record into the DB. `ApplicationCommand::CreateProject` inserts the record but does NOT create directories. The CLI must call both and overwrite the manifest with the application-layer project ID to keep them consistent.
- **`autore_app::application_service::requests::*` re-export**: The request/response structs (e.g., `GetProjectSummaryQuery`, `CreateProjectRequest`) were not re-exported from `autore_app` at the crate root. Added `pub use application_service::requests::*` to `autore-app/src/lib.rs` so CLI consumers can construct them directly.
- **`serde::Serialize` derives on `CommandResult`**: `CommandResult` and all response structs lacked `Serialize` derives, preventing JSON output of write-command results. Added `#[derive(serde::Serialize)]` to all types in `requests.rs`. Required adding `serde.workspace = true` to `autore-app/Cargo.toml`.
- **`Confidence.score` is private, accessed via method**: `Confidence` has a private `score: f32` field with a public `score()` method. CLI code must use `.score()` not `.score`.
- **Clippy `print_literal` lint**: Using `{}` format specifiers with string literals in `println!` triggers `clippy::print_literal`. Fix: embed the literal directly in the format string (e.g., `println!("{:<38} {:<30} Size", "ID", "Kind")` instead of `println!("{:<38} {:<30} {}", "ID", "Kind", "Size")`).
- **`clap::ValueEnum` with `#[default]`**: Clap's `ValueEnum` derive works with Rust's `#[default]` attribute on enum variants. Combined with `#[derive(Default)]`, this eliminates the need for a manual `Default` impl.
- **`--project-dir` global argument**: Using `#[arg(long, global = true)]` in clap makes the argument available to all subcommands. The default value `"."` resolves to the current working directory.
- **`Contradiction` and `Operation` have no `description`/`label` fields**: `Contradiction` uses `subject: EntityId, predicate: NamespacedId` for identification. `Operation` uses `kind: NamespacedId, requested_by: String`. Human-readable output must use these fields instead.

## 2026-07-17 (Task 27)

- **`assert_cmd` for CLI integration tests**: `Command::cargo_bin("auto-re")` locates the compiled binary. Combined with `predicates` for string assertions and `tempfile::TempDir` for isolation. Each test creates a fresh project directory and runs the real binary — no mocking needed.
- **`AddHypothesisResponse` has `id` directly, not nested under `hypothesis`**: Unlike `RegisterEntityResponse { entity: SemanticEntity }` which nests, `AddHypothesisResponse { id: HypothesisId }` exposes the ID at the top level. JSON extraction paths differ: `["HypothesisAdded"]["id"]` vs `["EntityRegistered"]["entity"]["id"]`.
- **`HypothesisStatus` state machine blocks `Proposed -> Accepted`**: The valid path is `Proposed -> UnderInvestigation -> Accepted`. The CLI's `hypothesis accept` command attempts a direct `Proposed -> Accepted` transition, which the state machine correctly rejects. The CLI lacks an "investigate" command to reach `UnderInvestigation`, so `accept` cannot succeed on freshly created hypotheses.
- **Write commands always produce JSON**: `print_command_result(schema, &result)` always emits JSON with `$schema`. Only read commands (`project info`, `operation list`, `events list`) support `--output human`. `project create` is the exception — it always prints human-readable output.
- **Evidence record JSON construction for tests**: `EvidenceRecord` requires `id`, `project`, `subject`, `predicate`, `value`, `derivation`, `provider_run`, `native_artifacts`, `assumptions`, `created_at`. The `derivation.method` uses internally-tagged serde (`{"kind":"DirectObservation"}`), `value` uses adjacently-tagged (`{"kind":"String","value":"..."}`), and `NamespacedId`/`Timestamp` serialize as bare strings.

## 2026-07-17 (Task 28)

- **Circular dependency between `autore-app` and `autore-tui`**: `autore-app` re-exported `autore_tui::{runtime, tui}` but no external code used these re-exports. Removed `autore-tui` from `autore-app`'s deps and the re-exports from `autore-app/src/lib.rs`. Added `autore-app` to `autore-tui`'s deps so the TUI can hold `Box<dyn AutoReClient>`. The binary crate (`autore-cli`/`autore-stage1`) wires both together.
- **`TuiState` separate from `Tui`**: The `client: Option<Box<dyn AutoReClient>>` lives on the `Tui` application struct, not on `TuiState`. This keeps `TuiState` pure data that derives `Clone`, `Default`, etc. easily. The client is accessed via the `Tui` methods, not directly from state.
- **`HashMap<ProjectId, ProjectViewState>` for project views**: Each project gets its own view snapshot keyed by ID. The TUI can render multiple projects simultaneously and switch between them via `Navigation::Project(id)`.
- **`HypothesisStatus::Accepted` is a unit variant**: Unlike `Superseded { by: HypothesisId }`, `Accepted` carries no data. Pattern matching must use `HypothesisStatus::Accepted` (not `Accepted { .. }`) to avoid clippy `unneeded_struct_pattern`.
- **Grep acceptance criteria covers comments too**: `grep -r 'rusqlite\|Database' autore-tui/src` returns matches in comments, not just imports. All mentions of these words were removed from comments/doc-comments to satisfy the strict no-match requirement.
- **Presentation-only state stores raw schema types**: `ProjectViewState.artifacts: Vec<Artifact>`, `hypotheses: Vec<Hypothesis>`, etc. — all are `autore-schema` domain types with serde derives. The TUI doesn't need wrapper types; it renders snapshots loaded from `QueryResult` variants.

## 2026-07-17 (Task 29)

- **`Box<dyn AutoReClient>` → `Arc<dyn AutoReClient>` for background tasks**: Background tokio tasks need to call `client.events_after()` and `client.query()`. `Box<dyn>` cannot be shared — it's not `Clone`. Converting to `Arc<dyn>` internally (while keeping the public `with_client(state, Box<dyn>)` API) lets background tasks clone the Arc without unsafe code. `Arc::from(box)` is a standard conversion in Rust.
- **`std::future::pending()` for inactive `tokio::select!` branches**: When there's no subscription attached, the subscription branch of `tokio::select!` must never fire. `std::future::pending()` returns `!` (never type), which coerces to any type and never resolves. This cleanly disables a branch without `if` guards or `Option`-wrapping the future.
- **`tokio::select!` borrow splitting across struct fields**: `tokio::select!` splits borrows across struct fields — each branch can borrow a different field of the same struct. This means `app.internal_rx.recv()` and `app.subscription.as_mut()` can coexist in the same `select!` without conflict. However, you CANNOT hold a `&mut Tui` (via `TuiEventLoop`) AND call `app.render()` — the borrow checker sees the whole struct as borrowed. Solution: inline the select logic in `run_tui` and keep `TuiEventLoop` for tests only.
- **`ListEventsQuery` requires `after_sequence` and `limit` fields**: The query struct has three fields: `project`, `after_sequence`, and `limit`. Tests must provide all three.
- **`QueryResult::Events(EventsResponse)`**: The variant wraps an `EventsResponse { events: Vec<ProjectEvent> }` struct, not a bare `Vec`. Match pattern is `QueryResult::Events(response)` and access `response.events`.
- **Move-after-use with `ev` in catchup handler**: When iterating events for catch-up, `view.recent_events.push(ev)` moves `ev`. Accessing `ev.sequence` afterward is a use-after-move. Fix: read `ev.sequence` BEFORE pushing `ev` into the vec.
- **`TuiEventLoop<'a>` borrows `&'a mut Tui`**: The test driver struct borrows the Tui mutably, which means the render loop cannot coexist with it. For tests, this is fine — tests drive the event loop step-by-step without rendering. For the real `run_tui`, the select logic is inlined directly.
- **Clippy `collapsible_if` with let-chains**: Nested `if let` and `if` can be collapsed using let-chain syntax (`if cond && let pat = expr && cond2 { ... }`). Clippy with `-D warnings` catches this.
- **Clippy `single_match` for `match` with one arm + wildcard**: `match x { Pattern => { ... }, _ => {} }` should be `if let Pattern = x { ... }`.

## 2026-07-17 (Task 30)

- **`ratatui::widgets::Tabs` requires `Vec<Line>`**: `Tabs::new(titles)` accepts `Vec<Line>` for tab titles, not bare strings. Use `titles.iter().map(|t| Line::from(*t)).collect::<Vec<_>>()` to convert string slices.
- **`ratatui::layout::Layout` 2-region split for tab strip + body**: Split the right column vertically with `Constraint::Length(2)` for the tab strip and `Constraint::Min(1)` for the body. This preserves the tab strip height while allowing the body to expand.
- **`Block::bordered().title()` accepts `impl Into<Title>`**: Both `&str` and `String` work. The title is rendered in the top border of the block.
- **`Paragraph::new(lines)` accepts `Vec<Line>`**: Each `Line` can be constructed from a `String` or from `vec![Span::raw(...), ...]`. Bold text uses `Span::raw("text").bold()`.
- **`HashMap::keys().next()` is non-deterministic**: In tests, `project_views.keys().next()` returns different keys across runs because `HashMap` iteration order is randomized. Always use `match &state.navigation { Navigation::Project(pid) => *pid, ... }` to get the project the TUI is actually viewing.
- **Generic fallback renderer with `impl IntoIterator`**: `render_generic_record(kind, id, fields)` accepts `I: IntoIterator<Item = (S, S)>` where `S: Display`. This lets callers pass `Vec<(String, String)>`, `&[(String, String)]`, or any other iterable. The function never panics on unknown kinds — it just renders the fields it's given.
- **`serde_json::Value` flattening for `ExtensionData`**: The `flatten_json` helper recursively walks JSON objects/arrays and emits `(key_path, value)` pairs. Object keys are dot-separated (`foo.bar`), array indices are bracketed (`foo[0]`). This lets the generic renderer display arbitrary `ExtensionData` payloads without knowing their schema.
- **`MetadataMap::iter()` yields `(&NamespacedId, &ExtensionData)`**: Each entry has a schema key and an `ExtensionData` value with `schema`, `version`, and `value` fields. The generic renderer formats these as `schema@version = json_value`.
- **Alt+1..Alt+7 for pane switching**: `KeyCode::Char('1')` with `KeyModifiers::ALT` switches to `Pane::Dashboard`, `Alt+2` to `Providers`, etc. This avoids conflicts with normal key handlers (which use unmodified keys).
- **`autore_core::operation::OperationState::to_string()` works**: `OperationState` implements `Display` via `f.write_str(self.kind())`, so `format!("{op.state}")` produces `"Queued"`, `"Running"`, etc. No need for `format!("{:?}", op.state)`.

## 2026-07-17 (Task 31)

- **Sync `dispatch_command` vs async `dispatch_query`**: user-initiated command dispatch (key press → `client.execute`) runs synchronously in `handle_key_event` because the call site is not async and user keypresses are rare enough that a brief block on the render thread is acceptable. Background `dispatch_query` uses `spawn_blocking` + `tokio::spawn` because queries run on project-event reception and must never block rendering. Both route through `AutoReClient`; neither bypasses validation.
- **`AutoReClient` trait methods are synchronous**: `execute` and `query` return `Result<CommandResult>` directly, not `Future<Output = Result<...>>`. Wrapping them in `spawn_blocking` requires a tokio runtime. Tests using `RecordingClient` with `#[test]` (not `#[tokio::test]`) work for synchronous dispatch but need `#[tokio::test]` + `yield_now` polling for async query dispatch.
- **`NamespacedId::new` takes `&[&str]` and returns `Result`**: call sites must unwrap (`.unwrap()` in tests, `.parse()` for dotted strings like `"core.binary"`). Two-arg `("segment1", "segment2")` form does NOT exist.
- **`Confidence` is at `autore_schema::domain::Confidence`, not `autore_schema::domain::records::Confidence`**: `records.rs` re-exports it, but the re-export is not publicly accessible via the `records` path. Use `autore_schema::domain::Confidence::new(f32)` (returns `Result`).
- **`RegisterArtifactRequest.kind` is `String`, but `Artifact.kind` is `NamespacedId`**: the application service does the parse. Recording-client tests must do the same — `NamespacedId::parse(&req.kind)` or fall back to a test default.
- **Dialog state machine**: `open_artifact_import_dialog` pushes an `Input` dialog and sets `focus = Focus::Dialog`. `handle_key_event` dispatches to `handle_dialog_key_event` when focus is Dialog. Enter confirms (pops dialog, dispatches command), Esc cancels (pops without dispatch). Buffer edits go through `KeyCode::Char(c)` arm; backspace pops the last char.
- **`ProjectEventSubscription::new` needs a `broadcast::Receiver`**: for tests that attach a subscription without a real broadcaster, create `broadcast::channel::<ProjectEvent>(N)` and immediately `drop(_tx)` so `receiver.recv()` returns `None` (closed channel).
- **Clippy `field_reassign_with_default`**: `let mut x = T::default(); x.field = value;` triggers this lint. Use struct literal syntax: `let x = T { field: value, ..Default::default() };`.
