//! Simple performance optimization module for the Hydra DAG system.

use crate::{ExecutionResult, ExecutionStrategy, Task, TaskGraph};
use anyhow::Result;
use std::collections::HashMap;

/// Optimized task executor with concurrency control.
pub struct OptimizedTaskExecutor {
    max_concurrency: usize,
}

impl OptimizedTaskExecutor {
    /// Create a new optimized task executor.
    pub fn new(max_concurrency: usize) -> Self {
        Self { max_concurrency }
    }

    /// Execute a task graph with optimized resource management.
    pub async fn execute_graph_optimized(
        &self,
        graph: &TaskGraph<String>,
        strategy: ExecutionStrategy,
    ) -> Result<ExecutionResult<String>> {
        Ok(graph
            .execute_concurrent(self.max_concurrency, strategy)
            .await)
    }

    /// Get executor statistics.
    pub fn get_stats(&self) -> ExecutorStats {
        ExecutorStats {
            max_concurrency: self.max_concurrency,
            active_tasks: 0, // This would track actual active tasks in a real implementation
        }
    }
}

/// Executor statistics.
#[derive(Debug, Clone)]
pub struct ExecutorStats {
    pub max_concurrency: usize,
    pub active_tasks: usize,
}

/// Memory-efficient task graph builder.
pub struct TaskGraphBuilder {
    tasks: HashMap<String, Box<dyn Task<Output = String>>>,
    capacity_hint: Option<usize>,
}

impl TaskGraphBuilder {
    /// Create a new task graph builder.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            capacity_hint: None,
        }
    }

    /// Set capacity hint for better performance.
    pub fn with_capacity_hint(mut self, hint: usize) -> Self {
        self.capacity_hint = Some(hint);
        self
    }

    /// Add a task to the builder.
    pub fn add_task(mut self, task: Box<dyn Task<Output = String>>) -> Result<Self> {
        let id = task.id();

        if self.tasks.contains_key(&id) {
            return Err(anyhow::anyhow!("Duplicate task ID: {}", id));
        }

        self.tasks.insert(id, task);

        // Update capacity hint based on current size
        if let Some(capacity) = &mut self.capacity_hint {
            if self.tasks.len() > *capacity {
                *capacity = self.tasks.len() * 2; // Double capacity
            }
        }

        Ok(self)
    }

    /// Build the task graph with optimized memory layout.
    pub fn build(self) -> Result<TaskGraph<String>> {
        let capacity = self.capacity_hint.unwrap_or(self.tasks.len());

        let mut graph = TaskGraph::with_capacity(capacity);
        for task in self.tasks.into_values() {
            graph.add_task(task)?;
        }

        Ok(graph)
    }
}

impl Default for TaskGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimpleTask;

    #[tokio::test]
    async fn test_task_graph_builder() {
        let task1 = Box::new(SimpleTask::new("task1".to_string(), vec![], || {
            Ok("result1".to_string())
        }));
        let task2 = Box::new(SimpleTask::new("task2".to_string(), vec![], || {
            Ok("result2".to_string())
        }));

        let builder = TaskGraphBuilder::new()
            .with_capacity_hint(2)
            .add_task(task1)
            .unwrap()
            .add_task(task2)
            .unwrap();

        let graph = builder.build().unwrap();
        assert_eq!(graph.len(), 2);
    }
}
