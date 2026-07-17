# Issues — Task 5: Stage 0 Typed IDs

## 2026-07-17

### EntityId / ArtifactId naming collision (LOW priority)
- `ids::EntityId` (UUIDv7 newtype) and `domain::evidence::EntityId` (semantic enum) coexist.
- Fixed for autore-stage1 via explicit imports; new consumers need the same.
- Resolution: replace M1 enum when spec §6 fully adopted.

### ContentHash serde format breaking change
- Changed from bare hex string to tagged struct.
- No persisted data exists yet; no impact until persistence is implemented.

### Binary target name collision (RESOLVED)
- Both `autore-cli` and `autore-stage1` defined `[[bin]] name = "auto-re"`.
- Resolved: `autore-stage1` renamed to `auto-re-stage1`; `autore-cli` keeps `auto-re`.
- `kill_resume.rs` updated to use `CARGO_BIN_EXE_auto_re_stage1`.
- `cargo build --workspace` now warning-free.

## 2026-07-17 (Task 6)

### NamespacedId missing Ord/PartialOrd (RESOLVED)
- `MetadataMap` uses `BTreeMap<NamespacedId, ExtensionData>` which requires `K: Ord`.
- Added `PartialOrd, Ord` derives to `NamespacedId(String)` — valid since `String: Ord`.
- No semantic impact: lexicographic ordering on the inner string is correct for namespaced IDs.

### values.rs LOC (INFORMATIONAL)
- 316 pure LOC including `#[cfg(test)]` module (~150 LOC implementation, ~166 LOC tests).
- Exceeds 250 pure LOC threshold but implementation alone is within bounds.
- Co-locating unit tests with implementation is idiomatic Rust.
- Codebase already has larger files: `domain/mod.rs` (945), `claim.rs` (603), `evidence.rs` (367).
- No split planned; will refactor if the module grows beyond current scope.

## 2026-07-17 (Task 7)

### values.rs growth (INFORMATIONAL)
- Implementation: 166 pure LOC (under 250 ceiling). Tests: ~339 LOC.
- Added `ModuleIdentity`, refined `BinaryLocation`, `StableEntityKey` (4 variants), `DerivationMethod` (10 variants), `Derivation` struct.
- Re-exports updated in `domain/mod.rs`. No `autore-stage1` changes required (no call sites touched `BinaryLocation`).
- All 152 schema tests + 42 stage1 tests pass.

## 2026-07-17 (Task 8)

### SchemaMismatch uses String instead of SchemaVersion (DESIGN NOTE)
- `autore_core::Error::SchemaMismatch` uses `expected: String, actual: String` instead of `SchemaVersion` to avoid circular dependency (autore_schema → autore_core).
- Callers format SchemaVersion to string when constructing this variant.
- No loss of information since SchemaVersion has a Display impl.

### autore-tui::runtime::run() return type mismatch (RESOLVED)
- `autore_tui::runtime::run()` returns `autore_core::Result<()>` but stage1's `cli::run()` now returns `autore_stage1::Result<()>`.
- Resolved with `.map_err(crate::Error::from)` at the call site.
- `autore-tui` `Error::Worker` variant migrated to `Error::Operation` (core).

## 2026-07-17 (Task 9)

### JSON format layer redaction (RESOLVED)
- Initially the JSON formatter did not redact sensitive fields.
- Resolved with `RedactingJsonFormatter` — a custom `FormatEvent` that builds JSON output with the same `RedactingFieldVisitor` used by plain-text mode.
- Both JSON and plain-text modes now redact fields matching `*key*`, `*token*`, `*secret*`, `*password*`, `*credential*`.

### autore-core LOC with co-located tests (INFORMATIONAL)
- logging.rs: 202 pure LOC implementation + 153 LOC tests = 355 total.
- validation.rs: 161 pure LOC implementation + 217 LOC tests = 378 total.
- Implementation LOC is within the 250 ceiling. Tests are co-located per Rust convention.
- Existing codebase files are larger: domain/mod.rs (945), claim.rs (603), evidence.rs (367).

## 2026-07-17 (Task 10)

### ProjectManifest placement deviation (DESIGN NOTE)
- Task description says to put `ProjectManifest` in `autore-core`, but `autore-schema` depends on `autore-core` (for `Error::Validation`), making it impossible for `autore-core` to depend on `autore-schema` without circular dependency.
- Resolved: `ProjectManifest` lives in `autore-schema/src/manifest.rs` where it can use `Project` and other schema types.
- Test command adjusted: `cargo test -p autore-schema -- project_manifest_load_save` instead of `cargo test -p autore-core -- project_manifest_load_save`.

### NamespacedId validation extended to allow hyphens (RESOLVED)
- Original validation allowed only `[a-z0-9_]` per segment.
- Artifact kind constants (`core.source-tree`, `core.native-provider-output`, `core.generated-candidate`) require hyphens.
- Extended validation to `[a-z0-9_-]`. Updated error message accordingly.
- Existing tests for valid/invalid NamespacedIds still pass.

### records.rs LOC (INFORMATIONAL)
- records.rs: ~140 pure LOC implementation + ~170 LOC tests = ~310 total.
- Implementation LOC is well within the 250 ceiling.
- Co-located unit tests per Rust convention.

## 2026-07-17 (Task 11)

### database.rs LOC (INFORMATIONAL)
- database.rs: ~165 pure LOC implementation + ~200 LOC tests = ~365 total.
- Implementation LOC is well within the 250 ceiling.
- Co-located unit tests per Rust convention.

### project_store.rs LOC (INFORMATIONAL)
- project_store.rs: ~160 pure LOC implementation + ~120 LOC tests = ~280 total.
- Implementation LOC is well within the 250 ceiling.
- Co-located unit tests per Rust convention.

### `next_project_event_sequence` references future table (DESIGN NOTE)
- The method queries `project_events` which doesn't exist in V2 migration.
- It's a utility for use by later stage-0 migrations that add the events table.
- Tests create the table inline to verify the query logic.
- No runtime impact: method is only called when the table exists.
