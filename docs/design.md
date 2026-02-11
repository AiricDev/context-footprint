## **1. Formal Definition: The Context Graph**

To provide an objective, mathematically rigorous definition, we model the software system as a directed graph.

### **1.1 The Graph Model**

Let a codebase be represented as a directed graph $G = (V, E)$, where:

- $V$ **(Nodes)**: The set of relevant code units that are **functions** and **variables** only. Type definitions are not graph nodes; they are stored in a separate **Type Registry** and referenced by type IDs from nodes (e.g. function return types, parameter types, variable types).
- $E$ **(Edges)**: The set of dependency relationships, where $(u, v) \in E$ implies $u$ depends on $v$. Edge types in the implementation are:
    - **Control flow**: `Call` (function → function)
    - **Data flow**: `Read`, `Write` (function → variable)
    - **Dynamic expansion (reverse deps)**: `SharedStateWrite` (reader → writer of shared mutable state), `CallIn` (callee → caller for underspecified functions)
    - **Annotations**: `Annotates { is_behavioral }` (decorated → decorator)

Type usage (param type, return type, field type, variable type) and type hierarchy (inherits, implements) are represented via node attributes and the Type Registry, not as graph edges.

Each node $u \in V$ has intrinsic physical and semantic properties:

- $S(u)$: The **context size** of unit $u$. This is an abstract measure of information volume, typically implemented as a token count using a standard tokenizer (e.g., `cl100k_base`) or a word-based approximation.
- $D(u)$: The **documentation score** of unit $u$, a value in range $[0.0, 1.0]$ representing documentation quality.
- $E(u)$: A boolean flag indicating if the node is **external** (3rd-party library).

### **1.2 Context as a Subgraph**

We define the **Context** of a unit $u$, denoted as $R(u)$, as a **specific subset of nodes** $R(u) \subseteq V$ that must be loaded to satisfy Information Completeness. This set is computed via conditional traversal from $u$ (see Section 2).

The **Context Footprint (CF)** is the total volume of all reached nodes:

$$CF(u) = \sum_{v \in R(u)} size(v)$$

where $size(v)$ is the context size of unit $v$. By definition, $R(u)$ includes $u$ itself, and significantly, it **includes the boundary nodes** themselves (representing the interface being consumed) but not their implementation details.

This definition removes subjective "cognitive weights." The metric is purely **volumetric**: it measures the exact volume of tokens that is **contextually reachable** from $u$.

---

## **2. Reachability and Pruning Rules**

The core of the algorithm is defining the Reachability Function that determines the set $C(u)$. Unlike a standard transitive closure (which would include the entire system), Context Reachability is **conditional**.

We define a **Pruning Function** $P(v)$ that determines whether a node acts as a "Context Boundary."

### **2.1 The Traversal Algorithm**

The set $C(u)$ is the set of all nodes visited by the following traversal:

1. **Start** at $u$. Mark $u$ as visited. Add $u$ to $R(u)$.
2. **For each neighbor** $v$ of $u$:
    - If $v$ is visited, skip (Cycle Handling).
    - Add $v$ to $R(u)$.
    - **Check Boundary Condition** $B(u, v, k)$ for edge kind $k$:
        - If $B(u, v, k) = \text{true}$ (boundary), **STOP**. Do not traverse outgoing edges from $v$.
        - If $B(u, v, k) = \text{false}$ (transparent), **CONTINUE**. Recursively traverse from $v$.

### **2.2 The Pruning Predicate $P(v)$**

To capture the nuance of human code reading (where a "well-documented interface" stops our reading, but a "poorly documented one" forces us to check the implementation), we define the boundary predicate $B(u, v, k)$ which determines whether traversal along edge $(u, v)$ of kind $k$ should stop.

**Core Asymmetry:** Forward edges check *target* specification; reverse edges check *source* specification. This reflects cognitive reality: forward dependencies ask "what does this thing I'm calling do?", while reverse dependencies ask "what inputs might I receive?" or "what state might I observe?"

### **Forward Dependencies (Call, Type, Data-Read, Inheritance)**

Traversal stops when the *target* $v$ provides sufficient specification:

- **Interface abstraction**: $v$ is accessed through an interface/abstract type with documented behavioral contract
- **Immutability**: $v$ is an immutable value object
- **Type completeness**: $v$ has fully specified signatures and documented semantics

### **Reverse Dependencies (Call-in, Shared-state Write)**

Traversal decision depends on the *source* $u$:

- **Call-in edges**: Traverse only if $u$ lacks complete specification (missing type annotations, loosely typed parameters, no documentation)
- **Shared-state write edges**: Always traverse—understanding $u$ requires knowing possible values of mutable shared state

**Conservative Principle:** When in doubt, traverse. This ensures CF remains a sound upper bound.

### **Micro-Decision Logic Table (Forward Dependencies)**

In the implementation, graph nodes are only **Function** and **Variable**; types live in TypeRegistry. The table below describes boundary decisions for the *target* node $v$ (and for CallIn/SharedStateWrite, the *source* $u$).

| **Target Type** | **Condition** | **$B(u,v,k)$ (Is Boundary?)** |
| --- | --- | --- |
| **Any** | `is_external == true` | **TRUE** (3rd-party always stops) |
| **Function** | `is_abstract_factory` (return type in TypeRegistry is abstract & doc ≥ threshold) | **TRUE\*** |
| **Function** | `is_interface_method && sig_complete && doc_score >= threshold` | **TRUE** |
| **Function** | `sig_complete && doc_score >= threshold` (Academic mode) | **TRUE** |
| **Function** | otherwise | **FALSE** |
| **Variable** | `Read` and (Const or Immutable) | **TRUE** |
| **Variable** | `Read` and Mutable, or `Write` | **FALSE** (transparent / expand to writers) |

> *Where `sig_complete` = `effectively_typed_param_count == param_count && has_return_type`. A parameter is "effectively typed" if it has a type annotation AND the type is not an unbounded TypeVar. An unbounded TypeVar (no bound, no constraints) is equivalent to `Any` and does not count as a valid annotation (see TypeVar handling in Section 6.2).*
> 
> - **Abstract Factory Rule**: A function whose return type (looked up in TypeRegistry) is an abstract type (Interface/Protocol/Trait) with doc_score ≥ threshold is a boundary, regardless of its own documentation.

> *Key Insight*: An undocumented interface is not a valid abstraction. In the implementation, type "abstraction" is checked via TypeRegistry when evaluating a function target (e.g. abstract factory); there are no type nodes in the graph.
> 

---

### **2.3. Reverse Dependencies and Dynamic Expansion**

Certain architectural patterns introduce dependencies that flow *opposite* to the usual call-graph direction. These **reverse dependencies** are required for understanding because they represent information that affects the target unit's behavior.

### **Shared-State Write Edges (The Global State Problem)**

If $u$ reads a mutable variable $S$ whose scope exceeds $u$'s own scope (module-level, class field, global state):

- Standard static analysis shows forward edge `u → S`.
- **Reverse Edge Construction**: To understand the possible values of $S$, we must know who writes to it.
- For all `w in WriteSet(S)`, add edge `u → w` to the graph.
- **Traversal Rule**: Shared-state write edges always traverse (no boundary stops them).
    - *Result*: All functions that may modify $S$ are pulled into the context. This **penalizes broad variable scope**: wider scope means more potential writers.

### **Call-in Edges (The Dynamic Typing Problem)**

If $u$ has incomplete specification (missing type annotations, loosely typed parameters like `Any` or `Object`, no documentation):

- We cannot determine the contract of $u$ locally.
- **Reverse Edge Construction**: We must check call sites to understand actual usage.
- For all callers `v` of function $u$, add edge `u → v` to the graph.
- **Traversal Rule**: Call-in edges traverse only when source $u$ (the callee) lacks complete specification. If $u$ has full type annotations and documentation, call-in edges are boundaries.
    - *Result*: For under-specified functions, context expands to all callers.

## **5. Codebase-Level Characterization**

While $CF(u)$ quantifies the context cost of a single code unit, reasoning about architectural quality at the **codebase level** requires examining the distribution of CF values across all units.

We characterize a codebase by its **CF Distribution**:

