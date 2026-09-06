# Hydra CLI

Hydra is a native Rust orchestration layer around an agent runtime. It adds
provider profiles and routing policy, dependency-aware task graphs, repository
indexing, isolated JavaScript execution, progress reporting, and adapter-based
integration on top of Pi agent capabilities.

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

Local commands work without provider setup:

```text
hydra-cli js "2 + 2"
hydra-cli execute --config hydra.json --task "check the repository"
hydra-cli index --root . --output code-index.json --languages rs,js,ts
hydra-cli tools
```

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

Use `hydra-cli <command> --help` for options. `prompt` requires a configured
candidate and credential; it routes by purpose, tools, capabilities, provider,
model, and profile, then retries transient failures and fails over in preference
order. `js`, `index`, `execute`, and routing inspection do not require an API
key. Credentials are resolved by Hydra and passed only to Pi session creation;
they are never printed.

## Build From Source

Requirements: Rust and the pinned `core/pi_agent` submodule.

```bash
git submodule update --init --recursive
cargo build --workspace --release
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The optimized binary is `target/release/hydra-cli` or
`target/release/hydra-cli.exe` on Windows.

## Architecture

```text
CLI -> Hydra adapters -> Pi agent or Hydra crates
                         dag      scheduling
                         matrix   indexing
                         sandbox  JavaScript isolation
```

Hydra owns routing, policy, orchestration, and adapters under `crates/`.
`core/pi_agent` is a pinned external dependency outside the Cargo workspace;
application code uses [agent_adapter.rs](crates/hydra-cli/src/agent_adapter.rs)
instead of Pi SDK types directly. See [dependency-plan.md](dependency-plan.md).

## Release Notes

Hydra is currently released as a workspace binary. Crates.io packaging requires
publishing `hydra-dag`, `hydra-matrix`, and `hydra-sandbox` first, or replacing
their path dependencies with pinned Git or registry versions.

Hydra source is MIT licensed. The Pi submodule retains its own terms in
`core/pi_agent/LICENSE`.

## Future Enhancements

- Multi-agent DAG workflows with dependency-aware parallel execution.
- Persistent indexes with richer symbol, dependency, and semantic search.
- Custom Hydra tools, extension loading, approvals, and sandbox policies.
- Persistent retry history and richer provider health scoring across CLI runs.
- Stronger JavaScript limits for modules and TypeScript support.
- Session-aware prompts, resumable workflows, and structured run history.
- Optional TUI workflows and packaged binaries/installers.
- Published Hydra crates and pinned registry or Git dependency releases.
