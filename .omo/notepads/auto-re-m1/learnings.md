# auto-re-m1 Learnings

## Todo 1 — Feature-aware build.rs and Cargo.toml

- Original repo had merge conflict with TUI/crossterm/ratatui branch that was aborted to proceed cleanly with M1 plan.
- `refinery 0.8` is incompatible with `rusqlite 0.37` because it depends on `rusqlite >=0.23, <=0.26`. Upgraded to `refinery 0.9` with `rusqlite-bundled` feature, which supports rusqlite 0.37.
- `cargo build` (no features) exits 0 — IDA/GDB/llama are fully optional.
- `cargo check --features llama` needs `libclang` for `llama_cpp_sys`'s build dependency `bindgen`. On NixOS, set `LIBCLANG_PATH=/nix/store/...-clang-21.1.8-lib/lib`.
- The pre-existing `Error` enum in `src/lib.rs` was non-compilable even before feature gating: missing `#[derive(Error)]` and had invalid `#[error]` syntax. Minimal fixes applied (derive + `#[from]`) to make feature-gated compilation possible; full enum replacement deferred to Todo 2.
- The pre-existing `main.rs` used `IDB::open` directly — gated behind `#[cfg(feature = "ida")]`.
- Feature resolution: `cargo build`, `cargo check --no-default-features`, `cargo check --features ida`, `cargo check --features gdb`, `cargo check --features llama`, `cargo check --features ida,gdb,llama`, and `cargo check --all-features` all pass.

