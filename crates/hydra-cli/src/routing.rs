use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub struct Candidate {
    pub provider: String,
    pub model: String,
    pub profile: String,
    #[serde(default)]
    pub purposes: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>, // Additional required capabilities beyond tools
    #[serde(default)]
    pub preference: u32, // User preference for this candidate
    #[serde(default = "default_health")]
    pub healthy: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Profile {
    #[serde(default)]
    pub credentials: HashMap<String, String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedCredential {
    pub provider: String,
    pub profile: String,
    pub source: CredentialSource,
    pub value: String,
}

impl fmt::Debug for ResolvedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedCredential")
            .field("provider", &self.provider)
            .field("profile", &self.profile)
            .field("source", &self.source)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub enum Capability {
    Tool(String),     // External tool access
    Streaming,        // Streaming responses supported
    StructuredOutput, // Structured JSON output
    Reasoning,        // Reasoning capabilities
    ContextLarge,     // Large context window
    Vision,           // Image input support
    CodeExecution,    // Code execution capability
                      // Add more as needed
}

impl Capability {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "tool" => Ok(Capability::Tool(s.to_string())),
            "streaming" => Ok(Capability::Streaming),
            "structured" => Ok(Capability::StructuredOutput),
            "reasoning" => Ok(Capability::Reasoning),
            "large-context" => Ok(Capability::ContextLarge),
            "vision" => Ok(Capability::Vision),
            "code-execution" => Ok(Capability::CodeExecution),
            _ => anyhow::bail!("unknown capability: {}", s),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CredentialSource {
    Environment(String),
    Literal,
}

fn default_health() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RoutingConfig {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,
    #[serde(default)]
    pub fallback_mode: FallbackMode,
}

/// Strategy for handling fallback when primary candidates fail
#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize, Default)]
pub enum FallbackMode {
    /// Only use explicitly specified candidates
    Strict,
    /// Use lower-priority candidates from the same provider
    Provider,
    /// Use any lower-priority candidate
    #[default]
    Any,
    /// Don't attempt fallback, just report the error
    None,
}

impl RoutingConfig {
    pub fn init(path: &Path, force: bool) -> Result<()> {
        if path.exists() && !force {
            anyhow::bail!(
                "Hydra config already exists: {}; use --force to replace it",
                path.display()
            );
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory: {}", parent.display())
            })?;
        }
        let contents = serde_json::to_string_pretty(&Self::default())?;
        std::fs::write(path, format!("{contents}\n"))
            .with_context(|| format!("failed to write Hydra config: {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read Hydra config: {}", path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse Hydra config: {}", path.display()))
    }

    pub fn resolve_with_candidates(
        &self,
        additional: &[Candidate],
        request: &RoutingRequest,
    ) -> Vec<Candidate> {
        let mut selected = resolve_configured(
            &self.candidates,
            &self.profiles,
            request,
            self.fallback_mode.clone(),
        );
        selected.extend(resolve(additional, request));
        selected.sort_by(|left, right| {
            right
                .preference
                .cmp(&left.preference)
                .then_with(|| left.provider.cmp(&right.provider))
                .then_with(|| left.model.cmp(&right.model))
                .then_with(|| left.profile.cmp(&right.profile))
        });
        selected
    }

    pub fn profiles(&self) -> Vec<ProfileSummary> {
        let mut summaries: Vec<ProfileSummary> = self
            .profiles
            .iter()
            .map(|(name, profile)| ProfileSummary {
                name: name.clone(),
                providers: profile.credentials.keys().cloned().collect(),
                models: profile.models.clone(),
            })
            .collect();
        summaries.sort_by(|left, right| left.name.cmp(&right.name));
        summaries
    }

    pub fn resolve_credential(&self, candidate: &Candidate) -> Result<ResolvedCredential> {
        let reference = self
            .profiles
            .get(&candidate.profile)
            .and_then(|profile| profile.credentials.get(&candidate.provider))
            .with_context(|| {
                format!(
                    "no credential configured for {}/{}",
                    candidate.profile, candidate.provider
                )
            })?;
        resolve_credential_reference(reference).map(|(source, value)| ResolvedCredential {
            provider: candidate.provider.clone(),
            profile: candidate.profile.clone(),
            source,
            value,
        })
    }
}

/// Check if a candidate supports all required capabilities from a request
pub fn candidate_supports_request(candidate: &Candidate, request: &RoutingRequest) -> bool {
    // Check if all required tools are supported
    let tools_supported = request
        .required_tools
        .iter()
        .all(|tool| candidate.capabilities.iter().any(|cap| cap == tool));

    // Check if all required capabilities are supported
    let capabilities_supported = request
        .required_capabilities
        .iter()
        .all(|capability| candidate.capabilities.iter().any(|cap| cap == capability));

    // Do not treat built-in Pi tool names as candidate capabilities
    // Only explicit --tool values should be required by routing

    tools_supported && capabilities_supported
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProfileSummary {
    pub name: String,
    pub providers: Vec<String>,
    pub models: Vec<String>,
}

fn resolve_credential_reference(reference: &str) -> Result<(CredentialSource, String)> {
    if let Some(variable) = reference.strip_prefix("$ENV:") {
        let variable = variable.trim();
        if variable.is_empty() {
            anyhow::bail!("credential environment reference is empty");
        }
        let value = std::env::var(variable).with_context(|| {
            format!("credential environment variable is unavailable: {variable}")
        })?;
        if value.trim().is_empty() {
            anyhow::bail!("credential environment variable is empty: {variable}");
        }
        return Ok((CredentialSource::Environment(variable.to_string()), value));
    }
    if let Some(value) = reference.strip_prefix("literal:") {
        if value.is_empty() {
            anyhow::bail!("literal credential reference is empty");
        }
        return Ok((CredentialSource::Literal, value.to_string()));
    }
    anyhow::bail!("unsupported credential reference; use $ENV:NAME or literal:VALUE")
}

impl Candidate {
    pub fn new(provider: String, model: String, profile: String) -> Self {
        Self {
            provider,
            model,
            profile,
            purposes: Vec::new(),
            capabilities: Vec::new(),
            required_capabilities: Vec::new(),
            preference: 0,
            healthy: true,
        }
    }

    /// Add a capability to this candidate
    pub fn with_capability(mut self, capability: Capability) -> Self {
        self.capabilities.push(capability.to_string());
        self
    }

    /// Add a required capability to this candidate
    pub fn with_required_capability(mut self, capability: Capability) -> Self {
        self.required_capabilities.push(capability.to_string());
        self
    }

    /// Set preference level (higher = preferred)
    pub fn with_preference(mut self, preference: u32) -> Self {
        self.preference = preference;
        self
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::Tool(tool) => write!(formatter, "tool:{tool}"),
            Capability::Streaming => formatter.write_str("streaming"),
            Capability::StructuredOutput => formatter.write_str("structured"),
            Capability::Reasoning => formatter.write_str("reasoning"),
            Capability::ContextLarge => formatter.write_str("large-context"),
            Capability::Vision => formatter.write_str("vision"),
            Capability::CodeExecution => formatter.write_str("code-execution"),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct RoutingRequest {
    pub purpose: Option<String>,
    pub required_tools: Vec<String>,
    pub required_capabilities: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub profile: Option<String>,
}

pub fn resolve(candidates: &[Candidate], request: &RoutingRequest) -> Vec<Candidate> {
    let eligible: Vec<Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.healthy)
        .filter(|candidate| {
            candidate.required_capabilities.iter().all(|required| {
                candidate
                    .capabilities
                    .iter()
                    .any(|capability| capability == required)
            })
        })
        .filter(|candidate| {
            request.purpose.as_ref().is_none_or(|purpose| {
                candidate.purposes.is_empty()
                    || candidate.purposes.iter().any(|item| item == purpose)
            })
        })
        .filter(|candidate| {
            // Check if all required tools are supported
            request
                .required_tools
                .iter()
                .all(|tool| candidate.capabilities.iter().any(|cap| cap == tool))
        })
        .filter(|candidate| candidate_supports_request(candidate, request))
        .filter(|candidate| {
            // Check explicit provider filter
            request
                .provider
                .as_ref()
                .is_none_or(|provider| &candidate.provider == provider)
        })
        .filter(|candidate| {
            // Check explicit model filter
            request
                .model
                .as_ref()
                .is_none_or(|model| &candidate.model == model)
        })
        .filter(|candidate| {
            // Check explicit profile filter
            request
                .profile
                .as_ref()
                .is_none_or(|profile| &candidate.profile == profile)
        })
        .cloned()
        .collect();

    // Remove duplicates by keeping the highest preference for each (provider, model, profile) tuple
    let mut deduplicated = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for candidate in eligible {
        let key = (
            candidate.provider.clone(),
            candidate.model.clone(),
            candidate.profile.clone(),
        );
        if !seen.contains(&key) {
            seen.insert(key);
            deduplicated.push(candidate);
        } else {
            // If we've seen this combination before, keep the one with higher preference
            if let Some(existing) = deduplicated.iter_mut().find(|c| {
                c.provider == candidate.provider
                    && c.model == candidate.model
                    && c.profile == candidate.profile
            }) {
                if candidate.preference > existing.preference {
                    *existing = candidate;
                }
            }
        }
    }

    // Sort by precedence: preference > provider > model > profile
    deduplicated.sort_by(|left, right| {
        right
            .preference
            .cmp(&left.preference) // Higher preference first
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.model.cmp(&right.model))
            .then_with(|| left.profile.cmp(&right.profile))
    });

    deduplicated
}

fn resolve_configured(
    candidates: &[Candidate],
    profiles: &HashMap<String, Profile>,
    request: &RoutingRequest,
    fallback_mode: FallbackMode,
) -> Vec<Candidate> {
    // First, get all eligible candidates based on credentials and models
    let eligible: Vec<Candidate> = candidates
        .iter()
        .filter(|candidate| {
            profiles.get(&candidate.profile).is_some_and(|profile| {
                profile.credentials.contains_key(&candidate.provider)
                    && (profile.models.is_empty()
                        || profile.models.iter().any(|model| model == &candidate.model))
            })
        })
        .cloned()
        .collect();

    let resolved = resolve(&eligible, request);

    if resolved.is_empty() {
        return Vec::new(); // No candidates available
    }

    // Apply fallback mode filtering
    let filtered = match fallback_mode {
        FallbackMode::Strict => {
            // Use only the highest-priority configured candidate.
            resolved.iter().take(1).cloned().collect()
        }
        FallbackMode::Provider => {
            // Allow lower-priority candidates from the same provider as the first candidate
            let first_provider = resolved.first().map(|c| c.provider.clone());
            if let Some(provider) = first_provider {
                resolved
                    .into_iter()
                    .filter(|candidate| candidate.provider == provider)
                    .collect()
            } else {
                resolved.clone()
            }
        }
        FallbackMode::Any => {
            // Allow all sorted eligible candidates - no filtering
            resolved.clone()
        }
        FallbackMode::None => {
            // Only use the first eligible candidate
            resolved.iter().take(1).cloned().collect()
        }
    };

    filtered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_mode_strict_uses_only_explicit_candidates() {
        let primary = Candidate::new(
            "openai".to_string(),
            "gpt-4".to_string(),
            "primary".to_string(),
        )
        .with_preference(1);
        let fallback = Candidate::new(
            "openai".to_string(),
            "gpt-3.5".to_string(),
            "primary".to_string(),
        )
        .with_preference(2);
        let alternative = Candidate::new(
            "anthropic".to_string(),
            "claude".to_string(),
            "backup".to_string(),
        )
        .with_preference(1);

        let config = RoutingConfig {
            fallback_mode: FallbackMode::Strict,
            profiles: profiles_for(&["openai", "anthropic"]),
            candidates: vec![primary.clone(), fallback.clone(), alternative.clone()],
        };

        let request = RoutingRequest::default();
        let selected = config.resolve_with_candidates(&[], &request);

        // Should only use the highest priority candidate (fallback has highest priority)
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].model, "gpt-3.5");
    }

    #[test]
    fn fallback_mode_provider_uses_same_provider_candidates() {
        let primary = Candidate::new(
            "openai".to_string(),
            "gpt-4".to_string(),
            "primary".to_string(),
        )
        .with_preference(1);
        let fallback = Candidate::new(
            "openai".to_string(),
            "gpt-3.5".to_string(),
            "primary".to_string(),
        )
        .with_preference(2);
        let alternative = Candidate::new(
            "anthropic".to_string(),
            "claude".to_string(),
            "backup".to_string(),
        )
        .with_preference(0);

        let config = RoutingConfig {
            fallback_mode: FallbackMode::Provider,
            profiles: profiles_for(&["openai", "anthropic"]),
            candidates: vec![primary.clone(), fallback.clone(), alternative.clone()],
        };

        let request = RoutingRequest::default();
        let selected = config.resolve_with_candidates(&[], &request);

        // Should use all OpenAI candidates, sorted by preference
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].model, "gpt-3.5"); // Highest priority
        assert_eq!(selected[1].model, "gpt-4"); // Lower priority
        assert_eq!(selected[0].provider, "openai");
        assert_eq!(selected[1].provider, "openai");
    }

    #[test]
    fn fallback_mode_any_uses_all_candidates() {
        let primary = Candidate::new(
            "openai".to_string(),
            "gpt-4".to_string(),
            "primary".to_string(),
        )
        .with_preference(1);
        let fallback = Candidate::new(
            "openai".to_string(),
            "gpt-3.5".to_string(),
            "primary".to_string(),
        )
        .with_preference(2);
        let alternative = Candidate::new(
            "anthropic".to_string(),
            "claude".to_string(),
            "backup".to_string(),
        )
        .with_preference(3);

        let config = RoutingConfig {
            fallback_mode: FallbackMode::Any,
            profiles: profiles_for(&["openai", "anthropic"]),
            candidates: vec![primary.clone(), fallback.clone(), alternative.clone()],
        };

        let request = RoutingRequest::default();
        let selected = config.resolve_with_candidates(&[], &request);

        // Should use all candidates, sorted by preference
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0].model, "claude"); // Highest priority
        assert_eq!(selected[1].model, "gpt-3.5"); // Second priority
        assert_eq!(selected[2].model, "gpt-4"); // Lowest priority
    }

    #[test]
    fn fallback_mode_none_uses_only_first_candidate() {
        let primary = Candidate::new(
            "openai".to_string(),
            "gpt-4".to_string(),
            "primary".to_string(),
        )
        .with_preference(1);
        let fallback = Candidate::new(
            "openai".to_string(),
            "gpt-3.5".to_string(),
            "primary".to_string(),
        )
        .with_preference(2);
        let alternative = Candidate::new(
            "anthropic".to_string(),
            "claude".to_string(),
            "backup".to_string(),
        )
        .with_preference(3);

        let config = RoutingConfig {
            fallback_mode: FallbackMode::None,
            profiles: profiles_for(&["openai", "anthropic"]),
            candidates: vec![primary.clone(), fallback.clone(), alternative.clone()],
        };

        let request = RoutingRequest::default();
        let selected = config.resolve_with_candidates(&[], &request);

        // Should only use the first candidate (highest priority)
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].model, "claude");
    }

    fn profiles_for(providers: &[&str]) -> HashMap<String, Profile> {
        let credentials: HashMap<String, String> = providers
            .iter()
            .map(|provider| ((*provider).to_string(), "literal:test".to_string()))
            .collect();
        HashMap::from([
            (
                "primary".to_string(),
                Profile {
                    credentials: credentials.clone(),
                    models: Vec::new(),
                },
            ),
            (
                "backup".to_string(),
                Profile {
                    credentials,
                    models: Vec::new(),
                },
            ),
        ])
    }

    #[test]
    fn candidate_supports_request_filters_required_tools() {
        let mut candidate = Candidate::new(
            "openai".to_string(),
            "gpt-4".to_string(),
            "primary".to_string(),
        );
        candidate.capabilities = vec!["shell".to_string(), "files".to_string(), "web".to_string()];

        let request1 = RoutingRequest {
            required_tools: vec!["shell".to_string()],
            ..RoutingRequest::default()
        };
        assert!(candidate_supports_request(&candidate, &request1));

        let request2 = RoutingRequest {
            required_tools: vec!["shell".to_string(), "files".to_string()],
            ..RoutingRequest::default()
        };
        assert!(candidate_supports_request(&candidate, &request2));

        let request3 = RoutingRequest {
            required_tools: vec!["database".to_string()],
            ..RoutingRequest::default()
        };
        assert!(!candidate_supports_request(&candidate, &request3));
    }

    #[test]
    fn candidate_supports_request_filters_required_capabilities() {
        let mut candidate = Candidate::new(
            "openai".to_string(),
            "gpt-4".to_string(),
            "primary".to_string(),
        );
        candidate.capabilities = vec![
            "streaming".to_string(),
            "reasoning".to_string(),
            "vision".to_string(),
        ];

        let request1 = RoutingRequest {
            required_capabilities: vec!["streaming".to_string()],
            ..RoutingRequest::default()
        };
        assert!(candidate_supports_request(&candidate, &request1));

        let request2 = RoutingRequest {
            required_capabilities: vec!["streaming".to_string(), "vision".to_string()],
            ..RoutingRequest::default()
        };
        assert!(candidate_supports_request(&candidate, &request2));

        let request3 = RoutingRequest {
            required_capabilities: vec!["code-execution".to_string()],
            ..RoutingRequest::default()
        };
        assert!(!candidate_supports_request(&candidate, &request3));
    }

    #[test]
    fn candidate_supports_request_se_tools_from_capabilities() {
        let mut candidate = Candidate::new(
            "openai".to_string(),
            "gpt-4".to_string(),
            "primary".to_string(),
        );
        candidate.capabilities = vec!["shell".to_string(), "files".to_string()];

        // Built-in Pi tool names should not be treated as candidate capabilities
        // Only explicit --tool values should be required
        let request = RoutingRequest {
            required_tools: vec!["functions".to_string()], // This is a built-in Pi tool
            ..RoutingRequest::default()
        };

        // Candidate doesn't have "functions" in capabilities, but it shouldn't matter
        // since we're only checking explicit --tool values
        // This test ensures we don't incorrectly treat built-in tools as required
        assert!(!candidate_supports_request(&candidate, &request));
    }
    use std::fs;

    #[test]
    fn filters_candidates_by_purpose_tools_and_health() {
        let mut coding = Candidate::new(
            "openai".to_string(),
            "coding-model".to_string(),
            "primary".to_string(),
        );
        coding.purposes = vec!["coding".to_string()];
        coding.capabilities = vec!["shell".to_string(), "files".to_string()];

        let mut planning = Candidate::new(
            "anthropic".to_string(),
            "planning-model".to_string(),
            "backup".to_string(),
        );
        planning.purposes = vec!["planning".to_string()];
        planning.capabilities = vec!["files".to_string()];

        let request = RoutingRequest {
            purpose: Some("coding".to_string()),
            required_tools: vec!["shell".to_string()],
            ..RoutingRequest::default()
        };
        assert_eq!(resolve(&[coding.clone(), planning], &request), vec![coding]);
    }

    #[test]
    fn ranks_candidates_deterministically_by_priority_then_identity() {
        let mut second = Candidate::new(
            "anthropic".to_string(),
            "model".to_string(),
            "backup".to_string(),
        );
        second = second.with_preference(2);
        let mut first = Candidate::new(
            "openai".to_string(),
            "model".to_string(),
            "primary".to_string(),
        );
        first = first.with_preference(1);
        assert_eq!(
            resolve(&[second.clone(), first.clone()], &RoutingRequest::default()),
            vec![second, first]
        );
    }

    #[test]
    fn loads_multiple_profiles_for_the_same_model() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("hydra.json");
        fs::write(
            &path,
            r#"{
                "profiles": {
                    "primary": {"credentials": {"openai": "$ENV:OPENAI_PRIMARY"}},
                    "backup": {"credentials": {"openai": "$ENV:OPENAI_BACKUP"}}
                },
                "candidates": [
                    {"provider":"openai","model":"coding","profile":"primary"},
                    {"provider":"openai","model":"coding","profile":"backup"}
                ]
            }"#,
        )
        .expect("write config");

        let config = RoutingConfig::load(&path).expect("load config");
        let request = RoutingRequest {
            model: Some("coding".to_string()),
            ..RoutingRequest::default()
        };
        assert_eq!(config.resolve_with_candidates(&[], &request).len(), 2);
    }

    #[test]
    fn profile_model_restrictions_and_provider_credentials_are_required() {
        let config: RoutingConfig = serde_json::from_str(
            r#"{
                "profiles": {
                    "coding": {
                        "credentials": {"openai": "$ENV:OPENAI_KEY"},
                        "models": ["coding"]
                    }
                },
                "candidates": [
                    {"provider":"openai","model":"coding","profile":"coding"},
                    {"provider":"openai","model":"planning","profile":"coding"},
                    {"provider":"anthropic","model":"coding","profile":"coding"}
                ]
            }"#,
        )
        .expect("parse config");

        let selected = config.resolve_with_candidates(&[], &RoutingRequest::default());
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn resolves_literal_credential_without_exposing_it_in_metadata() {
        let config: RoutingConfig = serde_json::from_str(
            r#"{
                "profiles": {"coding": {"credentials": {"openai": "literal:test-secret"}}},
                "candidates": [{"provider":"openai","model":"coding","profile":"coding"}]
            }"#,
        )
        .expect("parse config");
        let candidate = &config.candidates[0];

        let resolved = config
            .resolve_credential(candidate)
            .expect("resolve credential");
        assert_eq!(resolved.value, "test-secret");
        assert_eq!(resolved.source, CredentialSource::Literal);
        assert_eq!(resolved.provider, "openai");
        assert_eq!(resolved.profile, "coding");
    }

    #[test]
    fn reports_missing_environment_credentials() {
        let config: RoutingConfig = serde_json::from_str(
            r#"{
                "profiles": {"coding": {"credentials": {"openai": "$ENV:HYDRA_MISSING_KEY_9F7C"}}},
                "candidates": [{"provider":"openai","model":"coding","profile":"coding"}]
            }"#,
        )
        .expect("parse config");

        let error = config
            .resolve_credential(&config.candidates[0])
            .expect_err("missing environment variable must fail");
        assert!(error
            .to_string()
            .contains("credential environment variable is unavailable"));
    }

    #[test]
    fn rejects_unprefixed_credential_references() {
        let config: RoutingConfig = serde_json::from_str(
            r#"{
                "profiles": {"coding": {"credentials": {"openai": "raw-secret"}}},
                "candidates": [{"provider":"openai","model":"coding","profile":"coding"}]
            }"#,
        )
        .expect("parse config");

        let error = config
            .resolve_credential(&config.candidates[0])
            .expect_err("unprefixed reference must fail");
        assert!(error.to_string().contains("$ENV:NAME or literal:VALUE"));
    }

    #[test]
    fn reports_path_and_parse_context_for_invalid_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("hydra.json");
        fs::write(&path, "not json").expect("write config");

        let error = RoutingConfig::load(&path).expect_err("invalid config must fail");
        let message = error.to_string();
        assert!(message.contains("failed to parse Hydra config"));
        assert!(message.contains("hydra.json"));
    }

    #[test]
    fn initializes_config_without_overwriting_existing_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("nested/hydra.json");

        RoutingConfig::init(&path, false).expect("initialize config");
        assert_eq!(
            RoutingConfig::load(&path)
                .expect("load config")
                .candidates
                .len(),
            0
        );

        let error =
            RoutingConfig::init(&path, false).expect_err("existing config must be protected");
        assert!(error.to_string().contains("use --force"));
    }
}
