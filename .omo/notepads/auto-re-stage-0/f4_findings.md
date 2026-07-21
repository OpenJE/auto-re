# F4 Scope Fidelity Findings

**Date:** 2026-07-18
**Verifier:** Oracle (strategic-technical-advisor)

## Check Results

### 1. Provider-specific code (IDAError / idax:: / gdbstub / llama_cpp)
- **Result:** ✅ PASS
- **Detail:** `grep -rn 'IDAError\|idax::\|gdbstub\|llama_cpp'` across all 7 default crates returned zero matches.

### 2. autore-stage1 excluded from default-members
- **Result:** ✅ PASS
- **Detail:** `Cargo.toml` lines 12–20 list exactly 7 crates in `default-members`. `autore-stage1` is in `members` (line 10) but NOT in `default-members`.

### 3. TUI does not touch rusqlite / Database
- **Result:** ❌ FAIL
- **Detail:** `autore-tui/src/runtime.rs` directly imports and uses `Database`:
  - Line 12: `use autore_store::Database;`
  - Line 36: `let db = Arc::new(Database::open(&database_path)?);`
- **Impact:** The TUI runtime's `build_client()` function constructs a `Database` directly instead of receiving an already-constructed client or service abstraction. This couples the TUI to the store layer.
- **Note:** `rusqlite` itself is not directly imported in TUI code — the violation is via the `Database` type from `autore_store`.

### 4. No disassembly/decompilation/CFG/call-graph/SCC/symbolic execution/sandbox execution implementation code
- **Result:** ✅ PASS (with note)
- **Detail:** No implementation functions found (`fn disassemble`, `fn decompile`, `fn build_cfg`, etc. — all zero matches). Matches found are exclusively:
  - **Schema domain vocabulary** in `autore-schema`: enum variants (`ControlFlowGraph`), provider kind constants (`PROVIDER_KIND_DISASSEMBLER`, `PROVIDER_KIND_DECOMPILER`, `PROVIDER_KIND_SYMBOLIC_EXECUTOR`), capability flags (`decompilation: bool`, `disassembly: bool`), and doc comments referencing RE tools.
  - **Test fixtures** in `autore-app` and `autore-store`: string-based `NamespacedId` values like `"provider.disassembler"` and `"core.disassemble"` used in test data.
  - These are domain *metadata* describing what kinds of providers and capabilities exist — not executable RE logic. This is expected and correct for a schema crate.

### 5. No network transport code (TCP/HTTP/WebSocket/gRPC)
- **Result:** ✅ PASS
- **Detail:** `grep -rn 'TcpListener\|TcpStream\|HttpServer\|hyper::\|reqwest\|websocket\|WebSocket\|tonic::\|grpc\|axum::\|actix\|warp::\|UdpSocket\|UnixListener\|UnixStream'` returned zero matches across all 7 crates.

### 6. No LLM/model provider implementation code
- **Result:** ✅ PASS
- **Detail:** No LLM inference implementation found. Matches are:
  - **Schema constants:** `PROVIDER_KIND_LLM` and `llm.raw-response` in `autore-schema/src/domain/records.rs` — domain vocabulary only.
  - **Capability flag:** `model_inference: bool` in `autore-schema/src/domain/task/types.rs` — metadata, not implementation.
  - **False positives:** "completion" in task lifecycle context ("cancelled before completion", "query completion") — unrelated to LLM.

---

## VERDICT: REJECT

### Blocking Issue
1. **TUI directly uses `Database`** (`autore-tui/src/runtime.rs:12,36`): The `build_client()` function imports `autore_store::Database` and calls `Database::open()`. Per the stage-0 constraint, the TUI must not touch `Database`. The fix is to have `build_client()` receive a pre-constructed `ApplicationService` or client, or to move `build_client()` into `autore-app` and have the TUI call a factory function that returns a `Box<dyn AutoReClient>`.

### Non-blocking Observations (informational only)
2. Schema crate legitimately defines RE domain vocabulary (provider kinds, capability flags, evidence types). This is correct architectural layering — the schema describes *what* exists without implementing *how*.
3. Test fixtures in `autore-app` and `autore-store` reference domain string IDs. These are data, not logic.
