# Stage 0 Implementation Report

**Date:** 2026-07-18
**Spec reference:** §33
**Scope:** Final hardening pass and Stage 0 implementation summary for the `auto-re` workspace.

---

## 1. Implemented Schemas (Persisted Records)

Stage 0 defines the following canonical record types. Every type is stored in one or more SQLite tables; complex values are serialized as JSON TEXT or BLOB as noted.

| Record Type | Domain File | Table(s) | Notes |
|-------------|-------------|----------|-------|
| `Project` | `autore-schema/src/domain/records.rs` | `projects` | Top-level container. UUIDv7 BLOB PK, schema version, metadata JSON. |
| `Artifact` | `autore-schema/src/domain/records.rs` | `stage0_artifacts` | Content-addressed. Managed blobs copied under `artifacts/`; external files referenced by path. |
| `BinaryArtifactMetadata` | `autore-schema/src/domain/records.rs` | stored in `Artifact.metadata` as JSON | Optional extension metadata for binary artifacts. |
| `SemanticEntity` | `autore-schema/src/domain/records.rs` | `semantic_entities` | Optional `StableEntityKey` JSON for cross-revision identity. |
| `Provider` | `autore-schema/src/domain/records.rs` | `providers` | Tool/model/human descriptor. No project FK (global registry). |
| `ProviderRun` | `autore-schema/src/domain/records.rs` | `provider_runs` | Links provider, operation, input artifacts, environment JSON. |
| `ProviderEntityAlias` | `autore-schema/src/domain/records.rs` | `provider_entity_aliases` | Composite `(provider_run, provider_identifier)` unique key. |
| `NativeArtifact` | `autore-schema/src/domain/records.rs` | `native_artifacts` | Links `Artifact` + `ProviderRun` + subject entity UUIDs JSON. |
| `EvidenceRecord` | `autore-schema/src/domain/records.rs` | `evidence_records` | Append-only. Subject entity FK, value JSON, derivation JSON, native artifacts JSON. |
| `EvidenceLifecycleEvent` | `autore-schema/src/domain/records.rs` | `evidence_lifecycle_events` | Append-only lifecycle history for evidence records. |
| `Assumption` | `autore-schema/src/domain/records.rs` | stored in `evidence_records.assumptions` JSON | Inline in evidence record. |
| `Hypothesis` | `autore-schema/src/domain/records.rs` | `hypotheses` | Status + confidence JSON, supersession self-FK. |
| `Contradiction` | `autore-schema/src/domain/records.rs` | `contradictions` | Status + resolution JSON, evidence/hypotheses UUID arrays. |
| `ContradictionResolution` | `autore-schema/src/domain/records.rs` | stored in `contradictions.resolution` JSON | Attached when status becomes `Resolved`. |
| `VerificationRecord` | `autore-schema/src/domain/records.rs` | `verification_records` | Subject stored via `subject_kind` + `subject_id` discriminator. |
| `Operation` | `autore-schema/src/domain/records.rs` | `operations` | Long-running work, state machine, parent self-FK, failure JSON. |
| `ProgressUpdate` | `autore-schema/src/domain/records.rs` | `progress_updates` | Per-operation sequence + metrics JSON. |
| `CancellationRequest` | `autore-schema/src/domain/records.rs` | `cancellation_requests` | Cooperative cancellation request. |
| `ProjectEvent` | `autore-schema/src/domain/records.rs` | `project_events` | Append-only event stream. Per-project monotonic `sequence`. |
| `MigrationRecord` | `autore-store/src/migration.rs` | `migration_records` | Records V1→V2 migrations run by `MigrationService`. |

### Record Kinds / Constants (Selected)

