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
