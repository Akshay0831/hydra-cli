//! MCP tool connector facade for dynamic tool discovery and execution.

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

    /// Discover tools offered by MCP server.
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
            McpToolInfo {
                name: "hydra_code_scout".to_string(),
                description: "Searches AST symbols, callers, and definitions across the workspace".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    },
                    "required": ["query"]
                }),
            },
            McpToolInfo {
                name: "hydra_doc_search".to_string(),
                description: "Retrieves architectural invariants and tables from hierarchical doc tree".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    },
                    "required": ["query"]
                }),
            },
            McpToolInfo {
                name: "hydra_git_blame".to_string(),
                description: "Queries git commit history and author intent for legacy modules".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string" },
                        "limit": { "type": "integer" }
                    },
                    "required": ["file_path"]
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
            "hydra_code_scout" => {
                let query = params.get("query").and_then(|v| v.as_str()).unwrap_or_default();
                let mut matrix = hydra_matrix::CodeMatrix::new()?;
                matrix.config.paths = vec!["./**/*.rs".to_string()];
                let _ = matrix.index().await;
                let results = matrix.search(query).await?;
                let symbols: Vec<serde_json::Value> = results
                    .into_iter()
                    .take(20)
                    .map(|el| {
                        serde_json::json!({
                            "name": el.name,
                            "file": el.file_path.to_string_lossy(),
                            "line": el.line_number,
                            "type": format!("{:?}", el.element_type),
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "results": symbols, "query": query }))
            }
            "hydra_doc_search" => {
                let query = params.get("query").and_then(|v| v.as_str()).unwrap_or_default().to_lowercase();
                let mut matching_sections = Vec::new();
                for entry in walkdir_matching_docs(".") {
                    if let Ok(content) = tokio::fs::read_to_string(&entry).await {
                        for line in content.lines() {
                            if line.to_lowercase().contains(&query) && (line.starts_with('|') || line.starts_with('-')) {
                                matching_sections.push(line.to_string());
                            }
                        }
                    }
                }
                Ok(serde_json::json!({
                    "query": query,
                    "invariants": matching_sections.into_iter().take(15).collect::<Vec<_>>()
                }))
            }
            "hydra_git_blame" => {
                let file = params.get("file_path").and_then(|v| v.as_str()).unwrap_or_default();
                let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5);
                let out = tokio::process::Command::new("git")
                    .args(["log", &format!("-n{limit}"), "--oneline", "--", file])
                    .output()
                    .await;
                match out {
                    Ok(res) => Ok(serde_json::json!({
                        "file": file,
                        "history": String::from_utf8_lossy(&res.stdout).lines().map(|s| s.to_string()).collect::<Vec<_>>()
                    })),
                    Err(e) => Ok(serde_json::json!({
                        "file": file,
                        "error": e.to_string()
                    })),
                }
            }
            _ => {
                anyhow::bail!("Unknown MCP tool: {name}");
            }
        }
    }
}

fn walkdir_matching_docs(root: &str) -> Vec<std::path::PathBuf> {
    let mut docs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                docs.push(path);
            } else if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name != "target" && name != ".git" && name != "node_modules" {
                    docs.extend(walkdir_matching_docs(&path.to_string_lossy()));
                }
            }
        }
    }
    docs
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
        assert_eq!(tools.len(), 5);
        assert!(tools.iter().any(|t| t.name == "cargo_test"));
        assert!(tools.iter().any(|t| t.name == "cargo_check"));
        assert!(tools.iter().any(|t| t.name == "hydra_code_scout"));
        assert!(tools.iter().any(|t| t.name == "hydra_doc_search"));
        assert!(tools.iter().any(|t| t.name == "hydra_git_blame"));
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

    #[tokio::test]
    async fn test_mcp_call_hydra_doc_search() {
        let adapter = McpAdapter::new("cargo".to_string(), vec![]);
        let res = adapter.call_tool("hydra_doc_search", serde_json::json!({ "query": "hydra" })).await;
        assert!(res.is_ok());
        let val = res.unwrap();
        assert_eq!(val.get("query").unwrap(), "hydra");
        assert!(val.get("invariants").unwrap().is_array());
    }

    #[tokio::test]
    async fn test_mcp_call_hydra_git_blame() {
        let adapter = McpAdapter::new("cargo".to_string(), vec![]);
        let res = adapter.call_tool("hydra_git_blame", serde_json::json!({ "file_path": "README.md", "limit": 2 })).await;
        assert!(res.is_ok());
        let val = res.unwrap();
        assert_eq!(val.get("file").unwrap(), "README.md");
    }
}

