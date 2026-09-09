//! crates/hydra-cli/src/adapters/pi_agent.rs
//!
//! Hydra-owned facade adapter around the upstream Pi embedding API.
//! Single execution gateway for Coder, Tester, and Reviewer agent turns.

use anyhow::Result;
use pi::model::AssistantMessageEvent;
use pi::sdk::{AgentEvent, SessionOptions};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Specialized role for an agent worker in the swarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentRole {
    /// Implements code modifications and fixes.
    Coder,
    /// Executes unit tests and test suites; inspects compiler and runtime errors.
    Tester,
    /// Audits code diffs for security, logic flaws, and architectural integrity.
    Reviewer,
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentRole::Coder => write!(f, "Coder"),
            AgentRole::Tester => write!(f, "Tester"),
            AgentRole::Reviewer => write!(f, "Reviewer"),
        }
    }
}

/// Boundary defining which files a worker can read and modify.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScopeConstraint {
    pub allowed_paths: Vec<PathBuf>,
    pub read_only_paths: Vec<PathBuf>,
}

impl ScopeConstraint {
    pub fn new(allowed_paths: Vec<PathBuf>) -> Self {
        Self {
            allowed_paths,
            read_only_paths: Vec::new(),
        }
    }

    /// Validates whether a file write target is permitted within this partition scope.
    pub fn is_write_allowed(&self, target_path: &Path) -> bool {
        if self.allowed_paths.is_empty() {
            return true;
        }
        self.allowed_paths
            .iter()
            .any(|allowed| target_path.starts_with(allowed) || target_path == allowed)
    }
}

/// Execution outcome of an agent turn.
#[derive(Debug, Clone, Default)]
pub struct AgentTurnResult {
    pub output_text: String,
    pub touched_files: Vec<PathBuf>,
    pub error: Option<String>,
}

/// Inputs Hydra needs to start one Pi agent prompt.
pub struct PromptRequest {
    pub message: String,
    pub tools: Vec<String>,
    pub role: Option<AgentRole>,
    pub scope: Option<ScopeConstraint>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub model_alias: Option<String>,
    pub api_key: Option<String>,
    pub working_directory: PathBuf,
}

impl PromptRequest {
    pub fn new(message: String, working_directory: PathBuf) -> Self {
        Self {
            message,
            tools: Vec::new(),
            role: None,
            scope: None,
            provider: None,
            model: None,
            model_alias: None,
            api_key: None,
            working_directory,
        }
    }

    pub fn with_role(mut self, role: AgentRole) -> Self {
        self.role = Some(role);
        self
    }

    pub fn with_scope(mut self, scope: ScopeConstraint) -> Self {
        self.scope = Some(scope);
        self
    }
}

/// Minimal Hydra-owned facade for the upstream agent SDK.
pub struct AgentAdapter;

