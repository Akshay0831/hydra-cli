//! Hydra CLI error types and error handling utilities.
#![allow(dead_code)]

use crate::retry_manager::RetryConfig;
use anyhow::Result;
use thiserror::Error;

/// Main error type for the Hydra CLI.
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

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Retry error: {0}")]
    Retry(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Task graph contains cycles: {0}")]
    TaskGraphCycles(String),
}

/// Task execution specific errors.
#[derive(Debug, Error)]
pub enum TaskExecutionError {
    #[error("Task {id} failed to execute: {error}")]
    Task { id: String, error: String },

    #[error("Task {id} has unresolved dependencies: {dependencies:?}")]
    UnresolvedDependencies {
        id: String,
        dependencies: Vec<String>,
    },

    #[error("Task {id} execution timeout after {timeout_ms}ms")]
    Timeout { id: String, timeout_ms: u64 },

    #[error("Task graph contains cycles: {cycle_info}")]
    Cycle { cycle_info: String },
}

/// Indexing specific errors.
#[derive(Debug, Error)]
pub enum IndexingError {
    #[error("Failed to index file {path}: {error}")]
    File { path: String, error: String },

    #[error("Invalid file type {file_type} for indexing")]
    InvalidFileType { file_type: String },

    #[error("Index directory {path} not found")]
    DirectoryNotFound { path: String },

    #[error("Index exceeded size limit: {size} bytes (max: {max_size} bytes)")]
    SizeLimit { size: u64, max_size: u64 },
}

/// JavaScript execution specific errors.
#[derive(Debug, Error)]
pub enum JavaScriptError {
    #[error("JavaScript syntax error: {error}")]
    Syntax { error: String },

    #[error("JavaScript runtime error: {error}")]
    Runtime { error: String },

    #[error("JavaScript execution timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("JavaScript memory limit exceeded: {used_bytes} bytes (limit: {limit_bytes} bytes)")]
    MemoryLimit { used_bytes: u64, limit_bytes: u64 },
}

/// Configuration specific errors.
#[derive(Debug, Error)]
pub enum ConfigurationError {
    #[error("Configuration file {path} not found")]
    ConfigFileNotFound { path: String },

    #[error("Invalid configuration format: {error}")]
    InvalidFormat { error: String },

    #[error("Missing required configuration: {missing}")]
    Missing { missing: String },

    #[error("Invalid provider configuration for {provider}: {error}")]
    Provider { provider: String, error: String },

    #[error("Invalid retry configuration: {error}")]
    Retry { error: String },
}

/// Error context for better error reporting.
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub operation: String,
    pub details: Option<String>,
    pub timestamp: std::time::Instant,
    pub retry_count: u32,
}

impl ErrorContext {
    pub fn new(operation: &str) -> Self {
        Self {
            operation: operation.to_string(),
            details: None,
            timestamp: std::time::Instant::now(),
            retry_count: 0,
        }
    }

    pub fn with_details(mut self, details: String) -> Self {
        self.details = Some(details);
        self
    }

    pub fn with_retry_count(mut self, count: u32) -> Self {
        self.retry_count = count;
        self
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.timestamp.elapsed().as_millis() as u64
    }
}

/// Error handler for retryable operations.
pub struct ErrorHandler {
    max_retries: u32,
    backoff_config: RetryConfig,
}

impl ErrorHandler {
    pub fn new(max_retries: u32, backoff_config: RetryConfig) -> Self {
        Self {
            max_retries,
            backoff_config,
        }
    }

    /// Check if an error is retryable.
    pub fn is_retryable(&self, error: &HydraCliError) -> bool {
        match error {
            HydraCliError::TaskExecution(e) => {
                // Check if the error is network-related, temporary, or retryable
                e.to_string().contains("timeout")
                    || e.to_string().contains("network")
                    || e.to_string().contains("temporary")
            }
            HydraCliError::Indexing(e) => {
                // Retry IO-related indexing errors
                e.to_string().contains("IO") || e.to_string().contains("file")
            }
            HydraCliError::Provider(_) => true,
            HydraCliError::Retry(_) => true,
            _ => false,
        }
    }

    /// Handle a retryable error with backoff.
    pub async fn handle_retryable_error(&self, error: &HydraCliError) -> Result<()> {
        if !self.is_retryable(error) {
            return Err(anyhow::anyhow!("Non-retryable error: {}", error));
        }

        // Exponential backoff with jitter
        let delay = std::time::Duration::from_millis(1000); // Base delay
        tokio::time::sleep(delay).await;

        Ok(())
    }

    /// Execute a task with retry logic.
    pub async fn execute_with_retry<F, T>(
        &self,
        task_name: &str,
        mut operation: F,
    ) -> Result<T, HydraCliError>
    where
        F: FnMut() -> Result<T, HydraCliError>,
        F: std::marker::Send,
    {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            match operation() {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);

                    if attempt == self.max_retries {
                        break;
                    }

                    if !self.is_retryable(last_error.as_ref().unwrap()) {
                        break;
                    }

                    // Exponential backoff
                    let delay =
                        std::time::Duration::from_millis(1000 * 2_u64.pow(attempt));
                    tokio::time::sleep(delay).await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            HydraCliError::TaskExecution(anyhow::anyhow!(
                "Task {} failed after {} retries",
                task_name, self.max_retries
            ))
        }))
    }
}

impl From<anyhow::Error> for HydraCliError {
    fn from(err: anyhow::Error) -> Self {
        HydraCliError::TaskExecution(err)
    }
}

impl From<std::io::Error> for HydraCliError {
    fn from(err: std::io::Error) -> Self {
        HydraCliError::Io(err)
    }
}

impl From<IndexingError> for HydraCliError {
    fn from(err: IndexingError) -> Self {
        HydraCliError::Indexing(anyhow::anyhow!("{}", err))
    }
}

impl From<JavaScriptError> for HydraCliError {
    fn from(err: JavaScriptError) -> Self {
        HydraCliError::JavaScript(anyhow::anyhow!("{}", err))
    }
}

impl From<ConfigurationError> for HydraCliError {
    fn from(err: ConfigurationError) -> Self {
        HydraCliError::Configuration(anyhow::anyhow!("{}", err))
    }
}

impl From<TaskExecutionError> for HydraCliError {
    fn from(err: TaskExecutionError) -> Self {
        HydraCliError::TaskExecution(anyhow::anyhow!("{}", err))
    }
}
