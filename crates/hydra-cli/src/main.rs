mod commands;
mod error;
mod progress;
mod provider_adapter;
mod retry_manager;
mod routing;
mod spinner;
mod utils;

use crate::commands::CommandHandler;
use anyhow::Result;
use clap::Parser;

/// CLI entry point for Hydra
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
