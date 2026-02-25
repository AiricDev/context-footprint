/// Edge kind - forward dependencies only.
/// Reverse exploration (call-in, shared-state write) is done at traversal time via incoming_edges.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// Function → Function
    Call,
    /// Function → Variable
    Read,
    /// Function → Variable
    Write,
    /// Parent method → Child method (interface implementation + concrete override)
    OverriddenBy,
    /// Decorated → Decorator (understanding decorated requires decorator)
    Annotates,
}