$$D_{CF} = \{CF(u) : u \in U\}$$

where $U$ is the set of all code units (functions/methods) in the codebase.

**Why Distribution, Not Aggregation?**

A naive approach might sum or average CF values across the codebase. However, this conflates two orthogonal concerns: *codebase size* and *architectural quality*. A larger codebase naturally contains more code units, inflating any sum-based metric without necessarily indicating worse design.

The key insight is that in a well-architected codebase, **individual unit CF should remain bounded regardless of total codebase size**. Effective abstractions create "context firewalls" that prevent the graph traversal from exploding, even as the system grows. This leads to a testable prediction:

- **Good Architecture**: $D_{CF}$ exhibits a tight, scale-invariant shape—median CF remains low regardless of total LOC.
- **Poor Architecture**: $D_{CF}$ shifts rightward as the codebase grows—units increasingly "leak" into each other, inflating context requirements.

**Summary Statistics for Cross-Project Comparison**

While the full distribution provides the richest diagnostic information, practical applications (e.g., regression analysis, CI/CD thresholds) often require scalar summaries. We recommend reporting a **CF Profile**:

| **Statistic** | **Interpretation** | **Use Case** |
| --- | --- | --- |
| **Median (P50)** | Typical cognitive cost for routine modifications | Cross-project comparison; regression analysis |
| **P90** | Cognitive cost for "difficult" modules | Identifying candidates for refactoring |
| **P99** | Architectural hotspots / potential God Objects | Flagging high-risk areas for review |

This approach preserves the metric's local precision—its core contribution—while providing actionable system-level insights without loss of nuance.

## **6. Implementation Strategy: Semantic Data Architecture**

Semantic analysis is decoupled from graph algorithms: the tool consumes **semantic data** (e.g. JSON from LSP-based extractors) and builds the Context Graph from it.

### **6.1 Architecture Overview**

The system assumes semantic data (e.g. `SemanticData` JSON) is already produced by an extractor. The core architecture focuses on efficiently **transforming** this data into a queryable Context Graph.

**Input**: Semantic data JSON + Source Code.

**Phase 1: Graph Construction (The Builder)**

This phase consumes semantic data (e.g. from JSON) to build the in-memory `ContextGraph` and `TypeRegistry`. It uses a **Three-Pass Strategy** to handle forward references and dynamic expansion:

1. **Pass 1: Node Allocation and Type Registry**
    - Iterate over documents and definitions.
    - **Node vs Type**: Only **functions** and **variables** become graph nodes. **Type definitions** (classes, interfaces, structs, enums, etc.) are registered in `TypeRegistry` with attributes (`type_kind`, `is_abstract`, `type_param_count`, `context_size`, `doc_score`).
    - **Node selection**: Functions (including methods, constructors) and variables (global, class field, or local) are converted to nodes; parameters and locals resolve to their nearest enclosing node for edge wiring.
    - **Metrics**: Read source spans to compute `context_size` via `SizeFunction`; for interface/abstract methods, only the signature span is used.
    - **Metadata**: Compute `doc_score` via `DocumentationScorer`.
2. **Pass 2: Edge Wiring (References)**
    - Iterate over references; resolve source/target to nearest node symbol via an enclosing map.
    - **Static edges**: Map reference roles to `Call`, `Read`, `Write`. Collect writers per variable and callers per callee for Pass 3.
    - **Pass 2.5**: Fill type references in nodes from definition details (e.g. function `return_types`, `parameters[].param_type`, variable `var_type`) using `TypeRegistry`.
3. **Pass 3: Dynamic Expansion (Reverse Edges)**
    - Add edges directly to the graph (no persistent side indices):
        - **SharedStateWrite**: For each reader of a **mutable** variable, add edge reader → each writer of that variable.
        - **CallIn**: For each **underspecified** callee (incomplete signature), add edge callee → each caller.

**Phase 2: Context Analysis (The Solver)**

Executes the traversal algorithm on the constructed graph:

- **Input**: `ContextGraph` (graph + `symbol_to_node` + `type_registry`), `PruningParams` (doc_threshold + mode; solver uses `evaluate`).
- **Output**: CF score and reachable set for each requested node.

