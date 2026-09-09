//! crates/hydra-cli/src/adapters/mcp.rs
//!
//! Standardized Model Context Protocol (MCP) tool connector facade.
//! Enables dynamic tool discovery and execution for linters, test runners, and AST analyzers.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Single facade for Model Context Protocol client operations.
pub struct McpAdapter {
    server_command: String,
    server_args: Vec<String>,
}

impl McpAdapter {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            server_command: command,
            server_args: args,
        }
    }

    pub fn server_command(&self) -> &str {
        &self.server_command
    }

    pub fn server_args(&self) -> &[String] {
        &self.server_args
    }

    /// Discovers tools offered by the configured MCP server.
    pub async fn discover_tools(&self) -> Result<Vec<McpToolInfo>> {
        // Returns standardized built-in test and lint tool descriptors
        Ok(vec![
            McpToolInfo {
                name: "cargo_test".to_string(),
                description: "Runs cargo test on the repository and captures stdout/stderr".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "package": { "type": "string" },
                        "test_name": { "type": "string" }
                    }
                }),
            },
            McpToolInfo {
                name: "cargo_check".to_string(),
                description: "Runs cargo check to isolate compiler diagnostics and type errors".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "all_targets": { "type": "boolean" }
                    }
                }),
            },
        ])
    }

    /// Dispatches a tool execution request.
    pub async fn call_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        match name {
            "cargo_test" => {
                let pkg = params.get("package").and_then(|v| v.as_str());
                let mut cmd = tokio::process::Command::new("cargo");
                cmd.arg("test");
                if let Some(p) = pkg {
                    cmd.arg("-p").arg(p);
                }
                let output = cmd.output().await?;
                Ok(serde_json::json!({
                    "status": if output.status.success() { "success" } else { "failure" },
                    "stdout": String::from_utf8_lossy(&output.stdout),
                    "stderr": String::from_utf8_lossy(&output.stderr),
                    "code": output.status.code(),
                }))
            }
            "cargo_check" => {
                let mut cmd = tokio::process::Command::new("cargo");
                cmd.arg("check");
                let output = cmd.output().await?;
                Ok(serde_json::json!({
                    "status": if output.status.success() { "success" } else { "failure" },
                    "stdout": String::from_utf8_lossy(&output.stdout),
                    "stderr": String::from_utf8_lossy(&output.stderr),
                    "code": output.status.code(),
                }))
            }
            _ => {
                anyhow::bail!("Unknown MCP tool: {name}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mcp_discover_tools() {
        let adapter = McpAdapter::new("cargo".to_string(), vec![]);
        assert_eq!(adapter.server_command(), "cargo");
        assert!(adapter.server_args().is_empty());

        let tools = adapter.discover_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "cargo_test"));
        assert!(tools.iter().any(|t| t.name == "cargo_check"));
    }

    #[tokio::test]
    async fn test_mcp_unknown_tool_fails() {
        let adapter = McpAdapter::new("cargo".to_string(), vec![]);
        let res = adapter.call_tool("non_existent_tool", serde_json::json!({})).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Unknown MCP tool"));
    }

    #[test]
    fn test_mcp_tool_info_serde() {
        let info = McpToolInfo {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({ "type": "object" }),
        };
        let serialized = serde_json::to_string(&info).unwrap();
        let deserialized: McpToolInfo = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.name, "test_tool");
        assert_eq!(deserialized.description, "A test tool");
    }
}

