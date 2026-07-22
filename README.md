# auto-re

`auto-re` is a reverse-engineering project manager. It tracks binary artifacts, semantic entities, evidence, hypotheses, contradictions, and verifications across a project's lifetime, with an append-only event log as the source of truth.

Stage 0 delivers the domain ontology, SQLite storage, application service, CLI, and TUI. Stage 1 adds the reconstruction pipeline: provider substrate (gRPC protocol, runtime, coordinator), pluggable analysis/model/build providers, work graph scheduling, managed C++ generation, and differential verification.

## Quick start

```bash
# Build the default workspace (Stage 0 crates)
cargo build

# Build the full workspace including Stage 1 crates
# Stage 1 crates require protoc for gRPC codegen:
PROTOC=/tmp/opencode/protoc/bin/protoc cargo build --workspace

# Create a new project in the current directory
cargo run -p autore-cli -- project create --name my-binary

# Inspect it
cargo run -p autore-cli -- project info

# Register a binary artifact (import the project's own built binary as an example)
cargo run -p autore-cli -- artifact add --file ./target/debug/auto-re --kind core.binary

# Register a semantic entity
cargo run -p autore-cli -- entity add --kind core.function --display-name main

# Run project-wide validation
cargo run -p autore-cli -- project validate

# Start a reconstruction campaign (Stage 1)
PROTOC=/tmp/opencode/protoc/bin/protoc cargo run -p autore-cli -- reconstruct start \
  --binary ./target/debug/my-binary \
  --output ./generated \
  --analysis-provider ida:latest \
  --model-provider gpt-4o \
  --build-profile release

# Launch the TUI (12-pane dashboard with Stage 1 panes)
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

The workspace is split into fifteen crates:

### Stage 0 crates (default members)

| Crate | Role |
|---|---|
| `autore-schema` | Domain records, typed IDs (UUIDv7), namespaced kinds, serde fixtures |
| `autore-core` | Low-level validation primitives, errors, logging, operation state machine |
| `autore-store` | SQLite storage, refinery migrations, derived tables, migration service |
| `autore-events` | Project event subscription and broadcast |
| `autore-app` | `ApplicationService`, commands, queries, validation service, lifecycle |
| `autore-cli` | Clap-based CLI binary (`auto-re`) |
| `autore-tui` | Ratatui TUI with 12-pane dashboard |
| `autore-stage1` | Legacy operational code (retained for reference) |

### Stage 1 crates (off `default-members`; require `PROTOC` for gRPC codegen)

| Crate | Role |
|---|---|
| `autore-provider-protocol` | Versioned gRPC schema (`autore.provider.v1`) and codegen |
| `autore-provider-runtime` | Provider bootstrap, lifecycle, auth, cancellation, limits |
| `autore-reconstruction` | Coordinator, IDA ingestion, work graph, LLM analysis, generation, build, verification |
| `fixture-provider` | Fixture provider binary for testing (5 capabilities) |
| `ida-provider` | External IDA provider over idax (16 capabilities: 9 static + 7 debug) |
| `openai-compatible-provider` | OpenAI-compatible LLM provider (13 capabilities: 7 analysis + 6 generation) |
| `build-provider` | cmkr/CMake/Docker-MSVC2002 build provider |

All mutations route through `ApplicationCommand`; all reads through `ApplicationQuery`. Each command and its resulting `ProjectEvent` commit atomically in a single SQLite transaction.

### Artifact kinds

`core.binary`, `core.source-tree`, `core.native-provider-output`, `core.configuration`, `core.log`, `core.trace`, `core.generated-candidate`.

### Entity kinds

`core.function`, `core.type`, `core.global`, `core.string`, `core.external-function`, `core.source-symbol`.

## Building and testing

```bash
# Build default members (Stage 0 crates)
cargo build

# Build everything including Stage 1 crates (requires protoc)
PROTOC=/tmp/opencode/protoc/bin/protoc cargo build --workspace

# Run default-members tests (~800+ tests)
cargo test --workspace --exclude autore-stage1

# Run the full workspace test gate including ignored tests (~800+ tests)
PROTOC=/tmp/opencode/protoc/bin/protoc cargo test --workspace -- --include-ignored

# Run the PTY integration test separately
cargo test -p autore-tui --test pty_integration -- --ignored --nocapture

