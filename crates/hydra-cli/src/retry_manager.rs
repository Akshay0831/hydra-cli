//! Retry/failover state management for provider adapters.
//!
//! This module implements bounded retry logic with exponential backoff for
//! transient failures, maintaining failover state between provider calls.

use crate::routing::{Candidate, RoutingConfig, RoutingRequest};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstantWrapper {
    unix_timestamp: u64,
}

impl From<Instant> for InstantWrapper {
    fn from(instant: Instant) -> Self {
        Self {
            unix_timestamp: instant.elapsed().as_secs(),
        }
    }
}

impl From<InstantWrapper> for Option<Instant> {
    fn from(wrapper: InstantWrapper) -> Self {
        // Note: This is a simplified conversion for demonstration
        // In a real implementation, you'd need to handle the actual timestamp
        None
    }
}
use std::time::{Duration, Instant};

/// Retry configuration for provider calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Initial backoff duration.
    pub initial_backoff: Duration,
    /// Maximum backoff duration.
    pub max_backoff: Duration,
    /// Backoff multiplier for exponential growth.
    pub backoff_multiplier: f64,
    /// Whether to enable jitter for backoff.
    pub enable_jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(1000),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            enable_jitter: true,
        }
    }
}

/// State for a specific provider candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderState {
    /// Name of the provider.
    pub provider: String,
    /// Model being used.
    pub model: String,
    /// Profile being used.
    pub profile: String,
    /// Number of consecutive failures.
    pub consecutive_failures: u32,
    /// Last failure timestamp.
    pub last_failure: Option<InstantWrapper>,
    /// Total number of successful calls.
    pub successful_calls: u64,
    /// Total number of failed calls.
    pub failed_calls: u64,
    /// Last error (if any).
    pub last_error: Option<String>,
}

impl ProviderState {
    pub fn new(provider: String, model: String, profile: String) -> Self {
        Self {
            provider,
            model,
            profile,
            consecutive_failures: 0,
            last_failure: None,
            successful_calls: 0,
            failed_calls: 0,
            last_error: None,
        }
    }

    /// Mark a successful call.
    pub fn mark_success(&mut self) {
        self.consecutive_failures = 0;
        self.successful_calls += 1;
        self.last_failure = None;
        self.last_error = None;
    }

    /// Mark a failed call.
    pub fn mark_failure(&mut self, error: String) {
        self.consecutive_failures += 1;
        self.last_failure = Some(Instant::now());
        self.failed_calls += 1;
        self.last_error = Some(error);
    }

    /// Check if this provider is currently healthy (not in cooldown).
    pub fn is_healthy(&self) -> bool {
        // If we haven't failed recently, we're healthy
        if self.consecutive_failures == 0 {
            return true;
        }

        // If the last failure was more than 5 minutes ago, consider healthy again
        if let Some(last_failure) = self.last_failure {
            if last_failure.elapsed() > Duration::from_secs(300) {
                return true;
            }
        }

        false
    }

    /// Get the backoff duration for this provider based on consecutive failures.
    pub fn get_backoff_duration(&self, retry_config: &RetryConfig) -> Duration {
        if self.consecutive_failures == 0 {
            return Duration::from_millis(0);
        }

        let mut backoff = retry_config.initial_backoff;
        for _ in 1..self.consecutive_failures {
            backoff = Duration::from_millis((backoff.as_millis() as f64 * retry_config.backoff_multiplier) as u64);
            if backoff > retry_config.max_backoff {
                backoff = retry_config.max_backoff;
                break;
            }
        }

        if retry_config.enable_jitter {
            // Add jitter (±25%)
            let jitter_factor = 0.75 + (rand::random::<f64>() * 0.5);
            backoff = Duration::from_millis((backoff.as_millis() as f64 * jitter_factor) as u64);
        }

        backoff
    }
}

/// Global retry/failover manager.
#[derive(Debug, Clone, Default)]
pub struct RetryManager {
    /// Retry configuration.
    pub retry_config: RetryConfig,
    /// State for each provider candidate.
    pub provider_states: HashMap<String, ProviderState>,
    /// Configuration for routing.
    pub routing_config: RoutingConfig,
}

impl RetryManager {
    /// Create a new retry manager.
    pub fn new(routing_config: RoutingConfig) -> Self {
        Self {
            routing_config,
            ..Default::default()
        }
    }

    /// Create a retry manager with custom configuration.
    pub fn with_config(routing_config: RoutingConfig, retry_config: RetryConfig) -> Self {
        Self {
            routing_config,
            retry_config,
            ..Default::default()
        }
    }

