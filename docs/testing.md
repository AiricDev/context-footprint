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
│                    Unit Tests (65)                      │
│  domain/, adapters/doc_scorer, scip, ...               │
│  Fast, isolated, test internal logic                   │
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│              Integration Tests (15)                     │
│  graph_builder_test, lib_accessible, policy_comparison │
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

## Unit Tests (65 tests)

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

**Pruning / Policy** (`src/domain/policy.rs` - 2 tests)
- PruningParams default and academic vs strict mode
- Full pruning logic lives in domain; CfSolver takes PruningParams (doc_threshold + treat_typed_documented_function_as_boundary)

### Adapters Layer

**HeuristicDocScorer** (`src/adapters/doc_scorer/heuristic.rs` - 8 tests)
- Length-based score (word count tiers: >5, >10, >20, >50)
- Keyword-based score (returns, args, raises, example); keyword contribution capped at 0.6
- Total score capped at 1.0; empty/no doc returns 0.0

**TiktokenSizeFunction** (`src/adapters/size_function/tiktoken.rs` - 5 tests)
- Single-line and multi-line span extraction
- Boundary conditions and empty spans
- Out-of-range handling

**SCIP Adapter** (`src/adapters/scip/adapter.rs` - 3 tests)
- Load nonexistent file returns error
- Load invalid protobuf returns error
- Empty SCIP index returns empty SemanticData

## Integration Tests (15 tests)

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
- `create_semantic_data_empty_document()`: No definitions → 0 nodes
- `create_semantic_data_multiple_callers()`: One callee, two callers (CallIn edges)
- `create_semantic_data_chain_well_documented_middle()`: A→B→C with B well-documented (policy comparison)
- `source_reader_for_semantic_data()`: Helper to create source readers

### GraphBuilder Tests (`tests/graph_builder_test.rs` - 8 tests)

```rust
test_build_graph_from_semantic_data_simple  // Basic graph construction
test_build_graph_two_files                  // Multi-file project
test_three_pass_creates_nodes_then_edges    // Verify three-pass strategy
test_cycle_fixture_produces_cycle_edges     // Circular dependency handling
test_shared_state_fixture_produces_...      // SharedStateWrite edge creation
test_empty_document_produces_no_nodes       // No definitions → empty graph
test_multiple_writers_all_connected_to_reader  // SharedStateWrite count
test_multiple_callers_all_connected_to_callee // CallIn edges from callee to callers
```

### Policy Comparison Tests (`tests/policy_comparison_test.rs` - 3 tests)

- `test_academic_vs_strict_different_cf`: Same graph, different policies → different CF (Academic stops at well-doc nodes, Strict continues).
- `test_heuristic_scorer_vs_simple_scorer`: Build with Heuristic vs Simple doc scorer → different doc_score on nodes.
- `test_strict_policy_smaller_context_footprint`: CF with Strict policy completes and yields valid result.

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

# Generate HTML report (runs all tests: unit, integration, E2E)
cargo tarpaulin --out Html

# Report is written to tarpaulin-report.html
```

**Coverage target**: 85%+ (domain ≥85%, policies ≥80%, adapters ≥70%).  
**Current coverage**: ~80% (tarpaulin with all tests; HeuristicDocScorer at or near 100%).

## Current Test Statistics

- **Total tests**: 82 (65 unit + 15 integration + 2 E2E)
- **Unit tests**: 65 (~79%)
  - Domain layer: 36 tests (graph, solver, builder)
  - Adapters: 29 tests (academic, strict, heuristic, simple, tiktoken, scip adapter)
- **Integration tests**: 15 (~18%)
  - graph_builder_test: 8, lib_accessible: 4, policy_comparison_test: 3
- **E2E tests**: 2 (~2%)

**All unit and integration tests passing** ✓ (E2E may skip if SCIP fixtures missing)

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
