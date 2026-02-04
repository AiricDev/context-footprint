//! SemanticData - Intermediate representation of code semantics
//!
//! Responsibility: Provide language-agnostic semantic information as a contract
//! between Adapters and GraphBuilder.
//!
//! Principles:
//! 1. Only describe "what is", not "how to build the graph"
//! 2. Preserve raw reference information, let GraphBuilder infer Edge types
//! 3. Explicitly express core concepts that are common across languages
//!    (mutability, visibility, etc.)

use serde::Serialize;
use std::collections::HashMap;

/// ============================================================================
/// Top-level container
/// ============================================================================

/// Project-level semantic data
#[derive(Debug, Clone, Serialize)]
pub struct SemanticData {
    /// Project root directory
    pub project_root: String,

    /// Semantic information for all documents (files)
    pub documents: Vec<DocumentSemantics>,

    /// External symbols (stdlib/third-party)
    pub external_symbols: Vec<SymbolDefinition>,
}

/// Semantic information for a single file
#[derive(Debug, Clone, Serialize)]
pub struct DocumentSemantics {
    /// Relative path from project root
    pub relative_path: String,

    /// Programming language (e.g., "python", "rust", "java")
    pub language: String,

    /// Symbol definitions in this file
    pub definitions: Vec<SymbolDefinition>,

    /// Symbol references in this file
    pub references: Vec<SymbolReference>,
}

/// ============================================================================
/// Symbol Definition (GraphBuilder decides which become Nodes vs Types)
/// ============================================================================

/// Symbol definition - Unified representation of all definable entities
#[derive(Debug, Clone, Serialize)]
pub struct SymbolDefinition {
    /// Globally unique identifier (Adapter generates, format flexible)
    pub symbol_id: SymbolId,

    /// Symbol kind classification
    pub kind: SymbolKind,

    /// Short name (without path)
    pub name: String,

    /// Display name (may include signature, e.g., "foo(x: int) -> str")
    pub display_name: String,

    /// Definition location
    pub location: SourceLocation,

    /// Source span for context size calculation
    /// For functions, should include the entire function body
    pub span: SourceSpan,

    /// Enclosing scope (parent symbol)
    pub enclosing_symbol: Option<SymbolId>,

    /// Whether this is an external dependency
    pub is_external: bool,

    /// Documentation strings (for doc_score calculation)
    pub documentation: Vec<String>,

    /// Symbol-specific details (selected based on kind)
    pub details: SymbolDetails,
}

pub type SymbolId = String;

/// Symbol kind - Language-agnostic classification
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SymbolKind {
    // Types that may become Graph Nodes
    Function,
    Method,
    Constructor,

    Variable, // Global/module-level variables
    Field,    // Class/struct fields
    Constant,

    // Types that may become TypeRegistry entries
    Class,
    Interface,
    Struct,
    Enum,
    Trait,
    Protocol,
    TypeAlias,

    // Types that usually don't create Nodes (but info preserved for GraphBuilder)
    Parameter,     // Function parameters
    TypeParameter, // Generic parameters T

    // Others
    Module,
    Namespace,
    Package,

    Unknown,
}

/// Symbol-specific details
#[derive(Debug, Clone, Serialize)]
pub enum SymbolDetails {
    Function(FunctionDetails),
    Variable(VariableDetails),
    Type(TypeDetails),
    None,
}

impl SymbolDetails {
    pub fn as_function(&self) -> Option<&FunctionDetails> {
        match self {
            SymbolDetails::Function(f) => Some(f),
            _ => None,
        }
    }

    pub fn as_variable(&self) -> Option<&VariableDetails> {
        match self {
            SymbolDetails::Variable(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_type(&self) -> Option<&TypeDetails> {
        match self {
            SymbolDetails::Type(t) => Some(t),
            _ => None,
        }
    }
}

/// Function details
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDetails {
    /// Parameters (in definition order)
    pub parameters: Vec<ParameterInfo>,

    /// Return types (symbol IDs of type definitions)
    pub return_types: Vec<SymbolId>,

    /// Exception types that may be thrown
    pub throws: Vec<SymbolId>,

    /// Generic type parameters
    pub type_params: Vec<TypeParamInfo>,

    /// Function modifiers
    pub modifiers: FunctionModifiers,
}

impl Default for FunctionDetails {
    fn default() -> Self {
        Self {
            parameters: Vec::new(),
            return_types: Vec::new(),
            throws: Vec::new(),
            type_params: Vec::new(),
            modifiers: FunctionModifiers::default(),
        }
    }
}

/// Parameter information
#[derive(Debug, Clone, Serialize)]
pub struct ParameterInfo {
    pub name: String,
    /// Parameter type (symbol ID of type definition)
    pub param_type: Option<SymbolId>,
    pub has_default: bool,
    pub is_variadic: bool,
}

impl Default for ParameterInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            param_type: None,
            has_default: false,
            is_variadic: false,
        }
    }
}

