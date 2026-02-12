# ADR-004: Semantic Data Abstraction Layer

**Status**: Accepted  
**Date**: 2026-02  
**Deciders**: Core team

## Context

Context Footprint analysis requires rich semantic information about code structure: function signatures, variable scopes, type hierarchies, and cross-references. This information traditionally comes from language-specific indexers (like SCIP, LSP servers, or AST parsers).

**Problem**: Tight coupling to specific indexer formats makes the domain logic:
- **Difficult to test** without external indexers running
- **Hard to extend** to new languages (each requires adapter implementation)
- **Fragile** when indexer schemas change
- **Opaque** when debugging graph construction issues

**Core Insight**: The CF algorithm doesn't need indexer-specific formats—it needs a stable, well-documented interface that describes code semantics in graph construction terms.

## Decision

Define `SemanticData` as a **domain-level abstraction** that sits between language extractors and graph construction. This model is:

1. **Indexer-agnostic**: No dependency on SCIP, LSP, or any specific tool
2. **Graph-oriented**: Designed explicitly for `ContextGraph` construction needs
3. **JSON-serializable**: Enables testing, debugging, and external tool integration
4. **Contract-driven**: Every field has explicit "Adapter Contract" documentation

### Core Design

**Location**: `src/domain/semantic.rs` (~1000 lines)

**Three Symbol Kinds** (simplified from original 7):
- `Function`: Callable units (functions, methods, constructors)
- `Variable`: Data storage (globals, fields, parameters)
- `Type`: User-defined types (classes, interfaces, protocols, enums)

**Key Structures**:
```rust
pub struct SemanticData {
    pub project_root: PathBuf,
    pub documents: Vec<DocumentInfo>,
    pub external_symbols: Vec<ExternalSymbol>,
}

pub struct Definition {
    pub symbol_id: String,           // Unique identifier
    pub kind: SymbolKind,
    pub name: String,
    pub context_size: usize,         // Token count
    pub doc_score: f64,              // Documentation quality (0.0-1.0)
    pub is_external: bool,           // Third-party library flag
    // ... kind-specific fields
}

pub struct Reference {
    pub from_symbol: String,
    pub to_symbol: String,
    pub ref_kind: RefKind,           // Call, Read, Write, etc.
    pub receiver: Option<String>,    // Instance vs static access
}
```

**Adapter Contract**: Every field has documentation specifying:
- **What extractors must provide**: Exact semantics expected
- **How values map to graph attributes**: Direct field usage
- **Edge cases**: null/empty handling, ambiguous scenarios

## Rationale

### Why Domain-Level (Not Adapter-Level)

The original AGENTS.md states adapters implement domain traits—why is `SemanticData` in the domain?

**Answer**: `SemanticData` is a **port** (interface specification), not an adapter (implementation). It defines the contract that all extractors must fulfill, similar to how `SourceReader` trait defines what file readers must provide.

```
Language Extractors (Python, TypeScript, etc.)
              ↓
     [Implement SemanticData Contract]
              ↓
          JSON Format
              ↓
     src/domain/builder.rs (GraphBuilder)
              ↓
        ContextGraph
```

### Why JSON Intermediate Format

1. **Testability**: Write SemanticData by hand for unit tests (no indexer needed)
2. **Debuggability**: Inspect exact data fed to graph builder
3. **Extensibility**: External tools can generate CF-compatible data
4. **Validation**: Catch schema issues before graph construction

### Why Three Symbol Kinds (Not Seven)

Original design had `Class`, `Interface`, `Protocol`, `Struct`, `Enum`, `Union`, `TypeAlias`. Reduced to `Function`, `Variable`, `Type` because:

- **Graph construction doesn't need fine-grained type distinctions**: A Protocol and a Class both contribute nodes and type relationships
- **Extractors vary in terminology**: Python has Protocols, TypeScript has Interfaces, Go has interfaces—unified as `Type`
- **Simplifies adapter contract**: Fewer variants = clearer expectations
- **Type-specific behavior encoded in attributes**: `is_interface_method`, `is_abstract`, etc.

## Consequences

### Benefits

✅ **Domain isolation**: Graph construction testable without external tools  
✅ **Multi-language support**: New language = implement JSON extractor (no Rust changes needed)  
✅ **Clear contracts**: Adapter authors know exactly what to extract  
✅ **Debugging workflow**: `extract → inspect JSON → build graph` makes issues visible  
✅ **Version stability**: Domain API stable even when indexer formats evolve

### Trade-offs

⚠️ **Adapter complexity**: Extractors must handle more semantic analysis (can't delegate to domain)  
⚠️ **JSON overhead**: Extra serialization step (negligible for typical projects)  
⚠️ **Contract maintenance**: Field semantics must be documented and kept current

### Current Limitations

1. **No type inference**: Extractors must provide declared types only
2. **No cross-module resolution**: Symbol IDs are module-relative (requires global analysis)
3. **Receiver ambiguity**: Instance method calls need heuristics when receiver type unclear

## Implementation

### File Structure

```
src/domain/semantic.rs           # SemanticData model + Adapter Contract
src/domain/builder.rs            # GraphBuilder consumes SemanticData
src/app/engine.rs                # load_from_json() entry point
```

### Example: Python Extractor

```bash
# Extract semantics using AST parser
python3 scripts/extract_python_semantics.py /path/to/project --output semantics.json

# Build graph from JSON
cftool build-from-json semantics.json --symbol "module.function#Function"
```

**Python extractor capabilities** (590 lines, no external dependencies):
- ✅ Functions (methods, async, generators)
- ✅ Types (classes, protocols, abstract classes)
- ✅ Variables (globals, fields)
- ✅ Abstract method detection (`@abstractmethod`, Protocol methods)
- ✅ Receiver inference for method calls
- ✅ JSON output compatible with Rust serde

### Usage in Tests

```rust
// Unit test: hand-written SemanticData
let semantic = SemanticData {
    documents: vec![DocumentInfo {
        definitions: vec![
            Definition {
                symbol_id: "main.foo#Function".to_string(),
                kind: SymbolKind::Function,
                context_size: 50,
                doc_score: 0.8,
                // ...
            },
        ],
        references: vec![/* ... */],
    }],
    // ...
};
let graph = GraphBuilder::new().build(semantic)?;
```

### JSON Schema (Simplified)

```json
{
  "project_root": "/path/to/project",
  "documents": [
    {
      "file_path": "main.py",
      "definitions": [
        {
          "symbol_id": "main.process_file#Function",
          "kind": "Function",
          "name": "process_file",
          "context_size": 45,
          "doc_score": 0.6,
          "typed_param_count": 2,
          "has_return_type": true
        }
      ],
      "references": [
        {
          "from_symbol": "main.process_file#Function",
          "to_symbol": "main.FileReader.read#Function",
          "ref_kind": "Call",
          "receiver": "main.FileReader#Type"
        }
      ]
    }
  ],
  "external_symbols": []
}
```

## Related Decisions

- **ADR-001 (Hexagonal Architecture)**: SemanticData is a domain port; extractors are adapters
- **ADR-003 (Pruning in Domain)**: SemanticData provides `doc_score` field for pruning logic

## References

- **Implementation**: `src/domain/semantic.rs` (full Adapter Contract inline)
- **Example Extractor**: `scripts/extract_python_semantics.py`
- **Test Fixture**: `tests/fixtures/simple_python_for_extract/`
- **CLI Integration**: `cftool build-from-json` command
