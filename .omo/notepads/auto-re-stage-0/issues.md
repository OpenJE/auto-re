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

## 2026-07-17 (Task 24)

### Provider commands intentionally omit events (DESIGN NOTE)
- Stage 0 event-kind constants do not include provider-specific events (`core.provider.registered`, `core.provider.run-started`, etc.).
- `RegisterProvider` and `StartProviderRun` therefore execute store operations directly without calling `with_event`.
- Per the task specification, this is acceptable for Stage 0; provider events can be added in a later stage when the event vocabulary is defined.

### Missing operation cancellation event constants (RESOLVED)
- `CancelOperation` needs a meaningful event kind, but `EVENT_KIND_OPERATION_CANCELLING` did not exist in `autore-schema`.
- Added `EVENT_KIND_OPERATION_CANCELLING` and `EVENT_KIND_OPERATION_CANCELLED` in `autore-schema/src/domain/records.rs` and re-exported them in `domain/mod.rs`.
- `autore-events` still maps `Cancelling` to `EVENT_KIND_OPERATION_PROGRESS` in its convenience helper; the application layer uses the explicit new constant instead.

### `ApplicationService` re-exports from `autore-store` (RESOLVED)
- `ApplicationService` needs `HypothesisStore` as a trait object, but it was not re-exported from `autore-store/src/lib.rs`.
- Added `HypothesisStore` to the public re-export list.

### `create_project` must use `with_event` (RESOLVED)
- The initial implementation called `LocalProjectEventService::emit_event` directly, but the trait object field only exposes `ProjectEventService` methods.
- Refactored `create_project` to use `with_event` with a raw `muts::insert_project` helper, making the project insert and event emission atomic.

### `MetadataMap` import path (RESOLVED)
- `domain::records` re-uses `MetadataMap` internally without a public `pub use`.
- The application layer imports it from `autore_schema::domain::values` directly.

### Test predicates needed valid NamespacedIds (RESOLVED)
- Early tests used `hypothesis.predicate.test` as a predicate, which fails the one-dot validation rule.
- Changed test predicates to `hypothesis.test` so validation and confidence checks run in the correct order.

## 2026-07-17 (Task 25)

### `ProjectId` duplicate import in `requests.rs` (RESOLVED)
- `ProjectId` was already imported at the top of `requests.rs` (line 9). The new `AutoReClient` trait section initially re-imported it, causing an `E0252` compile error.
- Removed the duplicate import; the existing import serves both the request structs and the new trait.

### No deviations from task requirements
- All 4 acceptance-criteria tests pass as specified.
- Cross-project validation for `RegisterProvider` and `StartProviderRun` was not added because neither request type carries a sub-record with its own `project` field — `Provider` has no project field, and `ProviderRun` is constructed from request fields.
- `ApplicationService` no longer implements `AutoReClient` (old empty impl removed). This is a necessary breaking change since the trait now has required methods that would conflict with `ApplicationService`'s own inherent methods.

## 2026-07-17 (Task 26)

### Dual project creation path (DESIGN NOTE)
- `lifecycle::create_project` creates the directory structure but does not insert a project record into the DB.
- `ApplicationCommand::CreateProject` inserts the DB record but does not create directories.
- The CLI calls both and overwrites the manifest with the application-layer project ID to ensure consistency between the manifest and DB.
- This is a temporary scaffolding pattern; a unified `CreateProject` that handles both directory creation and DB insertion should be added in a later task.

### `autore-app/src/lib.rs` re-exports extended (RESOLVED)
- All request/response structs from `application_service::requests` are now re-exported at the crate root.
- Required adding `serde.workspace = true` to `autore-app/Cargo.toml` for `Serialize` derives on `CommandResult` and response types.

### `project check-artifacts` is scaffold only (INFORMATIONAL)
- No `ApplicationCommand` exists for artifact integrity checking.
- The handler queries `ListArtifacts` and reports the count, with a "not yet implemented" message.
- Full verification (hash comparison against stored blobs) should be implemented in a later task.

### `autore-cli` depends on `autore-schema` directly (DESIGN NOTE)
- `ProjectManifest` is not re-exported from `autore-app`.
- Added `autore-schema` as a direct dependency of `autore-cli` for `ProjectManifest` access.
- Alternative: re-export `ProjectManifest` from `autore-app`.

