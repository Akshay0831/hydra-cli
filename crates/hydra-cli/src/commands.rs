//! Command handlers for Hydra CLI.

use crate::agent_adapter::AgentAdapter;
use crate::error::{ErrorContext, HydraCliError};
use crate::progress::CliFeedback;
use crate::progress::ProgressManager;
use crate::prompt_router::PromptRouter;
use crate::provider_adapter::ProviderAdapterFactory;
use crate::retry_manager::RetryManager;
use crate::retry_state_store::RetryStateStore;
use crate::routing::{Candidate, Capability, RoutingConfig, RoutingRequest};
use crate::utils::{parse_execution_strategy, parse_languages};
use anyhow::Result;
use clap::Subcommand;
use hydra_dag::{SimpleTask, TaskGraph};
use hydra_matrix::{CodeMatrix, IndexConfig};
use hydra_sandbox::Sandbox;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
struct TaskFile {
    tasks: Vec<TaskDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
struct TaskDefinition {
    id: String,
    #[serde(default)]
    dependencies: Vec<String>,
    description: String,
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

pub fn redact_provider_error(error: &str, secret: &str) -> String {
    if secret.is_empty() {
        return error.to_string();
    }
    error.replace(secret, "[redacted]")
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
        #[arg(long = "task")]
        task: Vec<String>,
        #[arg(long)]
        tasks: Option<std::path::PathBuf>,
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
    /// Execute an autonomous parallel multi-agent swarm across workspace partitions.
    Swarm {
        #[arg(value_name = "INTENT")]
        intent: String,
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
        #[arg(long, default_value_t = 4)]
        concurrency: usize,
        #[arg(long, default_value = "gemini-2.5-pro")]
        coder_model: String,
        #[arg(long, default_value = "claude-3-5-sonnet")]
        reviewer_model: String,
    },
}

impl CommandHandler {
    fn retry_state_path(config: &Path) -> PathBuf {
        PathBuf::from(format!("{}.retry.json", config.display()))
    }

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
                tasks,
                concurrency,
                strategy,
            } => {
                self.execute(
                    config,
                    task.clone(),
                    tasks.clone(),
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
            CommandHandler::Swarm {
                intent,
                root,
                concurrency,
                coder_model,
                reviewer_model,
            } => {
                self.swarm(
                    intent,
                    root,
                    *concurrency,
                    coder_model,
                    reviewer_model,
                    feedback,
                )
                .await
            }
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
            factory.register_provider(provider)?;
        }

        let registered_providers = factory.registered_provider_names();
        feedback.info_message("Registered upstream providers:".to_string());
        for provider_name in &registered_providers {
            println!("- {}", provider_name);
        }

        if registered_providers.is_empty() {
            feedback.warning_message(
                "No upstream providers registered. Providers must be registered programmatically."
                    .to_string(),
            );
        } else {
            feedback.success_message(format!(
                "Found {} registered providers",
                registered_providers.len()
            ));
        }
        let candidates = configured.resolve_with_candidates(&[], &RoutingRequest::default());
        let mut retry_manager =
            RetryManager::new(configured.clone()).with_state_store(Self::retry_state_path(config));
        for candidate in &candidates {
            retry_manager.get_or_create_state(candidate);
        }
        let adapters = factory
            .create_adapters_for_request(&RoutingRequest::default(), &[])
            .await?;
        for adapter in adapters {
            let candidate = adapter.candidate();
            let backoff = retry_manager.get_backoff_duration(candidate);
            println!(
                "  ready: {}/{}/{} ({:?}, backoff={}ms)",
                candidate.provider,
                candidate.model,
                candidate.profile,
                adapter.resolved_credential().source,
                backoff.as_millis()
            );
        }
        Ok(())
    }

    async fn retry_status(
        &self,
        config: &Path,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(config)?;
        let state_path = Self::retry_state_path(config);
        let mut state = RetryStateStore::load(&state_path)?;
        for candidate in configured.resolve_with_candidates(&[], &RoutingRequest::default()) {
            state.get_provider_state(&candidate);
        }
        RetryStateStore::save(&state_path, &state)?;
        let manager = RetryManager::with_config(configured.clone(), Default::default())
            .with_state_store(state_path);

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
        if provider == "*" {
            RetryStateStore::reset_all(&Self::retry_state_path(config))?;
        } else {
            let candidate =
                Candidate::new(provider.to_string(), model.to_string(), profile.to_string());
            RetryStateStore::reset_candidate(&Self::retry_state_path(config), &candidate)?;
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
        task: Vec<String>,
        tasks_path: Option<PathBuf>,
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
                return Err(HydraCliError::Configuration(anyhow::anyhow!(
                    "Failed to load config: {error}"
                )));
            }
        };
        let strategy = match parse_execution_strategy(&strategy) {
            Ok(strategy) => strategy,
            Err(error) => {
                return Err(error.into());
            }
        };

