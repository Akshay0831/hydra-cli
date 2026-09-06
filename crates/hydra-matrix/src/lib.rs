use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use glob::Pattern;

/// A code element that can be indexed and searched
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeElement {
    pub id: String,
    pub file_path: PathBuf,
    pub element_type: ElementType,
    pub name: String,
    pub line_number: usize,
    pub content: String,
    pub dependencies: Vec<String>,
    pub metadata: HashMap<String, String>,
}

/// Types of code elements that can be indexed
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ElementType {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Module,
    Variable,
    Constant,
    Import,
    Class,
    Interface,
    TypeAlias,
    Macro,
}

impl std::fmt::Display for ElementType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElementType::Function => write!(f, "Function"),
            ElementType::Method => write!(f, "Method"),
            ElementType::Struct => write!(f, "Struct"),
            ElementType::Enum => write!(f, "Enum"),
            ElementType::Trait => write!(f, "Trait"),
            ElementType::Module => write!(f, "Module"),
            ElementType::Variable => write!(f, "Variable"),
            ElementType::Constant => write!(f, "Constant"),
            ElementType::Import => write!(f, "Import"),
            ElementType::Class => write!(f, "Class"),
            ElementType::Interface => write!(f, "Interface"),
            ElementType::TypeAlias => write!(f, "TypeAlias"),
            ElementType::Macro => write!(f, "Macro"),
        }
    }
}

/// Configuration for code indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Paths to index (supports glob patterns)
    pub paths: Vec<String>,
    /// File patterns to include
    pub include_patterns: Vec<String>,
    /// File patterns to exclude
    pub exclude_patterns: Vec<String>,
    /// Maximum file size to index (in bytes)
    pub max_file_size: usize,
    /// Whether to follow symbolic links
    pub follow_symlinks: bool,
    /// Whether to index documentation
    pub index_docs: bool,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            paths: vec!["./**/*".to_string()],
            include_patterns: vec![
                "*.rs".to_string(),
                "*.js".to_string(),
                "*.ts".to_string(),
                "*.jsx".to_string(),
                "*.tsx".to_string(),
            ],
            exclude_patterns: vec![
                "**/node_modules/**".to_string(),
                "**/target/**".to_string(),
                "**/build/**".to_string(),
                "**/dist/**".to_string(),
                "**/.git/**".to_string(),
            ],
            max_file_size: 10 * 1024 * 1024, // 10MB
            follow_symlinks: false,
            index_docs: false,
        }
    }
}

/// The main code indexing engine
pub struct CodeMatrix {
    index: RwLock<HashMap<String, CodeElement>>,
    pub config: IndexConfig,
    language_parsers: HashMap<String, Box<dyn LanguageParser>>,
}

/// Trait for language-specific parsing
pub trait LanguageParser: Send + Sync {
    fn supported_extensions(&self) -> Vec<&str>;
    fn parse_file(&self, file_path: &Path, content: &str) -> Result<Vec<CodeElement>>;
    fn extract_dependencies(&self, content: &str) -> Result<Vec<String>>;
}



/// Language-specific parsers
mod parsers {
    use super::*;
    
    /// Simple placeholder parsers (temporary implementation)
    pub struct RustParser;
    pub struct JSTypeScriptParser;
    
    impl LanguageParser for RustParser {
        fn supported_extensions(&self) -> Vec<&str> {
            vec!["rs"]
        }
        