- **Artifact kinds:** `core.binary`, `core.source-tree`, `core.native-provider-output`, `core.configuration`, `core.log`, `core.trace`, `core.generated-candidate`.
- **Entity kinds:** `core.function`, `core.type`, `core.global`, `core.string`, `core.external-function`, `core.source-symbol`.
- **Provider kinds:** `provider.disassembler`, `provider.decompiler`, `provider.debugger`, `provider.symbolic-executor`, `provider.llm`, `provider.human`.
- **Evidence predicates:** `evidence.predicate.function-name`, `evidence.predicate.function-signature`, `evidence.predicate.call-target`, `evidence.predicate.string-reference`, `evidence.predicate.type-info`, `evidence.predicate.control-flow`.
- **Operation kinds:** `core.artifact.import`, `core.project.validation`, `core.project.migration`, `core.project.rebuild-indexes`, `core.project.external-artifact-check`.
- **Project event kinds:** `core.project.created`, `core.artifact.registered`, `core.artifact.external-changed`, `core.entity.created`, `core.evidence.added`, `core.evidence.invalidated`, `core.hypothesis.proposed`, `core.hypothesis.accepted`, `core.hypothesis.rejected`, `core.contradiction.created`, `core.verification.recorded`, `core.operation.queued`, `core.operation.started`, `core.operation.progress`, `core.operation.completed`, `core.operation.failed`, `core.operation.cancelling`, `core.operation.cancelled`, `core.project.validation-failed`, `core.project.indexes-rebuilt`.

---

## 2. Storage Format

### Project Directory Layout

A project is a directory tree rooted at `<parent>/project.auto-re/`:

```
<parent>/
└── project.auto-re/
    ├── project.toml          # Manifest: schema_version, project_id, name, timestamps
    ├── project.sqlite3       # SQLite database with refinery migrations applied
    ├── artifacts/            # Managed blobs: <algo>/<prefix>/<digest>
    └── packages.lock         # Stub for future package-locking
```

Implemented in `autore-app/src/lifecycle.rs` (constants: `PROJECT_DIR_NAME`, `MANIFEST_FILE_NAME`, `DATABASE_FILE_NAME`, `ARTIFACTS_DIR_NAME`, `PACKAGES_LOCK_FILE_NAME`).

### Schema Version

- Current Stage 0 schema version: **`2.0`**.
- Stored in `Project.schema_version`, `projects.schema_version`, and `project.toml`.
- `lifecycle::open_project` rejects manifests with a schema version other than `2.0` with `Error::SchemaMismatch`.

### Artifact Layout

Managed blobs are stored under `<project_dir>/artifacts/<algorithm>/<2-char-prefix>/<digest>/data`:

- Example: `artifacts/sha256/ab/cdef0123.../data`.
- Content-addressed deduplication: identical content yields the same blob path; the database always receives a new `ArtifactId` row.
- External artifacts store the canonical filesystem path in `storage_path` and are verified on demand.

### Canonical vs Derived Tables

**Canonical tables** (source of truth, mutated only by application commands and store operations):

- `projects`, `stage0_artifacts`, `semantic_entities`, `providers`, `provider_runs`, `provider_entity_aliases`, `native_artifacts`, `evidence_records`, `evidence_lifecycle_events`, `hypotheses`, `contradictions`, `verification_records`, `operations`, `progress_updates`, `cancellation_requests`, `project_events`.

**Derived tables** (rebuilt from canonical tables only, never written by normal commands):

- `derived_project_summary` — per-project counts of every canonical record type.
- `derived_hypothesis_progress` — hypothesis status → count.
- `derived_evidence_progress` — evidence latest lifecycle state → count.
- `derived_reverse_references` — subject → referencing records (evidence, hypothesis, contradiction, verification).

Rebuild is exposed via `ApplicationCommand::RebuildIndexes` and implemented in `autore-store/src/storage/derived.rs`. A canonical-row hash snapshot proves the rebuild does not modify canonical data.

---

## 3. Application Commands and Queries

All commands/queries are defined in `autore-app/src/application_service/requests.rs` and dispatched by `ApplicationService` in `autore-app/src/application_service.rs`. Mutating commands are wrapped in a single SQLite transaction and emit a `ProjectEvent` atomically via `with_event`.

### Commands (`ApplicationCommand`)

