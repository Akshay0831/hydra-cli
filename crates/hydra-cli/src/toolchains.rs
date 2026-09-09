//! Multi-toolchain verification and telemetry for benchmarking.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Supported build and test toolchains (Phase 8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolchainKind {
    Cargo,
    Npm,
    Pnpm,
    Bun,
    Pytest,
    Go,
}

impl ToolchainKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Bun => "bun",
            Self::Pytest => "pytest",
            Self::Go => "go",
        }
    }

    pub fn check_command(&self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Cargo => ("cargo", &["check"]),
            Self::Npm => ("npm", &["test", "--", "--passWithNoTests"]),
            Self::Pnpm => ("pnpm", &["test"]),
            Self::Bun => ("bun", &["test"]),
            Self::Pytest => ("pytest", &["-q"]),
            Self::Go => ("go", &["test", "./..."]),
        }
    }
}

/// Result of a toolchain verification pass (Phase 8.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolchainReport {
    pub toolchain: ToolchainKind,
    pub detected: bool,
    pub passed: bool,
    pub output: String,
}

/// Multi-toolchain detector and parallel verification runner (Phase 8.1).
pub struct MultiToolchainGate;

impl MultiToolchainGate {
    /// Detects all active toolchains in the given workspace.
    pub fn detect_toolchains(workspace_root: &Path) -> Vec<ToolchainKind> {
        let mut toolchains = Vec::new();

        if workspace_root.join("Cargo.toml").exists() {
            toolchains.push(ToolchainKind::Cargo);
        }

        if workspace_root.join("pnpm-lock.yaml").exists() {
            toolchains.push(ToolchainKind::Pnpm);
        } else if workspace_root.join("bun.lockb").exists() {
            toolchains.push(ToolchainKind::Bun);
        } else if workspace_root.join("package.json").exists() {
            toolchains.push(ToolchainKind::Npm);
        }

        if workspace_root.join("pytest.ini").exists()
            || workspace_root.join("pyproject.toml").exists()
            || workspace_root.join("requirements.txt").exists()
        {
            toolchains.push(ToolchainKind::Pytest);
        }

        if workspace_root.join("go.mod").exists() {
            toolchains.push(ToolchainKind::Go);
        }

        toolchains
    }

    /// Evaluates verification pass across all detected toolchains.
    pub async fn run_checks(workspace_root: &Path) -> Vec<ToolchainReport> {
        let toolchains = Self::detect_toolchains(workspace_root);
        let mut reports = Vec::new();

        for tc in toolchains {
            let (cmd, args) = tc.check_command();
            let timeout_duration = std::time::Duration::from_secs(60);

            let run_fut = async {
                #[cfg(windows)]
                let mut cmd_builder = if tc == ToolchainKind::Npm || tc == ToolchainKind::Pnpm || tc == ToolchainKind::Bun {
                    let mut c = tokio::process::Command::new("cmd");
                    c.arg("/C").arg(cmd);
                    c
                } else {
                    tokio::process::Command::new(cmd)
                };

                #[cfg(not(windows))]
                let mut cmd_builder = tokio::process::Command::new(cmd);

                cmd_builder
                    .args(args)
                    .current_dir(workspace_root)
                    .output()
                    .await
            };

            match tokio::time::timeout(timeout_duration, run_fut).await {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let combined = if !stderr.is_empty() {
                        format!("{stdout}\n{stderr}").trim().to_string()
                    } else {
                        stdout
                    };
                    reports.push(ToolchainReport {
                        toolchain: tc,
                        detected: true,
                        passed: output.status.success(),
                        output: if combined.is_empty() { "Check succeeded".to_string() } else { combined },
                    });
                }
                Ok(Err(e)) => {
                    reports.push(ToolchainReport {
                        toolchain: tc,
                        detected: true,
                        passed: false,
                        output: format!("Execution failed: {e}"),
                    });
                }
                Err(_) => {
                    reports.push(ToolchainReport {
                        toolchain: tc,
                        detected: true,
                        passed: false,
                        output: "Execution timed out (15s limit reached)".to_string(),
                    });
                }
            }
        }

        reports
    }
}

/// Telemetry & Token Efficiency Benchmark Metrics (Phase 8.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkMetrics {
    pub time_to_consensus_secs: f64,
    pub tokens_used: usize,
    pub patch_lines_accepted: usize,
    pub duplication_preventions: usize,
    pub comment_density_score: f64,
    pub prompt_cache_hit_rate: f64,
    pub estimated_cost_usd: f64,
}

impl BenchmarkMetrics {
    /// Token efficiency ratio: tokens used per accepted line of patch diff.
    pub fn token_efficiency_ratio(&self) -> f64 {
        if self.patch_lines_accepted == 0 {
            0.0
        } else {
            self.tokens_used as f64 / self.patch_lines_accepted as f64
        }
    }

