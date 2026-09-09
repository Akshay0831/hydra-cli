//! crates/hydra-cli/src/partitioner/ast_splitter.rs
//!
//! Uses AST dependency graphs and tree-sitter indexing to partition the workspace
//! into disjoint, non-overlapping file scopes for parallel worker execution.

use anyhow::Result;
use hydra_matrix::{CodeMatrix, IndexConfig};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A disjoint scope of files allocated to a parallel worker trio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePartition {
    pub id: String,
    pub root_files: Vec<PathBuf>,
    pub dependency_files: Vec<PathBuf>,
    pub all_files: Vec<PathBuf>,
    pub estimated_complexity: u32,
}

impl FilePartition {
    pub fn contains_file(&self, path: &Path) -> bool {
        self.all_files.iter().any(|f| f == path || path.starts_with(f))
    }
}

/// AST Workspace partitioner leveraging hydra-matrix.
pub struct AstSplitter {
    matrix: Arc<CodeMatrix>,
}

impl AstSplitter {
    pub fn new(matrix: Arc<CodeMatrix>) -> Self {
        Self { matrix }
    }

    /// Convenience constructor creating an indexed CodeMatrix for the workspace root.
    pub fn from_workspace(root: &Path) -> Result<Self> {
        let config = IndexConfig {
            paths: vec![root.to_string_lossy().to_string()],
            ..IndexConfig::default()
        };
        let matrix = CodeMatrix::with_config(config)?;
        Ok(Self {
            matrix: Arc::new(matrix),
        })
    }

    /// Analyzes the codebase and partitions requested target paths into isolated groups.
    pub async fn partition_workspace(
        &self,
        targets: &[PathBuf],
        max_partitions: usize,
    ) -> Result<Vec<FilePartition>> {
        if targets.is_empty() {
            return Ok(vec![]);
        }

        // 1. Build adjacency map based on symbol dependencies
        let mut file_graph: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
        for target in targets {
            let mut related = HashSet::new();
            // Query elements belonging to this file in the matrix
            if let Ok(elements) = self.matrix.search(&target.to_string_lossy()).await {
                for element in elements {
                    for dep in element.dependencies {
                        let dep_path = PathBuf::from(dep);
                        if targets.contains(&dep_path) {
                            related.insert(dep_path);
                        }
                    }
                }
            }
            file_graph.insert(target.clone(), related);
        }

        // 2. Cluster targets into disjoint partition sets
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut clusters: Vec<Vec<PathBuf>> = Vec::new();

        for target in targets {
            if visited.contains(target) {
                continue;
            }

            let mut cluster = Vec::new();
            let mut queue = vec![target.clone()];
            visited.insert(target.clone());

            while let Some(current) = queue.pop() {
                cluster.push(current.clone());
                if let Some(neighbors) = file_graph.get(&current) {
                    for neighbor in neighbors {
                        if !visited.contains(neighbor) {
                            visited.insert(neighbor.clone());
                            queue.push(neighbor.clone());
                        }
                    }
                }
            }
            clusters.push(cluster);
        }

        // 3. Balance clusters up to max_partitions
        let mut partitions = Vec::new();
        for (i, cluster) in clusters.into_iter().enumerate() {
            if partitions.len() >= max_partitions {
                // Merge remainder into last partition
                if let Some(last) = partitions.last_mut() {
                    let last_part: &mut FilePartition = last;
                    last_part.all_files.extend(cluster);
                }
            } else {
                partitions.push(FilePartition {
                    id: format!("wt-part-{}", i + 1),
                    root_files: cluster.clone(),
                    dependency_files: Vec::new(),
                    all_files: cluster,
                    estimated_complexity: 1,
                });
            }
        }

        Ok(partitions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_partition_contains_file() {
        let part = FilePartition {
            id: "part-1".to_string(),
            root_files: vec![PathBuf::from("src/adapters")],
            dependency_files: vec![],
            all_files: vec![PathBuf::from("src/adapters/pi_agent.rs")],
            estimated_complexity: 1,
        };

        assert!(part.contains_file(&PathBuf::from("src/adapters/pi_agent.rs")));
        assert!(!part.contains_file(&PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_file_partition_serde() {
        let part = FilePartition {
            id: "part-test".to_string(),
            root_files: vec![PathBuf::from("src/lib.rs")],
            dependency_files: vec![PathBuf::from("src/util.rs")],
            all_files: vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/util.rs")],
            estimated_complexity: 2,
        };
        let serialized = serde_json::to_string(&part).unwrap();
        let deserialized: FilePartition = serde_json::from_str(&serialized).unwrap();
        assert_eq!(part.id, deserialized.id);
        assert_eq!(part.all_files, deserialized.all_files);
        assert_eq!(part.estimated_complexity, deserialized.estimated_complexity);
    }

    #[tokio::test]
    async fn test_ast_splitter_empty_targets() {
        let splitter = AstSplitter::from_workspace(Path::new(".")).unwrap();
        let partitions = splitter.partition_workspace(&[], 4).await.unwrap();
        assert!(partitions.is_empty());
    }

    #[tokio::test]
    async fn test_ast_splitter_single_target() {
        let splitter = AstSplitter::from_workspace(Path::new(".")).unwrap();
        let targets = vec![PathBuf::from("src/main.rs")];
        let partitions = splitter.partition_workspace(&targets, 4).await.unwrap();
        assert_eq!(partitions.len(), 1);
        assert_eq!(partitions[0].id, "wt-part-1");
        assert!(partitions[0].contains_file(&PathBuf::from("src/main.rs")));
    }

    #[tokio::test]
    async fn test_ast_splitter_multiple_targets_balanced() {
        let splitter = AstSplitter::from_workspace(Path::new(".")).unwrap();
        let targets = vec![
            PathBuf::from("src/adapters/pi_agent.rs"),
            PathBuf::from("src/consolidator/merger.rs"),
        ];
        let partitions = splitter.partition_workspace(&targets, 2).await.unwrap();
        assert_eq!(partitions.len(), 2);
    }
}

