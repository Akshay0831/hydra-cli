use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A code element that can be indexed and searched
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    // Add more types as needed
}

/// The main code index structure
#[derive(Debug, Default)]
pub struct CodeIndex {
    elements: HashMap<String, CodeElement>,
    file_indices: HashMap<PathBuf, Vec<String>>,
    type_indices: HashMap<ElementType, Vec<String>>,
    dependency_graph: HashMap<String, HashSet<String>>,
}

impl CodeIndex {
    /// Create a new empty code index
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Index a code element
    pub fn add_element(&mut self, element: CodeElement) -> Result<()> {
        let element_id = element.id.clone();
        
        // Add to main elements index
        self.elements.insert(element_id.clone(), element.clone());
        
        // Add to file index
        self.file_indices
            .entry(element.file_path.clone())
            .or_default()
            .push(element_id.clone());
        
        // Add to type index
        self.type_indices
            .entry(element.element_type.clone())
            .or_default()
            .push(element_id.clone());
        
        // Add to dependency graph
        for dep in &element.dependencies {
            self.dependency_graph
                .entry(element_id.clone())
                .or_default()
                .insert(dep.clone());
        }
        
        Ok(())
    }
    
    /// Get all elements in the index
    pub fn get_elements(&self) -> &HashMap<String, CodeElement> {
        &self.elements
    }
    
    /// Get elements by file path
    pub fn get_elements_by_file(&self, file_path: &Path) -> Vec<&CodeElement> {
        self.file_indices
            .get(file_path)
            .map(|ids| ids.iter().filter_map(|id| self.elements.get(id)).collect())
            .unwrap_or_default()
    }
    
    /// Get elements by type
    pub fn get_elements_by_type(&self, element_type: ElementType) -> Vec<&CodeElement> {
        self.type_indices
            .get(&element_type)
            .map(|ids| ids.iter().filter_map(|id| self.elements.get(id)).collect())
            .unwrap_or_default()
    }
    
    /// Find elements by name (case-insensitive)
    pub fn find_elements_by_name(&self, name: &str) -> Vec<&CodeElement> {
        self.elements
            .values()
            .filter(|elem| elem.name.to_lowercase() == name.to_lowercase())
            .collect()
    }
    
    /// Find elements containing text in their content
    pub fn find_elements_by_content(&self, text: &str) -> Vec<&CodeElement> {
        self.elements
            .values()
            .filter(|elem| elem.content.contains(text))
            .collect()
    }
    
    /// Get dependencies for an element
    pub fn get_dependencies(&self, element_id: &str) -> Option<&HashSet<String>> {
        self.dependency_graph.get(element_id)
    }
    
    /// Get dependents (elements that depend on this element)
    pub fn get_dependents(&self, element_id: &str) -> Vec<&CodeElement> {
        let dependents = self.dependency_graph
            .iter()
            .filter(|(_, deps)| deps.contains(element_id))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        
        dependents
            .into_iter()
            .filter_map(|id| self.elements.get(&id))
            .collect()
    }
    
    /// Check if an element exists in the index
    pub fn contains(&self, element_id: &str) -> bool {
        self.elements.contains_key(element_id)
    }
    
    /// Get the total number of indexed elements
    pub fn len(&self) -> usize {
        self.elements.len()
    }
    
    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
    
    /// Remove an element from the index
    pub fn remove_element(&mut self, element_id: &str) -> Option<CodeElement> {
        let element = self.elements.remove(element_id)?;
        
        // Remove from file index
        if let Some(file_elements) = self.file_indices.get_mut(&element.file_path) {
            file_elements.retain(|id| id != element_id);
            if file_elements.is_empty() {
                self.file_indices.remove(&element.file_path);
            }
        }
        
        // Remove from type index
        if let Some(type_elements) = self.type_indices.get_mut(&element.element_type) {
            type_elements.retain(|id| id != element_id);
            if type_elements.is_empty() {
                self.type_indices.remove(&element.element_type);
            }
        }
        
        // Remove from dependency graph
        self.dependency_graph.remove(element_id);
        
        // Remove reverse dependencies
        for (_, deps) in &mut self.dependency_graph {
            deps.remove(element_id);
        }
        
        Some(element)
    }
}

/// A context extraction utility
pub struct ContextExtractor {
    index: CodeIndex,
}

impl ContextExtractor {
    /// Create a new context extractor with an empty index
    pub fn new() -> Self {
        Self {
            index: CodeIndex::new(),
        }
    }
    