| Variant | Request | Response | Event Emitted |
|---------|---------|----------|---------------|
| `CreateProject` | `CreateProjectRequest` | `CreateProjectResponse` | `core.project.created` |
| `RegisterArtifact` | `RegisterArtifactRequest` | `RegisterArtifactResponse` | `core.artifact.registered` |
| `RegisterEntity` | `RegisterEntityRequest` | `RegisterEntityResponse` | `core.entity.created` |
| `RegisterProvider` | `RegisterProviderRequest` | `RegisterProviderResponse` | none (Stage 0) |
| `StartProviderRun` | `StartProviderRunRequest` | `StartProviderRunResponse` | none (Stage 0) |
| `AddEvidence` | `AddEvidenceRequest` | `AddEvidenceResponse` | `core.evidence.added` |
| `AddHypothesis` | `AddHypothesisRequest` | `AddHypothesisResponse` | `core.hypothesis.proposed` |
| `ChangeHypothesisStatus` | `ChangeHypothesisStatusRequest` | `ChangeHypothesisStatusResponse` | `core.hypothesis.accepted` / `rejected` |
| `RecordContradiction` | `RecordContradictionRequest` | `RecordContradictionResponse` | `core.contradiction.created` |
| `AddVerification` | `AddVerificationRequest` | `AddVerificationResponse` | `core.verification.recorded` |
| `CancelOperation` | `CancelOperationRequest` | `CancelOperationResponse` | `core.operation.cancelling` |
| `ValidateProject` | `ValidateProjectRequest` | `ValidateProjectResponse` | `core.project.validation-failed` (only on failure) |
| `MigrateProject` | `MigrateProjectRequest` | `MigrateProjectResponse` | `core.operation.queued` |
| `RebuildIndexes` | `RebuildIndexesRequest` | `RebuildIndexesResponse` | `core.project.indexes-rebuilt` |

### Queries (`ApplicationQuery`)

| Variant | Returns |
|---------|---------|
| `GetProjectSummary` | `ProjectSummaryResponse` |
| `GetArtifact` | `ArtifactResponse` |
| `ListArtifacts` | `ArtifactsResponse` |
| `GetEntity` | `EntityResponse` |
| `ListEntities` | `EntitiesResponse` |
| `GetProvider` | `ProviderResponse` |
| `ListProviders` | `ProvidersResponse` |
| `GetProviderRun` | `ProviderRunResponse` |
| `ListProviderRuns` | `ProviderRunsResponse` |
| `GetEvidence` | `EvidenceResponse` |
| `ListEvidence` | `EvidenceListResponse` |
| `GetHypothesis` | `HypothesisResponse` |
| `ListHypotheses` | `HypothesesResponse` |
| `GetContradiction` | `ContradictionResponse` |
| `ListContradictions` | `ContradictionsResponse` |
| `GetVerification` | `VerificationResponse` |
| `ListVerifications` | `VerificationsResponse` |
| `GetOperation` | `OperationResponse` |
| `ListOperations` | `OperationsResponse` |
| `ListEvents` | `EventsResponse` |
| `GetValidationReport` | `ValidationReportResponse` |

### Client Interface

The `AutoReClient` trait (`autore-app/src/application_service/requests.rs`) provides the in-process interface used by CLI/TUI:

- `execute(command: ApplicationCommand) -> Result<CommandResult>`
- `query(query: ApplicationQuery) -> Result<QueryResult>`
- `events_after(project, sequence, limit) -> Result<Vec<ProjectEvent>>`
- `subscribe_events(project, after) -> Result<ProjectEventSubscription>`

`LocalAutoReClient` is the sole Stage 0 implementation; it wraps `Arc<ApplicationService>` and delegates directly with no network transport.

---

## 4. Ratatui Changes (M1 → Stage 0)

Original M1 TUI sources lived in the single-crate `src/` tree. Stage 0 splits them into `autore-tui` and defers all operational scheduler/model code to `autore-stage1`.

### Classification Summary