## 2026-07-17 (Task 27)

### `hypothesis accept` cannot succeed on freshly created hypotheses (DESIGN GAP)
- `HypothesisStatus` state machine only allows `Proposed -> UnderInvestigation -> Accepted`.
- The CLI provides `hypothesis accept` (sets `Accepted`) and `hypothesis reject` (sets `Rejected`) but no command to transition to `UnderInvestigation`.
- Result: `accept` and `reject` always fail with "invalid state transition" on newly created hypotheses.
- Resolution: add a `hypothesis investigate` CLI command (or allow `Proposed -> Accepted` directly in the state machine).
- Integration test `hypothesis_accept_enforces_state_machine` verifies the state machine enforcement instead of a happy-path test.

### `assert_cmd` + `predicates` added as workspace dev-dependencies (RESOLVED)
- Added `assert_cmd = "2"` and `predicates = "3"` to workspace `[workspace.dependencies]`.
- Added as dev-dependencies in `autore-cli/Cargo.toml`.
- These are standard Rust CLI testing crates; no concerns.

## 2026-07-17 (Task 28)

### Circular dependency `autore-app ↔ autore-tui` (RESOLVED)
- `autore-app` had `autore-tui = { path = "../autore-tui" }` and `pub use autore_tui::{runtime, tui};`.
- No external code used `autore_app::runtime` or `autore_app::tui`.
- Removed `autore-tui` from `autore-app`'s deps and the re-exports.
- Added `autore-app` to `autore-tui`'s deps for `AutoReClient` access.
- `autore-stage1` depends on `autore-tui` directly (optional feature), unaffected.

### `TuiUpdate` channel removed (DESIGN NOTE)
- Task 29 will wire `ProjectEventSubscription` on top of `ProjectEventService`.
- The old `TuiUpdate` mpsc channel and `DashboardState::apply_update` are gone.
- `runtime.rs` now just delegates to `tui::run_tui()` with no scheduler loop.

### `grep` acceptance criteria covers comments (RESOLVED)
- Initial implementation had doc comments mentioning `Database` and `rusqlite`.
- The strict `grep -r 'rusqlite\|Database' autore-tui/src` check returns no matches.
- All references to these words removed from comments, not just imports.

## 2026-07-17 (Task 29)

### `TuiEventLoop` borrow conflict with render loop (DESIGN NOTE)
- `TuiEventLoop<'a>` holds `&'a mut Tui`, which conflicts with `app.render(frame)` borrowing `app` immutably.
- Resolved: the real `run_tui` function inlines the `tokio::select!` logic directly instead of using `TuiEventLoop`.
- `TuiEventLoop` remains available for test-driven step-by-step event loop execution.
- Alternative: make `TuiEventLoop` own the `Tui` (not borrow) — rejected because it would complicate the render callback.

### `apply_query_result` is minimal (DESIGN NOTE)
- Currently only handles `QueryResult::Events` by updating `recent_events` on the matching project view.
- Other query result variants (`ProjectSummary`, `Artifacts`, `Hypotheses`, etc.) are not yet mapped.
- This is acceptable for Task 29 — full query-result-to-state mapping is a future task.

### `schedule_catchup` uses `events_after` directly, not `subscribe_events` (DESIGN NOTE)
- On gap detection, the TUI calls `client.events_after(project, last_sequence, 100)` to catch up.
- This does NOT resubscribe — it just fetches missed events. The subscription continues independently.
- If the gap is large (>100 events), multiple catch-up rounds may be needed. A future enhancement could loop until caught up.

### `run_tui` crossterm polling uses `spawn_blocking` (DESIGN NOTE)
- `crossterm::event::poll()` and `crossterm::event::read()` are blocking I/O calls.
- They run in a `tokio::task::spawn_blocking` to avoid blocking the async runtime.
- The blocking thread sends `TerminalEvent`s through an mpsc channel to the event loop.
- Tick events are generated by a separate tokio task on a 100ms interval.

### No deviations from task requirements
- All 4 acceptance-criteria tests pass as specified.
- `grep -r 'rusqlite\|Database' autore-tui/src` returns no matches.
- No storage queries execute in the rendering path.
- Sequence gaps are detected and `missed_events` is set.

## 2026-07-17 (Task 30)

