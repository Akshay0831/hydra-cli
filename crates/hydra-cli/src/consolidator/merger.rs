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

    /// Filters out conversational comments and code logic restatements.
    pub fn filter_comment_density(patch_diff: &str, max_comment_ratio: f64) -> Result<String, String> {
        let mut code_lines = 0usize;
        let mut comment_lines = 0usize;
        let mut filtered_lines = Vec::new();

        for line in patch_diff.lines() {
            let trimmed = line.trim_start();
            // Check additions in unified diff format (+...)
            if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
                let content = trimmed[1..].trim();
                if content.starts_with("//") || content.starts_with("/*") || content.starts_with('*') {
                    // Check for obvious conversational narrative filler
                    let lower = content.to_lowercase();
                    if lower.contains("now let's")
                        || lower.contains("here we are")
                        || lower.contains("in this section")
                        || lower.contains("as requested")
                        || lower.contains("step 1:")
                        || lower.contains("step 2:")
                        || lower.contains("first we need to")
                    {
                        // Drop storytelling line
                        continue;
                    }
                    comment_lines += 1;
                } else if !content.is_empty() {
                    code_lines += 1;
                }
            }
            filtered_lines.push(line);
        }

        let total = code_lines + comment_lines;
        if total > 10 && (comment_lines as f64 / total as f64) > max_comment_ratio {
            return Err(format!(
                "REJECTED: Patch exceeds max comment ratio ({:.1}% > {:.1}%). Remove verbose conversational comments.",
                (comment_lines as f64 / total as f64) * 100.0,
                max_comment_ratio * 100.0
            ));
        }

        Ok(filtered_lines.join("\n"))
    }

    /// Validates patch structure: blocks wrapper files, forbidden idioms, and dependency modifications.
    pub fn validate_patch_invariants(patch_diff: &str) -> Result<(), String> {
        for line in patch_diff.lines() {
            let trimmed = line.trim();

            // Phase 1.4: Dependency Import Lock Gate
            if (trimmed.starts_with("--- a/") || trimmed.starts_with("+++ b/"))
                && (trimmed.contains("Cargo.toml") || trimmed.contains("package.json") || trimmed.contains("requirements.txt"))
            {
                return Err(format!(
                    "REJECTED: Dependency file modification detected in patch: '{}'. Dependency changes require explicit operator approval.",
                    trimmed
                ));
            }

            // Phase 1.2: Single Execution Gateway Enforcer (block new runner/wrapper files)
            if trimmed.starts_with("+++ b/") {
                let target = &trimmed[6..];
                if target.ends_with("_runner.rs") || target.ends_with("_helper.rs") || target.ends_with("_utils.rs") {
                    return Err(format!(
                        "REJECTED: Ad-hoc wrapper file '{}' blocked. Route all changes through established Hydra adapter facades.",
                        target
                    ));
                }
            }

            // Phase 1.3: AST-Level Redundancy & Idiom Scanner
            if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
                let added = trimmed[1..].trim();
                // Block silent regression shortcuts (forbidden unwrap/panic in library code)
                if (added.contains(".unwrap()") || added.contains("panic!("))
                    && !patch_diff.contains("/tests/")
                    && !patch_diff.contains("#[cfg(test)]")
                {
                    return Err(format!(
                        "REJECTED: Forbidden panic/unwrap idiom in non-test patch addition: '{}'. Use proper Result error handling.",
                        added
                    ));
                }
            }
        }

        Ok(())
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

    #[test]
    fn test_filter_comment_density_rejects_narrative_filler() {
        let diff_with_filler = r#"
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,6 @@
+// Now let's implement the helper function as requested
+// Step 1: Initialize the counter
+// Here we are setting up the state
+pub fn count() -> usize { 42 }
"#;
        let filtered = Consolidator::filter_comment_density(diff_with_filler, 0.50).unwrap();
        assert!(!filtered.contains("Now let's implement"));
        assert!(!filtered.contains("Step 1:"));
        assert!(filtered.contains("pub fn count() -> usize { 42 }"));
    }

    #[test]
    fn test_validate_patch_invariants_blocks_dependency_modifications() {
        let dep_diff = "--- a/Cargo.toml\n+++ b/Cargo.toml\n+rand = \"0.8\"\n";
        assert!(Consolidator::validate_patch_invariants(dep_diff).is_err());
    }

    #[test]
    fn test_validate_patch_invariants_blocks_spurious_wrappers() {
        let wrapper_diff = "+++ b/src/my_runner.rs\n+pub fn run() {}\n";
        assert!(Consolidator::validate_patch_invariants(wrapper_diff).is_err());
    }

    #[test]
    fn test_validate_patch_invariants_blocks_unwrap_in_library_code() {
        let bad_diff = "+++ b/src/core.rs\n+let x = res.unwrap();\n";
        assert!(Consolidator::validate_patch_invariants(bad_diff).is_err());

        let test_diff = "+++ b/tests/integration.rs\n+let x = res.unwrap();\n";
        assert!(Consolidator::validate_patch_invariants(test_diff).is_ok());
    }
}

