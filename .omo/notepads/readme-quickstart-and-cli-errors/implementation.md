# Implementation: README quick-start fix + CLI file-error context

**Date:** 2026-07-20

## Changes made

1. **README.md:20** — Changed artifact add command from `./target.bin` to `./target/debug/auto-re` with updated comment.
2. **autore-app/src/application_service/mutations.rs:48** — Wrapped `std::fs::read` error in a closure that preserves the `ErrorKind` and includes the source path in the message.
3. **autore-cli/src/handlers.rs:475-476** — Added `record.display()` to the evidence file read error message.

## Verification

- `cargo fmt --all --check` — passes
- `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` — passes
- `cargo test --workspace --exclude autore-stage1` — passes
- Error path: `cargo run -p autore-cli -- artifact add --file ./does-not-exist --kind core.binary` produces error containing `./does-not-exist`
- README quick start: fresh temp dir, commands run verbatim, `artifact add` succeeds

## Commit

`fix(cli,docs): name missing file in artifact/evidence errors and use a real file in README quick start`
