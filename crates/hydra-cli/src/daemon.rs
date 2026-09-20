//! Headless daemon with JSON-RPC 2.0 streaming interface and instant cancellation.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock, watch};

use crate::orchestrator::steering::{DecisionBrief, DecisionOption, DecisionSeam};

/// Cooperative cancellation token with instant cancellation
#[derive(Clone, Debug)]
pub struct CancellationToken {
    sender: Arc<watch::Sender<bool>>,
    receiver: watch::Receiver<bool>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    /// Create root cancellation token
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self {
            sender: Arc::new(sender),
            receiver,
        }
    }

    /// Spawn child token linked to parent
    pub fn child_token(&self) -> Self {
        let (child_tx, child_rx) = watch::channel(self.is_cancelled());
        let mut parent_rx = self.receiver.clone();
        let child_tx = Arc::new(child_tx);
        let child_tx_clone = child_tx.clone();

        tokio::spawn(async move {
            while parent_rx.changed().await.is_ok() {
                if *parent_rx.borrow() {
                    let _ = child_tx_clone.send(true);
                    break;
                }
            }
        });

        Self {
            sender: child_tx,
            receiver: child_rx,
        }
    }

    /// Trigger cooperative preemption
    pub fn cancel(&self) {
        let _ = self.sender.send(true);
    }

    /// Check if cancelled
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// Wait until cancelled
    pub async fn cancelled(&mut self) {
        if self.is_cancelled() {
            return;
        }
        while self.receiver.changed().await.is_ok() {
            if *self.receiver.borrow() {
                break;
            }
        }
    }
}

/// JSON-RPC 2.0 Request (Phase 5.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 Response (Phase 5.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Streaming Event Notification (Phase 5.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
}

impl JsonRpcNotification {
    pub fn new(method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        }
    }

    pub fn stream_delta(partition_id: &str, role: &str, delta: &str) -> Self {
        Self::new(
            "stream.delta",
            serde_json::json!({
                "partition_id": partition_id,
                "role": role,
                "delta": delta
            }),
        )
    }

    pub fn partition_status(partition_id: &str, status: &str) -> Self {
        Self::new(
            "partition.status",
            serde_json::json!({
                "partition_id": partition_id,
                "status": status
            }),
        )
    }

    pub fn decision_required(brief: &DecisionBrief) -> Self {
        Self::new(
            "decision.required",
            serde_json::to_value(brief).unwrap_or_default(),
        )
    }

    pub fn diff_preview(partition_id: &str, diff: &str) -> Self {
        Self::new(
            "diff.preview",
            serde_json::json!({
                "partition_id": partition_id,
                "diff": diff
            }),
        )
    }
}