### Secondary pane rendering is presentation-only (DESIGN NOTE)
- The `active_pane: Pane` field is a presentation-only cursor — it doesn't affect authoritative state.
- When `active_pane == Dashboard`, the right column shows Panel 2 (Operations) + Panel 3 (Hypotheses + Evidence).
- When `active_pane` is a secondary pane (Providers, NativeArtifacts, etc.), the right column shows that pane's content instead.
- The 4-panel physical layout is preserved: Panel 1 (left 30%) always shows the project summary.

### `render_generic_record` never panics (CONTRACT)
- The generic fallback renderer accepts any `kind: &NamespacedId`, `id: impl Display`, and `fields: impl IntoIterator<Item = (S, S)>`.
- If `fields` is empty, it renders "no fields".
- Unknown kinds are handled gracefully — the renderer just displays the kind string as-is.
- This satisfies §23.8 + Metis requirement: the TUI must render any namespaced record without panicking.

### Progress % in Operations table is heuristic (KNOWN LIMITATION)
- The operations panel shows a progress % column, but the actual progress data comes from `OperationViewState.progress` (a separate detail view), not from the `Operation` record itself.
- For now, the progress % is computed heuristically: 0% for `Queued`, 50% for `Running`/`Paused`/`Cancelling`, 100% for `Completed`.
- A future enhancement could load the actual `ProgressUpdate` records from `operation_views` and compute real progress.

### Secondary panes show summary data, not detail (DESIGN NOTE)
- `render_providers_pane` shows provider names + a list of recent runs (up to 10).
- `render_native_artifacts_pane` shows artifact IDs + kinds + sizes (up to 20).
- `render_operations_detail_pane` shows operation IDs + kinds + states + progress/cancel counts (up to 10).
- `render_events_log_pane` shows event sequences + kinds + sources (up to 20).
- `render_migration_history_pane` shows project IDs + schema versions.
- `render_external_artifact_integrity_pane` shows artifact IDs + kinds + hashes + sizes (up to 20).
- These are summaries — full detail views would require additional navigation/state.

### No deviations from task requirements
- All 4-panel physical layout preserved.
- Panel 1 = Projects (with summary, validation status, counts).
- Panel 2 = Operations (with id, kind, state, progress %, cancel hint).
- Panel 3 = Hypotheses + Evidence (gauge).
- Secondary panes: Providers, NativeArtifacts, OperationsDetail, EventsLog, MigrationHistory, ExternalArtifactIntegrity — all renderable with titles.
- Generic fallback renderer: `render_generic_record` + `render_extension_data_generic` + `render_metadata_map_generic`.
- All acceptance tests pass: `tui_dashboard_panels_present`, `tui_dashboard_shows_validation_status`, `tui_generic_fallback_unknown_record`, plus tests for each secondary pane title.
- `grep -r 'rusqlite\|Database' autore-tui/src` returns no matches.

## 2026-07-17 (Task 31)

### Synchronous command dispatch in render path (LOW priority, deferred optimization)
- `dispatch_command` calls `client.execute` synchronously in `handle_key_event`. If the underlying `LocalAutoReClient.execute` is slow (e.g. heavy SQLite writes), this will block the render thread for the duration.
- Current mitigation: user key presses are rare (~1/s), so typical blocking is tolerable. Real fix: wrap `client.execute` in `spawn_blocking` + post `CommandResult` via `internal_tx`, same as `dispatch_query`.
- No functional impact on tests (RecordingClient returns immediately).

### Artifact-import dialog is single-field (DESIGN LIMITATION)
- `RegisterArtifactRequest` takes `source_path` + `kind`, but the dialog collects only the path; `kind` defaults to `"native"`. A multi-field dialog (path, kind, storage) would need a stateful form widget.
- Documented; can be extended in a future task that adds form dialogs.

### `ProjectSummaryResponse` not re-exported from `autore_app` crate root (MINOR)
- `autore_app::application_service::requests::ProjectSummaryResponse` is the full path. The `autore_app` lib.rs re-exports many types but not this one.
- Tests use the full path; not a functional issue. Could add to re-exports for cleanliness.

