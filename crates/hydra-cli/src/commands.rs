//! Command handlers for Hydra CLI.

use crate::error::{ErrorContext, HydraCliError};
use crate::progress::CliFeedback;
use crate::progress::ProgressManager;
use crate::provider_adapter::ProviderAdapterFactory;
use crate::retry_manager::RetryManager;
use crate::routing::{Candidate, RoutingConfig, RoutingRequest};
use crate::utils::{parse_execution_strategy, parse_languages};
use anyhow::Result;
use clap::Subcommand;
use hydra_dag::{Task, TaskGraph};
use hydra_matrix::{CodeMatrix, IndexConfig};
use hydra_sandbox::Sandbox;
use pi::model::AssistantMessageEvent;
use pi::sdk::{AgentEvent, SessionOptions, BUILTIN_TOOL_NAMES};
use std::path::Path;

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

/// Simple task implementation for DAG execution
struct SimpleTask {
    description: String,
    context: ErrorContext,
}

impl SimpleTask {
    fn new(description: String) -> Self {
        Self {
            description,
            context: ErrorContext::new("simple_task"),
        }
    }
}

#[async_trait::async_trait]
impl Task for SimpleTask {
    type Output = String;

    fn id(&self) -> String {
        format!(
            "task_{}",
            self.description.chars().take(10).collect::<String>()
        )
    }

    fn dependencies(&self) -> Vec<String> {
        Vec::new()
    }

    async fn execute(&self) -> Result<Self::Output> {
        let result = self.execute_with_error_handling().await;
        match result {
            Ok(output) => Ok(output),
            Err(e) => Err(anyhow::anyhow!("Task {} failed: {}", self.id(), e)),
        }
    }
}

impl SimpleTask {
    async fn execute_with_error_handling(&self) -> Result<String> {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        if rand::random::<f64>() < 0.1 {
            return Err(anyhow::anyhow!("Simulated task failure"));
        }

        Ok(format!(
            "Completed: {} (took {}ms)",
            self.description,
            self.context.elapsed_ms()
        ))
    }
}

#[derive(Subcommand, Debug)]
#[command(author, version, about, long_about = None)]
pub enum CommandHandler {
    /// Create a starter Hydra routing configuration.
    Init {
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// List configured profiles without printing credentials.
    Profiles {
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
    },
    /// Validate configured credentials without printing secret values.
    Credentials {
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
    },
    /// List registered upstream providers and their status.
    Providers {
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
    },
    /// Show retry and failover status for providers.
    RetryStatus {
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
    },
    /// Reset retry state for a provider.
    ResetRetry {
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        profile: String,
    },
    /// Preview eligible provider, model, and profile candidates.
    Route {
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        #[arg(long)]
        purpose: Option<String>,
        #[arg(long = "tool")]
        tools: Vec<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long = "candidate", value_parser = parse_candidate)]
        candidates: Vec<Candidate>,
    },
    /// Execute a task with concurrent processing.
    Execute {
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        #[arg(long)]
        task: String,
        #[arg(long, default_value = "4")]
        concurrency: usize,
        #[arg(long, default_value = "concurrent")]
        strategy: String,
    },
    /// Index a codebase for analysis.
    Index {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        #[arg(long, default_value = "code_index.json")]
        output: std::path::PathBuf,
        #[arg(long, value_delimiter = ',')]
        languages: Vec<String>,
    },
    /// Execute JavaScript code in the sandbox.
    Js {
        #[arg(value_name = "CODE")]
        code: String,
    },
    /// Send a coding request through the configured upstream agent.
    Prompt {
        #[arg(value_name = "MESSAGE")]
        message: String,
        #[arg(long, value_delimiter = ',')]
        tools: Option<Vec<String>>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    /// List tools available to the upstream agent.
    Tools,
}

impl CommandHandler {
    pub async fn handle(&self, feedback: &mut CliFeedback) -> Result<(), HydraCliError> {
        match self {
            CommandHandler::Init { config, force } => self.init(config, *force, feedback).await,
            CommandHandler::Profiles { config } => self.profiles(config, feedback).await,
            CommandHandler::Credentials { config } => self.credentials(config, feedback).await,
            CommandHandler::Providers { config } => self.providers(config, feedback).await,
            CommandHandler::RetryStatus { config } => self.retry_status(config, feedback).await,
            CommandHandler::ResetRetry {
                config,
                provider,
                model,
                profile,
            } => {
                self.reset_retry(config, provider, model, profile, feedback)
                    .await
            }
            CommandHandler::Route {
                config,
                purpose,
                tools,
                provider,
                model,
                profile,
                candidates,
            } => {
                self.route(
                    config,
                    purpose.clone(),
                    tools.clone(),
                    provider.clone(),
                    model.clone(),
                    profile.clone(),
                    candidates.clone(),
                    feedback,
                )
                .await
            }
            CommandHandler::Execute {
                config,
                task,
                concurrency,
                strategy,
            } => {
                self.execute(
                    config,
                    task.clone(),
                    *concurrency,
                    strategy.clone(),
                    feedback,
                )
                .await
            }
            CommandHandler::Index {
                root,
                output,
                languages,
            } => self.index(root, output, languages.clone(), feedback).await,
            CommandHandler::Js { code } => self.js(code, feedback).await,
            CommandHandler::Prompt {
                message,
                tools,
                provider,
                model,
            } => self.prompt(message, tools, provider, model, feedback).await,
            CommandHandler::Tools => self.tools(feedback).await,
        }
    }