| Classification | Meaning | Original M1 Paths | Stage 0 Paths |
|---|---|---|---|
| **Retained unchanged** | Moved with no semantic changes | `src/event.rs` | `autore-events/src/lib.rs` |
| **Retained and adapted** | Moved with targeted changes | `src/tui.rs`, `src/tui/state.rs`, `src/runtime.rs`, `src/lib.rs`, `src/main.rs`, `src/storage/database.rs`, `src/ids.rs`, `src/domain/mod.rs` | `autore-tui/src/tui.rs`, `autore-tui/src/tui/state.rs`, `autore-tui/src/runtime.rs`, `autore-tui/src/lib.rs`, `autore-cli/src/main.rs`, `autore-store/src/storage/database.rs`, `autore-schema/src/ids.rs`, `autore-schema/src/domain/mod.rs` |
| **Moved behind shared services** | Types moved to shared crates, original module re-exports | `src/worker/output.rs` | `autore-schema/src/worker_output.rs` + `autore-stage1/src/worker/output.rs` re-export |
| **Deferred to Stage 1** | Operational code kept in `autore-stage1` | `src/analysis/*`, `src/cli/{campaign,task,headless,headless_queries}.rs`, `src/engine.rs`, `src/engine/graph.rs`, `src/model/*`, `src/scheduler/*`, `src/store.rs`, `src/worker/{mod,runner}.rs` | `autore-stage1/src/analysis/*`, `autore-stage1/src/cli/*`, `autore-stage1/src/engine.rs`, `autore-stage1/src/engine/graph.rs`, `autore-stage1/src/model/*`, `autore-stage1/src/scheduler/*`, `autore-stage1/src/store.rs`, `autore-stage1/src/worker/*` |
| **Removed / replaced** | M1 types replaced by Stage 0 ontology | `src/domain/campaign.rs`, `src/domain/claim.rs`, `src/domain/evidence.rs`, `src/domain/task/{mod,kind,types}.rs`, `tests/campaign_smoke.rs`, `tests/kill_resume.rs` | Replaced by `Project`, `Hypothesis`, `EvidenceRecord`, `Operation` and Stage 0 tests |

### Detailed TUI Disposition

| Original Path | Disposition | Explanation |
|---------------|-------------|-------------|
| `src/tui.rs` | **Adapted** → `autore-tui/src/tui.rs` | 4-panel layout retained; panel contents remapped from campaign/task/claim to project/operation/hypothesis/evidence; added secondary panes (Providers, NativeArtifacts, OpsDetail, EventsLog, MigrationHistory, ExternalArtifactIntegrity); generic fallback renderer added for unknown namespaced records. |
| `src/tui/state.rs` | **Adapted** → `autore-tui/src/tui/state.rs` | `DashboardState` → `TuiState`; holds `HashMap<ProjectId, ProjectViewState>`, `Navigation`, `Pane`, `FilterState`, `DialogState`; pure data with no storage references. |
| `src/tui/state/home.rs` | **Retained unchanged** → `autore-tui/src/tui/state/home.rs` | Empty `Home` placeholder preserved. |
| `src/runtime.rs` | **Adapted** → `autore-tui/src/runtime.rs` | Mock scheduler loop removed; now builds `LocalAutoReClient`, opens project, attaches live event subscription, and runs TUI event loop with `tokio::select!` over crossterm events, tick events, and subscription events. |
| `src/event.rs` | **Retained unchanged** → `autore-events/src/lib.rs` | Pure crossterm wrapper. |
| `src/main.rs` | **Adapted/split** | Stage 0 binary → `autore-cli/src/main.rs`; M1 binary → `autore-stage1/src/main.rs`. |
| `src/lib.rs` | **Adapted/split** | Split across `autore-app/src/lib.rs`, `autore-core/src/lib.rs`, `autore-schema/src/lib.rs`, `autore-store/src/lib.rs`, `autore-tui/src/lib.rs`. |

### TUI Verification Notes

