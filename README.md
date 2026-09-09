# Hydra CLI

Hydra is a terminal-first AI coding assistant built in Rust. Its goal is to
give developers one reliable command-line workspace for understanding a
repository, planning and executing coding work, choosing among AI providers
and models, and recovering cleanly when a provider or task fails.

## Ultimate Goal

**Hydra aims to be a complete, extensible terminal coding partner that turns a
developer's intent into context-aware, provider-resilient repository changes.**

The terminal is the primary product surface. Provider profiles, model
selection, credential isolation, key fallbacks, repository context, tools,
task workflows, sandboxed execution, progress reporting, and session history
are parts of that single assistant experience, not separate utilities.

## Product Direction

Hydra is built around six core capabilities:

- **Autonomous Parallel Swarms:** Partition workspaces into disjoint AST clusters and execute Coder, Tester, and Reviewer worker trios simultaneously across isolated Git worktrees.
- **Provider Intelligence:** Profiles, models, credentials, capability-aware routing, retry state, health tracking, and fallback across configured keys or providers.
- **Unified Adapter Facades:** Single execution gateways for LLMs, process management for LiteLLM, Model Context Protocol (MCP) tool dispatch, and agent runtime isolation.
- **Repository Intelligence:** Deterministic AST indexing, dependency clustering, symbol search, and context assembly for coding prompts.
- **Consensus & Patch Consolidation:** Actionable log deduplication (compiler diagnostics and test assertions) and unified diff patch reconciliation.
- **Safe Local Execution:** Isolated JavaScript tooling, resource policies, structured errors, and clear boundary enforcement.

## Use A Release Build

From a release checkout:

```bash
git submodule update --init --recursive
target/release/hydra-cli --help
```

Windows:

```powershell
.\target\release\hydra-cli.exe --help
```

Local and offline commands work without provider setup:

```text
hydra-cli js "2 + 2"
hydra-cli execute --config hydra.json --task "check the repository"
hydra-cli execute --config hydra.json --task "lint" --task "test" --concurrency 2
hydra-cli execute --config hydra.json --tasks workflow.json
hydra-cli index --root . --output code-index.json --languages rs,js,ts
hydra-cli tools
```

## Autonomous Parallel Swarm

Hydra includes a multi-threaded parallel swarm orchestrator that partitions repository AST dependencies into isolated scopes, running Coder, Tester, and Reviewer loops in ephemeral Git worktrees:

```bash
hydra-cli swarm "Refactor database connection pool and update integration tests" --concurrency 4
```

Options:
- `--root <DIR>`: Target repository root (default: `.`)
- `--concurrency <N>`: Maximum parallel worker trios (default: `4`)
- `--coder-model <MODEL>`: Model alias for implementation workers (default: `gemini-2.5-pro`)
- `--reviewer-model <MODEL>`: Model alias for auditing workers (default: `claude-3-5-sonnet`)

## Configure Prompts

Create a configuration file:

```bash
hydra-cli init --config hydra.json
```

Add a profile, candidate, and environment-backed credential:

```json
{
  "profiles": {
    "primary": {
      "credentials": {"openai": "$ENV:OPENAI_API_KEY"},
      "models": ["coding"]
    }
  },
  "candidates": [
    {"provider": "openai", "model": "coding", "profile": "primary"}
  ]
}
```

Then validate and prompt:

```bash
export OPENAI_API_KEY=...
hydra-cli credentials --config hydra.json
hydra-cli providers --config hydra.json
hydra-cli prompt --model coding "Explain this repository"
```

Use `hydra-cli <command> --help` for options. `prompt` routes by purpose, tools, capabilities, provider, model, and profile, then retries transient failures and fails over in preference order. Credentials are resolved by Hydra and passed securely to session creation; they are never logged or displayed.

Other inspection and maintenance commands include:

```text
hydra-cli route --config hydra.json --model coding
hydra-cli retry-status --config hydra.json
hydra-cli reset-retry --config hydra.json --provider openai --model coding --profile primary
```

## Build From Source

Requirements: Rust toolchain (2021 edition) and Git submodules.

```bash
git submodule update --init --recursive
cargo build --workspace --release
cargo test --workspace
```

The optimized binary is `target/release/hydra-cli` (or `target/release/hydra-cli.exe` on Windows).

## Architecture

```text
hydra-cli
├── adapters/
│   ├── pi_agent.rs   <-> submodules/pi_agent_rust (pinned upstream runtime)
│   ├── litellm.rs    <-> submodules/litellm (daemon process manager)
│   └── mcp.rs        <-> submodules/mcp-sdk (protocol connector & tools)
├── partitioner/
│   └── ast_splitter.rs (AST dependency clustering via hydra-matrix)
├── orchestrator/
│   └── swarm.rs (parallel Coder/Tester/Reviewer trios in Git worktrees)
├── consolidator/
│   └── merger.rs (log deduplication & diff patch reconciliation)
└── crates/
    ├── hydra-dag     (topological DAG task scheduling)
    ├── hydra-matrix  (AST parsing and codebase indexing)
    └── hydra-sandbox (isolated JavaScript execution environment)
```

Hydra owns routing, orchestration, adapters, and consolidation under `crates/`. External submodules (`pi_agent_rust`, `litellm`, `mcp-sdk`) are pinned stable dependencies under `submodules/` and remain untouched.

## Release Notes

Hydra is released as a workspace binary. All internal crates (`hydra-dag`, `hydra-matrix`, `hydra-sandbox`, `hydra-cli`) are integrated and covered by full test suites.

Hydra source is MIT licensed. External submodules retain their respective upstream licenses.
