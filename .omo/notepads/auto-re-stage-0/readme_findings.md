
# README Findings

**Date:** 2026-07-19
**Scope:** README.md creation for auto-re Stage 0

## Sources consulted

- `autore-cli/src/cli.rs` — CLI structure, subcommands, flags, output format enum
- `Cargo.toml` — workspace members, default-members, resolver, dependencies
- `docs/stage0-report.md` — project layout, schema version, artifact/entity kinds, architecture, test counts, Stage 0 vs Stage 1 split
- `autore-tui/src/tui.rs` — keybindings, pane enum, dialog handling
- `autore-tui/src/tui/state.rs` — Pane enum variants

## Verification

- `cargo run -p autore-cli -- --help` — confirmed binary name `auto-re`, all 10 subcommands present, `--project-dir` global flag
- `cargo run -p autore-cli -- project --help` — confirmed 6 project subcommands including scaffold `check-artifacts`

## Key decisions

- Used exact CLI structure from `cli.rs` rather than paraphrasing
- Included all 7 artifact kinds and 6 entity kinds from stage0-report.md
- Listed all 7 TUI panes with their Alt+N shortcuts
- Documented that `autore-stage1` is excluded from `default-members` and must be built explicitly
- Noted schema version `2.0` and V1→V2 migration path
- Included test counts (614 passed) and PTY test command from stage0-report.md

## Accuracy notes

- All subcommands match `cli.rs` exactly
- Pane names match `Pane` enum in `state.rs`: Dashboard, Providers, NativeArtifacts, OperationsDetail, EventsLog, MigrationHistory, ExternalArtifactIntegrity
- Keybindings verified against `tui.rs` lines 336-426
- Project layout matches `autore-app/src/lifecycle.rs` constants
