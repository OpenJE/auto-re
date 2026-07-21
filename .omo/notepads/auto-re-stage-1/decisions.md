# Stage 1 Implementation Decisions

## 2026-07-21 Session Start
- Adopted dual-reviewed plan from Metis + Momus + Oracle review rounds.
- Round 2 residuals folded: handler-wiring ownership for 7 deferred commands, variant count correction.
- Van Buren real-binary campaign classified as operator-acceptance (§20), not automated worker gate.
- Boulder session: `opencode:ses_07b120cc7ffePfarZ4ruWIR9gQ`.

## Pending Decisions
- (none yet — will be recorded as work proceeds)

## Append only; never overwrite.

## 2026-07-21 Wave 1 Todo 1 (Audit) — Module Classification Decisions

### Classification Applied
Per plan §5.3 criteria and explicit designations in Todo 1:
- `analysis/backend.rs` → **REPLACE** (as explicitly stated in plan)
- `worker/runner.rs` → **REPLACE** (as explicitly stated in plan)
- `model/router.rs` → **ADAPT** (as explicitly stated in plan)
- `scheduler/scheduler.rs` → **ADAPT** (as explicitly stated in plan; priority scoring preserved)
- `engine.rs` → **REMOVE** (as explicitly stated in plan; replaced by external IDA provider Wave 3)
- `store.rs` → **REMOVE** (as explicitly stated in plan; empty placeholder)

### Owner Crate Assignments
- All model provider files → `autore-llm` (Wave 5)
- All scheduler files → `autore-coordinator` (Wave 4/11), except `scheduler/repos.rs` → `autore-app`
- CLI files → `autore-cli` (Wave 11)
- Repository/SQLite files → removed, replaced by `autore-app` commands/queries
- Engine/store files → removed (replaced by external provider)
- Analysis backend → replaced by `autore-provider-protocol`
- Analysis mock → replaced by `providers/fixture`
- Analysis packet → **deferred**: assigned to `autore-coordinator` (work-item design) with open question

### Deferred Decisions
1. `analysis/packet.rs` final crate destination — revisit after Wave 3-4 when work-item schema is clearer.
2. `error.rs` fate — keep as umbrella through migration; re-evaluate at Wave 11 cleanup.
3. `cli/mod.rs` interim strategy — old subcommands stay through Wave 10; replaced in Wave 11.

### Append only; never overwrite.
