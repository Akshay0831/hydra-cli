use anyhow::Result;
use clap::Parser;
use hydra_cli::commands::CommandHandler;
use hydra_cli::progress::CliFeedback;

/// CLI entry point
#[derive(Debug, Parser)]
#[command(name = "hydra", version, about = "Hydra agentic coding assistant")]
struct Cli {
    /// Emit NDJSON events to stdout instead of human-readable output.
    /// Useful for IDE extensions, pipelines, and programmatic consumers.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: CommandHandler,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    let mut feedback = CliFeedback::new_with_json(cli.json);
    cli.command.handle(&mut feedback).await?;
    Ok(())
}
