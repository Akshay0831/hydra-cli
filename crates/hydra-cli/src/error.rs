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
    operation: String,
    timestamp: std::time::Instant,
    retry_count: u32,
}

impl ErrorContext {
    pub fn new(operation: &str) -> Self {
        Self {
            operation: operation.to_string(),
            timestamp: std::time::Instant::now(),
            retry_count: 0,
        }
    }

    pub fn with_retry_count(mut self, retry_count: u32) -> Self {
        self.retry_count = retry_count;
        self
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.timestamp.elapsed().as_millis() as u64
    }

    pub fn operation(&self) -> &str {
        &self.operation
    }

    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }
}

pub struct ErrorHandler {
    max_retries: u32,
    retry_config: RetryConfig,
}

impl ErrorHandler {
    pub fn new(max_retries: u32, retry_config: RetryConfig) -> Self {
        Self {
            max_retries,
            retry_config,
        }
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

    pub async fn execute_with_retry<F, T>(
        &self,
        operation: &str,
        mut operation_fn: F,
    ) -> Result<T, HydraCliError>
    where
        F: FnMut() -> Result<T, HydraCliError>,
    {
        let mut last_error = None;
        for attempt in 0..=self.max_retries {
            match operation_fn() {
                Ok(value) => return Ok(value),
                Err(error) if attempt < self.max_retries && self.is_retryable(&error) => {
                    let multiplier = self.retry_config.backoff_multiplier.powi(attempt as i32);
                    let delay = self
                        .retry_config
                        .initial_backoff
                        .mul_f64(multiplier)
                        .min(self.retry_config.max_backoff);
                    tokio::time::sleep(delay).await;
                    last_error = Some(error);
                }
                Err(error) => {
                    last_error = Some(error);
                    break;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            HydraCliError::TaskExecution(anyhow::anyhow!("{operation} failed without an error"))
        }))
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
