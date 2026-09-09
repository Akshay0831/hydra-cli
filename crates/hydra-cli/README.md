# hydra-cli

CLI interface and adapter layer for Hydra.

| Subsystem | Module | Description |
|---|---|---|
| **Swarm Orchestrator** | orchestrator::swarm | Coordinates parallel workers (Coder/Tester/Reviewer) in isolated Git worktrees |
| **AST Partitioner** | partitioner::ast_splitter | Splits targets into balanced AST clusters via hydra-matrix |
| **Adapters** | adapters/ | Facades for upstream runtime, LiteLLM, and MCP tools |
| **Anti-Duplication Gate** | ScopeConstraint | Blocks redundant wrapper files (utils.rs, helpers.rs, etc.) |
| **Consolidator & Anti-Bloat** | consolidator::merger | Comment density filter, log deduplication, and patch gates |
| **Prompt Router & Cache** | prompt_router | Provider routing with goal registry and cache alignment |
| **Headless IPC Daemon** | commands::daemon | TCP server for external GUI/frontend interfaces |
| **Doc Governance** | commands::doc | Documentation tree management and cross-link validation |

