// Git-based checkpoint manager for zero-friction "hydra undo"
// Creates ephemeral git stashes before patching; "hydra undo" reverts to pre-patch state
// Checkpoints labelled with unix timestamps for selective restoration

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Hydra git checkpoint metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMeta {
    pub stash_ref: String,     // Stash reference (e.g. `stash@{0}`)
    pub label: String,        // Human-readable stash message
    pub created_at_ms: u64,    // Creation timestamp (ms)
}

/// Git checkpoint manager with stash-based undo
pub struct CheckpointManager {
    repo_root: PathBuf,       // Git repository root path
}

impl CheckpointManager {
    /// Create manager scoped to repository root
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    // -----------------------------------------------------------------------
    // Checkpoint creation
    // -----------------------------------------------------------------------

    /// Create a checkpoint of the current workspace state.
    ///
    /// Runs `git stash push --include-untracked -m "hydra-checkpoint-<ms>"`.
    /// Returns the checkpoint label on success, or an error if the stash fails
    /// (e.g. when the working tree is already clean).
    pub async fn create(&self, label: Option<&str>) -> Result<String> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let stash_msg = match label {
            Some(l) => format!("hydra-checkpoint-{now_ms}-{l}"),
            None => format!("hydra-checkpoint-{now_ms}"),
        };

        let out = Command::new("git")
            .args([
                "stash",
                "push",
                "--include-untracked",
                "-m",
                &stash_msg,
            ])
            .current_dir(&self.repo_root)
            .output()
            .await
            .map_err(|e| anyhow!("failed to run git stash: {e}"))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow!("git stash push failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("No local changes to save") {
            return Err(anyhow!("No local changes to save"));
        }