/// Type parameter (generic) information
#[derive(Debug, Clone, Serialize)]
pub struct TypeParamInfo {
    pub name: String,
    /// Constraints (e.g., T: Clone)
    pub bounds: Vec<SymbolId>,
}

/// Function modifiers
#[derive(Debug, Clone, Serialize)]
pub struct FunctionModifiers {
    pub is_async: bool,
    pub is_generator: bool,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_constructor: bool,
    pub visibility: Visibility,
}

impl Default for FunctionModifiers {
    fn default() -> Self {
        Self {
            is_async: false,
            is_generator: false,
            is_static: false,
            is_abstract: false,
            is_constructor: false,
            visibility: Visibility::Unspecified,
        }
    }
}

/// Variable details
#[derive(Debug, Clone, Serialize)]
pub struct VariableDetails {
    /// Variable type (symbol ID of type definition)
    pub var_type: Option<SymbolId>,

    /// Mutability (key attribute! GraphBuilder uses for Expansion judgment)
    pub mutability: Mutability,

    /// Variable kind
    pub variable_kind: VariableKind,

    pub visibility: Visibility,
}

impl Default for VariableDetails {
    fn default() -> Self {
        Self {
            var_type: None,
            mutability: Mutability::Mutable,
            variable_kind: VariableKind::Global,
            visibility: Visibility::Unspecified,
        }
    }
}

/// Mutability - Used by GraphBuilder to determine SharedStateWrite Expansion
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Mutability {
    Const,     // Compile-time constant
    Immutable, // Runtime immutable (e.g., Java final, Rust let)
    Mutable,   // Mutable (triggers SharedStateWrite Expansion)
}

/// Variable kind
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum VariableKind {
    Global,     // Module-level global variables
    ClassField, // Class/struct fields
    Local,      // Local variables (GraphBuilder may ignore)
}

/// Type details
#[derive(Debug, Clone, Serialize)]
pub struct TypeDetails {
    pub kind: TypeKind,
    pub is_abstract: bool,
    pub is_final: bool,
    pub visibility: Visibility,

    /// Type parameters (generics)
    pub type_params: Vec<TypeParamInfo>,

    /// Member fields
    pub fields: Vec<FieldInfo>,

    /// Implementation/inheritance relationships (symbol IDs of other types)
    pub implements: Vec<SymbolId>,
    pub inherits: Vec<SymbolId>,
}

impl Default for TypeDetails {
    fn default() -> Self {
        Self {
            kind: TypeKind::Class,
            is_abstract: false,
            is_final: false,
            visibility: Visibility::Unspecified,
            type_params: Vec::new(),
            fields: Vec::new(),
            implements: Vec::new(),
            inherits: Vec::new(),
        }
    }
}

/// Type kind
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TypeKind {
    Class,
    Interface,
    Protocol,
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Union,        // Union types
    Intersection, // Intersection types
}

/// Field information (for class/struct fields)
#[derive(Debug, Clone, Serialize)]
pub struct FieldInfo {
    pub name: String,
    pub field_type: Option<SymbolId>,
    pub mutability: Mutability,
    pub visibility: Visibility,
}

/// Visibility
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
    Unspecified,
}

/// ============================================================================
/// Symbol Reference (Raw info, GraphBuilder infers Edge types)
/// ============================================================================

/// Symbol reference - Represents "symbol used at location"
#[derive(Debug, Clone, Serialize)]
pub struct SymbolReference {
    /// Referenced symbol
    pub target_symbol: SymbolId,

    /// Reference location (file + line/column)
    pub location: SourceLocation,

    /// Context containing this reference (function/method that contains it)
    pub enclosing_symbol: SymbolId,

    /// Reference role (Adapter reports as accurately as possible)
    pub role: ReferenceRole,

    /// Additional context (optional)
    pub context: Option<ReferenceContext>,
}

/// Reference role - Adapter reports based on language semantics
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ReferenceRole {
    // Control flow related
    Call, // Function call (GraphBuilder -> Call edge)

    // Data flow related
    Read,  // Variable read (GraphBuilder -> Read edge)
    Write, // Variable write (GraphBuilder -> Write edge)

    // Type related
    TypeAnnotation,    // Type annotation (e.g., parameter type, return type)
    TypeInstantiation, // Type instantiation (e.g., new Class())

    // Other
    Import,          // Import statement
    AttributeAccess, // Attribute access (e.g., obj.field)
    Documentation,   // Mentioned in documentation (usually ignored)
}

