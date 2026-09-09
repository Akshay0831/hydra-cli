//! Strategic decision steering with goal tracking and human intervention seams.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Represents a viable architectural path in a decision seam (Phase 4.2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionOption {
    pub id: String,
    pub title: String,
    pub description: String,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
    pub blast_radius_files: Vec<String>,
}

/// Concise architectural brief presented to human or IPC client (Phase 4.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionBrief {
    pub decision_id: String,
    pub title: String,
    pub context: String,
    pub options: Vec<DecisionOption>,
}

impl DecisionBrief {
    /// Format brief into machine-dense Markdown summary.
    pub fn format_brief(&self) -> String {
        let mut out = Vec::new();
        out.push(format!("### STRATEGIC DECISION REQUIRED: {}", self.title));
        out.push(format!("Context: {}", self.context));
        out.push("\n| Option ID | Title | Blast Radius | Pros | Cons |".to_string());
        out.push("|---|---|---|---|---|".to_string());
        for opt in &self.options {
            out.push(format!(
                "| {} | {} | {} | {} | {} |",
                opt.id,
                opt.title,
                opt.blast_radius_files.join(", "),
                opt.pros.join("; "),
                opt.cons.join("; ")
            ));
        }
        out.join("\n")
    }
}

/// Strategic Human Decision Seam (Phase 4.2).
pub struct DecisionSeam;

impl DecisionSeam {
    /// Evaluates and selects an architectural path from a decision brief.
    pub fn resolve(brief: &DecisionBrief, selected_option_id: &str) -> Result<DecisionOption> {
        for opt in &brief.options {
            if opt.id == selected_option_id {
                return Ok(opt.clone());
            }
        }
        bail!(
            "Option '{}' not found in decision brief '{}'",
            selected_option_id,
            brief.decision_id
        );
    }
}

/// Model capability tiers for cognitive steering (Phase 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityTier {
    /// Extended thinking / deep reasoning models (Claude 3.5 Sonnet, GPT-4o, O1, Gemini 2.5 Pro).
    DeepReasoning,
    /// High-throughput fast models (GLM-4.5 Flash, Gemini Flash, Claude Haiku, Llama 3.3).
    FastThroughput,
    /// Budget fallbacks or small local weights.
    Fallback,
}

/// Cognitive Steering prompt generator (Phase 4.3).
pub struct CognitiveSteering;

impl CognitiveSteering {
    /// Augments the base prompt with tier-specific steering directives.
    pub fn steer(tier: CapabilityTier, base_prompt: &str, project_invariants: &str) -> String {
        let mut sections = Vec::new();

        match tier {
            CapabilityTier::DeepReasoning => {
                sections.push(
                    "### COGNITIVE STEERING: DEEP REASONING AUDIT TIER\n\
                     - Conduct systemic architectural audits before proposing edits.\n\
                     - Verify all 1-hop and 2-hop caller blast radiuses across the workspace.\n\
                     - Simulate edge-case failure modes and test regression vectors.\n\
                     - Maintain existing public API contracts and document non-obvious invariants."
                        .to_string(),
                );
            }
            CapabilityTier::FastThroughput | CapabilityTier::Fallback => {
                sections.push(
                    "### COGNITIVE STEERING: CONSTRAINED FAST-THROUGHPUT TIER\n\
                     - STRICT ANTI-DUPLICATION: Never create new *_utils.rs, *_helper.rs, or ad-hoc wrappers.\n\
                     - Only edit existing scoped files within the assigned partition.\n\
                     - Do NOT modify dependencies in Cargo.toml or package.json.\n\
                     - No narrative storytelling or explanatory comments in diffs.\n\
                     - Implement minimal, precise diffs matching AST signatures."
                        .to_string(),
                );
            }
        }

        if !project_invariants.is_empty() {
            sections.push(project_invariants.to_string());
        }

        sections.push(format!("### ASSIGNED TASK\n{}", base_prompt));
        sections.join("\n\n")
    }
}

