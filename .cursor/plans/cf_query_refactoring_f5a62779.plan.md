---
name: CF Query Refactoring
overview: Refactor the cf-query module (Part 1) to align with the new Context Footprint design, focusing on removing reverse edges from the graph schema and implementing them as reverse explorations during graph traversal.
todos:
  - id: update-edge-kind
    content: Update EdgeKind enum in src/domain/edge.rs to match the new schema
    status: completed
  - id: update-graph-api
    content: Add incoming_edges and outgoing_edges methods to ContextGraph in src/domain/graph.rs
    status: completed
  - id: update-policy
    content: Refactor policy logic in src/domain/policy.rs (evaluate_forward and should_explore_callers)
    status: completed
  - id: update-solver
    content: Rewrite BFS traversal logic in src/domain/solver.rs to support dynamic reverse explorations
    status: completed
isProject: false
---

# Context Footprint (CF) Query Refactoring

Based on `docs/design.md` and `docs/the-paper.md`, I will refactor the `cf-query` module (Part 1 of the implementation). The most significant change is removing `SharedStateWrite` and `CallIn` edges from the persistent graph schema and replacing them with dynamic reverse explorations (incoming edges) during BFS traversal.

Here are the key changes we will make:

- **Update Graph Schema (`src/domain/edge.rs`)**
  - Restrict `EdgeKind` to the 5 core forward dependencies: `Call`, `Read`, `Write`, `OverriddenBy`, `Annotates`.
  - Remove `SharedStateWrite`, `CallIn`, and rename `ImplementedBy` to `OverriddenBy`.
- **Enhance Graph API (`src/domain/graph.rs`)**
  - Add an `incoming_edges(NodeIndex)` method to support reverse exploration.
  - Rename `neighbors` to `outgoing_edges` to make the traversal code explicit.
- **Refactor Pruning Policy (`src/domain/policy.rs`)**
  - Refactor `evaluate` to `evaluate_forward` as it now only applies to forward outgoing edges.
  - Remove `SharedStateWrite` and `CallIn` edge handling from `evaluate_forward`.
  - Introduce `should_explore_callers(func_node, incoming_edge, params, type_registry)` to determine if a function needs call-in exploration.
- **Rewrite BFS Traversal (`src/domain/solver.rs`)**
  - Define a new `TraversalPath` or `ReachedVia` enum to distinguish between forward and reverse explorations during BFS.
  - Update `compute_cf` and `compute_cf_total`:
    1. Perform **forward traversal** on `outgoing_edges` and evaluate with `evaluate_forward`.
    2. Perform **call-in exploration** on `incoming_edges(Call)` if `should_explore_callers` is true.
    3. Perform **shared-state write exploration** on `incoming_edges(Write)` if the current node is a `Mutable` variable reached via a `Read` edge.
- **Update Node & Type Registry Models (`src/domain/node.rs`, `src/domain/type_registry.rs`)**
  - Verify property naming aligns with `Graph Schema` (e.g., ensuring `is_interface_method` and `mutability` are correct).
  - Minor cleanups in `Node` struct if needed.

