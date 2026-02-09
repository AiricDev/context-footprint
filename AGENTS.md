# AI Agent Development Guide

> Quick reference for AI agents: architecture decisions, project-specific conventions, and development workflow.

## 📐 Core Concept

**Context Footprint (CF)**: Quantify code coupling via conditional graph traversal.

- **Model**: Directed graph (nodes = code units, edges = dependencies)
- **Metric**: Total tokens reachable from starting node
- **Innovation**: Traversal stops at "good abstractions" (typed + documented) but continues through "leaky" ones

**Design Details**: See [`docs/design.md`](docs/design.md) for formal definition and algorithm.

## 🏗️ Architecture Decisions (ADR)

### ADR-001: Hexagonal Architecture (Ports & Adapters)

**Decision**: Strict separation between domain logic (`src/domain/`) and external integrations (`src/adapters/`)

**Rationale**:
- Domain algorithms must be testable without external dependencies (file I/O, indexers)
- Future language support (TypeScript, Java) only requires new semantic data extractors
- Policy experimentation (different pruning strategies) isolated from core traversal

**Implementation**:
```
Domain Layer (src/domain/)     Adapters Layer (src/adapters/)
  ├─ graph.rs                    ├─ doc_scorer/    (doc quality)
  ├─ solver.rs                   ├─ size_function/ (token counting)
  ├─ builder.rs                  └─ test_detector/
  └─ ports.rs (traits)  ←──────── (semantic data: JSON from LSP extractor, etc.)
```

**Constraints**:
- Domain depends only on `std` + `petgraph`
- Adapters implement domain traits (`SourceReader`, etc.)

---

### ADR-002: Three-Pass Graph Construction

**Decision**: Build graph in three sequential passes (not single-pass)

**Rationale**:
- Semantic data may reference symbols before they're defined (forward references)
- Dynamic edges (SharedStateWrite, CallIn) require full static graph first
- Metrics (`context_size`, `doc_score`) need source file access at node creation

**Passes**:
1. **Allocate nodes** from semantic definitions → compute metrics via source spans
2. **Wire static edges** from semantic references → standard dependencies
3. **Add dynamic edges** → reverse lookups for state writers and callers

**File**: `src/domain/builder.rs`

---

### ADR-003: Pruning Logic Fully in Domain

**Decision**: Pruning logic lives entirely in domain; no policy trait. Only `doc_threshold` and a mode flag are configurable.

**Rationale**:
- "Good abstraction" rules (external → boundary, variable → transparent, abstract factory, sig+doc) are core algorithm
- `doc_threshold` (with doc_scorer supplying `doc_score`) gives enough flexibility
- CfSolver takes `PruningParams { doc_threshold, treat_typed_documented_function_as_boundary }`; engine maps `PolicyKind` → params

**Domain layer** (`src/domain/policy.rs`):
- `PruningParams`: doc_threshold + treat_typed_documented_function_as_boundary (Academic vs Strict)
- `evaluate(params, source, target, edge_kind, graph)`: full pruning algorithm
- `is_abstract_factory()`: abstract-factory detection (always boundary)

**No adapters** for policy; engine uses `PruningParams::academic(0.5)` or `PruningParams::strict(0.8)` from `PolicyKind`.

---

### ADR-004: Semantic Data Abstraction

**Decision**: Define indexer-agnostic `SemanticData` model in domain layer

**Rationale**:
- Semantic data is consumed as JSON (e.g. from LSP-based extractors)
- Enables testing without external indexers; graph built from in-memory or file JSON
- Future: support other formats or indexers without changing domain

**Boundary**: `SemanticData` (domain) ← JSON file / LSP extractor output

## 🗂️ Key Domain Concepts

### Core Types

| Type | Purpose | File |
|------|---------|------|
| `ContextGraph` | Directed graph (nodes + edges) + symbol lookup | `src/domain/graph.rs` |
| `Node` | Code unit (Function/Variable/Type) with metrics | `src/domain/node.rs` |
| `EdgeKind` | Dependency type (Call/Read/Write/ParamType/etc.) | `src/domain/edge.rs` |
| `CfSolver` | BFS traversal with conditional pruning | `src/domain/solver.rs` |
| `PruningParams` | doc_threshold + treat_typed_documented_function_as_boundary; CfSolver parameter | `src/domain/policy.rs` |
| `evaluate` | Core pruning algorithm (domain) | `src/domain/policy.rs` |
| `SemanticData` | Indexer-agnostic semantic model | `src/domain/semantic.rs` |

### Critical Node Attributes

Every node has:
- **`context_size`**: Token count (basis for CF calculation)
- **`doc_score`**: Documentation quality (0.0-1.0, used by policies)
- **`is_external`**: Third-party library flag (always acts as boundary)

Function-specific attributes:
- **`is_interface_method`**: Flag indicating method is defined in Interface/Protocol/Trait/Abstract Class (only signature, no implementation)
- **`typed_param_count`**: Number of parameters with type annotations (drives signature completeness check)
- **`has_return_type`**: Whether return type is specified

### Dynamic Expansion Edges

Two special edge types added in Pass 3:
- **`SharedStateWrite`**: Reader → Writer (mutable global state penalty)
- **`CallIn`**: Untyped function → Callers (resolve vague signatures from usage)

## 🧪 Testing Conventions

**Strategy**: Test pyramid (140 tests total: 100 unit, 34 integration, 6 E2E)