/// Headless Daemon State (Phase 5.1 & 5.2).
pub struct DaemonState {
    pub active_tokens: Mutex<HashMap<String, CancellationToken>>,
    pub pending_decisions: RwLock<HashMap<String, DecisionBrief>>,
    pub resolved_decisions: RwLock<HashMap<String, DecisionOption>>,
    pub diff_cache: RwLock<HashMap<String, String>>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonState {
    pub fn new() -> Self {
        Self {
            active_tokens: Mutex::new(HashMap::new()),
            pending_decisions: RwLock::new(HashMap::new()),
            resolved_decisions: RwLock::new(HashMap::new()),
            diff_cache: RwLock::new(HashMap::new()),
        }
    }

    pub async fn cancel_task(&self, task_id: &str) -> bool {
        let tokens = self.active_tokens.lock().await;
        if let Some(token) = tokens.get(task_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn register_task(&self, task_id: String, token: CancellationToken) {
        let mut tokens = self.active_tokens.lock().await;
        tokens.insert(task_id, token);
    }
}

/// Dispatches JSON-RPC requests to internal daemon handlers.
pub async fn dispatch_json_rpc(state: &Arc<DaemonState>, req: JsonRpcRequest) -> JsonRpcResponse {
    let id = req.id.clone();
    match req.method.as_str() {
        "swarm.start" => {
            let task_id = req
                .params
                .get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("default_task")
                .to_string();
            let token = CancellationToken::new();
            state.register_task(task_id.clone(), token.clone()).await;

            let intent = req
                .params
                .get("intent")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let root = req
                .params
                .get("root")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            let concurrency = req
                .params
                .get("concurrency")
                .and_then(|v| v.as_u64())
                .unwrap_or(4) as usize;
            let coder_model = req
                .params
                .get("coder_model")
                .and_then(|v| v.as_str())
                .unwrap_or("gemini-2.5-pro")
                .to_string();
            let reviewer_model = req
                .params
                .get("reviewer_model")
                .and_then(|v| v.as_str())
                .unwrap_or("claude-3-5-sonnet")
                .to_string();

            if let Some(task_intent) = intent {
                let state_clone = state.clone();
                tokio::spawn(async move {
                    let root_path = std::path::PathBuf::from(&root);
                    if let Ok(splitter) =
                        crate::partitioner::AstSplitter::from_workspace(&root_path).await
                    {
                        let target_files = vec![root_path.join("src/lib.rs")];
                        if let Ok(partitions) = splitter
                            .partition_workspace(&target_files, concurrency)
                            .await
                        {
                            let config = crate::orchestrator::SwarmConfig {
                                max_concurrency: concurrency,
                                coder_model,
                                reviewer_model,
                            };
                            let orchestrator =
                                crate::orchestrator::SwarmOrchestrator::new(config, None);
                            let (tx, mut rx) = tokio::sync::mpsc::channel(64);

                            let state_events = state_clone.clone();
                            tokio::spawn(async move {
                                while let Some(event) = rx.recv().await {
                                    if let crate::orchestrator::SwarmEvent::DiffReady {
                                        partition_id,
                                        diff,
                                    } = event
                                    {
                                        let mut cache = state_events.diff_cache.write().await;
                                        cache.insert(partition_id, diff);
                                    }
                                }
                            });

                            let _ = orchestrator
                                .run_swarm(&root_path, &task_intent, partitions, tx)
                                .await;
                        }
                    }
                });
            }

            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::json!({
                    "status": "started",
                    "task_id": task_id
                })),
                error: None,
            }
        }
        "swarm.cancel" => {
            let task_id = req
                .params
                .get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("default_task");
            let cancelled = state.cancel_task(task_id).await;
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::json!({
                    "status": if cancelled { "cancelled" } else { "not_found" },
                    "task_id": task_id
                })),
                error: None,
            }
        }
        "decision.respond" => {
            let dec_id = req
                .params
                .get("decision_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let choice_id = req
                .params
                .get("option_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            let pending = state.pending_decisions.read().await;
            if let Some(brief) = pending.get(dec_id) {
                match DecisionSeam::resolve(brief, choice_id) {
                    Ok(opt) => {
                        drop(pending);
                        let mut resolved = state.resolved_decisions.write().await;
                        resolved.insert(dec_id.to_string(), opt.clone());
                        JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: Some(serde_json::to_value(opt).unwrap_or_default()),
                            error: None,
                        }
                    }
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: e.to_string(),
                            data: None,
                        }),
                    },
                }
            } else {
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Decision '{}' not found", dec_id),
                        data: None,
                    }),
                }
            }
        }
        "diff.preview" => {
            let part_id = req
                .params
                .get("partition_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let diffs = state.diff_cache.read().await;
            let diff = diffs.get(part_id).cloned().unwrap_or_default();
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::json!({
                    "partition_id": part_id,
                    "diff": diff
                })),
                error: None,
            }
        }
        "patch.get" => {
            let part_id = req
                .params
                .get("partition_id")
                .and_then(|v| v.as_str());
            let diffs = state.diff_cache.read().await;
            let patch = if let Some(pid) = part_id {
                diffs.get(pid).cloned().unwrap_or_default()
            } else {
                let all: Vec<String> = diffs.values().cloned().collect();
                crate::consolidator::Consolidator::reconcile_diffs(&all).unwrap_or_default()
            };
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::json!({
                    "patch": patch
                })),
                error: None,
            }
        }
        "patch.apply" => {
            let root_str = req
                .params
                .get("root")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let part_id = req
                .params
                .get("partition_id")
                .and_then(|v| v.as_str());
            let diffs = state.diff_cache.read().await;
            let patch = if let Some(pid) = part_id {
                diffs.get(pid).cloned().unwrap_or_default()
            } else {
                let all: Vec<String> = diffs.values().cloned().collect();
                crate::consolidator::Consolidator::reconcile_diffs(&all).unwrap_or_default()
            };
            drop(diffs);

            if patch.trim().is_empty() {
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(serde_json::json!({
                        "status": "noop",
                        "message": "no patch to apply"
                    })),
                    error: None,
                }
            } else {
                let root_path = std::path::Path::new(root_str);
                let mut cmd = tokio::process::Command::new("git");
                cmd.args(["apply", "--whitespace=nowarn", "-"])
                    .current_dir(root_path)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                match cmd.spawn() {
                    Ok(mut child) => {
                        if let Some(mut stdin) = child.stdin.take() {
                            let _ = stdin.write_all(patch.as_bytes()).await;
                        }
                        match child.wait_with_output().await {
                            Ok(out) if out.status.success() => JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id,
                                result: Some(serde_json::json!({
                                    "status": "applied",
                                    "lines": patch.lines().count()
                                })),
                                error: None,
                            },
                            Ok(out) => JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32000,
                                    message: format!(
                                        "git apply failed: {}",
                                        String::from_utf8_lossy(&out.stderr).trim()
                                    ),
                                    data: None,
                                }),
                            },
                            Err(e) => JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32000,
                                    message: format!("git apply execution failed: {e}"),
                                    data: None,
                                }),
                            },
                        }
                    }
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32000,
                            message: format!("failed to spawn git: {e}"),
                            data: None,
                        }),
                    },
                }
            }
        }
        "workspace.blast_radius" => {
            let symbol = req
                .params
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let root = req
                .params
                .get("root")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let config = hydra_matrix::IndexConfig {
                paths: vec![format!("{root}/**/*.rs")],
                ..hydra_matrix::IndexConfig::default()
            };
            if let Ok(mut matrix) = hydra_matrix::CodeMatrix::with_config(config) {
                let _ = matrix.index().await;
                let files = matrix.calculate_blast_radius(symbol).await.unwrap_or_default();
                let files_str: Vec<String> = files
                    .into_iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(serde_json::json!({
                        "symbol": symbol,
                        "blast_radius_files": files_str
                    })),
                    error: None,
                }
            } else {
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(serde_json::json!({
                        "symbol": symbol,
                        "blast_radius_files": []
                    })),
                    error: None,
                }
            }
        }
        "workspace.index" => {
            let root = req
                .params
                .get("root")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let config = hydra_matrix::IndexConfig {
                paths: vec![format!("{root}/**/*.rs")],
                ..hydra_matrix::IndexConfig::default()
            };
            if let Ok(mut matrix) = hydra_matrix::CodeMatrix::with_config(config) {
                match matrix.index().await {
                    Ok(count) => {
                        let stats = matrix.get_stats().await;
                        JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: Some(serde_json::json!({
                                "indexed_files": count,
                                "total_elements": stats.total_elements,
                                "functions": stats.functions,
                                "classes": stats.classes,
                                "structs": stats.structs
                            })),
                            error: None,
                        }
                    }
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32001,
                            message: format!("indexing error: {e}"),
                            data: None,
                        }),
                    },
                }
            } else {
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32001,
                        message: "failed to initialize code matrix".to_string(),
                        data: None,
                    }),
                }
            }
        }
        "providers.list" => {
            let config_path = req
                .params
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or("hydra.json");
            match crate::routing::RoutingConfig::load(std::path::Path::new(config_path)) {
                Ok(cfg) => {
                    let candidates: Vec<serde_json::Value> = cfg
                        .candidates
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "provider": c.provider,
                                "model": c.model,
                                "profile": c.profile,
                                "healthy": c.healthy,
                                "preference": c.preference,
                                "capabilities": c.capabilities,
                            })
                        })
                        .collect();
                    JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: Some(serde_json::json!({
                            "candidates": candidates,
                            "profiles_count": cfg.profiles.len()
                        })),
                        error: None,
                    }
                }
                Err(e) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32002,
                        message: format!("failed to load config: {e}"),
                        data: None,
                    }),
                },
            }
        }
        "patch.reject" => {
            let part_id = req
                .params
                .get("partition_id")
                .and_then(|v| v.as_str());
            let mut diffs = state.diff_cache.write().await;
            let discarded = if let Some(pid) = part_id {
                diffs.remove(pid).is_some()
            } else {
                let count = diffs.len();
                diffs.clear();
                count > 0
            };
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::json!({
                    "status": "rejected",
                    "discarded": discarded
                })),
                error: None,
            }
        }
        "checkpoint.list" => {
            let root = req
                .params
                .get("root")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let mgr = crate::checkpoints::CheckpointManager::new(std::path::Path::new(root));
            match mgr.list().await {
                Ok(list) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(serde_json::to_value(list).unwrap_or_default()),
                    error: None,
                },
                Err(e) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32003,
                        message: format!("failed to list checkpoints: {e}"),
                        data: None,
                    }),
                },
            }
        }
        "checkpoint.rollback" => {
            let root = req
                .params
                .get("root")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let stash_ref = req
                .params
                .get("stash_ref")
                .and_then(|v| v.as_str());
            let mgr = crate::checkpoints::CheckpointManager::new(std::path::Path::new(root));
            let outcome = if let Some(sr) = stash_ref {
                mgr.restore(sr).await.map(|_| sr.to_string())
            } else {
                mgr.pop_latest().await
            };
            match outcome {
                Ok(label) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(serde_json::json!({
                        "status": "rolled_back",
                        "restored": label
                    })),
                    error: None,
                },
                Err(e) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32003,
                        message: format!("rollback failed: {e}"),
                        data: None,
                    }),
                },
            }
        }
        "status" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(serde_json::json!({
                "status": "online",
                "active_tasks": state.active_tokens.lock().await.len(),
            })),
            error: None,
        },
        unknown => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method '{}' not found", unknown),
                data: None,
            }),
        },
    }
}

