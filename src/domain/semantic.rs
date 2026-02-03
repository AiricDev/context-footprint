//! Unified semantic data representation: contract between SCIP (or other indexers) and the
//! Context Footprint (CF) algorithm.
//!
//! **SCIP mapping**: Types here correspond to SCIP Index → Document → Occurrence /
//! SymbolInformation. **CF usage**: scope = `enclosing_symbol`, code span = `range` (symbol name)
//! and `enclosing_range` (full definition, used for token count), documentation = `metadata.documentation`.

use std::collections::HashMap;

use serde::Serialize;

/// Unified semantic data: core information extracted from a SCIP Index (or other source) for
/// building the ContextGraph.
///
/// **SCIP**: Corresponds to `Index`; `project_root` from `Metadata.project_root` (often
/// `file://` — normalize in adapter); `documents` = per-file definitions + references;
/// `external_symbols` = `Index.external_symbols`.
#[derive(Debug, Clone)]
#[derive(Serialize)]
pub struct SemanticData {
    /// Project root path (adapter normalizes `file://` prefix from SCIP Metadata.project_root).
    pub project_root: String,
    /// Per-document definitions and references (from SCIP Document.occurrences partitioned by role).
    pub documents: Vec<DocumentData>,
    /// External dependencies (e.g. stdlib); from SCIP Index.external_symbols.
    pub external_symbols: Vec<SymbolMetadata>,
    /// Pre-aggregated index: symbol → kind/parent, function → parameters. Built during
    /// SemanticData construction so the builder can avoid a full scan. Used for node/type
    /// classification and function parameter attachment.
    pub symbol_index: SymbolIndex,
}

/// Pre-aggregated index over all definitions: used by the CF builder for node/type classification
/// and function parameter attachment without rescanning definitions.
///
/// **SCIP source**: Built from Document.occurrences where symbol_roles has Definition; kind and
/// enclosing_symbol come from SymbolInformation (or inferred). **CF use**: Pass 1 uses
/// `symbol_kind`/`symbol_parent` to decide GraphNode vs TypeOnly vs Skip; parameters come from
/// `function_parameters`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SymbolIndex {
    /// Map: symbol → SymbolKind. Used to classify definitions (node vs type vs parameter) and
    /// to resolve parent kind for variable/field scope checks.
    pub symbol_kind: HashMap<String, SymbolKind>,
    /// Map: symbol → enclosing symbol (scope). From SymbolInformation.enclosing_symbol. Used by
    /// builder to resolve reference.enclosing_symbol and definition parent chain to a graph node.
    pub symbol_parent: HashMap<String, String>,
    /// Map: function symbol → ordered parameters. Built from definitions with kind=Parameter
    /// (parent = function); param_type from TypeDefinition relationship. Order = definition order.
    pub function_parameters: HashMap<String, Vec<Parameter>>,
}

impl SymbolIndex {
    /// Build a SymbolIndex from all definitions across documents (e.g. in adapter or tests).
    /// - symbol_kind / symbol_parent: from every definition.
    /// - function_parameters: from (1) function definition's metadata.parameters (adapter merges
    ///   Parameter definitions into metadata.parameters), or (2) Parameter definitions in the list
    ///   when present (e.g. in tests). metadata.parameters takes precedence when both exist.
    pub fn from_definitions(documents: &[DocumentData]) -> Self {
        let mut symbol_kind = HashMap::new();
        let mut symbol_parent = HashMap::new();
        let mut function_parameters: HashMap<String, Vec<Parameter>> = HashMap::new();

        for doc in documents {
            for def in &doc.definitions {
                symbol_kind.insert(def.symbol.clone(), def.metadata.kind.clone());
                if let Some(ref parent) = def.metadata.enclosing_symbol {
                    symbol_parent.insert(def.symbol.clone(), parent.clone());
                }
                // From Parameter definitions (e.g. tests that still include them)
                if matches!(def.metadata.kind, SymbolKind::Parameter)
                    && let Some(ref parent) = def.metadata.enclosing_symbol
                {
                    let mut param_type = None;
                    for rel in &def.metadata.relationships {
                        if matches!(rel.kind, RelationshipKind::TypeDefinition) {
                            param_type = Some(rel.target_symbol.clone());
                            break;
                        }
                    }
                    let param = Parameter {
                        name: def.metadata.display_name.clone(),
                        param_type,
                    };
                    function_parameters
                        .entry(parent.clone())
                        .or_default()
                        .push(param);
                }
            }
            // From function metadata.parameters (adapter output; takes precedence)
            for def in &doc.definitions {
                if matches!(
                    def.metadata.kind,
                    SymbolKind::Function
                        | SymbolKind::Method
                        | SymbolKind::Constructor
                        | SymbolKind::StaticMethod
                        | SymbolKind::AbstractMethod
                ) && !def.metadata.parameters.is_empty()
                {
                    function_parameters.insert(def.symbol.clone(), def.metadata.parameters.clone());
                }
            }
        }

        Self {
            symbol_kind,
            symbol_parent,
            function_parameters,
        }
    }
}