/// Outcome of speculative execution (Phase 4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeculativeOutcome {
    /// Fast tier succeeded with clean invariants and passing tests.
    AcceptedFastTier,
    /// Fast tier failed; escalated to deep reasoning tier with diagnostic context.
    EscalatedToDeepReasoning { reason: String },
}

/// Speculative Execution & Cascading Escalation Engine (Phase 4.4).
pub struct SpeculativeEngine;

impl SpeculativeEngine {
    /// Evaluates whether a speculative fast-tier run can be accepted or requires escalation.
    pub fn evaluate_run(
        tests_passed: bool,
        has_invariant_violations: bool,
        patch_lines_churn: usize,
        max_allowed_churn: usize,
    ) -> SpeculativeOutcome {
        if !tests_passed {
            return SpeculativeOutcome::EscalatedToDeepReasoning {
                reason: "Unit tests failed during speculative fast-tier execution".to_string(),
            };
        }

        if has_invariant_violations {
            return SpeculativeOutcome::EscalatedToDeepReasoning {
                reason: "Architectural invariant violation detected in generated diff".to_string(),
            };
        }

        if patch_lines_churn > max_allowed_churn {
            return SpeculativeOutcome::EscalatedToDeepReasoning {
                reason: format!(
                    "Diff churn ({} lines) exceeded fast-tier boundary ({} lines)",
                    patch_lines_churn, max_allowed_churn
                ),
            };
        }

        SpeculativeOutcome::AcceptedFastTier
    }
}

/// Structured milestone in the Plan-Execute-Verify loop (Phase 4.5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanMilestone {
    pub step: usize,
    pub title: String,
    pub target_scope: Vec<String>,
    pub completed: bool,
}

/// Multi-Turn Plan-Execute-Verify Review Loop (Phase 4.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExecuteVerifyWorkflow {
    pub intent: String,
    pub milestones: Vec<PlanMilestone>,
}

impl PlanExecuteVerifyWorkflow {
    /// Decomposes a macro task intent into structured milestones.
    pub fn decompose(intent: &str, target_files: &[String]) -> Self {
        let mut milestones = Vec::new();

        // 1. AST & Invariant Inspection milestone
        milestones.push(PlanMilestone {
            step: 1,
            title: "Inspect target AST signatures and verify scope constraints".to_string(),
            target_scope: target_files.to_vec(),
            completed: false,
        });

        // 2. Focused Partition Code Generation milestone
        milestones.push(PlanMilestone {
            step: 2,
            title: "Execute targeted code edit without introducing wrappers".to_string(),
            target_scope: target_files.to_vec(),
            completed: false,
        });

        // 3. Verification & Invariant Audit milestone
        milestones.push(PlanMilestone {
            step: 3,
            title: "Run compiler checks, linter invariant gates, and test suites".to_string(),
            target_scope: target_files.to_vec(),
            completed: false,
        });

        Self {
            intent: intent.to_string(),
            milestones,
        }
    }

    /// Marks a milestone completed.
    pub fn complete_milestone(&mut self, step: usize) {
        if let Some(m) = self.milestones.iter_mut().find(|m| m.step == step) {
            m.completed = true;
        }
    }

