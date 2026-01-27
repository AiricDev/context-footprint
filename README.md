# Context Footprint

> **Quantify code coupling through context-aware graph traversal**

A tool to measure software coupling by computing the **Context Footprint (CF)** — the total token volume required to understand a code unit. Unlike traditional metrics, CF distinguishes between well-abstracted boundaries (documented + typed) and leaky abstractions that force readers to explore implementation details.

## 🎯 Core Concept

**Context Footprint** measures coupling by simulating how humans read code:
- Start from a function/class
- Follow dependencies (calls, types, variables)
- **Stop** at well-documented interfaces with complete type signatures
- **Continue** through poorly documented or untyped code
- Sum total tokens in the reachable subgraph

**Result**: A single number representing the "cognitive load" to understand that code unit.

## ✨ Key Features

- **🔍 Conditional Traversal**: Respects abstraction boundaries (unlike naive transitive closure)
- **📊 Token-Based Metric**: Objective measurement using standard tokenizers (cl100k_base)
- **🌐 Language Agnostic**: Built on [SCIP](https://github.com/sourcegraph/scip) protocol (Python, TypeScript, Java, Go, etc.)
- **🔬 Hexagonal Architecture**: Clean separation between domain logic and adapters
- **🧪 Comprehensive Tests**: 54 tests (unit, integration, E2E with real-world projects)
- **🎛️ Configurable Policies**: Swap pruning strategies for different use cases

## 🚀 Quick Start

### Prerequisites

- Rust 1.70+ (`cargo --version`)
- A SCIP indexer for your target language:
  - Python: [`scip-python`](https://github.com/sourcegraph/scip-python)
  - TypeScript: [`scip-typescript`](https://github.com/sourcegraph/scip-typescript)
  - [More languages...](https://github.com/sourcegraph/scip)

### Installation

```bash
git clone https://github.com/yourusername/context-footprint.git
cd context-footprint
cargo build --release
```

### Basic Usage

```bash
# 1. Generate SCIP index for your project
cd your-python-project
scip-python index . --output index.scip

# 2. Run context-footprint analysis
cd path/to/context-footprint
cargo run --release -- analyze \
  --scip your-python-project/index.scip \
  --symbol "my_module.MyClass.my_method"
```

**Output Example**:
```
Symbol: my_module.MyClass.my_method
Context Footprint: 2,847 tokens
Reachable Units: 23
  - Functions: 15
  - Types: 6
  - Variables: 2
Traversal stopped at 8 boundaries
```

## 📐 How It Works

### 1. Build Context Graph

Parse SCIP index into a directed graph where:
- **Nodes** = Functions, types, variables (with token counts)
- **Edges** = Dependencies (calls, type usage, reads/writes)

### 2. Conditional Traversal

Starting from a target node, traverse dependencies but:
- **✅ Stop** at external libraries
- **✅ Stop** at documented interfaces with complete type signatures
- **❌ Continue** through undocumented code
- **❌ Continue** through untyped parameters

### 3. Compute Footprint

Sum token counts of all reachable nodes.

**Visual Example**:
```
Target → [CallsA] → FunctionA (3rd-party) ✅ STOP
      → [CallsB] → FunctionB (no types) ❌ CONTINUE
                → [CallsC] → FunctionC (typed + docs) ✅ STOP
```

See [`docs/design.md`](docs/design.md) for formal algorithm definition.

## 🏗️ Architecture

**Hexagonal (Ports & Adapters)** pattern for testability:

```
src/
├─ domain/           # Core algorithm (no external deps)
│  ├─ graph.rs       # Context graph model
│  ├─ solver.rs      # BFS traversal with pruning
│  ├─ builder.rs     # Three-pass graph construction
│  └─ policy.rs      # Pruning decision trait
└─ adapters/         # External integrations
   ├─ scip/          # SCIP parser
   ├─ policy/        # Pruning implementations
   │  ├─ academic.rs # Fast heuristic (type + doc check)
   │  └─ strict.rs   # Aggressive pruning
   ├─ doc_scorer/    # Documentation quality scoring
   └─ size_function/ # Token counting (tiktoken)
```

**Design Rationale**: See [`AGENTS.md`](AGENTS.md) for architecture decisions and development guide.

## 🧪 Development

### Running Tests

```bash
# All tests (unit + integration)
cargo test --lib --tests

# Run linter and formatter
cargo fmt
cargo clippy -- -D warnings
```

### E2E Tests with Real Projects

```bash
# Setup FastAPI fixture (clone + generate SCIP)
./tests/fixtures/setup_fastapi.sh

# Run E2E tests
cargo test test_fastapi_project
```

**Testing Guide**: Comprehensive testing strategy documented in [`docs/testing.md`](docs/testing.md) (55 tests, 85%+ coverage).

## 📚 Documentation

- **[Design Document](docs/design.md)**: Formal algorithm definition and graph model
- **[Development Guide](AGENTS.md)**: Architecture decisions, coding conventions, extension points
- **[Testing Guide](docs/testing.md)**: Test organization and coverage goals

## 🤝 Contributing

Contributions welcome! This project follows Rust best practices:

1. **Format code**: `cargo fmt`
2. **Pass tests**: `cargo test --lib --tests`
3. **No warnings**: `cargo clippy -- -D warnings`
4. **Write tests**: Add unit tests for new features

## 📄 License

[Apache 2.0](LICENSE) — Free for academic and commercial use.

## 🔗 Related Work

- [SCIP Protocol](https://github.com/sourcegraph/scip) — Language-agnostic semantic indexing
- [Sourcegraph](https://sourcegraph.com/) — Code intelligence platform
- [Context Footprint Paper](docs/the-paper.md) — Theoretical foundation

---

**Status**: Early development | **Coverage**: 85%+ domain layer | **CI**: GitHub Actions
