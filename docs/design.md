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

Each node $u \in V$ has an intrinsic physical property:

- $S(u)$: The context size of unit $u$. (Using token size to represent context size, by using a standard tokenizer like cl100k_base or similar).

### 1.2 Context as a Subgraph

We define the **Context** of a unit $u$, denoted as $R(u)$, as a **specific subset of nodes** $R(u) \subseteq V$ that must be loaded to satisfy Information Completeness. This set is computed via conditional traversal from $u$ (see Section 2).

The **Context Footprint (CF)** is simply the total token volume of this subgraph:

$$
CF(u) = \sum_{v \in R(u)} size(v)
$$

where $size(v)$ is a non-negative measure of the information content of unit $v$ (e.g., token count). By definition, $R(u)$ includes $u$ itself—the footprint for sound reasoning includes the focal code, not merely its external dependencies.

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
| **Any** | `is_external = true` | **TRUE** (3rd-party always stops) |
| **Type** | `is_abstract && has_doc` | **TRUE** |
| **Type** | `!is_abstract || !has_doc` | **FALSE** (leaky abstraction) |
| **Function** | `sig_complete && has_doc` | **TRUE** |
| **Function** | `!sig_complete || !has_doc` | **FALSE** |
| **Variable** | (any) | **FALSE** (always traverse to type) |

> *Where `sig_complete` = `typed_param_count == param_count && has_return_type`*
> 

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
- **Traversal Rule**: Call-in edges traverse only when source $u$ lacks complete specification. If $u$ has full type annotations and documentation, call-in edges are boundaries.
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
    - **NodeType**: Extract node type from symbol string. Three primary types:
        - **Function**: Methods, functions, lambdas (module/class stored in `scope`)
        - **Variable**: Global variables, class fields (excludes local vars and parameters)
        - **Type**: Class, Interface, Protocol, Struct, Enum, TypeAlias, FunctionType, Union, Intersection
    - **Metrics**: Read source file ranges to calculate `token_count` (using `tiktoken` or similar).
    - **Metadata**: Extract type-specific facts:
        - Function: `param_count`, `typed_param_count`, `has_return_type`, `is_async`, `visibility`
        - Variable: `has_type_annotation`, `mutability`, `variable_kind`
        - Type: `type_kind`, `is_abstract`, `type_param_count`
2. **Pass 2: Edge Wiring (Occurrences)**
    - Iterate over `Occurrences` to identify relationships.
    - **Edges**: Map SCIP occurrences to granular `EdgeKind`:
        - Control flow: `Call`
        - Type usage: `ParamType`, `ReturnType`, `FieldType`, `VariableType`, `GenericBound`, `TypeArgument`
        - Type hierarchy: `Inherits`, `Implements`
        - Data flow: `Read`, `Write`
        - Annotations: `Annotates` (with `is_behavioral` flag)
        - Exceptions: `Throws`
    - **Indices**: Populate `StateIndex` (for global writes) and `CallerIndex` (for untyped calls).

**Phase 2: Context Analysis (The Solver)**

Executes the traversal algorithm on the constructed graph:

- **Input**: `ContextGraph`, `SideIndices`, `PruningPolicy`.
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
    token_count: u32,           // Physical volume (CF calculation basis)
    span: SourceSpan,           // Source location (for debugging/visualization)
    has_doc: bool,              // Universal: documentation presence
    is_external: bool,          // 3rd-party library (always a Boundary)
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
    visibility: Visibility,     // Public, Private, Protected
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

### 6.3 Extension Point: The Pruning Policy

The core novelty—**Pruning Predicate** $P(v)$—is implemented as a configurable Trait, allowing different "Context Definitions" for different use cases.

```rust
pub trait PruningPolicy {
    /// Evaluate if a node acts as a valid Context Boundary
    fn evaluate(&self, node: &Node, graph: &ContextGraph) -> PruningDecision;
}

enum PruningDecision {
    Boundary,     // Stop traversal here; node is a valid abstraction
    Transparent,  // Continue traversal through this node
}
```

**Reference Implementation: `AcademicBaseline`**

```rust
impl PruningPolicy for AcademicBaseline {
    fn evaluate(&self, node: &Node, graph: &ContextGraph) -> PruningDecision {
        // External nodes (3rd-party libs) are always boundaries
        if node.core().is_external {
            return PruningDecision::Boundary;
        }
        
        match node {
            Node::Type(t) => {
                // Type boundary: must be abstract (interface/protocol) + documented
                if t.is_abstract && t.core.has_doc {
                    PruningDecision::Boundary
                } else {
                    PruningDecision::Transparent
                }
            }
            Node::Function(f) => {
                // Function boundary: signature complete + documented
                let sig_complete = f.typed_param_count == f.param_count 
                                && f.has_return_type;
                if sig_complete && f.core.has_doc {
                    PruningDecision::Boundary
                } else {
                    PruningDecision::Transparent
                }
            }
            Node::Variable(_) => {
                // Variables are always transparent (traverse to their type)
                PruningDecision::Transparent
            }
        }
    }
}
```

**Policy Implementations:**

1. **`AcademicBaseline` (Fast)**:
    - **Logic**: See reference implementation above.
    - **Use Case**: Large-scale validation (BugsInPy, SWE-bench).
2. **`DeepAudit` (Slow, Future Work)**:
    - **Logic**: LLM-based evaluation of documentation quality ("Does this docstring explain the side effects?").
    - **Use Case**: CI/CD Quality Gates for enterprise projects.

### 6.4 Implementation Phases (Revised)

1. **Phase 1**: SCIP Ingestion & Graph Builder (Rust).
2. **Phase 2**: Baseline Policy (Type Hint + Doc presence check).
3. **Phase 3**: Dynamic Expansion Logic (State Index for Mutability).
4. **Phase 4**: Experiment Runner (Batch processing BugsInPy).