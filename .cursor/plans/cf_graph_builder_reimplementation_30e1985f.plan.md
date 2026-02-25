---
name: CF Graph Builder Reimplementation
overview: Reimplement the graph-builder module to match the new architecture design. Update the semantic data schema, enhance the multi-pass graph building process with type propagation and edge recovery, and ensure accurate edge creation without deprecated edge types.
todos:
  - id: update-semantic-schema
    content: Update SymbolReference in src/domain/semantic.rs
    status: completed
  - id: update-test-fixtures
    content: Update test fixtures in tests/common/fixtures.rs
    status: completed
  - id: pass-2-collection
    content: Implement Pass 2 unresolved_calls and call_assignments collection in src/domain/builder.rs
    status: completed
  - id: pass-2-5-propagation
    content: Implement Pass 2.5 External Call Return Type Propagation
    status: completed
  - id: pass-3-edge-recovery
    content: Implement Pass 3 Type-Driven Call Edge Recovery to fixpoint
    status: completed
isProject: false
---

# CF Graph Builder Reimplementation

Based on `docs/design.md`, the `graph-builder` module (Part 2) needs to be updated to match the new architecture. The Graph Builder translates `SemanticData` into the `ContextGraph`.

Here are the key changes we will make:

- **Update Semantic Data Schema (`src/domain/semantic.rs`)**
  - Update `SymbolReference` to align with the new data contract:
    - Change `target_symbol` to `Option<SymbolId>`.
    - Change `receiver` to `Option<SymbolId>` (so we can lookup the receiver variable in the graph).
    - Add `method_name: Option<String>`.
    - Add `assigned_to: Option<SymbolId>`.
  - Fix all fixture constructors in `tests/common/fixtures.rs` and other test files to use the new schema structure.
- **Enhance Pass 2: Edge Wiring (`src/domain/builder.rs`)**
  - Handle optional `target_symbol` and collect `unresolved_calls` for `Call` references where the target cannot be resolved immediately.
  - Collect `call_assignments` to track which variable receives the result of a function call (`assigned_to`), allowing us to propagate types in Pass 2.5.
- **Add Pass 2.5: Type Propagation (`src/domain/builder.rs`)**
  - Keep the existing `fill_type_references` logic to populate explicit types.
  - Add **External Call Return Type Propagation**: Iterate over `call_assignments`. If the target function has a known return type and the assigned variable does not yet have a type, propagate the return type to the variable. This is crucial for enabling type-driven edge recovery later.
- **Add Pass 3 Logic: Edge Recovery (`src/domain/builder.rs`)**
  - Implement **Type-Driven Call Edge Recovery**: Add a fixpoint loop over `unresolved_calls`.
  - For each unresolved call with a `receiver`, lookup the receiver variable in the graph to get its `var_type`.
  - Use the type to find the target method (via `method_by_scope` lookup) matching `method_name`.
  - If a match is found, add the missing `Call` edge and remove from the unresolved list. Loop until no new edges are recovered.
- **Cleanup and Verification**
  - Verify that the builder does not emit deprecated edges like `SharedStateWrite` or `CallIn` (already avoided, but we ensure adherence).
  - Ensure all integration tests in `tests/graph_builder_test.rs` pass with the new schema and logic.