    /// Create a new context extractor with an existing index
    pub fn with_index(index: CodeIndex) -> Self {
        Self { index }
    }
    
    /// Extract context around a specific element
    pub fn extract_context(&self, element_id: &str, context_lines: usize) -> Option<ContextInfo> {
        let element = self.index.get_elements().get(element_id)?;
        let file_elements = self.index.get_elements_by_file(&element.file_path);
        
        // Find the element in the file elements
        let element_position = file_elements
            .iter()
            .position(|elem| elem.id == element.id)?;
        
        // Get surrounding elements for context
        let start = element_position.saturating_sub(context_lines);
        let end = std::cmp::min(element_position + context_lines + 1, file_elements.len());
        
        // Clone the elements to create a new Vec<CodeElement>
        let context_elements: Vec<CodeElement> = file_elements[start..end].iter().map(|&elem| elem.clone()).collect();
        
        Some(ContextInfo {
            target_element: element.clone(),
            context_elements,
            file_path: element.file_path.clone(),
        })
    }
    
    /// Extract context for dependencies
    pub fn extract_dependency_context(&self, element_id: &str, context_lines: usize) -> Vec<ContextInfo> {
        let mut contexts = Vec::new();
        
        if let Some(dependencies) = self.index.get_dependencies(element_id) {
            for dep_id in dependencies {
                if let Some(context) = self.extract_context(dep_id, context_lines) {
                    contexts.push(context);
                }
            }
        }
        
        contexts
    }
    
    /// Extract context for dependents
    pub fn extract_dependent_context(&self, element_id: &str, context_lines: usize) -> Vec<ContextInfo> {
        let mut contexts = Vec::new();
        
        let dependents = self.index.get_dependents(element_id);
        for dep in dependents {
            if let Some(context) = self.extract_context(&dep.id, context_lines) {
                contexts.push(context);
            }
        }
        
        contexts
    }
    
    /// Get a reference to the underlying index
    pub fn index(&self) -> &CodeIndex {
        &self.index
    }
    
    /// Get a mutable reference to the underlying index
    pub fn index_mut(&mut self) -> &mut CodeIndex {
        &mut self.index
    }
}

/// Context information for code elements
#[derive(Debug, Clone)]
pub struct ContextInfo {
    pub target_element: CodeElement,
    pub context_elements: Vec<CodeElement>,
    pub file_path: PathBuf,
}

/// A code search utility
pub struct CodeSearch {
    index: CodeIndex,
}

impl CodeSearch {
    /// Create a new code search utility
    pub fn new(index: CodeIndex) -> Self {
        Self { index }
    }
    
    /// Search for elements by name with fuzzy matching
    pub fn search_by_name(&self, query: &str) -> Vec<&CodeElement> {
        let query_lower = query.to_lowercase();
        self.index
            .get_elements()
            .values()
            .filter(|elem| {
                elem.name.to_lowercase().contains(&query_lower)
            })
            .collect()
    }
    
    /// Search for elements by content with fuzzy matching
    pub fn search_by_content(&self, query: &str) -> Vec<&CodeElement> {
        let query_lower = query.to_lowercase();
        self.index
            .get_elements()
            .values()
            .filter(|elem| {
                elem.content.to_lowercase().contains(&query_lower)
            })
            .collect()
    }
    
    /// Search for elements by type
    pub fn search_by_type(&self, element_type: ElementType) -> Vec<&CodeElement> {
        self.index.get_elements_by_type(element_type)
    }
    
    /// Find all elements that match a search pattern
    pub fn search(&self, pattern: &SearchPattern) -> Vec<&CodeElement> {
        match pattern {
            SearchPattern::ByName(name) => self.search_by_name(name),
            SearchPattern::ByContent(content) => self.search_by_content(content),
            SearchPattern::ByType(element_type) => self.search_by_type(element_type.clone()),
            SearchPattern::Combined { name, content, element_type } => {
                let mut results = Vec::new();
                
                if let Some(name) = name {
                    results.extend(self.search_by_name(name));
                }
                
                if let Some(content) = content {
                    results.extend(self.search_by_content(content));
                }
                
                if let Some(element_type) = element_type {
                    results.extend(self.search_by_type(element_type.clone()));
                }
                
                // Remove duplicates
                results.dedup();
                results
            }
        }
    }
}

/// Search patterns for code elements
#[derive(Debug)]
pub enum SearchPattern {
    ByName(String),
    ByContent(String),
    ByType(ElementType),
    Combined {
        name: Option<String>,
        content: Option<String>,
        element_type: Option<ElementType>,
    },
}

