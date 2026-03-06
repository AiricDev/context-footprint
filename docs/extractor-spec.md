# Extractor Specification

> Formal contract for language-specific extractors. Any extractor that outputs SemanticData JSON
> conforming to this spec will produce correct CF results via the language-agnostic Builder + Solver.

## Overview

An extractor converts source code into `SemanticData` (defined in `src/domain/semantic.rs`).
The SemanticData schema defines the **shape** of the output. This document defines the
**behavioral completeness requirements** — i.e., exactly which source-code patterns MUST
produce which references, so that the CF graph captures all meaningful dependencies.

### Three Layers of This Spec

1. **Abstract Reference Patterns** (language-agnostic): enumerate every semantic situation
   where one symbol depends on another. This is the "what".
2. **Language Mapping** (per-language): map each abstract pattern to concrete AST forms
   and resolution strategies. This is the "how".
3. **Conformance Test Suite** (per-language): a fixture + assertion for each pattern.
   This is the "proof".

A new extractor is considered complete when every applicable abstract pattern has
a passing conformance test.

---

## Part 1: Abstract Reference Patterns

Each pattern describes a **semantic situation** where a function or variable depends on
another symbol. The pattern is language-agnostic; the AST forms differ per language.

### Call Patterns

| ID | Pattern | Role | Description |
|----|---------|------|-------------|
| REF-CALL-01 | Direct function call | Call | `foo()` — call a named function |
| REF-CALL-02 | Method call (self/this) | Call | `self.method()` / `this.method()` — call own method |
| REF-CALL-03 | Method call (typed receiver) | Call | `obj.method()` where obj has known type — resolved via type |
| REF-CALL-04 | Method call (untyped receiver) | Call | `obj.method()` where obj has no type — unresolved, builder may recover |
| REF-CALL-05 | Constructor call | Call | `MyClass()` / `new MyClass()` — target is the constructor function |
| REF-CALL-06 | Super/parent method call | Call | `super().method()` / `super.method()` — target is parent class method |
| REF-CALL-07 | Class method factory call (cls) | Call | `cls()` / `cls.method()` in classmethod — target via enclosing class |
| REF-CALL-08 | Decorator / annotation | Decorate | `@decorator` — decorator function applied to a definition |

### Read Patterns

| ID | Pattern | Role | Description |
|----|---------|------|-------------|
| REF-READ-01 | Read module/global variable | Read | `print(CONFIG)` — use a module-level or global variable |
| REF-READ-02 | Read instance/class field | Read | `self.field` / `obj.field` — read a field |
| REF-READ-03 | Default argument value | Read | `def f(x=SENTINEL)` — reference in default param expression |
| REF-READ-04 | Function/method as value | Read | `callback = handler` / `register(fn)` — function used as value, not called |
| REF-READ-05 | Exception type in catch | Read | `except MyError` / `catch (MyError e)` — type used in exception handler |

### Write Patterns

| ID | Pattern | Role | Description |
|----|---------|------|-------------|
| REF-WRITE-01 | Assign to module/global variable | Write | `CONFIG = val` — write to module-level variable |
| REF-WRITE-02 | Assign to instance/class field | Write | `self.field = val` / `obj.field = val` |
| REF-WRITE-03 | Compound assignment (augmented) | Read + Write | `x += 1` — both reads and writes the target |
| REF-WRITE-04 | Delete variable/field | Write | `del obj.field` — removal is a mutation |

### Notes

- **Local variables are excluded**: references to/from local variables do not produce edges.
  Only module-level variables, class fields, functions, and methods are graph nodes.
- **Type annotations** are captured as metadata (`param_type`, `return_types`, `var_type`)
  for signature completeness and type-driven edge recovery, but do NOT produce graph edges
  themselves. Type context (class docstrings, bases) is handled at the context assembly layer.
- **Unresolved references**: when a target cannot be statically resolved, emit the reference
  with `target_symbol=None` and fill `receiver` + `method_name` if available. The builder's
  Pass 3 may recover the edge via type propagation. If recovery also fails, the reference
  remains unresolved — the CF result should track this for confidence scoring.

---

## Part 2: Python Language Mapping

Maps each abstract pattern to Python AST forms and resolution strategies.

### Definitions (Pass 1)