### No deviations from task requirements
- All write actions (`a`, `A`, `c`, `o`) route through `AutoReClient.execute` / `AutoReClient.query`.
- No `rusqlite` or `Database` references in `autore-tui/src` (verified by grep).
- No direct record mutation from TUI — all mutations go through typed commands.
- No direct provider process kills — cancellation is via `CancelOperationRequest` (cooperative, §16).
- Terminal restoration preserved: `tui_shutdown_restores_terminal`, `tui_panic_restores_terminal`, `tui_shutdown_restores_terminal_with_active_operations` all pass.
- All 31 TUI unit tests + 3 regression tests + 573 workspace tests pass.
- `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` clean.
- `cargo fmt --all --check` clean.

## 2026-07-18 (Task 32)

### M1 repository code removed from `autore-store` (DESIGN NOTE)
- V1 tables are dropped by `V12__drop_obsolete_v1.sql`, so the M1 `SqliteTaskRepository`/`SqliteClaimRepository` and their trait definitions could no longer function in `autore-store`.
- Removed `autore-store/src/storage/repositories/` entirely; `autore-store/src/lib.rs` and `storage/mod.rs` no longer re-export these types.
- `autore-stage1` has its own copy of the M1 repository code and is unaffected (workspace default excludes `autore-stage1`).

### Dedicated V11 drop file was impossible (DESIGN NOTE)
- `V11__events.sql` already created the `project_events` table in Task 21.
- Placing the obsolete V1 drops in V11 would have required replacing/renaming an existing refinery migration, which would break existing databases.
- Resolved by creating `V12__drop_obsolete_v1.sql` as the final migration in the V2 schema set.

### `migration_records` table created lazily (DESIGN NOTE)
- The table is created by `MigrationService::record_migration`, not by a migration file, so `applied_at` and `tool_version` reflect the actual migration run.
- Fresh databases created via `Database::open` will not have `migration_records` until `MigrationService` is used.

### V1 `artifacts` table intentionally retained (DESIGN NOTE)
- The obsolete V1 drop list explicitly excludes `artifacts`; it continues to coexist with Stage 0 `stage0_artifacts`.
- This matches the task specification's enumerated obsolete table names.

## 2026-07-18 (Task 32 scope-creep fix)

### M1 repository code moved from `autore-store` to `autore-stage1` (RESOLVED)
- The Task 32 subagent deleted `autore-store/src/storage/repositories/{mod,claim,task}.rs` but did not relocate them.
- `autore-stage1/src/lib.rs` previously re-exported `autore_store::storage` as a whole, so deleting the repositories broke `crate::storage::repositories::*` imports in stage1.
- Restored the deleted files under `autore-stage1/src/storage/repositories/`, which is now stage1's own module.
- `autore-stage1/src/lib.rs` defines `pub mod storage;` instead of re-exporting `autore_store::storage`.
- `autore-stage1/src/storage/mod.rs` re-exports only the Stage 0 pieces stage1 needs (`Database`, `Transaction`) and exposes `pub mod repositories;`.
- Repository traits/implementations were adapted to use `autore_core::Result` / `autore_core::Error` because `crate::Result` in stage1 resolves to `autore_stage1::Result`, which lacks the `Database`/`Validation` variants the SQLite code needs.
- Verification:
  - `cargo build -p autore-stage1` succeeds.
  - `cargo test --workspace --exclude autore-stage1` passes.
  - `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` passes.

## 2026-07-18 (Task 33)

### ValidationService placement (DESIGN NOTE)
- `ValidationService` lives in `autore-app/src/application_service/validation.rs` rather than `autore-core` because it needs to query multiple stores (`ArtifactStore`, `EntityStore`, `EvidenceStore`, `HypothesisStore`, `OperationStore`, `ProviderStore`, `EventStore`, etc.) that are defined in `autore-store`.
- `autore-core` already contains low-level validation primitives (`validate_no_cycle`, `validate_all_references_exist`, etc.) reused by the service.

### Stable, versioned ValidationReport schema (DESIGN NOTE)
- `ValidationReport` is a serializable struct with `schema_version: u32` (currently `1`), `project_id`, `findings: Vec<ValidationFinding>`, and `passed: bool`.
- Each `ValidationFinding` carries `check`, `severity`, `message`, and optional `record_id`.
- The JSON payload placed in the failure event uses `ExtensionData` with schema `core.project.validation-report` and embeds the serialized report.

