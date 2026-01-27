# Testing Guide

Comprehensive guide to the context-footprint testing infrastructure.

## Quick Start

```bash
# Run all tests
cargo test --lib --tests

# Run a specific test module
cargo test graph::tests
cargo test solver::tests

# Check code quality
cargo fmt -- --check
cargo clippy -- -D warnings
```

## Test Architecture

### Three-Tier Testing Strategy

```
┌─────────────────────────────────────────────────────────┐
│                    Unit Tests (44)                      │
│  src/domain/graph.rs, solver.rs, policy/academic.rs    │
│  Fast, isolated, test internal logic                   │
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│               Integration Tests (9)                     │
│  tests/graph_builder_test.rs, lib_accessible.rs        │
│  Use mocks, test component interactions                │
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│              End-to-End Tests (2)                       │
│  tests/end_to_end_test.rs                              │
│  Full pipeline with real SCIP indexes                  │
└─────────────────────────────────────────────────────────┘
```

## Unit Tests (44 tests)

Located in `#[cfg(test)]` modules within source files. Can access private functions.

### Domain Layer

**ContextGraph** (`src/domain/graph.rs` - 14 tests)
- Basic operations: create, add node/edge, lookup by symbol
- Edge cases: duplicate symbols, empty neighbors, nonexistent symbols
- Multi-edge support and neighbor iteration

**CfSolver** (`src/domain/solver.rs` - 13 tests)
- Algorithm correctness: single node, linear chains, diamond dependencies
- Cycle detection and handling
- Boundary vs transparent traversal
- Dynamic expansion: SharedStateWrite, CallIn edges
- Policy impact on results

**AcademicBaseline Policy** (`src/adapters/policy/academic.rs` - 9 tests)
- Boundary detection: external nodes, well-documented functions, abstract types
- Transparency: poorly documented functions, concrete types, variables
- Special edge handling: SharedStateWrite (always transparent), CallIn (depends on signature)
- Documentation threshold impact

### Adapters Layer

**TiktokenSizeFunction** (`src/adapters/size_function/tiktoken.rs` - 5 tests)
- Single-line and multi-line span extraction
- Boundary conditions and empty spans
- Out-of-range handling

**SimpleDocScorer** (`src/adapters/doc_scorer/simple.rs` - 3 tests)
- No doc → 0.0
- Empty doc → 0.0
- Valid doc → 1.0

## Integration Tests (9 tests)

Located in `tests/` directory. Use public API only.

### Test Fixtures

**Mock Implementations** (`tests/common/mock.rs`)
- `MockSizeFunction`: Returns fixed size (configurable)
- `MockDocScorer`: Returns configurable score for docs
- `MockSourceReader`: In-memory file system for testing

**Fixture Generators** (`tests/common/fixtures.rs`)
- `create_semantic_data_simple()`: Two functions, one call
- `create_semantic_data_two_files()`: Cross-file dependency
- `create_semantic_data_with_cycle()`: Circular dependencies
- `create_semantic_data_with_shared_state()`: Reader + multiple writers
- `source_reader_for_semantic_data()`: Helper to create source readers

### GraphBuilder Tests (`tests/graph_builder_test.rs` - 5 tests)

```rust
test_build_graph_from_semantic_data_simple  // Basic graph construction
test_build_graph_two_files                  // Multi-file project
test_three_pass_creates_nodes_then_edges    // Verify three-pass strategy
test_cycle_fixture_produces_cycle_edges     // Circular dependency handling
test_shared_state_fixture_produces_...      // SharedStateWrite edge creation
```

### Library Accessibility (`tests/lib_accessible.rs` - 4 tests)

Verifies the library API is usable from integration tests and that mock implementations work.

## End-to-End Tests (2 tests)

Full pipeline: load SCIP → build graph → compute CF.

### Simple Python Project

Minimal fixture (`tests/fixtures/simple_python/`):
- `main.py`: Well-documented function calling helper
- `utils.py`: Poorly documented helper function
- Tests boundary detection based on documentation

