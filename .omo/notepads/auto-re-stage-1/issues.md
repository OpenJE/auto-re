# Stage 1 Implementation Issues

## 2026-07-21 Session Start
- No issues yet.

## 2026-07-21 Wave 1 Todo 2 (App Commands)
- No blocking issues encountered.
- **Note**: Existing Stage 0 `ApplicationCommand`/`ApplicationQuery`/`CommandResult`/`QueryResult` enums only derive `serde::Serialize`, not `serde::Deserialize`. If a future todo needs enum-level deserialization (e.g., for wire transport), all Stage 0 request structs will need `Deserialize` added too. Stage 1 structs are forward-compatible with both.
- **Note**: Stage 1 request structs use `String` for domain-specific IDs (work_item_id, campaign_id, etc.). These should be replaced with typed IDs from `autore-schema` when the corresponding domain records are created in Todo 3+.

## Append only; never overwrite.
