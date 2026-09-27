//! User-configurable, tamper-proof command safety policy and execution guards.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Result of evaluating a shell command against the safety policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandSafetyResult {
    /// Safe to execute automatically without prompting the user.
    Allowed,
    /// Requires explicit confirmation from the user before execution.
    RequiresApproval { reason: String, command: String },
    /// Dangerous or disallowed command; hard-rejected.
    Blocked { reason: String, command: String },
}

/// Specific safety violation error when safety boundaries are breached.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SafetyViolation {
    #[error("Agent attempted to modify protected safety configuration at '{path}' without explicit user permission")]
    ProtectedFileMutationAttempt { path: String },

    #[error("Command contains blocked dangerous keyword '{keyword}' in '{command}'")]
    BlockedKeywordDetected { keyword: String, command: String },

    #[error("Command contains unauthorized chaining or subshell execution: '{command}'")]
    DisallowedChainedCommand { command: String },
}

/// User-configurable command safety policy with tamper-proof protections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSafetyPolicy {
    /// Commands permitted for auto-approval strictly when executed standalone.
    #[serde(default = "default_safe_standalone_commands")]
    pub safe_standalone_commands: Vec<String>,

    /// Dangerous keywords or substrings that trigger hard rejection.
    #[serde(default = "default_blocked_keywords")]
    pub blocked_keywords: Vec<String>,

    /// Configuration files that AI agents are forbidden from mutating.
    #[serde(default = "default_protected_paths")]
    pub protected_paths: Vec<String>,

    /// Whether safe commands must strictly be standalone (no chaining, pipes, or subshells).
    #[serde(default = "default_true")]
    pub allow_standalone_only: bool,
}

fn default_true() -> bool {
    true
}

fn default_safe_standalone_commands() -> Vec<String> {
    vec![
        "ls".into(),
        "dir".into(),
        "pwd".into(),
        "git".into(),
        "cargo".into(),
        "echo".into(),
        "cat".into(),
        "type".into(),
        "head".into(),
        "tail".into(),
        "which".into(),
        "where".into(),
    ]
}

fn default_blocked_keywords() -> Vec<String> {
    vec![
        "rm -rf".into(),
        "sudo".into(),
        "dd if=".into(),
        "mkfs".into(),
        "format ".into(),
        ":(){ :|:& };:".into(),
        "> /dev/sda".into(),
        "shutdown".into(),
        "reboot".into(),
    ]
}

fn default_protected_paths() -> Vec<String> {
    vec![
        ".hydra/safety.json".into(),
        "hydra.json".into(),
        ".hydra/goals.json".into(),
    ]
}

impl Default for CommandSafetyPolicy {
    fn default() -> Self {
        Self {
            safe_standalone_commands: default_safe_standalone_commands(),
            blocked_keywords: default_blocked_keywords(),
            protected_paths: default_protected_paths(),
            allow_standalone_only: true,
        }
    }
}