        fn parse_file(&self, file_path: &Path, content: &str) -> Result<Vec<CodeElement>> {
            let mut elements = Vec::new();
            
            for (line_num, line) in content.lines().enumerate() {
                let declaration = line.trim().strip_prefix("pub ").unwrap_or(line.trim());
                if declaration.starts_with("fn ") {
                    let name = declaration.strip_prefix("fn ").unwrap().split('(').next().unwrap().trim();
                    elements.push(CodeElement {
                        id: format!("{}:{}:{}", file_path.display(), line_num, name),
                        file_path: file_path.to_path_buf(),
                        element_type: ElementType::Function,
                        name: name.to_string(),
                        line_number: line_num + 1,
                        content: line.to_string(),
                        dependencies: Vec::new(),
                        metadata: HashMap::new(),
                    });
                } else if declaration.starts_with("struct ") {
                    let name = declaration.strip_prefix("struct ").unwrap().split('{').next().unwrap().trim();
                    elements.push(CodeElement {
                        id: format!("{}:{}:{}", file_path.display(), line_num, name),
                        file_path: file_path.to_path_buf(),
                        element_type: ElementType::Struct,
                        name: name.to_string(),
                        line_number: line_num + 1,
                        content: line.to_string(),
                        dependencies: Vec::new(),
                        metadata: HashMap::new(),
                    });
                } else if declaration.starts_with("enum ") {
                    let name = declaration.strip_prefix("enum ").unwrap().split('{').next().unwrap().trim();
                    elements.push(CodeElement {
                        id: format!("{}:{}:{}", file_path.display(), line_num, name),
                        file_path: file_path.to_path_buf(),
                        element_type: ElementType::Enum,
                        name: name.to_string(),
                        line_number: line_num + 1,
                        content: line.to_string(),
                        dependencies: Vec::new(),
                        metadata: HashMap::new(),
                    });
                }
            }
            
            Ok(elements)
        }
        
        fn extract_dependencies(&self, content: &str) -> Result<Vec<String>> {
            let mut dependencies = Vec::new();
            
            // Simple use statement extraction
            for line in content.lines() {
                if line.trim().starts_with("use ") {
                    let dep = line.trim().strip_prefix("use ").unwrap().trim_end_matches(';').trim();
                    if !dep.is_empty() {
                        dependencies.push(dep.to_string());
                    }
                }
            }
            
            Ok(dependencies)
        }
    }
    
    impl LanguageParser for JSTypeScriptParser {
        fn supported_extensions(&self) -> Vec<&str> {
            vec!["js", "ts", "jsx", "tsx"]
        }
        
        fn parse_file(&self, file_path: &Path, content: &str) -> Result<Vec<CodeElement>> {
            let mut elements = Vec::new();
            
            // Simple text-based parsing for demo
            for (line_num, line) in content.lines().enumerate() {
                let declaration = line.trim().strip_prefix("export ").unwrap_or(line.trim());
                if declaration.starts_with("function ")
                    || declaration.starts_with("const ")
                    || declaration.starts_with("let ")
                    || declaration.starts_with("class ")
                    || declaration.starts_with("interface ")
                {
                    let name = line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .split('(')
                        .next()
                        .unwrap_or("")
                        .trim_end_matches(':')
                        .trim();
                    
                    if !name.is_empty() {
                        let element_type = if declaration.starts_with("function ") {
                            ElementType::Function
                        } else if declaration.starts_with("class ") {
                            ElementType::Class
                        } else if declaration.starts_with("interface ") {
                            ElementType::Interface
                        } else {
                            ElementType::Variable
                        };
                        
                        elements.push(CodeElement {
                            id: format!("{}:{}:{}", file_path.display(), line_num, name),
                            file_path: file_path.to_path_buf(),
                            element_type,
                            name: name.to_string(),
                            line_number: line_num + 1,
                            content: line.to_string(),
                            dependencies: Vec::new(),
                            metadata: HashMap::new(),
                        });
                    }
                }
            }
            
            Ok(elements)
        }
        
        fn extract_dependencies(&self, content: &str) -> Result<Vec<String>> {
            let mut dependencies = Vec::new();
            
            // Simple import/require extraction
            for line in content.lines() {
                if line.trim().starts_with("import ") || line.trim().starts_with("const ") && line.contains("require(") {
                    let dep = line
                        .trim()
                        .split('"')
                        .nth(1)
                        .unwrap_or("")
                        .split("'")
                        .next()
                        .unwrap_or("");
                    
                    if !dep.is_empty() {
                        dependencies.push(dep.to_string());
                    }
                }
            }
            
            Ok(dependencies)
        }
    }
}

