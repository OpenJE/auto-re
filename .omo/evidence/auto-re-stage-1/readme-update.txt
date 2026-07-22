README.md update for Stage 1 completion
========================================

Date: 2026-07-22
Task: Update README.md to reflect completed Stage 1 implementation

Changes made:
1. Updated intro paragraph to describe Stage 1 as implemented (not deferred)
2. Updated Quick start with Stage 1 build commands and PROTOC requirement
3. Updated Architecture section: 8 crates -> 15 crates, split into Stage 0 and Stage 1 tables
4. Updated Building and testing: added PROTOC env var, --include-ignored flag, ~800+ test count
5. Added Stage 1 CLI usage sections: reconstruct, provider, work, generated, build, verification coverage
6. Updated TUI section: 7-pane -> 12-pane, added Campaign/WorkQueue/ActiveProviders/CompilerFailures/VerificationDiffs panes
7. Updated keybindings: added Alt+8..Alt+= for new panes, p/r/X/R/P for coordinator/work/provider actions
8. Updated Stage 0 vs Stage 1 section: Stage 1 marked as implemented with 7 new crates
9. Updated Further documentation: added stage1-report.md, stage1-architectural-test.md, stage1-completion-gate.md, stage1-audit.md

Source files consulted:
- autore-cli/src/cli.rs (exact subcommand names and flags)
- autore-tui/src/tui.rs (exact pane names and keybindings)
- autore-tui/src/tui/state.rs (Pane enum with 12 variants)
- docs/stage1-report.md (crate descriptions and capability counts)

Verification:
- cargo fmt --all --check: clean
- README.md: 355 lines (was 254)
- All Stage 0 content preserved
- All Stage 1 subcommands match CLI source
- All TUI panes match state.rs Pane enum
- All keybindings match tui.rs handle_key_event()
