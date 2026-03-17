# Context Footprint Requirements for Variable/Constant and Class Anchors

## Background

In the current `cf-research` workflow, the LLM-based entry-function extraction sometimes returns:

- callable symbols: functions and methods
- non-callable but still meaningful anchors: module-level constants, regex objects, rule objects, class attributes
- class symbols that represent plugin-like or interface-driven entry surfaces

For downstream Context Footprint (CF) computation, these cases should not be discarded as invalid. They represent real maintenance entry points:

- a bug fix may change the value or initialization logic of a module variable or constant
- a framework/plugin class may be the best semantic entry, even if runtime dispatch eventually happens through one or more methods

The CF project should support these anchors directly, instead of requiring the research pipeline to approximate them as unrelated functions.


## Goals

1. Allow CF computation starting from variable/constant-like anchors.
2. Define deterministic expansion behavior for class anchors.
3. Preserve current function/method behavior.
4. Keep the semantics explicit enough that `cf-research` can rely on them without repo-specific hacks.


## Non-Goals

- This does not require whole-program dynamic tracing.
- This does not require perfect runtime entrypoint reconstruction.
- This does not require inferring private helper importance separately; normal graph reachability can continue to bring private helpers into the footprint when referenced.


## Desired Anchor Types

The CF engine should accept at least these anchor kinds:

- `function`
- `method`
- `class`
- `module_variable`
- `class_variable`
- `constant`
- `module_object`

Notes:

- `constant` is semantic sugar for a read-mostly variable-like anchor.
- `module_object` covers objects such as compiled regexes, rule tables, registries, and descriptor-like objects created at module import time.
- The concrete internal taxonomy may differ, but the user-facing behavior must distinguish callable anchors from non-callable anchors.


## Important Clarification: Anchor Kind vs Graph Node Kind

`class anchor` does **not** mean the CF graph must contain class nodes.

If the current CF implementation only models callable/code-level nodes in the graph, that is fine.

In this document, `class anchor` means:

- the user or upstream tool provides a class symbol as the input anchor
- the resolver recognizes that the input refers to a class
- before normal CF traversal, the resolver expands that class into a deterministic set of callable members
- CF is then computed over those expanded callable nodes using the existing graph machinery

So `class anchor` is an **input-layer concept**, not necessarily a **graph-layer concept**.

Equivalent interpretation:

- `function` / `method` anchor: directly map to existing graph nodes
- `class` anchor: syntactic sugar for a predefined set of function/method anchors
- `variable-like` anchor: may require additional non-callable dependency modeling

This means class-anchor support can be implemented without introducing explicit class nodes into the graph.


## Required Semantics

### 1. Function and method anchors

No semantic change from current behavior.


### 2. Class anchors

When a class is passed as an anchor, CF should treat the class as a shorthand for a set of callable entry members.

Operationally:

- resolve the class symbol
- enumerate the member methods that match the expansion rule
- convert the class anchor into a set of function/method anchors
- run the normal CF algorithm on that expanded set

Required expansion rule:

- Include all public methods defined on the class.
- Include all override-style dunder methods defined on the class.
- Exclude private helpers by default.

Definitions:

- Public method: method name does not start with `_`.
- Override-style dunder method: name matches `__name____`.
- Private helper: name starts with `_` but is not a dunder method.

Examples:

- Include `run`, `dispatch`, `as_sql`, `clean`, `get_context_data`
- Include `__init__`, `__call__`, `__iter__`, `__getitem__`, `__enter__`, `__exit__`, `__str__`
- Exclude `_validate`, `_setup`, `_compile_cache`

Rationale:

- In framework and plugin code, the meaningful entry surface is often the class contract rather than a single statically identifiable function.
- Private helpers usually should not be treated as direct entry anchors, but they should still appear in CF transitively when referenced by included methods.

Optional extension:

- Add a mode to include selected private methods if a caller explicitly requests full-class expansion.


### 3. Variable / constant / module-object anchors

CF for a variable-like anchor should represent the impact surface of the symbol's definition and uses.

Minimum required behavior:

- Resolve the defining site of the symbol.
- Include code that initializes or mutates the symbol.
- Include functions, methods, class bodies, or module-level code that read the symbol.
- Include re-export/import sites when they are part of the symbol access path.

Examples:

- `django.contrib.messages.storage.base.LEVEL_TAGS`
  CF should include the assignment site, update logic, and code paths that consume the mapping.
- `django.utils.translation.trans_real.language_code_prefix_re`
  CF should include regex initialization and functions that use it for parsing.
- `sympy.core.assumptions._assume_rules`
  CF should include rule construction and callers that query those rules.

This is intentionally different from callable-anchor CF. The anchor is not "what executes first", but "what symbol change propagates through which code."


## Search and Resolution Requirements

The project should expose symbol lookup for non-callable anchors as first-class search results.

Required behavior:

- Search should return variable-like anchors in addition to callables.
- Search output should include symbol kind.
- Search should distinguish between:
  - module-level variable/object
  - class attribute
  - function/method
  - class

Example desirable search output:

