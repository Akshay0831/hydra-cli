//! AI-dense documentation interleaving adhering to docs/profiles/ai-dense.toml.

use crate::prompt_router::ProjectGoalRegistry;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Documentation inclusion strategy for prompt assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DocInclusionStrategy {
    /// Concise invariant tables adhering to ai-dense.toml (Default).
    #[default]
    AiDenseConcise,
    /// Macro invariants only from .hydra/goals.json.
    MinimalInvariants,
    /// Disables doc interleaving.
    None,
}

/// Interface for repository documentation extraction.
pub trait DocContextInjector: Send + Sync {
    fn extract_documentation(&self, workspace_root: &Path, intent: &str) -> Vec<String>;
}

/// Factory for doc injectors.
pub fn create_doc_injector(strategy: DocInclusionStrategy) -> Box<dyn DocContextInjector> {
    match strategy {
        DocInclusionStrategy::AiDenseConcise => Box::new(AiDenseConciseDocInjector),
        DocInclusionStrategy::MinimalInvariants => Box::new(MinimalInvariantsDocInjector),
        DocInclusionStrategy::None => Box::new(NoDocInjector),
    }
}

/// Machine-dense doc injector adhering to docs/profiles/ai-dense.toml.
#[derive(Debug, Clone, Default)]
pub struct AiDenseConciseDocInjector;

impl AiDenseConciseDocInjector {
    pub fn new() -> Self {
        Self
    }
}

impl DocContextInjector for AiDenseConciseDocInjector {
    fn extract_documentation(&self, workspace_root: &Path, intent: &str) -> Vec<String> {
        let mut docs = Vec::new();
        let tokens: Vec<String> = intent
            .split_whitespace()
            .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() >= 3)
            .collect();

        // Architectural invariants from registry with intent relevance
        let registry = ProjectGoalRegistry::load_or_default_sync(workspace_root);
        let header = if tokens.is_empty() {
            registry.format_invariant_header()
        } else {
            let matched_invs: Vec<String> = registry
                .invariants
                .iter()
                .filter(|inv| tokens.iter().any(|t| inv.to_lowercase().contains(t)))
                .cloned()
                .collect();
            if matched_invs.is_empty() {
                registry.format_invariant_header()
            } else {
                format!(
                    "ARCHITECTURAL INVARIANTS (INTENT MATCHED):\n{}",
                    matched_invs.iter().map(|inv| format!("- {inv}")).collect::<Vec<_>>().join("\n")
                )
            }
        };
        if !header.is_empty() {
            docs.push(header);
        }

        // Subsystem contract tables from root README
        let readme = workspace_root.join("README.md");
        if let Ok(content) = std::fs::read_to_string(&readme) {
            let mut table = Vec::new();
            let mut in_table = false;
            let mut header_lines = Vec::new();
            for line in content.lines() {
                if line.starts_with("| Component") || line.starts_with("| Capability") {
                    in_table = true;
                    header_lines.push(line);
                } else if in_table {
                    if line.starts_with("|-") || line.starts_with("| -") {
                        header_lines.push(line);
                    } else if line.starts_with('|') {
                        let line_lower = line.to_lowercase();
                        if tokens.is_empty() || tokens.iter().any(|t| line_lower.contains(t)) {
                            table.push(line);
                        }
                    } else {
                        break;
                    }
                }
            }
            if !table.is_empty() {
                let mut full_table = header_lines;
                full_table.extend(table);
                docs.push(format!("### REPOSITORY SUBSYSTEM CONTRACTS\n{}", full_table.join("\n")));
            }
        }

        docs
    }
}

/// Injects high-level invariants without subsystem tables.
#[derive(Debug, Clone, Default)]
pub struct MinimalInvariantsDocInjector;

impl DocContextInjector for MinimalInvariantsDocInjector {
    fn extract_documentation(&self, workspace_root: &Path, intent: &str) -> Vec<String> {
        let registry = ProjectGoalRegistry::load_or_default_sync(workspace_root);
        let tokens: Vec<String> = intent
            .split_whitespace()
            .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() >= 3)
            .collect();
        let header = if tokens.is_empty() {
            registry.format_invariant_header()
        } else {
            let matched_invs: Vec<String> = registry
                .invariants
                .iter()
                .filter(|inv| tokens.iter().any(|t| inv.to_lowercase().contains(t)))
                .cloned()
                .collect();
            if matched_invs.is_empty() {
                registry.format_invariant_header()
            } else {
                format!(
                    "ARCHITECTURAL INVARIANTS (INTENT MATCHED):\n{}",
                    matched_invs.iter().map(|inv| format!("- {inv}")).collect::<Vec<_>>().join("\n")
                )
            }
        };
        if header.is_empty() {
            Vec::new()
        } else {
            vec![header]
        }
    }
}

