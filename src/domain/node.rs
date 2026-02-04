/// Unique identifier for a node in the graph
pub type NodeId = u32;

/// Scope identifier (module/namespace)
pub type ScopeId = String;

/// Source code span
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// Shared core attributes for all nodes
#[derive(Debug, Clone)]
pub struct NodeCore {
    pub id: NodeId,
    pub name: String,
    pub scope: Option<ScopeId>,
    pub context_size: u32, // Abstract context size (computed by SizeFunction)
    pub span: SourceSpan,
    pub doc_score: f32, // Documentation quality score [0.0, 1.0]
    pub is_external: bool,
    pub file_path: String, // Path to source file (relative to project root)
}

impl NodeCore {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: NodeId,
        name: String,
        scope: Option<ScopeId>,
        context_size: u32,
        span: SourceSpan,
        doc_score: f32,
        is_external: bool,
        file_path: String,
    ) -> Self {
        Self {
            id,
            name,
            scope,
            context_size,
            span,
            doc_score,
            is_external,
            file_path,
        }
    }
}

/// Visibility level
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}

/// Function node
#[derive(Debug, Clone)]
pub struct FunctionNode {
    pub core: NodeCore,

    // Signature completeness signals
    // Behavioral signals
    pub is_async: bool,
    pub is_generator: bool,
    pub visibility: Visibility,

    // Signature - type references are stored as TypeIds (symbols)
    // The actual type information is in TypeRegistry
    pub parameters: Vec<Parameter>,
    pub return_types: Vec<String>, // TypeId (symbol) of return types
    pub throws: Vec<String>, // TypeId (symbol) of exception types
}

impl FunctionNode {
    /// Count parameters with type annotations
    pub fn typed_param_count(&self) -> usize {
        self.parameters
            .iter()
            .filter(|p| p.param_type.is_some())
            .count()
    }

    /// Total parameter count
    pub fn param_count(&self) -> usize {
        self.parameters.len()
    }

    /// Has return type annotation
    pub fn has_return_type(&self) -> bool {
        !self.return_types.is_empty()
    }

    /// Check if function signature is complete (all params typed + has return type)
    pub fn is_signature_complete(&self) -> bool {
        self.typed_param_count() == self.param_count() && self.has_return_type()
    }

    /// Get return type IDs
    pub fn return_type_ids(&self) -> &[String] {
        &self.return_types
    }
}

/// Function parameter
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    /// Type ID (symbol) of the parameter type, stored in TypeRegistry
    pub param_type: Option<String>,
    // We could add default value presence, etc.
}

/// Mutability
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mutability {
    Const,     // Compile-time constant
    Immutable, // Runtime immutable
    Mutable,   // Mutable (Expansion trigger)
}

/// Variable kind
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableKind {
    Global,     // Module-level
    ClassField, // Class/struct field
    Local,      // Local variable
}

/// Variable node
#[derive(Debug, Clone)]
pub struct VariableNode {
    pub core: NodeCore,

    // Type ID of this variable (stored in TypeRegistry)
    pub var_type: Option<String>,

    // Mutability (critical for Expansion)
    pub mutability: Mutability,

    // Scope kind
    pub variable_kind: VariableKind,
}

/// Polymorphic node type
#[derive(Debug, Clone)]
pub enum Node {
    Function(FunctionNode),
    Variable(VariableNode),
}

impl Node {
    pub fn core(&self) -> &NodeCore {
        match self {
            Node::Function(f) => &f.core,
            Node::Variable(v) => &v.core,
        }
    }

    pub fn core_mut(&mut self) -> &mut NodeCore {
        match self {
            Node::Function(f) => &mut f.core,
            Node::Variable(v) => &mut v.core,
        }
    }
}
