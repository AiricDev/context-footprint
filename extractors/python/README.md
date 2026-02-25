# Context-Footprint Python Extractor

Prototype extractor that outputs [SemanticData](https://github.com/context-footprint/context-footprint) JSON using **Python AST** + **Jedi**.

## Setup (uv)

From this directory:

```bash
uv sync
```

This creates a venv and installs the package in editable mode with dependencies (jedi, pydantic). Dev deps (pytest) are included.

## Usage

```bash
uv run cf-extract /path/to/python/project [--pretty]
# or
uv run python -m cf_extractor.main /path/to/project [--pretty]
```

Output is written to stdout (valid JSON consumable by the Rust GraphBuilder).

## Tests

```bash
uv run pytest tests/ -v
```

## Requirements

- Python >= 3.9
- [uv](https://docs.astral.sh/uv/) for dependency and run management
- jedi, pydantic (installed via `uv sync`)
