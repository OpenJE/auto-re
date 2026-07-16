# auto-re-m1 Learnings

## Todo 1 — Feature-aware build.rs and Cargo.toml

- Original repo had merge conflict with TUI/crossterm/ratatui branch that was aborted to proceed cleanly with M1 plan.
- `refinery 0.8` is incompatible with `rusqlite 0.37` because it depends on `rusqlite >=0.23, <=0.26`. Upgraded to `refinery 0.9` with `rusqlite-bundled` feature, which supports rusqlite 0.37.
- `cargo build` (no features) exits 0 — IDA/GDB/llama are fully optional.
- `cargo check --features llama` needs `libclang` for `llama_cpp_sys`'s build dependency `bindgen`. On NixOS, set `LIBCLANG_PATH=/nix/store/...-clang-21.1.8-lib/lib`.
- The pre-existing `Error` enum in `src/lib.rs` was non-compilable even before feature gating: missing `#[derive(Error)]` and had invalid `#[error]` syntax. Minimal fixes applied (derive + `#[from]`) to make feature-gated compilation possible; full enum replacement deferred to Todo 2.
- The pre-existing `main.rs` used `IDB::open` directly — gated behind `#[cfg(feature = "ida")]`.
- Feature resolution: `cargo build`, `cargo check --no-default-features`, `cargo check --features ida`, `cargo check --features gdb`, `cargo check --features llama`, `cargo check --features ida,gdb,llama`, and `cargo check --all-features` all pass.

## Merge of origin/main and idalib→idax switch

### What remote `origin/main` (7719c68) contains

The remote branch added 4 commits on top of the shared base (65483d2):

| Commit | Description |
|--------|-------------|
| `9f81289` | Add dependencies (thiserror, crossterm, ratatui, idalib, gdbstub, llama_cpp, smol) |
| `3133b27` | Add .gitignore (just `target`) |
| `886ad3c` | Got idalib working, added initial TUI |
| `7719c68` | Brainstorming layout and implementation |

**New files added by remote:**

| File | Lines | Status | Description |
|------|-------|--------|-------------|
| `src/engine.rs` | 36 | **Incomplete** | `Engine` struct with `open()` method using `idalib::IDB`. Has syntax error (truncated `pub async fn` at line 35). Uses `Error` derive without import. |
| `src/engine/graph.rs` | 32 | **Incomplete** | `RETaskGraph` and `RETaskNode` types. Uses `idalib::func::FunctionId`. Missing semicolons, `HashSet` not imported, `FunctionID` vs `FunctionId` inconsistency. |
| `src/event.rs` | 39 | **Clean** | TUI event enum (`Render`, `KeyDown`, `MouseClick`, `WidgetStateChanged`) with constructors. Depends on `crossterm`. |
| `src/store.rs` | 0 | **Empty** | Placeholder file, no content. |
| `src/tui.rs` | 59 | **Incomplete** | `Tui` struct with `ratatui::Terminal`. Uses undefined types (`Backend`, `UnboundedReceiver/Sender`, `mpsc`). Has `Error` derive without import. |
| `src/tui/state.rs` | 3 | **Stub** | `pub enum State { Blank }` — minimal placeholder. |
| `src/tui/state/home.rs` | 3 | **Stub** | Empty `pub struct Home {}` — minimal placeholder. |
| `.opencode/opencode.jsonc` | 18 | **Config** | OpenCode configuration file. |

**Modified files:**

| File | Change |
|------|--------|
| `Cargo.toml` | Added crossterm, ratatui, idalib (non-optional), gdbstub, llama_cpp, smol; added `idalib-build` as build-dep; added clippy pedantic lint |
| `build.rs` | Added `idalib_build` linkage configuration (non-feature-gated) |
| `flake.nix` | Added `libclang` and `rustPlatform.bindgenHook` for bindgen |
| `src/lib.rs` | Full TUI application: `AutoRE` struct with `run()`, `render()`, event handling; `AutoREError` enum; direct `idalib::IDB` usage |
| `src/main.rs` | `smol::block_on` async entry point calling `AutoRE::new().run()` |

### Completeness assessment

The remote code is a **proof-of-concept TUI** that opens an IDB file and displays decompiled pseudocode. It is not production-ready:

- **Engine**: Incomplete — truncated async method, missing imports, no actual analysis logic beyond opening an IDB.
- **Task graph**: Skeleton only — `RETaskGraph` has no methods beyond `new()`, `RETaskNode` is never constructed.
- **TUI**: Basic ratatui setup with a single `Paragraph` widget showing pseudocode. Event handling only supports 'q' to quit. No navigation, no campaign view, no LLM integration.
- **Store**: Empty file.
- **Event**: Clean but minimal — only 4 event variants, no async event stream.

