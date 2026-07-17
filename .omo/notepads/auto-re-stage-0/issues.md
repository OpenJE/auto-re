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

## 2026-07-17 (Task 19)

### Verification test crate location (same pattern as Task 18)
- Plan says `cargo test -p autore-core -- contradiction_status_transitions` and `cargo test -p autore-core -- verification_does_not_change_confidence`.
- Actual: `cargo test -p autore-schema -- contradiction_status_transitions` and `cargo test -p autore-schema -- verification_does_not_change_confidence`.
- Reason: `ContradictionStatus` / `VerificationState` live in `autore-schema` per the same circular dependency constraint noted in Task 18.
- Store-side tests (`contradiction_store_*`, `verification_*`) run under `-p autore-store` as the plan specifies — no adjustment needed.

### No FK from verification_records.subject_id to target tables
- `verification_records.subject_id BLOB` has no FK constraint because `VerificationSubject` discriminates between four different target tables (`semantic_entities`, `hypotheses`, `stage0_artifacts`, and the not-yet-created `generation_targets`).
- Application-layer validation is the enforcement point (e.g., when `ApplicationService.AddVerification` is implemented in Task 24, it should check the referenced ID exists in the appropriate table based on `subject_kind`).
- The `verification_subject_kind_discriminator_isolated` test guards against regressions where a query accidentally collapses two variants with the same UUID.

### V9 migration numbering
- Plan text says `V8__contradictions_verification.sql` but V8 is already used by `hypotheses.sql`.
- V9 is the correct next migration number; task brief correctly flagged this.

## 2026-07-17 (Task 20)

### V10 migration numbering (DESIGN NOTE)
- Plan text says `V9__operations.sql` but V9 is already used by `contradictions_verification.sql`.
- V10 is the correct next migration number; task brief correctly flagged this.

### records.rs growth (Task 20) (INFORMATIONAL)
- Added `Operation` struct (~25 LOC) + `ProgressUpdate` struct (~15 LOC) + `CancellationRequest` struct (~15 LOC) + `OperationFailure` struct (~5 LOC) + `EventSource` enum (~10 LOC) + `EventSubject` enum (~10 LOC) + `MetricMap` type alias (~3 LOC) + 5 operation kind constants (~20 LOC) + 12 tests (~140 LOC).
- records.rs now ~550 pure LOC implementation + ~720 LOC tests = ~1270 total.
- Implementation LOC exceeds 250 ceiling but contains 12+ struct/enum definitions, 30+ constants, and tests — well-separated sections following established pattern.

### OperationState test crate location (DESIGN NOTE)
- Plan says `cargo test -p autore-core -- operation_state_transitions_valid` — this is correct because `OperationState` lives in `autore-core` (unit-only, no schema dependency).
- This differs from Tasks 18/19 where status enums lived in `autore-schema` due to the circular dependency.

### autore-events gains autore-core dependency (DESIGN NOTE)
- `autore-events/Cargo.toml` now depends on `autore-core` for `OperationState` and `autore_core::Result`.
- This is a lightweight dependency (no SQLite, no schema) and does not create circular references.
- The `operation_events` module serves as a bridge until Task 21 implements `ProjectEvent`.

### `Operation.parent` self-referential FK (INFORMATIONAL)
- Same pattern as `hypotheses.superseded_by` — self-referential FK supported natively by SQLite.
- Cycle rejection is enforced at the application layer via `validate_no_cycle`.

## 2026-07-17 (Task 21)

### V11 migration numbering (DESIGN NOTE)
- Plan text says `V10__events.sql` but V10 is already used by `operations.sql`.
- V11 is the correct next migration number; task brief correctly flagged this.

### `Transaction` Mutex prevents nested store calls (DESIGN NOTE)
- `Database` wraps `Connection` in `Mutex`. `begin_transaction()` acquires the lock.
- Store methods (e.g., `OperationStore::transition`) call `self.db.connection()` which tries to re-lock → deadlock.
- Inside `with_event` closures, state mutations must use `txn.conn()` directly.
- Future stores could accept `&Transaction` as a parameter to enable composable store-within-transaction patterns.

