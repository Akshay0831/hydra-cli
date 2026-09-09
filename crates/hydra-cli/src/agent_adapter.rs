//! crates/hydra-cli/src/agent_adapter.rs
//!
//! Re-export bridge to the unified `crate::adapters::pi_agent` facade.
//! Preserves backward compatibility while enforcing single gateway execution.

pub use crate::adapters::pi_agent::{
    AgentAdapter, AgentRole, AgentTurnResult, PromptRequest, ScopeConstraint,
};
