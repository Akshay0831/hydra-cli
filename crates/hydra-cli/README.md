# hydra-cli

CLI interface, headless JSON-RPC daemon, and public library crate for Hydra.

| Subsystem | Module | Description |
|---|---|---|
| **Swarm Orchestrator** | `orchestrator::swarm` | Coordinates parallel workers (Coder/Tester/Reviewer) with multi-turn self-healing in isolated Git worktrees |
| **AST Partitioner** | `partitioner::ast_splitter` | Splits targets into balanced AST clusters via `hydra-matrix` with recursive traversal |
| **Checkpoints & Undo** | `checkpoints` | Atomic pre-patch Git-stash snapshots for instant `hydra undo` rollback |
| **Headless IPC Daemon** | `daemon` | Dual-transport JSON-RPC 2.0 streaming daemon (stdio pipe & TCP socket) |
| **Adapters** | `adapters/` | Facades for upstream runtime (`pi_agent_rust`), LiteLLM, and MCP tools |
| **Anti-Duplication Gate** | `ScopeConstraint` | Blocks redundant wrapper files (`utils.rs`, `helpers.rs`, etc.) |
| **Consolidator & Anti-Bloat** | `consolidator::merger` | Comment density filter, compiler log deduplication, and patch gates |
| **Prompt Router & Cache** | `prompt_router` | Provider routing with goal registry, AST skeleton injection, and cache alignment |
| **Progress & Telemetry** | `progress` | Human-readable terminal spinners and machine-readable NDJSON streaming (`--json`) |
| **Doc Governance** | `commands::doc` | Documentation tree management and cross-link validation |

## Crate Topology
- **Binary Target**: `hydra-cli` (entry point: `src/main.rs`)
- **Library Target**: `hydra_cli` (entry point: `src/lib.rs`) - re-exports all core orchestrators, routers, and daemon state for external Rust applications.

