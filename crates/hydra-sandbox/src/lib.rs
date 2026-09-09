use anyhow::Result;
use rquickjs::{Context, Runtime};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// JavaScript/TypeScript sandbox using rquickjs runtime.
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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// In-Memory Virtual File System (VFS) Diff Evaluation (Phase 7.2).
#[derive(Debug, Clone, Default)]
pub struct MemoryVfs {
    staged_files: HashMap<PathBuf, String>,
}

impl MemoryVfs {
    pub fn new() -> Self {
        Self {
            staged_files: HashMap::new(),
        }
    }

    /// Stages a file content update in memory.
    pub fn stage_file(&mut self, path: impl AsRef<Path>, content: impl Into<String>) {
        self.staged_files.insert(path.as_ref().to_path_buf(), content.into());
    }

    /// Reads staged file content from memory.
    pub fn get_file(&self, path: impl AsRef<Path>) -> Option<&str> {
        self.staged_files.get(path.as_ref()).map(|s| s.as_str())
    }

    /// Verifies delimiter syntax balance before touching physical disk.
    pub fn validate_staged_syntax(&self, path: impl AsRef<Path>) -> Result<(), String> {
        if let Some(content) = self.get_file(path.as_ref()) {
            let mut stack = Vec::new();
            for (idx, ch) in content.chars().enumerate() {
                match ch {
                    '{' | '(' | '[' => stack.push(ch),
                    '}' => {
                        if stack.pop() != Some('{') {
                            return Err(format!("Unmatched closing brace '}}' at char {}", idx));
                        }
                    }
                    ')' => {
                        if stack.pop() != Some('(') {
                            return Err(format!("Unmatched closing parenthesis ')' at char {}", idx));
                        }
                    }
                    ']' => {
                        if stack.pop() != Some('[') {
                            return Err(format!("Unmatched closing bracket ']' at char {}", idx));
                        }
                    }
                    _ => {}
                }
            }
            if !stack.is_empty() {
                return Err(format!("Unclosed delimiters: {:?}", stack));
            }
            Ok(())
        } else {
            Err("File not found in VFS".to_string())
        }
    }

    /// Commits all staged VFS files to physical disk.
    pub fn commit_to_disk(&self, base_path: impl AsRef<Path>) -> Result<usize> {
        let mut count = 0;
        for (rel_path, content) in &self.staged_files {
            let full_path = base_path.as_ref().join(rel_path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full_path, content)?;
            count += 1;
        }
        Ok(count)
    }

    pub fn len(&self) -> usize {
        self.staged_files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.staged_files.is_empty()
    }
}

/// Copy-on-Write (CoW) / Hard-Linked Worktrees (Phase 7.1).
pub struct CowWorktree;

impl CowWorktree {
    /// Creates a fast hardlink/CoW clone of a directory in under 50ms.
    pub fn spawn_clone(source: &Path, target: &Path) -> Result<usize> {
        if target.exists() {
            let _ = std::fs::remove_dir_all(target);
        }
        std::fs::create_dir_all(target)?;
        Self::copy_or_hardlink_dir(source, target)
    }

    fn copy_or_hardlink_dir(src: &Path, dst: &Path) -> Result<usize> {
        let mut count = 0;
        if !dst.exists() {
            std::fs::create_dir_all(dst)?;
        }
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name == "target" || name == ".git" || name == "node_modules" {
                continue;
            }
            let target_path = dst.join(&file_name);
            if path.is_dir() {
                count += Self::copy_or_hardlink_dir(&path, &target_path)?;
            } else if path.is_file() {
                if std::fs::hard_link(&path, &target_path).is_err() {
                    std::fs::copy(&path, &target_path)?;
                }
                count += 1;
            }
        }
        Ok(count)
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

    #[test]
    fn test_memory_vfs_lifecycle_and_validation() {
        let mut vfs = MemoryVfs::new();
        let rel_path = PathBuf::from("src/test.rs");

        // Staged valid content
        vfs.stage_file(&rel_path, "fn hello() { let x = (1 + 2); }");
        assert_eq!(vfs.len(), 1);
        assert!(vfs.validate_staged_syntax(&rel_path).is_ok());

        // Staged invalid content
        vfs.stage_file(&rel_path, "fn broken() { let x = (1 + 2; }");
        assert!(vfs.validate_staged_syntax(&rel_path).is_err());

        // Commit to disk
        let tmp = tempfile::tempdir().unwrap();
        vfs.stage_file(&rel_path, "fn ok() {}");
        let written = vfs.commit_to_disk(tmp.path()).unwrap();
        assert_eq!(written, 1);
        assert!(tmp.path().join("src/test.rs").exists());
    }

    #[test]
    fn test_cow_worktree_spawn() {
        let src_tmp = tempfile::tempdir().unwrap();
        let dst_tmp = tempfile::tempdir().unwrap();

        std::fs::write(src_tmp.path().join("Cargo.toml"), "[package]").unwrap();
        std::fs::create_dir_all(src_tmp.path().join("src")).unwrap();
        std::fs::write(src_tmp.path().join("src/lib.rs"), "pub fn run() {}").unwrap();

        let cloned = CowWorktree::spawn_clone(src_tmp.path(), dst_tmp.path()).unwrap();
        assert_eq!(cloned, 2);
        assert!(dst_tmp.path().join("Cargo.toml").exists());
        assert!(dst_tmp.path().join("src/lib.rs").exists());
    }
}
