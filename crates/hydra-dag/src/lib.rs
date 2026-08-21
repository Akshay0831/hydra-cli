use std::collections::{HashMap, HashSet};

use anyhow::Result;

/// A task that can be executed with dependencies.
/// 
/// The generic parameter `T` represents the task's result type.
/// Implementations should handle their own error types and convert to `Result<T>`.
#[async_trait::async_trait]
pub trait Task: Send + Sync {
    /// The type of result this task produces.
    type Output;
    
    /// Unique identifier for this task.
    fn id(&self) -> String;
    
    /// List of dependency task IDs that must complete before this task can run.
    fn dependencies(&self) -> Vec<String>;
    
    /// Execute the task and return its result.
    async fn execute(&self) -> Result<Self::Output>;
}

/// A directed acyclic graph of tasks.
pub struct TaskGraph<T> {
    /// All tasks in the graph.
    tasks: HashMap<String, Box<dyn Task<Output = T>>>,
    /// Mapping from task ID to its dependency count.
    dependency_counts: HashMap<String, usize>,
    /// Mapping from task ID to its dependents.
    dependents: HashMap<String, Vec<String>>,
    /// Tasks with no dependencies (ready to execute).
    ready_tasks: Vec<String>,
}

impl<T> TaskGraph<T>
where
    T: Send + Sync + 'static,
{
    /// Create a new empty task graph.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            dependency_counts: HashMap::new(),
            dependents: HashMap::new(),
            ready_tasks: Vec::new(),
        }
    }
    
    /// Add a task to the graph.
    /// 
    /// # Returns
    /// - `Ok(())` if the task was added successfully
    /// - `Err(anyhow::Error)` if there's a cycle or invalid configuration
    pub fn add_task(&mut self, task: Box<dyn Task<Output = T>>) -> Result<()> {
        let id = task.id();
        
        // Check for duplicate task IDs
        if self.tasks.contains_key(&id) {
            return Err(anyhow::anyhow!("Duplicate task ID: {}", id));
        }
        
        let dependencies = task.dependencies();
        
        // Check for cycles in dependencies
        if self.would_create_cycle(&id, &dependencies) {
            return Err(anyhow::anyhow!("Adding task {} would create a cycle", id));
        }
        
        // Update dependency counts and dependents
        for dep_id in &dependencies {
            self.dependents
                .entry(dep_id.clone())
                .or_insert_with(Vec::new)
                .push(id.clone());
            
            *self.dependency_counts.entry(dep_id.clone()).or_insert(0) += 1;
        }
        
        // If the task has no dependencies, it's ready to execute
        if dependencies.is_empty() {
            self.ready_tasks.push(id.clone());
        }
        
        self.tasks.insert(id, task);
        Ok(())
    }
    
    /// Add multiple tasks to the graph.
    /// 
    /// Tasks will be added in order. The method stops at the first error and returns it.
    pub fn add_tasks(&mut self, tasks: Vec<Box<dyn Task<Output = T>>>) -> Result<()> {
        for task in tasks {
            self.add_task(task)?;
        }
        Ok(())
    }
    
    /// Check if adding a task with the given dependencies would create a cycle.
    fn would_create_cycle(&self, task_id: &str, dependencies: &[String]) -> bool {
        // Build the complete graph including the new task
        let mut graph = HashMap::new();
        
        // Add all existing tasks and their dependencies
        for (task_name, task) in &self.tasks {
            graph.insert(task_name.clone(), task.dependencies());
        }
        
        // Add the new task's dependencies
        graph.insert(task_id.to_string(), dependencies.to_vec());
        
        // Classic cycle detection using DFS with recursion stack
        fn has_cycle(
            node: &str,
            graph: &HashMap<String, Vec<String>>,
            visited: &mut HashSet<String>,
            recursion_stack: &mut HashSet<String>,
        ) -> bool {
            // If we encounter a node that's already in the recursion stack, we have a cycle
            if recursion_stack.contains(node) {
                return true;
            }
            
            // If we've already visited this node, no cycle from here
            if visited.contains(node) {
                return false;
            }
            
            // Mark as visited and add to recursion stack
            visited.insert(node.to_string());
            recursion_stack.insert(node.to_string());
            
            // Visit all dependents of this node
            if let Some(dependents) = graph.get(node) {
                for dependent in dependents {
                    if has_cycle(dependent, graph, visited, recursion_stack) {
                        return true;
                    }
                }
            }
            
            // Backtrack
            recursion_stack.remove(node);
            false
        }
        
        // Check for cycles starting from each dependency
        for dep_id in dependencies {
            let mut visited = HashSet::new();
            let mut recursion_stack = HashSet::new();
            
            if has_cycle(dep_id, &graph, &mut visited, &mut recursion_stack) {
                return true;
            }
        }
        
        false
    }
    
    /// Get the number of tasks in the graph.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }
    
    /// Check if the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
    
    /// Get all task IDs in the graph.
    pub fn task_ids(&self) -> impl Iterator<Item = &String> {
        self.tasks.keys()
    }
    
    /// Check if a specific task exists in the graph.
    pub fn contains_task(&self, task_id: &str) -> bool {
        self.tasks.contains_key(task_id)
    }
    
    /// Get a topological ordering of tasks.
    /// 
    /// Returns `None` if the graph contains cycles.
    pub fn topological_order(&self) -> Option<Vec<String>> {
        let mut order = Vec::new();
        
        // Create a copy of dependency counts for traversal
        // Include all tasks that have dependencies, even if they're not in dependency_counts
        let mut remaining_deps = self.dependency_counts.clone();
        
        // Add tasks with dependencies that aren't in dependency_counts
        for task_id in self.task_ids() {
            if !remaining_deps.contains_key(task_id) && self.tasks[task_id].dependencies().len() > 0 {
                remaining_deps.insert(task_id.clone(), self.tasks[task_id].dependencies().len());
            }
        }
        
        // Start with nodes that have no dependencies
        let mut queue: Vec<String> = self
            .dependency_counts
            .iter()
            .filter(|(_, &count)| count == 0)
            .map(|(id, _)| id.clone())
            .collect();
        
        // A task should only be in the initial queue if it has no dependencies
        // Check each task to see if it has any dependencies
        for task_id in self.task_ids() {
            let has_dependencies = self.tasks[task_id].dependencies().iter().any(|dep| {
                self.tasks.contains_key(dep)
            });
            
            if !has_dependencies && !queue.contains(task_id) {
                queue.push(task_id.clone());
            }
        }
        
        while !queue.is_empty() {
            let current = queue.remove(0);
            order.push(current.clone());
            
            // Visit all dependents
            if let Some(dependents) = self.dependents.get(&current) {
                for dependent in dependents {
                    if let Some(count) = remaining_deps.get_mut(dependent) {
                        *count -= 1;
                        if *count == 0 {
                            queue.push(dependent.clone());
                        }
                    }
                }
            }
        }
        
        // Check if we processed all nodes (no cycles)
        if order.len() == self.tasks.len() {
            Some(order)
        } else {
            None
        }
    }
    
    /// Check if the graph contains any cycles.
    pub fn has_cycles(&self) -> bool {
        self.topological_order().is_none()
    }
    
    /// Execute all tasks sequentially in topological order.
    /// 
    /// This is a simplified implementation that respects dependencies but
    /// doesn't do parallel execution. For production use, you'd want a more
    /// sophisticated executor.
    pub async fn execute_sequential(&self) -> ExecutionResult<T> {
        let mut results = HashMap::new();
        let mut failed = Vec::new();
        
        if let Some(order) = self.topological_order() {
            for task_id in order {
                if let Some(task) = self.tasks.get(&task_id) {
                    match task.execute().await {
                        Ok(output) => {
                            results.insert(task_id, output);
                        }
                        Err(error) => {
                            failed.push((task_id, error.to_string()));
                            // For sequential execution, we stop on first failure
                            break;
                        }
                    }
                }
            }
        } else {
            // Graph has cycles, can't execute
            return ExecutionResult {
                successful: HashMap::new(),
                failed: vec![("graph_cycle".to_string(), "Graph contains cycles and cannot be executed".to_string())],
                strategy: ExecutionStrategy::Sequential,
            };
        }
        
        ExecutionResult {
            successful: results,
            failed,
            strategy: ExecutionStrategy::Sequential,
        }
    }
}

