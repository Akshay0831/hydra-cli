# hydra-matrix

High-performance AST indexing and context loading engine.

| Capability | Implementation | Description |
|---|---|---|
| **AST Indexing** | CodeMatrix | Tree-sitter symbol extraction for Rust, JS, and TS |
| **Context Loader** | ContextLoader | In-memory hash deduplication preventing repeat reads |
| **AST Skeletonizer** | generate_skeleton | Strips function bodies; retains structs, enums, traits |
| **Hierarchical Discovery** | locate_hierarchical_docs | Traverses upward (File -> module -> crate -> root) |
| **Pluggable Strategies** | ContextStrategyKind | Supports ASTSkeleton, DenseDoc, RelevanceScored, SliceWindow |