### **6.2 Graph Schema: ContextGraph + TypeRegistry**

**Implementation Reference**: See `src/domain/graph.rs`, `src/domain/node.rs`, `src/domain/type_registry.rs`, `src/domain/edge.rs` for complete schema definitions.

The implementation uses a directed graph of **nodes** (functions and variables only), a **symbol→node** map, and a separate **Type Registry** for type definitions. Dynamic expansion is **materialized as edges** at build time (Pass 3); there are no persistent "side indices" after construction.

### **Core Components and Design Principles**

**1. ContextGraph Structure**:

- Directed graph using `petgraph::DiGraph<Node, EdgeKind>`
- Symbol lookup map: `HashMap<SymbolId, NodeIndex>`
- TypeRegistry: stores type definitions (not graph nodes)
- All edges (including `SharedStateWrite`, `CallIn`) materialized in graph after Pass 3

**2. Node Types** (only Function and Variable are nodes):

- **Function**: `parameters` (with `param_type`), `return_types`, `is_interface_method`, `is_constructor`
    - Key method: `is_signature_complete_with_registry(type_registry)` = all params *effectively* typed + has return type. A parameter typed with an unbounded TypeVar is NOT effectively typed.
- **Variable**: `var_type`, `mutability` (Const/Immutable/Mutable), `variable_kind` (Global/ClassField/Local)
- **NodeCore** (shared): `context_size`, `doc_score`, `is_external`, `span`, `file_path`

**3. TypeRegistry** (types not in graph):

- Stores Class, Interface, Struct, Enum, TypeAlias, and **TypeVar** definitions
- Attributes: `type_kind`, `is_abstract`, `type_param_count`, `type_var_info` (for TypeVar: bound + constraints), `context_size`, `doc_score`
- Queried during pruning (e.g., abstract factory detection, TypeVar-aware signature completeness)

**4. EdgeKind**:

- Forward: `Call`, `Read`, `Write`
- Reverse (dynamic expansion): `SharedStateWrite`, `CallIn`
- Annotation: `Annotates`
- Note: Type usage (param/return types) stored as node attributes, not edges

**Design Principles**:

- **Information Maximization**: Graph retains all semantic data; pruning logic decides boundary vs transparent
- **Types out of graph**: Type definitions in TypeRegistry; nodes reference by type ID
- **Dynamic expansion materialized**: Reverse edges added in Pass 3; no separate indices after build

```
/// Shared core attributes for all nodes
struct NodeCore {
    id: NodeId,
    name: String,
    scope: Option<ScopeId>,
    context_size: u32,
    span: SourceSpan,
    doc_score: f32,
    is_external: bool,
    file_path: String,
}

enum Node {
    Function(FunctionNode),
    Variable(VariableNode),
}
```

**FunctionNode**

```
struct FunctionNode {
    core: NodeCore,

    // Signature: type IDs reference TypeRegistry
    parameters: Vec<Parameter>,   // name + param_type: Option<String>
    return_types: Vec<String>,    // type IDs

    is_async: bool,
    is_generator: bool,
    visibility: Visibility,

    /// True if defined in Interface/Protocol/Trait/Abstract Class (signature only)
    is_interface_method: bool,
}

struct Parameter {
    name: String,
    param_type: Option<String>,   // TypeId
}

// Derived methods:
//   typed_param_count()  — params with any type annotation
//   effectively_typed_param_count(type_registry) — excludes unbounded TypeVar params
//   is_signature_complete() — typed_param_count == param_count && has_return_type (legacy)
//   is_signature_complete_with_registry(type_registry) — effectively_typed == param_count && has_return_type
//
// is_param_effectively_typed(param, type_registry):
//   No annotation → false
//   Type not in registry → true (conservative)
//   TypeVar with bound or constraints → true
//   TypeVar without bound/constraints → false (≈ Any)
//   Other types → true
//
// Interface methods: context_size computed only for signature span
```

**VariableNode**

