# auto-re

`auto-re` is a reverse-engineering project manager. It tracks binary artifacts, semantic entities, evidence, hypotheses, contradictions, and verifications across a project's lifetime, with an append-only event log as the source of truth.

Stage 0 delivers the domain ontology, SQLite storage, application service, CLI, and TUI. Operational code (IDA integration, model providers, scheduler, worker runner) lives in `autore-stage1` and is excluded from the default workspace build.

## Quick start

```bash
# Build the default workspace (excludes autore-stage1)
cargo build

# Create a new project in the current directory
cargo run -p autore-cli -- project create --name my-binary

# Inspect it
cargo run -p autore-cli -- project info

# Register a binary artifact
cargo run -p autore-cli -- artifact add --file ./target.bin --kind core.binary

# Register a semantic entity
cargo run -p autore-cli -- entity add --kind core.function --display-name main

# Run project-wide validation
cargo run -p autore-cli -- project validate

# Launch the TUI
cargo run -p autore-cli -- tui
```

All commands accept the global `--project-dir <PATH>` flag, which points to the parent directory containing `project.auto-re/`. It defaults to `.`.

## Project layout

A project is a directory tree rooted at `<parent>/project.auto-re/`:

```
<parent>/
└── project.auto-re/
    ├── project.toml          # Manifest: schema_version, project_id, name, timestamps
    ├── project.sqlite3       # SQLite database with refinery migrations applied
    ├── artifacts/            # Managed blobs: <algo>/<prefix>/<digest>/data
    └── packages.lock         # Stub for future package-locking
```

The current schema version is `2.0`. Manifests with any other version are rejected at open time.

## Architecture

The workspace is split into eight crates:

| Crate | Role |
|---|---|
| `autore-schema` | Domain records, typed IDs (UUIDv7), namespaced kinds, serde fixtures |
| `autore-core` | Low-level validation primitives, errors, logging, operation state machine |
| `autore-store` | SQLite storage, refinery migrations, derived tables, migration service |
| `autore-events` | Project event subscription and broadcast |
| `autore-app` | `ApplicationService`, commands, queries, validation service, lifecycle |
| `autore-cli` | Clap-based CLI binary (`auto-re`) |
| `autore-tui` | Ratatui TUI with 7-pane dashboard |
| `autore-stage1` | Deferred operational code (IDA, model providers, scheduler, workers) |

All mutations route through `ApplicationCommand`; all reads through `ApplicationQuery`. Each command and its resulting `ProjectEvent` commit atomically in a single SQLite transaction.

### Artifact kinds

`core.binary`, `core.source-tree`, `core.native-provider-output`, `core.configuration`, `core.log`, `core.trace`, `core.generated-candidate`.

### Entity kinds

`core.function`, `core.type`, `core.global`, `core.string`, `core.external-function`, `core.source-symbol`.

## Building and testing

```bash
# Build default members (excludes autore-stage1)
cargo build

# Build everything including stage1
cargo build --workspace

# Run default-members tests (614 tests)
cargo test --workspace --exclude autore-stage1

# Run the PTY integration test separately
cargo test -p autore-tui --test pty_integration -- --ignored --nocapture

# Format and lint
cargo fmt --all --check
cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings
```

`autore-stage1` is excluded from `default-members` because it depends on external SDKs. Build it explicitly with `cargo build -p autore-stage1`.

## CLI usage

The binary is `auto-re` (crate `autore-cli`).

```
auto-re [OPTIONS] [COMMAND]

Global options:
      --project-dir <PROJECT_DIR>  Parent directory containing project.auto-re/ [default: .]
  -h, --help                       Print help
  -V, --version                    Print version
```

Read commands accept `--output human` (default) or `--output json`.

### project

```bash
auto-re project create --name <NAME>
auto-re project info [--output json]
auto-re project validate [--output json]
auto-re project migrate
auto-re project rebuild-indexes [--output json]
auto-re project check-artifacts          # scaffold, not yet implemented
```

### artifact

```bash
auto-re artifact add --file <PATH> --kind <KIND>
auto-re artifact list [--output json]
auto-re artifact show --id <UUID> [--output json]
```

