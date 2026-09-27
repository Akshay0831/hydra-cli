//! Selectable multi-turn session management, conversation persistence, and context compaction.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::docs::DocInclusionStrategy;
use crate::harness::ApprovalMode;
use crate::routing::ModelSelectorStrategy;

/// Selectable session configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Selectable model selection strategy (Default: Heuristic).
    #[serde(default)]
    pub model_selector_strategy: ModelSelectorStrategy,

    /// Selectable documentation inclusion strategy (Default: AiDenseConcise).
    #[serde(default)]
    pub doc_inclusion_strategy: DocInclusionStrategy,

    /// Selectable tool approval mode (Default: RulesBased).
    #[serde(default)]
    pub tool_approval_mode: ApprovalMode,

    /// Token budget threshold before context compaction triggers.
    #[serde(default = "default_token_budget")]
    pub context_token_budget: usize,
}

fn default_token_budget() -> usize {
    128_000
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            model_selector_strategy: ModelSelectorStrategy::Heuristic,
            doc_inclusion_strategy: DocInclusionStrategy::AiDenseConcise,
            tool_approval_mode: ApprovalMode::RulesBased,
            context_token_budget: default_token_budget(),
        }
    }
}

/// A single turn in a multi-turn conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationTurn {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: u64,
}

impl ConversationTurn {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.into(),
            content: content.into(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

/// An active multi-turn conversation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub workspace_root: PathBuf,
    pub config: SessionConfig,
    pub turns: Vec<ConversationTurn>,
    pub created_at: u64,
}

impl Session {
    pub fn new(session_id: String, workspace_root: PathBuf, config: SessionConfig) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            session_id,
            workspace_root,
            config,
            turns: Vec::new(),
            created_at,
        }
    }

    /// Appends a new conversation turn to the session history.
    pub fn add_turn(&mut self, turn: ConversationTurn) {
        self.turns.push(turn);
    }

    /// Compaction: retains system persona and recent turns, compressing middle turns if history grows large.
    pub fn compact_context(&mut self) -> bool {
        // Approximate 4 chars per token
        let estimated_tokens: usize = self.turns.iter().map(|t| t.content.len() / 4).sum();
        if estimated_tokens <= self.config.context_token_budget || self.turns.len() <= 6 {
            return false;
        }

        // Keep turn 0 (system prompt) and last 4 turns, summarizing the middle
        let system_turn = self.turns.first().cloned();
        let recent_turns: Vec<ConversationTurn> = self.turns.iter().rev().take(4).rev().cloned().collect();
        let compacted_count = self.turns.len().saturating_sub(5);

        let summary_content = format!(
            "[Context summary: {} earlier conversation turns compacted to maintain token budget]",
            compacted_count
        );
        let summary_turn = ConversationTurn::new("system", summary_content);

        let mut new_turns = Vec::new();
        if let Some(st) = system_turn {
            new_turns.push(st);
        }
        new_turns.push(summary_turn);
        new_turns.extend(recent_turns);

        self.turns = new_turns;
        true
    }

    /// Persists session state to .hydra/sessions/<session_id>.json.
    pub fn save(&self) -> Result<()> {
        let sessions_dir = self.workspace_root.join(".hydra").join("sessions");
        if !sessions_dir.exists() {
            std::fs::create_dir_all(&sessions_dir)?;
        }
        let file_path = sessions_dir.join(format!("{}.json", self.session_id));
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&file_path, content)?;
        Ok(())
    }

    /// Loads session state from .hydra/sessions/<session_id>.json.
    pub fn load(workspace_root: &Path, session_id: &str) -> Result<Self> {
        let file_path = workspace_root
            .join(".hydra")
            .join("sessions")
            .join(format!("{session_id}.json"));
        let content = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Session '{session_id}' not found at {}", file_path.display()))?;
        let session: Self = serde_json::from_str(&content)?;
        Ok(session)
    }
}

/// Central in-memory and disk session manager.
pub struct SessionManager {
    pub sessions: Arc<RwLock<HashMap<String, Session>>>,
    pub workspace_root: PathBuf,
}

