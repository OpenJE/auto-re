---
slug: auto-re-stage-0
status: written
intent: clear
review_required: false
pending-action: present summary + ask start-vs-high-accuracy-review
approach: Full 7-crate workspace split + autore-stage1 deferral off default-members; UUIDv7 IDs; aggressive M1 domain replacement with kill→resume-port via durable Operation state + atomic state+event transactions + V1→V2 migration rollback. 40 todos across 11 waves (0A-0K).
---

# Draft: auto-re-stage-0

## Components (topology ledger)
- autore-schema | Core IDs/NamespacedId/ContentHash/SchemaVersion/ExtensionData/EvidenceValue/BinaryLocation/StableEntityKey/Derivation with committed serialization fixtures | active | .omo/plans/auto-re-stage-0.md Wave 0B
- autore-core | Stage 0 Error type (no IDAError), tracing logging with redaction + panic-hook, validation primitives (no_cycle etc.), domain record types/state machines | active | Wave 0B+0C+0D+0E+0F
- autore-store | SQLite Database with atomic transactions, no DB-generated IDs, focused storage traits + implementations + V2 schema migrations; managed/external artifact storage | active | Wave 0C+0D+0E+0F+0J
- autore-events | ProjectEventService: replay, in-process broadcast subscription, lag detection, gap recovery | active | Wave 0F
- autore-app | ApplicationService with typed ApplicationCommand/ApplicationQuery, atomic state+event commits, LocalAutoReClient, cross-project isolation, ValidationService, Derived-state rebuild | active | Wave 0G+0J
- autore-cli | Stage 0 verb set with --output json stable versioned output | active | Wave 0H
- autore-tui | Presentation-only TuiState with EventCursor, durable ProjectEvent subscription, 4-panel layout preserved + Stage 0 panels, generic fallback renderer, write actions via AutoReClient, terminal safety preserved | active | Wave 0I
- autore-stage1 | Deferred M1 analysis/model/scheduler/worker/engine + Campaign/Task/Claim/Evidence domain; off default-members | deferred | Wave 0A relocation
- M1 kill→resume guarantee | Ported as Stage 0 Operation restart durability + atomic state+event rollback + sequence-gap recovery + migration-rollback tests | active (replaced) | Wave 0F+0K

## Open assumptions (announced defaults)
| assumption | adopted default | rationale | reversible? |
| --- | --- | --- | --- |
| Hash algorithm | SHA-256 default, BLAKE3 retained for V1 backward compatibility | Spec §8 layout shows `artifacts/sha256/`; BLAKE3 already in deps for V1 artifact parity | high cost (fixtures + backward-compat) |
| Live event channel | tokio::sync::broadcast | Spec §17 "in-process channel sufficient"; broadcast gives lag detection | medium |
| Storage backend | SQLite via rusqlite(bundled) + refinery | Spec §19 permits existing deliberate choice; M1 already used it | low (refinery migrations forward-compat) |
| PTY harness | expectrl (portable-pty fallback) for §29.15 | Spec §29.15 mandates pseudo-terminal test; both crates are maintained | medium |
| Lint gates | cargo clippy --workspace --all-targets -- -D warnings; cargo fmt --all --check | Spec §32 #45/#46 explicit | low |
| Serialization | explicit #[serde(tag=..., content=...)] everywhere; fixtures committed per type | Spec §3.7/§26 explicit "Do not rely on accidental Rust enum serialization" | IRREVERSIBLE on committed fixtures |
| IDAError location | autore-stage1 only; never a core variant | Spec §27 explicit | low |

## Findings (cited - path:lines)
- Repo is the completed M1 milestone (all 20 todos checked in .omo/plans/auto-re-m1.md).
- Cargo.toml:1-51 — single package; default=["tui"]; optional ida/gdb/llama features; uuid v4 + boat of M1 deps.
- src/tui.rs:1-426 — 4-panel Ratatui dashboard with render_campaign_list/render_campaign_status/render_task_list/render_claim_summary, j/k/q keybindings, run_tui(Some(receiver)) consuming TuiUpdate, TestBackend render tests present.
- src/tui/state.rs:1-271 — DashboardState, TuiUpdate enum, ClaimSummary/TaskSummary with progress.
- src/runtime.rs:1-276 — scheduler_loop driving TuiUpdate channel; M1 mock快来 political process model.
- src/storage/database.rs:1-246 — Database wraps Mutex<rusqlite::Connection>, WAL+FK on, refinery embed_migrations!("migrations"); foreign keys enforced (test exists).
- migrations/V1__initial_schema.sql:1-108 — campaigns/binary_revisions/modules/functions/tasks/claims/evidences/leases/artifacts tables + 6 indexes.
- src/ids.rs:1-203 — define_id! macro over uuid::Uuid for 13 M1 IDs; UUIDv4.
- src/domain/mod.rs:97-120 — ContentHash(String) BLAKE3-only.
- tests/campaign_smoke.rs, tests/kill_resume.rs — M1 integration tests; replaced in Stage 0.

