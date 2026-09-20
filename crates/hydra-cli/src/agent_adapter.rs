// Re-export bridge to pi_agent facade; preserves backward compatibility

pub use crate::adapters::pi_agent::{
    AgentAdapter, AgentRole, AgentTurnResult, PromptRequest, ScopeConstraint,
};
