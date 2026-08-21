mod routing;
mod provider_adapter;
mod retry_manager;

use anyhow::Result;
use clap::{Parser, Subcommand};
use routing::{Candidate, RoutingConfig, RoutingRequest};
use provider_adapter::ProviderAdapterFactory;
use retry_manager::RetryManager;
use hydra_dag::{Task, TaskGraph, ExecutionStrategy};
use hydra_matrix::CodeMatrix;
use hydra_sandbox::Sandbox;

/// A simple task for demonstration purposes
struct SimpleTask {
    description: String,
}

impl SimpleTask {
    fn new(description: String) -> Self {
        Self { description }
    }
}

#[async_trait::async_trait]
impl Task for SimpleTask {
    type Output = String;
    
    fn id(&self) -> String {
        format!("task_{}", self.description.chars().take(10).collect::<String>())
    }
    
    fn dependencies(&self) -> Vec<String> {
        Vec::new()
    }
    
    async fn execute(&self) -> Result<Self::Output> {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        Ok(format!("Completed: {}", self.description))
    }
}

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
    /// Execute a task with concurrent processing.
    Execute {
        /// Hydra routing configuration JSON file.
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        /// Task description or code to execute.
        #[arg(long)]
        task: String,
        /// Maximum concurrent tasks.
        #[arg(long, default_value = "4")]
        concurrency: usize,
        /// Execution strategy: sequential | concurrent | parallel.
        #[arg(long, default_value = "concurrent")]
        strategy: String,
    },
    /// Index codebase for analysis.
    Index {
        /// Root directory to index.
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        /// Output file for code index.
        #[arg(long, default_value = "code_index.json")]
        output: std::path::PathBuf,
        /// Programming languages to index.
        #[arg(long, value_delimiter = ',')]
        languages: Vec<String>,
    },
    /// Execute JavaScript code in sandbox.
    Js {
        /// JavaScript code to execute.
        code: String,
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

fn parse_execution_strategy(strategy: &str) -> Result<ExecutionStrategy> {
    match strategy {
        "sequential" => Ok(ExecutionStrategy::Sequential),
        "concurrent" => Ok(ExecutionStrategy::FailFast),
        "parallel" => Ok(ExecutionStrategy::CollectFailures),
        _ => Err(anyhow::anyhow!("Unknown execution strategy: {}", strategy)),
    }
}

fn parse_languages(langs: &[String]) -> Vec<String> {
    langs.iter()
        .filter_map(|lang| match lang.as_str() {
            "rs" => Some("rs".to_string()),
            "js" => Some("js".to_string()),
            "jsx" => Some("jsx".to_string()),
            "ts" => Some("ts".to_string()),
            "tsx" => Some("tsx".to_string()),
            _ => None,
        })
        .collect()
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
            let _configured = RoutingConfig::load(&config)?;
            let manager = RetryManager::new(_configured);
            
            println!("Provider retry/failover status:");
            for status in manager.get_provider_status() {
                println!(
                    "{}/{}/{} - failures={} success={} last_failure={:?} healthy={}",
                    status.provider,
                    status.model,
                    status.profile,
                    status.consecutive_failures,
                    status.successful_calls,
                    status.last_failure.map(|t| t.timestamp_secs).unwrap_or(0),
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
        Command::Execute { config, task, concurrency, strategy } => {
            let _configured = RoutingConfig::load(&config)?;
            let strategy = parse_execution_strategy(&strategy)?;
            
            // Create a simple task using a boxed trait object
            let task = Box::new(SimpleTask::new(task.to_string()));
            let mut graph = TaskGraph::new();
            graph.add_task(task)?;
            
            println!("Executing task with strategy: {:?} (concurrency: {})", strategy, concurrency);
            
            let results = graph.execute_concurrent(concurrency, strategy).await;
            println!("Task execution completed with strategy: {:?}", results.strategy);
            if !results.successful.is_empty() {
                println!("Successfully completed tasks:");
                for (task_id, result) in results.successful {
                    println!("  {}: {}", task_id, result);
                }
            }
            if !results.failed.is_empty() {
                println!("Failed tasks:");
                for (task_id, error) in results.failed {
                    println!("  {}: {}", task_id, error);
                }
            }
        }
        Command::Index { root, output, languages } => {
            let languages = parse_languages(&languages);
            if languages.is_empty() {
                println!("No valid languages specified, using all supported languages");
            }
            
            let mut matrix = CodeMatrix::new()?;
            println!("Indexing codebase at: {}", root.display());
            
            // Index files based on language filters
            for file_path in std::fs::read_dir(&root)? {
                let file_path = file_path?.path();
                if file_path.is_file() {
                    if languages.is_empty() || languages.contains(&file_path.extension().unwrap_or_default().to_string_lossy().to_string()) {
                        matrix.index_file(&file_path).await?;
                    }
                }
            }
            
            let stats = matrix.get_stats().await;
            println!("Found {} indexed elements", stats.total_elements);
            
            // Save the index to file
            let json = serde_json::to_string_pretty(&stats)?;
            std::fs::write(&output, json)?;
            println!("Code index saved to: {}", output.display());
        }
        Command::Js { code } => {
            let mut sandbox = Sandbox::new()?;
            println!("Executing JavaScript code in sandbox...");
            
            let result = sandbox.execute(&code).await;
            println!("Execution result: {:?}", result);
        }
    }
    Ok(())
}