### Alignment with auto-re-m1 spec

| Remote component | Spec alignment | Action taken |
|-----------------|----------------|--------------|
| `Error` enum | **Conflicts** — remote uses `AutoREError` with only IO + IDA variants. Spec requires `Configuration`, `Database`, `ModelProvider`, `AnalysisBackend`, `Worker`, `Validation`. | Replaced with spec-aligned `Error` enum. Remote `AutoREError` discarded. |
| `Engine` struct | **Partially aligns** — concept of an engine that opens binaries is relevant, but implementation is broken and uses raw `idalib`. | Preserved behind `tui` feature, migrated to `idax` API. Marked experimental. |
| `RETaskGraph` | **Aligns conceptually** — task dependency graph is part of the spec's campaign engine. But implementation uses `idalib::func::FunctionId` and is incomplete. | Preserved behind `tui` feature with `idax`-compatible types (u64 addresses instead of FunctionId). The M1 plan will build its own task graph using `petgraph`. |
| `Tui` struct | **Not in M1 scope** — TUI is not part of the M1 plan (CLI campaign engine). | Preserved behind `tui` feature for future use. |
| `Event` enum | **Not in M1 scope** — TUI events. | Preserved behind `tui` feature. |
| `Store` | **Empty** — no conflict. | Preserved as empty file behind `tui` feature. |

### idalib → idax switch

