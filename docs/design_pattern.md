# Design Patterns and Context Footprint (CF)

This document summarizes how the Context Footprint (CF) algorithm interacts with standard software design patterns. It demonstrates that "good" design patterns—those that promote encapsulation, decoupling, and abstraction—consistently result in a lower CF score (reduced cognitive load), while anti-patterns result in a higher score.

The CF algorithm uses two primary mechanisms to reflect code quality:
1.  **Semantic Boundaries**: Well-documented and strongly-typed interfaces stop graph traversal, hiding implementation details.
2.  **Graph Topology**: Patterns that simplify dependency graphs (e.g., Star vs. Mesh) naturally reduce the reachable node count.

## Verified Patterns

We have empirically verified the following patterns using integration tests (`tests/domain_patterns_test.rs`).

### 1. Strategy Pattern
**Mechanism**: Encapsulation via Interface.
*   **Scenario**: A `Client` uses a `Context`, which depends on an `IStrategy` interface.
*   **CF Behavior**: Traversal stops at the `IStrategy` interface (Boundary).
*   **Impact**: The `ConcreteStrategy` implementation is **excluded** from the Client's footprint.
*   **Score**: **Lower**. The Client is isolated from the complexity of specific algorithms.

### 2. Observer Pattern
**Mechanism**: Inversion of Control.
*   **Scenario**: A `Subject` notifies a list of `IObserver`s.
*   **CF Behavior**: Traversal stops at the `IObserver` interface.
*   **Impact**: The `Subject` does not "see" the `ConcreteObserver` classes or their complex update logic.
*   **Score**: **Lower**. Adding new observers does not increase the cognitive load of the Subject.

### 3. Facade Pattern
**Mechanism**: Simplified Interface.
*   **Scenario**: A `Client` interacts with a well-documented `Facade` to perform complex tasks involving multiple subsystems.
*   **CF Behavior**: Traversal stops at the `Facade` boundary.
*   **Impact**: The complex subgraph of `SubsystemA`, `SubsystemB`, etc., is pruned.
*   **Score**: **Lower**. Compare to a "Transparent Facade" (no docs/types) or direct access, where the entire subsystem subgraph is included.

### 4. Template Method Pattern
**Mechanism**: Abstract Class Boundary.
*   **Scenario**: A `Client` calls a `template_method()` on an Abstract Class.
*   **CF Behavior**: Traversal stops at the Abstract Class.
*   **Impact**: The `primitive_operation()` implementations in concrete subclasses are excluded.
*   **Score**: **Lower**. The Client relies on the stable skeleton, not the variable details.

### 5. Adapter Pattern
**Mechanism**: Interface Adaptation / Wrapper.
*   **Scenario**: A `Client` uses a `Target` interface implemented by an `Adapter`, which wraps an `Adaptee`.
*   **CF Behavior**: Traversal stops at the `Adapter`'s public interface.
*   **Impact**: The `Adaptee` (legacy/external code) is effectively hidden.
*   **Score**: **Lower**. The legacy complexity does not leak into the Client's context.

### 6. Mediator Pattern
**Mechanism**: Topology Optimization (Mesh to Star).
*   **Scenario**: `Colleague` objects communicate via a central `Mediator` instead of calling each other directly.
*   **CF Behavior**: 
    *   *Spaghetti (Mesh)*: `A` -> `B`, `C`, `D`. Context includes all colleagues.
    *   *Mediator (Star)*: `A` -> `Mediator`. Traversal stops at Mediator (if boundary).
*   **Impact**: Disconnects the $N \times N$ dependency graph.
*   **Score**: **Significantly Lower**. Reduces local complexity from $O(N)$ to $O(1)$.

## Verified Principles & Anti-Patterns

### 1. Law of Demeter (LoD)
**Principle**: "Don't talk to strangers."
*   **Violation (Train Wreck)**: `a.getB().getC().doAction()`.
    *   **CF Result**: High. The context includes `A`, `B`, and `C` because the client explicitly depends on all of them.
*   **Compliance (Encapsulation)**: `a.doWrapper()`.
    *   **CF Result**: Low. The context includes only `A`. `B` and `C` are hidden details of `A`.

### 2. Interface Segregation Principle (ISP)
**Principle**: "Clients should not be forced to depend on interfaces they do not use."
*   **Violation (Fat Interface)**: Client depends on a "Utils" module that imports the world.
    *   **CF Result**: High. If the "Utils" module is transparent (leaky), the Client inherits all its transitive dependencies.
*   **Compliance (Segregated)**: Client depends on a specific, lean interface.
    *   **CF Result**: Low. Only relevant dependencies are included.

### 3. Global Mutable State (Singleton)
**Anti-Pattern**: Hidden coupling via shared state.
*   **Scenario**: Client reads a Singleton; various Writers mutate it.
*   **CF Behavior**: The algorithm detects `SharedStateWrite` edges. If a Client reads a mutable variable, its context is expanded to include **all writers** to that variable.
*   **Impact**: Massive expansion of the footprint.
*   **Score**: **Penalized (High)**. This accurately reflects the high cognitive load of debugging global state (you must know everyone who changes it).

## Summary

The Context Footprint metric aligns with established software engineering wisdom. It provides a quantitative backing to qualitative design advice:

| Design Choice | CF Impact | Reason |
| :--- | :--- | :--- |
| **Program to Interface** | 📉 Reduces | Interfaces act as pruning boundaries. |
| **High Cohesion** | 📉 Reduces | Related code stays together; unrelated code is pruned. |
| **Loose Coupling** | 📉 Reduces | Fewer edges means smaller reachability sets. |
| **God Classes / Utils** | 📈 Increases | Transparent nodes propagate dependencies. |
| **Global State** | 📈 Increases | Implicit dependencies become explicit penalties. |
| **Deep Inheritance** | 📉 Reduces | If base classes are boundaries, hierarchy details are hidden. |

By optimizing for a lower Context Footprint, developers are naturally guided toward these proven architectural patterns.
