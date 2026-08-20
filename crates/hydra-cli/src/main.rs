mod routing;
mod provider_adapter;
mod retry_manager;

use anyhow::Result;
use clap::{Parser, Subcommand};
use routing::{Candidate, RoutingConfig, RoutingRequest};
use provider_adapter::ProviderAdapterFactory;
use retry_manager::RetryManager;

#[derive(Debug, Parser)]
#[command(name = "hydra", version, about = "Hydra agentic coding assistant")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a starter Hydra routing configuration.
    Init {
        /// Destination for the Hydra routing configuration.
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        /// Replace an existing configuration file.
        #[arg(long)]
        force: bool,
    },
    /// List configured profiles without printing credentials.
    Profiles {
        /// Hydra routing configuration JSON file.
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
    },
    /// Validate configured credentials without printing secret values.
    Credentials {
        /// Hydra routing configuration JSON file.
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
    },
    /// List registered upstream providers and their status.
    Providers {
        /// Hydra routing configuration JSON file.
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
    },
    /// Show retry/failover status for providers.
    RetryStatus {
        /// Hydra routing configuration JSON file.
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
    },
    /// Reset retry state for a provider.
    ResetRetry {
        /// Hydra routing configuration JSON file.
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        /// Provider name.
        #[arg(long)]
        provider: String,
        /// Model name.
        #[arg(long)]
        model: String,
        /// Profile name.
        #[arg(long)]
        profile: String,
    },
    /// Preview eligible provider/model/profile candidates.
    Route {
        /// Hydra routing configuration JSON file.
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        /// Agent purpose, such as coding or indexing.
        #[arg(long)]
        purpose: Option<String>,
        /// Required tool capabilities.
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// Preferred provider.
        #[arg(long)]
        provider: Option<String>,
        /// Preferred model.
        #[arg(long)]
        model: Option<String>,
        /// Preferred credential profile.
        #[arg(long)]
        profile: Option<String>,
        /// Candidate in provider/model/profile form; repeatable.
        #[arg(long = "candidate", value_parser = parse_candidate)]
        candidates: Vec<Candidate>,
    },
}

fn parse_candidate(value: &str) -> Result<Candidate, String> {
    let mut parts = value.split('/');
    let provider = parts.next().filter(|part| !part.is_empty());
    let model = parts.next().filter(|part| !part.is_empty());
    let profile = parts.next().filter(|part| !part.is_empty());
    if parts.next().is_some() || provider.is_none() || model.is_none() || profile.is_none() {
        return Err("candidate must use provider/model/profile format".to_string());
    }
    Ok(Candidate::new(
        provider.unwrap_or_default().to_string(),
        model.unwrap_or_default().to_string(),
        profile.unwrap_or_default().to_string(),
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Init { config, force } => {
            RoutingConfig::init(&config, force)?;
            println!("Initialized Hydra config at {}", config.display());
        }
        Command::Profiles { config } => {
            let configured = RoutingConfig::load(&config)?;
            for profile in configured.profiles() {
                println!(
                    "{} providers={} models={}",
                    profile.name,
                    profile.providers.join(","),
                    if profile.models.is_empty() {
                        "*".to_string()
                    } else {
                        profile.models.join(",")
                    }
                );
            }
        }
        Command::Credentials { config } => {
            let configured = RoutingConfig::load(&config)?;
            for candidate in configured.resolve_with_candidates(&[], &RoutingRequest::default()) {
                match configured.resolve_credential(&candidate) {
                    Ok(credential) => println!(
                        "{}/{}/{} available source={:?}",
                        credential.provider, candidate.model, credential.profile, credential.source
                    ),
                    Err(error) => println!(
                        "{}/{}/{} unavailable: {}",
                        candidate.provider, candidate.model, candidate.profile, error
                    ),
                }
            }
        }
        Command::Providers { config } => {
            let configured = RoutingConfig::load(&config)?;
            let factory = ProviderAdapterFactory::new(configured);
            
            println!("Registered upstream providers:");
            for provider_name in factory.registered_providers() {
                println!("- {}", provider_name);
            }
            
            if factory.registered_providers().is_empty() {
                println!("No upstream providers registered. Providers must be registered programmatically.");
            }
        }
        Command::RetryStatus { config } => {
            let configured = RoutingConfig::load(&config)?;
            let mut manager = RetryManager::new(configured);
            
            println!("Provider retry/failover status:");
            for status in manager.get_provider_status() {
                println!(
                    "{}/{}/{} - failures={} success={} last_failure={:?} healthy={}",
                    status.provider,
                    status.model,
                    status.profile,
                    status.consecutive_failures,
                    status.successful_calls,
                    status.last_failure.map(|t| t.elapsed().as_secs()).unwrap_or(0),
                    status.consecutive_failures == 0
                );
                
                if let Some(error) = &status.last_error {
                    println!("  Last error: {}", error);
                }
            }
            
            if manager.get_provider_status().is_empty() {
                println!("No provider retry state available.");
            }
        }
        Command::ResetRetry { config, provider, model, profile } => {
            let configured = RoutingConfig::load(&config)?;
            let mut manager = RetryManager::new(configured);
            
            manager.reset_provider(&provider, &model, &profile);
            println!("Reset retry state for {}/{}/{}", provider, model, profile);
        }
        Command::Route {
            config,
            purpose,
            tools,
            provider,
            model,
            profile,
            candidates,
        } => {
            let configured = RoutingConfig::load(&config)?;
            let request = RoutingRequest {
                purpose,
                required_tools: tools,
                provider,
                model,
                profile,
            };
            for candidate in configured.resolve_with_candidates(&candidates, &request) {
                println!(
                    "{}/{}/{}",
                    candidate.provider, candidate.model, candidate.profile
                );
            }
        }
    }
    Ok(())
}
