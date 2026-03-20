# cf-extractor

Python semantic data extractor for [Context-Footprint](https://github.com/context-footprint/context-footprint). Outputs SemanticData JSON using **Python AST** plus a pluggable resolver backend. `jedi` is the default backend; `ty` is available as an experimental LSP-backed resolver.

## As a dependency

This package is a dependency of `cftool`. When you install cftool via uv or pip, cf-extractor is installed automatically:

```bash
uv tool install cftool   # includes cf-extractor
```

## Standalone usage

You can also install and use cf-extractor directly:

```bash
# From PyPI (when published)
pip install cf-extractor

# From Git
uv pip install "cf-extractor @ git+https://github.com/context-footprint/context-footprint#subdirectory=extractors/python"

# Development
cd extractors/python
uv sync
```

Run the extractor:

```bash
cf-extract /path/to/python/project
# or
uv run cf-extract /path/to/python/project
# or
python -m cf_extractor.main /path/to/project
```

Without arguments, uses the current directory (`.`). Output is written to stdout (valid JSON for cftool).

Resolver backend options:

```bash
cf-extract /path/to/python/project --resolver-backend jedi
cf-extract /path/to/python/project --resolver-backend ty --ty-path /path/to/ty
```

Optional metrics output for benchmarking:

```bash
cf-extract /path/to/python/project --metrics-out metrics.json
cf-extract-benchmark \
  --dataset small=tests/fixtures \
  --dataset medium=/path/to/project \
  --dataset large=/path/to/project \
  --report-out benchmark.md
```

## Tests

```bash
uv run pytest tests/ -v
```

## Requirements

- Python >= 3.9
- jedi, pydantic (installed automatically)
- `ty` is optional and only required for `--resolver-backend ty`
