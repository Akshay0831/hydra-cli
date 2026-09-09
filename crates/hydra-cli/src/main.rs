pub mod adapters;
pub mod agent_adapter;
mod commands;
pub mod consolidator;
pub mod daemon;
mod error;
pub mod orchestrator;
pub mod partitioner;
mod progress;
mod prompt_router;
mod provider_adapter;
mod retry_manager;
mod retry_state_store;
mod routing;
mod spinner;
pub mod toolchains;
mod utils;

use crate::commands::CommandHandler;
use anyhow::Result;
use clap::Parser;

/// CLI entry point
#[derive(Debug, Parser)]
#[command(name = "hydra", version, about = "Hydra agentic coding assistant")]
struct Cli {
    #[command(subcommand)]
    command: CommandHandler,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let mut feedback = progress::CliFeedback::new();
    let command = Cli::parse().command;
    command.handle(&mut feedback).await?;
    Ok(())
}