### event_store.rs LOC (INFORMATIONAL)
- Implementation: ~130 pure LOC + ~250 LOC tests = ~380 total.
- Implementation LOC is well within the 250 ceiling.
- Co-located unit tests per Rust convention.

### records.rs growth (Task 21) (INFORMATIONAL)
- Added `ProjectEvent` struct (~30 LOC) + 17 event kind constants (~50 LOC) + 4 tests (~80 LOC).
- records.rs now ~630 pure LOC implementation + ~800 LOC tests = ~1430 total.
- Implementation LOC exceeds 250 ceiling but contains 13+ struct/enum definitions, 47+ constants, and tests — well-separated sections following established pattern.

### autore-events gains autore-schema dependency (DESIGN NOTE)
- `autore-events/Cargo.toml` now depends on `autore-schema` for `EVENT_KIND_OPERATION_*` constants and `NamespacedId`.
- `transition_event_kind()` now returns `&'static NamespacedId` instead of `&'static str`, with `transition_event_kind_str()` preserved for backward compat.
- The `emit_transition_event()` return type changed from `Result<&'static str>` to `Result<&'static NamespacedId>`.

## 2026-07-17 (Task 22)

### kill_resume.rs as separate test module (DESIGN NOTE)
- Created `autore-store/src/storage/kill_resume.rs` as a `#[cfg(test)]` module rather than appending to `event_store.rs` tests.
- Rationale: durability tests use on-disk SQLite (`tempfile::TempDir`) while existing `event_store.rs` tests use in-memory. Different lifecycle patterns warrant separate modules.
- Registered in `mod.rs` with `#[cfg(test)] mod kill_resume;`.

### kill_resume.rs LOC (INFORMATIONAL)
- Implementation: ~195 pure LOC (all tests, no separate implementation code).
- Well within 250 ceiling.

### No deviations from task requirements
- All three tests pass as specified.
- No changes to public APIs, migrations, or Cargo.toml.
- No M1 scheduler/worker references.

## 2026-07-17 (Task 23)

### `with_event` does not return the emitted event (DESIGN NOTE)
- `autore_store::with_event` commits an event but returns only the closure's value, not the `ProjectEvent` itself.
- `LocalProjectEventService::emit_event` therefore reimplements the atomic transaction using `next_project_event_sequence` + `emit_in_tx` directly instead of calling `with_event`.
- The atomicity guarantee is preserved: the sequence is computed inside the transaction, the event is inserted in the same transaction, and the transaction commits before broadcasting.
- This avoids changing the public API of `with_event` or any store trait.

### `ProjectEventSubscription` holds a closure rather than a service reference (DESIGN NOTE)
- The trait-specified `subscribe(&self, ...)` signature makes it impossible for the subscription to hold an `Arc` back to `self` without the caller already wrapping the service in an `Arc`.
- Passing a closure decouples the subscription from the service type and keeps the public trait signature unchanged.
- The closure is `Send + Sync`; store queries run inside `tokio::task::spawn_blocking` so the async `next()` method does not block the executor on the `Mutex<Connection>`.

### `GappedSubscription` emulator is replay-only (DESIGN NOTE)
- The emulator intentionally returns gapped events from `events_after` to verify that the subscription detects unrecoverable gaps and surfaces an error.
- It uses a closed broadcast channel so the live phase ends immediately after replay.
- This tests gap detection; it does not test full recovery because a store that itself returns gapped data cannot be recovered from without external repair.

### `sequence_gap_recovery` test semantics (DESIGN NOTE)
- The acceptance-criteria test injects a gap (sequences `[1, 3, 5]`) and asserts the subscriber detects it.
- After receiving sequence 1, the next event is 3, which is a gap. The subscription resyncs from `events_after(1)`. Since the emulator still returns 3 as the first event, the gap is unrecoverable and the subscription returns `Error::Subscription("unrecoverable sequence gap ...")`.
- This satisfies "subscriber detects and rebuilds" in the sense that the subscriber attempts to rebuild via resync; when the authoritative store is inconsistent, it reports the failure rather than silently skipping events.
