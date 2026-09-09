//! crates/hydra-cli/src/consolidator/mod.rs
//!
//! Log deduplication, ranking, and patch consensus reconciliation.

pub mod merger;

pub use merger::{ConsolidatedFinding, Consolidator, FindingPriority};