/// No-op injector for empty doc context.
#[derive(Debug, Clone, Default)]
pub struct NoDocInjector;

impl DocContextInjector for NoDocInjector {
    fn extract_documentation(&self, _workspace_root: &Path, _intent: &str) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_inclusion_strategy_defaults_to_ai_dense() {
        assert_eq!(
            DocInclusionStrategy::default(),
            DocInclusionStrategy::AiDenseConcise
        );
    }

    #[test]
    fn test_no_doc_injector_returns_empty() {
        let injector = NoDocInjector;
        let docs = injector.extract_documentation(Path::new("."), "fix bug");
        assert!(docs.is_empty());
    }

    #[test]
    fn test_ai_dense_injector_extracts_goals_invariants() {
        let temp = tempfile::tempdir().expect("tempdir");
        let hydra_dir = temp.path().join(".hydra");
        std::fs::create_dir_all(&hydra_dir).expect("mkdir");
        let goals_file = hydra_dir.join("goals.json");
        std::fs::write(
            &goals_file,
            r#"{"project_name":"test","primary_goals":[],"invariants":["Zero unwrap"],"forbidden_patterns":[]}"#,
        )
        .expect("write goals");

        let injector = AiDenseConciseDocInjector;
        let docs = injector.extract_documentation(temp.path(), "refactor");
        assert_eq!(docs.len(), 1);
        assert!(docs[0].contains("ARCHITECTURAL INVARIANTS"));
        assert!(docs[0].contains("Zero unwrap"));
    }

    #[test]
    fn test_ai_dense_injector_intent_matching() {
        let temp = tempfile::tempdir().expect("tempdir");
        let hydra_dir = temp.path().join(".hydra");
        std::fs::create_dir_all(&hydra_dir).expect("mkdir");
        let goals_file = hydra_dir.join("goals.json");
        std::fs::write(
            &goals_file,
            r#"{"project_name":"test","primary_goals":[],"invariants":["Zero unwrap","Deterministic sorting"],"forbidden_patterns":[]}"#,
        )
        .expect("write goals");

        let injector = AiDenseConciseDocInjector;
        let docs = injector.extract_documentation(temp.path(), "fix unwrap panic");
        assert_eq!(docs.len(), 1);
        assert!(docs[0].contains("INTENT MATCHED"));
        assert!(docs[0].contains("Zero unwrap"));
        assert!(!docs[0].contains("Deterministic sorting"));
    }

    #[test]
    fn test_minimal_invariants_injector_filters_by_intent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let hydra_dir = temp.path().join(".hydra");
        std::fs::create_dir_all(&hydra_dir).expect("mkdir");
        let goals_file = hydra_dir.join("goals.json");
        std::fs::write(
            &goals_file,
            r#"{"project_name":"test","primary_goals":[],"invariants":["Zero unwrap","Deterministic sorting"],"forbidden_patterns":[]}"#,
        )
        .expect("write goals");

        let injector = MinimalInvariantsDocInjector;
        let docs = injector.extract_documentation(temp.path(), "sorting algorithm");
        assert_eq!(docs.len(), 1);
        assert!(docs[0].contains("INTENT MATCHED"));
        assert!(docs[0].contains("Deterministic sorting"));
        assert!(!docs[0].contains("Zero unwrap"));

        // Fallback when no keywords match
        let fallback_docs = injector.extract_documentation(temp.path(), "completely unrelated query");
        assert_eq!(fallback_docs.len(), 1);
        assert!(fallback_docs[0].contains("ARCHITECTURAL INVARIANTS"));
        assert!(fallback_docs[0].contains("Zero unwrap"));
        assert!(fallback_docs[0].contains("Deterministic sorting"));
    }

    #[test]
    fn test_create_doc_injector_factory() {
        let ai_dense = create_doc_injector(DocInclusionStrategy::AiDenseConcise);
        let docs_ai = ai_dense.extract_documentation(Path::new("nonexistent"), "");
        assert_eq!(docs_ai.len(), 1);
        assert!(docs_ai[0].contains("ARCHITECTURAL INVARIANTS"));

        let minimal = create_doc_injector(DocInclusionStrategy::MinimalInvariants);
        let docs_min = minimal.extract_documentation(Path::new("nonexistent"), "");
        assert_eq!(docs_min.len(), 1);
        assert!(docs_min[0].contains("ARCHITECTURAL INVARIANTS"));

        let none = create_doc_injector(DocInclusionStrategy::None);
        assert_eq!(none.extract_documentation(Path::new("nonexistent"), "").len(), 0);
    }
}

