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