/// Strategy for task execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// Stop execution on first failure.
    FailFast,
    /// Continue execution even if some tasks fail.
    CollectFailures,
    /// Wait for dependencies before running parallel tasks.
    Sequential,
}

/// Result of executing a task graph.
#[derive(Debug, Clone)]
pub struct ExecutionResult<T> {
    /// Successfully completed tasks and their results.
    pub successful: HashMap<String, T>,
    /// Failed tasks and their errors.
    pub failed: Vec<(String, String)>,
    /// The execution strategy used.
    pub strategy: ExecutionStrategy,
}

impl<T> ExecutionResult<T> {
    /// Check if all tasks completed successfully.
    pub fn all_successful(&self) -> bool {
        self.failed.is_empty()
    }
    
    /// Get the number of successful tasks.
    pub fn successful_count(&self) -> usize {
        self.successful.len()
    }
    
    /// Get the number of failed tasks.
    pub fn failed_count(&self) -> usize {
        self.failed.len()
    }
    
    /// Check if any tasks failed.
    pub fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }
    
    /// Get results for a specific task ID.
    pub fn get_result(&self, task_id: &str) -> Option<&T> {
        self.successful.get(task_id)
    }
    
    /// Get error for a specific task ID if it failed.
    pub fn get_error(&self, task_id: &str) -> Option<&String> {
        self.failed
            .iter()
            .find(|(id, _)| id == task_id)
            .map(|(_, error)| error)
    }
    
    /// Consume this result and return only the successful results.
    pub fn into_successful(self) -> HashMap<String, T> {
        self.successful
    }
    
    /// Consume this result and return only the failures.
    pub fn into_failures(self) -> Vec<(String, String)> {
        self.failed
    }
}

