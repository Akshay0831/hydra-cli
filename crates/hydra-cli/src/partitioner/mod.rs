//! crates/hydra-cli/src/partitioner/mod.rs
//!
//! Workspace partitioning and AST dependency clustering.

pub mod ast_splitter;

pub use ast_splitter::{AstSplitter, FilePartition};
