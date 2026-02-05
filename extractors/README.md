# Context Footprint Extractors

Language-agnostic CLI utilities for extracting `SemanticData` via Language Server Protocol.

## Requirements

- [Bun](https://bun.sh/) >= 1.3

## Quick Start

```bash
# Install dependencies
bun install

# Run extraction
bun run src/cli.ts python /path/to/project --output semantics.json
```

## Testing

```bash
# Run integration tests
bun test

# Run with watch mode for development
bun test --watch
```

Integration tests use the Python code in `tests/simple_python_for_extract/` as test fixtures.

**Note:** Tests use result caching to avoid re-running LSP extraction (which takes ~15-20s) for each test.

## Commands

```
extract-semantics <language> <project-root> [options]
```

### Options
- `--output <file>`: Write JSON to file (defaults to stdout)
- `--config <file>`: Language-specific config (e.g., pyrightconfig.json)
- `--exclude <pattern>`: Glob patterns to ignore
- `--verbose`: Enable debug logs

## Languages
- `python` (Pyright)
- _Coming soon_: `typescript`, `rust`

## Architecture

```
src/
├── cli.ts                    # CLI entry point
├── index.ts                  # Public API exports
├── core/
│   ├── types.ts              # SemanticData schema definitions
│   ├── extractor-base.ts     # Base class for language extractors
│   ├── lsp-client.ts         # LSP client implementation
│   └── utils.ts              # Utility functions
└── languages/
    └── python/
        ├── extractor.ts      # Python-specific extractor
        └── config.ts         # Python LSP configuration
```

## SemanticData Schema

The output follows the schema defined in `src/core/types.ts`:

- **project_root**: Absolute path to the project
- **documents**: Array of source files with definitions and references
- **external_symbols**: Symbols from dependencies (stdlib, third-party)

Each symbol has:
- **symbol_id**: Globally unique identifier (e.g., `module.Class.method#Function`)
- **kind**: `Function`, `Variable`, or `Type`
- **enclosing_symbol**: Parent symbol ID (null for module-level)
- **location**: File path and 0-based line/column position
- **span**: Full extent of the definition
