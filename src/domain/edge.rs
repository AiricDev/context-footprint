/// Edge kind - granular classification of dependencies
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    // ============ Control Flow ============
    Call, // Function → Function

    // ============ Type Usage (granular) ============
    ParamType,  // Function → Variable (TypeDef) (parameter type dependency)
    ReturnType, // Function → Variable (TypeDef) (return type dependency)
    // FieldType removed: Use VariableType (Variable(Field) → Variable(TypeDef))
    VariableType, // Variable → Variable (TypeDef) (declared type)
    GenericBound, // Variable (TypeDef) → Variable (TypeDef) (e.g., T: Comparable)
    TypeArgument, // Usage → Variable (TypeDef) (generic instantiation)

    // ============ Type Hierarchy ============
    // ============ Type Hierarchy ============
    Inherits,   // Variable (TypeDef) → Variable (TypeDef) (class extends)
    Implements, // Variable (TypeDef) → Variable (TypeDef) (class implements interface)

    // ============ Data Flow (Expansion triggers) ============
    Read,  // Function → Variable
    Write, // Function → Variable

    // ============ Dynamic Expansion (Reverse Dependencies) ============
    SharedStateWrite, // Reader → Writer (of shared mutable state)
    CallIn,           // Callee → Caller (for underspecified functions)

    // ============ Annotations & Decorators ============
    /// Decorated → Decorator direction (understanding decorated requires decorator)
    Annotates {
        is_behavioral: bool, // true = decorator (strong dep), false = metadata (weak dep)
    },

    // ============ Exception Flow ============
    // ============ Exception Flow ============
    Throws, // Function → Variable (TypeDef) (exception type)
}
