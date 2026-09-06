//! Router for prompt execution with retry and failover.
//!
//! This service encapsulates all routing, provider selection, retry logic,
//! and progress feedback for prompt commands. It is the only point where
//! provider retry decisions are made.

use crate::agent_adapter::{AgentAdapter, PromptRequest};
use crate::error::{ErrorHandler, HydraCliError};
use crate::provider_adapter::ProviderAdapterFactory;
use crate::retry_manager::RetryManager;
use crate::routing::{RoutingConfig, RoutingRequest};
use anyhow::Result;
use std::sync::Arc;

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
                            provider: Some(adapter.provider().to_string()),
                            model: Some(adapter.model().to_string()),
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
}