impl SessionManager {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            workspace_root,
        }
    }

    /// Creates and persists a new session with the specified configuration.
    pub async fn create_session(&self, config: SessionConfig) -> Result<String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = Session::new(session_id.clone(), self.workspace_root.clone(), config);
        let _ = session.save();

        let mut lock = self.sessions.write().await;
        lock.insert(session_id.clone(), session);
        Ok(session_id)
    }

    /// Retrieves an active session from memory or disk.
    pub async fn get_session(&self, session_id: &str) -> Option<Session> {
        {
            let lock = self.sessions.read().await;
            if let Some(s) = lock.get(session_id) {
                return Some(s.clone());
            }
        }
        if let Ok(loaded) = Session::load(&self.workspace_root, session_id) {
            let mut lock = self.sessions.write().await;
            lock.insert(session_id.to_string(), loaded.clone());
            Some(loaded)
        } else {
            None
        }
    }

    /// Adds a turn to an active session and saves to disk.
    pub async fn add_turn(&self, session_id: &str, turn: ConversationTurn) -> Result<()> {
        let mut lock = self.sessions.write().await;
        if let Some(session) = lock.get_mut(session_id) {
            session.add_turn(turn);
            session.compact_context();
            session.save()?;
            Ok(())
        } else {
            anyhow::bail!("Session '{session_id}' not found");
        }
    }

    /// Lists all available sessions in .hydra/sessions/.
    pub fn list_sessions(&self) -> Vec<String> {
        self.list_sessions_for_workspace(&self.workspace_root)
    }

    /// Lists all available sessions for a specific workspace root.
    pub fn list_sessions_for_workspace(&self, workspace_root: &Path) -> Vec<String> {
        let mut session_ids = Vec::new();
        let sessions_dir = workspace_root.join(".hydra").join("sessions");
        if let Ok(entries) = std::fs::read_dir(sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        session_ids.push(stem.to_string());
                    }
                }
            }
        }
        session_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_lifecycle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = SessionManager::new(temp.path().to_path_buf());

        let config = SessionConfig::default();
        let session_id = manager.create_session(config).await.expect("create session");

        let turn1 = ConversationTurn::new("user", "Hello Hydra");
        manager.add_turn(&session_id, turn1).await.expect("add turn");

        let loaded = manager.get_session(&session_id).await.expect("load session");
        assert_eq!(loaded.turns.len(), 1);
        assert_eq!(loaded.turns[0].content, "Hello Hydra");

        let sessions = manager.list_sessions();
        assert!(sessions.contains(&session_id));
    }

    #[test]
    fn test_context_compaction() {
        let mut session = Session::new(
            "test_compact".into(),
            PathBuf::from("."),
            SessionConfig {
                context_token_budget: 10, // Very low budget to trigger compaction
                ..Default::default()
            },
        );

        session.add_turn(ConversationTurn::new("system", "System instruction"));
        for i in 1..=10 {
            session.add_turn(ConversationTurn::new("user", format!("Turn content {}", i)));
        }

        assert_eq!(session.turns.len(), 11);
        let compacted = session.compact_context();
        assert!(compacted);
        // Turn 0 (system) + 1 summary turn + 4 recent turns = 6 turns
        assert_eq!(session.turns.len(), 6);
        assert_eq!(session.turns[0].role, "system");
        assert!(session.turns[1].content.contains("Context summary"));
    }

    #[test]
    fn test_session_save_load_roundtrip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut session = Session::new("test_roundtrip".into(), temp.path().to_path_buf(), SessionConfig::default());
        session.add_turn(ConversationTurn::new("user", "Hello world"));
        session.add_turn(ConversationTurn::new("assistant", "Hi there"));

        session.save().expect("save session");

        let loaded = Session::load(temp.path(), "test_roundtrip").expect("load session");
        assert_eq!(loaded.session_id, "test_roundtrip");
        assert_eq!(loaded.turns.len(), 2);
        assert_eq!(loaded.turns[0].content, "Hello world");
        assert_eq!(loaded.turns[1].content, "Hi there");
    }

    #[test]
    fn test_list_sessions_for_workspace() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = SessionManager::new(temp.path().to_path_buf());

        let s1 = Session::new("sess_alpha".into(), temp.path().to_path_buf(), SessionConfig::default());
        let s2 = Session::new("sess_beta".into(), temp.path().to_path_buf(), SessionConfig::default());
        s1.save().expect("save s1");
        s2.save().expect("save s2");

        let list = manager.list_sessions_for_workspace(temp.path());
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"sess_alpha".to_string()));
        assert!(list.contains(&"sess_beta".to_string()));
    }

    #[tokio::test]
    async fn test_add_turn_triggers_compaction_above_budget() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = SessionManager::new(temp.path().to_path_buf());

        let config = SessionConfig {
            context_token_budget: 10,
            ..Default::default()
        };
        let session_id = manager.create_session(config).await.expect("create session");

        manager
            .add_turn(&session_id, ConversationTurn::new("system", "System prompt"))
            .await
            .expect("add turn");

        for i in 1..=10 {
            manager
                .add_turn(
                    &session_id,
                    ConversationTurn::new("user", format!("Turn content with enough length to exceed budget {}", i)),
                )
                .await
                .expect("add turn");
        }

        let loaded = manager.get_session(&session_id).await.expect("get session");
        // Compacted: 1 system turn + 1 summary turn + 4 recent turns = 6 turns
        assert_eq!(loaded.turns.len(), 6);
        assert_eq!(loaded.turns[0].role, "system");
        assert!(loaded.turns[1].content.contains("Context summary"));
    }
}