/// Classification of a definition for the CF graph: whether it becomes a graph node, is only
/// registered as a type, is an inline parameter, or is skipped.
///
/// **CF use**: Builder uses this to decide create node vs register type vs skip; avoids
/// duplicating kind/parent logic in the builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionRole {
    /// Becomes a ContextGraph node (functions; variables/fields/constants not enclosed by a function).
    GraphNode,
    /// Registered in TypeRegistry only, no graph node (Class, Interface, Struct, Enum, etc.).
    TypeOnly,
    /// Inline parameter of a function; not a standalone node; parameters attached to function via SymbolIndex.
    InlineParameter,
    /// Not a node and not a type (Module, Namespace, Macro, etc.).
    Skip,
}

/// Classifies a definition into GraphNode, TypeOnly, InlineParameter, or Skip using only
/// SymbolKind and SymbolIndex (no SizeFunction/DocumentationScorer). Used by the builder in Pass 1.
pub fn definition_role(definition: &Definition, index: &SymbolIndex) -> DefinitionRole {
    let kind = &definition.metadata.kind;

    if matches!(kind, SymbolKind::Parameter) {
        return DefinitionRole::InlineParameter;
    }

    match kind {
        SymbolKind::Function
        | SymbolKind::Method
        | SymbolKind::Constructor
        | SymbolKind::StaticMethod
        | SymbolKind::AbstractMethod => DefinitionRole::GraphNode,

        SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::TypeAlias
        | SymbolKind::Trait
        | SymbolKind::Protocol => DefinitionRole::TypeOnly,

        SymbolKind::Variable | SymbolKind::Field | SymbolKind::Constant => {
            let parent_kind = definition
                .metadata
                .enclosing_symbol
                .as_ref()
                .and_then(|p| index.symbol_kind.get(p));
            match parent_kind {
                None => DefinitionRole::GraphNode,
                Some(SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
                | SymbolKind::StaticMethod | SymbolKind::AbstractMethod) => DefinitionRole::Skip,
                _ => DefinitionRole::GraphNode,
            }
        }

        _ => DefinitionRole::Skip,
    }
}

/// Semantic information for a single source file.
///
/// **SCIP**: Corresponds to `Document`; `relative_path`, `language`; definitions and references
/// come from partitioning Occurrences by Definition role.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentData {
    /// Path relative to project_root (SCIP Document.relative_path).
    pub relative_path: String,
    /// Programming language id (e.g. "python", "java") (SCIP Document.language).
    pub language: String,
    /// Symbols defined in this file (Occurrence with Definition role).
    pub definitions: Vec<Definition>,
    /// References to other symbols (Occurrence without Definition role).
    pub references: Vec<Reference>,
}

