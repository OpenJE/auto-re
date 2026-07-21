---
slug: auto-re-m1
status: approved
intent: clear
review_required: false
pending-action: execute .omo/plans/auto-re-m1.md
approval_received: 2026-07-16 via $start-work auto-re-m1
approach: feature-gated modular-monolith Milestone 1 campaign engine; full plan written and ready for execution
metis_review: NEEDS_FIX on draft; blockers B1–B3 and gaps G1–G7 folded into final plan
---

# Draft: auto-re-m1

## Components (topology ledger)
| id | outcome | status | evidence path |
| -- | ------- | ------ | ------------- |
| build-deps | Feature-aware build.rs + Cargo deps so core compiles without IDA | active | `Cargo.toml`, `build.rs` |
| error-domain | Correct Error enum, typed IDs, domain primitives | active | `src/error.rs`, `src/ids.rs`, `src/domain/` |
| adapters | AnalysisBackend + ModelProvider traits + deterministic mocks | active | `src/analysis/`, `src/model/` |
| storage | SQLite + refinery + TaskRepository + atomic leasing | active | `src/storage/`, `migrations/` |
| scheduler | Deterministic scheduler with campaign loop and model routing | active | `src/scheduler/` |
| worker | Worker runner with cancellation, timeout, schema validation | active | `src/worker/` |
| cli | clap status commands + tokio main | active | `src/cli/`, `src/main.rs` |
| proof | Kill→resume recovery test and campaign smoke test | active | `tests/` |

## Open assumptions (adopted defaults)
| assumption | adopted default | rationale | reversible? |
| ---------- | --------------- | --------- | ----------- |
| Plan scope | Full Milestone 1 in one plan | Spec is decision-complete; user invoked $start-work on it | Yes, but user confirmed via start |
| IDA feature gating | Default-off features `ida`, `gdb`, `llama` | Spec §39 says must work without them | Yes |
| Mock fixture | In-memory deterministic 10-function fixture | Spec §39 proof uses mock binary | Yes |
| SQLite stack | rusqlite(bundled) + refinery | Spec §5 recommends | Yes |
| Test strategy | TDD / tests-first | /shared/programming skill | Yes |
| Monolith | One Cargo package, no workspace | Spec §4 | Yes |
| Artifact storage | Metadata-only in SQLite; BLAKE3 deferred to M2 | Spec §16; not in §42 file list | Yes |
| Claim dependencies | Linear dependency recording only; no cycle detection | Keep M1 bounded | Yes |

## Findings (cited - path:lines)
- `build.rs:1-10` unconditionally calls `idalib_build::configure_linkage()` ⇒ blocks no-IDA build.
- `src/lib.rs:1-11` has broken `Error` enum from spec §3.
- `src/main.rs:1-7` uses `IDB::open` directly.
- `Cargo.toml:14-22` lacks all M1 deps except thiserror/idalib/gdbstub/llama_cpp.
- `.codegraph` index empty (cold repo).

## Decisions (with rationale)
1. Full Milestone 1 plan written (20 todos, 4 waves). User approval via `$start-work auto-re-m1`; plan updated to integrate remote TUI and switch to `idax`.
2. Feature-gate optional backends: `idax`/`gdbstub`/`llama_cpp` default-off; `build.rs` made feature-aware.
3. TUI is a first-class default surface; it works without IDA and observes campaign state from storage/scheduler.
4. `idax` replaces `idalib` for the IDA adapter (when `ida` feature is enabled).
5. In-memory deterministic mock analysis backend and model provider satisfy §39 proof without real binaries/models.
6. SQLite repository traits defined for all domains, but only `TaskRepository` gets a SQLite implementation in M1; others are in-memory stubs.
7. BLAKE3 artifact store and full claim DAG cycle detection deferred to later milestones.

## Scope IN
See `.omo/plans/auto-re-m1.md` Scope section.

## Scope OUT (Must NOT have)
- No real IDA/GDB/llama.cpp integration in default build (feature-gated only).
- No C++ generation, no behavioral validation, no network model provider.
- No workspace split, no distributed workers.
- No BLAKE3 artifact store, no claim cycle detection.

## Open questions
None remaining. Plan is approved and execution begins.

## Approval gate
status: approved
approach: Full Milestone 1 plan written to `.omo/plans/auto-re-m1.md`; execution authorized by `$start-work auto-re-m1`.
