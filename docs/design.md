**Status**: Draft

**Related Paper**: [ Context Footprint](https://www.notion.so/Context-Footprint-2e979ac6bd188055a3cbe83a06729809?pvs=21) 

## 1. Formal Definition: The Context Graph

To provide an objective, mathematically rigorous definition, we model the software system as a directed graph.

### 1.1 The Graph Model

Let a codebase be represented as a directed graph $G = (V, E)$, where:

- $V$ **(Nodes)**: The set of relevent code units (functions, variables, type definitions).
- $E$ **(Edges)**: The set of dependency relationships, where $(u, v) \in E$ implies $u$ depends on $v$. Edge types include:
    - **Control flow**: `Call`
    - **Type usage**: `ParamType`, `ReturnType`, `FieldType`, `VariableType`, `GenericBound`, `TypeArgument`
    - **Type hierarchy**: `Inherits`, `Implements`
    - **Data flow**: `Read`, `Write`
    - **Annotations**: `Annotates`
    - **Exceptions**: `Throws`

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

| **Node Type** | **Condition** | $B(u,v,k)$ **(Is Boundary?)** |
| --- | --- | --- |
| **Any** | `is_external == true` | **TRUE** (3rd-party always stops) |
| **Type** | `is_abstract && doc_score >= threshold` | **TRUE** |
| **Type** | `!is_abstract || doc_score < threshold` | **FALSE** (leaky abstraction) |
| **Function** | `is_abstract_factory` | **TRUE*** |
| **Function** | `sig_complete && doc_score >= threshold` | **TRUE** |
| **Function** | `!sig_complete || doc_score < threshold` | **FALSE** |
| **Variable** | (any) | **FALSE** (always traverse to type) |

> *Where `sig_complete` = `typed_param_count == param_count && has_return_type`*
> 
> ***Abstract Factory Rule**: A function that returns an abstract type (Interface/Protocol) with sufficient documentation is considered a boundary, regardless of its own documentation quality. This identifies patterns where the caller only interacts with the returned interface.

> *Key Insight*: An undocumented interface is not a valid abstraction. It fails to compress context because the reader must still inspect the implementation to understand the behavior. In our graph, this "leaky abstraction" is treated as a transparent node.
> 

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

This phase consumes the SCIP stream to build the in-memory `ContextGraph` and `SideIndices`. It requires a **Two-Pass Strategy** to handle forward references and efficient memory allocation:

1. **Pass 1: Node Allocation (Definitions)**
    - Iterate over SCIP `Documents` and `Symbols`.
    - **Node Selection**: Only definitions that represent "independent" code units are converted to nodes. 
        - Functions, Classes, and Types are always nodes.
        - Variables and Fields are only nodes if they are not local to a function (e.g., global variables or class fields).
        - Parameters and Local variables are resolved to their nearest node ancestor.
    - **Metrics**: Read source file ranges to calculate `context_size`.
    - **Metadata**: Extract facts and compute `doc_score` using a `DocumentationScorer`.
2. **Pass 2: Edge Wiring (Occurrences)**
    - Iterate over `Occurrences` to identify relationships.
    - **Static Edges**: Map SCIP occurrences to granular `EdgeKind` (`Call`, `Read`, `Write`, etc.).
    - **Relationship Resolution**: Symbols that are not nodes (like a local variable reference) are resolved to their nearest enclosing node symbol to maintain graph connectivity.
3. **Pass 3: Dynamic Expansion (Reverse Edges)**
    - Automatically inject reverse dependency edges:
        - **SharedStateWrite**: From readers of mutable state to all known writers of that state.
        - **CallIn**: From callees to all known callers.

**Phase 2: Context Analysis (The Solver)**

Executes the traversal algorithm on the constructed graph:

- **Input**: `ContextGraph`, `SideIndices`, `PruningParams` (doc_threshold + mode; solver uses `evaluate`).
- **Output**: `CF` Score for each requested Node.

### 6.2 Graph Schema: The "Compact Graph + Side Indices" Model

To balance memory efficiency with the complex traversal requirements (pruning & expansion), we define a compact core graph supplemented by specific reverse lookups.

#### Design Principles

1. **Information Maximization**: Retain as much semantic information as possible during graph building; let the Pruning Policy decide what to use.
2. **Polymorphic Nodes**: Different node types have distinct attributes; use a shared core with type-specific extensions.
3. **Language Superset**: Schema covers the union of all target languages' features; weaker languages simply use a subset.

#### Node Hierarchy

Three primary node types: **Function**, **Variable**, **Type**. Module/Namespace is demoted to a node property (`scope`), not a graph entity.

```rust
/// Shared core attributes for all nodes
struct NodeCore {
    id: NodeId,
    name: String,               // Symbol name (e.g., "calculate_total")
    scope: Option<ScopeId>,     // Module/Namespace (organizational, not an entity)
    context_size: u32,          // Physical volume (CF calculation basis)
    span: SourceSpan,           // Source location (for debugging/visualization)
    doc_score: f32,             // Documentation quality score [0.0, 1.0]
    is_external: bool,          // 3rd-party library (always a Boundary)
    file_path: String,          // Source file path
}

/// Sum type for polymorphic nodes
enum Node {
    Function(FunctionNode),
    Variable(VariableNode),
    Type(TypeNode),
}
```

**FunctionNode**

```rust
struct FunctionNode {
    core: NodeCore,
    
    // === Signature Completeness Signals ===
    param_count: u32,           // Total parameters in signature
    typed_param_count: u32,     // Parameters with type annotations
    has_return_type: bool,      // Return type annotation present
    
    // === Behavioral Signals ===
    is_async: bool,
    is_generator: bool,
    visibility: Visibility,     // Public, Private, Protected, Internal
}

// Derived: Signature Completeness =
//   typed_param_count == param_count && has_return_type
// Missing ParamType edges → triggers Uncertainty Expansion
```

**VariableNode**

```rust
struct VariableNode {
    core: NodeCore,
    
    // === Type Annotation ===
    has_type_annotation: bool,
    
    // === Mutability (critical for Expansion) ===
    mutability: Mutability,
    
    // === Scope Kind ===
    variable_kind: VariableKind,
}

enum VariableKind {
    Global,         // Module-level (Mutability Expansion focus)
    ClassField,     // Class/struct field
    // Note: LocalVar excluded—token cost already in FunctionNode
    // Note: Parameter expressed as ParamType edge, not a node
}

enum Mutability {
    Const,      // Compile-time constant
    Immutable,  // Runtime immutable
    Mutable,    // Mutable (Expansion trigger)
}
```

**TypeNode**

```rust
struct TypeNode {
    core: NodeCore,
    
    // === Type Classification ===
    type_kind: TypeKind,
    
    // === Abstraction Signal (Pruning key) ===
    is_abstract: bool,          // interface, protocol, abstract class
    
    // === Generics ===
    type_param_count: u32,      // Generic parameters (e.g., List<T> → 1)
}

enum TypeKind {
    Class,
    Interface,      // Java, Go, TypeScript
    Protocol,       // Python, Swift
    Struct,
    Enum,
    TypeAlias,      // type UserId = string
    FunctionType,   // (int, int) -> bool
    Union,          // A | B
    Intersection,   // A & B
}
```

#### Edge Schema

```rust
struct Edge {
    source: NodeId,
    target: NodeId,
    kind: EdgeKind,
}

enum EdgeKind {
    // ============ Control Flow ============
    Call,               // Function → Function

    // ============ Type Usage (granular) ============
    ParamType,          // Function → Type (parameter type dependency)
    ReturnType,         // Function → Type (return type dependency)
    FieldType,          // Type → Type (field type, e.g., Order.customer: Customer)
    VariableType,       // Variable → Type (declared type)
    GenericBound,       // Type → Type (e.g., T: Comparable)
    TypeArgument,       // Usage → Type (generic instantiation, e.g., User in List<User>)

    // ============ Type Hierarchy ============
    Inherits,           // Type → Type (class extends)
    Implements,         // Type → Type (class implements interface)

    // ============ Data Flow (Expansion triggers) ============
    Read,               // Function → Variable
    Write,              // Function → Variable

    // ============ Dynamic Expansion (Reverse Dependencies) ============
    SharedStateWrite,   // Reader → Writer (of shared mutable state)
    CallIn,             // Callee → Caller (for underspecified functions)

    // ============ Annotations & Decorators ============
    /// Decorated → Decorator direction (understanding decorated requires decorator)
    Annotates {
        is_behavioral: bool,  // true = decorator (strong dep), false = metadata (weak dep)
    },

    // ============ Exception Flow ============
    Throws,             // Function → Type (exception type)
}
```

> **Edge Direction Clarification**: For `Annotates`, the edge points from the decorated/annotated code to the decorator/annotation. E.g., `dashboard -[Annotates]-> login_required`. This reflects the cognitive dependency: understanding `dashboard`'s actual behavior requires understanding `login_required`.
> 

**Side Indices (The "Nervous System")**

To handle *Dynamic Expansion* without bloating the static graph with bidirectional edges, we maintain targeted auxiliary indices:

1. **State Index (for Mutability Expansion)**
    - *Purpose*: Finding writers of global state.
    - *Structure*: `HashMap<SymbolId, Vec<NodeId>>`
    - *Query*: `state_symbol -> list_of_writers`
2. **Caller Index (for Uncertainty Expansion)**
    - *Purpose*: Resolving untyped parameters by checking call sites.
    - *Structure*: `HashMap<NodeId, Vec<NodeId>>`
    - *Query*: `callee_id -> list_of_callers`
    - *Optimization*: Can be lazily populated or restricted to "Untyped" callees if memory becomes a bottleneck.

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