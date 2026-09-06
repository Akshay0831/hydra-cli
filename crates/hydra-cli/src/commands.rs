//! Command handlers for Hydra CLI.

use crate::agent_adapter::{AgentAdapter, PromptRequest};
use crate::error::{ErrorContext, ErrorHandler, HydraCliError};
use crate::progress::CliFeedback;
use crate::progress::ProgressManager;
use crate::provider_adapter::ProviderAdapterFactory;
use crate::retry_manager::{RetryContext, RetryManager};
use crate::routing::{Candidate, Capability, RoutingConfig, RoutingRequest};
use crate::utils::{parse_execution_strategy, parse_languages};
use anyhow::Result;
use clap::Subcommand;
use hydra_dag::{Task, TaskGraph};
use hydra_matrix::{CodeMatrix, IndexConfig};
use hydra_sandbox::Sandbox;
use std::collections::HashSet;
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

fn redact_provider_error(error: &str, secret: &str) -> String {
    if secret.is_empty() {
        return error.to_string();
    }
    error.replace(secret, "[redacted]")
}

/// Simple task implementation for DAG execution
struct SimpleTask {
    description: String,
    context: ErrorContext,
}

struct PromptOptions {
    config: std::path::PathBuf,
    message: String,
    tools: Option<Vec<String>>,
    provider: Option<String>,
    model: Option<String>,
    profile: Option<String>,
    purpose: Option<String>,
    required_capabilities: Vec<String>,
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
        let handler = ErrorHandler::new(2, Default::default());
        let context = self.context.clone().with_retry_count(0);
        handler
            .execute_with_retry("simple task", || {
                if rand::random::<f64>() < 0.1 {
                    return Err(HydraCliError::TaskExecution(anyhow::anyhow!(
                        "temporary task failure"
                    )));
                }
                Ok(format!(
                    "Completed: {} (took {}ms, operation={}, retries={})",
                    self.description,
                    context.elapsed_ms(),
                    context.operation(),
                    context.retry_count()
                ))
            })
            .await
            .map_err(anyhow::Error::msg)
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
        #[arg(long = "capability")]
        capabilities: Vec<String>,
        #[arg(long = "required-capability")]
        required_capabilities: Vec<String>,
        #[arg(long, default_value_t = 0)]
        preference: u32,
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
        #[arg(long, default_value = "hydra.json")]
        config: std::path::PathBuf,
        #[arg(value_name = "MESSAGE")]
        message: String,
        #[arg(long, value_delimiter = ',')]
        tools: Option<Vec<String>>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        purpose: Option<String>,
        #[arg(long = "required-capability")]
        required_capabilities: Vec<String>,
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
                capabilities,
                required_capabilities,
                preference,
                provider,
                model,
                profile,
                candidates,
            } => {
                self.route(
                    config,
                    purpose.clone(),
                    tools.clone(),
                    capabilities.clone(),
                    required_capabilities.clone(),
                    *preference,
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
                config,
                message,
                tools,
                provider,
                model,
                profile,
                purpose,
                required_capabilities,
            } => {
                self.prompt(
                    PromptOptions {
                        config: config.clone(),
                        message: message.clone(),
                        tools: tools.clone(),
                        provider: provider.clone(),
                        model: model.clone(),
                        profile: profile.clone(),
                        purpose: purpose.clone(),
                        required_capabilities: required_capabilities.clone(),
                    },
                    feedback,
                )
                .await
            }
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
        let mut factory = ProviderAdapterFactory::new(configured.clone());
        for provider in configured
            .profiles()
            .into_iter()
            .flat_map(|profile| profile.providers)
        {
            factory.register_provider(provider.clone(), provider);
        }

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
        let candidates = configured.resolve_with_candidates(&[], &RoutingRequest::default());
        let mut retry_manager = RetryManager::new(configured.clone());
        for candidate in &candidates {
            retry_manager.get_or_create_state(candidate);
        }
        let adapters = factory
            .create_adapters_for_request(&RoutingRequest::default(), &[])
            .await?;
        let ready = adapters
            .iter()
            .map(|adapter| {
                format!(
                    "{}/{}/{}",
                    adapter.candidate().provider,
                    adapter.candidate().model,
                    adapter.candidate().profile
                )
            })
            .collect::<HashSet<_>>();
        for adapter in adapters {
            let candidate = adapter.candidate();
            let retry_context = RetryContext::new(&mut retry_manager, candidate.clone(), 0);
            let backoff = retry_context.backoff_duration();
            retry_context.mark_success();
            println!(
                "  ready: {}/{}/{} ({:?}, backoff={}ms)",
                candidate.provider,
                candidate.model,
                candidate.profile,
                adapter.resolved_credential().source,
                backoff.as_millis()
            );
        }
        for candidate in candidates {
            let key = format!(
                "{}/{}/{}",
                candidate.provider, candidate.model, candidate.profile
            );
            if !ready.contains(&key) {
                RetryContext::new(&mut retry_manager, candidate, 0)
                    .mark_failure("provider adapter unavailable".to_string());
            }
        }
        Ok(())
    }

    async fn retry_status(
        &self,
        config: &Path,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
        let mut manager = RetryManager::with_config(configured.clone(), Default::default());
        for candidate in configured.resolve_with_candidates(&[], &RoutingRequest::default()) {
            manager.get_or_create_state(&candidate);
        }

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
        let healthy = manager.get_healthy_candidates(&RoutingRequest::default(), &[]);
        println!("Healthy candidates: {}", healthy.len());
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

        if provider == "*" {
            manager.reset_all();
        } else {
            let candidate =
                Candidate::new(provider.to_string(), model.to_string(), profile.to_string());
            manager.get_or_create_state(&candidate);
            manager.reset_provider(provider, model, profile);
        }
        println!("Reset retry state for {}/{}/{}", provider, model, profile);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn route(
        &self,
        config: &Path,
        purpose: Option<String>,
        tools: Vec<String>,
        capabilities: Vec<String>,
        required_capabilities: Vec<String>,
        preference: u32,
        provider: Option<String>,
        model: Option<String>,
        profile: Option<String>,
        candidates: Vec<Candidate>,
        _feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
        let mut candidates = candidates;
        for candidate in &mut candidates {
            *candidate = candidate.clone().with_preference(preference);
            for capability in &capabilities {
                *candidate = candidate
                    .clone()
                    .with_capability(Capability::from_str(capability)?);
            }
            for capability in &required_capabilities {
                *candidate = candidate
                    .clone()
                    .with_required_capability(Capability::from_str(capability)?);
            }
        }
        let request = RoutingRequest {
            purpose,
            required_tools: tools,
            required_capabilities,
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
        feedback.start_spinner("Preparing task execution".to_string());
        feedback.update_spinner("Building task graph".to_string());
        feedback.info_message(format!(
            "Starting task execution with config: {}",
            config.display()
        ));

        let _configured = match RoutingConfig::load(config) {
            Ok(configured) => configured,
            Err(error) => {
                feedback.stop_spinner();
                return Err(HydraCliError::Configuration(anyhow::anyhow!(
                    "Failed to load config: {error}"
                )));
            }
        };
        let strategy = match parse_execution_strategy(&strategy) {
            Ok(strategy) => strategy,
            Err(error) => {
                feedback.stop_spinner();
                return Err(error.into());
            }
        };

        let mut progress_manager = ProgressManager::new();
        let progress_bar = progress_manager.create_progress_bar("Task Execution".to_string(), 1);
        progress_bar.increment();
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
            feedback.stop_spinner();
            return Err(HydraCliError::TaskExecution(anyhow::anyhow!(
                "{}", error_msg
            )));
        }

        task_progress.task_completed("task");

        progress_manager.complete_all();
        feedback.stop_spinner();
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
        feedback.start_spinner(format!("Indexing {}", root.display()));
        feedback.update_spinner("Scanning files".to_string());
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

        let mut matrix = match CodeMatrix::with_config(index_config) {
            Ok(matrix) => matrix,
            Err(error) => {
                feedback.stop_spinner();
                return Err(HydraCliError::Indexing(anyhow::anyhow!(
                    "Failed to create code matrix: {error}"
                )));
            }
        };

        let context = ErrorContext::new("indexing");

        let mut progress_manager = ProgressManager::new();
        let progress_bar = progress_manager.create_progress_bar("Indexing".to_string(), 100);
        progress_bar.increment();

        feedback.info_message(format!("Indexing codebase at: {}", root.display()));
        progress_bar.set_message("Starting indexing...".to_string());

        if !root.exists() {
            progress_manager.complete_all();
            feedback.stop_spinner();
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

        progress_manager.complete_all();
        feedback.stop_spinner();
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

    async fn js(&self, code: &str, feedback: &mut CliFeedback) -> Result<(), HydraCliError> {
        feedback.start_spinner("Executing JavaScript".to_string());
        feedback.update_spinner("Starting sandbox".to_string());
        let mut sandbox = match Sandbox::new() {
            Ok(sandbox) => sandbox,
            Err(error) => {
                feedback.stop_spinner();
                return Err(HydraCliError::JavaScript(anyhow::anyhow!(
                    "Failed to create sandbox: {error}"
                )));
            }
        };

        let context = ErrorContext::new("javascript_execution");
        println!("Executing JavaScript code in sandbox...");

        let result =
            tokio::time::timeout(std::time::Duration::from_secs(30), sandbox.execute(code)).await;

        match result {
            Ok(Ok(sandbox_result)) => {
                feedback.stop_spinner();
                println!("Execution result: {:?}", sandbox_result);
                println!("Execution completed in {}ms", context.elapsed_ms());
            }
            Ok(Err(e)) => {
                feedback.stop_spinner();
                return Err(HydraCliError::JavaScript(anyhow::anyhow!(
                    "JavaScript execution failed: {}",
                    e
                )));
            }
            Err(_) => {
                feedback.stop_spinner();
                return Err(HydraCliError::JavaScript(anyhow::anyhow!(
                    "JavaScript execution timeout after 30s"
                )));
            }
        }
        Ok(())
    }

    async fn prompt(
        &self,
        options: PromptOptions,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(&options.config)?;
        let enabled_tools = options.tools.clone().unwrap_or_else(|| {
            AgentAdapter::builtin_tool_names()
                .iter()
                .map(|name| (*name).to_string())
                .collect()
        });
        let request = RoutingRequest {
            purpose: options.purpose.clone(),
            required_tools: options.tools.clone().unwrap_or_default(),
            required_capabilities: options.required_capabilities.clone(),
            provider: options.provider.clone(),
            model: options.model.clone(),
            profile: options.profile.clone(),
        };
        let mut factory = ProviderAdapterFactory::new(configured.clone());
        for provider_name in configured
            .profiles()
            .into_iter()
            .flat_map(|profile| profile.providers)
        {
            factory.register_provider(provider_name.clone(), provider_name);
        }
        let mut retry_manager = RetryManager::with_config(
            configured.clone(),
            crate::retry_manager::RetryConfig {
                enable_jitter: false,
                ..Default::default()
            },
        );
        let candidates = retry_manager.get_healthy_candidates(&request, &[]);
        if candidates.is_empty() {
            return Err(HydraCliError::NoEligibleCandidate {
                request: format!(
                    "provider={:?}, model={:?}, profile={:?}",
                    options.provider, options.model, options.profile
                ),
            });
        }
        let working_directory = std::env::current_dir().map_err(HydraCliError::Io)?;
        feedback.start_spinner("Routing prompt".to_string());
        let mut last_error = None;

        for candidate in candidates {
            let adapter = match factory.create_adapter(&candidate).await {
                Ok(adapter) => adapter,
                Err(error) => {
                    feedback.stop_spinner();
                    return Err(HydraCliError::ProviderAttempt {
                        candidate: format!(
                            "{}/{}/{}",
                            candidate.provider, candidate.model, candidate.profile
                        ),
                        attempt: 1,
                        error: error.to_string(),
                    });
                }
            };
            let Some(adapter) = adapter else {
                continue;
            };
            retry_manager.get_or_create_state(&candidate);
            for attempt in 1..=retry_manager.retry_config.max_attempts {
                feedback.update_spinner(format!(
                    "Trying {}/{} (attempt {attempt})",
                    adapter.provider(),
                    adapter.model()
                ));
                let result = AgentAdapter::prompt(
                    PromptRequest {
                        message: options.message.clone(),
                        tools: enabled_tools.clone(),
                        provider: Some(adapter.provider().to_string()),
                        model: Some(adapter.model().to_string()),
                        api_key: Some(adapter.api_key().to_string()),
                        working_directory: working_directory.clone(),
                    },
                    |delta| {
                        print!("{delta}");
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    },
                )
                .await;
                match result {
                    Ok(()) => {
                        retry_manager.mark_success(&candidate);
                        feedback.stop_spinner();
                        println!();
                        return Ok(());
                    }
                    Err(error) => {
                        let error_message =
                            redact_provider_error(&error.to_string(), adapter.api_key());
                        let error = HydraCliError::ProviderAttempt {
                            candidate: format!(
                                "{}/{}/{}",
                                candidate.provider, candidate.model, candidate.profile
                            ),
                            attempt,
                            error: error_message,
                        };
                        let retryable =
                            ErrorHandler::new(0, Default::default()).is_retryable(&error);
                        retry_manager.mark_failure(&candidate, error.to_string());
                        last_error = Some(error);
                        if !retryable || attempt == retry_manager.retry_config.max_attempts {
                            break;
                        }
                        tokio::time::sleep(retry_manager.get_backoff_duration(&candidate)).await;
                    }
                }
            }
        }
        feedback.stop_spinner();
        Err(
            last_error.unwrap_or_else(|| HydraCliError::NoEligibleCandidate {
                request: "all eligible provider adapters were unavailable".to_string(),
            }),
        )
    }

    async fn tools(&self, _feedback: &mut CliFeedback) -> Result<(), HydraCliError> {
        for name in AgentAdapter::builtin_tool_names() {
            println!("{name}");
        }
        Ok(())
    }
}