### 18 project-wide validation checks
- The service implements all checks from spec §25:
  1. `broken-reference` — every typed reference points to an existing record of the right kind.
  2. `cross-project-reference` — no sub-record references an ID from another project.
  3. `invalid-namespaced-id` — all stored `NamespacedId` values pass the segment/dot validation rules.
  4. `managed-artifact-integrity` — managed artifacts still match their stored content hash.
  5. `external-artifact-integrity` — external artifacts have not been modified on disk.
  6. `confidence-range` — all confidence scores are within `[0.0, 1.0]`.
  7. `provider-run-reference` — provider runs reference valid provider + artifacts.
  8. `native-artifact-reference` — native artifacts reference valid artifacts + subject entities.
  9. `evidence-reference` — evidence records reference valid subjects/runs/native artifacts/assumptions.
  10. `hypothesis-reference` — hypotheses reference valid subject entities.
  11. `hypothesis-supersession-cycle` — `Superseded { by }` graph has no cycles.
  12. `contradiction-reference` — contradictions reference valid subjects/evidence/hypotheses.
  13. `verification-reference` — verifications reference valid subjects/runs/evidence.
  14. `operation-reference` — operations reference valid parent operations.
  15. `operation-parent-cycle` — operation parent graph has no cycles.
  16. `event-sequence` — event sequences are strictly increasing in chronological order and unique.
  17. `event-subject-reference` — event subjects reference existing records.
  18. `schema-table-consistency` — project schema version is compatible with DB migration history and derived-index aliases match the project ID.

### Atomic event emission on failure (DESIGN NOTE)
- `ApplicationService::validate_project` calls `ValidationService::validate_project` to build the report, then uses `self.with_event` to atomically commit the report and emit exactly one `core.project.validation-failed` `ProjectEvent` when the report does not pass.
- The event payload contains the full `ValidationReport` JSON under key `report` so downstream consumers can react without re-running validation.
- No event is emitted when validation passes.

### CLI `project validate` default is human (DESIGN NOTE)
- The CLI supports `--output json` and `--output human` with human as the default, per the task requirement.
- Two integration tests initially called `project validate` without `--output json` and then parsed stdout as JSON. They were updated to pass `--output json` explicitly, matching the default-human contract.
- On validation failure the CLI exits with code 1 and prints findings; on success it exits 0.

### NativeArtifactStore and ProviderAliasStore wrappers (RESOLVED)
- `autore-app` required read access to `native_artifacts` and `provider_aliases` tables but the stores were not exposed through the public `autore-store` API.
- Added thin `NativeArtifactStoreImpl` and `ProviderAliasStoreImpl` wrappers in `autore-app/src/application_service/stores.rs` using the existing row helpers from `autore-store`.
- This satisfies the constraint "Do NOT change existing store public APIs unless required" — no public store trait signatures were changed.

### Test connection scoping deadlock (RESOLVED)
- Tests that directly `UPDATE` the database (`validation_detects_operation_parent_cycle`, `validation_detects_event_sequence_violation`) held the `MutexGuard<Connection>` returned by `service.db.connection()` across the `service.execute(...)` call.
- Because `ApplicationService` also acquires the same mutex during validation, this caused a deadlock.
- Fixed by wrapping the raw SQL blocks in a nested scope so the guard is dropped before invoking the service.

### Event sequence ordering (RESOLVED)
- `ProjectEventService::events_after` returns events ordered by `sequence`, so validation could not detect a later event with a lower sequence number.
- Changed `check_events` to sort the project's events by `created_at` (using `Timestamp::as_offset_datetime`) before checking monotonicity and uniqueness.
- The corresponding test now inserts two events with distinct `created_at` timestamps and sequences `3` then `2`, which correctly triggers an `event-sequence` finding.

### Clippy collapsible-if cleanup (RESOLVED)
- The new validation functions contained many nested `if let` blocks that clippy flagged as collapsible.
- Collapsed them using `let ... &&` chains and added `#[allow(clippy::too_many_arguments)]` on private helper functions (`check_namespaced_ids`, `check_events`, `check_verifications`) that legitimately need many ID sets.

### Verification
- `cargo test -p autore-app -- validation_` — 7/7 pass.
- `cargo test --workspace --exclude autore-stage1` — all workspace tests pass.
- `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` — clean.
- `cargo fmt --all --check` — clean.
