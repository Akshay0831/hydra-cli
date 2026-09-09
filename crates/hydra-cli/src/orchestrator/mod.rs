//! Multi-worker swarm orchestration across isolated worktrees.

pub mod steering;
pub mod swarm;

pub use steering::{
    CapabilityTier, CognitiveSteering, DecisionBrief, DecisionOption, DecisionSeam, PlanMilestone,
    PlanExecuteVerifyWorkflow, SpeculativeEngine, SpeculativeOutcome,
};
pub use swarm::{GitWorktreeGuard, SwarmConfig, SwarmEvent, SwarmOrchestrator};
