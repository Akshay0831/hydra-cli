# Dependency Plan

## Architecture

- Keep external projects behind Hydra-owned adapters.
- Application code uses adapter APIs, not upstream types.
- Keep routing, policy, configuration, and orchestration in Hydra.
- Keep upstream authentication, providers, and runtime logic upstream.
- One adapter per major dependency or integration boundary.

Example:

```text
Hydra commands
    -> Hydra adapter
        -> upstream dependency
```

## Dependency Sources

Use the least-maintenance source that supports the current phase:

1. Published crate with a pinned version and lockfile.
2. Git dependency pinned to an exact revision.
3. Submodule for active coordinated development only.

Keep submodules outside the Cargo workspace when they are consumed only as
path dependencies. This keeps Hydra builds, tests, and lints focused.

## Phases

### Development

- Use a local path or submodule for fast iteration.
- Pin the exact commit in the parent repository.
- Keep upstream changes separate from Hydra changes.

### Stabilization

- Freeze the adapter API.
- Add adapter tests and one integration smoke test.
- Remove direct upstream imports from application code.

### Release

- Prefer a published version; otherwise use an exact Git revision.
- Commit `Cargo.lock`.
- Record the source, revision, and required features.
- Publish internal crates before packaging dependents.
- Re-run workspace check, test, lint, format, and smoke tests.

## `pi_agent_rust`

`pi_agent_rust` is the current reference dependency.

- Adapter: `crates/hydra-cli/src/agent_adapter.rs`
- Cargo package: `pi_agent_rust`
- Current integration: local path or pinned submodule
- Hydra boundary: `AgentAdapter` and `PromptRequest`
- Do not expose `pi` SDK types outside the adapter.

If Hydra needs a missing Pi capability, add an explicit upstream SDK change,
pin the resulting revision, and document the synchronization step.

## Required Checks

For every dependency change:

- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- Relevant CLI or integration smoke test