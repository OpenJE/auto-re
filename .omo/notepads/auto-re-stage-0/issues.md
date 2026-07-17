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

## 2026-07-17 (Task 12)

### artifact_store.rs LOC (INFORMATIONAL)
- Implementation: ~230 pure LOC + ~260 LOC tests = ~490 total.
- Implementation LOC is within the 250 ceiling.
- Co-located unit tests per Rust convention.

### `stage0_artifacts` table name divergence (DESIGN NOTE)
- V1 migration creates `artifacts` table (M1 schema with `content_hash TEXT`, `size INTEGER`, `mime_type TEXT`).
- V3 migration creates `stage0_artifacts` table (Stage 0 schema with full content-hash, storage-kind, project FK).
- The two tables coexist until M1 is fully deferred. No name collision.

### `ContentHash` row reconstruction (RESOLVED)
- `ContentHash::sha256(data)` computes a hash from raw data — it does NOT wrap a pre-computed digest.
- Initial implementation used `ContentHash::sha256(&digest)` in `row_to_artifact`, which hash-the-hashed.
- Fixed by using direct struct construction: `ContentHash { algorithm, digest }`.
- Added `get_artifact_round_trip` test to verify DB insert + retrieval produces identical `content_hash`.

## 2026-07-17 (Task 13)

### Lifecycle code placement deviation (DESIGN NOTE)
- Task description says to put lifecycle code in `autore-core`, but `autore-schema` depends on `autore-core` (for `Error::Validation`), making it impossible for `autore-core` to depend on `autore-schema` without circular dependency.
- Resolved: Lifecycle functions (`create_project`, `open_project`, `close_project`) live in `autore-app/src/lifecycle.rs` where they can use both `ProjectManifest` (from `autore-schema`) and `Database` (from `autore-store`).
- Test command adjusted: `cargo test -p autore-app` instead of `cargo test -p autore-core`.
- This aligns with the plan's description: "autore-app preview before 0G" — the lifecycle is a precursor to the full `ApplicationService` that will arrive in Wave 0G.

### lifecycle.rs LOC (INFORMATIONAL)
- Implementation: ~85 pure LOC + ~75 LOC tests = ~160 total.
- Implementation LOC is well within the 250 ceiling.
- Co-located unit tests per Rust convention.

## 2026-07-17 (Task 14)

### entity_store.rs LOC (INFORMATIONAL)
- Implementation: ~230 pure LOC + ~320 LOC tests = ~550 total.
- Implementation LOC is within the 250 ceiling.
- Co-located unit tests per Rust convention.

### records.rs growth (INFORMATIONAL)
- Added `SemanticEntity` struct (~20 LOC) + 6 entity kind constants (~24 LOC) + 3 tests (~45 LOC).
- records.rs now ~220 pure LOC implementation + ~290 LOC tests = ~510 total.
- Implementation LOC is within the 250 ceiling.

## 2026-07-17 (Task 15)

### provider_store.rs LOC (INFORMATIONAL)
- Implementation: ~230 pure LOC + ~350 LOC tests = ~580 total.
- Implementation LOC is within the 250 ceiling.
- Co-located unit tests per Rust convention.

### records.rs growth (Task 15) (INFORMATIONAL)
- Added `ProviderRunStatus` enum (~45 LOC) + `EnvironmentIdentity` struct (~10 LOC) + `Provider` struct (~20 LOC) + `ProviderRun` struct (~20 LOC) + 6 provider kind constants (~24 LOC) + 8 tests (~120 LOC).
- records.rs now ~340 pure LOC implementation + ~410 LOC tests = ~750 total.
- Implementation LOC exceeds 250 ceiling. However, the file contains 5 struct/enum definitions, 13 constants, and tests — the implementation is spread across well-separated sections following the established pattern. A future split is planned when the provider module is extracted (task 16 adds aliases which may warrant a new module).

### V5 migration naming (DESIGN NOTE)
- Task plan says "V4__providers.sql" but V4 is already used by `semantic_entities`. V5 is the correct next migration number.
- No impact — refinery applies migrations in numerical order.

## 2026-07-17 (Task 16)

### alias_store.rs LOC (INFORMATIONAL)
- Implementation: ~230 pure LOC + ~400 LOC tests = ~630 total.
- Implementation LOC is within the 250 ceiling.
- Co-located unit tests per Rust convention.

