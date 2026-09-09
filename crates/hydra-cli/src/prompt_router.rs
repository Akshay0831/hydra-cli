//! Prompt router with retry logic and failover handling.

use crate::agent_adapter::{AgentAdapter, PromptRequest};
use crate::error::{ErrorHandler, HydraCliError};
use crate::provider_adapter::ProviderAdapterFactory;
use crate::retry_manager::RetryManager;
use crate::routing::{RoutingConfig, RoutingRequest};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

/// Project goal and architectural invariant registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ProjectGoalRegistry {
    pub project_name: String,
    pub primary_goals: Vec<String>,
    pub invariants: Vec<String>,
    pub forbidden_patterns: Vec<String>,
}

impl Default for ProjectGoalRegistry {
    fn default() -> Self {
        Self {
            project_name: "hydra".to_string(),
            primary_goals: vec![
                "High performance single binary autonomous coding orchestrator".to_string(),
                "Token-efficient execution with AST skeletons and zero repetitive reads".to_string(),
            ],
            invariants: vec![
                "All external dependencies interact through Hydra adapter facades".to_string(),
                "Zero panic/unwrap shortcuts in non-test code".to_string(),
                "Hierarchical documentation tree must maintain 100% valid cross-links".to_string(),
            ],
            forbidden_patterns: vec![
                "Ad-hoc *_runner.rs or *_utils.rs wrapper files".to_string(),
                "Unapproved Cargo.toml/package.json dependency additions".to_string(),
            ],
        }
    }
}

#[allow(dead_code)]
impl ProjectGoalRegistry {
    /// Load from .hydra/goals.json or use defaults.
    pub async fn load_or_default(workspace_root: &Path) -> Self {
        let goals_path = workspace_root.join(".hydra").join("goals.json");
        if let Ok(content) = tokio::fs::read_to_string(&goals_path).await {
            if let Ok(registry) = serde_json::from_str(&content) {
                return registry;
            }
        }
        Self::default()
    }

    /// Format goals and invariants for prompt injection.
    pub fn format_invariant_header(&self) -> String {
        let mut out = Vec::new();
        out.push(format!("### ARCHITECTURAL INVARIANTS [{}]", self.project_name));
        for inv in &self.invariants {
            out.push(format!("- INVARIANT: {inv}"));
        }
        for forbid in &self.forbidden_patterns {
            out.push(format!("- FORBIDDEN: {forbid}"));
        }
        out.join("\n")
    }
}

/// Universal KV-cache prefix aligner (Phase 3.5).
/// Enforces deterministic prompt ordering to maximize provider prompt caching hit rate.
#[allow(dead_code)]
pub struct PromptPrefixAligner;

#[allow(dead_code)]
impl PromptPrefixAligner {
    /// Assemble prompts: system -> invariants -> docs -> AST -> user intent.
    pub fn build_cache_aligned_prompt(
        system_persona: &str,
        invariants: &str,
        doc_invariants: &[String],
        ast_skeleton: &str,
        user_intent: &str,
    ) -> String {
        let mut sections = Vec::new();

        // 1. Static system persona (longest lived)
        sections.push(system_persona.to_string());

        // 2. Static macro invariants & goals
        if !invariants.is_empty() {
            sections.push(invariants.to_string());
        }

        // 3. Hierarchical documentation tables
        if !doc_invariants.is_empty() {
            sections.push(format!("### REPOSITORY CONTRACTS\n{}", doc_invariants.join("\n\n")));
        }

        // 4. Code AST Skeletons
        if !ast_skeleton.is_empty() {
            sections.push(format!("### CODE CONTEXT SKELETON\n{}", ast_skeleton));
        }

        // 5. Active User Intent (dynamic payload at tail)
        sections.push(format!("### TASK INTENT\n{}", user_intent));

        sections.join("\n\n")
    }
}

#[async_trait::async_trait]
pub trait PromptExecutor: Send + Sync {
    async fn execute(&self, request: PromptRequest, on_text: TextOutputWrapper) -> Result<()>;
}

