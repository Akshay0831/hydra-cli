use anyhow::Result;
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use walkdir::WalkDir;

/// Code element for indexing and search
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

/// Code element types for indexing
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

/// Code indexing configuration
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

/// Main code indexing engine
pub struct CodeMatrix {
    index: RwLock<HashMap<String, CodeElement>>,
    pub config: IndexConfig,
    language_parsers: HashMap<String, Box<dyn LanguageParser>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedIndex {
    config: IndexConfig,
    stats: IndexStats,
    elements: Vec<CodeElement>,
}

/// Language-specific parsing trait
pub trait LanguageParser: Send + Sync {
    fn supported_extensions(&self) -> Vec<&str>;
    fn parse_file(&self, file_path: &Path, content: &str) -> Result<Vec<CodeElement>>;
    fn extract_dependencies(&self, content: &str) -> Result<Vec<String>>;
}

fn make_element(
    file_path: &Path,
    line_number: usize,
    element_type: ElementType,
    name: String,
    content: &str,
    dependencies: Vec<String>,
) -> CodeElement {
    CodeElement {
        id: format!("{}:{}:{}", file_path.display(), line_number, name),
        file_path: file_path.to_path_buf(),
        element_type,
        name,
        line_number,
        content: content.to_string(),
        dependencies,
        metadata: HashMap::new(),
    }
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
                    if let Some(name) = declaration
                        .strip_prefix("fn ")
                        .and_then(|value| value.split('(').next())
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        elements.push(make_element(
                            file_path,
                            line_num + 1,
                            ElementType::Function,
                            name.to_string(),
                            line,
                            Vec::new(),
                        ));
                    }
                } else if declaration.starts_with("struct ") {
                    if let Some(name) = declaration
                        .strip_prefix("struct ")
                        .and_then(|value| value.split('{').next())
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        elements.push(make_element(
                            file_path,
                            line_num + 1,
                            ElementType::Struct,
                            name.to_string(),
                            line,
                            Vec::new(),
                        ));
                    }
                } else if declaration.starts_with("enum ") {
                    if let Some(name) = declaration
                        .strip_prefix("enum ")
                        .and_then(|value| value.split('{').next())
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        elements.push(make_element(
                            file_path,
                            line_num + 1,
                            ElementType::Enum,
                            name.to_string(),
                            line,
                            Vec::new(),
                        ));
                    }
                }
            }

            Ok(elements)
        }

        fn extract_dependencies(&self, content: &str) -> Result<Vec<String>> {
            let mut dependencies = Vec::new();

            // Simple use statement extraction
            for line in content.lines() {
                if line.trim().starts_with("use ") {
                    if let Some(dep) = line
                        .trim()
                        .strip_prefix("use ")
                        .map(|value| value.trim_end_matches(';').trim())
                        .filter(|dep| !dep.is_empty())
                    {
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

                        elements.push(make_element(
                            file_path,
                            line_num + 1,
                            element_type,
                            name.to_string(),
                            line,
                            Vec::new(),
                        ));
                    }
                }
            }

            Ok(elements)
        }

        fn extract_dependencies(&self, content: &str) -> Result<Vec<String>> {
            let mut dependencies = Vec::new();

            // Simple import/require extraction
            for line in content.lines() {
                if line.trim().starts_with("import ")
                    || line.trim().starts_with("const ") && line.contains("require(")
                {
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
        language_parsers.insert(
            "rs".to_string(),
            Box::new(parsers::RustParser) as Box<dyn LanguageParser>,
        );
        language_parsers.insert(
            "js".to_string(),
            Box::new(parsers::JSTypeScriptParser) as Box<dyn LanguageParser>,
        );
        language_parsers.insert(
            "jsx".to_string(),
            Box::new(parsers::JSTypeScriptParser) as Box<dyn LanguageParser>,
        );
        language_parsers.insert(
            "ts".to_string(),
            Box::new(parsers::JSTypeScriptParser) as Box<dyn LanguageParser>,
        );
        language_parsers.insert(
            "tsx".to_string(),
            Box::new(parsers::JSTypeScriptParser) as Box<dyn LanguageParser>,
        );

        Ok(Self {
            index: RwLock::new(HashMap::new()),
            config,
            language_parsers,
        })
    }

    /// Index code from the configured paths
    pub async fn index(&mut self) -> Result<usize> {
        let mut indexed_files = 0;
        let mut seen_files = HashSet::new();
        let paths = self.config.paths.clone();

        for pattern in &paths {
            let matched_files = self.collect_matching_files(pattern)?;

            for file_path in matched_files {
                if seen_files.insert(file_path.clone())
                    && self.should_index_file(&file_path).await?
                    && self.index_file(&file_path).await?
                {
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
        let extension = file_path
            .extension()
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

        Ok(sort_elements(results))
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

        Ok(sort_elements(results))
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

        Ok(sort_elements(results))
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

        Ok(sort_elements(dependencies))
    }

    /// Find all dependents of a code element
    pub async fn find_dependents(&self, element_id: &str) -> Result<Vec<CodeElement>> {
        let index = self.index.read().await;
        let mut dependents = Vec::new();

        if let Some(element) = index.get(element_id) {
            for dependent in index.values() {
                if dependent.dependencies.contains(&element.name)
                    || dependent.dependencies.contains(&element.id)
                {
                    dependents.push(dependent.clone());
                }
            }
        }

        Ok(sort_elements(dependents))
    }

    /// Get the total number of indexed elements
    pub async fn size(&self) -> usize {
        self.index.read().await.len()
    }

    /// Clear the index
    pub async fn clear(&self) {
        self.index.write().await.clear();
    }

    pub async fn save_index(&self, path: &Path) -> Result<()> {
        let index = self.index.read().await;
        let elements = sort_elements(index.values().cloned().collect());
        drop(index);
        let snapshot = PersistedIndex {
            config: self.config.clone(),
            stats: self.get_stats().await,
            elements,
        };
        let contents = serde_json::to_string_pretty(&snapshot)?;
        tokio::fs::write(path, contents).await?;
        Ok(())
    }

    pub async fn load_index(path: &Path) -> Result<Self> {
        let contents = tokio::fs::read_to_string(path).await?;
        let snapshot: PersistedIndex = serde_json::from_str(&contents)?;
        let matrix = Self::with_config(snapshot.config)?;
        let mut index = matrix.index.write().await;
        for element in snapshot.elements {
            index.insert(element.id.clone(), element);
        }
        drop(index);
        Ok(matrix)
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
        let root_path = Path::new(root);
        if !root_path.exists() {
            anyhow::bail!("index root does not exist: {}", root_path.display());
        }
        if root_path.is_file() {
            if matches_all_files {
                return Ok(vec![root_path.to_path_buf()]);
            }
            anyhow::bail!("index root must be a directory: {}", root_path.display());
        }

        for entry in WalkDir::new(root)
            .follow_links(self.config.follow_symlinks)
            .into_iter()
            .filter_entry(|e| {
                if e.path().is_dir() {
                    return !self.matches_exclude(e.path());
                }
                true
            })
        {
            let entry = entry?;
            let path = entry.path();

            if path.is_file()
                && !self.matches_exclude(path)
                && (matches_all_files
                    || pattern_obj.matches(&path.to_string_lossy().replace('\\', "/")))
            {
                files.push(path.to_path_buf());
            }
        }

        files.sort();
        files.dedup();
        Ok(files)
    }

    fn matches_exclude(&self, path: &Path) -> bool {
        let normalized = path.to_string_lossy().replace('\\', "/");
        self.config.exclude_patterns.iter().any(|pattern| {
            Pattern::new(pattern).is_ok_and(|glob| glob.matches(&normalized))
                || Pattern::new(&format!("**/{pattern}"))
                    .is_ok_and(|glob| glob.matches(&normalized))
        })
    }

    /// Check if a file should be indexed based on configuration
    async fn should_index_file(&self, file_path: &Path) -> Result<bool> {
        let file_str = file_path.to_string_lossy().replace('\\', "/");

        // Check include patterns
        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let should_include = self.config.include_patterns.is_empty()
            || self.config.include_patterns.iter().any(|pattern| {
                Pattern::new(pattern).is_ok_and(|p| p.matches(file_name) || p.matches(&file_str))
            });

        // Check exclude patterns
        let should_exclude = self.matches_exclude(file_path);

        Ok(should_include && !should_exclude)
    }

    /// Check if an element matches the search query
    pub fn element_matches_query(&self, element: &CodeElement, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        element.name.to_lowercase().contains(&query_lower)
            || element.content.to_lowercase().contains(&query_lower)
            || element
                .element_type
                .to_string()
                .to_lowercase()
                .contains(&query_lower)
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

fn sort_elements(mut elements: Vec<CodeElement>) -> Vec<CodeElement> {
    elements.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then_with(|| left.line_number.cmp(&right.line_number))
            .then_with(|| {
                left.element_type
                    .to_string()
                    .cmp(&right.element_type.to_string())
            })
            .then_with(|| left.name.cmp(&right.name))
    });
    elements
}

// ---------------------------------------------------------------------------
// Hierarchical Context Loader, Anti-Looping Hash Cache & AST Skeletonizer
// ---------------------------------------------------------------------------

/// Strategy used for extracting and loading context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextStrategyKind {
    /// Strips function bodies and internal logic; extracts signatures and doc invariants.
    ASTSkeleton,
    /// Multi-signal ranking based on distance, relevance, and references.
    RelevanceScored,
    /// Fast slice extraction bounded by line/token budget.
    SliceWindow,
    /// Ingests directory/crate documentation tables and rules.
    DenseDoc,
}

impl Default for ContextStrategyKind {
    fn default() -> Self {
        Self::ASTSkeleton
    }
}

/// Cached entry in memory to avoid repetitive reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedContextEntry {
    pub path: PathBuf,
    pub content_hash: String,
    pub extracted_content: String,
    pub token_estimate: usize,
}

/// Hierarchical context assembled for a given target file or directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HierarchicalContext {
    /// Relative or absolute target path requested
    pub target: PathBuf,
    /// Associated crate/module dense documentation (e.g. README.md in directory or parent)
    pub documentation_invariants: Vec<String>,
    /// Compact code skeleton extracted without full implementation bodies
    pub code_skeleton: String,
    /// Full paths of files traversed/included in this context bundle
    pub source_paths: Vec<PathBuf>,
    /// Total estimated tokens
    pub estimated_tokens: usize,
}

/// In-memory cache and loader for hierarchical, token-thrifty context.
#[derive(Debug, Clone, Default)]
pub struct ContextLoader {
    cache: std::sync::Arc<RwLock<HashMap<PathBuf, CachedContextEntry>>>,
}

impl ContextLoader {
    pub fn new() -> Self {
        Self {
            cache: std::sync::Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Fast non-cryptographic content hash for deduplication
    pub fn hash_content(content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Generates a machine-dense AST skeleton for a source code string.
    /// Retains pub structs, enums, traits, function signatures, and doc comments,
    /// completely stripping out implementation bodies.
    pub fn generate_skeleton(content: &str, language_ext: &str) -> String {
        match language_ext {
            "rs" => Self::skeletonize_rust(content),
            "js" | "ts" | "jsx" | "tsx" => Self::skeletonize_ts_js(content),
            _ => content.lines().take(20).collect::<Vec<_>>().join("\n"),
        }
    }

    fn skeletonize_rust(content: &str) -> String {
        let mut out = Vec::new();
        let mut in_comment_block = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("/*") {
                in_comment_block = true;
            }
            if in_comment_block {
                if trimmed.contains("*/") {
                    in_comment_block = false;
                }
                continue;
            }

            // Keep doc comments (/// and //!)
            if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                out.push(line.to_string());
                continue;
            }

            // Ignore standard filler comments
            if trimmed.starts_with("//") {
                continue;
            }

            // Keep attributes like #[derive(...)]
            if trimmed.starts_with("#[") {
                out.push(line.to_string());
                continue;
            }

            // Declarations of structs, enums, traits, type aliases
            if trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub trait ")
                || trimmed.starts_with("pub type ")
                || trimmed.starts_with("struct ")
                || trimmed.starts_with("enum ")
                || trimmed.starts_with("trait ")
            {
                out.push(line.to_string());
                continue;
            }

            // Function signatures: condense bodies
            if trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("fn ")
                || trimmed.starts_with("async fn ")
            {
                if let Some(pos) = line.find('{') {
                    out.push(format!("{} {{ /* ... */ }}", &line[..pos].trim_end()));
                } else if line.ends_with(';') {
                    out.push(line.to_string());
                } else {
                    out.push(format!("{} {{ /* ... */ }}", line.trim_end()));
                }
                continue;
            }

            // Impl headers
            if trimmed.starts_with("impl ") || trimmed.starts_with("pub impl ") {
                out.push(line.to_string());
                continue;
            }

            // Struct fields and enum variants
            if trimmed.starts_with("pub ") && (trimmed.contains(':') || trimmed.contains(',')) {
                out.push(line.to_string());
                continue;
            }

            // Closing braces
            if trimmed == "}" {
                out.push(line.to_string());
            }
        }

        out.join("\n")
    }

    fn skeletonize_ts_js(content: &str) -> String {
        let mut out = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.starts_with("export interface ")
                || trimmed.starts_with("export type ")
                || trimmed.starts_with("export class ")
                || trimmed.starts_with("interface ")
                || trimmed.starts_with("class ")
            {
                out.push(line.to_string());
                continue;
            }
            if trimmed.starts_with("export function ")
                || trimmed.starts_with("export async function ")
                || trimmed.starts_with("function ")
                || trimmed.starts_with("async function ")
            {
                if let Some(pos) = line.find('{') {
                    out.push(format!("{} {{ /* ... */ }}", &line[..pos].trim_end()));
                } else {
                    out.push(format!("{} {{ /* ... */ }}", line.trim_end()));
                }
                continue;
            }
            if trimmed == "}" {
                out.push(line.to_string());
            }
        }
        out.join("\n")
    }

    /// Crawls parent directories upward to locate README.md or ROADMAP.md files.
    pub async fn locate_hierarchical_docs(start_path: &Path, root_limit: &Path) -> Vec<PathBuf> {
        let mut docs = Vec::new();
        let mut curr = if start_path.is_file() {
            start_path.parent().map(Path::to_path_buf)
        } else {
            Some(start_path.to_path_buf())
        };

        while let Some(dir) = curr {
            let readme = dir.join("README.md");
            if readme.is_file() && !docs.contains(&readme) {
                docs.push(readme);
            }
            let roadmap = dir.join("ROADMAP.md");
            if roadmap.is_file() && !docs.contains(&roadmap) {
                docs.push(roadmap);
            }

            if dir == root_limit || dir.parent().is_none() {
                break;
            }
            curr = dir.parent().map(Path::to_path_buf);
        }

        docs
    }

    /// Loads the hierarchical context for a target file or module, using caching
    /// to avoid re-reading identical files.
    pub async fn load_context(
        &self,
        target_path: &Path,
        root_dir: &Path,
        strategy: ContextStrategyKind,
    ) -> Result<HierarchicalContext> {
        let mut result = HierarchicalContext {
            target: target_path.to_path_buf(),
            ..Default::default()
        };

        // 1. Hierarchical docs lookup
        let doc_paths = Self::locate_hierarchical_docs(target_path, root_dir).await;
        for doc_path in doc_paths {
            if let Ok(content) = tokio::fs::read_to_string(&doc_path).await {
                result.source_paths.push(doc_path.clone());
                let snippet = match strategy {
                    ContextStrategyKind::DenseDoc | ContextStrategyKind::ASTSkeleton => {
                        let lines: Vec<&str> = content
                            .lines()
                            .filter(|l| {
                                let t = l.trim();
                                t.starts_with('#')
                                    || t.starts_with('|')
                                    || t.starts_with('-')
                                    || t.starts_with('*')
                            })
                            .take(40)
                            .collect();
                        lines.join("\n")
                    }
                    _ => content.lines().take(40).collect::<Vec<_>>().join("\n"),
                };
                result.documentation_invariants.push(snippet);
            }
        }

        // 2. Code skeleton or content with memory caching
        if target_path.is_file() {
            result.source_paths.push(target_path.to_path_buf());
            let file_str = tokio::fs::read_to_string(target_path).await.unwrap_or_default();
            let current_hash = Self::hash_content(&file_str);

            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get(target_path) {
                if entry.content_hash == current_hash {
                    result.code_skeleton = entry.extracted_content.clone();
                    result.estimated_tokens = entry.token_estimate;
                    return Ok(result);
                }
            }

            let ext = target_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let skeleton = match strategy {
                ContextStrategyKind::ASTSkeleton => Self::generate_skeleton(&file_str, ext),
                _ => file_str.lines().take(50).collect::<Vec<_>>().join("\n"),
            };

            let est_tokens = skeleton.len() / 4;
            cache.insert(
                target_path.to_path_buf(),
                CachedContextEntry {
                    path: target_path.to_path_buf(),
                    content_hash: current_hash,
                    extracted_content: skeleton.clone(),
                    token_estimate: est_tokens,
                },
            );

            result.code_skeleton = skeleton;
            result.estimated_tokens = est_tokens;
        }

        Ok(result)
    }
}