| What | Python AST | Scope Rule |
|------|-----------|------------|
| Module-level function | `FunctionDef` / `AsyncFunctionDef` (no class enclosing) | symbol_id = `module.func_name` |
| Class method | `FunctionDef` inside `ClassDef` | symbol_id = `module.Class.method`, enclosing_symbol = class |
| Class definition | `ClassDef` | kind=Type, inherits from bases |
| Module-level variable | `Assign` / `AnnAssign` (no func enclosing) | kind=Variable, scope=Global |
| Class body field | `Assign` / `AnnAssign` in class (no func) | kind=Variable, scope=Field |
| Instance field (self.x) | `Assign` / `AnnAssign` with target `self.x` in method | kind=Variable, scope=Field |
| Nested function | `FunctionDef` inside function | NOT extracted; refs attributed to enclosing method |

### References (Pass 2)

| Pattern ID | Python AST Form | Resolution Strategy |
|------------|----------------|---------------------|
| REF-CALL-01 | `Call(func=Name('foo'))` | Jedi `goto` at func position |
| REF-CALL-02 | `Call(func=Attribute(value=Name('self'), attr='m'))` | Enclosing class prefix + method name; Jedi fallback |
| REF-CALL-03 | `Call(func=Attribute(value=Name('obj'), attr='m'))` where `obj` has type annotation | Jedi `goto`; fallback: lookup param type → class → method |
| REF-CALL-04 | `Call(func=Attribute(value=Name('obj'), attr='m'))` where `obj` is untyped | Emit with `target_symbol=None`, `receiver=obj_symbol`, `method_name='m'` |
| REF-CALL-05 | `Call(func=Name('MyClass'))` | Jedi resolves to class; Builder maps via init_map → `__init__` |
| REF-CALL-06 | `Call(func=Attribute(value=Call(func=Name('super')), attr='m'))` | AST pattern match; enclosing class → `inherits[0]` (short name) → resolve base: same-module prefix first; then **cross-module** fallback by type name when unique; then **alias fallback**: walk ClassDef bases in AST, use Jedi `goto(..., follow_imports=True)` to resolve import aliases to their actual class → `parent.m` |
| REF-CALL-07 | `Call(func=Name('cls'))` in `@classmethod` | Treat as constructor of enclosing class → `__init__` |
| REF-CALL-07 | `Call(func=Attribute(value=Name('cls'), attr='m'))` | Same fallback logic as `self` — enclosing class + method name |
| REF-CALL-08 | `decorator_list` entries on `FunctionDef` / `ClassDef` | Jedi `goto` on decorator expression |
| REF-READ-01 | `Name(ctx=Load)` resolving to Variable definition | Jedi `goto` → check kind=Variable → emit Read |
| REF-READ-02 | `Attribute(ctx=Load)` resolving to Variable/Field | Jedi `goto` → check kind=Variable → emit Read with receiver |
| REF-READ-03 | `FunctionDef.args.defaults[]`, `args.kw_defaults[]` | Walk default expressions; Jedi; same-file by name; **cross-module unique**: by name in `all_definitions` when only one match; **import-context disambiguation**: when multiple same-named definitions exist, walk module's `ImportFrom` nodes to narrow to the one actually imported in this file |
| REF-READ-04 | `Name(ctx=Load)` resolving to Function definition | Jedi `goto` → check kind=Function → emit Read |
| REF-READ-04 | `Attribute(ctx=Load)` resolving to Function definition | Same, with receiver |
| REF-READ-05 | `ExceptHandler.type` | Walk type expression, resolve names, emit Read |
| REF-WRITE-01 | `Name(ctx=Store)` resolving to Variable | Jedi `goto` → emit Write |
| REF-WRITE-02 | `Attribute(ctx=Store)` resolving to Variable/Field | Jedi `goto` → emit Write with receiver |
| REF-WRITE-03 | `AugAssign(target=Name/Attribute)` | Resolve target → emit **both** Read and Write |
| REF-WRITE-04 | `Delete(targets=[Name/Attribute])` | Resolve target → emit Write |

### Resolution Priority

For each reference, the extractor tries resolution in this order:

1. **Jedi `goto`** at the source position → match against `all_definitions` by file + line
2. **Jedi `full_name`** → match by qualified name (may be external symbol)
3. **AST-based fallback** (pattern-specific):
   - `self.method()` → enclosing class + method name
   - `super().method()` → enclosing class → first base: (a) same-module prefix, (b) cross-module by type name when unique, (c) alias resolution via Jedi `follow_imports=True` on ClassDef base node → method
   - `cls()` → enclosing class `__init__`
   - Default arg names → (a) same-file variable by name, (b) cross-file unique by name, (c) import-context: resolve via `ImportFrom` nodes to disambiguate when multiple same-named definitions exist
