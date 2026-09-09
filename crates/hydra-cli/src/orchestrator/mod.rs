//! crates/hydra-cli/src/orchestrator/mod.rs
//!
//! Multi-worker swarm orchestration across isolated worktrees.

pub mod swarm;

pub use swarm::{GitWorktreeGuard, SwarmConfig, SwarmEvent, SwarmOrchestrator};