/// A simple concrete task implementation for testing and basic use cases.
pub struct SimpleTask {
    pub id: String,
    pub dependencies: Vec<String>,
    pub execute_fn: Box<dyn Fn() -> anyhow::Result<String> + Send + Sync>,
}

impl SimpleTask {
    pub fn new(
        id: String,
        dependencies: Vec<String>,
        execute_fn: impl Fn() -> anyhow::Result<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id,
            dependencies,
            execute_fn: Box::new(execute_fn),
        }
    }
}

#[async_trait::async_trait]
impl Task for SimpleTask {
    type Output = String;
    
    fn id(&self) -> String {
        self.id.clone()
    }
    
    fn dependencies(&self) -> Vec<String> {
        self.dependencies.clone()
    }
    
    async fn execute(&self) -> Result<Self::Output> {
        (self.execute_fn)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    
    #[tokio::test]
    async fn test_empty_graph() {
        let graph: TaskGraph<String> = TaskGraph::new();
        assert_eq!(graph.len(), 0);
        assert!(graph.is_empty());
        let result = graph.execute_sequential().await;
        assert!(result.all_successful());
        assert_eq!(result.successful_count(), 0);
        assert_eq!(result.failed_count(), 0);
    }
    
    #[tokio::test]
    async fn test_single_task() {
        let mut graph = TaskGraph::new();
        
        let task = SimpleTask::new(
            "task1".to_string(),
            vec![],
            || Ok("result1".to_string()),
        );
        
        graph.add_task(Box::new(task)).unwrap();
        
        let result = graph.execute_sequential().await;
        println!("Result: {:?}", result);
        if !result.all_successful() {
            println!("Errors: {:?}", result.failed);
        }
        assert!(result.all_successful());
        assert_eq!(result.successful_count(), 1);
        assert_eq!(result.get_result("task1"), Some(&"result1".to_string()));
    }
    
    #[tokio::test]
    async fn test_sequential_tasks() {
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec![],
            || Ok("result1".to_string()),
        );
        
        let task2 = SimpleTask::new(
            "task2".to_string(),
            vec!["task1".to_string()],
            || Ok("result2".to_string()),
        );
        
        graph.add_tasks(vec![
            Box::new(task1),
            Box::new(task2),
        ]).unwrap();
        
        let result = graph.execute_sequential().await;
        assert!(result.all_successful());
        assert_eq!(result.successful_count(), 2);
        assert_eq!(result.get_result("task1"), Some(&"result1".to_string()));
        assert_eq!(result.get_result("task2"), Some(&"result2".to_string()));
    }
    