    /// Get or create state for a provider candidate.
    pub fn get_or_create_state(&mut self, candidate: &Candidate) -> String {
        let key = self.get_candidate_key(candidate);
        
        if !self.provider_states.contains_key(&key) {
            self.provider_states.insert(
                key.clone(),
                ProviderState::new(
                    candidate.provider.clone(),
                    candidate.model.clone(),
                    candidate.profile.clone(),
                ),
            );
        }

        key
    }

    /// Get the candidate state key.
    fn get_candidate_key(&self, candidate: &Candidate) -> String {
        format!("{}/{}/{}", candidate.provider, candidate.model, candidate.profile)
    }

    /// Check if a candidate should be retried based on its state.
    pub fn should_retry(&self, candidate: &Candidate) -> bool {
        let key = self.get_candidate_key(candidate);
        let state = match self.provider_states.get(&key) {
            Some(state) => state,
            None => return true, // No state, allow retry
        };

        // If we've exceeded max attempts, don't retry
        if state.consecutive_failures >= self.retry_config.max_attempts {
            return false;
        }

        // If the provider is in cooldown, don't retry
        !state.is_healthy()
    }

    /// Mark a successful call for a candidate.
    pub fn mark_success(&mut self, candidate: &Candidate) {
        let key = self.get_candidate_key(candidate);
        if let Some(state) = self.provider_states.get_mut(&key) {
            state.mark_success();
        }
    }

    /// Mark a failed call for a candidate.
    pub fn mark_failure(&mut self, candidate: &Candidate, error: String) {
        let key = self.get_candidate_key(candidate);
        if let Some(state) = self.provider_states.get_mut(&key) {
            state.mark_failure(error);
        }
    }

    /// Get the next backoff duration for a candidate.
    pub fn get_backoff_duration(&self, candidate: &Candidate) -> Duration {
        let key = self.get_candidate_key(candidate);
        match self.provider_states.get(&key) {
            Some(state) => state.get_backoff_duration(&self.retry_config),
            None => Duration::from_millis(0),
        }
    }

    /// Get candidates sorted by priority, excluding those that shouldn't be retried.
    pub fn get_healthy_candidates(&self, request: &RoutingRequest, additional_candidates: &[Candidate]) -> Vec<Candidate> {
        let all_candidates = self.routing_config.resolve_with_candidates(additional_candidates, request);
        
        all_candidates
            .into_iter()
            .filter(|candidate| self.should_retry(candidate))
            .collect()
    }

    /// Get status information for all providers.
    pub fn get_provider_status(&self) -> Vec<ProviderStatus> {
        self.provider_states
            .values()
            .cloned()
            .map(|state| ProviderStatus {
                provider: state.provider,
                model: state.model,
                profile: state.profile,
                consecutive_failures: state.consecutive_failures,
                last_failure: state.last_failure,
                successful_calls: state.successful_calls,
                failed_calls: state.failed_calls,
                last_error: state.last_error,
            })
            .collect()
    }

    /// Reset state for a specific provider.
    pub fn reset_provider(&mut self, provider: &str, model: &str, profile: &str) {
        let key = format!("{}/{}/{}", provider, model, profile);
        if let Some(state) = self.provider_states.get_mut(&key) {
            state.consecutive_failures = 0;
            state.last_failure = None;
            state.last_error = None;
        }
    }

    /// Reset all provider states.
    pub fn reset_all(&mut self) {
        for state in self.provider_states.values_mut() {
            state.consecutive_failures = 0;
            state.last_failure = None;
            state.last_error = None;
        }
    }
}

/// Status information for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider: String,
    pub model: String,
    pub profile: String,
    pub consecutive_failures: u32,
    pub last_failure: Option<Instant>,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub last_error: Option<String>,
}

/// Retry execution context for a specific operation.
pub struct RetryContext<'a> {
    /// The retry manager.
    pub manager: &'a mut RetryManager,
    /// The candidate being retried.
    pub candidate: Candidate,
    /// The current attempt number.
    pub attempt: u32,
    /// The backoff duration for this attempt.
    pub backoff: Duration,
}

impl<'a> RetryContext<'a> {
    /// Create a new retry context.
    pub fn new(manager: &'a mut RetryManager, candidate: Candidate, attempt: u32) -> Self {
        let backoff = manager.get_backoff_duration(&candidate);
        Self {
            manager,
            candidate,
            attempt,
            backoff,
        }
    }

