# Python Protocol Support

## Overview

This document describes how the Context Footprint tool handles Python's `typing.Protocol` for abstract factory pattern recognition.

## Problem

Python's `typing.Protocol` is a structural typing mechanism similar to interfaces in other languages. However, SCIP-python (the Python indexer) has limitations:

1. **No return type relationships**: SCIP-python doesn't generate `TypeDefinition` relationships for function return types
2. **Protocols marked as Class**: SCIP-python marks `Protocol` classes with `kind=Class` instead of `kind=Protocol`

This prevented the tool from recognizing abstract factory patterns in Python code.

## Solution

### 1. Return Type Inference from Occurrences

**File**: `src/adapters/scip/adapter.rs`

The `enrich_python_return_types` function:
- Scans function definitions for type references within their range
- Identifies return types using Python syntax pattern: `-> TypeName:`
- Adds `TypeDefinition` relationships from functions to their return types

**Key insight**: SCIP-python marks type annotations as `ReferenceRole::Read`, not `TypeUsage`.

```python
def get_auth_port(...) -> AuthPort:  # Line 247
    ...
```

The tool:
1. Finds `AuthPort` reference at line 247 with role `Read`
2. Checks source line contains `->` and ends with `:`
3. Adds `ReturnType` edge: `get_auth_port` → `AuthPort`

### 2. Protocol Detection via Inheritance

**File**: `src/domain/builder.rs`

SCIP-python marks Protocols as `Class` but adds an `Implements` relationship:

```
Kind: Class
Relationships:
  - Implements -> scip-python python python-stdlib 3.11 typing/Protocol#
```

The `create_node_from_definition` function:
- Checks if a `Class` implements `typing.Protocol#`
- Sets `is_abstract = true` for such classes
- Changes `type_kind` to `Protocol`

## Verification

### Test Suite

**File**: `tests/python_abstract_factory_test.rs`

Four tests verify the complete flow:

1. **test_llmrelay_auth_port_is_abstract**
   - Verifies `AuthPort` is recognized as abstract Protocol
   - Checks `is_abstract = true` and `doc_score >= 0.5`

2. **test_llmrelay_get_auth_port_has_return_type_edge**
   - Verifies `get_auth_port` has `ReturnType` edge to `AuthPort`

3. **test_llmrelay_get_auth_port_is_abstract_factory**
   - Verifies `AcademicBaseline` policy identifies `get_auth_port` as `Boundary`

4. **test_llmrelay_caller_of_get_auth_port_cf_excludes_implementation**
   - Verifies callers don't traverse into concrete implementation (`JuhellmAuthAdapter`)

### Real-world Impact

**LLMRelay project** (Clean Architecture with Dependency Injection):

Before:
```
create_response CF: 24,094 tokens
- Includes: JuhellmAuthAdapter (full 85-line class definition)
```

After:
```
create_response CF: 13,752 tokens (43% reduction)
- Includes: get_auth_port (abstract factory function)
- Excludes: JuhellmAuthAdapter (concrete implementation hidden)
```

## Architecture

The solution follows the Hexagonal Architecture principle:

```
Domain Layer          Adapter Layer
-----------          --------------
builder.rs     ←--   scip/adapter.rs
  ↓                        ↓
  Checks                Enriches semantic data
  Protocol              Infers return types
  relationships         (Python-specific)
```

Language-specific logic is isolated in the SCIP adapter:
- `enrich_semantic_data`: Dispatches by file extension (`.py`)
- `enrich_python_return_types`: Python-specific enrichment

## Future Work

1. **TypeScript/JavaScript**: May need similar enrichment if SCIP indexers have gaps
2. **Other patterns**: Generic type parameter inference
3. **Performance**: Cache enrichment results for large codebases

## References

- [SCIP Protocol](https://github.com/sourcegraph/scip)
- [Python typing.Protocol](https://docs.python.org/3/library/typing.html#typing.Protocol)
- [Abstract Factory Pattern](https://refactoring.guru/design-patterns/abstract-factory)
