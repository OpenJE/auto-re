# auto-re-stage-0 Issues Notepad

## 2026-07-17 Atlas: initial orchestration
- No blockers yet.

## 2026-07-17 Sisyphus: task 1 known issues
- **Resolved**: `autore-cli` no longer depends on `autore-stage1`. The root `auto-re` binary was moved to `autore-stage1` as `[[bin]] name = "auto-re"`, while `autore-cli/src/main.rs` became a Stage 0 placeholder. `cargo build` default-members now skips `autore-stage1`, and `cargo test -p autore-stage1` continues to pass `campaign_smoke`/`kill_resume` via `CARGO_BIN_EXE_auto_re`.
- **Optional `ida` feature does not compile**: `cargo check -p autore-stage1 --features ida` fails inside `idax-sys` C++ shim compilation (missing IDA SDK / struct member mismatches). This is an environmental/upstream issue, not caused by the workspace split; default-feature builds are unaffected.
- **M1 headless CLI logic is in `autore-stage1`, not `autore-cli`**: `src/cli/headless.rs` and `src/cli/headless_queries.rs` were moved to `autore-stage1/src/cli/` because they depend on M1 modules. The plain status/list CLI subcommands remain in `autore-stage1/src/cli/` as well since they share the same dispatch module; only the argument-parsing dispatch framework notionally belongs in Stage 0.

## 2026-07-17 Sisyphus: task 4 known issues
- No new blockers. `expectrl` v0.8 works on Linux; its `Expect` trait must be explicitly imported (`use expectrl::Expect`) for `.expect()` method resolution — non-obvious but documented in the smoke test. No V2 migration exists yet; `migration_runs_v1_then_v2` covers V1 tables and will automatically pick up V2 when added.
