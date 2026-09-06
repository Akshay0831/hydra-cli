//! Error types and retry handling used by Hydra CLI commands.

use crate::retry_manager::RetryConfig;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HydraCliError {
    #[error("Task execution failed: {0}")]
    TaskExecution(anyhow::Error),
    #[error("Indexing operation failed: {0}")]
    Indexing(anyhow::Error),
    #[error("JavaScript execution failed: {0}")]
    JavaScript(anyhow::Error),
    #[error("Configuration error: {0}")]
    Configuration(anyhow::Error),
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("No eligible provider candidate for {request}")]
    NoEligibleCandidate { request: String },
    #[error("Provider attempt failed for {candidate} (attempt {attempt}): {error}")]
    ProviderAttempt {
        candidate: String,
        attempt: u32,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct ErrorContext {
    timestamp: std::time::Instant,
}

impl ErrorContext {
    pub fn new(_operation: &str) -> Self {
        Self {
            timestamp: std::time::Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.timestamp.elapsed().as_millis() as u64
    }
}

pub struct ErrorHandler;

impl ErrorHandler {
    pub fn new(max_retries: u32, retry_config: RetryConfig) -> Self {
        let _ = (max_retries, retry_config);
        Self
    }

    pub fn is_retryable(&self, error: &HydraCliError) -> bool {
        match error {
            HydraCliError::TaskExecution(error) => {
                let message = error.to_string().to_ascii_lowercase();
                ["timeout", "temporary", "network", "rate limit"]
                    .iter()
                    .any(|marker| message.contains(marker))
            }
            HydraCliError::ProviderAttempt { error, .. } => {
                let message = error.to_ascii_lowercase();
                ["timeout", "temporary", "network", "rate limit", "429"]
                    .iter()
                    .any(|marker| message.contains(marker))
            }
            _ => false,
        }
    }
}

impl From<anyhow::Error> for HydraCliError {
    fn from(error: anyhow::Error) -> Self {
        Self::TaskExecution(error)
    }
}

impl From<std::io::Error> for HydraCliError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