impl AgentAdapter {
    pub fn builtin_tool_names() -> &'static [&'static str] {
        pi::sdk::BUILTIN_TOOL_NAMES
    }

    /// Primary execution gateway for CLI prompts and backwards-compatible callers.
    pub async fn prompt(
        request: PromptRequest,
        on_text: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<()> {
        let _ = Self::prompt_extended(request, on_text).await?;
        Ok(())
    }

    /// Extended gateway returning turn result metadata and handling role injection.
    pub async fn prompt_extended(
        request: PromptRequest,
        on_text: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<AgentTurnResult> {
        let effective_message = if let Some(role) = request.role {
            let role_prompt = Self::format_role_prompt(role, request.scope.as_ref());
            format!("{}\n\n{}", role_prompt, request.message)
        } else {
            request.message
        };

        let tools = if request.tools.is_empty() {
            Self::builtin_tool_names()
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            request.tools
        };

        let options = SessionOptions {
            provider: request.provider,
            model: request.model_alias.or(request.model),
            api_key: request.api_key,
            enabled_tools: Some(tools),
            working_directory: Some(request.working_directory),
            ..SessionOptions::default()
        };

        let mut session = pi::sdk::create_agent_session(options).await?;
        let output_accumulator = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
        let acc_clone = output_accumulator.clone();

        session
            .prompt(effective_message, move |event| {
                if let AgentEvent::MessageUpdate {
                    assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
                    ..
                } = event
                {
                    on_text(&delta);
                    let mut lock = acc_clone.blocking_lock();
                    lock.push_str(&delta);
                }
            })
            .await?;

        let output_text = output_accumulator.lock().await.clone();
        let touched_files = request
            .scope
            .map(|s| s.allowed_paths)
            .unwrap_or_default();

        Ok(AgentTurnResult {
            output_text,
            touched_files,
            error: None,
        })
    }

    /// Formats role-specific system guidance for the agent context.
    fn format_role_prompt(role: AgentRole, scope: Option<&ScopeConstraint>) -> String {
        match role {
            AgentRole::Coder => {
                let scope_text = if let Some(s) = scope {
                    format!("Allowed write paths: {:?}", s.allowed_paths)
                } else {
                    "Unconstrained workspace write permissions".to_string()
                };
                format!(
                    "ROLE INSTRUCTION: You are the CODER agent.\n\
                     Goal: Implement concise, accurate code edits that fulfill the task.\n\
                     Constraint: {scope_text}. Never edit files outside your assigned scope."
                )
            }
            AgentRole::Tester => {
                "ROLE INSTRUCTION: You are the TESTER agent.\n\
                 Goal: Run test suites, verify compiler diagnostics, and isolate test regressions.\n\
                 Constraint: Do not alter source implementation files; only report test results."
                    .to_string()
            }
            AgentRole::Reviewer => {
                "ROLE INSTRUCTION: You are the REVIEWER agent.\n\
                 Goal: Audit proposed code diffs for security vulnerabilities, race conditions, edge cases, and consistency.\n\
                 Constraint: Provide structured, actionable critique without modifying files."
                    .to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_constraint_allows_subpaths() {
        let scope = ScopeConstraint::new(vec![PathBuf::from("src/adapters")]);
        assert!(scope.is_write_allowed(&PathBuf::from("src/adapters/pi_agent.rs")));
        assert!(!scope.is_write_allowed(&PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_empty_scope_allows_any_path() {
        let scope = ScopeConstraint::default();
        assert!(scope.is_write_allowed(&PathBuf::from("src/anything.rs")));
    }

    #[test]
    fn test_agent_role_display() {
        assert_eq!(format!("{}", AgentRole::Coder), "Coder");
        assert_eq!(format!("{}", AgentRole::Tester), "Tester");
        assert_eq!(format!("{}", AgentRole::Reviewer), "Reviewer");
    }

    #[test]
    fn test_prompt_request_builder() {
        let req = PromptRequest::new("hello".to_string(), PathBuf::from("."))
            .with_role(AgentRole::Coder)
            .with_scope(ScopeConstraint::new(vec![PathBuf::from("src")]));
        assert_eq!(req.role, Some(AgentRole::Coder));
        assert!(req.scope.is_some());
    }

    #[test]
    fn test_scope_constraint_exact_file_matching() {
        let scope = ScopeConstraint::new(vec![PathBuf::from("src/main.rs"), PathBuf::from("tests/test.rs")]);
        assert!(scope.is_write_allowed(&PathBuf::from("src/main.rs")));
        assert!(scope.is_write_allowed(&PathBuf::from("tests/test.rs")));
        assert!(!scope.is_write_allowed(&PathBuf::from("src/lib.rs")));
    }

    #[test]
    fn test_agent_role_serde_roundtrip() {
        for role in &[AgentRole::Coder, AgentRole::Tester, AgentRole::Reviewer] {
            let json = serde_json::to_string(role).unwrap();
            let deserialized: AgentRole = serde_json::from_str(&json).unwrap();
            assert_eq!(*role, deserialized);
        }
    }

    #[test]
    fn test_format_role_prompts_contain_guidance() {
        let coder_p = AgentAdapter::format_role_prompt(AgentRole::Coder, None);
        assert!(coder_p.contains("CODER"));
        assert!(coder_p.contains("Goal:"));

        let tester_p = AgentAdapter::format_role_prompt(AgentRole::Tester, None);
        assert!(tester_p.contains("TESTER"));

        let rev_p = AgentAdapter::format_role_prompt(AgentRole::Reviewer, None);
        assert!(rev_p.contains("REVIEWER"));
    }
}

