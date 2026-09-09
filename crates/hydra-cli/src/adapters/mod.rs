//! crates/hydra-cli/src/adapters/mod.rs
//!
//! Unified adapter facades for external subsystems.
//! Pattern B: Exactly one adapter interface file per external dependency.

pub mod litellm;
pub mod mcp;
pub mod pi_agent;

pub use litellm::{LiteLLMConfig, LiteLLMManager};
pub use mcp::{McpAdapter, McpToolInfo};
pub use pi_agent::{AgentAdapter, AgentRole, AgentTurnResult, PromptRequest, ScopeConstraint};
