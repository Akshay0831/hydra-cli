//! Utility functions for CLI parsing

use anyhow::Result;
use hydra_dag::ExecutionStrategy;

pub fn parse_execution_strategy(strategy: &str) -> Result<ExecutionStrategy> {
    match strategy {
        "sequential" => Ok(ExecutionStrategy::Sequential),
        "concurrent" => Ok(ExecutionStrategy::CollectFailures),
        "parallel" => Ok(ExecutionStrategy::FailFast),
        _ => Err(anyhow::anyhow!("Unknown execution strategy: {}", strategy)),
    }
}

pub fn parse_languages(langs: &[String]) -> Vec<String> {
    langs
        .iter()
        .filter_map(|lang| match lang.as_str() {
            "rs" => Some("rs".to_string()),
            "js" => Some("js".to_string()),
            "jsx" => Some("jsx".to_string()),
            "ts" => Some("ts".to_string()),
            "tsx" => Some("tsx".to_string()),
            _ => None,
        })
        .collect()
}
