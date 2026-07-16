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

- **Todo 2 (Error enum):** Already done in this merge — spec-aligned `Error` enum with all required variants.
- **Todo 3 (IDA adapter):** Should target `idax` API instead of `idalib`. The adapter wraps `idax::database`, `idax::function`, `idax::decompiler` into the spec's `AnalysisBackend` trait.
- **Todo 4 (SQLite + migrations):** Unchanged — `rusqlite` + `refinery` are in place.
- **Todo 5 (Campaign engine):** Unchanged — build on `petgraph`, not the remote `RETaskGraph`.

