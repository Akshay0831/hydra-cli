//! Provider adapter that bridges Hydra routing decisions with upstream provider execution.
use crate::routing::{Candidate, ResolvedCredential, RoutingConfig, RoutingRequest};
use anyhow::Result;
use std::collections::HashSet;

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

    pub fn provider(&self) -> &str {
        &self.candidate.provider
    }

    pub fn model(&self) -> &str {
        &self.candidate.model
    }

    pub fn api_key(&self) -> &str {
        &self.resolved_credential.value
    }
}

/// Factory for creating provider adapters from routing candidates.
#[derive(Debug, Clone)]
pub struct ProviderAdapterFactory {
    pub(crate) routing_config: RoutingConfig,
    upstream_providers: HashSet<String>,
}

impl ProviderAdapterFactory {
    pub fn new(routing_config: RoutingConfig) -> Self {
        Self {
            routing_config,
            upstream_providers: HashSet::new(),
        }
    }

    pub fn routing_provider_names(&self) -> Vec<String> {
        let mut providers: Vec<_> = self
            .routing_config
            .profiles
            .values()
            .flat_map(|profile| profile.credentials.keys().cloned())
            .collect();
        providers.sort_unstable();
        providers.dedup();
        providers
    }

    /// Register a provider configured by at least one routing profile.
    pub fn register_provider(&mut self, provider_name: String) -> Result<()> {
        if provider_name.trim().is_empty() {
            anyhow::bail!("provider name cannot be empty");
        }
        let configured = self
            .routing_config
            .profiles
            .values()
            .any(|profile| profile.credentials.contains_key(&provider_name));
        if !configured {
            anyhow::bail!("no profiles configured for provider: {}", provider_name);
        }

        self.upstream_providers.insert(provider_name);
        Ok(())
    }

    pub async fn create_adapter(
        &self,
        candidate: &Candidate,
    ) -> Result<Option<HydraProviderAdapter>> {
        // Resolve the credential for this candidate
        let resolved_credential = self.routing_config.resolve_credential(candidate)?;

        // Find the matching upstream provider
        if self.upstream_providers.contains(&candidate.provider) {
            let adapter = HydraProviderAdapter::new(candidate.clone(), resolved_credential);
            Ok(Some(adapter))
        } else {
            Ok(None)
        }
    }

    pub fn registered_provider_names(&self) -> Vec<String> {
        let mut providers: Vec<_> = self.upstream_providers.iter().cloned().collect();
        providers.sort_unstable();
        providers
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{Candidate, Profile, RoutingConfig};
    use std::collections::HashMap;

    fn factory_with_provider(provider: &str) -> ProviderAdapterFactory {
        let mut config = RoutingConfig::default();
        let mut credentials = HashMap::new();
        credentials.insert(provider.to_string(), "literal:test-credential".to_string());
        config.profiles.insert(
            "test-profile".to_string(),
            Profile {
                credentials,
                models: Vec::new(),
            },
        );
        ProviderAdapterFactory::new(config)
    }

    #[test]
    fn register_provider_accepts_configured_provider() {
        let mut factory = factory_with_provider("custom-provider");
        assert!(factory
            .register_provider("custom-provider".to_string())
            .is_ok());
        assert_eq!(factory.registered_provider_names(), vec!["custom-provider"]);
    }

    #[test]
    fn register_provider_rejects_unconfigured_provider() {
        let mut factory = ProviderAdapterFactory::new(RoutingConfig::default());
        assert!(factory.register_provider("unknown".to_string()).is_err());
    }

    #[test]
    fn register_provider_rejects_empty_values() {
        let mut factory = ProviderAdapterFactory::new(RoutingConfig::default());
        assert!(factory.register_provider("".to_string()).is_err());
    }

    #[test]
    fn registered_provider_names_reflect_registration() {
        let mut factory = factory_with_provider("openai");
        factory.register_provider("openai".to_string()).unwrap();
        assert_eq!(factory.registered_provider_names(), vec!["openai"]);
    }

    #[tokio::test]
    async fn create_adapter_returns_none_for_unregistered_provider() {
        let factory = factory_with_provider("unregistered");
        let candidate = Candidate::new(
            "unregistered".to_string(),
            "model".to_string(),
            "test-profile".to_string(),
        );
        assert!(factory.create_adapter(&candidate).await.unwrap().is_none());
    }
}