impl CodeMatrix {
    /// Create a new CodeMatrix with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(IndexConfig::default())
    }
    
    /// Create a new CodeMatrix with custom configuration
    pub fn with_config(config: IndexConfig) -> Result<Self> {
        let mut language_parsers = HashMap::new();
        language_parsers.insert("rs".to_string(), Box::new(parsers::RustParser) as Box<dyn LanguageParser>);
        language_parsers.insert("js".to_string(), Box::new(parsers::JSTypeScriptParser) as Box<dyn LanguageParser>);
        language_parsers.insert("jsx".to_string(), Box::new(parsers::JSTypeScriptParser) as Box<dyn LanguageParser>);
        language_parsers.insert("ts".to_string(), Box::new(parsers::JSTypeScriptParser) as Box<dyn LanguageParser>);
        language_parsers.insert("tsx".to_string(), Box::new(parsers::JSTypeScriptParser) as Box<dyn LanguageParser>);
        
        Ok(Self {
            index: RwLock::new(HashMap::new()),
            config,
            language_parsers,
        })
    }
    
    /// Index code from the configured paths
    pub async fn index(&mut self) -> Result<usize> {
        let mut indexed_files = 0;
        let paths = self.config.paths.clone();
        
        for pattern in &paths {
            let matched_files = self.collect_matching_files(pattern)?;
            
            for file_path in matched_files {
                if self.should_index_file(&file_path).await? && self.index_file(&file_path).await? {
                        indexed_files += 1;
                    }
            }
        }
        
        Ok(indexed_files)
    }
    
    /// Index a specific file
    pub async fn index_file(&mut self, file_path: &Path) -> Result<bool> {
        // Check file size
        let metadata = tokio::fs::metadata(file_path).await?;
        if metadata.len() > self.config.max_file_size as u64 {
            return Ok(false);
        }
        
        // Read file content
        let content = tokio::fs::read_to_string(file_path).await?;
        
        // Get file extension
        let extension = file_path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        
        // Get appropriate parser
        if let Some(parser) = self.language_parsers.get(extension) {
            let elements = parser.parse_file(file_path, &content)?;
            let dependencies = parser.extract_dependencies(&content)?;
            
            // Add elements to index
            let mut index = self.index.write().await;
            for mut element in elements {
                element.dependencies = dependencies.clone();
                index.insert(element.id.clone(), element);
            }
            
            Ok(true)
        } else {
            Ok(false) // Unsupported file type
        }
    }
    
    /// Search for code elements matching the query
    pub async fn search(&self, query: &str) -> Result<Vec<CodeElement>> {
        let index = self.index.read().await;
        let mut results = Vec::new();
        
        for element in index.values() {
            if self.element_matches_query(element, query) {
                results.push(element.clone());
            }
        }
        
        Ok(results)
    }
    
    /// Search for code elements by type
    pub async fn search_by_type(&self, element_type: ElementType) -> Result<Vec<CodeElement>> {
        let index = self.index.read().await;
        let mut results = Vec::new();
        
        for element in index.values() {
            if element.element_type == element_type {
                results.push(element.clone());
            }
        }
        
        Ok(results)
    }
    
    /// Search for code elements by name
    pub async fn search_by_name(&self, name: &str) -> Result<Vec<CodeElement>> {
        let index = self.index.read().await;
        let mut results = Vec::new();
        
        for element in index.values() {
            if element.name.contains(name) {
                results.push(element.clone());
            }
        }
        
        Ok(results)
    }
    
    /// Find all dependencies for a code element
    pub async fn find_dependencies(&self, element_id: &str) -> Result<Vec<CodeElement>> {
        let index = self.index.read().await;
        let mut dependencies = Vec::new();
        
        if let Some(element) = index.get(element_id) {
            for dep_name in &element.dependencies {
                for dep_element in index.values() {
                    if dep_element.name.contains(dep_name) || dep_element.id.contains(dep_name) {
                        dependencies.push(dep_element.clone());
                    }
                }
            }
        }
        
        Ok(dependencies)
    }
    
    /// Find all dependents of a code element
    pub async fn find_dependents(&self, element_id: &str) -> Result<Vec<CodeElement>> {
        let index = self.index.read().await;
        let mut dependents = Vec::new();
        
        if let Some(element) = index.get(element_id) {
            for dependent in index.values() {
                if dependent.dependencies.contains(&element.name) || dependent.dependencies.contains(&element.id) {
                    dependents.push(dependent.clone());
                }
            }
        }
        
        Ok(dependents)
    }
    
    /// Get the total number of indexed elements
    pub async fn size(&self) -> usize {
        self.index.read().await.len()
    }
    
    /// Clear the index
    pub async fn clear(&self) {
        self.index.write().await.clear();
    }
    
    /// Get statistics about the index
    pub async fn get_stats(&self) -> IndexStats {
        let index = self.index.read().await;
        let mut stats = IndexStats::default();
        
        for element in index.values() {
            stats.total_elements += 1;
            match element.element_type {
                ElementType::Function => stats.functions += 1,
                ElementType::Struct => stats.structs += 1,
                ElementType::Enum => stats.enums += 1,
                ElementType::Class => stats.classes += 1,
                ElementType::Interface => stats.interfaces += 1,
                ElementType::Trait => stats.traits += 1,
                ElementType::Module => stats.modules += 1,
                ElementType::Variable => stats.variables += 1,
                ElementType::Constant => stats.constants += 1,
                ElementType::Import => stats.imports += 1,
                ElementType::Method | ElementType::TypeAlias | ElementType::Macro => {
                    // Count in others
                    stats.others += 1;
                }
            }
            
            stats.files.insert(element.file_path.clone());
        }
        
        stats
    }
    
    /// Collect matching files based on glob patterns
    fn collect_matching_files(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let pattern_obj = Pattern::new(pattern)?;
        let exact_root = Path::new(pattern);
        let matches_all_files = exact_root.is_dir();
        let root = pattern
            .find(['*', '?'])
            .map(|index| &pattern[..index])
            .unwrap_or(pattern)
            .trim_end_matches(['/', '\\']);
        let root = if root.is_empty() { "." } else { root };
        
        for entry in WalkDir::new(root)
            .follow_links(self.config.follow_symlinks)
            .into_iter()
            .filter_entry(|e| {
            if e.path().is_dir() && e.path().components().count() > 2 {
                return !e.path().components().any(|c| {
                    matches!(c.as_os_str().to_str(), Some("node_modules" | "target" | ".git"))
                });
            }
            true
        }) {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file()
                && (matches_all_files || pattern_obj.matches(path.to_str().unwrap_or("")))
            {
                    files.push(path.to_path_buf());
                }
        }
        
        Ok(files)
    }
    
    /// Check if a file should be indexed based on configuration
    async fn should_index_file(&self, file_path: &Path) -> Result<bool> {
        let file_str = file_path.to_string_lossy().to_string();
        
        // Check include patterns
        let file_name = file_path.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let should_include = self.config.include_patterns.is_empty() ||
            self.config.include_patterns.iter().any(|pattern| {
                Pattern::new(pattern).is_ok_and(|p| p.matches(file_name) || p.matches(&file_str))
            });
        
        // Check exclude patterns
        let should_exclude = self.config.exclude_patterns.iter().any(|pattern| {
            Pattern::new(pattern).is_ok_and(|p| p.matches(&file_str))
        });
        
        Ok(should_include && !should_exclude)
    }
    
    /// Check if an element matches the search query
    pub fn element_matches_query(&self, element: &CodeElement, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        element.name.to_lowercase().contains(&query_lower) ||
        element.content.to_lowercase().contains(&query_lower) ||
        element.element_type.to_string().to_lowercase().contains(&query_lower)
    }
}

/// Statistics about the code index
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub total_elements: usize,
    pub functions: usize,
    pub structs: usize,
    pub enums: usize,
    pub classes: usize,
    pub interfaces: usize,
    pub traits: usize,
    pub modules: usize,
    pub variables: usize,
    pub constants: usize,
    pub imports: usize,
    pub others: usize,
    pub files: HashSet<PathBuf>,
}