        let task_definitions = if let Some(tasks_path) = tasks_path {
            let contents = std::fs::read_to_string(&tasks_path).map_err(|error| {
                HydraCliError::TaskExecution(anyhow::anyhow!(
                    "Failed to read task file {}: {}",
                    tasks_path.display(),
                    error
                ))
            })?;
            serde_json::from_str::<TaskFile>(&contents)
                .map(|file| file.tasks)
                .map_err(|error| {
                    HydraCliError::TaskExecution(anyhow::anyhow!(
                        "Failed to parse task file {}: {}",
                        tasks_path.display(),
                        error
                    ))
                })?
        } else {
            task.into_iter()
                .enumerate()
                .map(|(index, description)| TaskDefinition {
                    id: format!("task-{}", index + 1),
                    dependencies: Vec::new(),
                    description,
                })
                .collect()
        };

        if task_definitions.is_empty() {
            return Err(HydraCliError::TaskExecution(anyhow::anyhow!(
                "at least one --task or --tasks input is required"
            )));
        }

        let total_tasks = task_definitions.len();
        let mut progress_manager = ProgressManager::new();
        let progress_bar =
            progress_manager.create_progress_bar("Task Execution".to_string(), total_tasks);
        let mut task_progress = progress_manager.create_task_execution_progress(total_tasks);
        task_progress.add_progress_bar(progress_bar.clone());

        let mut graph = TaskGraph::with_capacity(total_tasks);
        for definition in task_definitions {
            let id = definition.id;
            let description = definition.description;
            graph
                .add_task(Box::new(SimpleTask::new(
                    id,
                    definition.dependencies,
                    move || Ok(description.clone()),
                )))
                .map_err(|error| {
                    HydraCliError::TaskExecution(anyhow::anyhow!(
                        "Failed to add task to graph: {}",
                        error
                    ))
                })?;
        }

        feedback.info_message(format!(
            "Executing task with strategy: {:?} (concurrency: {})",
            strategy, concurrency
        ));
        let spinner_guard = feedback.spinner_guard();
        progress_bar.set_message("Starting task execution...".to_string());
        let results = graph.execute_concurrent(concurrency, strategy).await;

        if !results.failed.is_empty() {
            let error_msg = results
                .failed
                .first()
                .map(|(id, err)| format!("Task {} failed: {}", id, err))
                .unwrap_or_else(|| "Unknown task failure".to_string());

            if let Some((task_id, _)) = results.failed.first() {
                task_progress.task_failed(task_id, &error_msg);
            }
            progress_manager.complete_all();
            drop(spinner_guard);
            return Err(HydraCliError::TaskExecution(anyhow::anyhow!(
                "{}", error_msg
            )));
        }

        for task_id in results.successful.keys() {
            task_progress.task_completed(task_id);
        }

        progress_manager.complete_all();
        drop(spinner_guard);
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
                println!(
                    "  Set {}: {}/{} completed, {} failed, {:.1}% success rate, {:.2}s elapsed, {:.2}s avg duration",
                    i + 1,
                    stat.completed,
                    stat.total,
                    stat.failed,
                    stat.success_rate(),
                    stat.elapsed.as_secs_f64(),
                    stat.average_duration().as_secs_f64()
                );
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
            paths: vec![root.to_string_lossy().to_string()],
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

        let spinner_guard = feedback.spinner_guard();
        let indexed_files = matrix.index().await.map_err(|e| {
            HydraCliError::Indexing(anyhow::anyhow!(
                "Failed to index directory {}: {}",
                root.display(),
                e
            ))
        })?;
        drop(spinner_guard);
        progress_bar.set_current(100);
        progress_bar.set_message(format!("Indexed {} files", indexed_files));

        progress_manager.complete_all();
        feedback.stop_spinner();
        let stats = matrix.get_stats().await;
        feedback.success_message(format!(
            "Found {} indexed elements from {} files",
            stats.total_elements, indexed_files
        ));

        feedback.success_message("Indexing completed successfully".to_string());

        matrix.save_index(output).await.map_err(|e| {
            HydraCliError::Io(std::io::Error::other(format!(
                "Failed to write index to {}: {}",
                output.display(),
                e
            )))
        })?;

        println!("Code index saved to: {}", output.display());
        println!("Indexing completed in {}ms", context.elapsed_ms());
        Ok(())
    }

    async fn js(&self, code: &str, feedback: &mut CliFeedback) -> Result<(), HydraCliError> {
        feedback.start_spinner("Executing JavaScript".to_string());
        feedback.update_spinner("Starting sandbox".to_string());
        let spinner_guard = feedback.spinner_guard();
        let mut sandbox = match Sandbox::new() {
            Ok(sandbox) => sandbox,
            Err(error) => {
                return Err(HydraCliError::JavaScript(anyhow::anyhow!(
                    "Failed to create sandbox: {error}"
                )));
            }
        };

        let context = ErrorContext::new("javascript_execution");
        println!("Executing JavaScript code in sandbox...");

        let sandbox_result = sandbox.execute(code).await.map_err(|error| {
            HydraCliError::JavaScript(anyhow::anyhow!("JavaScript execution failed: {}", error))
        })?;
        drop(spinner_guard);
        println!("Execution result: {:?}", sandbox_result);
        println!("Execution completed in {}ms", context.elapsed_ms());
        sandbox.cleanup().await.map_err(|error| {
            HydraCliError::JavaScript(anyhow::anyhow!("Failed to clean up sandbox: {}", error))
        })?;
        Ok(())
    }

