//! Provider adapter that bridges Hydra routing decisions with upstream provider execution.
use crate::routing::{Candidate, ResolvedCredential, RoutingConfig, RoutingRequest};
use anyhow::Result;
use std::collections::HashMap;

// Simplified provider adapter without complex upstream dependencies
#[derive(Debug, Clone)]
pub struct HydraProviderAdapter {
    candidate: Candidate,
    resolved_credential: ResolvedCredential,
}

impl HydraProviderAdapter {
    pub fn new(candidate: Candidate, resolved_credential: ResolvedCredential) -> Self {
        Self {
            candidate,
            resolved_credential,
        }
    }

    pub fn candidate(&self) -> &Candidate {
        &self.candidate
    }

    pub fn resolved_credential(&self) -> &ResolvedCredential {
        &self.resolved_credential
    }
}

/// Factory for creating provider adapters from routing candidates.
#[derive(Debug, Clone)]
pub struct ProviderAdapterFactory {
    routing_config: RoutingConfig,
    upstream_providers: HashMap<String, String>, // Simple provider name to type mapping
}

impl ProviderAdapterFactory {
    pub fn new(routing_config: RoutingConfig) -> Self {
        Self {
            routing_config,
            upstream_providers: HashMap::new(),
        }
    }

    pub fn register_provider(&mut self, provider_name: String, provider_type: String) {
        self.upstream_providers.insert(provider_name, provider_type);
    }

    pub async fn create_adapter(
        &self,
        candidate: &Candidate,
    ) -> Result<Option<HydraProviderAdapter>> {
        // Resolve the credential for this candidate
        let resolved_credential = self.routing_config.resolve_credential(candidate)?;

        // Find the matching upstream provider
        if self.upstream_providers.contains_key(&candidate.provider) {
            let adapter = HydraProviderAdapter::new(candidate.clone(), resolved_credential);
            Ok(Some(adapter))
        } else {
            Ok(None)
        }
    }

    pub async fn create_adapters_for_request(
        &self,
        request: &RoutingRequest,
        additional_candidates: &[Candidate],
    ) -> Result<Vec<HydraProviderAdapter>> {
        let candidates = self
            .routing_config
            .resolve_with_candidates(additional_candidates, request);
        let mut adapters = Vec::new();

        for candidate in candidates {
            if let Some(adapter) = self.create_adapter(&candidate).await? {
                adapters.push(adapter);
            }
        }

        Ok(adapters)
    }

    pub fn registered_providers(&self) -> Vec<String> {
        self.upstream_providers.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_adapter_factory_initializes_empty() {
        let routing_config = RoutingConfig::default();
        let factory = ProviderAdapterFactory::new(routing_config);
        assert!(factory.registered_providers().is_empty());
    }

    #[test]
    fn provider_adapter_factory_registers_providers() {
        let routing_config = RoutingConfig::default();
        let mut factory = ProviderAdapterFactory::new(routing_config);

        factory.register_provider("openai".to_string(), "openai-api".to_string());
        factory.register_provider("anthropic".to_string(), "anthropic-api".to_string());

        let mut providers = factory.registered_providers();
        providers.sort();
        assert_eq!(
            providers,
            vec!["anthropic".to_string(), "openai".to_string()]
        );
    }

    #[tokio::test]
    async fn provider_adapter_creation_fails_without_registered_provider() {
        let mut routing_config = RoutingConfig::default();

        // Add a profile with credentials for the unregistered provider
        use crate::routing::Profile;
        let mut credentials = std::collections::HashMap::new();
        credentials.insert(
            "unregistered".to_string(),
            "literal:test-credential".to_string(),
        );

        routing_config.profiles.insert(
            "test-profile".to_string(),
            Profile {
                credentials,
                models: Vec::new(),
            },
        );

        let factory = ProviderAdapterFactory::new(routing_config);

        let candidate = Candidate::new(
            "unregistered".to_string(),
            "model".to_string(),
            "test-profile".to_string(),
        );

        let result = factory.create_adapter(&candidate).await;
        match result {
            Ok(adapter) => {
                assert!(
                    adapter.is_none(),
                    "Expected None adapter for unregistered provider"
                );
            }
            Err(e) => {
                panic!("Expected Ok result but got error: {}", e);
            }
        }
    }
}