/// Reference context (optional additional information)
#[derive(Debug, Clone, Serialize)]
pub enum ReferenceContext {
    /// Function call context
    CallArgument {
        arg_index: usize,
        param_name: Option<String>,
    },
    /// Binary operator context
    BinaryOp { op: String },
}

/// ============================================================================
/// Common structures
/// ============================================================================

/// Source location
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub file_path: String, // Relative path
    pub line: u32,         // 0-based
    pub column: u32,       // 0-based
}

/// Source span
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceSpan {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// ============================================================================
/// Helper functions
/// ============================================================================

/// Check if a symbol kind typically becomes a graph node
pub fn is_node_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Constructor
            | SymbolKind::Variable
            | SymbolKind::Field
            | SymbolKind::Constant
    )
}

/// Check if a symbol kind typically becomes a type registry entry
pub fn is_type_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Protocol
            | SymbolKind::TypeAlias
    )
}

/// Check if a symbol kind should be skipped (not node, not type)
pub fn should_skip_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Parameter | SymbolKind::TypeParameter | SymbolKind::Unknown
    ) || matches!(
        kind,
        SymbolKind::Module | SymbolKind::Namespace | SymbolKind::Package
    )
}

/// Get all symbol definitions from all documents
pub fn collect_all_definitions(data: &SemanticData) -> Vec<&SymbolDefinition> {
    let mut result: Vec<&SymbolDefinition> = Vec::new();

    // Collect from documents
    for doc in &data.documents {
        for def in &doc.definitions {
            result.push(def);
        }
    }

    // Include external symbols
    for ext in &data.external_symbols {
        result.push(ext);
    }

    result
}

/// Build a map from symbol_id to its enclosing symbol_id
pub fn build_enclosing_map(data: &SemanticData) -> HashMap<SymbolId, SymbolId> {
    let mut map = HashMap::new();

    for doc in &data.documents {
        for def in &doc.definitions {
            if let Some(ref parent) = def.enclosing_symbol {
                map.insert(def.symbol_id.clone(), parent.clone());
            }
        }
    }

    map
}

/// Resolve a symbol to the nearest ancestor that is a node
pub fn resolve_to_node_symbol(
    symbol: &str,
    node_symbols: &std::collections::HashSet<SymbolId>,
    enclosing_map: &HashMap<SymbolId, SymbolId>,
) -> Option<SymbolId> {
    let mut current = symbol.to_string();

    loop {
        if node_symbols.contains(&current) {
            return Some(current);
        }

        match enclosing_map.get(&current) {
            Some(parent) => current = parent.clone(),
            None => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_definition(symbol_id: &str, kind: SymbolKind) -> SymbolDefinition {
        SymbolDefinition {
            symbol_id: symbol_id.to_string(),
            kind,
            name: symbol_id.to_string(),
            display_name: symbol_id.to_string(),
            location: SourceLocation {
                file_path: "test.py".to_string(),
                line: 0,
                column: 0,
            },
            span: SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 10,
                end_column: 0,
            },
            enclosing_symbol: None,
            is_external: false,
            documentation: vec![],
            details: SymbolDetails::None,
        }
    }

    #[test]
    fn test_is_node_kind() {
        assert!(is_node_kind(&SymbolKind::Function));
        assert!(is_node_kind(&SymbolKind::Method));
        assert!(is_node_kind(&SymbolKind::Variable));
        assert!(!is_node_kind(&SymbolKind::Class));
        assert!(!is_node_kind(&SymbolKind::Parameter));
    }

    #[test]
    fn test_is_type_kind() {
        assert!(is_type_kind(&SymbolKind::Class));
        assert!(is_type_kind(&SymbolKind::Interface));
        assert!(!is_type_kind(&SymbolKind::Function));
        assert!(!is_type_kind(&SymbolKind::Variable));
    }

    #[test]
    fn test_resolve_to_node_symbol() {
        let mut node_symbols = std::collections::HashSet::new();
        node_symbols.insert("pkg::func()".to_string());

        let mut enclosing_map = HashMap::new();
        enclosing_map.insert("pkg::func().local".to_string(), "pkg::func()".to_string());

        // Direct node
        assert_eq!(
            resolve_to_node_symbol("pkg::func()", &node_symbols, &enclosing_map),
            Some("pkg::func()".to_string())
        );

        // Child of node
        assert_eq!(
            resolve_to_node_symbol("pkg::func().local", &node_symbols, &enclosing_map),
            Some("pkg::func()".to_string())
        );

        // Unknown symbol
        assert_eq!(
            resolve_to_node_symbol("unknown", &node_symbols, &enclosing_map),
            None
        );
    }
}
