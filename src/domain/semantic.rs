/// Unified semantic data representation (contract between Domain and Infrastructure)
/// This is the core information extracted from SCIP Index or other data sources
/// for building ContextGraph
#[derive(Debug, Clone)]
pub struct SemanticData {
    pub project_root: String,
    pub documents: Vec<DocumentData>,
    pub external_symbols: Vec<SymbolMetadata>, // External dependencies (e.g., stdlib)
}

/// Semantic information for a single file
#[derive(Debug, Clone)]
pub struct DocumentData {
    pub relative_path: String,        // Path relative to project_root
    pub language: String,             // Programming language (e.g., "python", "java")
    pub definitions: Vec<Definition>, // Symbols defined in this file
    pub references: Vec<Reference>,   // References to other symbols
}

/// Symbol definition (corresponds to SCIP Occurrence with Definition role)
#[derive(Debug, Clone)]
pub struct Definition {
    pub symbol: String,               // SCIP symbol format (e.g., "scip python ...")
    pub range: SourceRange,           // Position of symbol name
    pub enclosing_range: SourceRange, // Full definition range (including docs, function body, etc.)
    pub metadata: SymbolMetadata,     // Symbol metadata
}

/// Symbol reference (corresponds to SCIP Occurrence without Definition role)
#[derive(Debug, Clone)]
pub struct Reference {
    pub symbol: String,           // Referenced target symbol
    pub range: SourceRange,       // Reference position
    pub enclosing_symbol: String, // Symbol in which this reference occurs
    pub role: ReferenceRole,      // Type of reference (read/write/call, etc.)
}

/// Symbol metadata (corresponds to SCIP SymbolInformation)
#[derive(Debug, Clone)]
pub struct SymbolMetadata {
    pub symbol: String,                   // SCIP symbol identifier
    pub kind: SymbolKind,                 // Symbol type (Function/Class/Variable, etc.)
    pub display_name: String,             // Display name
    pub documentation: Vec<String>,       // Documentation strings (may have multiple segments)
    pub signature: Option<String>,        // Signature (e.g., function signature)
    pub relationships: Vec<Relationship>, // Relationships with other symbols
    pub enclosing_symbol: Option<String>, // Enclosing symbol (e.g., method's class)
    pub is_external: bool,                // Whether it's an external dependency
}

/// Source code range
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRange {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// Reference role (corresponds to SCIP SymbolRole)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceRole {
    Read,      // ReadAccess
    Write,     // WriteAccess
    Call,      // Function call (needs to be inferred from context)
    Import,    // Import
    TypeUsage, // Type reference (needs to be inferred from context)
    Unknown,
}

/// Symbol type (corresponds to SCIP SymbolInformation.Kind)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    // Functions & Methods
    Function,
    Method,
    Constructor,
    StaticMethod,
    AbstractMethod,

    // Types
    Class,
    Interface,
    Struct,
    Enum,
    TypeAlias,
    Trait,    // Rust/Scala trait
    Protocol, // Swift/ObjC protocol

    // Variables & Fields
    Variable,
    Field,
    Constant,
    Parameter,

    // Namespaces
    Namespace,
    Module,
    Package,

    // Special
    Macro,
    Unknown,
}

/// Symbol relationship (corresponds to SCIP Relationship)
#[derive(Debug, Clone)]
pub struct Relationship {
    pub target_symbol: String,
    pub kind: RelationshipKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationshipKind {
    Implements,     // is_implementation
    Inherits,       // is_implementation for base class
    References,     // is_reference
    TypeDefinition, // is_type_definition
}
