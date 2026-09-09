//! crates/hydra-cli/src/adapters/litellm.rs
//!
//! Process manager and lifecycle adapter for the local LiteLLM sidecar daemon.
//! Ensures automatic spawning, health-checking, and graceful termination.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

#[derive(Debug, Clone)]
pub struct LiteLLMConfig {
    pub host: String,
    pub port: u16,
    pub config_path: Option<PathBuf>,
    pub startup_timeout: Duration,
}

impl Default for LiteLLMConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 4000,
            config_path: None,
            startup_timeout: Duration::from_secs(10),
        }
    }
}

/// Single facade for managing the local LiteLLM sidecar daemon process.
pub struct LiteLLMManager {
    child: Option<Child>,
    config: LiteLLMConfig,
}

impl LiteLLMManager {
    /// Launches the LiteLLM sidecar process if not already running on the configured port.
    pub async fn spawn(config: LiteLLMConfig) -> Result<Self> {
        let mut manager = Self {
            child: None,
            config,
        };

        // If LiteLLM is already responding on the port, reuse it
        if manager.health_check().await.unwrap_or(false) {
            return Ok(manager);
        }

        let mut cmd = Command::new("litellm");
        cmd.arg("--host")
            .arg(&manager.config.host)
            .arg("--port")
            .arg(manager.config.port.to_string());

        if let Some(ref config_file) = manager.config.config_path {
            cmd.arg("--config").arg(config_file);
        }

        // Spawn detached process
        match cmd.spawn() {
            Ok(child) => {
                manager.child = Some(child);
                manager.wait_until_ready().await?;
                Ok(manager)
            }
            Err(e) => {
                // If litellm binary is not in PATH, log warning and return degraded manager
                anyhow::bail!(
                    "LiteLLM sidecar executable could not be spawned: {e}. Ensure litellm is installed."
                );
            }
        }
    }

    /// Health-checks the running daemon by sending an HTTP probe to /health.
    pub async fn health_check(&self) -> Result<bool> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let stream = match tokio::time::timeout(Duration::from_millis(800), TcpStream::connect(&addr)).await {
            Ok(Ok(stream)) => stream,
            _ => return Ok(false),
        };

        let (mut reader, mut writer) = tokio::io::split(stream);
        let request = format!(
            "GET /health HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
            self.config.host, self.config.port
        );

        if writer.write_all(request.as_bytes()).await.is_err() {
            return Ok(false);
        }

        let mut buf = [0u8; 128];
        match reader.read(&mut buf).await {
            Ok(n) if n > 0 => {
                let response = String::from_utf8_lossy(&buf[..n]);
                Ok(response.contains("200 OK") || response.contains("HTTP/1.1"))
            }
            _ => Ok(false),
        }
    }

    /// Retries health-check probe with exponential backoff until startup timeout.
    pub async fn wait_until_ready(&self) -> Result<()> {
        let start = std::time::Instant::now();
        let mut delay = Duration::from_millis(200);

        while start.elapsed() < self.config.startup_timeout {
            if self.health_check().await.unwrap_or(false) {
                return Ok(());
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_millis(1500));
        }

        anyhow::bail!(
            "LiteLLM sidecar failed to become healthy at http://{}:{} within {:?}",
            self.config.host,
            self.config.port,
            self.config.startup_timeout
        );
    }

    /// Returns the base URL for routing LLM requests.
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}", self.config.host, self.config.port)
    }

    /// Gracefully terminates the sidecar daemon.
    pub async fn terminate(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await.context("Failed to kill LiteLLM process");
        }
        Ok(())
    }
}

impl Drop for LiteLLMManager {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_litellm_config_defaults() {
        let config = LiteLLMConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 4000);
        assert!(config.config_path.is_none());
        assert_eq!(config.startup_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_litellm_endpoint_format() {
        let config = LiteLLMConfig {
            host: "localhost".to_string(),
            port: 8080,
            config_path: None,
            startup_timeout: Duration::from_secs(5),
        };
        let manager = LiteLLMManager {
            child: None,
            config,
        };
        assert_eq!(manager.endpoint(), "http://localhost:8080");
    }

    #[tokio::test]
    async fn test_health_check_offline_returns_false() {
        // Connect to a port unlikely to be open
        let config = LiteLLMConfig {
            host: "127.0.0.1".to_string(),
            port: 49151,
            config_path: None,
            startup_timeout: Duration::from_millis(500),
        };
        let manager = LiteLLMManager {
            child: None,
            config,
        };
        let is_healthy = manager.health_check().await.unwrap();
        assert!(!is_healthy);
    }

    #[tokio::test]
    async fn test_health_check_online_returns_true() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn a mock server responding with HTTP 200 OK
        let server_task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let config = LiteLLMConfig {
            host: "127.0.0.1".to_string(),
            port,
            config_path: None,
            startup_timeout: Duration::from_secs(2),
        };
        let manager = LiteLLMManager {
            child: None,
            config,
        };

        let is_healthy = manager.health_check().await.unwrap();
        assert!(is_healthy);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_terminate_without_child_returns_ok() {
        let mut manager = LiteLLMManager {
            child: None,
            config: LiteLLMConfig::default(),
        };
        assert!(manager.terminate().await.is_ok());
    }

    #[tokio::test]
    async fn test_wait_until_ready_timeout_fails_gracefully() {
        let config = LiteLLMConfig {
            host: "127.0.0.1".to_string(),
            port: 49150,
            config_path: None,
            startup_timeout: Duration::from_millis(400),
        };
        let manager = LiteLLMManager {
            child: None,
            config,
        };
        let res = manager.wait_until_ready().await;
        assert!(res.is_err());
    }
}