        Ok(stash_msg)
    }

    // -----------------------------------------------------------------------
    // Listing checkpoints
    // -----------------------------------------------------------------------

    /// Return all Hydra checkpoints currently in the stash, newest first.
    pub async fn list(&self) -> Result<Vec<CheckpointMeta>> {
        let out = Command::new("git")
            .args(["stash", "list", "--format=%gd\t%gs"])
            .current_dir(&self.repo_root)
            .output()
            .await
            .map_err(|e| anyhow!("failed to run git stash list: {e}"))?;

        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut checkpoints = Vec::new();

        for line in stdout.lines() {
            let mut parts = line.splitn(2, '\t');
            let stash_ref = parts.next().unwrap_or("").trim().to_string();
            let message = parts.next().unwrap_or("").trim().to_string();

            // Only include our own labelled entries
            if let Some(label_part) = message.strip_prefix("On ") {
                // git stash list format: "On <branch>: <msg>"
                if let Some(hydra_label) = label_part.split_once(": ").map(|x| x.1)
                    && hydra_label.starts_with("hydra-checkpoint-") {
                        let created_at_ms = hydra_label
                            .strip_prefix("hydra-checkpoint-")
                            .and_then(|rest| rest.split('-').next())
                            .and_then(|ms_str| ms_str.parse::<u64>().ok())
                            .unwrap_or(0);

                        checkpoints.push(CheckpointMeta {
                            stash_ref,
                            label: hydra_label.to_string(),
                            created_at_ms,
                        });
                    }
            }
        }

        Ok(checkpoints)
    }

    // -----------------------------------------------------------------------
    // Restore (undo)
    // -----------------------------------------------------------------------

    /// Pop (restore) the most recent Hydra checkpoint.
    ///
    /// Returns the label of the restored checkpoint, or an error if no
    /// Hydra checkpoints exist.
    pub async fn pop_latest(&self) -> Result<String> {
        let checkpoints = self.list().await?;
        let latest = checkpoints
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no Hydra checkpoints found — nothing to undo"))?;

        self.restore(&latest.stash_ref).await?;
        Ok(latest.label)
    }

    /// Restore a specific stash entry by its stash reference (e.g. `stash@{0}`).
    pub async fn restore(&self, stash_ref: &str) -> Result<()> {
        let out = Command::new("git")
            .args(["stash", "pop", stash_ref])
            .current_dir(&self.repo_root)
            .output()
            .await
            .map_err(|e| anyhow!("failed to run git stash pop: {e}"))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow!("git stash pop failed: {}", stderr.trim()));
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Drop / cleanup
    // -----------------------------------------------------------------------

    /// Drop a specific Hydra checkpoint without restoring it.
    pub async fn drop(&self, stash_ref: &str) -> Result<()> {
        let out = Command::new("git")
            .args(["stash", "drop", stash_ref])
            .current_dir(&self.repo_root)
            .output()
            .await
            .map_err(|e| anyhow!("failed to run git stash drop: {e}"))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow!("git stash drop failed: {}", stderr.trim()));
        }

        Ok(())
    }

    /// Drop all Hydra checkpoints (clean slate).
    pub async fn drop_all(&self) -> Result<usize> {
        let checkpoints = self.list().await?;
        let count = checkpoints.len();
        // Drop in reverse order so indices stay stable
        for cp in checkpoints.into_iter().rev() {
            self.drop(&cp.stash_ref).await?;
        }
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Convenience helpers used by commands.rs
// ---------------------------------------------------------------------------

/// Create a checkpoint before applying a patch.
///
/// Silently ignores "nothing to stash" — a clean tree needs no checkpoint.
pub async fn checkpoint_before_patch(
    repo_root: &Path,
    label: Option<&str>,
) -> Result<Option<String>> {
    let mgr = CheckpointManager::new(repo_root);
    match mgr.create(label).await {
        Ok(l) => Ok(Some(l)),
        Err(e) if e.to_string().contains("No local changes") => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn init_git_repo() -> Result<TempDir> {
        let dir = tempfile::tempdir()?;
        // Init repo
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .await?;
        Command::new("git")
            .args(["config", "user.email", "test@hydra"])
            .current_dir(dir.path())
            .output()
            .await?;
        Command::new("git")
            .args(["config", "user.name", "Hydra Test"])
            .current_dir(dir.path())
            .output()
            .await?;
        // Create an initial commit so stash works
        tokio::fs::write(dir.path().join("README.md"), "# test").await?;
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .await?;
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .await?;
        Ok(dir)
    }

    #[tokio::test]
    async fn checkpoint_create_and_list() {
        let dir = init_git_repo().await.expect("git init");
        let path = dir.path();

        // Make a dirty change
        tokio::fs::write(path.join("foo.rs"), "fn foo() {}").await.unwrap();

        let mgr = CheckpointManager::new(path);
        let label = mgr.create(Some("test")).await.expect("create checkpoint");
        assert!(label.starts_with("hydra-checkpoint-"));

        let list = mgr.list().await.expect("list checkpoints");
        assert!(!list.is_empty(), "should have at least one checkpoint");
        assert!(list[0].label.starts_with("hydra-checkpoint-"));
    }

    #[tokio::test]
    async fn no_checkpoints_returns_empty() {
        let dir = init_git_repo().await.expect("git init");
        let mgr = CheckpointManager::new(dir.path());
        let list = mgr.list().await.expect("list");
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn checkpoint_pop_latest_restores_file() {
        let dir = init_git_repo().await.expect("git init");
        let path = dir.path();
        let target_file = path.join("data.txt");

        // Commit baseline
        tokio::fs::write(&target_file, "baseline").await.unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().await.unwrap();
        Command::new("git").args(["commit", "-m", "add data"]).current_dir(path).output().await.unwrap();

        // Mutate
        tokio::fs::write(&target_file, "mutated_state").await.unwrap();

        let mgr = CheckpointManager::new(path);
        let label = mgr.create(Some("mutation")).await.expect("checkpoint");

        // Working tree should now be reverted to baseline by stash push
        let content_after_stash = tokio::fs::read_to_string(&target_file).await.unwrap();
        assert_eq!(content_after_stash, "baseline");

        // Pop latest (restore mutated_state)
        let restored_label = mgr.pop_latest().await.expect("pop latest");
        assert_eq!(restored_label, label);

        let content_restored = tokio::fs::read_to_string(&target_file).await.unwrap();
        assert_eq!(content_restored, "mutated_state");
    }

    #[tokio::test]
    async fn checkpoint_drop_and_drop_all() {
        let dir = init_git_repo().await.expect("git init");
        let path = dir.path();

        tokio::fs::write(path.join("file1.rs"), "fn a() {}").await.unwrap();
        let mgr = CheckpointManager::new(path);
        mgr.create(Some("cp1")).await.expect("cp1");

        tokio::fs::write(path.join("file2.rs"), "fn b() {}").await.unwrap();
        mgr.create(Some("cp2")).await.expect("cp2");

        let list = mgr.list().await.expect("list");
        assert_eq!(list.len(), 2);

        let dropped = mgr.drop_all().await.expect("drop_all");
        assert_eq!(dropped, 2);

        let list_after = mgr.list().await.expect("list after");
        assert!(list_after.is_empty());
    }

    #[tokio::test]
    async fn test_checkpoint_before_patch_behavior() {
        let dir = init_git_repo().await.expect("git init");
        let path = dir.path();

        // Clean tree -> returns Ok(None)
        let clean_res = checkpoint_before_patch(path, Some("clean")).await.expect("clean");
        assert!(clean_res.is_none());

        // Dirty tree -> returns Ok(Some(label))
        tokio::fs::write(path.join("change.txt"), "dirty").await.unwrap();
        let dirty_res = checkpoint_before_patch(path, Some("dirty")).await.expect("dirty");
        assert!(dirty_res.is_some());
        assert!(dirty_res.unwrap().contains("dirty"));
    }
}