- `grep -r 'rusqlite\|Database' autore-tui/src` returns no matches (storage access is strictly through `AutoReClient`).
- All write actions in the TUI route through `AutoReClient.execute` / `AutoReClient.query`.
- Terminal restoration is preserved via `tui_shutdown_restores_terminal`, `tui_panic_restores_terminal`, and `tui_shutdown_restores_terminal_with_active_operations` tests.

---

## 5. Architectural Decisions

### 3 Irreversible Forks

1. **UUIDv7 for all typed IDs**
   - `define_id!` in `autore-schema/src/ids.rs` uses `Uuid::now_v7()` instead of `Uuid::new_v4()`.
   - Rationale: time-ordered IDs give monotonic event sequencing and efficient database indexing for append-only logs.

2. **M1 domain model replaced by Stage 0 ontology**
   - `Campaign` → `Project`, `Task` → `Operation`, `Claim` → `Hypothesis`, M1 `Evidence` → `EvidenceRecord` + `EvidenceValue`, `Provenance` → `Derivation` + `DerivationMethod`, closed `EntityId` enum → opaque `EntityId` + namespaced kind.
   - Rationale: the spec defines the final ontology; carrying M1 names in shared crates creates migration debt.

3. **8-crate workspace with `autore-stage1` deferred**
   - Shared crates: `autore-schema`, `autore-core`, `autore-store`, `autore-events`, `autore-app`, `autore-cli`, `autore-tui`.
   - Operational M1 code isolated in `autore-stage1`, excluded from `default-members`.
   - Rationale: Stage 0 delivers domain/storage/events/TUI without pulling in IDA SDK, model providers, or scheduler; `autore-stage1` builds independently via `cargo build -p autore-stage1`.

### 7 Default Choices

1. **SQLite with refinery migrations**
   - Rationale: single-file, durable, well-supported; refinery gives ordered, versioned migrations.

2. **Application-assigned UUIDv7 BLOB primary keys**
   - No `AUTOINCREMENT`, no `DEFAULT uuid()` in SQL.
   - Rationale: portable IDs, deterministic serialization, and testable migration scripts.

3. **Atomic state + event transactions via `with_event`**
   - Mutations and the corresponding `ProjectEvent` are committed in one SQLite transaction.
   - Rationale: crash-recovery source of truth is the event log; partial writes are impossible.

4. **JSON TEXT for complex domain values**
   - Complex enums, arrays, metadata, and extension data serialize to JSON in TEXT columns.
   - Rationale: avoids schema churn for evolving domain types; acceptable for expected dataset sizes.

5. **Append-only evidence and event records**
   - `EvidenceRecord` and `ProjectEvent` have no update/delete APIs; lifecycle changes are new rows.
   - Rationale: auditability and reproducibility; event log is the authoritative history.

6. **Namespaced string identifiers for kinds/predicates/events**
   - `NamespacedId` replaces closed enums for artifact kinds, entity kinds, provider kinds, evidence predicates, operation kinds, event kinds, etc.
   - Rationale: runtime extensibility without code changes; uniform `core.*` / `provider.*` / `evidence.*` namespaces.

7. **Validation as a project-wide service in `autore-app`**
   - `ValidationService` lives in `autore-app` (not `autore-core`) because it queries multiple `autore-store` traits.
   - Rationale: avoids circular dependency (`autore-schema` → `autore-core`); `autore-core` keeps low-level validation primitives reusable.

---

## 6. Deferred Capabilities (Stage 1+)

The following M1/operational capabilities are intentionally deferred to `autore-stage1` and are not part of the Stage 0 shared surface:

