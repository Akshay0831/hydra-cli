use anyhow::Result;
use rquickjs::{Context, Runtime};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// A JavaScript/TypeScript execution sandbox using rquickjs
///
/// This provides basic JavaScript execution capabilities with
/// a real rquickjs runtime.
pub struct Sandbox {
    _runtime: Runtime,
    context: Context,
    config: SandboxConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Maximum memory usage in bytes
    pub max_memory: Option<usize>,
    /// Timeout for execution in milliseconds
    pub timeout_ms: Option<u64>,
    /// Whether to enable console logging
    pub enable_console: bool,
    /// Whether to enable module loading
    pub enable_modules: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory: Some(100 * 1024 * 1024), // 100MB default
            timeout_ms: Some(30000),             // 30 seconds default
            enable_console: true,
            enable_modules: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    /// The execution result
    pub result: String,
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
    /// Memory usage in bytes
    pub memory_usage_bytes: Option<usize>,
    /// Any errors that occurred
    pub errors: Vec<String>,
    pub error_kind: Option<SandboxErrorKind>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum SandboxErrorKind {
    Syntax,
    Runtime,
    Timeout,
    MemoryLimit,
    Policy,
}

impl Sandbox {
    /// Create a new JavaScript sandbox with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(SandboxConfig::default())
    }

    /// Create a new JavaScript sandbox with custom configuration
    pub fn with_config(config: SandboxConfig) -> Result<Self> {
        let runtime = Runtime::new()?;
        if let Some(max_memory) = config.max_memory {
            runtime.set_memory_limit(max_memory);
        }
        let context = Context::full(&runtime)?;

        Ok(Self {
            _runtime: runtime,
            context,
            config,
        })
    }

    /// Execute JavaScript code in the sandbox
    pub async fn execute(&mut self, code: &str) -> Result<SandboxResult> {
        let start_time = Instant::now();
        let mut errors = Vec::new();

        if !self.config.enable_console && code.contains("console.") {
            return Ok(self.error_result(
                "console access is disabled",
                SandboxErrorKind::Policy,
                start_time,
            ));
        }
        if !self.config.enable_modules && (code.contains("import ") || code.contains("require(")) {
            return Ok(self.error_result(
                "module loading is disabled",
                SandboxErrorKind::Policy,
                start_time,
            ));
        }

        match self.execute_sync(code) {
            Ok(result) => {
                let execution_time = start_time.elapsed();
                Ok(SandboxResult {
                    result,
                    execution_time_ms: execution_time.as_millis() as u64,
                    memory_usage_bytes: None,
                    errors,
                    error_kind: None,
                })
            }
            Err(e) => {
                let message = e.to_string();
                errors.push(message.clone());
                let execution_time = start_time.elapsed();
                Ok(SandboxResult {
                    result: format!("Error: {message}"),
                    execution_time_ms: execution_time.as_millis() as u64,
                    memory_usage_bytes: None,
                    errors,
                    error_kind: Some(classify_error(&message)),
                })
            }
        }
    }

    fn error_result(
        &self,
        message: &str,
        kind: SandboxErrorKind,
        start_time: Instant,
    ) -> SandboxResult {
        SandboxResult {
            result: format!("Error: {message}"),
            execution_time_ms: start_time.elapsed().as_millis() as u64,
            memory_usage_bytes: None,
            errors: vec![message.to_string()],
            error_kind: Some(kind),
        }
    }

    /// Execute JavaScript code synchronously using rquickjs
    fn execute_sync(&mut self, code: &str) -> Result<String> {
        let deadline = self
            .config
            .timeout_ms
            .map(|timeout| Instant::now() + std::time::Duration::from_millis(timeout));
        if let Some(deadline) = deadline {
            self._runtime
                .set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        }
        let result = self.context.with(|ctx| {
            ctx.eval::<String, _>(format!("String(({code}))"))
                .map_err(|e| anyhow::anyhow!("JS execution error: {}", e))
        });
        self._runtime.set_interrupt_handler(None);
        result
    }

    /// Evaluate JavaScript code and get the result as a string
    pub async fn eval(&mut self, code: &str) -> Result<String> {
        let result = self.execute(code).await?;
        Ok(result.result)
    }

    /// Get information about the sandbox
    pub fn info(&self) -> SandboxInfo {
        SandboxInfo {
            engine_id: uuid::Uuid::new_v4().to_string(),
            runtime_config: self.config.clone(),
        }
    }

    /// Cleanup resources
    pub async fn cleanup(&mut self) -> Result<()> {
        self._runtime.set_interrupt_handler(None);
        Ok(())
    }
}

/// Information about the sandbox instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub engine_id: String,
    pub runtime_config: SandboxConfig,
}

fn classify_error(message: &str) -> SandboxErrorKind {
    let lower = message.to_ascii_lowercase();
    if lower.contains("interrupt") || lower.contains("timeout") {
        SandboxErrorKind::Timeout
    } else if lower.contains("memory") || lower.contains("out of memory") {
        SandboxErrorKind::MemoryLimit
    } else if lower.contains("syntax") || lower.contains("unexpected token") {
        SandboxErrorKind::Syntax
    } else {
        SandboxErrorKind::Runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sandbox_creation() {
        let sandbox = Sandbox::new().unwrap();
        let info = sandbox.info();
        assert!(!info.engine_id.is_empty());
    }

    #[tokio::test]
    async fn test_simple_execution() {
        let mut sandbox = Sandbox::new().unwrap();
        let result = sandbox.execute("2 + 2").await.unwrap();
        assert_eq!(result.result, "4");
    }

    #[tokio::test]
    async fn test_string_execution() {
        let mut sandbox = Sandbox::new().unwrap();
        let result = sandbox.execute("'hello' + ' world'").await.unwrap();
        assert_eq!(result.result, "hello world");
    }

    #[tokio::test]
    async fn test_error_handling() {
        let mut sandbox = Sandbox::new().unwrap();
        let result = sandbox
            .execute("throw new Error('Test error')")
            .await
            .unwrap();
        assert!(!result.errors.is_empty());
        assert_eq!(result.error_kind, Some(SandboxErrorKind::Runtime));
    }
}
