# Hydra CLI

Hydra is an autonomous, terminal-first AI coding orchestrator built in Rust. It transforms developer intent into verified repository changes through AST partitioning, isolated parallel Git worktrees, multi-provider routing, and consensus patch reconciliation.

---

## Core Capabilities

| Capability | Description |
|---|---|
| **Autonomous Swarms** | Decomposes codebases into disjoint AST clusters; runs parallel Coder, Tester, and Reviewer trios in isolated Git worktrees. |
| **Unified Adapters** | Single execution gateways for LLMs, LiteLLM daemon supervision, Model Context Protocol (MCP) tools, and agent runtimes. |
| **Provider Routing** | Health tracking, preference order, exponential backoff with jitter, and automatic failovers across keys/models. |
| **Repository Intelligence** | Deterministic AST indexing, dependency graph analysis, and symbol search via `hydra-matrix`. |
| **Consensus Engine** | Actionable compiler/test log deduplication and non-conflicting multi-partition diff patch consolidation. |
| **Safe Sandboxing** | Resource-limited JavaScript isolation and structured task execution graphs via `hydra-sandbox` and `hydra-dag`. |

---

## Quick Install

Clone and install with one command:

### Linux / macOS
```bash
git clone --recurse-submodules https://github.com/Akshay0831/hydra-cli.git
cd hydra-cli && ./install.sh
```

### Windows (PowerShell)
```powershell
git clone --recurse-submodules https://github.com/Akshay0831/hydra-cli.git
cd hydra-cli; .\install.ps1
```

Or run without installation from release checkout:
```bash
git submodule update --init --recursive
cargo build --workspace --release
./target/release/hydra-cli --help
```

---

## Command Reference

### Autonomous Parallel Swarm
Partition AST dependencies and execute parallel worker trios in ephemeral Git worktrees:
```bash
# Execute swarm and review proposed patch
hydra-cli swarm "Refactor database pool and update tests" --concurrency 4

# Execute swarm, review, and automatically apply patch with pre-patch safety checkpoint
hydra-cli swarm "Fix race condition in session picker" --apply --output patch.diff
```
* `--root <DIR>`: Repository root (default: `.`)
* `--concurrency <N>`: Worker trio concurrency limit (default: `4`)
* `--coder-model <MODEL>`: Implementation model (default: `gemini-2.5-pro`)
* `--reviewer-model <MODEL>`: Audit/review model (default: `claude-3-5-sonnet`)
* `--apply`: Creates a pre-patch checkpoint and applies the unified diff to the workspace
* `--output <FILE>`: Save the unified patch to disk

### Git Checkpoints & Rollback
Safety mechanisms for instant workspace reversion:
```bash
hydra-cli checkpoint --label "before-big-refactor"   # Create labelled snapshot
hydra-cli checkpoint --list                          # View all Hydra checkpoints
hydra-cli undo                                       # Revert to the latest snapshot
hydra-cli undo --list                                # Preview available rollback points
```

### Headless JSON-RPC 2.0 Daemon
Connect external frontends, VS Code extensions, JetBrains plugins, and web dashboards:
```bash
# Stdio transport (LSP-style for IDE extensions):
hydra-cli daemon --stdio

# TCP transport (for local web apps and dashboards):
hydra-cli daemon --bind 127.0.0.1:4545
```

### Programmatic Rust Library Usage (`hydra-core`)
Rust web apps (Axum, Leptos, Tauri) can import `hydra-cli` directly as a library:
```rust
use hydra_cli::{SwarmOrchestrator, SwarmConfig, DaemonState};

let orchestrator = SwarmOrchestrator::new(SwarmConfig::default(), None);
let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(128);
let handle = tokio::spawn(async move {
    orchestrator.run_swarm(repo_path, intent, partitions, event_tx).await
});
```

### Prompt & Routing
```bash
# Initialize config & test credentials
hydra-cli init --config hydra.json
hydra-cli credentials --config hydra.json
hydra-cli providers --config hydra.json

# Execute prompt with automatic fallback & retry
hydra-cli prompt --model coding "Explain this module"
hydra-cli route --config hydra.json --model coding
hydra-cli retry-status --config hydra.json
hydra-cli reset-retry --config hydra.json --provider openai --model coding --profile primary
```

### Machine-Readable Streaming & Local Tools
```bash
# Global --json flag emits NDJSON events to stdout for piping:
hydra-cli --json swarm "Implement auth middleware"

# Offline tools:
hydra-cli js "2 + 2"                                                 # Sandboxed JS
hydra-cli execute --config hydra.json --task "lint" --task "test"    # DAG execution
hydra-cli index --root . --output code-index.json --languages rs,ts  # AST Indexer
hydra-cli tools                                                      # List built-in tools
```

---

## Architecture & Hierarchical Navigation Hub

Hydra is organized as a modular, traceable workspace. Each subsystem maintains its own machine-dense documentation linked from this root hub:

| Component | Documentation | Role |
|---|---|---|
| **CLI & Swarm** | [crates/hydra-cli/README.md](crates/hydra-cli/README.md) | CLI commands, multi-agent swarm orchestrator, and facade adapters |
| **AST Matrix** | [crates/hydra-matrix/README.md](crates/hydra-matrix/README.md) | AST indexing, context loader, and skeletonization engine |
| **Task DAG** | [crates/hydra-dag/README.md](crates/hydra-dag/README.md) | Concurrent DAG execution and dependency graph engine |
| **Sandbox** | [crates/hydra-sandbox/README.md](crates/hydra-sandbox/README.md) | QuickJS isolated evaluation sandbox |
| **Submodules** | [submodules/README.md](submodules/README.md) | Pinned upstream submodules registry & adapter seams |

```text
hydra-cli
├── README.md                   ← Navigation Hub
├── docs/profiles/ai-dense.toml ← Default Machine-Dense Doc Specification
├── crates/
│   ├── hydra-cli/README.md     ← Swarm, adapters, and CLI commands
│   ├── hydra-matrix/README.md  ← ContextLoader, AST skeletonizer, symbol index
│   ├── hydra-dag/README.md     ← Topological DAG scheduler
│   └── hydra-sandbox/README.md ← Sandboxed JS engine
└── submodules/README.md        ← pi_agent_rust, litellm, mcp-sdk
```

### Dependency & Architectural Invariants
* **Adapter Seam**: Application code interacts exclusively with Hydra adapter facades (`crates/hydra-cli/src/adapters/`). Direct upstream SDK imports are forbidden.
* **Submodules Intact**: External submodules (`pi_agent_rust`, `litellm`, `mcp-sdk`) remain 100% clean and untouched; custom logic and role steering reside in Hydra adapters.
* **Doc Integrity Enforcement**: Run `hydra-cli doc check` to validate internal cross-links and ensure documentation invariants are maintained.
### Security Boundary
Hydra validates paths, rejects traversal and unsafe symlink writes, isolates swarm workers in Git worktrees, fails closed when isolation fails, restricts the daemon to loopback, and bounds subprocess time and output. Upstream modules are consumed through Hydra adapters and are not modified. The JavaScript sandbox is an in-process runtime; use an OS-level container or separate worker process when hostile code requires a hard isolation boundary.

---

## License

Hydra source code is licensed under the [MIT License](LICENSE). Pinned submodules retain their respective upstream licenses.