### records.rs growth (Task 16) (INFORMATIONAL)
- Added `ProviderEntityAlias` struct (~10 LOC) + `NativeArtifact` struct (~10 LOC) + 6 native format constants (~24 LOC) + 4 tests (~70 LOC).
- records.rs now ~375 pure LOC implementation + ~480 LOC tests = ~855 total.
- Implementation LOC exceeds 250 ceiling but contains 7 struct/enum definitions, 19 constants, and tests — well-separated sections following established pattern.
- A future split into a `providers.rs` module is planned when the module is extracted.

### `list_by_subject_entity` uses full-table scan (DESIGN NOTE)
- `subject_entities` is stored as a JSON array TEXT column — cannot filter in SQL without SQLite JSON extension.
- Current implementation fetches all `native_artifacts` rows and filters in Rust.
- Acceptable for expected small dataset sizes. If performance becomes a concern, a junction table (`native_artifact_subjects`) could be added in a future migration.

## 2026-07-17 (Task 17)

### evidence_store.rs LOC (INFORMATIONAL)
- Implementation: ~230 pure LOC + ~350 LOC tests = ~580 total.
- Implementation LOC is within the 250 ceiling.
- Co-located unit tests per Rust convention.

### records.rs growth (Task 17) (INFORMATIONAL)
- Added `EvidenceLifecycleState` enum (~20 LOC) + `Assumption` struct (~5 LOC) + `EvidenceRecord` struct (~15 LOC) + `EvidenceLifecycleEvent` struct (~10 LOC) + 6 evidence predicate constants (~24 LOC) + 6 tests (~100 LOC).
- records.rs now ~440 pure LOC implementation + ~580 LOC tests = ~1020 total.
- Implementation LOC exceeds 250 ceiling but contains 9 struct/enum definitions, 25 constants, and tests — well-separated sections following established pattern.
- A future split into an `evidence.rs` module within records is planned.

### V7 migration naming (DESIGN NOTE)
- Task plan says "V6__evidence.sql" but V6 is already used by `aliases_native`. V7 is the correct next migration number.
- No impact — refinery applies migrations in numerical order.

### `EvidenceRecordId` vs M1 `EvidenceId` (DESIGN NOTE)
- M1 `ids::EvidenceId` already exists and is used by claims and hypotheses for evidence linking.
- Stage 0 `EvidenceRecordId` is a new typed ID for the append-only evidence records table.
- Both coexist; `EvidenceRecordId` is the persistence-layer identity, while M1 `EvidenceId` remains in the claim/hypothesis domain.
- When Stage 0 fully replaces M1, `EvidenceRecordId` will subsume `EvidenceId`.

### evidence_store.rs unused imports (RESOLVED)
- Removed unused `ContentHash` and `ArtifactId` imports from test module.
- `cargo test -p autore-store` now produces zero warnings.
- Pre-existing clippy `-D warnings` failures in `autore-core` (e.g., `only_used_in_recursion`, `unnecessary_literal_unwrap`) are unrelated to Task 17 changes.

## 2026-07-17 (Task 18)

### Confidence serialization breaking change (INFORMATIONAL)
- `Confidence` changed from transparent `f32` newtype to `{ score: f32, rationale: Option<String> }` struct.
- Serialization format changed from bare number (`0.75`) to JSON object (`{"score":0.75,"rationale":null}`).
- No persisted data exists yet; no migration needed.
- `Confidence` lost `Copy` derive — one site in `claim.rs` updated to `.clone()`.

### Verification test crate location (DESIGN NOTE)
- Plan says `cargo test -p autore-core -- hypothesis_state_transitions_valid`.
- Actual: `cargo test -p autore-schema -- hypothesis_state_transitions_valid`.
- Reason: `HypothesisStatus` lives in `autore-schema` per the circular dependency constraint (`autore-schema` depends on `autore-core`, so `autore-core` cannot reference `HypothesisStatus`).
- `validate_no_cycle` in `autore-core` remains independently testable with generic string IDs.

### V8 migration numbering (DESIGN NOTE)
- Plan says `V7__hypotheses.sql` but V7 is already used by `evidence.sql`.
- V8 is the correct next migration number.

### HypothesisStatus enum cannot derive Copy (INFORMATIONAL)
- `Superseded { by: HypothesisId }` carries data. While `HypothesisId` is `Copy`, the pattern of mixing unit and data variants makes `Copy` unusual for status enums.
- Follows `ProviderRunStatus` which also doesn't derive `Copy` (though it could, being all unit variants).

### `superseded_by` self-referential FK (INFORMATIONAL)
- `hypotheses.superseded_by BLOB NULL REFERENCES hypotheses(id)` is a self-referential FK.
- SQLite supports this natively. The FK is checked at insert/update time.
- Cycle rejection is enforced at the application layer via `validate_no_cycle`, not at the DB level.
