# Context Footprint Extractors

Language-agnostic CLI utilities for extracting `SemanticData` via Language Server Protocol.

## Quick Start

```bash
npm install
npm run build
./bin/extract-semantics python /path/to/project --output semantics.json
```

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
