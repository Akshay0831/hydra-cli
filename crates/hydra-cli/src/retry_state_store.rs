//! Persistent retry state for provider candidates.
//!
//! This module provides atomic save/load of provider retry state across CLI
//! sessions, including proper error handling and secret safety.

use crate::retry_manager::ProviderState;
use crate::routing::Candidate;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryState {
    pub provider_states: HashMap<String, ProviderState>,
    pub last_updated: SystemTime,
}

impl RetryState {
    pub fn new() -> Self {
        Self {
            provider_states: HashMap::new(),
            last_updated: SystemTime::now(),
        }
    }

    pub fn get_provider_state(&mut self, candidate: &Candidate) -> &mut ProviderState {
        let key = format!(
            "{}/{}/{}",
            candidate.provider, candidate.model, candidate.profile
        );
        self.provider_states.entry(key).or_insert_with(|| {
            ProviderState::new(
                candidate.provider.clone(),
                candidate.model.clone(),
                candidate.profile.clone(),
            )
        })
    }

    pub fn reset_candidate(&mut self, candidate: &Candidate) {
        let key = format!(
            "{}/{}/{}",
            candidate.provider, candidate.model, candidate.profile
        );
        self.provider_states.remove(&key);
    }
}

pub struct RetryStateStore;

impl RetryStateStore {
    /// Load retry state from file, returning empty state if missing.
    pub fn load(path: &Path) -> Result<RetryState> {
        if !path.exists() {
            return Ok(RetryState::new());
        }

        let contents = fs::read_to_string(path)?;
        let state: RetryState = serde_json::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("invalid retry state: {}", e))?;

        Ok(state)
    }

    /// Save retry state atomically.
    pub fn save(path: &Path, state: &RetryState) -> Result<()> {
        let serialized = serde_json::to_string_pretty(state)
            .map_err(|e| anyhow::anyhow!("failed to serialize retry state: {}", e))?;

        // Write to temporary file, then rename for atomicity
        let temp_file = NamedTempFile::new_in(path.parent().unwrap_or_else(|| Path::new(".")))?;
        fs::write(temp_file.path(), serialized.as_bytes())?;
        temp_file.persist(path)?;

        Ok(())
    }

    /// Reset retry state for a specific candidate.
    pub fn reset_candidate(path: &Path, candidate: &Candidate) -> Result<()> {
        let mut state = Self::load(path)?;
        state.reset_candidate(candidate);
        Self::save(path, &state)
    }

    /// Reset all retry state.
    pub fn reset_all(path: &Path) -> Result<()> {
        let state = RetryState::new();
        Self::save(path, &state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_state_operations() {
        let mut state = RetryState::new();
        let candidate = Candidate::new(
            "openai".to_string(),
            "model".to_string(),
            "profile".to_string(),
        );

        // Test initial state
        let provider_state = state.get_provider_state(&candidate);
        assert_eq!(provider_state.consecutive_failures, 0);
        assert_eq!(provider_state.successful_calls, 0);

        // Test marking a failure
        provider_state.mark_failure("test error".to_string());
        assert_eq!(provider_state.consecutive_failures, 1);
        assert_eq!(provider_state.failed_calls, 1);

        // Test reset
        state.reset_candidate(&candidate);
        let provider_state = state.get_provider_state(&candidate);
        assert_eq!(provider_state.consecutive_failures, 0);
        assert_eq!(provider_state.failed_calls, 0);
    }

    #[test]
    fn test_state_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("retry_state.json");

        let candidate = Candidate::new(
            "openai".to_string(),
            "model".to_string(),
            "profile".to_string(),
        );

        // Create state and save
        let mut state = RetryState::new();
        state
            .get_provider_state(&candidate)
            .mark_failure("test error".to_string());
        RetryStateStore::save(&path, &state).unwrap();

        // Load and verify
        let loaded_state = RetryStateStore::load(&path).unwrap();
        assert_eq!(loaded_state.provider_states.len(), 1);
        let provider_state = loaded_state.provider_states.values().next().unwrap();
        assert_eq!(provider_state.consecutive_failures, 1);
        assert_eq!(provider_state.failed_calls, 1);
        assert_eq!(provider_state.last_error, Some("test error".to_string()));
    }

    #[test]
    fn test_reset_candidate() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("retry_state.json");

        let candidate = Candidate::new(
            "openai".to_string(),
            "model".to_string(),
            "profile".to_string(),
        );

        // Save state with failure
        let mut state = RetryState::new();
        state
            .get_provider_state(&candidate)
            .mark_failure("test error".to_string());
        RetryStateStore::save(&path, &state).unwrap();

        // Reset candidate
        RetryStateStore::reset_candidate(&path, &candidate).unwrap();

        // Verify it's gone
        let loaded_state = RetryStateStore::load(&path).unwrap();
        assert!(loaded_state.provider_states.is_empty());
    }
}
