# F3 — Final Verification Wave QA Findings

Environment: NixOS Linux x86_64 (`shane-laptop 6.18.38`), Rust 1.95.0, cargo 1.95.0.  
All commands were executed in `/mnt/shanes-ssd-1tb/code/shane/auto-re`. No source code was modified.

## 1. Workspace test suite

Command:

```bash
cargo test --workspace --exclude autore-stage1
```

Result: **EXIT 0**  
All tests passed, no failures. Per-crate counts observed:

- `autore-app`: 28 unit + 1 persistence round-trip
- `autore-cli`: 20 integration tests
- `autore-core`: 74 unit tests
- `autore-events`: 12 unit tests
- `autore-schema`: 248 unit tests
- `autore-store`: 158 unit + 6 migration fixture + 1 migration smoke
- `autore-tui`: 56 unit + 1 PTY smoke + 3 regression tests
- `autore-core`: 5 doc-tests
- `autore-tui` PTY integration test: 1 ignored (run separately below)

Total: **613 passed, 0 failed, 1 ignored**.

## 2. `project create`

Command:

```bash
rm -rf /tmp/opencode/f3-create && mkdir -p /tmp/opencode/f3-create
cargo run -q -p autore-cli -- project create --name "f3-qa" --project-dir /tmp/opencode/f3-create
```

Result: **EXIT 0**  
Stdout:

```text
Project created: f3-qa (019f7727-b00b-7641-b936-31866d618ac4)
```

Created directory layout is correct:

```text
/tmp/opencode/f3-create/project.auto-re/
  project.toml
  project.sqlite3
  artifacts/
  packages.lock
```

Manifest contents confirm schema version 2.0 and project name `f3-qa`.

## 3. `project info`

Command:

```bash
cargo run -q -p autore-cli -- project info --project-dir /tmp/opencode/f3-create
```

Result: **EXIT 0**  
Stdout:

```text
Project: f3-qa
  ID:             019f7727-b00b-7641-b936-31866d618ac4
  Schema version: 2.0
  Created at:     2026-07-18T21:35:17.515052094Z
  Updated at:     2026-07-18T21:35:17.515052094Z
```

The project ID, name, and schema version match the manifest.

## 4. `project validate --output json`

Command:

```bash
cargo run -q -p autore-cli -- project validate --project-dir /tmp/opencode/f3-create --output json
```

Result: **EXIT 0**  
Stdout:

```json
{
  "$schema": "auto-re/schema/validation-report/v2.0",
  "findings": [],
  "passed": true,
  "project_id": "019f7727-b00b-7641-b936-31866d618ac4",
  "schema_version": "1.0.0"
}
```

Empty findings and `passed: true`.

## 5. `tui` manual start

No real terminal is attached to this headless session, so the TUI was launched inside a PTY using the same mechanism the integration test uses (`script` utility, 24x80 terminal).  
Command wrapper:

```bash
script -q -c "stty rows 24 cols 80; cargo run -q -p autore-cli -- --project-dir /tmp/opencode/f3-create tui" /tmp/opencode/f3-tui.script
```

After rendering was detected, `q` was injected to exit cleanly.

Result: **EXIT 0**  
Observations from the captured typescript:

- TUI entered the alternate screen (`ESC[?1049h`).
- Dashboard rendered with `Projects (1)`, `Dashboard`, `Operations`, `Hypotheses`, `Evidence`.
- Project name `f3-qa` appeared in the summary panel.
- Counts showed 0 artifacts/entities/evidence/hypotheses/contradictions/verifications.
- TUI left the alternate screen (`ESC[?1049l`) and restored the cursor (`ESC[?25h`) after `q`.

Conclusion: the TUI starts and renders a real project in this environment.

## 6. PTY integration test

Command:

```bash
cargo test -p autore-tui --test pty_integration -- --ignored --nocapture
```

Result: **EXIT 0**  
Output: `test pty_tui_lifecycle ... ok`  
This confirms the same `cargo run -p autore-cli -- tui` path works under a Linux PTY, opens the project, reacts to a side-process entity-add event, and exits cleanly.

---

## VERDICT: **APPROVE**

All F3 acceptance criteria pass:

1. `cargo test --workspace --exclude autore-stage1` passes with 0 failures.
2. `cargo run -p autore-cli -- project create --name "f3-qa"` creates a project and exits 0.
3. `cargo run -p autore-cli -- project info --project-dir <dir>` shows project info and exits 0.
4. `cargo run -p autore-cli -- project validate --project-dir <dir> --output json` exits 0 and reports `passed: true`.
5. `cargo run -p autore-cli -- tui --project-dir <dir>` starts, renders the dashboard, and exits cleanly via `q` (verified via a PTY because no real terminal is attached).
6. `cargo test -p autore-tui --test pty_integration -- --ignored --nocapture` passes on Linux.
7. No source code was modified; only temporary QA directories were created.

No findings require rejection.