## Decisions (with rationale)
Recorded in .omo/drafts/auto-re-stage-0-pre-approval.md and above; user-answered Q1/Q2/Q3 + 7 adopted defaults.

## Scope IN
The 19 Must-have items in .omo/plans/auto-re-stage-0.md §Scope.

## Scope OUT (Must NOT have)
The 13 explicit exclusions in .omo/plans/auto-re-stage-0.md §Scope (Must NOT have guardrails); spec §31 explicit exclusions catalog.

## Open questions
None — all forks resolved. User can still veto adopted defaults in the "start or review" reply.

## Approval gate
status: approved (user wrote "approve")
Native Momus receipt: NOT run initially — CLEAR intent + review_required=false. User then opted in ("run high accuracy review first") and the dual review ran.

## High-accuracy dual review (the user opted in)
- **Round 1**: Dispatched Momus + independent Oracle in parallel against the complete plan. Momus → OKAY with 2 minor non-blocking residuals (phantom dep-matrix refs; "§46 gates" wording). Oracle → NEEDS-FIX with 7 issues (2 HIGH: §32 #1-46 not fully traced, `cargo test --workspace` includes autore-stage1; 2 MEDIUM: 0G/0H under-split, §29.2/§29.7 untraced; 3 LOW: §3.2, broadcast capacity, project.toml).
- **Fixes folded**: dependency matrix rewritten (only labels 1-40 + F); waves 0G and 0H merged to 0GH; §32 #1-46 explicit 46-row coverage matrix added under Success criteria; task 39 reworded with `--exclude autore-stage1` three-command structure (`cargo test --workspace --exclude autore-stage1`, `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings`, `cargo build -p autore-stage1 --no-default-features`); success criteria §32 #44/#46 rows aligned; tasks 12 (§29.2) and 20 (§29.7) References trace added; task 15 §3.2 note; task 23 `EVENT_BROADCAST_CAPACITY = 256` rationale; task 13 TOML plan-level-choice note; "§46 gates" reworded as "§32 criteria numbers #44/#45/#46".
- **Round 2**: Resubmitted both fresh. Momus → OKAY (confirmed all 7 fixes + matrix clean; flagged minor residual: per-todo `Parallelization: Blocks:` lines still had phantom numbers outside the matrix table). Oracle → NEEDS-FIX with 2 residuals: HIGH regression (Scope §18 still used bare `cargo test --workspace` without `--exclude autore-stage1`); MEDIUM per-todo Parallelization phantoms (todos 1, 22, 31, 32-38, 40 referenced 41/44/45/46/47/48).
- **Fixes folded #2**: Scope §18 (line 40) updated to `cargo clippy --workspace --exclude autore-stage1 --all-targets -- -D warnings` + `cargo test --workspace --exclude autore-stage1` + `cargo build -p autore-stage1 --no-default-features`; all 11 per-todo `Parallelization: Blocks:` lines cleaned to match the dependency matrix table (replaced phantom 41/44/45/46/47/48 with real todo numbers + "F" + human notes).
- **Round 3**: Resubmitted both fresh. **Momus → VERDICT: OKAY** ("Both the HIGH finding and the residual phantom-todo-number issue are resolved. All prior-round fixes remain intact. Plan is executable."). **Oracle → VERDICT: OKAY** ("Both previously-flagged issues are fully resolved. No regressions detected across all five verification dimensions...uniform `--exclude autore-stage1` discipline across every `--workspace`-scoped test/clippy invocation...per-todo Parallelization metadata is free of phantom references.").
- **Receipts**: Momus final session `ses_08f76a6a4ffeI6yxvQN5m37lGs`; Oracle final session `ses_08f7651d3ffewhmq6z2NOE7PPm`. Both unconditional approval — round 3.

## Metis gap review (mandatory)
Receipt: Metis ran via `task(subagent_type="metis", ...)` and produced a structured gap report covering: 3 contradictions (autore-stage1 non-default semantics; V1 fixture disposition; Operation-record-vs-execution scope); 5 missing constraints (atomic state+event same-tx; no DB-IDs; EventCursor gap recovery; projection-only TUI state; generic fallback renderer); 3 scope-creep risks (deferred-on-default-path, Operation execution, derived-state rebuild); 5 unvalidated assumptions (uuid v7, sha2, refinery V1 path, TestBackend, expectrl); 8 missing acceptance criteria (external-artifact mod detection, derived-index rebuild, generic fallback, terminal-restore-on-panic, FK enforcement, pagination+stable ordering, migration rollback, sequence-gap persistence); topology sanity with one 0F-intermediate-test hazard. All folded into todos (tasks 1, 4, 11, 12, 17, 18, 20, 21, 22, 27, 28, 29, 30, 31, 33, 34, 35, 37, 39 explicitly encode each folded finding).