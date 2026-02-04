use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;
use crate::domain::node::{
    FunctionNode, Mutability as NodeMutability, Node, NodeCore, SourceSpan, VariableKind,
    VariableNode, Visibility as NodeVisibility,
};
use crate::domain::policy::{DocumentationScorer, NodeInfo, NodeType, SizeFunction};
use crate::domain::ports::SourceReader;
use crate::domain::semantic::{
    Mutability, ReferenceRole, SemanticData, SourceSpan as SemanticSpan, SymbolDefinition,
    SymbolDetails, SymbolId, SymbolKind, VariableScope as SemanticVarScope, Visibility,
};
use crate::domain::type_registry::{TypeDefAttribute, TypeInfo, TypeKind, TypeRegistry};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Graph builder - Domain Service for constructing ContextGraph
pub struct GraphBuilder {
    size_function: Box<dyn SizeFunction>,
    doc_scorer: Box<dyn DocumentationScorer>,
}

impl GraphBuilder {
    pub fn new(
        size_function: Box<dyn SizeFunction>,
        doc_scorer: Box<dyn DocumentationScorer>,
    ) -> Self {
        Self {
            size_function,
            doc_scorer,
        }
    }

    /// Three-pass build strategy
    pub fn build(
        &self,
        semantic_data: SemanticData,
        source_reader: &dyn SourceReader,
    ) -> Result<ContextGraph> {
        let mut graph = ContextGraph::new();
        let mut type_registry = TypeRegistry::new();

        // Pre-compute enclosing map for symbol resolution
        let enclosing_map = semantic_data.build_enclosing_map();

        // Collect all node candidate symbols
        let mut node_symbols: HashSet<SymbolId> = HashSet::new();

        // Pass 1: Node Allocation - Create FunctionNode/VariableNode and TypeRegistry entries
        for document in &semantic_data.documents {
            let source_path = Path::new(&semantic_data.project_root).join(&document.relative_path);
            let source_code = source_reader.read(&source_path)?;

            for def in &document.definitions {
                let node_id = graph.graph.node_count() as u32;
                let doc_texts = def.documentation.clone();
                let span = convert_span(&def.span);

                // Check if this is an interface/abstract method
                // Now we check is_abstract directly from FunctionModifiers
                let is_interface_method = if def.kind == SymbolKind::Function {
                    if let Some(func_details) = def.as_function() {
                        func_details.modifiers.is_abstract
                    } else {
                        false
                    }
                } else {
                    false
                };

                // For interface methods, only compute context_size for signature (not implementation body)
                let context_size = if is_interface_method {
                    let signature_span = extract_signature_span(&def.span, &source_code);
                    self.size_function
                        .compute(&source_code, &signature_span, &doc_texts)
                } else {
                    self.size_function.compute(
                        &source_code,
                        &convert_span_for_size(&def.span),
                        &doc_texts,
                    )
                };

                let doc_text = doc_texts.first().map(|s| s.as_str());

                let language = document
                    .relative_path
                    .split('.')
                    .next_back()
                    .map(|ext| ext.to_lowercase());

                let node_info = NodeInfo {
                    node_type: infer_node_type_from_kind(&def.kind),
                    name: def.name.clone(),
                    signature: extract_signature(def),
                    language,
                };
                let doc_score = self.doc_scorer.score(&node_info, doc_text);

                match def.kind {
                    SymbolKind::Type => {
                        // Register in TypeRegistry
                        let type_info = create_type_info(def, context_size, doc_score);
                        type_registry.register(def.symbol_id.clone(), type_info);
                    }
                    SymbolKind::Function | SymbolKind::Variable => {
                        // Create graph node
                        node_symbols.insert(def.symbol_id.clone());

                        let core = NodeCore::new(
                            node_id,
                            def.name.clone(),
                            def.enclosing_symbol.clone(),
                            context_size,
                            span,
                            doc_score,
                            def.is_external,
                            document.relative_path.clone(),
                        );

                        let node = create_node_from_definition(core, def, is_interface_method)?;
                        graph.add_node(def.symbol_id.clone(), node);
                    }
                }
            }
        }

        // Pass 2: Edge Wiring - Process references to create edges
        let mut state_writers: HashMap<SymbolId, Vec<petgraph::graph::NodeIndex>> = HashMap::new();
        let mut callers: HashMap<SymbolId, Vec<petgraph::graph::NodeIndex>> = HashMap::new();
        let mut readers: Vec<(petgraph::graph::NodeIndex, SymbolId)> = Vec::new();

        for document in &semantic_data.documents {
            for reference in &document.references {
                // Resolve source symbol to nearest node
                let source_node_sym = Self::resolve_to_node_symbol(
                    &reference.enclosing_symbol,
                    &node_symbols,
                    &enclosing_map,
                );

                // Resolve target symbol to nearest node or type
                let target_node_sym =
                    Self::resolve_to_node_symbol(&reference.target_symbol, &node_symbols, &enclosing_map);

                if let Some(source_sym) = source_node_sym {
                    let source_idx = match graph.get_node_by_symbol(&source_sym) {
                        Some(idx) => idx,
                        None => continue,
                    };

                    // Handle Call edges
                    if reference.role == ReferenceRole::Call
                        && let Some(target_sym) = &target_node_sym
                        && let Some(target_idx) = graph.get_node_by_symbol(target_sym)
                        && source_idx != target_idx
                    {
                        graph.add_edge(source_idx, target_idx, EdgeKind::Call);
                        callers
                            .entry(reference.target_symbol.clone())
                            .or_default()
                            .push(source_idx);
                    }

                    // Handle Read/Write edges for variable references
                    if matches!(reference.role, ReferenceRole::Read | ReferenceRole::Write) {
                        // Target might be a variable (node) or a type (in type_registry)
                        if let Some(target_sym) = &target_node_sym
                            && let Some(target_idx) = graph.get_node_by_symbol(target_sym)
                            && source_idx != target_idx
                        {
                            let edge_kind = if reference.role == ReferenceRole::Write {
                                EdgeKind::Write
                            } else {
                                EdgeKind::Read
                            };
                            graph.add_edge(source_idx, target_idx, edge_kind);

                            if reference.role == ReferenceRole::Write {
                                state_writers
                                    .entry(reference.target_symbol.clone())
                                    .or_default()
                                    .push(source_idx);
                            } else {
                                readers.push((source_idx, reference.target_symbol.clone()));
                            }
                        }
                    }
                }
            }
        }

        // Pass 2.5: Fill in type references in nodes from SymbolDetails
        for document in &semantic_data.documents {
            for def in &document.definitions {
                if let Some(node_idx) = graph.get_node_by_symbol(&def.symbol_id) {
                    match &def.details {
                        SymbolDetails::Function(func_details) => {
                            if let Some(Node::Function(func_node)) =
                                graph.graph.node_weight_mut(node_idx)
                            {
                                // Set return types
                                for type_id in &func_details.return_types {
                                    if type_registry.contains(type_id) {
                                        func_node.return_types.push(type_id.clone());
                                    }
                                }
                                // Note: Parameters are already set in create_node_from_definition
                            }
                        }
                        SymbolDetails::Variable(var_details) => {
                            if let Some(Node::Variable(var_node)) =
                                graph.graph.node_weight_mut(node_idx)
                                && let Some(ref type_id) = var_details.var_type
                                && type_registry.contains(type_id)
                            {
                                var_node.var_type = Some(type_id.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Pass 3: Dynamic Expansion Edges
        // 1. SharedStateWrite edges: Reader -> Writer
        for (reader_idx, var_symbol) in readers {
            // Check if variable is mutable
            let is_mutable = self.is_variable_mutable(&var_symbol, &semantic_data);

            if is_mutable && let Some(writers) = state_writers.get(&var_symbol) {
                for &writer_idx in writers {
                    if reader_idx != writer_idx {
                        graph.add_edge(reader_idx, writer_idx, EdgeKind::SharedStateWrite);
                    }
                }
            }
        }

        // 2. CallIn edges: Callee -> Caller for underspecified functions
        let all_node_symbols: Vec<SymbolId> = graph.symbol_to_node.keys().cloned().collect();
        for callee_symbol in all_node_symbols {
            if let (Some(callee_idx), Some(caller_indices)) = (
                graph.get_node_by_symbol(&callee_symbol),
                callers.get(&callee_symbol),
            ) {
                // Check if function is underspecified
                let is_underspecified = self.is_function_underspecified(&callee_symbol, &graph);

                if is_underspecified {
                    for &caller_idx in caller_indices {
                        if callee_idx != caller_idx {
                            graph.add_edge(callee_idx, caller_idx, EdgeKind::CallIn);
                        }
                    }
                }
            }
        }

        graph.type_registry = type_registry;
        Ok(graph)
    }

    /// Check if a variable is mutable
    fn is_variable_mutable(&self, symbol: &str, semantic_data: &SemanticData) -> bool {
        for doc in &semantic_data.documents {
            for def in &doc.definitions {
                if def.symbol_id == symbol
                    && let SymbolDetails::Variable(var_details) = &def.details
                {
                    return matches!(var_details.mutability, Mutability::Mutable);
                }
            }
        }
        // Default to mutable for safety
        true
    }

    /// Check if a function is underspecified (incomplete signature)
    fn is_function_underspecified(&self, symbol: &str, graph: &ContextGraph) -> bool {
        if let Some(node_idx) = graph.get_node_by_symbol(symbol)
            && let Some(Node::Function(func)) = graph.graph.node_weight(node_idx)
        {
            // Use is_signature_complete() from FunctionNode
            return !func.is_signature_complete();
        }
        false
    }

    /// Resolve a symbol to the nearest ancestor that is a node
    fn resolve_to_node_symbol(
        symbol: &str,
        node_symbols: &HashSet<SymbolId>,
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
}

/// Convert semantic span to node SourceSpan
fn convert_span(span: &SemanticSpan) -> SourceSpan {
    SourceSpan {
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
    }
}

/// Convert semantic span for size function (0-indexed to what size_function expects)
fn convert_span_for_size(span: &SemanticSpan) -> crate::domain::node::SourceSpan {
    crate::domain::node::SourceSpan {
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
    }
}

/// Infer node type from symbol kind
fn infer_node_type_from_kind(kind: &SymbolKind) -> NodeType {
    match kind {
        SymbolKind::Function => NodeType::Function,
        SymbolKind::Variable => NodeType::Variable,
        SymbolKind::Type => NodeType::Variable, // Should not happen, but default to Variable
    }
}

/// Extract signature text for documentation scoring
fn extract_signature(def: &SymbolDefinition) -> Option<String> {
    match &def.details {
        SymbolDetails::Function(func) => {
            let params: Vec<String> = func
                .parameters
                .iter()
                .map(|p| {
                    if let Some(ref type_id) = p.param_type {
                        format!("{}: {}", p.name, type_id)
                    } else {
                        p.name.clone()
                    }
                })
                .collect();
            let sig = format!("({}) -> {:?}", params.join(", "), func.return_types);
            Some(sig)
        }
        _ => None,
    }
}

/// Create a graph node from symbol definition
fn create_node_from_definition(
    core: NodeCore,
    def: &SymbolDefinition,
    is_interface_method: bool,
) -> Result<Node> {
    match &def.details {
        SymbolDetails::Function(func_details) => {
            let parameters: Vec<crate::domain::node::Parameter> = func_details
                .parameters
                .iter()
                .map(|p| crate::domain::node::Parameter {
                    name: p.name.clone(),
                    param_type: p.param_type.clone(),
                })
                .collect();

            Ok(Node::Function(FunctionNode {
                core,
                parameters,
                is_async: func_details.modifiers.is_async,
                is_generator: func_details.modifiers.is_generator,
                visibility: convert_visibility(&func_details.modifiers.visibility),
                return_types: func_details.return_types.clone(),
                is_interface_method,
            }))
        }
        SymbolDetails::Variable(var_details) => {
            let variable_kind = match var_details.scope {
                SemanticVarScope::Global => VariableKind::Global,
                SemanticVarScope::Field => VariableKind::ClassField,
            };

            Ok(Node::Variable(VariableNode {
                core,
                var_type: var_details.var_type.clone(),
                mutability: convert_mutability(&var_details.mutability),
                variable_kind,
            }))
        }
        SymbolDetails::Type(_) => {
            // Types should not become nodes, this is an error case
            anyhow::bail!("Type symbol should not be converted to node: {}", def.symbol_id)
        }
    }
}

/// Convert semantic visibility to node visibility
fn convert_visibility(vis: &Visibility) -> NodeVisibility {
    match vis {
        Visibility::Public => NodeVisibility::Public,
        Visibility::Private => NodeVisibility::Private,
        Visibility::Protected => NodeVisibility::Protected,
        Visibility::Internal => NodeVisibility::Internal,
    }
}

/// Convert semantic mutability to node mutability
fn convert_mutability(mutability: &Mutability) -> NodeMutability {
    match mutability {
        Mutability::Const => NodeMutability::Const,
        Mutability::Immutable => NodeMutability::Immutable,
        Mutability::Mutable => NodeMutability::Mutable,
    }
}

/// Create TypeInfo for registering in TypeRegistry
fn create_type_info(def: &SymbolDefinition, context_size: u32, doc_score: f32) -> TypeInfo {
    let mut type_kind = TypeKind::Class;
    let mut is_abstract = false;
    let mut type_param_count = 0;

    if let SymbolDetails::Type(type_details) = &def.details {
        type_kind = match type_details.kind {
            crate::domain::semantic::TypeKind::Class => TypeKind::Class,
            crate::domain::semantic::TypeKind::Interface => TypeKind::Interface,
            crate::domain::semantic::TypeKind::Struct => TypeKind::Struct,
            crate::domain::semantic::TypeKind::Enum => TypeKind::Enum,
            crate::domain::semantic::TypeKind::TypeAlias => TypeKind::TypeAlias,
            _ => TypeKind::Class,
        };
        is_abstract = type_details.is_abstract;
        type_param_count = type_details.type_params.len() as u32;
    }

    TypeInfo {
        definition: TypeDefAttribute {
            type_kind,
            is_abstract,
            type_param_count,
        },
        context_size,
        doc_score,
    }
}

/// Check if a symbol is enclosed by an abstract type (Interface/Protocol/Trait/Abstract Class)
fn is_enclosed_by_abstract_type(
    enclosing_symbol: &Option<SymbolId>,
    semantic_data: &SemanticData,
) -> bool {
    let Some(parent_id) = enclosing_symbol else {
        return false;
    };

    // Search in document definitions
    for doc in &semantic_data.documents {
        for def in &doc.definitions {
            if &def.symbol_id == parent_id
                && let SymbolDetails::Type(type_details) = &def.details
            {
                return type_details.is_abstract;
            }
        }
    }

    // Search in external_symbols
    for def in &semantic_data.external_symbols {
        if &def.symbol_id == parent_id
            && let SymbolDetails::Type(type_details) = &def.details
        {
            return type_details.is_abstract;
        }
    }

    false
}

/// Extract only the signature portion of a method span (first line to colon/semicolon)
/// For interface methods, we only want to count the signature, not any implementation body
fn extract_signature_span(span: &SemanticSpan, source_code: &str) -> SourceSpan {
    let lines: Vec<&str> = source_code.lines().collect();
    let start_line = span.start_line as usize;

    if start_line >= lines.len() {
        // Fallback: use original span
        return SourceSpan {
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
        };
    }

    // Find first colon or semicolon (Python/TypeScript signature end marker)
    for (i, line) in lines.iter().enumerate().skip(start_line) {
        if line.contains(':') || line.contains(';') {
            return SourceSpan {
                start_line: span.start_line,
                start_column: span.start_column,
                end_line: i as u32,
                end_column: line.len() as u32,
            };
        }
    }

    // Fallback: use original span
    SourceSpan {
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::semantic::{FunctionDetails, SourceLocation};

    fn test_function_def(symbol_id: &str) -> SymbolDefinition {
        SymbolDefinition {
            symbol_id: symbol_id.to_string(),
            kind: SymbolKind::Function,
            name: "test_func".to_string(),
            display_name: "test_func".to_string(),
            location: SourceLocation {
                file_path: "test.py".to_string(),
                line: 0,
                column: 0,
            },
            span: SemanticSpan {
                start_line: 0,
                start_column: 0,
                end_line: 10,
                end_column: 0,
            },
            enclosing_symbol: None,
            is_external: false,
            documentation: vec![],
            details: SymbolDetails::Function(FunctionDetails::default()),
        }
    }

    #[test]
    fn test_convert_visibility() {
        assert!(matches!(
            convert_visibility(&Visibility::Public),
            NodeVisibility::Public
        ));
        assert!(matches!(
            convert_visibility(&Visibility::Private),
            NodeVisibility::Private
        ));
    }

    #[test]
    fn test_create_type_info_from_class() {
        let def = test_function_def("TestClass");
        let info = create_type_info(&def, 100, 0.8);
        assert_eq!(info.definition.type_kind, TypeKind::Class);
        assert!(!info.definition.is_abstract);
    }

    #[test]
    fn test_is_enclosed_by_abstract_type_with_protocol() {
        use crate::domain::semantic::{DocumentSemantics, TypeDetails};

        let protocol_id = "MyProtocol#";
        let semantic_data = SemanticData {
            project_root: "/test".to_string(),
            documents: vec![DocumentSemantics {
                relative_path: "test.py".to_string(),
                language: "python".to_string(),
                definitions: vec![SymbolDefinition {
                    symbol_id: protocol_id.to_string(),
                    kind: SymbolKind::Type,
                    name: "MyProtocol".to_string(),
                    display_name: "MyProtocol".to_string(),
                    location: SourceLocation {
                        file_path: "test.py".to_string(),
                        line: 0,
                        column: 0,
                    },
                    span: SemanticSpan {
                        start_line: 0,
                        start_column: 0,
                        end_line: 5,
                        end_column: 0,
                    },
                    enclosing_symbol: None,
                    is_external: false,
                    documentation: vec![],
                    details: SymbolDetails::Type(TypeDetails {
                        kind: crate::domain::semantic::TypeKind::Interface,
                        is_abstract: true,
                        is_final: false,
                        visibility: Visibility::Public,
                        type_params: vec![],
                        implements: vec![],
                        inherits: vec![],
                        fields: vec![],
                    }),
                }],
                references: vec![],
            }],
            external_symbols: vec![],
        };

        assert!(is_enclosed_by_abstract_type(
            &Some(protocol_id.to_string()),
            &semantic_data
        ));
        assert!(!is_enclosed_by_abstract_type(&None, &semantic_data));
        assert!(!is_enclosed_by_abstract_type(
            &Some("NonExistent#".to_string()),
            &semantic_data
        ));
    }

    #[test]
    fn test_extract_signature_span_python() {
        let source = "    def method(self, x: int) -> str:\n        return str(x)\n        pass\n";
        let span = SemanticSpan {
            start_line: 0,
            start_column: 4,
            end_line: 2,
            end_column: 12,
        };

        let sig_span = extract_signature_span(&span, source);

        // Should stop at the first line with colon
        assert_eq!(sig_span.start_line, 0);
        assert_eq!(sig_span.end_line, 0);
    }
}