impl CommandSafetyPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load user safety policy from workspace `.hydra/safety.json`, falling back to default.
    pub fn load_from_workspace(workspace_root: &Path) -> Result<Self> {
        let policy_path = workspace_root.join(".hydra").join("safety.json");
        if policy_path.exists() {
            let content = std::fs::read_to_string(&policy_path)?;
            let policy: Self = serde_json::from_str(&content)?;
            Ok(policy)
        } else {
            Ok(Self::default())
        }
    }

    /// Save safety policy to `.hydra/safety.json`.
    pub fn save_to_workspace(&self, workspace_root: &Path) -> Result<()> {
        let hydra_dir = workspace_root.join(".hydra");
        if !hydra_dir.exists() {
            std::fs::create_dir_all(&hydra_dir)?;
        }
        let policy_path = hydra_dir.join("safety.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(policy_path, content)?;
        Ok(())
    }

    /// Evaluates a shell command string against the safety policy.
    pub fn evaluate_command(&self, cmd: &str) -> CommandSafetyResult {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return CommandSafetyResult::Allowed;
        }

        // 1. Check for blocked dangerous keywords (highest precedence)
        for keyword in &self.blocked_keywords {
            if trimmed.contains(keyword) {
                return CommandSafetyResult::Blocked {
                    reason: format!("Command contains blocked dangerous keyword: '{keyword}'"),
                    command: trimmed.to_string(),
                };
            }
        }

        // 2. Check for command chaining, pipes, or subshells
        let has_chaining = trimmed.contains("&&")
            || trimmed.contains(';')
            || trimmed.contains("||")
            || trimmed.contains('|')
            || trimmed.contains("$(")
            || trimmed.contains('`');

        if self.allow_standalone_only && has_chaining {
            return CommandSafetyResult::RequiresApproval {
                reason: "Command contains chaining operators (&&, ;, ||, |) or subshells".to_string(),
                command: trimmed.to_string(),
            };
        }

        // 3. Extract the root executable name
        let first_token = trimmed.split_whitespace().next().unwrap_or("");
        let clean_token = first_token.trim_matches([';', '&', '|', '(', ')', '`']);
        let base_cmd = Path::new(clean_token)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(clean_token)
            .to_lowercase();
        // Strip common Windows extensions like .exe, .cmd, .bat
        let base_name = base_cmd
            .trim_end_matches(".exe")
            .trim_end_matches(".cmd")
            .trim_end_matches(".bat");

        // 4. Evaluate against safe standalone whitelist
        let is_whitelisted = self
            .safe_standalone_commands
            .iter()
            .any(|s| s.to_lowercase() == base_name);

        if is_whitelisted {
            return CommandSafetyResult::Allowed;
        }

        // 5. Default non-whitelisted command requires user confirmation
        CommandSafetyResult::RequiresApproval {
            reason: format!("Command '{base_name}' is not in the safe standalone whitelist"),
            command: trimmed.to_string(),
        }
    }

    /// Checks if a file path is a protected configuration file.
    pub fn is_path_protected(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy().replace('\\', "/");
        self.protected_paths.iter().any(|protected| {
            let protected_normalized = protected.replace('\\', "/");
            path_str.ends_with(&protected_normalized)
                || path_str.contains(&format!("/{}", protected_normalized))
        })
    }

    /// Validates whether a file write or edit is permissible.
    /// Strictly protects safety configurations from AI agent modifications.
    pub fn validate_file_mutation(
        &self,
        path: &Path,
        caller_is_agent: bool,
    ) -> Result<(), SafetyViolation> {
        if caller_is_agent && self.is_path_protected(path) {
            return Err(SafetyViolation::ProtectedFileMutationAttempt {
                path: path.to_string_lossy().to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standalone_safe_command_allowed() {
        let policy = CommandSafetyPolicy::default();
        assert_eq!(policy.evaluate_command("ls"), CommandSafetyResult::Allowed);
        assert_eq!(
            policy.evaluate_command("dir /s /b"),
            CommandSafetyResult::Allowed
        );
        assert_eq!(
            policy.evaluate_command("git status"),
            CommandSafetyResult::Allowed
        );
        assert_eq!(
            policy.evaluate_command("cargo check --workspace"),
            CommandSafetyResult::Allowed
        );
        assert_eq!(
            policy.evaluate_command("git.exe log -n 5"),
            CommandSafetyResult::Allowed
        );
    }

    #[test]
    fn test_chained_safe_command_requires_approval() {
        let policy = CommandSafetyPolicy::default();
        match policy.evaluate_command("ls && rm -rf /") {
            // Blocked keyword takes precedence or requires approval
            CommandSafetyResult::Blocked { .. } | CommandSafetyResult::RequiresApproval { .. } => {}
            other => panic!("Expected blocked or approval, got {:?}", other),
        }

        match policy.evaluate_command("dir; echo test") {
            CommandSafetyResult::RequiresApproval { reason, .. } => {
                assert!(reason.contains("chaining"));
            }
            other => panic!("Expected RequiresApproval, got {:?}", other),
        }

        match policy.evaluate_command("cat file.txt | grep foo") {
            CommandSafetyResult::RequiresApproval { reason, .. } => {
                assert!(reason.contains("chaining"));
            }
            other => panic!("Expected RequiresApproval, got {:?}", other),
        }
    }

    #[test]
    fn test_blocked_dangerous_keywords() {
        let policy = CommandSafetyPolicy::default();
        match policy.evaluate_command("rm -rf target") {
            CommandSafetyResult::Blocked { reason, .. } => {
                assert!(reason.contains("rm -rf"));
            }
            other => panic!("Expected Blocked, got {:?}", other),
        }

        match policy.evaluate_command("sudo apt update") {
            CommandSafetyResult::Blocked { reason, .. } => {
                assert!(reason.contains("sudo"));
            }
            other => panic!("Expected Blocked, got {:?}", other),
        }
    }

    #[test]
    fn test_tamper_proof_guard_protects_safety_file() {
        let policy = CommandSafetyPolicy::default();
        let safety_file = Path::new("C:/repo/.hydra/safety.json");

        assert!(policy.is_path_protected(safety_file));

        // Agent mutation must be blocked
        let agent_res = policy.validate_file_mutation(safety_file, true);
        assert!(agent_res.is_err());
        match agent_res.unwrap_err() {
            SafetyViolation::ProtectedFileMutationAttempt { path } => {
                assert!(path.contains("safety.json"));
            }
            other => panic!("Unexpected error: {:?}", other),
        }

        // Direct user mutation is permitted
        let user_res = policy.validate_file_mutation(safety_file, false);
        assert!(user_res.is_ok());
    }

    #[test]
    fn test_regular_file_mutation_allowed_for_agent() {
        let policy = CommandSafetyPolicy::default();
        let src_file = Path::new("src/main.rs");
        assert!(!policy.is_path_protected(src_file));
        assert!(policy.validate_file_mutation(src_file, true).is_ok());
    }

    #[test]
    fn test_save_load_from_workspace() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut policy = CommandSafetyPolicy::default();
        policy.safe_standalone_commands.push("custom_tool".to_string());

        policy.save_to_workspace(temp.path()).expect("save");
        let loaded = CommandSafetyPolicy::load_from_workspace(temp.path()).expect("load");
        assert!(loaded.safe_standalone_commands.contains(&"custom_tool".to_string()));
    }

    #[test]
    fn test_is_path_protected_windows_absolute() {
        let policy = CommandSafetyPolicy::default();
        let win_path1 = Path::new(r"C:\project\.hydra\safety.json");
        let win_path2 = Path::new(r"C:\project\hydra.json");
        let win_path3 = Path::new(r"C:\project\.hydra\goals.json");
        let safe_win_path = Path::new(r"C:\project\src\lib.rs");

        assert!(policy.is_path_protected(win_path1));
        assert!(policy.is_path_protected(win_path2));
        assert!(policy.is_path_protected(win_path3));
        assert!(!policy.is_path_protected(safe_win_path));
    }

    #[test]
    fn test_evaluate_command_subshell() {
        let policy = CommandSafetyPolicy::default();
        match policy.evaluate_command("echo $(whoami)") {
            CommandSafetyResult::RequiresApproval { reason, .. } => {
                assert!(reason.contains("subshells") || reason.contains("chaining"));
            }
            other => panic!("Expected RequiresApproval, got {:?}", other),
        }

        match policy.evaluate_command("echo `cat /etc/passwd`") {
            CommandSafetyResult::RequiresApproval { reason, .. } => {
                assert!(reason.contains("subshells") || reason.contains("chaining"));
            }
            other => panic!("Expected RequiresApproval, got {:?}", other),
        }
    }
}