    /// Returns true if all milestones are verified and completed.
    pub fn is_fully_verified(&self) -> bool {
        self.milestones.iter().all(|m| m.completed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decision_seam_resolution() {
        let brief = DecisionBrief {
            decision_id: "DEC-001".to_string(),
            title: "Storage Backend Strategy".to_string(),
            context: "Evaluating index caching for daemon".to_string(),
            options: vec![
                DecisionOption {
                    id: "sqlite".to_string(),
                    title: "SQLite WAL Cache".to_string(),
                    description: "Persistent disk cache".to_string(),
                    pros: vec!["Persists across reboots".to_string()],
                    cons: vec!["File I/O overhead".to_string()],
                    blast_radius_files: vec!["crates/hydra-matrix/src/lib.rs".to_string()],
                },
                DecisionOption {
                    id: "in-memory".to_string(),
                    title: "Pure RAM Cache".to_string(),
                    description: "Volatile memory hash map".to_string(),
                    pros: vec!["Microsecond speed".to_string()],
                    cons: vec!["Cold start on restart".to_string()],
                    blast_radius_files: vec![],
                },
            ],
        };

        let formatted = brief.format_brief();
        assert!(formatted.contains("DEC-001") || formatted.contains("Storage Backend Strategy"));
        assert!(formatted.contains("sqlite"));

        let chosen = DecisionSeam::resolve(&brief, "sqlite").unwrap();
        assert_eq!(chosen.id, "sqlite");

        assert!(DecisionSeam::resolve(&brief, "invalid").is_err());
    }

    #[test]
    fn test_cognitive_steering_tiers() {
        let prompt = "Implement context skeletonizer";
        let invariants = "- INVARIANT: No unwrap()";

        let deep = CognitiveSteering::steer(CapabilityTier::DeepReasoning, prompt, invariants);
        assert!(deep.contains("DEEP REASONING AUDIT TIER"));
        assert!(deep.contains("No unwrap()"));

        let fast = CognitiveSteering::steer(CapabilityTier::FastThroughput, prompt, invariants);
        assert!(fast.contains("CONSTRAINED FAST-THROUGHPUT TIER"));
        assert!(fast.contains("STRICT ANTI-DUPLICATION"));

        let fallback = CognitiveSteering::steer(CapabilityTier::Fallback, prompt, invariants);
        assert!(fallback.contains("CONSTRAINED FAST-THROUGHPUT TIER"));
        assert!(fallback.contains("STRICT ANTI-DUPLICATION"));
    }

    #[test]
    fn test_decision_brief_serde() {
        let brief = DecisionBrief {
            decision_id: "DEC-TEST".to_string(),
            title: "Test Decision".to_string(),
            context: "Testing context".to_string(),
            options: vec![DecisionOption {
                id: "opt-1".to_string(),
                title: "Option 1".to_string(),
                description: "Description 1".to_string(),
                pros: vec!["Pro 1".to_string()],
                cons: vec!["Con 1".to_string()],
                blast_radius_files: vec!["file.rs".to_string()],
            }],
        };

        let json = serde_json::to_string(&brief).unwrap();
        let deserialized: DecisionBrief = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.decision_id, "DEC-TEST");
        assert_eq!(deserialized.options.len(), 1);
    }

    #[test]
    fn test_speculative_escalation() {
        // Successful fast tier run
        let res = SpeculativeEngine::evaluate_run(true, false, 50, 100);
        assert_eq!(res, SpeculativeOutcome::AcceptedFastTier);

        // Test failure causes escalation
        let res_failed_test = SpeculativeEngine::evaluate_run(false, false, 50, 100);
        assert!(matches!(
            res_failed_test,
            SpeculativeOutcome::EscalatedToDeepReasoning { .. }
        ));

        // Invariant violation causes escalation
        let res_invariant = SpeculativeEngine::evaluate_run(true, true, 50, 100);
        assert!(matches!(
            res_invariant,
            SpeculativeOutcome::EscalatedToDeepReasoning { .. }
        ));

        // Excessive churn causes escalation
        let res_churn = SpeculativeEngine::evaluate_run(true, false, 150, 100);
        assert!(matches!(
            res_churn,
            SpeculativeOutcome::EscalatedToDeepReasoning { .. }
        ));
    }

    #[test]
    fn test_plan_execute_verify_workflow() {
        let targets = vec!["src/lib.rs".to_string()];
        let mut workflow = PlanExecuteVerifyWorkflow::decompose("Add caching", &targets);

        assert_eq!(workflow.milestones.len(), 3);
        assert!(!workflow.is_fully_verified());

        workflow.complete_milestone(1);
        workflow.complete_milestone(2);
        assert!(!workflow.is_fully_verified());

        workflow.complete_milestone(3);
        assert!(workflow.is_fully_verified());
    }
}
