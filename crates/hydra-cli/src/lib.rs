// Autonomous AI coding orchestrator with AST partitioning and provider routing

pub mod adapters;
pub mod agent_adapter;
pub mod checkpoints;
pub mod commands;
pub mod consolidator;
pub mod daemon;
pub mod error;
pub mod orchestrator;
pub mod partitioner;
pub mod progress;
pub mod prompt_router;
pub mod provider_adapter;
pub mod retry_manager;
pub mod retry_state_store;
pub mod routing;
pub mod spinner;
pub mod toolchains;
pub mod utils;

// Re-exports for convenient high-level consumption by external frontend crates
pub use agent_adapter::{AgentAdapter, AgentRole, AgentTurnResult, PromptRequest, ScopeConstraint};
pub use checkpoints::{CheckpointManager, CheckpointMeta, checkpoint_before_patch};
pub use consolidator::{ConsolidatedFinding, Consolidator, FindingPriority};
pub use daemon::{
    CancellationToken, DaemonState, JsonRpcError, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, dispatch_json_rpc, run_daemon_server, run_daemon_stdio,
};
pub use error::{ErrorContext, ErrorHandler, HydraCliError};
pub use orchestrator::{
    CognitiveSteering, DecisionBrief, DecisionOption, DecisionSeam, GitWorktreeGuard,
    SpeculativeEngine, SpeculativeOutcome, SwarmConfig, SwarmEvent, SwarmOrchestrator,
};
pub use partitioner::{AstSplitter, FilePartition};
pub use prompt_router::{ProjectGoalRegistry, PromptPrefixAligner, PromptRouter};
pub use retry_manager::{RetryConfig, RetryManager};
pub use routing::{Candidate, Capability, RoutingConfig, RoutingRequest};
pub use toolchains::{MultiToolchainGate, ToolchainKind, ToolchainReport};