### entity

```bash
auto-re entity add --kind <KIND> [--display-name <NAME>] [--stable-key <JSON>]
auto-re entity list [--output json]
auto-re entity show --id <UUID> [--output json]
```

### evidence

```bash
auto-re evidence add --record <PATH>     # JSON file containing an EvidenceRecord
auto-re evidence list [--output json]
```

### hypothesis

```bash
auto-re hypothesis add --subject <UUID> --predicate <PRED> --candidate <JSON> --confidence <0.0-1.0>
auto-re hypothesis list [--output json]
auto-re hypothesis accept --id <UUID>
auto-re hypothesis reject --id <UUID>
```

### contradiction

```bash
auto-re contradiction list [--output json]
auto-re contradiction show --id <UUID> [--output json]
```

### verification

```bash
auto-re verification list [--output json]
auto-re verification show --id <UUID> [--output json]
```

### operation

```bash
auto-re operation list [--output json]
auto-re operation show --id <UUID> [--output json]
auto-re operation cancel --id <UUID> [--requested-by <WHO>] [--reason <TEXT>]
```

### events

```bash
auto-re events list [--after <SEQ>] [--limit <N>] [--output json]
```

### tui

```bash
auto-re tui
```

## TUI usage

The TUI is a 7-pane dashboard rendered with ratatui.

### Panes

| # | Pane | Contents |
|---|---|---|
| 1 | Dashboard | Project summary, operations, hypotheses |
| 2 | Providers | Registered providers and runs |
| 3 | NativeArtifacts | Provider-produced native artifacts |
| 4 | OpsDetail | Selected operation detail |
| 5 | EventsLog | Project event stream |
| 6 | MigrationHistory | Schema migration records |
| 7 | ExternalArtifactIntegrity | External artifact verification status |

### Keybindings

| Key | Action |
|---|---|
| `q` | Quit |
| `j` / `Down` | Select next item |
| `k` / `Up` | Select previous item |
| `Tab` | Cycle focus |
| `Alt+1` .. `Alt+7` | Switch active pane |
| `o` | Open selected project |
| `a` | Open artifact import dialog |
| `A` | Accept selected hypothesis |
| `c` | Cancel selected operation |
| `Enter` | Confirm dialog |
| `Esc` | Cancel dialog |

## Schema versioning

- Current version: `2.0`
- Stored in `project.toml`, `projects.schema_version`, and `Project.schema_version`
- V1 databases (M1 schema) must be migrated via `auto-re project migrate`, which runs `MigrationService`: copies the source DB, creates a timestamped `.bak` backup, applies refinery migrations V2..V13, drops obsolete V1 tables, records the migration, and validates the result
- V2 databases open idempotently; re-running migrations is a no-op

## Stage 0 vs Stage 1

**Stage 0** (this workspace's default build) delivers:

- Domain ontology: `Project`, `Artifact`, `SemanticEntity`, `Provider`, `ProviderRun`, `NativeArtifact`, `EvidenceRecord`, `Hypothesis`, `Contradiction`, `VerificationRecord`, `Operation`, `ProjectEvent`
- SQLite storage with refinery migrations and derived tables
- Application service with atomic command + event transactions
- CLI with all Stage 0 subcommands
- TUI with 7-pane dashboard and live event subscription
- V1 → V2 migration service

**Stage 1** (`autore-stage1`, excluded from `default-members`) adds:

- IDA integration and analysis backends
- Model providers and LLM routing
- Lease-based scheduler
- Worker runner
- RE engine / IDAGraph
- Headless CLI and campaign/task subcommands

Build stage 1 explicitly: `cargo build -p autore-stage1`.

## Further documentation

- Stage 0 implementation report: `docs/stage0-report.md`
- Stage 0 audit: `docs/stage0-audit.md`
- Notepad: `.omo/notepads/auto-re-stage-0/learnings.md`, `.omo/notepads/auto-re-stage-0/issues.md`
- Evidence: `.omo/evidence/task-39-auto-re-stage-0-gates.log` and sibling files
