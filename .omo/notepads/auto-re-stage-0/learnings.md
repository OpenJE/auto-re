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
