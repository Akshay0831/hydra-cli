# Changelog

---

## [0.3.2] — 2026-09-20

### Use Hydra as a Library
- You can now import `hydra-cli` directly into your own Rust project — VS Code extensions, web UIs, IDE plugins, and custom tools can all build on top of Hydra without re-implementing anything.

### Smarter Swarm Agents
- Agents now automatically fix their own mistakes — if a code change fails tests, the swarm retries and repairs it (up to 2 times) before giving up.
- A reviewer agent signs off before any change is accepted, so only verified fixes land.
- Use `--apply` to apply the result directly, or `--output <file>` to save it for review.

### Safe Patching with Checkpoints
- Before applying any patch, Hydra now takes a snapshot of your workspace automatically.
- Use `hydra checkpoint` to save your current state manually, and `hydra undo` to roll back to it.
- Never lose work from a bad patch again.

### Headless Daemon (for Frontends & Tools)
- Hydra can now run as a background daemon and respond to commands over a socket or standard I/O — making it easy to power frontends and editor integrations.
- Supports a full set of remote actions: get/apply/reject patches, trigger swarm runs, manage checkpoints, query workspace status, and more.

### Machine-Readable Output
- Pass `--json` to any command and get clean, structured JSON output — perfect for piping into other tools or building dashboards on top of Hydra.

### Better File Discovery
- Hydra now scans your entire project recursively instead of just the top level, so it finds all your source files regardless of how deep they are.

---

## [0.3.1] — 2026-09-18

### Security & Stability
- Daemon access is now restricted to local connections only, with request size limits.
- Swarm agents fail safely instead of proceeding with invalid or missing worktrees.
- Stricter sandbox rules and improved audit logging across the board.
- More reliable patch tracking to prevent unintended file modifications.

---

## [0.3.0] — 2026-09-15

### Background Daemon & Steering
- Hydra can run as a background service with real-time streaming output and instant cancellation.
- Set high-level goals and let Hydra track progress — with clear points for you to step in and guide it.

### Multi-Toolchain Support
- Automatically detects and runs the right tools for your project: Cargo, npm, pnpm, bun, pytest, and Go.

### Smarter Code Understanding
- Hydra indexes your codebase and uses it to generate better, more context-aware changes.
- Prompt routing improved with smarter model selection and goal-aware context injection.