    #[tokio::test]
    async fn test_parallel_tasks() {
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec![],
            || Ok("result1".to_string()),
        );
        
        let task2 = SimpleTask::new(
            "task2".to_string(),
            vec![],
            || Ok("result2".to_string()),
        );
        
        graph.add_tasks(vec![
            Box::new(task1),
            Box::new(task2),
        ]).unwrap();
        
        let result = graph.execute_sequential().await;
        assert!(result.all_successful());
        assert_eq!(result.successful_count(), 2);
        assert_eq!(result.get_result("task1"), Some(&"result1".to_string()));
        assert_eq!(result.get_result("task2"), Some(&"result2".to_string()));
    }
    
    #[tokio::test]
    async fn test_diamond_dependencies() {
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec![],
            || Ok("result1".to_string()),
        );
        
        let task2 = SimpleTask::new(
            "task2".to_string(),
            vec!["task1".to_string()],
            || Ok("result2".to_string()),
        );
        
        let task3 = SimpleTask::new(
            "task3".to_string(),
            vec!["task1".to_string()],
            || Ok("result3".to_string()),
        );
        
        let task4 = SimpleTask::new(
            "task4".to_string(),
            vec!["task2".to_string(), "task3".to_string()],
            || Ok("result4".to_string()),
        );
        
        graph.add_tasks(vec![
            Box::new(task1),
            Box::new(task2),
            Box::new(task3),
            Box::new(task4),
        ]).unwrap();
        
        let result = graph.execute_sequential().await;
        assert!(result.all_successful());
        assert_eq!(result.successful_count(), 4);
        assert!(result.get_result("task4").is_some());
    }
    
    #[tokio::test]
    async fn test_cycle_detection() {
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec!["task2".to_string()], // task1 depends on task2
            || Ok("result1".to_string()),
        );
        
        let _task2 = SimpleTask::new(
            "task2".to_string(),
            vec!["task1".to_string()], // task2 depends on task1 - cycle
            || Ok("result2".to_string()),
        );
        
        // Add task1 first - this should succeed since task2 doesn't exist yet
        let result1 = graph.add_task(Box::new(task1));
        assert!(result1.is_ok(), "Adding task1 should succeed when task2 doesn't exist yet");
        
        // Now add task2 - this should fail because it creates a cycle (task2 -> task1 -> task2)
        let result2 = graph.add_task(Box::new(_task2));
        assert!(result2.is_err(), "Adding task2 should fail because it creates a cycle");
    }
    
    #[tokio::test]
    async fn test_duplicate_task_id() {
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec![],
            || Ok("result1".to_string()),
        );
        
        let task2 = SimpleTask::new(
            "task1".to_string(), // Same ID
            vec![],
            || Ok("result2".to_string()),
        );
        
        graph.add_task(Box::new(task1)).unwrap();
        
        let result = graph.add_task(Box::new(task2));
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_topological_order() {
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec![],
            || Ok("result1".to_string()),
        );
        
        let task2 = SimpleTask::new(
            "task2".to_string(),
            vec!["task1".to_string()],
            || Ok("result2".to_string()),
        );
        
        let task3 = SimpleTask::new(
            "task3".to_string(),
            vec!["task1".to_string()],
            || Ok("result3".to_string()),
        );
        
        graph.add_tasks(vec![
            Box::new(task1),
            Box::new(task2),
            Box::new(task3),
        ]).unwrap();
        
        let order = graph.topological_order().unwrap();
        assert_eq!(order.len(), 3);
        
        // task1 should come before task2 and task3
        let task1_pos = order.iter().position(|id| id == "task1").unwrap();
        let task2_pos = order.iter().position(|id| id == "task2").unwrap();
        let task3_pos = order.iter().position(|id| id == "task3").unwrap();
        
        assert!(task1_pos < task2_pos);
        assert!(task1_pos < task3_pos);
        // task2 and task3 can be in any order relative to each other
    }
    
    #[tokio::test]
    async fn test_cycle_detection_topological() {
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec!["task2".to_string()],
            || Ok("result1".to_string()),
        );
        
        let task2 = SimpleTask::new(
            "task2".to_string(),
            vec!["task1".to_string()],
            || Ok("result2".to_string()),
        );
        
        // Add task1 first - this should succeed since task2 doesn't exist yet
        graph.add_task(Box::new(task1)).unwrap();
        
        // Try to add task2 - this should fail because it creates a cycle
        let result = graph.add_task(Box::new(task2));
        assert!(result.is_err(), "Adding task2 should fail because it creates a cycle");
        
        // After the failed addition, the graph should still contain only task1
        // and task1 should not have any cycles (since task2 was never added)
        assert!(!graph.has_cycles(), "Graph should not have cycles after failed task addition");
        
        // Verify the graph still has task1 and works normally
        assert_eq!(graph.tasks.len(), 1, "Graph should contain only task1");
        assert_eq!(graph.tasks.contains_key("task1"), true, "Graph should contain task1");
    }
    
    #[tokio::test]
    async fn test_task_states() {
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec![],
            || Ok("result1".to_string()),
        );
        
        let task2 = SimpleTask::new(
            "task2".to_string(),
            vec!["task1".to_string()],
            || Ok("result2".to_string()),
        );
        
        graph.add_tasks(vec![
            Box::new(task1),
            Box::new(task2),
        ]).unwrap();
        
        assert!(graph.contains_task("task1"));
        assert!(graph.contains_task("task2"));
        assert_eq!(graph.len(), 2);
        
        let result = graph.execute_sequential().await;
        assert!(result.all_successful());
    }
    
    #[tokio::test]
    async fn test_task_with_dependencies() {
        let execution_order = Arc::new(AtomicUsize::new(0));
        let execution_order_clone1 = execution_order.clone();
        let execution_order_clone2 = execution_order.clone();
        
        let mut graph = TaskGraph::new();
        
        let task1 = SimpleTask::new(
            "task1".to_string(),
            vec![],
            move || {
                execution_order_clone1.fetch_add(1, Ordering::SeqCst);
                Ok("result1".to_string())
            },
        );
        
        let task2 = SimpleTask::new(
            "task2".to_string(),
            vec!["task1".to_string()],
            move || {
                execution_order_clone2.fetch_add(1, Ordering::SeqCst);
                Ok("result2".to_string())
            },
        );
        
        graph.add_tasks(vec![
            Box::new(task1),
            Box::new(task2),
        ]).unwrap();
        
        let result = graph.execute_sequential().await;
        assert!(result.all_successful());
        
        // task1 should execute first (value 1), then task2 (value 2)
        assert_eq!(execution_order.load(Ordering::SeqCst), 2);
    }
}