```
struct VariableNode {
    core: NodeCore,

    var_type: Option<String>,     // TypeId (in TypeRegistry)
    mutability: Mutability,
    variable_kind: VariableKind,
}

enum VariableKind {
    Global,
    ClassField,
    Local,
}

enum Mutability {
    Const,
    Immutable,
    Mutable,
}
```

**TypeRegistry (types are not graph nodes)**

```
struct TypeRegistry {
    types: HashMap<TypeId, TypeInfo>,
}

struct TypeInfo {
    definition: TypeDefAttribute,
    context_size: u32,
    doc_score: f32,
}

struct TypeDefAttribute {
    type_kind: TypeKind,    // Class, Interface, Struct, Enum, TypeAlias, TypeVar, ...
    is_abstract: bool,
    type_param_count: u32,
    type_var_info: Option<TypeVarInfo>,  // Only present for TypeVar
}

struct TypeVarInfo {
    bound: Option<TypeId>,       // T(bound=SomeProtocol) — single upper bound
    constraints: Vec<TypeId>,    // T(int, str) — constrained to specific types
}
// TypeVarInfo.is_effectively_typed() = bound.is_some() OR !constraints.is_empty()
```

Pruning looks up type IDs in `graph.type_registry` for abstract factory detection and TypeVar-aware signature completeness.

### **Edge Schema**

Edges are stored as `DiGraph<Node, EdgeKind>`; each edge has a single `EdgeKind`. Type usage (param/return/field/variable type) is **not** represented as graph edges—only as node fields and TypeRegistry.

```
enum EdgeKind {
    Call,               // Function → Function

    Read,               // Function → Variable
    Write,              // Function → Variable

    SharedStateWrite,   // Reader(Function) → Writer(Function) of shared mutable state
    CallIn,             // Callee(Function) → Caller(Function) for underspecified functions

    Annotates { is_behavioral: bool },  // Decorated → Decorator
}
```

> **Edge direction**: `Annotates` points from the decorated code to the decorator (e.g. `dashboard -[Annotates]-> login_required`).
> 

**Dynamic expansion at build time**

During Pass 2 the builder collects `state_writers: HashMap<SymbolId, Vec<NodeIndex>>` and `callers: HashMap<SymbolId, Vec<NodeIndex>>`. In Pass 3 it adds `SharedStateWrite` and `CallIn` edges to the graph. The final `ContextGraph` does not retain these maps; traversal uses only the graph’s outgoing edges.

### **6.3 Pruning: Fully in Domain**

The core novelty—**Pruning Predicate** $P(v)$—is implemented entirely in the domain. Only **doc_threshold** (and a mode flag) are configurable; doc_scorer supplies doc_score.

```
#[derive(Debug, Clone)]
pub struct PruningParams {
    pub doc_threshold: f32,
    pub treat_typed_documented_function_as_boundary: bool,  // Academic vs Strict
}

pub fn evaluate(
    params: &PruningParams,
    source: &Node, target: &Node, edge_kind: &EdgeKind, graph: &ContextGraph,
) -> PruningDecision;
```

**Modes:**

1. **Academic** (`PruningParams::academic(0.5)`): Abstract factory + sig complete + doc_score >= doc_threshold → Boundary. Use case: Large-scale validation (BugsInPy, SWE-bench).
2. **Strict** (`PruningParams::strict(0.8)`): Only external and abstract factory → Boundary; other internal functions Transparent.
3. **DeepAudit** (Slow, Future Work):
    - **Logic**: LLM-based evaluation of documentation quality ("Does this docstring explain the side effects?").
    - **Use Case**: CI/CD Quality Gates for enterprise projects.

### **6.4 Detailed Algorithm Pseudocode**

This section provides detailed pseudocode for the two core algorithms: **Graph Construction** and **CF Traversal**. See implementation in `src/domain/builder.rs` and `src/domain/solver.rs`.

### **Algorithm 1: Graph Construction (Three-Pass Builder)**

**Input**: `SemanticData` (JSON), `SourceReader`, `SizeFunction`, `DocScorer`  
**Output**: `ContextGraph` with all nodes, edges (including dynamic expansion), and TypeRegistry

