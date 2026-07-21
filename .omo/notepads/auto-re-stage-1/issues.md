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