| Capability | M1 Source | Stage 0 Status |
|---|---|---|
| Analysis backends (IDA integration, mock) | `src/analysis/` | `autore-stage1/src/analysis/`; behind `ida` feature. |
| Model providers / LLM routing | `src/model/` | `autore-stage1/src/model/`; external API dependencies. |
| Scheduler (lease-based task dispatch) | `src/scheduler/` | `autore-stage1/src/scheduler/`; core operational loop. |
| Worker runner | `src/worker/runner.rs` | `autore-stage1/src/worker/runner.rs`; execution engine. |
| RE engine / IDAGraph | `src/engine/`, `src/engine/graph.rs` | `autore-stage1/src/engine.rs`, `autore-stage1/src/engine/graph.rs`; experimental. |
| Headless CLI | `src/cli/headless*.rs` | `autore-stage1/src/cli/headless*.rs`; depends on scheduler. |
| Campaign/task CLI subcommands | `src/cli/{campaign,task}.rs` | `autore-stage1/src/cli/{campaign,task}.rs`; depends on M1 domain. |
| M1 campaign smoke / kill-resume integration tests | `tests/campaign_smoke.rs`, `tests/kill_resume.rs` | Retained in `autore-stage1/tests/` as Stage 1 regression tests; Stage 0 equivalents use Operation + events. |
| Provider-specific event kinds | n/a | Not defined in Stage 0; `RegisterProvider`/`StartProviderRun` emit no events. |
| Real progress-update computation in TUI | n/a | TUI shows heuristic progress percentages; actual `ProgressUpdate` records not loaded. |
| Multi-field artifact-import dialog | n/a | Single-field dialog; full form widget deferred. |
| External artifact integrity check command | n/a | `project check-artifacts` is scaffold only. |

---

## 7. Compatibility

### Supported Schema Versions

- **V1** — Original M1 schema (`migrations/V1__initial_schema.sql`). Tables: `campaigns`, `binary_revisions`, `modules`, `functions`, `tasks`, `claims`, `evidences`, `leases`, `artifacts`.
- **V2** — Stage 0 schema. Current version: `2.0`. Tables listed in Section 1 plus derived tables.

`autore-store` only opens V2 databases directly. V1 databases must be migrated via `MigrationService` (invoked by `ApplicationCommand::MigrateProject`).

### Migration Paths

- **V1 → V2:** `MigrationService::migrate` in `autore-store/src/migration.rs`:
  1. Copies source DB to destination.
  2. Creates a timestamped `*.bak` backup.
  3. Runs refinery migrations V2..V13 on the destination.
  4. Drops obsolete V1 tables (`functions`, `modules`, `binary_revisions`, `leases`, `tasks`, `campaigns`, `claims`, `evidences`) via `V12__drop_obsolete_v1.sql`.
  5. Records the migration in `migration_records`.
  6. Validates the migrated project with `ApplicationService::validate_project`.
- **V2 → V2:** `Database::open` is idempotent; re-running migrations is a no-op.
- **Forward incompatibility:** `lifecycle::open_project` rejects manifest schema versions other than `2.0`.

### Serialization Fixture Versions

Committed serialization fixtures in `autore-schema/tests/fixtures/` define the stable JSON representation of core values:

| Fixture | File | Version/Notes |
|---|---|---|
| `Project` | `project.json` | Current schema version `2.0`. |
| `Artifact` managed | `artifact_managed.json` | `ArtifactStorage` adjacently tagged. |
| `Artifact` external | `artifact_external.json` | `ArtifactStorage` adjacently tagged. |
| `ContentHash` SHA-256 | `content_hash_sha256.json` | `{ "algorithm": "sha256", "digest": "..." }`. |
| `NamespacedId` | `namespaced_id.json` | Bare string with exactly one dot. |
| `SchemaVersion` | `schema_version.json` | Bare string `"2.0"`. |
| `StableEntityKey` | `stable_entity_key.json` | Adjacently tagged enum. |
| `BinaryLocation` | `binary_location.json` | `ModuleIdentity` + RVA. |
| `Derivation` | `derivation.json` | Struct with `method`, `operation`, `supporting_evidence`, `source_hypotheses`. |
| `EvidenceValue` map | `evidence_value_map.json` | Adjacently tagged enum. |
| `ExtensionData` | `extension_data.json` | `{ schema, version, value }`. |
| `ProjectId` | `project_id.json` | UUID string. |
| Manifest | `project_manifest.toml` | `schema_version = "2.0"`, flat TOML. |