    async fn prompt(
        &self,
        options: PromptOptions,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        let configured = RoutingConfig::load(&options.config)?;
        let _enabled_tools = options.tools.clone().unwrap_or_else(|| {
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

        // Set up retry state store path
        let retry_state_path = Self::retry_state_path(&options.config);

        let retry_manager = RetryManager::with_config(
            configured.clone(),
            crate::retry_manager::RetryConfig {
                enable_jitter: false,
                ..Default::default()
            },
        )
        .with_state_store(retry_state_path.clone());

        // Create prompt router with retry manager
        let working_directory = std::env::current_dir().map_err(HydraCliError::Io)?;
        feedback.start_spinner("Routing prompt".to_string());
        let spinner_guard = feedback.spinner_guard();

        let mut router = PromptRouter::new(configured.clone(), retry_manager);

        let on_text = crate::prompt_router::TextOutputWrapper::new(|delta| {
            print!("{delta}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        });

        let result = router
            .execute(request, options.message, working_directory, on_text)
            .await;

        drop(spinner_guard);
        match result {
            Ok(()) => {
                println!();
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn tools(&self, _feedback: &mut CliFeedback) -> Result<(), HydraCliError> {
        for name in AgentAdapter::builtin_tool_names() {
            println!("{name}");
        }
        Ok(())
    }

    async fn swarm(
        &self,
        intent: &str,
        root: &Path,
        concurrency: usize,
        coder_model: &str,
        reviewer_model: &str,
        feedback: &mut CliFeedback,
    ) -> Result<(), HydraCliError> {
        feedback.info_message(format!("Initiating Hydra Swarm for intent: \"{intent}\""));
        feedback.start_spinner("Partitioning workspace AST dependencies...".to_string());

        let partitioner = crate::partitioner::AstSplitter::from_workspace(root)
            .map_err(|e| HydraCliError::TaskExecution(anyhow::anyhow!("Failed to initialize AST partitioner: {e}")))?;

        // Find candidate project files to partition
        let mut target_files = Vec::new();
        let src_dir = root.join("src");
        if src_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&src_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().map(|e| e == "rs" || e == "js" || e == "ts").unwrap_or(false) {
                        target_files.push(p);
                    }
                }
            }
        }
        if target_files.is_empty() {
            target_files.push(root.join("src/lib.rs"));
        }

        let partitions = partitioner.partition_workspace(&target_files, concurrency).await
            .map_err(|e| HydraCliError::TaskExecution(anyhow::anyhow!("AST Partitioning failed: {e}")))?;

        feedback.update_spinner(format!("Spawned {} workspace partitions", partitions.len()));

        let config = crate::orchestrator::SwarmConfig {
            max_concurrency: concurrency,
            coder_model: coder_model.to_string(),
            reviewer_model: reviewer_model.to_string(),
        };

        let orchestrator = crate::orchestrator::SwarmOrchestrator::new(config, None);
        let (tx, mut rx) = tokio::sync::mpsc::channel(128);

        // Process swarm events
        let feedback_printer = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    crate::orchestrator::SwarmEvent::WorkerTurnStarted { partition_id, role } => {
                        println!("  [{partition_id}] Worker {role} started turn");
                    }
                    crate::orchestrator::SwarmEvent::DiffReady { partition_id, diff } => {
                        println!("  [{partition_id}] Diff generated ({} bytes)", diff.len());
                    }
                    crate::orchestrator::SwarmEvent::TestCompleted { partition_id, passed, .. } => {
                        println!("  [{partition_id}] Test execution passed: {passed}");
                    }
                    crate::orchestrator::SwarmEvent::ReviewCompleted { partition_id, approved, .. } => {
                        println!("  [{partition_id}] Audit review approved: {approved}");
                    }
                    crate::orchestrator::SwarmEvent::ConsensusReached { partition_id } => {
                        println!("  [{partition_id}] Unanimous consensus reached!");
                    }
                    crate::orchestrator::SwarmEvent::PartitionFailed { partition_id, error } => {
                        eprintln!("  [{partition_id}] FAILED: {error}");
                    }
                    _ => {}
                }
            }
        });

        let results = orchestrator.run_swarm(root, intent, partitions, tx).await
            .map_err(|e| HydraCliError::TaskExecution(anyhow::anyhow!("Swarm execution failed: {e}")))?;

        let _ = feedback_printer.await;
        feedback.stop_spinner();

        let diffs: Vec<String> = results.into_iter().map(|(_, d)| d).collect();
        let unified_patch = crate::consolidator::Consolidator::reconcile_diffs(&diffs)
            .map_err(|e| HydraCliError::TaskExecution(anyhow::anyhow!("Diff consolidation failed: {e}")))?;

        if unified_patch.is_empty() {
            feedback.info_message("Swarm completed with no modifications required.".to_string());
        } else {
            feedback.success_message(format!(
                "Swarm achieved consensus! Generated unified patch ({} lines)",
                unified_patch.lines().count()
            ));
        }

        Ok(())
    }
}
