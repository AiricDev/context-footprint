# Context Footprint

> A static analysis tool for measuring architectural context exposure in codebases.

Context Footprint is a research prototype that computes a **Context Footprint (CF)** metric for functions and types in a codebase.  
CF approximates the amount of code context that must be traversed to analyze a given symbol, based on language-level dependencies and abstraction boundaries.

This tool is developed alongside an ongoing academic study and is intended for **measurement, comparison, and empirical analysis**, rather than production use.

---

## Status

⚠️ **Research Preview**

- The CF definition and traversal rules are still evolving.
- APIs, outputs, and heuristics may change before publication.
- The repository currently serves reproducibility and early feedback purposes.

---

## What the Tool Does

Given a semantic index of a codebase, the tool can:

- Compute the **distribution of CF values** across all functions or types
- Identify symbols with **unusually large context exposure**
- Query the CF of a specific symbol
- Print the source code that contributes to a symbol’s CF

CF is computed via **conservative graph traversal** over language-level dependencies, with configurable pruning rules.

---

## Supported Languages

The tool consumes **semantic data** (JSON) produced by language-specific extractors (e.g. LSP-based), and is therefore language-agnostic in principle.

Tested languages include:

- Python
- TypeScript

Support for additional languages depends on the availability of semantic data extractors that output the `SemanticData` JSON format.

---

## Installation

### Option 1: uv / pip (recommended)

Install as a Python tool—includes the `cf-extract` command for Python project extraction:

```bash
uv tool install cftool
# or: pip install cftool
```

Requires Python 3.9+.

### Option 2: Cargo

Build from source:

```bash
git clone https://github.com/yourusername/context-footprint.git
cd context-footprint
cargo build --release
```

Requires Rust 1.70+.

### Prerequisites

- A semantic data JSON file for the target project (e.g. from `cf-extract` for Python)

---

## Basic Usage

### 1. Generate semantic data

For Python projects, use the bundled extractor:

```bash
cf-extract /path/to/python/project > semantic_data.json
```

Or use another extractor (e.g. LSP-based) that outputs the `SemanticData` JSON format.

### 2. Analyze CF distribution

```bash
cftool semantic_data.json stats
# or with cargo build: ./target/release/cftool semantic_data.json stats
```

### 3. Find symbols with highest CF

```bash
cftool semantic_data.json top --limit 10
```

### 4. Query a specific symbol

```bash
cftool semantic_data.json compute "<symbol-id>"
```

### 5. Inspect contributing context

```bash
cftool semantic_data.json context "<symbol-id>"
```

---

## Output

The tool reports CF values as **token counts**, using a configurable size function.
Output includes percentile distributions and summary statistics for large codebases.

Example:

```
Functions - Context Footprint Distribution:
  Count: 856
  Median: 245 tokens
  90th percentile: 20,567 tokens
```

---

## How CF Is Computed (Brief)

1. A directed dependency graph is constructed from the semantic data (JSON).
2. Starting from a target symbol, dependencies are traversed conservatively.
3. Traversal stops at:

   * External libraries
   * Explicit abstraction boundaries defined by the pruning policy
4. The size of the reachable subgraph is summed.

The default pruning policy is intentionally conservative and favors soundness over precision.

For a formal definition, see `docs/design.md`.

---

## Project Structure

The implementation separates core analysis logic from language-specific adapters:

```
src/
├─ domain/        # Graph model and traversal logic
└─ adapters/      # Size functions, doc scoring, test detection
```

---

## License

Apache 2.0

---

## Acknowledgements

Semantic data is consumed as JSON (e.g. from LSP-based extractors).
