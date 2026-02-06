/// Edge kind - granular classification of dependencies
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    // ============ Control Flow ============
    Call, // Function → Function

    // ============ Type Hierarchy (within TypeRegistry, not graph edges) ============
    // Type relationships like Inherits/Implements are stored in TypeRegistry,
    // not as graph edges, since types are no longer graph nodes.

    // ============ Data Flow (Expansion triggers) ============
    Read,  // Function → Variable
    Write, // Function → Variable

    // ============ Dynamic Expansion (Reverse Dependencies) ============
    SharedStateWrite, // Reader(Function) → Writer(Function) of shared mutable state
    CallIn,           // Callee(Function) → Caller(Function) for underspecified functions

    // ============ Annotations & Decorators ============
    /// Decorated → Decorator direction (understanding decorated requires decorator)
    Annotates,
}
