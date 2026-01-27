/// Edge kind - granular classification of dependencies
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeKind {
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