    async fn init(
        &self,
        config: &Path,
        force: bool,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        RoutingConfig::init(config, force)?;
        feedback.success_message(format!("Initialized Hydra config at {}", config.display()));
        Ok(())
    }

    async fn profiles(
        &self,
        config: &Path,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
        feedback.info_message(format!(
            "Found {} configured profiles",
            configured.profiles().len()
        ));
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
        Ok(())
    }

    async fn credentials(
        &self,
        config: &Path,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
        feedback.info_message("Checking available credentials...".to_string());
        let mut available_count = 0;
        let mut unavailable_count = 0;

        for candidate in configured.resolve_with_candidates(&[], &RoutingRequest::default()) {
            match configured.resolve_credential(&candidate) {
                Ok(credential) => {
                    println!(
                        "{}/{}/{} available source={:?}",
                        credential.provider, candidate.model, credential.profile, credential.source
                    );
                    available_count += 1;
                }
                Err(error) => {
                    println!(
                        "{}/{}/{} unavailable: {}",
                        candidate.provider, candidate.model, candidate.profile, error
                    );
                    unavailable_count += 1;
                }
            }
        }

        feedback.success_message(format!(
            "{} available, {} unavailable credentials",
            available_count, unavailable_count
        ));
        Ok(())
    }

    async fn providers(
        &self,
        config: &Path,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
        let factory = ProviderAdapterFactory::new(configured);

        feedback.info_message("Registered upstream providers:".to_string());
        for provider_name in factory.registered_providers() {
            println!("- {}", provider_name);
        }

        if factory.registered_providers().is_empty() {
            feedback.warning_message(
                "No upstream providers registered. Providers must be registered programmatically."
                    .to_string(),
            );
        } else {
            feedback.success_message(format!(
                "Found {} registered providers",
                factory.registered_providers().len()
            ));
        }
        Ok(())
    }

    async fn retry_status(
        &self,
        config: &Path,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
        let manager = RetryManager::new(configured);

        feedback.info_message("Provider retry/failover status:".to_string());
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
        Ok(())
    }

    async fn reset_retry(
        &self,
        config: &Path,
        provider: &str,
        model: &str,
        profile: &str,
        _feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
        let mut manager = RetryManager::new(configured);

        manager.reset_provider(provider, model, profile);
        println!("Reset retry state for {}/{}/{}", provider, model, profile);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn route(
        &self,
        config: &Path,
        purpose: Option<String>,
        tools: Vec<String>,
        provider: Option<String>,
        model: Option<String>,
        profile: Option<String>,
        candidates: Vec<Candidate>,
        _feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
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
        Ok(())
    }

    async fn execute(
        &self,
        config: &Path,
        task: String,
        concurrency: usize,
        strategy: String,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        feedback.info_message(format!(
            "Starting task execution with config: {}",
            config.display()
        ));

        let _configured = RoutingConfig::load(config).map_err(|e| {
            HydraCliError::Configuration(anyhow::anyhow!("Failed to load config: {}", e))
        })?;
        let strategy = parse_execution_strategy(&strategy)?;

        let mut progress_manager = ProgressManager::new();
        let progress_bar = progress_manager.create_progress_bar("Task Execution".to_string(), 1);
        let mut task_progress = progress_manager.create_task_execution_progress(1);
        task_progress.add_progress_bar(progress_bar.clone());

        let task = Box::new(SimpleTask::new(task));
        let mut graph = TaskGraph::with_capacity(1);
        graph.add_task(task).map_err(|e| {
            HydraCliError::TaskExecution(anyhow::anyhow!("Failed to add task: {}", e))
        })?;

        feedback.info_message(format!(
            "Executing task with strategy: {:?} (concurrency: {})",
            strategy, concurrency
        ));
        progress_bar.set_message("Starting task execution...".to_string());
        let results = graph.execute_concurrent(concurrency, strategy).await;

        if !results.failed.is_empty() {
            let error_msg = results
                .failed
                .first()
                .map(|(id, err)| format!("Task {} failed: {}", id, err))
                .unwrap_or_else(|| "Unknown task failure".to_string());

            task_progress.task_failed("task", &error_msg);
            return Err(HydraCliError::TaskExecution(anyhow::anyhow!(
                "{}", error_msg
            )));
        }

        task_progress.task_completed("task");

        progress_bar.complete();
        feedback.success_message(format!(
            "Task execution completed with strategy: {:?}",
            results.strategy
        ));
        if !results.successful.is_empty() {
            println!("Successfully completed tasks:");
            for (task_id, result) in results.successful {
                println!("  {}: {}", task_id, result);
            }
        }

        let stats = progress_manager.get_statistics();
        if !stats.is_empty() {
            println!("Execution Statistics:");
            for (i, stat) in stats.iter().enumerate() {
                println!("  Set {}: {}/{} completed, {} failed, {:.1}% success rate, {:.2}s elapsed, {:.2}s avg duration", 
                    i + 1, stat.completed, stat.total, stat.failed, 
                    stat.success_rate(), stat.elapsed.as_secs_f64(), stat.average_duration().as_secs_f64());
            }
        }

        Ok(())
    }

    async fn index(
        &self,
        root: &Path,
        output: &Path,
        languages: Vec<String>,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let languages = parse_languages(&languages);
        if languages.is_empty() {
            feedback.info_message(
                "No valid languages specified, using all supported languages".to_string(),
            );
        }

        let mut index_config = IndexConfig {
            paths: vec![format!("{}\\**\\*", root.display())],
            ..IndexConfig::default()
        };
        if !languages.is_empty() {
            index_config.include_patterns = languages
                .iter()
                .map(|language| format!("*.{language}"))
                .collect();
        }

        let mut matrix = CodeMatrix::with_config(index_config).map_err(|e| {
            HydraCliError::Indexing(anyhow::anyhow!("Failed to create code matrix: {}", e))
        })?;

        let context = ErrorContext::new("indexing").with_details(format!(
            "Root: {}, Languages: {:?}",
            root.display(),
            languages
        ));

        let mut progress_manager = ProgressManager::new();
        let progress_bar = progress_manager.create_progress_bar("Indexing".to_string(), 100);

        feedback.info_message(format!("Indexing codebase at: {}", root.display()));
        progress_bar.set_message("Starting indexing...".to_string());

        if !root.exists() {
            progress_bar.complete();
            return Err(HydraCliError::Indexing(anyhow::anyhow!(
                "Index directory {} not found",
                root.display()
            )));
        }

        let indexed_files = matrix.index().await.map_err(|e| {
            HydraCliError::Indexing(anyhow::anyhow!(
                "Failed to index directory {}: {}",
                root.display(),
                e
            ))
        })?;
        progress_bar.set_current(indexed_files);
        progress_bar.set_message(format!("Indexed {} files", indexed_files));

        progress_bar.complete();
        let stats = matrix.get_stats().await;
        feedback.success_message(format!(
            "Found {} indexed elements from {} files",
            stats.total_elements, indexed_files
        ));

        feedback.success_message("Indexing completed successfully".to_string());

        let json = serde_json::to_string_pretty(&stats).map_err(|e| {
            HydraCliError::Indexing(anyhow::anyhow!("Failed to serialize index stats: {}", e))
        })?;

        std::fs::write(output, json).map_err(|e| {
            HydraCliError::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("Failed to write index to {}: {}", output.display(), e),
            ))
        })?;

