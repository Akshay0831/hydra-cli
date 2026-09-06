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

Hydra is being built around five connected capabilities:

- **Provider intelligence:** profiles, models, credentials, capability-aware
  routing, retry state, health tracking, and fallback across configured keys or
  providers.
- **Repository intelligence:** deterministic indexing, persisted context,
  symbol and dependency search, and context selection for coding prompts.
- **Coding workflows:** prompts, multi-task DAG execution, progress feedback,
  resumable work, and clear command-level results.
- **Safe local execution:** isolated JavaScript tooling, resource policies,
  structured errors, and a boundary between Hydra policy and upstream runtime
  implementation.
- **Terminal-native extensibility:** built-in tools today, with a path toward
  custom tools, approvals, extensions, interactive workflows, and automation.

The current release foundation already includes provider routing and retry
persistence, task graphs, repository indexing and snapshots, sandboxed
JavaScript execution, progress handling, and an adapter boundary around the
upstream agent runtime. The remaining roadmap turns these foundations into a
cohesive coding loop with richer context, sessions, tools, and interactive
terminal workflows.

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
hydra-cli execute --config hydra.json --task "lint" --task "test" --concurrency 2
hydra-cli execute --config hydra.json --tasks workflow.json
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

`execute` also accepts `--tasks <path>` with a JSON document shaped like:

```json
{"tasks":[{"id":"lint","description":"run lint"},{"id":"test","dependencies":["lint"],"description":"run tests"}]}
```

Retry state is stored beside the selected config as `<config>.retry.json`.
`index` writes a complete serialized snapshot containing indexed elements,
configuration, and statistics. Sandbox memory usage is reported as unavailable
when the runtime cannot provide a measurement; TypeScript source is not
transpiled by the sandbox.

Other inspection and maintenance commands include:

```text
hydra-cli route --config hydra.json --model coding
hydra-cli retry-status --config hydra.json
hydra-cli reset-retry --config hydra.json --provider openai --model coding --profile primary
```

Routing supports `strict`, `provider`, `any`, and `none` fallback modes in the
configuration. Candidate capabilities and explicit tool requirements are
checked before a provider is attempted.

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

## Roadmap To The Complete Assistant

The following work completes the product vision rather than introducing a
different direction:

- Richer AST-backed repository understanding, semantic search, and automatic
  context assembly for prompts.
- Multi-provider and multi-key fallback policies with clearer health scoring,
  cost or latency preferences, and provider capability discovery.
- A complete coding loop for inspect, plan, edit, test, review, and retry,
  including session-aware prompts, resumable workflows, and run history.
- Custom tools, extension loading, approvals, and explicit sandbox policies.
- TypeScript transpilation and stronger JavaScript resource enforcement.
- Optional interactive terminal views, packaged binaries, and published Hydra
  crates.
