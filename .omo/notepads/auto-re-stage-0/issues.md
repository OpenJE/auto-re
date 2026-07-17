# auto-re-stage-0 Issues Notepad

## 2026-07-17 Atlas: initial orchestration
- No blockers yet.

## 2026-07-17 Sisyphus: task 1 known issues
- **Deviation from strict rule**: `autore-cli` (a default-member crate) depends on `autore-stage1` because the root `auto-re` binary lives in `autore-cli` and must dispatch the M1 `campaign run` command. This was required to keep the binary buildable via `cargo build` default-members while preserving `campaign_smoke`/`kill_resume` tests. The dependency does not pull in `idax`/`gdbstub`/`llama_cpp` (those remain optional in `autore-stage1` only). A later wave may need to refactor the binary boundary to remove this edge.
- **Optional `ida` feature does not compile**: `cargo check -p autore-stage1 --features ida` fails inside `idax-sys` C++ shim compilation (missing IDA SDK / struct member mismatches). This is an environmental/upstream issue, not caused by the workspace split; default-feature builds are unaffected.
- **M1 headless CLI logic is in `autore-stage1`, not `autore-cli`**: `src/cli/headless.rs` and `src/cli/headless_queries.rs` were moved to `autore-stage1/src/cli/` because they depend on M1 modules. The plain status/list CLI subcommands remain in `autore-stage1/src/cli/` as well since they share the same dispatch module; only the argument-parsing dispatch framework notionally belongs in Stage 0.
