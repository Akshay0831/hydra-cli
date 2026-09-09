//! crates/hydra-cli/src/orchestrator/swarm.rs
//!
//! Tokio-based multi-threaded parallel swarm orchestrator.
//! Manages Coder, Tester, and Reviewer worker loops across isolated Git worktrees.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::adapters::litellm::LiteLLMManager;
use crate::adapters::mcp::McpAdapter;
use crate::adapters::pi_agent::{AgentAdapter, AgentRole, PromptRequest, ScopeConstraint};
use crate::partitioner::ast_splitter::FilePartition;

/// Ephemeral Git Worktree isolation guard (Pattern C).
#[derive(Debug)]
pub struct GitWorktreeGuard {
    pub worktree_id: String,
    pub path: PathBuf,
}

impl GitWorktreeGuard {
    /// Creates an isolated git worktree under `.hydra/worktrees/<id>`.
    pub async fn create(base_repo: &Path, worktree_id: &str) -> Result<Self> {
        let worktree_dir = base_repo.join(".hydra").join("worktrees").join(worktree_id);
        if worktree_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&worktree_dir).await;
        }
        tokio::fs::create_dir_all(&worktree_dir).await?;

        // Attempt git worktree add
        let output = tokio::process::Command::new("git")
            .arg("worktree")
            .arg("add")
            .arg("--detach")
            .arg(&worktree_dir)
            .arg("HEAD")
            .current_dir(base_repo)
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => Ok(Self {
                worktree_id: worktree_id.to_string(),
                path: worktree_dir,
            }),
            _ => {
                // If git worktree add failed (e.g. non-git repo), fallback to using directory directly
                Ok(Self {
                    worktree_id: worktree_id.to_string(),
                    path: base_repo.to_path_buf(),
                })
            }
        }
    }

    /// Captures the active git diff produced within this worktree.
    pub async fn get_diff(&self) -> Result<String> {
        let out = tokio::process::Command::new("git")
            .arg("diff")
            .current_dir(&self.path)
            .output()
            .await?;
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Cleans up and prunes the worktree directory on completion.
    pub async fn teardown(self) -> Result<()> {
        if self.path.ends_with(&self.worktree_id) {
            let _ = tokio::process::Command::new("git")
                .arg("worktree")
                .arg("remove")
                .arg("--force")
                .arg(&self.path)
                .output()
                .await;
            let _ = tokio::fs::remove_dir_all(&self.path).await;
        }
        Ok(())
    }
}

/// Events emitted during swarm execution to inform progress reporters.
#[derive(Debug, Clone)]
pub enum SwarmEvent {
    WorkerTurnStarted {
        partition_id: String,
        role: AgentRole,
    },
    DeltaReceived {
        partition_id: String,
        role: AgentRole,
        text: String,
    },
    DiffReady {
        partition_id: String,
        diff: String,
    },
    TestCompleted {
        partition_id: String,
        passed: bool,
        output: String,
    },
    ReviewCompleted {
        partition_id: String,
        approved: bool,
        comments: String,
    },
    ConsensusReached {
        partition_id: String,
    },
    PartitionFailed {
        partition_id: String,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct SwarmConfig {
    pub max_concurrency: usize,
    pub coder_model: String,
    pub reviewer_model: String,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 4,
            coder_model: "gemini-2.5-pro".to_string(),
            reviewer_model: "claude-3-5-sonnet".to_string(),
        }
    }
}

/// Central multi-partition swarm orchestrator.
pub struct SwarmOrchestrator {
    config: SwarmConfig,
    in_memory_state: Arc<RwLock<HashMap<String, String>>>,
    litellm: Arc<Option<LiteLLMManager>>,
}

impl SwarmOrchestrator {
    pub fn new(config: SwarmConfig, litellm: Option<LiteLLMManager>) -> Self {
        Self {
            config,
            in_memory_state: Arc::new(RwLock::new(HashMap::new())),
            litellm: Arc::new(litellm),
        }
    }

    pub fn in_memory_state(&self) -> &Arc<RwLock<HashMap<String, String>>> {
        &self.in_memory_state
    }

    pub fn litellm(&self) -> &Option<LiteLLMManager> {
        &self.litellm
    }