/// Initialize the hydra-matrix module
pub fn init() {
    // Initialize the module and register any global utilities
    println!("Hydra matrix initialized");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_code_element_creation() {
        let element = CodeElement {
            id: "test_func".to_string(),
            file_path: PathBuf::from("test.rs"),
            element_type: ElementType::Function,
            name: "test_function".to_string(),
            line_number: 10,
            content: "fn test_function() {}".to_string(),
            dependencies: vec![],
            metadata: HashMap::new(),
        };
        
        assert_eq!(element.id, "test_func");
        assert_eq!(element.name, "test_function");
        assert_eq!(element.line_number, 10);
    }

    #[test]
    fn test_code_index_operations() {
        let mut index = CodeIndex::new();
        
        let element1 = CodeElement {
            id: "func1".to_string(),
            file_path: PathBuf::from("file1.rs"),
            element_type: ElementType::Function,
            name: "function1".to_string(),
            line_number: 1,
            content: "fn function1() {}".to_string(),
            dependencies: vec!["dep1".to_string()],
            metadata: HashMap::new(),
        };
        
        let element2 = CodeElement {
            id: "func2".to_string(),
            file_path: PathBuf::from("file1.rs"),
            element_type: ElementType::Function,
            name: "function2".to_string(),
            line_number: 2,
            content: "fn function2() {}".to_string(),
            dependencies: vec![],
            metadata: HashMap::new(),
        };
        
        // Add elements
        assert!(index.add_element(element1.clone()).is_ok());
        assert!(index.add_element(element2.clone()).is_ok());
        
        // Check element count
        assert_eq!(index.len(), 2);
        assert!(!index.is_empty());
        
        // Check file index
        let file_elements = index.get_elements_by_file(&PathBuf::from("file1.rs"));
        assert_eq!(file_elements.len(), 2);
        
        // Check type index
        let function_elements = index.get_elements_by_type(ElementType::Function);
        assert_eq!(function_elements.len(), 2);
        
        // Check dependencies
        let deps = index.get_dependencies("func1");
        assert!(deps.is_some());
        assert!(deps.unwrap().contains("dep1"));
        
        // Remove element
        assert!(index.contains("func1"));
        let removed = index.remove_element("func1");
        assert!(removed.is_some());
        assert!(!index.contains("func1"));
    }

    #[test]
    fn test_context_extractor() {
        let mut index = CodeIndex::new();
        
        let element = CodeElement {
            id: "test_func".to_string(),
            file_path: PathBuf::from("test.rs"),
            element_type: ElementType::Function,
            name: "test_function".to_string(),
            line_number: 5,
            content: "fn test_function() {}".to_string(),
            dependencies: vec![],
            metadata: HashMap::new(),
        };
        
        index.add_element(element).unwrap();
        
        let extractor = ContextExtractor::with_index(index);
        
        // Extract context for existing element
        let context = extractor.extract_context("test_func", 2);
        assert!(context.is_some());
        assert_eq!(context.unwrap().target_element.id, "test_func");
        
        // Extract context for non-existing element
        let context = extractor.extract_context("nonexistent", 2);
        assert!(context.is_none());
    }

    #[test]
    fn test_code_search() {
        let mut index = CodeIndex::new();
        
        let element1 = CodeElement {
            id: "func1".to_string(),
            file_path: PathBuf::from("file1.rs"),
            element_type: ElementType::Function,
            name: "calculate_sum".to_string(),
            line_number: 1,
            content: "fn calculate_sum(a: i32, b: i32) -> i32 { a + b }".to_string(),
            dependencies: vec![],
            metadata: HashMap::new(),
        };
        
        let element2 = CodeElement {
            id: "struct1".to_string(),
            file_path: PathBuf::from("file1.rs"),
            element_type: ElementType::Struct,
            name: "User".to_string(),
            line_number: 10,
            content: "struct User { name: String, age: u32 }".to_string(),
            dependencies: vec![],
            metadata: HashMap::new(),
        };
        
        index.add_element(element1).unwrap();
        index.add_element(element2).unwrap();
        
        let search = CodeSearch::new(index);
        
        // Search by name
        let results = search.search_by_name("calculate");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "func1");
        
        // Search by content
        let results = search.search_by_content("String");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "struct1");
        
        // Search by type
        let results = search.search_by_type(ElementType::Function);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "func1");
        
        // Combined search
        let pattern = SearchPattern::Combined {
            name: Some("calculate".to_string()),
            content: None,
            element_type: None,
        };
        let results = search.search(&pattern);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "func1");
    }
}
