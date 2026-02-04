**Status**: Draft

**Related Paper**: [ Context Footprint](https://www.notion.so/Context-Footprint-2e979ac6bd188055a3cbe83a06729809?pvs=21) 

## 1. Formal Definition: The Context Graph

To provide an objective, mathematically rigorous definition, we model the software system as a directed graph.

### 1.1 The Graph Model

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

### 1.2 Context as a Subgraph

We define the **Context** of a unit $u$, denoted as $R(u)$, as a **specific subset of nodes** $R(u) \subseteq V$ that must be loaded to satisfy Information Completeness. This set is computed via conditional traversal from $u$ (see Section 2).

The **Context Footprint (CF)** is the total volume of all reached nodes:

$$
CF(u) = \sum_{v \in R(u)} size(v)
$$

where $size(v)$ is the context size of unit $v$. By definition, $R(u)$ includes $u$ itself, and significantly, it **includes the boundary nodes** themselves (representing the interface being consumed) but not their implementation details.

This definition removes subjective "cognitive weights." The metric is purely **volumetric**: it measures the exact volume of tokens that is **contextually reachable** from $u$.

---

## 2. Reachability and Pruning Rules

The core of the algorithm is defining the Reachability Function that determines the set $C(u)$. Unlike a standard transitive closure (which would include the entire system), Context Reachability is **conditional**.

We define a **Pruning Function** $P(v)$ that determines whether a node acts as a "Context Boundary."

### 2.1 The Traversal Algorithm

The set $C(u)$ is the set of all nodes visited by the following traversal:

1. **Start** at $u$. Mark $u$ as visited. Add $u$ to $R(u)$.
2. **For each neighbor** $v$ of $u$:
    - If $v$ is visited, skip (Cycle Handling).
    - Add $v$ to $R(u)$.
    - **Check Boundary Condition** $B(u, v, k)$ for edge kind $k$:
        - If $B(u, v, k) = \text{true}$ (boundary), **STOP**. Do not traverse outgoing edges from $v$.
        - If $B(u, v, k) = \text{false}$ (transparent), **CONTINUE**. Recursively traverse from $v$.

### 2.2 The Pruning Predicate $P(v)$

To capture the nuance of human code reading (where a "well-documented interface" stops our reading, but a "poorly documented one" forces us to check the implementation), we define the boundary predicate $B(u, v, k)$ which determines whether traversal along edge $(u, v)$ of kind $k$ should stop.

**Core Asymmetry:** Forward edges check *target* specification; reverse edges check *source* specification. This reflects cognitive reality: forward dependencies ask "what does this thing I'm calling do?", while reverse dependencies ask "what inputs might I receive?" or "what state might I observe?"

#### Forward Dependencies (Call, Type, Data-Read, Inheritance)

Traversal stops when the *target* $v$ provides sufficient specification:

- **Interface abstraction**: $v$ is accessed through an interface/abstract type with documented behavioral contract
- **Immutability**: $v$ is an immutable value object
- **Type completeness**: $v$ has fully specified signatures and documented semantics

#### Reverse Dependencies (Call-in, Shared-state Write)

Traversal decision depends on the *source* $u$:

- **Call-in edges**: Traverse only if $u$ lacks complete specification (missing type annotations, loosely typed parameters, no documentation)
- **Shared-state write edges**: Always traverse—understanding $u$ requires knowing possible values of mutable shared state

**Conservative Principle:** When in doubt, traverse. This ensures CF remains a sound upper bound.

#### Micro-Decision Logic Table (Forward Dependencies)

In the implementation, graph nodes are only **Function** and **Variable**; types live in TypeRegistry. The table below describes boundary decisions for the *target* node $v$ (and for CallIn/SharedStateWrite, the *source* $u$).

| **Target Type** | **Condition** | $B(u,v,k)$ **(Is Boundary?)** |
| --- | --- | --- |
| **Any** | `is_external == true` | **TRUE** (3rd-party always stops) |
| **Function** | `is_abstract_factory` (return type in TypeRegistry is abstract & doc ≥ threshold) | **TRUE*** |
| **Function** | `is_interface_method && sig_complete && doc_score >= threshold` | **TRUE** |
| **Function** | `sig_complete && doc_score >= threshold` (Academic mode) | **TRUE** |
| **Function** | otherwise | **FALSE** |
| **Variable** | `Read` and (Const or Immutable) | **TRUE** |
| **Variable** | `Read` and Mutable, or `Write` | **FALSE** (transparent / expand to writers) |

> *Where `sig_complete` = `typed_param_count == param_count && has_return_type` (derived from `parameters` and `return_types`).*
>
> ***Abstract Factory Rule**: A function whose return type (looked up in TypeRegistry) is an abstract type (Interface/Protocol/Trait) with doc_score ≥ threshold is a boundary, regardless of its own documentation.

> *Key Insight*: An undocumented interface is not a valid abstraction. In the implementation, type "abstraction" is checked via TypeRegistry when evaluating a function target (e.g. abstract factory); there are no type nodes in the graph.

---

### 2.3. Reverse Dependencies and Dynamic Expansion

Certain architectural patterns introduce dependencies that flow *opposite* to the usual call-graph direction. These **reverse dependencies** are required for understanding because they represent information that affects the target unit's behavior.

### Shared-State Write Edges (The Global State Problem)

If $u$ reads a mutable variable $S$ whose scope exceeds $u$'s own scope (module-level, class field, global state):

- Standard static analysis shows forward edge `u → S`.
- **Reverse Edge Construction**: To understand the possible values of $S$, we must know who writes to it.
- For all `w in WriteSet(S)`, add edge `u → w` to the graph.
- **Traversal Rule**: Shared-state write edges always traverse (no boundary stops them).
    - *Result*: All functions that may modify $S$ are pulled into the context. This **penalizes broad variable scope**: wider scope means more potential writers.

### Call-in Edges (The Dynamic Typing Problem)

If $u$ has incomplete specification (missing type annotations, loosely typed parameters like `Any` or `Object`, no documentation):

- We cannot determine the contract of $u$ locally.
- **Reverse Edge Construction**: We must check call sites to understand actual usage.
- For all callers `v` of function $u$, add edge `u → v` to the graph.
- **Traversal Rule**: Call-in edges traverse only when source $u$ (the callee) lacks complete specification. If $u$ has full type annotations and documentation, call-in edges are boundaries.
    - *Result*: For under-specified functions, context expands to all callers.

## 5. Codebase-Level Characterization

While $CF(u)$ quantifies the context cost of a single code unit, reasoning about architectural quality at the **codebase level** requires examining the distribution of CF values across all units.

We characterize a codebase by its **CF Distribution**:

$$
D_{CF} = \{CF(u) : u \in U\}
$$

where $U$ is the set of all code units (functions/methods) in the codebase.

**Why Distribution, Not Aggregation?**

A naive approach might sum or average CF values across the codebase. However, this conflates two orthogonal concerns: *codebase size* and *architectural quality*. A larger codebase naturally contains more code units, inflating any sum-based metric without necessarily indicating worse design.

The key insight is that in a well-architected codebase, **individual unit CF should remain bounded regardless of total codebase size**. Effective abstractions create "context firewalls" that prevent the graph traversal from exploding, even as the system grows. This leads to a testable prediction:

- **Good Architecture**: $D_{CF}$ exhibits a tight, scale-invariant shape—median CF remains low regardless of total LOC.
- **Poor Architecture**: $D_{CF}$ shifts rightward as the codebase grows—units increasingly "leak" into each other, inflating context requirements.

**Summary Statistics for Cross-Project Comparison**

While the full distribution provides the richest diagnostic information, practical applications (e.g., regression analysis, CI/CD thresholds) often require scalar summaries. We recommend reporting a **CF Profile**:

| Statistic | Interpretation | Use Case |
| --- | --- | --- |
| **Median (P50)** | Typical cognitive cost for routine modifications | Cross-project comparison; regression analysis |
| **P90** | Cognitive cost for "difficult" modules | Identifying candidates for refactoring |
| **P99** | Architectural hotspots / potential God Objects | Flagging high-risk areas for review |

This approach preserves the metric's local precision—its core contribution—while providing actionable system-level insights without loss of nuance.

## 6. Implementation Strategy: SCIP-based Architecture

We leverage the **SCIP (Source Code Indexing Protocol)** ecosystem to decouple semantic analysis from graph algorithms.

### 6.1 Architecture Overview

The system assumes `index.scip` is already generated by standard CLI tools. The core architecture focuses on efficiently **transforming** this flat index into a queryable Context Graph.

**Input**: `index.scip` (Protobuf) + Source Code.

**Phase 1: Graph Construction (The Builder)**

This phase consumes semantic data (e.g. from SCIP) to build the in-memory `ContextGraph` and `TypeRegistry`. It uses a **Three-Pass Strategy** to handle forward references and dynamic expansion:

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

### 6.2 Graph Schema: ContextGraph + TypeRegistry

The implementation uses a directed graph of **nodes** (functions and variables only), a **symbol→node** map, and a separate **Type Registry** for type definitions. Dynamic expansion is **materialized as edges** at build time (Pass 3); there are no persistent "side indices" after construction.

#### ContextGraph Structure

```rust
/// Symbol identifier (SCIP symbol string)
pub type SymbolId = String;

pub struct ContextGraph {
    /// The directed graph: petgraph DiGraph<Node, EdgeKind>
    pub graph: DiGraph<Node, EdgeKind>,

    /// Mapping from symbol to node index (for lookup and traversal entry)
    pub symbol_to_node: HashMap<SymbolId, NodeIndex>,

    /// Type definitions live here, not as graph nodes; queried during pruning (e.g. abstract factory)
    pub type_registry: TypeRegistry,
}
```

Traversal uses `graph.neighbors_directed(idx, Outgoing)`; all edges (including `SharedStateWrite` and `CallIn`) are stored in the graph. The solver does not use separate state/caller indices.

#### Design Principles

1. **Information Maximization**: Retain as much semantic information as possible during graph building; pruning policy (`evaluate`) decides boundary vs transparent.
2. **Polymorphic Nodes**: Two node variants—Function and Variable—with a shared `NodeCore` and type-specific fields.
3. **Types out of graph**: Type definitions are not nodes; they are in `TypeRegistry`. Type usage is represented by node attributes (e.g. `return_types`, `parameters[].param_type`, `var_type`) referencing type IDs.

#### Node Hierarchy

Only **Function** and **Variable** are graph nodes. Module/namespace is a node property (`scope`). Types are in `TypeRegistry`.

```rust
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

```rust
struct FunctionNode {
    core: NodeCore,

    // Signature: type IDs reference TypeRegistry
    parameters: Vec<Parameter>,   // name + param_type: Option<String>
    return_types: Vec<String>,    // type IDs

    is_async: bool,
    is_generator: bool,
    visibility: Visibility,

    /// True if defined in Interface/Protocol/Trait/Abstract Class (signature only)
    is_interface_method: bool,
}

struct Parameter {
    name: String,
    param_type: Option<String>,   // TypeId
}

// Derived (methods): typed_param_count(), param_count(), has_return_type(), is_signature_complete()
// Signature completeness = typed_param_count == param_count && has_return_type
// Interface methods: context_size computed only for signature span
```

**VariableNode**

```rust
struct VariableNode {
    core: NodeCore,

    var_type: Option<String>,     // TypeId (in TypeRegistry)
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

```rust
struct TypeRegistry {
    types: HashMap<TypeId, TypeInfo>,
}

struct TypeInfo {
    definition: TypeDefAttribute,
    context_size: u32,
    doc_score: f32,
}

struct TypeDefAttribute {
    type_kind: TypeKind,    // Class, Interface, Struct, Enum, TypeAlias, ...
    is_abstract: bool,
    type_param_count: u32,
}
```

Pruning (e.g. abstract factory) looks up return type IDs in `graph.type_registry` to decide boundary.

#### Edge Schema

Edges are stored as `DiGraph<Node, EdgeKind>`; each edge has a single `EdgeKind`. Type usage (param/return/field/variable type) is **not** represented as graph edges—only as node fields and TypeRegistry.

```rust
enum EdgeKind {
    Call,               // Function → Function

    Read,               // Function → Variable
    Write,              // Function → Variable

    SharedStateWrite,   // Reader(Function) → Writer(Function) of shared mutable state
    CallIn,             // Callee(Function) → Caller(Function) for underspecified functions

    Annotates { is_behavioral: bool },  // Decorated → Decorator
}
```

> **Edge direction**: `Annotates` points from the decorated code to the decorator (e.g. `dashboard -[Annotates]-> login_required`).
>

**Dynamic expansion at build time**

During Pass 2 the builder collects `state_writers: HashMap<SymbolId, Vec<NodeIndex>>` and `callers: HashMap<SymbolId, Vec<NodeIndex>>`. In Pass 3 it adds `SharedStateWrite` and `CallIn` edges to the graph. The final `ContextGraph` does not retain these maps; traversal uses only the graph’s outgoing edges.

### 6.3 Pruning: Fully in Domain

The core novelty—**Pruning Predicate** $P(v)$—is implemented entirely in the domain. Only **doc_threshold** (and a mode flag) are configurable; doc_scorer supplies doc_score.

```rust
#[derive(Debug, Clone)]
pub struct PruningParams {
    pub doc_threshold: f32,
    pub treat_typed_documented_function_as_boundary: bool,  // Academic vs Strict
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

### 6.4 Implementation Phases (Revised)

1. **Phase 1**: SCIP Ingestion & Graph Builder (Rust).
2. **Phase 2**: Baseline Policy (Type Hint + Doc presence check).
3. **Phase 3**: Dynamic Expansion Logic (State Index for Mutability).
4. **Phase 4**: CLI Tools for Visualization & Statistics.
5. **Phase 5**: Experiment Runner (Batch processing BugsInPy).