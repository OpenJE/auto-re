# auto-re Milestone 1 — Plan Draft (resume point)

slug: auto-re-m1
status: awaiting-approval
intent: clear
review_required: false
pending_action: write .omo/plans/auto-re-m1.md
request_created: 2026-07-16

## Grounding (repo facts)
- 5 commits, clean tree, on `main`, ahead of origin/main by 1.
- `Cargo.toml`: deps thiserror 2.0.18, idalib 0.7.2, gdbstub 0.7.8, llama_cpp 0.3.2; build-dep idalib-build 0.7. edition 2024. lib `auto_re`, bin `auto-re`.
- `build.rs`: UNCONDITIONALLY calls `idalib_build::idalib_install_paths_with(false)` + `configure_linkage()` ⇒ `cargo build` currently REQUIRES an IDA install. BLOCKS Milestone 1 ("must work without IDA").
- `src/lib.rs` (11 lines): broken `Error { IdaError( #[error] IDAError ) }` — spec §3 immediate cleanup target.
- `src/main.rs` (7 lines): directly uses `IDB::open("/path/to/binary")` — must move behind IDA adapter.
- devenv + flake configured; `.codegraph` empty (cold).
- Spec is exhaustive & self-decision-complete. §42 "First Files to Implement" + §39 Milestone 1 pin the immediate target: "Core Rust Campaign Engine", "Must work without IDA, GDB, or llama.cpp." Proof = mock binary of 10 functions inventoried + analyzed by deterministic mock workers; kill→resume without duplicate accepted claim completion.

## Scope (this plan)
Full Milestone 1 (§39) as ONE plan, multiple waves:
- Repo cleanup (§3): fix `Error`, move direct `IDB` use behind adapter boundary.
- Feature-gate idalib/gdbstub/llama_cpp behind default-OFF features (ida/gdb/llama) so `cargo build` works without those installs. Core lib NEVER depends on them directly (§6).
- Trail the §42 file list: error.rs, ids.rs; domain/{task,campaign,function,claim,evidence}.rs; storage/database.rs + repositories/task.rs; analysis/backend.rs + analysis/mock.rs; model/provider.rs + model/mock.rs; scheduler/{scheduler,lease}.rs; worker/{runner,output}.rs; cli/{mod,campaign,task}.rs + main wired to `#[tokio::main]`.
- §43 required tests: IDs serialize, task-state transitions, claim-state transitions, invalid-confidence, priority calc, capability match, schema validation, transaction preconditions; DB atomic lease, concurrent lease contention, expired-lease recovery, completion idempotency, migration, txn rollback, artifact-ref integrity; scheduler routing/retry/escalation/dep-block/stale-invalidate/completion/blocked; worker valid/malformed/timeout/cancel/crash/repair/verify-independence; end-to-end kill→resume.
- Mock analysis backend emits an in-memory 10-function fixture binary (NO real IDB, NO real binary). Mock model provider returns deterministic schema-bound responses.

## Approach (decided)
- Modular-monolith Rust crate (§4), async via Tokio (§36/§37), SQLite via rusqlite(bundled)+refinery (§5/§15), defined repositories (§15), typed IDs via macro (§8), capability-graded `AnalysisBackend`/`ModelProvider` traits with mocks (§10/§13), deterministic scheduler owning all state (§19), atomic SQLite leasing (§18), worker packets→schema-validated output→claim/evidence (§21/§22/§24), dependency-backed claims (§25), autonomous loop + cancellation + recovery (§37), clap CLI (§35).
- Domain layer never depends on idalib/gdbstub/llama_cpp/HTTP/SQLite/fs; adapters depend on domain (§6).

## Adopted defaults (spec-resolved; announced, not asked)
1. Feature-gate idalib/gdbstub/llama_cpp default-OFF. Core builds/runs Milestone 1 without those installs.
2. In-memory deterministic mock analysis backend (10-function fixture) + deterministic mock model provider — NO real IDB, NO real binary, NO real model. Matches §39 proof exactly.
3. TDD per /shared/programming: tests-first for each module; agent-executed QA (happy + failure) in every todo.
4. Keep Cargo as ONE package (monolith); do NOT split to workspace yet (§4).
5. SQL migrations via refinery under `migrations/`; artifact storage under `.auto-re/` with BLAKE3 content hashing (§16).

## Surviving fork (to ask)
F1. Plan scope: (a) full Milestone 1 in this one plan [RECOMMENDED — spec is decision-complete], vs (b) a thinner foundation-first plan (error/ids/domain-core/storage only), then iterate.

## Review
Not requested by user. CLEAR + review_required=false ⇒ after approval + plan write: present summary, ask ONE q (start work now vs run dual high-accuracy review first).