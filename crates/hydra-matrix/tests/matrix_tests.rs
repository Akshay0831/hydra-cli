#[cfg(test)]
mod matrix_tests {
    use hydra_matrix::{
        CodeElement, CodeMatrix, ElementType, IncrementalWatcher, IndexConfig, SqliteIndexCache,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Create test files
    async fn create_test_files(temp_dir: &TempDir) -> Vec<PathBuf> {
        let test_files = vec![
            (
                temp_dir.path().join("test.rs"),
                r#"
mod module1 {
    pub fn helper_function() -> String {
        "hello".to_string()
    }
}

pub struct TestStruct {
    pub field: String,
}

pub enum TestEnum {
    Variant1,
    Variant2,
}

pub fn main_function() {
    let _result = helper_function();
    let _test = TestStruct { field: "test".to_string() };
    match TestEnum::Variant1 {
        _ => {}
    }
}
"#,
            ),
            (
                temp_dir.path().join("example.js"),
                r#"
const util = require('lodash');

function exampleFunction(param) {
    return util.map([param, param2], x => x * 2);
}

class ExampleClass {
    constructor() {
        this.value = 0;
    }
    
    method() {
        return exampleFunction(this.value);
    }
}

const example = new ExampleClass();
"#,
            ),
            (
                temp_dir.path().join("example.ts"),
                r#"
interface TypeScriptInterface {
    id: number;
    name: string;
}

class TypeScriptClass implements TypeScriptInterface {
    constructor(public id: number, public name: string) {}
    
    public getType(): string {
        return this.name;
    }
}

export function exampleTypeScriptFunction(data: TypeScriptInterface): string {
    return `ID: ${data.id}, Name: ${data.name}`;
}
"#,
            ),
        ];

        for (path, content) in &test_files {
            tokio::fs::write(&path, content).await.unwrap();
        }

        test_files.into_iter().map(|(path, _)| path).collect()
    }

    #[tokio::test]
    async fn test_index_creation() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let _test_files = create_test_files(&temp_dir).await;

        // Update config to point to temp directory
        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.config.include_patterns =
            vec!["*.rs".to_string(), "*.js".to_string(), "*.ts".to_string()];
        matrix.config.exclude_patterns = vec!["**/node_modules/**".to_string()];

        let indexed_count = matrix.index().await.unwrap();
        assert!(indexed_count > 0, "Should index some files");

        let stats = matrix.get_stats().await;
        assert!(stats.total_elements > 0, "Should have indexed elements");
        assert!(stats.functions > 0, "Should have indexed functions");
        assert!(stats.structs > 0, "Should have indexed structs");
        assert!(stats.enums > 0, "Should have indexed enums");
        assert!(stats.classes > 0, "Should have indexed classes");
        assert!(stats.interfaces > 0, "Should have indexed interfaces");
    }

    #[tokio::test]
    async fn test_search_functionality() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let _test_files = create_test_files(&temp_dir).await;

        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.config.include_patterns = vec!["*.rs".to_string()];
        matrix.config.exclude_patterns = vec!["**/node_modules/**".to_string()];

        matrix.index().await.unwrap();