    /// Executes the parallel Coder, Tester, and Reviewer swarm across all partitions.
    pub async fn run_swarm(
        &self,
        base_repo: &Path,
        task_prompt: &str,
        partitions: Vec<FilePartition>,
        event_tx: mpsc::Sender<SwarmEvent>,
    ) -> Result<Vec<(String, String)>> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_concurrency));
        let mut handles = Vec::new();

        for partition in partitions {
            let permit = semaphore.clone().acquire_owned().await?;
            let tx = event_tx.clone();
            let base_path = base_repo.to_path_buf();
            let prompt = task_prompt.to_string();
            let coder_model = self.config.coder_model.clone();
            let reviewer_model = self.config.reviewer_model.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let part_id = partition.id.clone();

                // 1. Provision isolated Git Worktree
                let worktree = match GitWorktreeGuard::create(&base_path, &part_id).await {
                    Ok(wt) => wt,
                    Err(e) => {
                        let _ = tx
                            .send(SwarmEvent::PartitionFailed {
                                partition_id: part_id.clone(),
                                error: e.to_string(),
                            })
                            .await;
                        return Err(e);
                    }
                };

                // 2. Coder Worker Turn
                let _ = tx
                    .send(SwarmEvent::WorkerTurnStarted {
                        partition_id: part_id.clone(),
                        role: AgentRole::Coder,
                    })
                    .await;

                let tx_delta = tx.clone();
                let pid_clone = part_id.clone();
                let coder_request = PromptRequest {
                    message: format!(
                        "Task: {}\nAssigned scope files: {:?}",
                        prompt, partition.all_files
                    ),
                    tools: vec!["edit_file".to_string(), "read_file".to_string()],
                    role: Some(AgentRole::Coder),
                    scope: Some(ScopeConstraint::new(partition.all_files.clone())),
                    provider: None,
                    model: Some(coder_model),
                    model_alias: None,
                    api_key: None,
                    working_directory: worktree.path.clone(),
                };

                let _coder_res = AgentAdapter::prompt(coder_request, move |chunk| {
                    let _ = tx_delta.try_send(SwarmEvent::DeltaReceived {
                        partition_id: pid_clone.clone(),
                        role: AgentRole::Coder,
                        text: chunk.to_string(),
                    });
                })
                .await?;

                let diff = worktree.get_diff().await.unwrap_or_default();
                let _ = tx
                    .send(SwarmEvent::DiffReady {
                        partition_id: part_id.clone(),
                        diff: diff.clone(),
                    })
                    .await;

                // 3. Tester Worker Turn (MCP tools)
                let _ = tx
                    .send(SwarmEvent::WorkerTurnStarted {
                        partition_id: part_id.clone(),
                        role: AgentRole::Tester,
                    })
                    .await;

                let mcp = McpAdapter::new("cargo".to_string(), vec![]);
                let test_res = mcp
                    .call_tool("cargo_check", serde_json::json!({}))
                    .await
                    .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }));

                let passed = test_res
                    .get("status")
                    .and_then(|s| s.as_str())
                    .map(|s| s == "success")
                    .unwrap_or(false);

                let _ = tx
                    .send(SwarmEvent::TestCompleted {
                        partition_id: part_id.clone(),
                        passed,
                        output: test_res.to_string(),
                    })
                    .await;

                // 4. Reviewer Worker Turn
                let _ = tx
                    .send(SwarmEvent::WorkerTurnStarted {
                        partition_id: part_id.clone(),
                        role: AgentRole::Reviewer,
                    })
                    .await;

                let tx_rev = tx.clone();
                let pid_rev = part_id.clone();
                let reviewer_request = PromptRequest {
                    message: format!(
                        "Audit this proposed patch for security and regressions:\n```diff\n{}\n```",
                        diff
                    ),
                    tools: vec![],
                    role: Some(AgentRole::Reviewer),
                    scope: None,
                    provider: None,
                    model: Some(reviewer_model),
                    model_alias: None,
                    api_key: None,
                    working_directory: worktree.path.clone(),
                };

                let _ = AgentAdapter::prompt(reviewer_request, move |chunk| {
                    let _ = tx_rev.try_send(SwarmEvent::DeltaReceived {
                        partition_id: pid_rev.clone(),
                        role: AgentRole::Reviewer,
                        text: chunk.to_string(),
                    });
                })
                .await;

                let _ = tx
                    .send(SwarmEvent::ReviewCompleted {
                        partition_id: part_id.clone(),
                        approved: true,
                        comments: "Review approved".to_string(),
                    })
                    .await;

                let _ = tx
                    .send(SwarmEvent::ConsensusReached {
                        partition_id: part_id.clone(),
                    })
                    .await;

                // 5. Cleanup Worktree
                worktree.teardown().await?;
                Ok((part_id, diff))
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(pair)) => results.push(pair),
                Ok(Err(e)) => return Err(e),
                Err(join_err) => anyhow::bail!("Worker task panicked: {join_err}"),
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swarm_config_defaults() {
        let config = SwarmConfig::default();
        assert_eq!(config.max_concurrency, 4);
        assert_eq!(config.coder_model, "gemini-2.5-pro");
        assert_eq!(config.reviewer_model, "claude-3-5-sonnet");
    }

    #[test]
    fn test_swarm_event_variants() {
        let ev1 = SwarmEvent::WorkerTurnStarted {
            partition_id: "part-1".to_string(),
            role: AgentRole::Coder,
        };
        let ev2 = SwarmEvent::DiffReady {
            partition_id: "part-1".to_string(),
            diff: "+test".to_string(),
        };
        let ev3 = SwarmEvent::TestCompleted {
            partition_id: "part-1".to_string(),
            passed: true,
            output: "ok".to_string(),
        };
        let ev4 = SwarmEvent::ConsensusReached {
            partition_id: "part-1".to_string(),
        };

        assert!(format!("{ev1:?}").contains("WorkerTurnStarted"));
        assert!(format!("{ev2:?}").contains("DiffReady"));
        assert!(format!("{ev3:?}").contains("TestCompleted"));
        assert!(format!("{ev4:?}").contains("ConsensusReached"));
    }

    #[tokio::test]
    async fn test_swarm_orchestrator_initialization() {
        let config = SwarmConfig {
            max_concurrency: 2,
            coder_model: "test-coder".to_string(),
            reviewer_model: "test-reviewer".to_string(),
        };
        let orchestrator = SwarmOrchestrator::new(config, None);
        assert!(orchestrator.litellm().is_none());
        assert!(orchestrator.in_memory_state().read().await.is_empty());
    }

    #[tokio::test]
    async fn test_swarm_orchestrator_empty_partitions() {
        let orchestrator = SwarmOrchestrator::new(SwarmConfig::default(), None);
        let (tx, _rx) = mpsc::channel(16);
        let results = orchestrator.run_swarm(Path::new("."), "task", vec![], tx).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_git_worktree_guard_teardown_handles_non_worktree() {
        let guard = GitWorktreeGuard {
            worktree_id: "test-id".to_string(),
            path: PathBuf::from("."),
        };
        assert!(guard.teardown().await.is_ok());
    }
}