/// A symbol definition: one occurrence with the Definition role.
///
/// **SCIP**: Occurrence where symbol_roles has Definition; `range` = Occurrence.range (symbol name
/// span), `enclosing_range` = Occurrence.enclosing_range (full definition including docs/body),
/// used by CF for context_size; `metadata` from SymbolInformation plus adapter enrichment.
#[derive(Debug, Clone, Serialize)]
pub struct Definition {
    /// SCIP symbol string (e.g. "scip python pkg ...").
    pub symbol: String,
    /// Position of the symbol name (SCIP Occurrence.range).
    pub range: SourceRange,
    /// Full definition range including documentation and body (SCIP Occurrence.enclosing_range).
    pub enclosing_range: SourceRange,
    /// Symbol metadata (from SymbolInformation + adapter).
    pub metadata: SymbolMetadata,
}

/// A reference to a symbol (non-definition occurrence).
///
/// **SCIP**: Occurrence without Definition role. `enclosing_symbol` is used in Pass 2 to resolve
/// the source node of edges (via parent chain to a graph node).
#[derive(Debug, Clone, Serialize)]
pub struct Reference {
    /// Referenced symbol (Occurrence.symbol).
    pub symbol: String,
    /// Reference position (Occurrence.range).
    pub range: SourceRange,
    /// Symbol containing this reference (used to resolve source node in builder Pass 2).
    pub enclosing_symbol: String,
    /// Role: Read/Write/Call/Import/TypeUsage (from SCIP SymbolRole bitset; Call/TypeUsage often inferred).
    pub role: ReferenceRole,
}

/// Single parameter in a function signature (language-agnostic).
///
/// **CF**: param_type is a type symbol ID for TypeRegistry/ParamType edges.
#[derive(Debug, Clone, Serialize)]
pub struct Parameter {
    pub name: String,
    /// Type symbol ID (from signature parsing or TypeDefinition relationship).
    pub param_type: Option<String>,
}

/// Symbol metadata: corresponds to SCIP SymbolInformation plus adapter-filled fields.
///
/// **SCIP**: symbol, kind, display_name, documentation, relationships, enclosing_symbol;
/// signature from signature_documentation. **Adapter**: parameters and return_type from
/// signature or relationships; **scope** in CF = enclosing_symbol.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolMetadata {
    /// SCIP symbol identifier.
    pub symbol: String,
    /// Symbol kind (SCIP SymbolInformation.Kind; Unknown may be refined from symbol string in adapter).
    pub kind: SymbolKind,
    /// Display name (SCIP display_name).
    pub display_name: String,
    /// Documentation strings (SCIP documentation; CF uses for doc_score).
    pub documentation: Vec<String>,
    /// Raw signature from indexer (SCIP signature_documentation).
    pub signature: Option<String>,
    /// Parsed parameters (adapter from signature or from Parameter definitions).
    pub parameters: Vec<Parameter>,
    /// Return type symbol (adapter from signature or relationships).
    pub return_type: Option<String>,
    /// Relationships to other symbols (SCIP Relationship: TypeDefinition, Implements, etc.).
    pub relationships: Vec<Relationship>,
    /// Enclosing symbol = scope in CF (SCIP enclosing_symbol).
    pub enclosing_symbol: Option<String>,
    /// True if from Index.external_symbols.
    pub is_external: bool,
    /// Exception/throws type symbols (optional; SCIP rarely provides; adapter may infer from signature/source).
    #[allow(dead_code)]
    pub throws: Vec<String>,
}

/// Source code range (line/column).
///
/// **SCIP**: Same encoding as Occurrence.range and enclosing_range (0-based). Convert to 1-based
/// in UI if needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRange {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// Reference role: maps from SCIP SymbolRole bitset (ReadAccess, WriteAccess, Import, etc.).
/// Call and TypeUsage are often inferred when not explicitly marked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ReferenceRole {
    Read,
    Write,
    Call,
    Import,
    TypeUsage,
    Unknown,
}