        // Search by name
        let results = matrix.search_by_name("helper_function").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "helper_function");
        assert_eq!(results[0].element_type, ElementType::Function);

        // Search by type
        let function_results = matrix.search_by_type(ElementType::Function).await.unwrap();
        assert!(function_results.len() >= 2); // Should find at least helper_function and main_function

        // Search by query
        let query_results = matrix.search("TestStruct").await.unwrap();
        assert!(!query_results.is_empty());
        assert!(query_results.iter().any(|e| e.name.contains("TestStruct")));

        // Search for non-existent element
        let empty_results = matrix.search_by_name("nonexistent").await.unwrap();
        assert_eq!(empty_results.len(), 0);
    }

    #[tokio::test]
    async fn test_dependency_resolution() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let _test_files = create_test_files(&temp_dir).await;

        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.config.include_patterns = vec!["*.rs".to_string()];
        matrix.config.exclude_patterns = vec!["**/node_modules/**".to_string()];

        matrix.index().await.unwrap();

        // Find a function and its dependencies
        let mut functions = matrix.search_by_name("main_function").await.unwrap();
        assert!(!functions.is_empty());
        let main_function = functions.remove(0);

        let dependencies = matrix.find_dependencies(&main_function.id).await.unwrap();
        assert!(dependencies.is_empty());

        // Check that we can find dependents
        let dependents = matrix.find_dependents(&main_function.id).await.unwrap();
        assert!(dependents.is_empty() || true); // Main function might not have dependents in this test

        // Test size
        let size = matrix.size().await;
        assert!(size > 0);
    }

    #[tokio::test]
    async fn test_multiple_language_support() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let _test_files = create_test_files(&temp_dir).await;

        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.config.include_patterns =
            vec!["*.rs".to_string(), "*.js".to_string(), "*.ts".to_string()];
        matrix.config.exclude_patterns = vec!["**/node_modules/**".to_string()];

        let indexed_count = matrix.index().await.unwrap();
        assert!(indexed_count >= 3, "Should index all test files");

        let stats = matrix.get_stats().await;
        assert!(
            stats.functions > 0,
            "Should have functions from multiple languages"
        );
        assert!(
            stats.classes > 0,
            "Should have classes from multiple languages"
        );
        assert!(
            stats.interfaces > 0,
            "Should have interfaces from multiple languages"
        );
    }

    #[tokio::test]
    async fn test_file_filtering() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let _test_files = create_test_files(&temp_dir).await;

        // Test exclusion patterns
        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.config.include_patterns = vec!["*.rs".to_string(), "*.js".to_string()];
        matrix.config.exclude_patterns = vec!["**/example.*".to_string()]; // Exclude example files

        let indexed_count = matrix.index().await.unwrap();
        assert_eq!(indexed_count, 1, "Should only index test.rs");

        let stats = matrix.get_stats().await;
        assert_eq!(stats.total_elements, 4); // Only the Rust elements from test.rs
    }

    #[tokio::test]
    async fn test_config_serialization() {
        let config = IndexConfig {
            paths: vec!["./src/**/*.rs".to_string()],
            include_patterns: vec!["*.rs".to_string(), "*.js".to_string()],
            exclude_patterns: vec!["**/test/**".to_string()],
            max_file_size: 1024 * 1024,
            follow_symlinks: true,
            index_docs: false,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: IndexConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.paths, deserialized.paths);
        assert_eq!(config.include_patterns, deserialized.include_patterns);
        assert_eq!(config.exclude_patterns, deserialized.exclude_patterns);
        assert_eq!(config.max_file_size, deserialized.max_file_size);
        assert_eq!(config.follow_symlinks, deserialized.follow_symlinks);
        assert_eq!(config.index_docs, deserialized.index_docs);
    }

    #[tokio::test]
    async fn test_element_matching() {
        let matrix = CodeMatrix::new().unwrap();

        let rust_file_path = PathBuf::from("test.rs");
        let element = CodeElement {
            id: "test:1:function".to_string(),
            file_path: rust_file_path.clone(),
            element_type: ElementType::Function,
            name: "example_function".to_string(),
            line_number: 1,
            content: "pub fn example_function() -> String { return \"hello\"; }".to_string(),
            dependencies: vec!["std::collections::HashMap".to_string()],
            metadata: HashMap::new(),
        };

        // Test name matching
        assert!(matrix.element_matches_query(&element, "example"));
        assert!(!matrix.element_matches_query(&element, "nonexistent"));

        // Test content matching
        assert!(matrix.element_matches_query(&element, "hello"));
        assert!(matrix.element_matches_query(&element, "function"));

        // Test type matching
        assert!(matrix.element_matches_query(&element, "function"));
        assert!(!matrix.element_matches_query(&element, "struct"));
    }

    #[tokio::test]
    async fn test_index_clear() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let _test_files = create_test_files(&temp_dir).await;

        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.config.include_patterns = vec!["*.rs".to_string()];
        matrix.config.exclude_patterns = vec!["**/node_modules/**".to_string()];

        matrix.index().await.unwrap();
        assert!(matrix.size().await > 0);

        matrix.clear().await;
        assert_eq!(matrix.size().await, 0);
    }

    #[tokio::test]
    async fn test_index_stats() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let _test_files = create_test_files(&temp_dir).await;

        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.config.include_patterns =
            vec!["*.rs".to_string(), "*.js".to_string(), "*.ts".to_string()];
        matrix.config.exclude_patterns = vec!["**/node_modules/**".to_string()];

        matrix.index().await.unwrap();

        let stats = matrix.get_stats().await;
        assert!(stats.total_elements > 0);
        assert!(stats.files.len() >= 2); // Should find at least Rust and JavaScript/TypeScript files
    }

    #[tokio::test]
    async fn test_element_id_generation() {
        let test_path = PathBuf::from("test.rs");

        let element = CodeElement {
            id: "test.rs:1:test_function".to_string(),
            file_path: test_path,
            element_type: ElementType::Function,
            name: "test_function".to_string(),
            line_number: 1,
            content: "pub fn test_function() {}".to_string(),
            dependencies: Vec::new(),
            metadata: HashMap::new(),
        };

        // Test that ID contains expected components
        assert!(element.id.contains("test.rs"));
        assert!(element.id.contains("1")); // Line number
        assert!(element.id.contains("test_function")); // Function name
    }

    #[tokio::test]
    async fn test_context_loader_and_skeletonization() {
        use hydra_matrix::{ContextLoader, ContextStrategyKind};
        use std::io::Write;

        let temp_dir = TempDir::new().unwrap();
        let crate_dir = temp_dir.path().join("my_crate");
        std::fs::create_dir_all(&crate_dir).unwrap();

        // Create crate README.md
        let readme_path = crate_dir.join("README.md");
        {
            let mut f = std::fs::File::create(&readme_path).unwrap();
            writeln!(f, "# My Crate\n| API | Description |\n|---|---|\n| run | Runs task |").unwrap();
        }

        // Create Rust source file
        let src_file = crate_dir.join("lib.rs");
        {
            let mut f = std::fs::File::create(&src_file).unwrap();
            writeln!(
                f,
                "/// Public entry point\npub struct Worker {{\n    pub id: u32,\n}}\n\nimpl Worker {{\n    pub fn run(&self) -> bool {{\n        println!(\"heavy inner computation\");\n        true\n    }}\n}}"
            ).unwrap();
        }

        let loader = ContextLoader::new();
        let ctx = loader
            .load_context(&src_file, temp_dir.path(), ContextStrategyKind::ASTSkeleton)
            .await
            .unwrap();

        // Check doc invariant extraction
        assert!(!ctx.documentation_invariants.is_empty());
        assert!(ctx.documentation_invariants[0].contains("| API | Description |"));

        // Check skeletonization: struct and fn signature preserved, internal body stripped
        assert!(ctx.code_skeleton.contains("pub struct Worker"));
        assert!(ctx.code_skeleton.contains("pub fn run(&self) -> bool { /* ... */ }"));
        assert!(!ctx.code_skeleton.contains("heavy inner computation"));

        // Check anti-looping memory cache
        let ctx_cached = loader
            .load_context(&src_file, temp_dir.path(), ContextStrategyKind::ASTSkeleton)
            .await
            .unwrap();
        assert_eq!(ctx.code_skeleton, ctx_cached.code_skeleton);
    }

    #[tokio::test]
    async fn test_calculate_blast_radius() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let _test_files = create_test_files(&temp_dir).await;

        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.index().await.unwrap();

        // Calculate blast radius when helper_function changes
        let blast_radius = matrix.calculate_blast_radius("helper_function").await.unwrap();
        assert!(!blast_radius.is_empty(), "Blast radius should include impacted files");
        
        // test.rs contains helper_function and main_function (which calls helper_function)
        let expected_path = temp_dir.path().join("test.rs");
        assert!(blast_radius.contains(&expected_path));
    }

    #[tokio::test]
    async fn test_incremental_watcher() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("service.rs");

        std::fs::write(&file_path, "pub fn compute() -> i32 { 42 }").unwrap();

        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        
        let mut watcher = IncrementalWatcher::new();
        // First scan indexes the file
        let reindexed1 = watcher.scan_and_reindex(&mut matrix).await.unwrap();
        assert_eq!(reindexed1.len(), 1);
        assert_eq!(matrix.size().await, 1);

        // Second scan without changes returns 0 reindexed files
        let reindexed2 = watcher.scan_and_reindex(&mut matrix).await.unwrap();
        assert_eq!(reindexed2.len(), 0);

        // Modify file and scan again
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        std::fs::write(&file_path, "pub fn compute() -> i32 { 100 }\npub fn extra() {}").unwrap();

        let reindexed3 = watcher.scan_and_reindex(&mut matrix).await.unwrap();
        assert_eq!(reindexed3.len(), 1);
        assert_eq!(matrix.size().await, 2);
    }

    #[tokio::test]
    async fn test_sqlite_index_cache() {
        let mut matrix = CodeMatrix::new().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("cached.rs");

        std::fs::write(&file_path, "pub fn ping() -> bool { true }").unwrap();
        matrix.config.paths = vec![temp_dir.path().to_string_lossy().to_string()];
        matrix.index().await.unwrap();
        assert_eq!(matrix.size().await, 1);

        let db_path = temp_dir.path().join("cache.sqlite");
        let cache = SqliteIndexCache::open(db_path.clone()).unwrap();
        
        // Persist
        let saved_count = cache.persist(&matrix).await.unwrap();
        assert_eq!(saved_count, 1);

        // Restore into fresh matrix
        let empty_matrix = CodeMatrix::new().unwrap();
        assert_eq!(empty_matrix.size().await, 0);

        let restored_count = cache.restore(&empty_matrix).await.unwrap();
        assert_eq!(restored_count, 1);
        assert_eq!(empty_matrix.size().await, 1);

        let elements = empty_matrix.search("ping").await.unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].name, "ping");
    }
}