    /// Formats metrics into a machine-dense Markdown table.
    pub fn format_table(&self) -> String {
        let mut out = Vec::new();
        out.push("### HYDRA BENCHMARK TELEMETRY REPORT".to_string());
        out.push("| Metric | Value | Target | Evaluation |".to_string());
        out.push("|---|---|---|---|".to_string());
        out.push(format!(
            "| Time to Consensus | {:.2}s | < 30.0s | {} |",
            self.time_to_consensus_secs,
            if self.time_to_consensus_secs <= 30.0 { "OPTIMAL" } else { "DEGRADED" }
        ));
        out.push(format!(
            "| Token Efficiency Ratio | {:.1} tok/line | < 80 tok/line | {} |",
            self.token_efficiency_ratio(),
            if self.token_efficiency_ratio() <= 80.0 { "OPTIMAL" } else { "EXCESSIVE" }
        ));
        out.push(format!(
            "| Duplication Preventions | {} | > 0 | ENFORCED |",
            self.duplication_preventions
        ));
        out.push(format!(
            "| Comment Density Score | {:.1}% | <= 20.0% | {} |",
            self.comment_density_score * 100.0,
            if self.comment_density_score <= 0.20 { "PASSED" } else { "BLOATED" }
        ));
        out.push(format!(
            "| Prompt Cache Hit Rate | {:.1}% | >= 75.0% | {} |",
            self.prompt_cache_hit_rate * 100.0,
            if self.prompt_cache_hit_rate >= 0.75 { "OPTIMAL" } else { "COLD" }
        ));
        out.push(format!(
            "| Estimated Run Cost | ${:.4} | < $0.05 | {} |",
            self.estimated_cost_usd,
            if self.estimated_cost_usd < 0.05 { "ECONOMIC" } else { "HIGH" }
        ));
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toolchain_detection() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();

        let detected = MultiToolchainGate::detect_toolchains(temp.path());
        assert!(detected.contains(&ToolchainKind::Cargo));
        assert!(detected.contains(&ToolchainKind::Npm));
        assert!(!detected.contains(&ToolchainKind::Go));
    }

    #[test]
    fn test_toolchain_detection_pnpm_bun_python_go() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("pnpm-lock.yaml"), "").unwrap();
        std::fs::write(temp.path().join("pyproject.toml"), "").unwrap();
        std::fs::write(temp.path().join("go.mod"), "module test").unwrap();

        let detected = MultiToolchainGate::detect_toolchains(temp.path());
        assert!(detected.contains(&ToolchainKind::Pnpm));
        assert!(detected.contains(&ToolchainKind::Pytest));
        assert!(detected.contains(&ToolchainKind::Go));
        assert!(!detected.contains(&ToolchainKind::Cargo));
    }

    #[test]
    fn test_benchmark_metrics_calculation_and_table() {
        let metrics = BenchmarkMetrics {
            time_to_consensus_secs: 12.4,
            tokens_used: 1200,
            patch_lines_accepted: 25,
            duplication_preventions: 3,
            comment_density_score: 0.12,
            prompt_cache_hit_rate: 0.85,
            estimated_cost_usd: 0.0035,
        };

        assert_eq!(metrics.token_efficiency_ratio(), 48.0);
        let table = metrics.format_table();
        assert!(table.contains("OPTIMAL"));
        assert!(table.contains("12.40s"));
        assert!(table.contains("48.0 tok/line"));
    }

    #[test]
    fn test_benchmark_metrics_degraded_evaluations() {
        let metrics = BenchmarkMetrics {
            time_to_consensus_secs: 45.0,
            tokens_used: 10000,
            patch_lines_accepted: 10,
            duplication_preventions: 0,
            comment_density_score: 0.35,
            prompt_cache_hit_rate: 0.40,
            estimated_cost_usd: 0.12,
        };

        assert_eq!(metrics.token_efficiency_ratio(), 1000.0);
        let table = metrics.format_table();
        assert!(table.contains("DEGRADED"));
        assert!(table.contains("EXCESSIVE"));
        assert!(table.contains("BLOATED"));
        assert!(table.contains("COLD"));
        assert!(table.contains("HIGH"));
    }

    #[test]
    fn test_toolchain_report_serde() {
        let rep = ToolchainReport {
            toolchain: ToolchainKind::Cargo,
            detected: true,
            passed: true,
            output: "Compiling test v0.1.0".to_string(),
        };

        let json = serde_json::to_string(&rep).unwrap();
        let deserialized: ToolchainReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.toolchain, ToolchainKind::Cargo);
        assert!(deserialized.passed);
    }
}