/// Runs the streaming headless daemon server over standard input/output (LSP-style stdio).
/// Essential for VS Code extensions and IDE plugins.
pub async fn run_daemon_stdio() -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();
    let state = Arc::new(DaemonState::new());

    while reader.read_line(&mut line).await? > 0 {
        if line.len() > 1024 * 1024 {
            anyhow::bail!("daemon request exceeds 1 MiB");
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(trimmed) {
                let resp = dispatch_json_rpc(&state, req).await;
                let mut resp_str = serde_json::to_string(&resp)?;
                resp_str.push('\n');
                stdout.write_all(resp_str.as_bytes()).await?;
                stdout.flush().await?;
            } else {
                let err = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: "Parse error: invalid JSON".to_string(),
                        data: None,
                    }),
                };
                let mut err_str = serde_json::to_string(&err)?;
                err_str.push('\n');
                stdout.write_all(err_str.as_bytes()).await?;
                stdout.flush().await?;
            }
        }
        line.clear();
    }
    Ok(())
}

/// Runs the streaming headless daemon server over TCP (Phase 5.1 & 5.2).
pub async fn run_daemon_server(bind_addr: &str) -> Result<()> {
    let address: std::net::SocketAddr = bind_addr
        .parse()
        .map_err(|_| anyhow!("daemon requires an explicit loopback socket address"))?;
    if !address.ip().is_loopback() {
        anyhow::bail!("daemon refuses non-loopback address {bind_addr}");
    }
    let listener = TcpListener::bind(address)
        .await
        .map_err(|e| anyhow!("Failed to bind daemon on {bind_addr}: {e}"))?;
    let state = Arc::new(DaemonState::new());

    while let Ok((socket, _peer)) = listener.accept().await {
        let state = state.clone();
        tokio::spawn(async move {
            let _ = handle_client_connection(socket, state).await;
        });
    }

    Ok(())
}