```
function build_graph(semantic_data, source_reader, size_function, doc_scorer):
    graph = empty_graph()
    type_registry = empty_registry()

    // Pre-compute enclosing symbol map for symbol resolution
    enclosing_map = semantic_data.build_enclosing_map()
    node_symbols = empty_set()

    // PASS 1: Node Allocation and TypeRegistry
    for each document in semantic_data.documents:
        source_code = source_reader.read(document.path)

        for each definition in document.definitions:
            // Check if this is an interface/abstract method
            is_interface_method = (definition.kind == Function AND
                                  definition.details.modifiers.is_abstract)

            // Compute context_size
            if is_interface_method:
                // Only signature span for interface methods
                signature_span = extract_signature_span(definition.span, source_code)
                context_size = size_function.compute(source_code, signature_span, definition.documentation)
            else:
                context_size = size_function.compute(source_code, definition.span, definition.documentation)

            // Compute doc_score
            doc_score = doc_scorer.score(definition, definition.documentation)

            if definition.kind == Type:
                // Register in TypeRegistry (not a graph node)
                type_info = create_type_info(definition, context_size, doc_score)
                type_registry.register(definition.symbol_id, type_info)

            else if definition.kind in [Function, Variable]:
                // Create graph node
                node_symbols.add(definition.symbol_id)
                node_core = NodeCore {
                    id: graph.next_node_id(),
                    name: definition.name,
                    scope: definition.enclosing_symbol,
                    context_size: context_size,
                    span: definition.span,
                    doc_score: doc_score,
                    is_external: definition.is_external,
                    file_path: document.path
                }

                node = create_node(node_core, definition, is_interface_method)
                graph.add_node(definition.symbol_id, node)

    // Build constructor init map: type_symbol -> __init__ node symbol
    init_map = {}
    for each definition in all_definitions:
        if (definition.kind == Function AND
            definition.name == "__init__" AND
            definition.enclosing_symbol in type_registry AND
            definition.symbol_id in node_symbols):
            init_map[definition.enclosing_symbol] = definition.symbol_id

    // PASS 2: Edge Wiring (Static Edges + Collect for Dynamic Expansion)
    state_writers = {}  // variable_symbol -> list of writer nodes
    callers = {}        // function_symbol -> list of caller nodes
    readers = []        // list of (reader_node, variable_symbol) pairs

    for each document in semantic_data.documents:
        for each reference in document.references:
            // Resolve symbols to nearest node ancestors
            source_node_symbol = resolve_to_node_symbol(
                reference.enclosing_symbol, node_symbols, enclosing_map)
            target_node_symbol = resolve_to_node_symbol(
                reference.target_symbol, node_symbols, enclosing_map)

            if source_node_symbol is None:
                continue

            source_idx = graph.get_node_index(source_node_symbol)

            // Handle Call edges
            if reference.role == Call:
                // Try direct target, or fallback to constructor (__init__)
                target_idx = graph.get_node_index(target_node_symbol)
                            OR graph.get_node_index(init_map[reference.target_symbol])

                if target_idx exists AND source_idx != target_idx:
                    graph.add_edge(source_idx, target_idx, EdgeKind.Call)
                    callers[resolved_target_symbol].append(source_idx)

            // Handle Read/Write edges
            if reference.role in [Read, Write]:
                target_idx = graph.get_node_index(target_node_symbol)
                if target_idx exists AND source_idx != target_idx:
                    edge_kind = EdgeKind.Write if reference.role == Write else EdgeKind.Read
                    graph.add_edge(source_idx, target_idx, edge_kind)

                    if reference.role == Write:
                        state_writers[reference.target_symbol].append(source_idx)
                    else:
                        readers.append((source_idx, reference.target_symbol))

            // Handle Decorate edges (decorated -> decorator)
            if reference.role == Decorate:
                target_idx = graph.get_node_index(target_node_symbol)
                if target_idx exists AND source_idx != target_idx:
                    graph.add_edge(source_idx, target_idx, EdgeKind.Annotates)

    // PASS 2.5: Fill type references in nodes from definition details
    for each definition in all_definitions:
        if definition.symbol_id in graph:
            node = graph.get_node(definition.symbol_id)
            if node is FunctionNode:
                // Set return_types from definition
                for type_id in definition.details.return_types:
                    if type_id in type_registry:
                        node.return_types.append(type_id)
            else if node is VariableNode:
                // Set var_type from definition
                if definition.details.var_type in type_registry:
                    node.var_type = definition.details.var_type

    // PASS 3: Dynamic Expansion Edges
    // 1. SharedStateWrite: reader -> writers of mutable variables
    for (reader_idx, var_symbol) in readers:
        if is_variable_mutable(var_symbol, semantic_data):
            for writer_idx in state_writers[var_symbol]:
                if reader_idx != writer_idx:
                    graph.add_edge(reader_idx, writer_idx, EdgeKind.SharedStateWrite)

    // 2. CallIn: underspecified callee -> callers
    for callee_symbol in graph.all_node_symbols():
        callee_idx = graph.get_node_index(callee_symbol)
        if is_function_underspecified(callee_symbol, graph):
            for caller_idx in callers[callee_symbol]:
                if callee_idx != caller_idx:
                    graph.add_edge(callee_idx, caller_idx, EdgeKind.CallIn)

    graph.type_registry = type_registry
    return graph

function resolve_to_node_symbol(symbol, node_symbols, enclosing_map):
    // Walk up enclosing chain until we find a node symbol
    current = symbol
    while current is not None:
        if current in node_symbols:
            return current
        current = enclosing_map.get(current, None)
    return None

function is_function_underspecified(symbol, graph):
    node = graph.get_node(symbol)
    if node is not FunctionNode:
        return false
    return NOT node.is_signature_complete()

function is_variable_mutable(symbol, semantic_data):
    definition = semantic_data.find_definition(symbol)
    if definition AND definition.kind == Variable:
        return definition.details.mutability == Mutable
    return true  // Conservative default
```

