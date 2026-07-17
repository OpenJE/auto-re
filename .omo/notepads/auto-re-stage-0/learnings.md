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