#[async_trait::async_trait]
impl PromptExecutor for AgentAdapter {
    async fn execute(&self, request: PromptRequest, on_text: TextOutputWrapper) -> Result<()> {
        AgentAdapter::prompt(request, move |delta| on_text.call(delta)).await
    }
}

pub trait TextOutput: Send + Sync + Clone {
    fn call(&self, delta: &str);
}

#[derive(Clone)]
pub struct TextOutputWrapper {
    inner: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
}

impl TextOutputWrapper {
    pub fn new<F: Fn(&str) + Send + Sync + 'static>(f: F) -> Self {
        Self {
            inner: std::sync::Arc::new(f),
        }
    }
}

impl TextOutput for TextOutputWrapper {
    fn call(&self, delta: &str) {
        (self.inner)(delta);
    }
}

pub struct PromptRouter {
    factory: ProviderAdapterFactory,
    retry_manager: RetryManager,
    executor: Arc<dyn PromptExecutor>,
}

impl PromptRouter {
    pub fn new(config: RoutingConfig, retry_config: RetryManager) -> Self {
        Self::with_executor(config, retry_config, Arc::new(AgentAdapter))
    }

    pub fn with_executor(
        config: RoutingConfig,
        retry_manager: RetryManager,
        executor: Arc<dyn PromptExecutor>,
    ) -> Self {
        let mut factory = ProviderAdapterFactory::new(config);
        for provider in factory.routing_provider_names() {
            let _ = factory.register_provider(provider);
        }
        Self {
            factory,
            retry_manager,
            executor,
        }
    }

