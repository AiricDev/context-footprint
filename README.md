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

The tool consumes code indexed using the **SCIP protocol**, and is therefore language-agnostic in principle.

Tested languages include:

- Python
- TypeScript

Support for additional languages depends on the availability and quality of SCIP indexers.

---

## Installation

### Prerequisites

- Rust 1.70+
- A SCIP index for the target project

### Build

```bash
git clone https://github.com/yourusername/context-footprint.git
cd context-footprint
cargo build --release
````

---

## Basic Usage

### 1. Generate a SCIP index

Example for Python:

```bash
scip-python index . --output index.scip
```

### 2. Analyze CF distribution

```bash
./target/release/context-footprint index.scip stats
```

### 3. Find symbols with highest CF

```bash
./target/release/context-footprint index.scip top --limit 10
```

### 4. Query a specific symbol

```bash
./target/release/context-footprint index.scip compute \
  "<symbol-id>"
```

### 5. Inspect contributing context

```bash
./target/release/context-footprint index.scip context \
  "<symbol-id>"
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

1. A directed dependency graph is constructed from the SCIP index.
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
└─ adapters/      # SCIP parsing, size functions, pruning policies
```

---

## License

Apache 2.0

---

## Acknowledgements

This tool builds on the SCIP protocol developed by Sourcegraph.
