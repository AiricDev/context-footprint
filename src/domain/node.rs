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
    pub param_count: u32,
    pub typed_param_count: u32,
    pub has_return_type: bool,

    // Behavioral signals
    pub is_async: bool,
    pub is_generator: bool,
    pub visibility: Visibility,
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
}

/// Variable node
#[derive(Debug, Clone)]
pub struct VariableNode {
    pub core: NodeCore,

    // Type annotation
    pub has_type_annotation: bool,

    // Mutability (critical for Expansion)
    pub mutability: Mutability,

    // Scope kind
    pub variable_kind: VariableKind,
}

/// Type kind
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    Class,
    Interface, // Java, Go, TypeScript
    Protocol,  // Python, Swift
    Struct,
    Enum,
    TypeAlias,    // type UserId = string
    FunctionType, // (int, int) -> bool
    Union,        // A | B
    Intersection, // A & B
}

/// Type node
#[derive(Debug, Clone)]
pub struct TypeNode {
    pub core: NodeCore,

    // Type classification
    pub type_kind: TypeKind,

    // Abstraction signal (Pruning key)
    pub is_abstract: bool, // interface, protocol, abstract class

    // Generics
    pub type_param_count: u32, // Generic parameters (e.g., List<T> → 1)
}

/// Polymorphic node type
#[derive(Debug, Clone)]
pub enum Node {
    Function(FunctionNode),
    Variable(VariableNode),
    Type(TypeNode),
}

impl Node {
    pub fn core(&self) -> &NodeCore {
        match self {
            Node::Function(f) => &f.core,
            Node::Variable(v) => &v.core,
            Node::Type(t) => &t.core,
        }
    }

    pub fn core_mut(&mut self) -> &mut NodeCore {
        match self {
            Node::Function(f) => &mut f.core,
            Node::Variable(v) => &mut v.core,
            Node::Type(t) => &mut t.core,
        }
    }
}