/// Symbol kind: maps from SCIP SymbolInformation.Kind. When kind is Unknown, adapter may infer
/// from symbol string (e.g. infer_kind_from_symbol using descriptor suffixes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SymbolKind {
    Function,
    Method,
    Constructor,
    StaticMethod,
    AbstractMethod,
    Class,
    Interface,
    Struct,
    Enum,
    TypeAlias,
    Trait,
    Protocol,
    Variable,
    Field,
    Constant,
    Parameter,
    Namespace,
    Module,
    Package,
    Macro,
    Unknown,
}

/// Relationship to another symbol: maps from SCIP Relationship (is_type_definition,
/// is_implementation, is_reference, etc.).
#[derive(Debug, Clone, Serialize)]
pub struct Relationship {
    pub target_symbol: String,
    pub kind: RelationshipKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Serialize)]
pub enum RelationshipKind {
    Implements,
    Inherits,
    References,
    TypeDefinition,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(symbol: &str, kind: SymbolKind, enclosing: Option<&str>) -> Definition {
        Definition {
            symbol: symbol.to_string(),
            range: SourceRange {
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 10,
            },
            enclosing_range: SourceRange {
                start_line: 0,
                start_column: 0,
                end_line: 5,
                end_column: 20,
            },
            metadata: SymbolMetadata {
                symbol: symbol.to_string(),
                kind,
                display_name: symbol.to_string(),
                documentation: vec![],
                signature: None,
                parameters: vec![],
                return_type: None,
                relationships: vec![],
                enclosing_symbol: enclosing.map(String::from),
                is_external: false,
                throws: vec![],
            },
        }
    }

    #[test]
    fn definition_role_function_is_graph_node() {
        let idx = SymbolIndex::default();
        let d = def("pkg::foo().", SymbolKind::Function, None);
        assert_eq!(definition_role(&d, &idx), DefinitionRole::GraphNode);
    }

    #[test]
    fn definition_role_class_is_type_only() {
        let idx = SymbolIndex::default();
        let d = def("pkg::Bar#", SymbolKind::Class, None);
        assert_eq!(definition_role(&d, &idx), DefinitionRole::TypeOnly);
    }

    #[test]
    fn definition_role_parameter_is_inline_parameter() {
        let idx = SymbolIndex::default();
        let d = def("pkg::foo().(x)", SymbolKind::Parameter, Some("pkg::foo()."));
        assert_eq!(definition_role(&d, &idx), DefinitionRole::InlineParameter);
    }

    #[test]
    fn definition_role_module_is_skip() {
        let idx = SymbolIndex::default();
        let d = def("pkg/", SymbolKind::Module, None);
        assert_eq!(definition_role(&d, &idx), DefinitionRole::Skip);
    }

    #[test]
    fn definition_role_variable_under_function_is_skip() {
        let mut idx = SymbolIndex::default();
        idx.symbol_kind.insert("pkg::func().".into(), SymbolKind::Function);
        let d = def("pkg::func().(local)", SymbolKind::Variable, Some("pkg::func()."));
        assert_eq!(definition_role(&d, &idx), DefinitionRole::Skip);
    }

    #[test]
    fn definition_role_variable_no_parent_is_graph_node() {
        let idx = SymbolIndex::default();
        let d = def("pkg::global.", SymbolKind::Variable, None);
        assert_eq!(definition_role(&d, &idx), DefinitionRole::GraphNode);
    }

    #[test]
    fn symbol_index_from_definitions_collects_kind_and_parameters() {
        let docs = vec![DocumentData {
            relative_path: "a.py".into(),
            language: "python".into(),
            definitions: vec![
                def("pkg::f().", SymbolKind::Function, None),
                def("pkg::f().(x)", SymbolKind::Parameter, Some("pkg::f().")),
            ],
            references: vec![],
        }];
        let idx = SymbolIndex::from_definitions(&docs);
        assert_eq!(idx.symbol_kind.get("pkg::f()."), Some(&SymbolKind::Function));
        assert_eq!(idx.function_parameters.get("pkg::f().").map(|v| v.len()), Some(1));
    }
}