**Details**: See [`docs/testing.md`](docs/testing.md) for comprehensive guide.

### Quick Reference

```bash
# Run all tests
cargo test --lib --tests

# E2E fixture setup
./tests/fixtures/setup_fastapi.sh  # Clone FastAPI for real-world test
```

### Test Data Strategy

| Test Type | Data Source | Location |
|-----------|-------------|----------|
| Unit | Inline helpers | `#[cfg(test)]` modules in source files |
| Integration | Mock fixtures | `tests/common/fixtures.rs` generators |
| E2E | Real semantic data JSON | `tests/fixtures/simple_python/`, etc. |

**Key Convention**: E2E tests gracefully skip if semantic data file missing (CI-friendly).

## 🔄 Development Workflow

### TDD Cycle

1. **Write failing test** (domain unit test or integration test)
2. **Implement minimal code** (pass the test)
3. **Run quality checks**: `cargo fmt && cargo clippy -- -D warnings`
4. **Commit** when all tests pass

### Pre-Commit Checklist

```bash
cargo fmt                         # Format code
cargo test --lib --tests          # All tests pass
cargo clippy --all-targets -- -D warnings  # No warnings
```

## 📋 Project-Specific Conventions

### Code Organization Rules

1. **Domain-first implementation**: Write domain logic before adapters (enables testing without I/O)
2. **Trait-based boundaries**: All external dependencies injected via traits (see `src/domain/ports.rs`)
3. **Error propagation**: Use `Result<T>` + `.context()` (no `.unwrap()` in production code)

### Naming Patterns

- **Test functions**: `test_<scenario>_<expected_result>`
  - Example: `test_boundary_node_stops_traversal_but_included_in_reachable_set`
- **Fixture generators**: `create_semantic_data_<scenario>` in `tests/common/fixtures.rs`
  - Example: `create_semantic_data_with_shared_state()`

### Module Structure

**Adapters grouped by concern** (not by implementation):
```
src/adapters/
 ├─ doc_scorer/
 ├─ size_function/
 └─ test_detector/
```
(Semantic data is loaded from JSON, e.g. produced by LSP extractors. Pruning is domain-only.)

### Testing Helpers

- **Dead code allowed**: Add `#![allow(dead_code)]` to `tests/common/*.rs` (helpers used selectively)
- **Mock implementations**: Prefer constructor chaining (`.with_file().with_file()`)
- **E2E resilience**: Always check fixture existence and skip gracefully

## 🔧 Extension Points

### Adding New Node/Edge Types

1. Update enums in `src/domain/node.rs` or `src/domain/edge.rs`
2. Modify `src/domain/builder.rs` creation logic
3. Update pruning logic in `src/domain/policy.rs` if needed
4. Add tests

### Changing Pruning Behaviour

- **Domain**: Adjust `PruningParams` (doc_threshold, treat_typed_documented_function_as_boundary) or extend `evaluate()` in `src/domain/policy.rs`.
- **Engine**: Map new `PolicyKind` variants to `PruningParams` in `pruning_params()` (e.g. `PolicyKind::Custom => PruningParams { doc_threshold: 0.7, ... }`).
- **Doc quality**: Use a different `DocumentationScorer` adapter; `doc_score` on nodes drives boundary decisions via doc_threshold.

### Adding New Language Support

1. Implement a semantic data extractor (e.g. LSP-based) that outputs `SemanticData` JSON
2. Use `ContextEngine::load_from_json(path)` or build graph via `GraphBuilder` with domain `SemanticData`
3. Add E2E test in `tests/fixtures/<language>/` with a sample JSON fixture

## 📊 Quality Standards

### CI Requirements

All commits must pass (`.github/workflows/test.yml`):
- Unit + integration tests
- `cargo fmt -- --check`
- `cargo clippy -- -D warnings`
- Coverage report to Codecov

### Current Status

- **Tests**: 55 (44 unit, 9 integration, 2 E2E) ✅
- **Coverage**: Domain ≥85%, Policies ≥80%, Adapters ≥70%

## 🚨 Known Issues

### Doc Tests

**Workaround**: Use `cargo test --lib --tests` if any doc tests are skipped.

**Fix applied**: `#[allow(clippy::doc_overindented_list_items)]` in `src/lib.rs`

### Test Helper Dead Code

**Issue**: Clippy warns about unused helpers in `tests/common/`

**Fix**: `#![allow(dead_code)]` at module top (helpers used selectively across test binaries)

## 📚 References

- **Algorithm & Design**: [`docs/design.md`](docs/design.md) (formal CF definition, traversal rules)
- **Testing Guide**: [`docs/testing.md`](docs/testing.md) (comprehensive test strategy)
- **Theoretical Foundation**: [`docs/the-paper.md`](docs/the-paper.md)

### External Resources

- Semantic data is JSON-serialized `SemanticData` (e.g. from LSP-based extractors)
- [petgraph](https://docs.rs/petgraph/) (Rust graph library)

---

## 🎓 Terminology

| Term | Definition |
|------|------------|
| **Context Footprint (CF)** | Total tokens reachable from a node via conditional traversal |
| **Boundary Node** | Well-abstracted code (typed + documented) where traversal stops |
| **Transparent Node** | Leaky abstraction (untyped/undocumented) where traversal continues |
| **Dynamic Expansion** | Reverse edges (SharedStateWrite, CallIn) added in Pass 3 |
| **Semantic Data** | Domain model for definitions/references (JSON from LSP or other extractors) |