**Setup:**
```bash
cd tests/fixtures/simple_python
scip-python index . --output index.scip
```

### FastAPI Project (Real-World)

Real open-source project from BugsInPy dataset.

**Setup:**
```bash
./tests/fixtures/setup_fastapi.sh
```

**What it tests:**
- Handles large codebases (FastAPI ~50k+ LOC)
- Complex dependency graphs
- Real-world code patterns
- Performance characteristics

**Test behavior:**
- If `index.scip` missing: skips gracefully with helpful message
- If present: runs full CF computation on multiple symbols
- Validates: graph structure, CF results, reachability

## CI/CD Integration

### GitHub Actions Workflow

**Test Job:**
```yaml
- Run unit tests (cargo test --lib)
- Run integration tests (cargo test --tests)
- Check formatting (cargo fmt -- --check)
- Run clippy (cargo clippy -- -D warnings)
```

**Coverage Job:**
```yaml
- Generate coverage with Tarpaulin
- Upload to Codecov
```

### Local Test Script

The workflow commands are captured in a local script:
```bash
./scripts/test.sh  # Runs all checks (note: currently deleted, recreate if needed)
```

Or run manually:
```bash
cargo test --lib
cargo test --tests
cargo fmt -- --check
cargo clippy -- -D warnings
```

## Test Development Workflow

### Adding a New Unit Test

1. Locate the source file (e.g., `src/domain/graph.rs`)
2. Add test to `#[cfg(test)] mod tests { ... }`
3. Run: `cargo test graph::tests::test_name`

Example:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_my_feature() {
        let graph = ContextGraph::new();
        // ...
        assert_eq!(result, expected);
    }
}
```

### Adding an Integration Test

1. Create or edit a file in `tests/` (e.g., `tests/my_feature_test.rs`)
2. Import common helpers: `mod common;`
3. Use public API: `use context_footprint::domain::...;`
4. Use fixtures: `use common::fixtures::create_semantic_data_simple;`
5. Run: `cargo test --test my_feature_test`

Example:
```rust
mod common;

use context_footprint::domain::builder::GraphBuilder;
use common::mock::MockSizeFunction;

#[test]
fn test_my_integration() {
    let data = common::fixtures::create_semantic_data_simple();
    // ...
}
```

### Adding a New Fixture

Edit `tests/common/fixtures.rs`:
```rust
pub fn create_semantic_data_for_scenario_x() -> SemanticData {
    SemanticData {
        project_root: "/test".into(),
        documents: vec![
            // ... define documents, definitions, references
        ],
        external_symbols: vec![],
    }
}
```

## Debugging Tests

```bash
# Show println! output
cargo test -- --nocapture

# Run tests serially (easier to debug)
cargo test -- --test-threads=1

# Run specific test with backtrace
RUST_BACKTRACE=1 cargo test test_cycle_detection -- --nocapture

# Watch mode (with cargo-watch)
cargo watch -x "test --lib"
```

## Test Coverage

View coverage locally:
```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate HTML report
cargo tarpaulin --out Html

# Open coverage/index.html in browser
```

## Current Test Statistics

- **Total tests**: 55
- **Unit tests**: 44 (80%)
  - Domain layer: 36 tests
  - Adapters layer: 8 tests
- **Integration tests**: 9 (16%)
- **E2E tests**: 2 (4%)

**All tests passing** ✓

## Known Limitations

1. **Doc tests disabled**: Generated SCIP protobuf code has doc-test incompatible comments
2. **FastAPI fixture optional**: E2E test skips if not set up (requires `scip-python` installation)
3. **No property-based tests yet**: Can be added in future for invariant testing

## Future Enhancements

- [ ] Add property-based tests with `proptest` for graph invariants
- [ ] Benchmark tests with `criterion` for performance regression detection
- [ ] More real-world fixtures (different languages: TypeScript, Java, Rust)
- [ ] Mutation testing with `cargo-mutants`
- [ ] Snapshot testing for graph structure
