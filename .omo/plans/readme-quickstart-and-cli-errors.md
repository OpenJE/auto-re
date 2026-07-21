# Plan: README quick-start fix + CLI file-error context

**Date:** 2026-07-20
**Scope:** Two small, independent fixes that together remove the friction you hit when running the README quick start line-by-line.

---

## TODOs

- [x] 1. Apply README quick-start fix + CLI file-error context, verify, and commit

---

## Problem

Running the README quick start verbatim fails at the artifact-add step:

```
cargo run -p autore-cli -- artifact add --file ./target.bin --kind core.binary
Error: io error: No such file or directory (os error 2)
```

Two root causes:

1. **README quick start uses a non-existent file.** `./target.bin` is a placeholder; a reader running the commands in order has no such file. The example should use a file that already exists after the `cargo build` step earlier in the same quick start.

2. **CLI artifacts a raw `io::Error` with no path context.** `insert_artifact_managed` maps `std::fs::read(source_path)` straight to `Error::Io`, whose Display is `io error: No such file or directory (os error 2)` — no mention of *which* file was missing. The `evidence add` handler has the same gap: it says "failed to read evidence record file: ..." but omits the path.

---

## Changes

### 1. README quick start — use a real file

**File:** `README.md` (line 20)

**Before:**
```bash
# Register a binary artifact
cargo run -p autore-cli -- artifact add --file ./target.bin --kind core.binary
```

**After:**
```bash
# Register a binary artifact (import the project's own built binary as an example)
cargo run -p autore-cli -- artifact add --file ./target/debug/auto-re --kind core.binary
```

**Rationale:** `./target/debug/auto-re` always exists after the `cargo build` step earlier in the quick start, so the line runs successfully end-to-end. It also doubles as a realistic reverse-engineering example (importing the binary you're studying).

### 2. CLI artifact-add error — name the missing file

**File:** `autore-app/src/application_service/mutations.rs` (line 48)

**Before:**
```rust
let data = std::fs::read(source_path).map_err(Error::Io)?;
```

**After:**
```rust
let data = std::fs::read(source_path).map_err(|e| {
    Error::Io(std::io::Error::new(
        e.kind(),
        format!("failed to read artifact source file {}: {e}", source_path.display()),
    ))
})?;
```

**Resulting error:** `io error: failed to read artifact source file ./target.bin: No such file or directory (os error 2)`

The error kind (`std::io::ErrorKind`) is preserved by `std::io::Error::new`, so `is_not_found()` still works for any caller that checks it. No change to the `Error` enum or the `#[from]` derive.

### 3. CLI evidence-add error — name the missing file

**File:** `autore-cli/src/handlers.rs` (line 475-476)

**Before:**
```rust
let json_str = std::fs::read_to_string(&record)
    .map_err(|e| format!("failed to read evidence record file: {e}"))?;
```

**After:**
```rust
let json_str = std::fs::read_to_string(&record)
    .map_err(|e| format!("failed to read evidence record file {}: {e}", record.display()))?;
```

**Resulting error:** `failed to read evidence record file ./missing.json: No such file or directory (os error 2)`

---

## Validation

A single worker session executes all three edits, then:

1. `cargo fmt --all --check` — exit 0
2. `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` — exit 0
3. `cargo test --workspace --exclude autore-stage1` — exit 0 (existing tests unaffected; no new tests needed for a message-only change)
4. Hands-on: in a fresh temp dir, run the README quick start verbatim and confirm the `artifact add` line now succeeds (imports `./target/debug/auto-re`).
5. Hands-on: `cargo run -p autore-cli -- artifact add --file ./does-not-exist --kind core.binary` and confirm the error now names `./does-not-exist`.
6. Commit: `fix(cli,docs): name missing file in artifact/evidence errors and use a real file in README quick start`

---

## Out of scope

- No new error variants, no `Error` enum changes, no `anyhow`-style context layer.
- No changes to `autore-store` (its `std::fs::read` sites are internal, not user-facing CLI paths).
- No test additions beyond what already covers these handlers; the change is message-only.