    /// Mark the operation as successful.
    pub async fn mark_success(self) {
        self.manager.mark_success(&self.candidate);
    }

    /// Mark the operation as failed.
    pub async fn mark_failure(self, error: String) {
        self.manager.mark_failure(&self.candidate, error);
    }

    /// Get the current backoff duration.
    pub fn backoff_duration(&self) -> Duration {
        self.backoff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::Candidate;
    use std::time::Duration;

    #[test]
    fn retry_manager_initializes_with_empty_states() {
        let routing_config = RoutingConfig::default();
        let manager = RetryManager::new(routing_config);
        
        assert_eq!(manager.provider_states.len(), 0);
        assert!(manager.retry_config.max_attempts > 0);
    }

    #[test]
    fn provider_state_tracks_failures_success() {
        let mut state = ProviderState::new("openai".to_string(), "gpt-4".to_string(), "primary".to_string());
        
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.successful_calls, 0);
        assert_eq!(state.failed_calls, 0);
        
        state.mark_success();
        assert_eq!(state.successful_calls, 1);
        assert_eq!(state.consecutive_failures, 0);
        
        state.mark_failure("test error".to_string());
        assert_eq!(state.failed_calls, 1);
        assert_eq!(state.consecutive_failures, 1);
        assert_eq!(state.last_error, Some("test error".to_string()));
    }

    #[test]
    fn provider_state_health_check() {
        let mut state = ProviderState::new("openai".to_string(), "gpt-4".to_string(), "primary".to_string());
        
        // Initially healthy
        assert!(state.is_healthy());
        
        // After failure, still healthy (no cooldown yet)
        state.mark_failure("test error".to_string());
        assert!(state.is_healthy());
    }

    #[test]
    fn backoff_duration_increases_with_failures() {
        let state = ProviderState::new("openai".to_string(), "gpt-4".to_string(), "primary".to_string());
        let retry_config = RetryConfig::default();
        
        let backoff1 = state.get_backoff_duration(&retry_config);
        assert_eq!(backoff1, Duration::from_millis(0));
        
        // Create state with 1 failure
        let mut state_with_failure = ProviderState::new("openai".to_string(), "gpt-4".to_string(), "primary".to_string());
        state_with_failure.mark_failure("test".to_string());
        let backoff2 = state_with_failure.get_backoff_duration(&retry_config);
        assert!(backoff2 > Duration::from_millis(0));
        assert_eq!(backoff2, Duration::from_millis(1000));
        
        // State with 2 failures
        state_with_failure.mark_failure("test".to_string());
        let backoff3 = state_with_failure.get_backoff_duration(&retry_config);
        assert!(backoff3 > backoff2);
        assert_eq!(backoff3, Duration::from_millis(2000));
    }

    #[test]
    fn retry_manager_should_retry_healthy_providers() {
        let routing_config = RoutingConfig::default();
        let mut manager = RetryManager::new(routing_config);
        
        let candidate = Candidate::new("openai".to_string(), "gpt-4".to_string(), "primary".to_string());
        
        // Initially, should retry
        assert!(manager.should_retry(&candidate));
        
        // Mark failure, should still retry (not exceeded max attempts)
        manager.mark_failure(&candidate, "test error".to_string());
        assert!(manager.should_retry(&candidate));
        
        // Mark max failures, should not retry
        for _ in 0..manager.retry_config.max_attempts {
            manager.mark_failure(&candidate, "test error".to_string());
        }
        assert!(!manager.should_retry(&candidate));
    }

    #[test]
    fn retry_manager_manages_provider_states() {
        let routing_config = RoutingConfig::default();
        let mut manager = RetryManager::new(routing_config);
        
        let candidate = Candidate::new("openai".to_string(), "gpt-4".to_string(), "primary".to_string());
        
        // Get or create state
        let key = manager.get_or_create_state(&candidate);
        assert!(!key.is_empty());
        
        // Mark success
        manager.mark_success(&candidate);
        
        // Check status
        let statuses = manager.get_provider_status();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].successful_calls, 1);
        assert_eq!(statuses[0].failed_calls, 0);
    }

    #[test]
    fn retry_context_creation() {
        let routing_config = RoutingConfig::default();
        let mut manager = RetryManager::new(routing_config);
        let candidate = Candidate::new("openai".to_string(), "gpt-4".to_string(), "primary".to_string());
        
        let context = RetryContext::new(&mut manager, candidate, 1);
        assert_eq!(context.attempt, 1);
        assert!(context.backoff_duration().is_zero());
    }
}