4. **Emit unresolved** with `receiver` + `method_name` for Builder Pass 3 recovery

---

## Part 3: Conformance Test Suite

Each abstract pattern has a corresponding test fixture and assertion. A language extractor
is considered complete when all applicable tests pass.

### Test Structure

Tests live in `extractors/<language>/tests/fixtures/` with one fixture file per pattern
(or per related group). Each test:

1. Creates a fixture file exercising the pattern
2. Runs the extractor on the fixtures directory
3. Asserts the expected reference exists: `(enclosing_symbol, role, target_symbol)`

### Python Conformance Tests

| Pattern ID | Fixture File | Assertion |
|------------|-------------|-----------|
| REF-CALL-01 | `simple.py` | Call from `foo` to `bar` |
| REF-CALL-02 | `test_self_call.py` | Call from `APIRouter.put` to `APIRouter.api_route` |
| REF-CALL-03 | `test_method_resolve.py` | Call from `create_image_edit` to `RelayImageUseCase.execute` |
| REF-CALL-05 | (covered by Builder test) | Constructor `MyClass()` → `__init__` via init_map |
| REF-CALL-06 | `test_super_call.py` | Call from `Child.__init__` to `Base.__init__` (same module) |
| REF-CALL-06 | `base_super.py`, `child_super.py` | Call from `child_super.Child.__init__` to `base_super.Base.__init__` (cross-module, same name) |
| REF-CALL-06 | `base_alias.py`, `child_alias.py` | Call from `child_alias.Child.__init__` to `base_alias.Base.__init__` (base imported under alias `AliasBase`) |
| REF-CALL-07 | `test_cls_call.py` | Call from `MyClass.create` to `MyClass.__init__` |
| REF-CALL-08 | `test_annotated_doc.py` | Decorator extraction (use_signature_only_for_size) |
| REF-READ-01 | `test_default_arg_ref.py` | Read from `foo` to `SENTINEL` (module variable) |
| REF-READ-03 | `test_default_arg_ref.py` | Read from `foo` to `SENTINEL` (same-module default arg) |
| REF-READ-03 | `sentinel_def.py`, `default_arg_cross.py` | Read from `default_arg_cross.foo` to `sentinel_def.SENTINEL` (cross-module default arg, single definition) |
| REF-READ-03 | `sentinel_a.py`, `sentinel_b.py`, `default_arg_ambig.py` | Read from `default_arg_ambig.bar` to `sentinel_a._sentinel` (multiple same-named defs, import context selects the right one) |
| REF-READ-04 | `test_func_as_value.py` | Read from `setup` to `handler` (function as value) |
| REF-READ-05 | `test_except_resolve.py` | Read from `create_image_edit` to `QuotaError` |
| REF-WRITE-03 | `test_augassign.py` | Read + Write from `increment` to `counter` |

### Adding a New Language

When implementing an extractor for a new language:

1. Create `extractors/<language>/` with the extractor implementation
2. Copy this conformance table, replacing Python fixtures with equivalent code in the target language
3. For each row, create the fixture file and test
4. Implement extractor logic until all tests pass
5. Add the Language Mapping table to this document (Part 2 equivalent for the new language)

Patterns that don't apply to a language (e.g., REF-CALL-07/cls for Java) should be
marked N/A in the mapping table.

---

## Appendix: Patterns Explicitly Out of Scope

These patterns involve fundamentally dynamic behavior that no static extractor can
reliably resolve. They are documented here so extractor authors know NOT to spend
effort on them — the CF algorithm handles the impact via its confidence/uncertainty
reporting.

| Pattern | Example | Why Out of Scope |
|---------|---------|------------------|
| Dynamic attribute access | `getattr(obj, name)` | Attribute name is a runtime value |
| Metaclass-generated methods | `class Meta(type): ...` | Methods created at class-creation time |
| Monkey-patching | `obj.method = lambda: ...` | Replaces method at runtime |
| `exec` / `eval` generated symbols | `exec("def foo(): ...")` | Code generated from strings |
| Star imports (`from mod import *`) | Namespace pollution | Requires full module resolution |
| Descriptor protocol | `__get__`, `__set__` | Implicit method dispatch |
| `__getattr__` / `__getattribute__` | Fallback attribute resolution | Intercepts all attribute access |

When an extractor encounters these patterns, it should emit the reference as
**unresolved** (`target_symbol=None`) with whatever partial information is available
(`method_name`, etc.), so the CF result can report reduced confidence.