    pub async fn execute(
        &mut self,
        request: RoutingRequest,
        message: String,
        working_directory: std::path::PathBuf,
        on_text: TextOutputWrapper,
    ) -> Result<(), HydraCliError> {
        let healthy_candidates = self.retry_manager.get_healthy_candidates(&request, &[]);

        if healthy_candidates.is_empty() {
            return Err(HydraCliError::NoEligibleCandidate {
                request: format!(
                    "provider={:?}, model={:?}, profile={:?}",
                    request.provider, request.model, request.profile
                ),
            });
        }

        let enabled_tools = request.required_tools;

        for candidate in healthy_candidates {
            let adapter = match self.factory.create_adapter(&candidate).await {
                Ok(adapter) => adapter,
                Err(error) => {
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

            let adapter = match adapter {
                Some(adapter) => adapter,
                None => continue,
            };

            self.retry_manager.get_or_create_state(&candidate);

            for attempt in 1..=self.retry_manager.retry_config.max_attempts {
                let on_text_ref = on_text.clone();
                let result = self
                    .executor
                    .execute(
                        PromptRequest {
                            message: message.clone(),
                            tools: enabled_tools.clone(),
                            role: None,
                            scope: None,
                            provider: Some(adapter.provider().to_string()),
                            model: Some(adapter.model().to_string()),
                            model_alias: None,
                            api_key: Some(adapter.api_key().to_string()),
                            working_directory: working_directory.clone(),
                        },
                        on_text_ref,
                    )
                    .await;

                match result {
                    Ok(()) => {
                        self.retry_manager.mark_success(&candidate);
                        return Ok(());
                    }
                    Err(error) => {
                        let error_message = crate::commands::redact_provider_error(
                            &error.to_string(),
                            adapter.api_key(),
                        );
                        let error_for_retry = HydraCliError::ProviderAttempt {
                            candidate: format!(
                                "{}/{}/{}",
                                candidate.provider, candidate.model, candidate.profile
                            ),
                            attempt,
                            error: error_message.clone(),
                        };
                        let retryable =
                            ErrorHandler::new(0, Default::default()).is_retryable(&error_for_retry);

                        self.retry_manager.mark_failure(&candidate, error_message);

                        if !retryable || attempt == self.retry_manager.retry_config.max_attempts {
                            break;
                        }

                        let backoff = self.retry_manager.get_backoff_duration(&candidate);
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }

        Err(HydraCliError::NoEligibleCandidate {
            request: "all eligible provider adapters were unavailable".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{Candidate, Profile};
    use std::sync::Mutex;

    struct TestExecutor {
        requests: Mutex<Vec<PromptRequest>>,
    }

    #[async_trait::async_trait]
    impl PromptExecutor for TestExecutor {
        async fn execute(&self, request: PromptRequest, on_text: TextOutputWrapper) -> Result<()> {
            on_text.call("ok");
            self.requests.lock().unwrap().push(request);
            Ok(())
        }
    }

    #[test]
    fn test_router_initializes_with_config() {
        let config = RoutingConfig::default();
        let retry_manager = RetryManager::new(config.clone());
        let router = PromptRouter::new(config, retry_manager);

        let _router = router;
    }

    #[tokio::test]
    async fn test_router_handles_no_healthy_candidates() {
        let mut config = RoutingConfig::default();
        let mut profile = Profile::default();
        profile
            .credentials
            .insert("test".to_string(), "literal:test".to_string());
        config.profiles.insert("test".to_string(), profile);

        let retry_manager = RetryManager::new(config.clone());
        let mut router = PromptRouter::new(config, retry_manager);

        let request = RoutingRequest::default();
        let output = TextOutputWrapper::new(|_| {});
        let result = router
            .execute(
                request,
                String::new(),
                std::env::current_dir().unwrap(),
                output,
            )
            .await;

        assert!(matches!(
            result,
            Err(HydraCliError::NoEligibleCandidate { .. })
        ));
    }

    #[tokio::test]
    async fn test_router_executes_through_hydra_executor_seam() {
        let mut config = RoutingConfig::default();
        config.candidates.push(Candidate::new(
            "test".to_string(),
            "model".to_string(),
            "test".to_string(),
        ));
        config.profiles.insert(
            "test".to_string(),
            Profile {
                credentials: [("test".to_string(), "literal:secret".to_string())]
                    .into_iter()
                    .collect(),
                models: Vec::new(),
            },
        );
        let executor = Arc::new(TestExecutor {
            requests: Mutex::new(Vec::new()),
        });
        let mut router = PromptRouter::with_executor(
            config.clone(),
            RetryManager::new(config),
            executor.clone(),
        );

        router
            .execute(
                RoutingRequest::default(),
                "hello".to_string(),
                std::env::current_dir().unwrap(),
                TextOutputWrapper::new(|_| {}),
            )
            .await
            .unwrap();

        let requests = executor.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].provider.as_deref(), Some("test"));
        assert_eq!(requests[0].model.as_deref(), Some("model"));
        assert_eq!(requests[0].api_key.as_deref(), Some("secret"));
    }

    #[test]
    fn test_project_goal_registry_formats_invariants() {
        let registry = ProjectGoalRegistry::default();
        let header = registry.format_invariant_header();
        assert!(header.contains("ARCHITECTURAL INVARIANTS"));
        assert!(header.contains("INVARIANT: All external dependencies"));
        assert!(header.contains("FORBIDDEN: Ad-hoc *_runner.rs"));
    }

    #[test]
    fn test_prompt_prefix_aligner_order() {
        let aligned = PromptPrefixAligner::build_cache_aligned_prompt(
            "SYSTEM: coder",
            "INVARIANT: zero unsafe",
            &["| API | Desc |".to_string()],
            "pub struct Model;",
            "Implement feature X",
        );

        let system_pos = aligned.find("SYSTEM: coder").unwrap();
        let inv_pos = aligned.find("INVARIANT: zero unsafe").unwrap();
        let doc_pos = aligned.find("REPOSITORY CONTRACTS").unwrap();
        let code_pos = aligned.find("CODE CONTEXT SKELETON").unwrap();
        let intent_pos = aligned.find("TASK INTENT").unwrap();

        // Must follow static -> dynamic KV cache order
        assert!(system_pos < inv_pos);
        assert!(inv_pos < doc_pos);
        assert!(doc_pos < code_pos);
        assert!(code_pos < intent_pos);
    }
}