### **Algorithm 2: CF Computation (BFS with Conditional Pruning)**

**Input**: `ContextGraph`, `start_nodes[]`, `PruningParams`, `max_tokens` (optional)  
**Output**: `CfResult` with reachable set, total size, traversal steps, layers

```
function compute_cf(graph, start_nodes, pruning_params, max_tokens):
    visited = empty_set()
    ordered = []  // reachable nodes in BFS order
    traversal_steps = []
    layers = []  // reachable nodes grouped by BFS depth
    queue = empty_queue()
    total_size = 0

    // Initialize with start nodes at depth 0
    for start in start_nodes:
        queue.enqueue((start, depth=0, incoming_edge=None, decision=None))

    while queue is not empty:
        (current, depth, incoming_edge, incoming_decision) = queue.dequeue()
        current_node = graph.node(current)
        current_id = current_node.core.id

        // Skip if already visited
        if current_id in visited:
            continue

        // Mark as visited and accumulate
        visited.add(current_id)
        node_size = current_node.core.context_size
        total_size += node_size
        ordered.append(current_id)
        traversal_steps.append(TraversalStep {
            node_id: current_id,
            incoming_edge_kind: incoming_edge,
            decision: incoming_decision
        })

        // Add to layers
        if depth >= layers.length:
            layers.append([])
        layers[depth].append(current_id)

        // Check token limit
        if max_tokens is not None AND total_size >= max_tokens:
            break

        // Get neighbors and sort by symbol for deterministic traversal
        neighbors = graph.neighbors(current).sort_by_symbol()

        for (neighbor, edge_kind) in neighbors:
            neighbor_node = graph.node(neighbor)
            neighbor_id = neighbor_node.core.id

            // Evaluate pruning decision
            decision = evaluate(pruning_params, current_node, neighbor_node, edge_kind, graph)

            if decision == Transparent:
                // Continue traversal through this node
                queue.enqueue((neighbor, depth + 1, edge_kind, decision))

            else:  // decision == Boundary
                // Include boundary node in reachable set but don't traverse
                if neighbor_id not in visited:
                    boundary_size = neighbor_node.core.context_size

                    // Check if adding boundary exceeds limit
                    if max_tokens is not None AND total_size + boundary_size > max_tokens:
                        break

                    visited.add(neighbor_id)
                    total_size += boundary_size
                    ordered.append(neighbor_id)
                    traversal_steps.append(TraversalStep {
                        node_id: neighbor_id,
                        incoming_edge_kind: edge_kind,
                        decision: decision
                    })
                    layers[depth + 1].append(neighbor_id)

        // Re-check token limit after processing neighbors
        if max_tokens is not None AND total_size >= max_tokens:
            break

    return CfResult {
        reachable_set: visited,
        reachable_nodes_ordered: ordered,
        reachable_nodes_by_layer: layers,
        traversal_steps: traversal_steps,
        total_context_size: total_size
    }

// Pruning decision function (domain/policy.rs)
function evaluate(params, source, target, edge_kind, graph):
    // 1. Dynamic expansion edges
    if edge_kind == SharedStateWrite:
        return Transparent  // Always traverse to understand mutable state

    if edge_kind == CallIn:
        // Check source (callee) specification
        if source is Function AND
           source.is_signature_complete() AND
           source.core.doc_score >= params.doc_threshold:
            return Boundary
        return Transparent

    // 2. External dependencies always stop
    if target.core.is_external:
        return Boundary

    // 3. Node type dispatch
    if target is Variable:
        if edge_kind == Write:
            return Transparent  // Writing is an action
        else:  // Read
            if target.mutability in [Const, Immutable]:
                return Boundary  // Immutable values fully determined
            else:
                return Transparent  // Mutable state triggers expansion

    else if target is Function:
        // Interface/abstract methods
        if target.is_interface_method:
            if target.is_signature_complete() AND
               target.core.doc_score >= params.doc_threshold:
                return Boundary
            return Transparent  // Undocumented interface = leaky abstraction

        // Constructor with complete signature
        if target.is_constructor AND target.is_signature_complete():
            return Boundary

        // Abstract factory pattern
        if is_abstract_factory(target, graph.type_registry, params.doc_threshold):
            return Boundary

        // Academic mode: typed + documented function is boundary
        if params.treat_typed_documented_function_as_boundary AND
           target.is_signature_complete() AND
           target.core.doc_score >= params.doc_threshold:
            return Boundary

        return Transparent

function is_abstract_factory(function_node, type_registry, doc_threshold):
    if function_node.return_types is empty OR
       NOT function_node.is_signature_complete():
        return false

    // Check if ANY return type is abstract
    for type_id in function_node.return_types:
        type_info = type_registry.get(type_id)
        if type_info AND type_info.definition.is_abstract:
            return true

    return false
```

