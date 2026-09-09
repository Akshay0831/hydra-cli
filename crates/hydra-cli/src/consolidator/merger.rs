//! crates/hydra-cli/src/consolidator/merger.rs
//!
//! Log deduplication, feedback ranking, and diff reconciliation engine.
//! Evaluates consensus among Coder, Tester, and Reviewer outputs.

use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingPriority {
    CompilerError = 1,
    TestFailure = 2,
    SecurityVulnerability = 3,
    StyleWarning = 4,
}

impl std::fmt::Display for FindingPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FindingPriority::CompilerError => write!(f, "COMPILER ERROR"),
            FindingPriority::TestFailure => write!(f, "TEST FAILURE"),
            FindingPriority::SecurityVulnerability => write!(f, "SECURITY RISK"),
            FindingPriority::StyleWarning => write!(f, "STYLE NOTE"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConsolidatedFinding {
    pub priority: FindingPriority,
    pub message: String,
    pub file_target: Option<PathBuf>,
    pub line: Option<usize>,
}

pub struct Consolidator;

impl Consolidator {
    /// Strips repetitive stack traces and extracts actionable compiler diagnostics.
    pub fn deduplicate_logs(raw_stderr: &str, raw_stdout: &str) -> Vec<ConsolidatedFinding> {
        let mut findings = Vec::new();
        let mut seen_messages = HashSet::new();

        let combined = format!("{}\n{}", raw_stderr, raw_stdout);
        for line in combined.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || seen_messages.contains(trimmed) {
                continue;
            }

            if trimmed.contains("error[E") || trimmed.contains("error:") {
                seen_messages.insert(trimmed.to_string());
                findings.push(ConsolidatedFinding {
                    priority: FindingPriority::CompilerError,
                    message: trimmed.to_string(),
                    file_target: None,
                    line: None,
                });
            } else if trimmed.contains("FAILED") || trimmed.contains("assertion failed") {
                seen_messages.insert(trimmed.to_string());
                findings.push(ConsolidatedFinding {
                    priority: FindingPriority::TestFailure,
                    message: trimmed.to_string(),
                    file_target: None,
                    line: None,
                });
            } else if trimmed.contains("warning:") {
                seen_messages.insert(trimmed.to_string());
                findings.push(ConsolidatedFinding {
                    priority: FindingPriority::StyleWarning,
                    message: trimmed.to_string(),
                    file_target: None,
                    line: None,
                });
            }
        }

        findings.sort_by_key(|f| f.priority);
        findings
    }

    /// Reconciles diffs across partitions into a single unified consensus patch.
    pub fn reconcile_diffs(diffs: &[String]) -> Result<String> {
        let non_empty: Vec<&str> = diffs
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if non_empty.is_empty() {
            return Ok(String::new());
        }

        Ok(non_empty.join("\n\n"))
    }

    /// Evaluates whether the worker trio reached unanimous consensus.
    pub fn evaluate_consensus(
        test_passed: bool,
        review_approved: bool,
        findings: &[ConsolidatedFinding],
    ) -> bool {
        let has_blocking_errors = findings
            .iter()
            .any(|f| f.priority == FindingPriority::CompilerError || f.priority == FindingPriority::TestFailure);

        test_passed && review_approved && !has_blocking_errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deduplicate_logs_identifies_errors() {
        let stderr = "error[E0308]: mismatched types\nwarning: unused variable\n";
        let stdout = "running 1 test\ntest my_test ... FAILED\n";
        let findings = Consolidator::deduplicate_logs(stderr, stdout);

        assert_eq!(findings.len(), 3);
        assert_eq!(findings[0].priority, FindingPriority::CompilerError);
        assert_eq!(findings[1].priority, FindingPriority::TestFailure);
        assert_eq!(findings[2].priority, FindingPriority::StyleWarning);
    }

    #[test]
    fn test_reconcile_diffs_combines_non_empty_patches() {
        let diff1 = "diff --git a/file1.rs b/file1.rs\n+fn hello() {}\n".to_string();
        let diff2 = "diff --git a/file2.rs b/file2.rs\n+fn world() {}\n".to_string();
        let empty = "   \n".to_string();

        let merged = Consolidator::reconcile_diffs(&[diff1, empty, diff2]).unwrap();
        assert!(merged.contains("file1.rs"));
        assert!(merged.contains("file2.rs"));
    }

    #[test]
    fn test_evaluate_consensus_logic() {
        assert!(Consolidator::evaluate_consensus(true, true, &[]));
        assert!(!Consolidator::evaluate_consensus(false, true, &[]));
        assert!(!Consolidator::evaluate_consensus(true, false, &[]));

        let blocking = vec![ConsolidatedFinding {
            priority: FindingPriority::CompilerError,
            message: "compile error".to_string(),
            file_target: None,
            line: None,
        }];
        assert!(!Consolidator::evaluate_consensus(true, true, &blocking));
    }
}