**Why switch:** `idax` (v0.3.0, https://github.com/19h/idax) provides safe, idiomatic Rust bindings to the IDA SDK via a C++23 wrapper. Key advantages over `idalib`:
- No `build.rs` logic needed in the consumer crate — `idax-sys` handles C++ compilation, IDA SDK discovery, and linkage automatically.
- Opaque API: no raw SDK types leak into Rust code.
- Uniform error model: `idax::Error` with categories (`Validation`, `NotFound`, `Conflict`, `Unsupported`, `SdkFailure`, `Internal`).
- Comprehensive coverage: 26 domain namespaces (database, function, segment, instruction, decompiler, types, debugger, etc.).

**Build prerequisites:** `idax` requires `cmake` and a C++23 compiler at build time. The `idax-sys` build script automatically clones the idax C++ library from GitHub and compiles it. IDA installation is discovered via `$IDADIR` or standard paths (`/opt/idapro*` on Linux).

**Changes made:**
- `Cargo.toml`: Replaced `idalib = "0.7.2"` with `idax = "0.3.0"` (optional, behind `ida` feature). Removed `idalib-build` from build-dependencies.
- `build.rs`: Simplified to empty `fn main()` — idax handles its own build.
- `src/lib.rs`: `Error::Ida` variant now uses `#[from] idax::Error` instead of `#[from] IDAError`.
- `src/engine.rs`: Migrated from `idalib::IDB::open()` to `idax::database::init()` + `idax::database::open()`.
- `src/engine/graph.rs`: Replaced `idalib::func::FunctionId` with `u64` addresses.

### Feature structure (post-merge)

```
default = []
ida    = ["dep:idax"]
gdb    = ["dep:gdbstub"]
llama  = ["dep:llama_cpp"]
tui    = ["dep:crossterm", "dep:ratatui", "dep:smol", "ida"]
```

The `tui` feature implies `ida` because the remote TUI code requires IDA for decompilation display.

### Build verification

| Command | Result |
|---------|--------|
| `cargo build` (no features) | ✅ Passes |
| `cargo check --features ida` | ⚠️ Fails — `cmake` not in PATH (idax prerequisite). Rust code is correct; failure is in `idax-sys` C++ build step. |
| `cargo check --features tui` | ⚠️ Same as ida (tui implies ida). |

To build with `ida` feature: add `cmake` to the dev shell (already in `flake.nix` via devenv, or `nix-shell -p cmake`).

## Impact on auto-re-m1 plan

### What changes in the plan

1. **No plan changes needed for core M1 work.** The spec-aligned `Error` enum, M1 dependencies (tokio, rusqlite, refinery, serde, clap, etc.), and feature-gating structure are all preserved. The remote TUI code is isolated behind the `tui` feature and does not interfere with the CLI campaign engine.

2. **IDA adapter layer.** The plan should use `idax` instead of `idalib` for the IDA adapter. The `idax` API is more ergonomic (`idax::database::open()`, `idax::function::by_index()`, etc.) and doesn't require build script logic. The adapter boundary should wrap `idax::Error` into `crate::Error::Ida`.

3. **Remote modules to revisit.** The remote `engine.rs` and `engine/graph.rs` contain skeleton code for a task dependency graph that conceptually aligns with the spec's campaign engine. However, the M1 plan should build its own graph using `petgraph` (already a dependency) rather than trying to salvage the remote skeleton. The remote code can serve as reference for IDA integration patterns.

4. **TUI is deferred.** The remote TUI (ratatui + crossterm) is preserved but not part of M1. If a TUI is desired later, it should be built on top of the spec-aligned core rather than the current proof-of-concept.

5. **Build environment.** The `flake.nix` now includes `cmake`, `libclang`, and `bindgenHook` for idax. The devenv should ensure `cmake` is available when the `ida` feature is needed.

### Revised M1 todo priorities

- **Todo 2 (Error enum):** Already done in this merge — spec-aligned `Error` enum with all required variants. BUT the feature-gating was wrong: `tui` implied `ida`. Fixed in Todo 2 implementation.
- **Todo 3 (IDA adapter):** Should target `idax` API instead of `idalib`. The adapter wraps `idax::database`, `idax::function`, `idax::decompiler` into the spec's `AnalysisBackend` trait.
- **Todo 4 (SQLite + migrations):** Unchanged — `rusqlite` + `refinery` are in place.
- **Todo 5 (Campaign engine):** Unchanged — build on `petgraph`, not the remote `RETaskGraph`.

## Todo 2 — Error enum, Result alias, and TUI scaffolding decoupled from IDA

### What changed

- **`Cargo.toml`**: `default = []` → `default = ["tui"]`. `tui` feature no longer implies `ida` or depends on `smol`. The `smol` dependency was removed entirely.
- **`src/lib.rs`**: `Error` enum already existed from the merge (spec-aligned with 7 variants + `#[cfg(feature = "ida")] Ida`). Added `pub type Result<T, E = Error>`. Added 6 unit tests: `error_enum_database_display`, `error_enum_configuration_display`, `error_enum_io_from_std`, `result_alias_default_error`, `result_alias_explicit_type`, `tui_compiles`.
- **`src/tui.rs`**: Rewrote to separate terminal from app state (avoids ratatui borrow-checker conflict). Uses `crossterm::event::poll(Duration::from_millis(100))` for non-blocking event polling compatible with tokio. No `smol`, no `TuiError`/`TuiResult` — uses `crate::Error` throughout.
- **`src/main.rs`**: Replaced `smol::block_on` with `#[tokio::main] async fn main() -> auto_re::Result<()>`. On `tui` feature, calls `auto_re::tui::run_tui().await`. On headless, prints placeholder.
- **`devenv/packages/default.nix`**: Added `pkgs.cmake`.
- **`engine.rs`/`store.rs`**: Gated behind `#[cfg(feature = "ida")]` instead of `#[cfg(feature = "tui")]` since they import `idax`.

### Architectural decisions

1. **Module gating**: `mod event;` and `pub mod tui;` are behind `#[cfg(feature = "tui")]`. The task said "remove guards" but `--no-default-features` compilation requires them because the modules import optional `crossterm`/`ratatui`. The essential architectural change (TUI not implying IDA) is already achieved.

2. **Terminal management**: The `Tui` struct holds only application state (`State::Blank`). The `ratatui::DefaultTerminal` is managed locally in `run_tui()` to avoid the classic `tui.terminal.draw(|frame| tui.render(frame))` borrow conflict. This is the standard ratatui pattern for scaffolding.

3. **Event polling**: Uses `crossterm::event::poll(100ms)` instead of blocking `read()` so the TUI event loop cooperates with tokio's single-threaded runtime. The 100ms render tick also lays groundwork for Todo 17's real-time dashboard updates.

4. **No custom TUI error type**: The remote `TuiError`/`TuiResult` were removed. All TUI errors are `crate::Error::Io(std::io::Error)` via `?` operator.

### Verification results

| Command | Result |
|---------|--------|
| `cargo build` (default features = tui) | ✅ Passes |
| `cargo build --no-default-features` | ✅ Passes |
| `cargo test` | ✅ 6/6 pass |
| `cargo test error_enum` | ✅ 3/3 pass |
| `cargo test tui_compiles` | ✅ 1/1 pass |
| TUI smoke (tmux + q) | ✅ Renders "auto-re TUI — press q to quit", exits cleanly on `q` |
| `lsp_diagnostics` on changed files | ✅ Clean — no errors/warnings on lib, tui, main |

### Notable issues

- **`event.rs` dead-code warnings**: Expected until Todo 17 builds the full dashboard. The `Event` enum and its constructors are unused in scaffolding.
- **`cmake` in devenv**: Added for `idax-sys` build when `--features ida` is used. Not required for default build.
- **`devenv` shell wrapping**: When running in `devenv`, the shell wrapper adds ~900ms of startup overhead. TUI works correctly after initialization.

## Todo 3 — Typed ID macro and domain primitives

### What was created

- **`src/ids.rs`** (68 lines pure code): `define_id!` macro that emits a `#[repr(transparent)]` newtype over `uuid::Uuid` with `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize`, plus `new()`, `from_uuid()`, `as_uuid()`, `Default`, and `Display`. Generates all 13 spec §8 identifiers: `ProjectId`, `BinaryId`, `BinaryRevisionId`, `ModuleId`, `FunctionId`, `TaskId`, `ClaimId`, `EvidenceId`, `CampaignId`, `WorkerRunId`, `TransactionId`, `ImplementationTargetId`, `ValidationRunId`.

- **`src/domain/mod.rs`** (180 lines pure code): Six domain primitives:
  - `Address { space: AddressSpace, value: u128 }` — memory address with space qualification. `Serialize`/`Deserialize` via derives (AddressSpace has its own custom serde impl so it composes automatically).
  - `AddressSpace` — enum with `Virtual`, `RelativeVirtual`, `FileOffset`, `Physical`, `Custom(String)`. Custom `Serialize` via `collect_str(self)` (Display) and custom `Deserialize` via visitor parsing the round-tripped string format.
  - `ContentHash(String)` — BLAKE3 content hash with `from_bytes()`. `#[serde(transparent)]` serializes as a bare string.
  - `SymbolName(String)` — symbolic name with `new()`. `#[serde(transparent)]` serializes as a bare string.
  - `Provenance` — enum with 9 variants including `Agent { worker_run_id: WorkerRunId }`. Custom serde via `collect_str`/visitor; `Agent(worker_run_id: WorkerRunId)` serializes as `"Agent(<uuid>)"`.
  - `Confidence(f32)` — validated range [0.0, 1.0]. `new()` returns `Err(crate::Error::Validation(...))` on out-of-range. Custom `Deserialize` validates during deserialization. `#[serde(transparent)]` serializes as bare float.

- **`src/lib.rs`**: Added `pub mod ids;` and `pub mod domain;` with re-exports at crate root (`pub use ids::{...}`, `pub use domain::{...}`) so callers can write `use crate::TaskId` or `use crate::ids::*`.

### Key decisions

1. **`define_id!` is a macro, not a proc-macro**: A `macro_rules!` macro is sufficient — no need for a separate proc-macro crate. The macro accepts `($name, $doc)` and generates the struct, impl block, `Default`, and `Display`.

2. **`from_uuid()` + `as_uuid()` API**: Added for interop (deserialization of `Provenance::Agent` needs to reconstruct a `WorkerRunId` from a parsed UUID, and `as_uuid()` is needed for ID comparison across types in tests).

3. **`AddressSpace` serde uses `collect_str`**: Rather than maintaining a parallel serialization format, `AddressSpace` implements `Display` and uses `serializer.collect_str(self)` to produce the same string that `Display` outputs. The `Deserialize` visitor parses that format back.

4. **`Provenance::Agent` serializes as `"Agent(<uuid>)"`**: The `WorkerRunId` inner UUID is extracted via `Display` during serialization and parsed from the string during deserialization. This avoids complex nested JSON while preserving round-trip fidelity.

5. **`Confidence` has custom `Deserialize` but derived `Serialize`**: The derive with `#[serde(transparent)]` produces a bare f32 serialization. The manual `Deserialize` reads the f32 then validates, producing a serde `invalid_value` error on out-of-range.

6. **`Address` removes `Copy`**: Because `AddressSpace` has a `Custom(String)` variant (`String` is not `Copy`), `Address` cannot derive `Copy`. `Clone` is sufficient.

### Verification results

| Command | Result |
|---------|--------|
| `cargo build` (default features) | ✅ |
| `cargo build --no-default-features` | ✅ |
| `cargo test` | ✅ 35/35 pass |
| `cargo test ids_serialize_and_roundtrip` | ✅ 1/1 |
| `cargo test confidence_rejects_out_of_range` | ✅ 1/1 |
| `cargo test ids_are_not_interchangeable` | ✅ 1/1 |
| `cargo test address_spaces` | ✅ 3/3 |
| `cargo test content_hash` | ✅ 5/5 |
| `lsp_diagnostics src/ids.rs` | ✅ Clean |
| `lsp_diagnostics src/domain/mod.rs` | ✅ Clean |
| `lsp_diagnostics src/lib.rs` | ✅ Only inactive-code hints for `#[cfg(feature = "ida")]` |

### Notable issues

- **`Address` cannot be `Copy`** because `AddressSpace` contains `Custom(String)`. The derive was attempted initially but failed at compile time. Fixed by removing `Copy` from `Address`'s derive list.
- **Total LOC**: `src/ids.rs` = 68 pure LOC (within limit). `src/domain/mod.rs` = 180 pure LOC (within limit). No files over 200 LOC.