# Format and lint
cargo fmt --all --check
PROTOC=/tmp/opencode/protoc/bin/protoc cargo clippy --workspace --all-targets -- -D warnings
```

Stage 1 crates are excluded from `default-members` because they depend on external SDKs and require `protoc` for gRPC codegen. Set `PROTOC=/tmp/opencode/protoc/bin/protoc` (or your system protoc path) when building or testing the full workspace.

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

### reconstruct (Stage 1)

```bash
auto-re reconstruct start --binary <PATH> --output <DIR> --analysis-provider <SPEC> --model-provider <SPEC> --build-profile <PROFILE>
auto-re reconstruct status [--output json]
auto-re reconstruct pause
auto-re reconstruct resume
auto-re reconstruct stop
auto-re reconstruct validate [--output json]
```

### provider (Stage 1)

```bash
auto-re provider refresh
auto-re provider list [--output json]
auto-re provider show --id <ID> [--output json]
auto-re provider start --installation-id <ID>
auto-re provider stop --id <ID>
auto-re provider restart --id <ID>
auto-re provider health --id <ID> [--output json]
```

### work (Stage 1)

```bash
auto-re work list [--output json]
auto-re work show --id <ID> [--output json]
auto-re work blockers --id <ID> [--output json]
auto-re work retry --id <ID>
auto-re work dependencies --id <ID> [--output json]
```

### generated (Stage 1)

```bash
auto-re generated status [--output json]
auto-re generated files [--output json]
auto-re generated entity --id <ID> [--output json]
```

### build (Stage 1)

```bash
auto-re build latest [--output json]
```

### verification coverage (Stage 1)

```bash
auto-re verification coverage [--output json]
```

## TUI usage

The TUI is a 12-pane dashboard rendered with ratatui.

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
| 8 | Campaign | Reconstruction campaign state, coverage, work items |
| 9 | WorkQueue | Current work items, blocked items, stagnation reasons |
| 10 | ActiveProviders | Running provider instances and health |
| 11 | CompilerFailures | Recent build diagnostics and failures |
| 12 | VerificationDiffs | Verification comparisons and generated source mappings |

### Keybindings

| Key | Action |
|---|---|
| `q` | Quit |
| `j` / `Down` | Select next item |
| `k` / `Up` | Select previous item |
| `Tab` | Cycle focus |
| `Alt+1` .. `Alt+7` | Switch to pane 1-7 |
| `Alt+8` | Switch to Campaign pane |
| `Alt+9` | Switch to WorkQueue pane |
| `Alt+0` | Switch to ActiveProviders pane |
| `Alt+-` | Switch to CompilerFailures pane |
| `Alt+=` | Switch to VerificationDiffs pane |
| `o` | Open selected project / start campaign dialog |
| `a` | Open artifact import dialog |
| `A` | Accept selected hypothesis |
| `c` / `n` | Cancel selected operation |
| `p` | Pause reconstruction coordinator |
| `r` | Resume reconstruction coordinator |
| `X` | Stop reconstruction coordinator |
| `R` | Requeue selected work item |
| `P` | Open provider start/stop dialog |
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
- TUI with 12-pane dashboard and live event subscription
- V1 → V2 migration service

**Stage 1** (implemented across 7 new crates, off `default-members`) adds:

- Provider substrate: gRPC protocol (`autore-provider-protocol`), runtime with bootstrap/auth/lifecycle (`autore-provider-runtime`)
- Pluggable providers: fixture (testing), IDA (16 capabilities over idax), OpenAI-compatible LLM (13 capabilities), cmkr/CMake/Docker-MSVC2002 build
- Reconstruction coordinator (`autore-reconstruction`): IDA ingestion, work graph with SCC scheduling, LLM analysis, managed C++ generation, build orchestration, differential verification
- CLI subcommands: `reconstruct`, `provider`, `work`, `generated`, `build`, `verification coverage`
- TUI panes: Campaign, WorkQueue, ActiveProviders, CompilerFailures, VerificationDiffs

Stage 1 implementation details: `docs/stage1-report.md`, `docs/stage1-architectural-test.md`, `docs/stage1-completion-gate.md`.

## Further documentation

- Stage 0 implementation report: `docs/stage0-report.md`
- Stage 0 audit: `docs/stage0-audit.md`
- Stage 1 implementation report (§22): `docs/stage1-report.md`
- Stage 1 architectural test / pluggability proof (§23): `docs/stage1-architectural-test.md`
- Stage 1 completion gate cross-check (§19): `docs/stage1-completion-gate.md`
- Stage 1 retain/adapt/defer/remove audit: `docs/stage1-audit.md`
- Notepad: `.omo/notepads/auto-re-stage-0/learnings.md`, `.omo/notepads/auto-re-stage-0/issues.md`
- Stage 1 notepad: `.omo/notepads/auto-re-stage-1/learnings.md`, `.omo/notepads/auto-re-stage-1/issues.md`
- Evidence: `.omo/evidence/task-39-auto-re-stage-0-gates.log` and sibling files
