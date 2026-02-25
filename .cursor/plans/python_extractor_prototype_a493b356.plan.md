---
name: Python Extractor Prototype
overview: Build a Python-based Semantic Data Extractor prototype using `ast` and `jedi` to extract codebase structure, references, and types into the standardized `SemanticData` JSON schema.
todos:
  - id: setup-project
    content: Setup Python project in extractors/python with pyproject.toml
    status: completed
  - id: define-schema
    content: Define Pydantic schema mirroring src/domain/semantic.rs
    status: completed
  - id: implement-ast-visitor
    content: Implement AST NodeVisitor for extracting Definitions
    status: completed
  - id: integrate-jedi
    content: Integrate Jedi for SymbolReference resolution and Types
    status: completed
  - id: build-cli
    content: Build CLI entrypoint (main.py) to output SemanticData JSON
    status: completed
  - id: validate-prototype
    content: Create test fixtures and validate extractor JSON output
    status: in_progress
isProject: false
---

# Python Extractor Prototype (Jedi + AST)

We will build a proof-of-concept Python extractor within the monorepo at `extractors/python`. This prototype will combine the speed of Python's built-in `ast` module for structural parsing with the power of `jedi` for symbol resolution (find references/definitions) and type inference. 

The output must exactly mirror the `SemanticData` JSON schema defined in `src/domain/semantic.rs`.

Here is the proposed implementation plan:

- **1. Project Setup (`extractors/python`)**
  - Create the `extractors/python` directory.
  - Set up a standard `pyproject.toml` or `requirements.txt`.
  - Add dependencies: `jedi` (for static analysis) and `pydantic` (for robust JSON schema modeling and serialization).
- **2. Schema Definition (`extractors/python/schema.py`)**
  - Create Python Pydantic models mapping directly to the Rust structs in `src/domain/semantic.rs`.
  - Define `SemanticData`, `DocumentSemantics`, `SymbolDefinition`, `SymbolReference`, `FunctionDetails`, `VariableDetails`, `TypeDetails`, etc.
  - Ensure Enums (`SymbolKind`, `Visibility`, `ReferenceRole`, `Mutability`) correctly serialize to their string equivalents.
- **3. AST Traversal Engine (`extractors/python/extractor.py`)**
  - Implement a two-pass architecture.
  - **Pass 1 (AST)**: Use `ast.NodeVisitor` to traverse the code and find all explicit definitions (`ClassDef`, `FunctionDef`, `AsyncFunctionDef`, and top-level/class-level `Assign` or `AnnAssign`).
  - Compute accurate 0-indexed line and column spans for `SourceSpan`.
  - Collect docstrings (`ast.get_docstring`) and generate globally unique `SymbolId`s (e.g., `pkg.module.Class.method`).
- **4. Jedi Integration for Resolution (`extractors/python/jedi_resolver.py`)**
  - **Pass 2 (Jedi)**: For each `Call` (function call) or `Attribute`/`Name` read/write found in the AST:
    - Invoke `jedi.Script(path).goto(line, column)` to resolve the target to its original definition file and scope.
    - Convert Jedi's definition result into a canonical `target_symbol` (or provide `receiver` and `method_name` if `target_symbol` is unresolvable).
  - Use Jedi's `infer(line, column)` on function parameters and return statements to extract `param_type` and `return_types` on a best-effort basis.
- **5. CLI Interface (`extractors/python/main.py`)**
  - Build a simple command-line interface that accepts a project directory path.
  - Iterate through all `.py` files in the directory.
  - Construct the global `SemanticData` object and print it as a formatted JSON string to `stdout`.
- **6. Prototype Validation (`extractors/python/tests/`)**
  - Create a small `fixtures/` directory containing a few interconnected Python files with classes, global variables, and cross-file method calls.
  - Run the extractor CLI against these fixtures and manually verify that the output JSON captures all the `Definitions` and `References` required by the `GraphBuilder`.