These fixtures are used by round-trip tests to detect accidental serialization changes.

---

## 8. Test Results

All gates were run on 2026-07-18 and produced exit code `0`. Full evidence is captured under `.omo/evidence/`:

- `.omo/evidence/task-39-auto-re-stage-0-gates.log` — combined summary.
- `.omo/evidence/task-39-fmt.log` — `cargo fmt --all --check`.
- `.omo/evidence/task-39-clippy-workspace.log` — workspace clippy.
- `.omo/evidence/task-39-test-workspace.log` — workspace tests.
- `.omo/evidence/task-39-build-stage1-no-default.log` — stage1 no-default-features build.
- `.omo/evidence/task-39-build-stage1.log` — stage1 default build.
- `.omo/evidence/task-39-test-default.log` — default-members tests.
- `.omo/evidence/task-39-clippy-default.log` — default-members clippy.
- `.omo/evidence/task-39-build-default.log` — default-members build.
- `.omo/evidence/task-39-pty-test.log` — PTY integration test.

### Unit / Store / Migration / CLI / TUI State Test Counts

From `cargo test --workspace --exclude autore-stage1`:

| Crate | Tests | Notes |
|---|---|---|
| `autore_app` (lib) | 28 passed | Includes validation, cross-project rejection, event emission. |
| `autore_app` (`persistence_round_trip`) | 1 passed | Full Stage 0 record round-trip across reopen. |
| `autore_cli` | 20 passed | CLI integration tests for create, add, list, validate, migrate, rebuild. |
| `autore_core` | 74 passed | Errors, logging, operation state machine, validation primitives. |
| `autore_events` | 12 passed | Project event subscription, broadcast, gap recovery. |
| `autore_schema` | 248 passed | IDs, domain values, records, serde fixtures. |
| `autore_store` (lib) | 158 passed | SQLite stores, migrations, kill-resume durability. |
| `autore_store` (`migration_fixture`) | 6 passed | V1 fixture → V2 migration scenarios. |
| `autore_store` (other integration) | 1 + 3 passed | Additional integration tests. |
| `autore_tui` (lib) | 56 passed | State machine + render tests. |
| `autore_tui` (`pty_integration`, ignored) | 0 passed, 1 ignored | Run separately with `--ignored`. |
| `autore_tui` (other integration) | 1 + 3 passed | Additional integration tests. |
| **Total unit/integration** | **609 passed** | 0 failed, 1 ignored (PTY). |
| Doc-tests | 5 passed | In `autore_core`. |
| **Grand total** | **614 passed** | 0 failed. |

### PTY Test

`cargo test -p autore-tui --test pty_integration -- --ignored --nocapture`:

- 1 passed, 0 failed, 0 ignored.
- Verified real `auto-re tui` binary renders primary screen, reflects side-process entity addition, and exits cleanly with terminal restored.

### Format / Clippy / Build

| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASS (exit 0) |
| `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` | PASS (exit 0) |
| `cargo clippy --all-targets -- -D warnings` (default-members) | PASS (exit 0) |
| `cargo build -p autore-stage1 --no-default-features` | PASS (exit 0) |
| `cargo build -p autore-stage1` | PASS (exit 0) |
| `cargo build` (default-members) | PASS (exit 0) |

### Migration Fixture Verification

`cargo test -p autore-store --test migration_fixture`:

1. Successful V1→V2 migration.
2. Failed migration rollback leaves source usable.
3. Backup `*.bak` is created and remains pristine V1.
4. Migration history recorded in `migration_records`.
5. Validation passes after migration.
6. Reopening migrated project via `lifecycle::open_project` succeeds.

All 6 scenarios passed.

---

## References

- Notepad: `.omo/notepads/auto-re-stage-0/learnings.md` and `.omo/notepads/auto-re-stage-0/issues.md`.
- Audit: `docs/stage0-audit.md`.
- Evidence: `.omo/evidence/task-39-auto-re-stage-0-gates.log` and sibling files.