/// Handles a single TCP client connection with line-delimited JSON-RPC streaming.
pub async fn handle_client_connection(
    mut socket: TcpStream,
    state: Arc<DaemonState>,
) -> Result<()> {
    let (reader, mut writer) = socket.split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    while buf_reader.read_line(&mut line).await? > 0 {
        if line.len() > 1024 * 1024 {
            anyhow::bail!("daemon request exceeds 1 MiB");
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(trimmed) {
                let resp = dispatch_json_rpc(&state, req).await;
                let mut resp_str = serde_json::to_string(&resp)?;
                resp_str.push('\n');
                writer.write_all(resp_str.as_bytes()).await?;
            } else {
                let err = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: "Parse error: invalid JSON".to_string(),
                        data: None,
                    }),
                };
                let mut err_str = serde_json::to_string(&err)?;
                err_str.push('\n');
                writer.write_all(err_str.as_bytes()).await?;
            }
        }
        line.clear();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cancellation_token_hierarchy() {
        let root = CancellationToken::new();
        let child = root.child_token();

        assert!(!root.is_cancelled());
        assert!(!child.is_cancelled());

        // Cancel root -> child must also cancel instantly
        root.cancel();
        assert!(root.is_cancelled());

        // Wait brief tick for watch propagation
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn test_json_rpc_dispatch_start_and_cancel() {
        let state = Arc::new(DaemonState::new());

        // Start task
        let start_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "swarm.start".to_string(),
            params: serde_json::json!({ "task_id": "task-42" }),
        };
        let resp = dispatch_json_rpc(&state, start_req).await;
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap().get("status").unwrap(), "started");

        // Status check
        let status_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "status".to_string(),
            params: serde_json::json!({}),
        };
        let status_resp = dispatch_json_rpc(&state, status_req).await;
        assert_eq!(status_resp.result.unwrap().get("active_tasks").unwrap(), 1);

        // Cancel task
        let cancel_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(3)),
            method: "swarm.cancel".to_string(),
            params: serde_json::json!({ "task_id": "task-42" }),
        };
        let cancel_resp = dispatch_json_rpc(&state, cancel_req).await;
        assert_eq!(
            cancel_resp.result.unwrap().get("status").unwrap(),
            "cancelled"
        );
    }

    #[tokio::test]
    async fn test_json_rpc_notifications_formatting() {
        let delta_notif = JsonRpcNotification::stream_delta("p1", "Coder", "let x = 1;");
        assert_eq!(delta_notif.method, "stream.delta");
        assert_eq!(delta_notif.params.get("delta").unwrap(), "let x = 1;");

        let status_notif = JsonRpcNotification::partition_status("p1", "Reviewing");
        assert_eq!(status_notif.method, "partition.status");
        assert_eq!(status_notif.params.get("status").unwrap(), "Reviewing");

        let diff_notif = JsonRpcNotification::diff_preview("p1", "--- a/f\n+++ b/f\n");
        assert_eq!(diff_notif.method, "diff.preview");
        assert_eq!(diff_notif.params.get("partition_id").unwrap(), "p1");
    }

    #[tokio::test]
    async fn test_json_rpc_diff_preview_and_unknown_method() {
        let state = Arc::new(DaemonState::new());
        {
            let mut diffs = state.diff_cache.write().await;
            diffs.insert("part-1".to_string(), "--- a.rs\n+++ a.rs\n".to_string());
        }

        // diff.preview
        let diff_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(10)),
            method: "diff.preview".to_string(),
            params: serde_json::json!({ "partition_id": "part-1" }),
        };
        let resp = dispatch_json_rpc(&state, diff_req).await;
        assert!(resp.error.is_none());
        assert_eq!(
            resp.result.unwrap().get("diff").unwrap(),
            "--- a.rs\n+++ a.rs\n"
        );

        // unknown method
        let unknown_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(11)),
            method: "non_existent_method".to_string(),
            params: serde_json::json!({}),
        };
        let err_resp = dispatch_json_rpc(&state, unknown_req).await;
        assert!(err_resp.error.is_some());
        assert_eq!(err_resp.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn test_cancellation_child_cancel_does_not_cancel_parent() {
        let root = CancellationToken::new();
        let child = root.child_token();

        child.cancel();
        assert!(child.is_cancelled());
        assert!(!root.is_cancelled());
    }

    #[tokio::test]
    async fn test_json_rpc_patch_get_and_apply_noop() {
        let state = Arc::new(DaemonState::new());
        {
            let mut diffs = state.diff_cache.write().await;
            diffs.insert("part-1".to_string(), "--- a.rs\n+++ a.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string());
        }

        let get_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(20)),
            method: "patch.get".to_string(),
            params: serde_json::json!({ "partition_id": "part-1" }),
        };
        let resp = dispatch_json_rpc(&state, get_req).await;
        assert!(resp.error.is_none());
        assert!(resp.result.unwrap().get("patch").unwrap().as_str().unwrap().contains("+new"));

        // Noop apply test
        let state_empty = Arc::new(DaemonState::new());
        let apply_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(21)),
            method: "patch.apply".to_string(),
            params: serde_json::json!({ "partition_id": "nonexistent" }),
        };
        let apply_resp = dispatch_json_rpc(&state_empty, apply_req).await;
        assert!(apply_resp.error.is_none());
        assert_eq!(apply_resp.result.unwrap().get("status").unwrap(), "noop");
    }

    #[tokio::test]
    async fn test_json_rpc_workspace_index() {
        let state = Arc::new(DaemonState::new());
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(22)),
            method: "workspace.index".to_string(),
            params: serde_json::json!({ "root": "." }),
        };
        let resp = dispatch_json_rpc(&state, req).await;
        assert!(resp.error.is_none());
        assert!(resp.result.unwrap().get("indexed_files").is_some());
    }

    #[tokio::test]
    async fn test_json_rpc_patch_reject() {
        let state = Arc::new(DaemonState::new());
        {
            let mut diffs = state.diff_cache.write().await;
            diffs.insert("part-1".to_string(), "diff content".to_string());
        }

        let reject_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(23)),
            method: "patch.reject".to_string(),
            params: serde_json::json!({ "partition_id": "part-1" }),
        };
        let resp = dispatch_json_rpc(&state, reject_req).await;
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap().get("status").unwrap(), "rejected");

        let diffs = state.diff_cache.read().await;
        assert!(!diffs.contains_key("part-1"));
    }

    #[tokio::test]
    async fn test_json_rpc_checkpoint_list() {
        let state = Arc::new(DaemonState::new());
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(24)),
            method: "checkpoint.list".to_string(),
            params: serde_json::json!({ "root": "." }),
        };
        let resp = dispatch_json_rpc(&state, req).await;
        assert!(resp.error.is_none());
        assert!(resp.result.unwrap().is_array());
    }

    #[tokio::test]
    async fn test_json_rpc_patch_get_all_merged() {
        let state = Arc::new(DaemonState::new());
        {
            let mut diffs = state.diff_cache.write().await;
            diffs.insert("p1".to_string(), "--- a.rs\n+++ a.rs\n@@ -1 +1 @@\n-1\n+2\n".to_string());
            diffs.insert("p2".to_string(), "--- b.rs\n+++ b.rs\n@@ -1 +1 @@\n-3\n+4\n".to_string());
        }

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(25)),
            method: "patch.get".to_string(),
            params: serde_json::json!({}),
        };
        let resp = dispatch_json_rpc(&state, req).await;
        assert!(resp.error.is_none());
        let patch = resp.result.unwrap().get("patch").unwrap().as_str().unwrap().to_string();
        assert!(patch.contains("a.rs"));
        assert!(patch.contains("b.rs"));
    }

    #[tokio::test]
    async fn test_json_rpc_decision_respond_not_found() {
        let state = Arc::new(DaemonState::new());
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(26)),
            method: "decision.respond".to_string(),
            params: serde_json::json!({
                "decision_id": "nonexistent",
                "option_id": "opt1"
            }),
        };
        let resp = dispatch_json_rpc(&state, req).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }
}