```text
django.contrib.messages.storage.base.LEVEL_TAGS [module_variable]
django.utils.translation.trans_real.language_code_prefix_re [module_object]
sympy.core.assumptions._assume_rules [module_object]
django.core.paginator.Paginator.__iter__ [method]
django.core.paginator.Paginator [class]
```


## Compute API Requirements

### CLI / API inputs

`compute` should accept a mixed list of anchors:

- functions/methods
- classes
- variable-like symbols

If the engine already has a single `compute(symbols)` interface, it can keep that shape, but it must accept these broader symbol kinds.


### Explain / debug output

For each input anchor, the engine should expose how it was interpreted.

Required explainability fields:

- input symbol
- resolved kind
- expansion result
- unresolved reason if resolution fails

Example:

```json
{
  "input": "django.core.paginator.Paginator",
  "resolved_kind": "class",
  "expanded_to": [
    "django.core.paginator.Paginator.__init__",
    "django.core.paginator.Paginator.__iter__",
    "django.core.paginator.Paginator.page",
    "django.core.paginator.Paginator.validate_number"
  ]
}
```

For a variable anchor:

```json
{
  "input": "django.contrib.messages.storage.base.LEVEL_TAGS",
  "resolved_kind": "module_variable",
  "expanded_to": [
    "definition:django.contrib.messages.storage.base.LEVEL_TAGS",
    "use:django.contrib.messages.storage.base.Message.level_tag",
    "use:django.contrib.messages.apps.update_level_tags"
  ]
}
```

The exact format is flexible, but the engine must provide enough detail that downstream tooling can audit what happened.


## Graph Construction Expectations

The exact internal representation is up to the CF project, but the following cases must be modeled:

- assignment to module variable
- reassignment / mutation of module variable
- references from functions and methods to module variables
- references from class methods to class attributes
- import and re-export references when they preserve symbol identity
- class anchor expansion to member methods before CF traversal

At minimum, the graph should connect:

- variable definition -> readers
- variable update site -> readers

For class anchors, this expansion may happen entirely before graph traversal. A dedicated class node is optional and not required.


## Output Expectations

The CF result should remain a single scalar metric if that is the current project contract.

In addition, the engine should expose optional metadata:

- resolved anchor kinds
- class expansion results
- variable-use hits

This metadata is important for downstream evaluation and debugging, even if the main metric remains unchanged.


## Backward Compatibility

- Existing function/method-based CF calls must continue to work unchanged.
- New symbol kinds should be additive.
- If a class or variable symbol cannot be resolved, the tool should fail gracefully with an explicit unresolved report rather than silently dropping it.


## Acceptance Criteria

### Class anchors

1. Inputting a class symbol produces the same CF as computing over the union of:
   - all public methods on the class
   - all dunder override methods on the class
2. Private non-dunder methods are not included in the initial expansion set.
3. Expanded methods are surfaced in explain/debug output.


### Variable / constant anchors

1. The tool can resolve module-level constants and objects as anchors.
2. The tool can compute a non-empty CF when those symbols have downstream readers.
3. The tool can surface which functions/methods/classes read or update the anchor.
4. Search can return these symbols and label them with a non-callable kind.


### Mixed-anchor compute

1. A single compute call can include a mix of methods, classes, and variable-like anchors.
2. The returned CF is deterministic.
3. The explain/debug output makes anchor expansion auditable.


## Suggested Test Cases

### Class anchor tests

- A class with:
  - public methods
  - private helper methods
  - dunder override methods
- Verify only public + dunder are expanded.

Example pattern:

```python
class Plugin:
    def run(self): ...
    def render(self): ...
    def __call__(self): ...
    def _helper(self): ...
```

Expected expansion:

- `Plugin.run`
- `Plugin.render`
- `Plugin.__call__`

Not included initially:

- `Plugin._helper`


### Variable-anchor tests

- Module-level compiled regex used by one parser function.
- Module-level mapping updated by one setup hook and consumed by one property/method.
- Class attribute read by several methods.
- Re-exported constant imported into another module and used there.


## Suggested Rollout Plan

1. Add symbol-kind support to search/indexing.
2. Add class-anchor expansion.
3. Add variable-like anchor resolution and dependency edges.
4. Add explain/debug metadata.
5. Validate on known examples from `cf-research`.


## Example Instances from cf-research

These concrete cases motivated the requirement:

- Class anchors:
  - `django.core.paginator.Paginator`
  - `django.utils.datastructures.OrderedSet`
  - `django.contrib.auth.validators.ASCIIUsernameValidator`
  - `django.contrib.auth.validators.UnicodeUsernameValidator`

- Variable / constant / module-object anchors:
  - `django.contrib.messages.storage.base.LEVEL_TAGS`
  - `django.utils.translation.trans_real.language_code_prefix_re`
  - `sympy.core.assumptions._assume_rules`


## Summary

The CF project should support three interpretations of anchors:

- callable anchor: current behavior
- class anchor: expand to public + dunder methods
- variable-like anchor: compute the impact surface of the symbol's definition, updates, and uses

This will let `cf-research` preserve semantically correct LLM-selected anchors instead of forcing everything into a function-only approximation.
