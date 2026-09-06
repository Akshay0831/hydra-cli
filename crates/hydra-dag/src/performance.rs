//! Simple performance optimization module for the Hydra DAG system.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Semaphore};
use anyhow::Result;
use crate::{Task, TaskGraph, ExecutionStrategy, ExecutionResult};

/// Optimized task executor with concurrency control.
pub struct OptimizedTaskExecutor {
    semaphore: Arc<Semaphore>,
    max_concurrency: usize,
}

impl OptimizedTaskExecutor {
    /// Create a new optimized task executor.
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            max_concurrency,
        }
    }
    
    /// Execute a task graph with optimized resource management.
    pub async fn execute_graph_optimized(
        &self,
        graph: &TaskGraph<String>,
        strategy: ExecutionStrategy,
    ) -> Result<ExecutionResult<String>> {
        let mut results = HashMap::new();
        let mut errors = Vec::new();
        
        // Get all tasks with no dependencies (ready to execute)
        let ready_tasks = self.get_ready_tasks(graph);
        
        // Execute ready tasks concurrently
        let mut execution_tasks = Vec::new();
        
        for task_id in ready_tasks {
            if let Some(task) = graph.tasks.get(&task_id) {
                let semaphore = self.semaphore.clone();
                let task_id_clone = task_id.clone();
                
                let execution_task = async move {
                    let _permit = semaphore.acquire().await;
                    
                    match _permit {
                        Ok(_permit) => {
                            match task.execute().await {
                                Ok(result) => Ok((task_id_clone, result)),
                                Err(error) => Err((task_id_clone, error)),
                            }
                        }
                        Err(e) => Err((task_id_clone, anyhow::anyhow!("Semaphore acquire failed: {}", e))),
                    }
                };
                
                execution_tasks.push(execution_task);
            }
        }
        
        // Execute tasks concurrently
        let task_results = futures::future::join_all(execution_tasks).await;
        
        // Collect results and errors
        for task_result in task_results {
            match task_result {
                Ok((task_id, result)) => {
                    results.insert(task_id, result);
                }
                Err((task_id, error)) => {
                    errors.push((task_id, error.to_string()));
                }
            }
        }
        
        // Execute remaining tasks sequentially
        if let Some(order) = graph.topological_order() {
            for task_id in order {
                if !results.contains_key(&task_id) && !errors.iter().any(|(id, _)| id == &task_id) {
                    if let Some(task) = graph.tasks.get(&task_id) {
                        match task.execute().await {
                            Ok(result) => {
                                results.insert(task_id, result);
                            }
                            Err(error) => {
                                errors.push((task_id, error.to_string()));
                            }
                        }
                    }
                }
            }
        }
        
        Ok(ExecutionResult {
            successful: results,
            failed: errors,
            strategy,
        })
    }
    
    /// Get ready tasks (tasks with no dependencies).
    fn get_ready_tasks(&self, graph: &TaskGraph<String>) -> Vec<String> {
        let mut ready = Vec::new();
        
        for task_id in graph.task_ids() {
            let has_deps = graph.get_task_dependencies(task_id)
                .iter()
                .any(|dep| graph.tasks.contains_key(dep));
            
            if !has_deps {
                ready.push(task_id.clone());
            }
        }
        
        ready
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
    pub fn build(self) -> TaskGraph<String> {
        let capacity = self.capacity_hint.unwrap_or(self.tasks.len());
        
        let mut graph = TaskGraph::with_capacity(capacity);
        for task in self.tasks.into_values() {
            let _ = graph.add_task(task);
        }
        
        graph
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
        let task1 = Box::new(SimpleTask::new(
            "task1".to_string(),
            vec![],
            || Ok("result1".to_string())
        ));
        let task2 = Box::new(SimpleTask::new(
            "task2".to_string(),
            vec![],
            || Ok("result2".to_string())
        ));
        
        let builder = TaskGraphBuilder::new()
            .with_capacity_hint(2)
            .add_task(task1)
            .unwrap()
            .add_task(task2)
            .unwrap();
        
        let graph = builder.build();
        assert_eq!(graph.len(), 2);
    }
}