### **Key Implementation Details**

**Symbol Resolution** (`resolve_to_node_symbol`):

- References may point to non-node symbols (e.g., parameters, local variables, type definitions)
- Walk up the enclosing chain until finding a Function or Variable node
- Example: Reference to parameter `x` resolves to its enclosing function

**Constructor Handling**:

- Type instantiation references (e.g., `MyClass()`) map to `__init__` method via `init_map`
- Fallback mechanism: try direct target, then try constructor

**Dynamic Expansion Timing**:

- `SharedStateWrite` edges added only for mutable variables (checked via semantic data)
- `CallIn` edges added only for underspecified functions (checked via `is_signature_complete()`)

**Traversal Ordering**:

- BFS ensures shortest path to each node
- Neighbors sorted by symbol for deterministic results (testing, debugging)
- Layers group nodes by distance from start

**Boundary Node Inclusion**:

- Boundary nodes ARE included in reachable set and CF total
- Represents the interface/contract being consumed
- Their outgoing edges are NOT traversed

---

## **7. Future Work: Constrained TypeVar Multi-Type Expansion**

For constrained TypeVars like `T(int, str)`, `is_signature_complete_with_registry()` returns `true` (no call-in expansion). However, when the function body calls methods on a constrained TypeVar parameter, all constraint types' corresponding methods should be added as dependencies.

**Approach**: Extend Pass 2.5 — when a function parameter is typed with a constrained TypeVar and the function body calls methods on that parameter, create Call edges to each constraint type's corresponding method.

**Priority**: P1. First verify LSP behavior for constrained TypeVars — if the LSP already resolves method calls to concrete constraint type methods, no additional handling is needed.