        println!("Code index saved to: {}", output.display());
        println!("Indexing completed in {}ms", context.elapsed_ms());
        Ok(())
    }

    async fn js(&self, code: &str, _feedback: &mut CliFeedback) -> Result<(), HydraCliError> {
        let mut sandbox = Sandbox::new().map_err(|e| {
            HydraCliError::JavaScript(anyhow::anyhow!("Failed to create sandbox: {}", e))
        })?;

        let context = ErrorContext::new("javascript_execution");
        println!("Executing JavaScript code in sandbox...");

        let result =
            tokio::time::timeout(std::time::Duration::from_secs(30), sandbox.execute(code)).await;

        match result {
            Ok(Ok(sandbox_result)) => {
                println!("Execution result: {:?}", sandbox_result);
                println!("Execution completed in {}ms", context.elapsed_ms());
            }
            Ok(Err(e)) => {
                return Err(HydraCliError::JavaScript(anyhow::anyhow!(
                    "JavaScript execution failed: {}",
                    e
                )));
            }
            Err(_) => {
                return Err(HydraCliError::JavaScript(anyhow::anyhow!(
                    "JavaScript execution timeout after 30s"
                )));
            }
        }
        Ok(())
    }

    async fn prompt(
        &self,
        message: &str,
        tools: &Option<Vec<String>>,
        provider: &Option<String>,
        model: &Option<String>,
        _feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let enabled_tools = tools.clone().unwrap_or_else(|| {
            BUILTIN_TOOL_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect()
        });
        let options = SessionOptions {
            provider: provider.clone(),
            model: model.clone(),
            enabled_tools: Some(enabled_tools),
            working_directory: Some(std::env::current_dir().map_err(HydraCliError::Io)?),
            ..SessionOptions::default()
        };
        let mut session = pi::sdk::create_agent_session(options)
            .await
            .map_err(|error| HydraCliError::Provider(error.to_string()))?;
        let response = session
            .prompt(message, |event| {
                if let AgentEvent::MessageUpdate {
                    assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
                    ..
                } = event
                {
                    print!("{delta}");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
            })
            .await
            .map_err(|error| HydraCliError::Provider(error.to_string()))?;
        println!();
        let _ = response;
        Ok(())
    }

    async fn tools(&self, _feedback: &mut CliFeedback) -> Result<(), HydraCliError> {
        for name in BUILTIN_TOOL_NAMES {
            println!("{name}");
        }
        Ok(())
    }
}
