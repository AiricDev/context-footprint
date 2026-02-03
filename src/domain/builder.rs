use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;
use crate::domain::node::{
    FunctionNode, Mutability, Node, NodeCore, SourceSpan, VariableKind, VariableNode, Visibility,
};
use crate::domain::policy::{DocumentationScorer, NodeInfo, NodeType, SizeFunction};
use crate::domain::ports::SourceReader;
use crate::domain::semantic::{
    definition_role, DefinitionRole, ReferenceRole, SemanticData, SymbolKind, SymbolMetadata,
};
use crate::domain::type_registry::{TypeDefAttribute, TypeInfo, TypeKind, TypeRegistry};
use anyhow::Result;
use std::collections::HashMap;
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
        let symbol_index = &semantic_data.symbol_index;

        // Pass 1: Node Allocation (uses definition_role and symbol_index; no pre-collection loop)
        for document in &semantic_data.documents {
            let source_path = Path::new(&semantic_data.project_root).join(&document.relative_path);
            let source_code = source_reader.read(&source_path)?;

            for definition in &document.definitions {
                let role = definition_role(definition, symbol_index);

                if role == DefinitionRole::InlineParameter || role == DefinitionRole::Skip {
                    continue;
                }

                let kind = &definition.metadata.kind;
                let node_id = graph.graph.node_count() as u32;
                let doc_texts: Vec<String> = definition.metadata.documentation.clone();
                let span = SourceSpan {
                    start_line: definition.enclosing_range.start_line,
                    start_column: definition.enclosing_range.start_column,
                    end_line: definition.enclosing_range.end_line,
                    end_column: definition.enclosing_range.end_column,
                };
                let context_size = self.size_function.compute(&source_code, &span, &doc_texts);
                let doc_text = doc_texts.first().map(|s| s.as_str());
                let language = document
                    .relative_path
                    .split('.')
                    .next_back()
                    .map(|ext| ext.to_lowercase());
                let node_info = NodeInfo {
                    node_type: infer_node_type_from_kind(kind),
                    name: definition.metadata.display_name.clone(),
                    signature: definition.metadata.signature.clone(),
                    language,
                };
                let doc_score = self.doc_scorer.score(&node_info, doc_text);

                if role == DefinitionRole::TypeOnly {
                    let type_info =
                        create_type_info(kind, &definition.metadata, context_size, doc_score);
                    type_registry.register(definition.symbol.clone(), type_info);
                    continue;
                }

                // GraphNode: create node; parameters from symbol_index for functions
                let core = NodeCore::new(
                    node_id,
                    definition.metadata.display_name.clone(),
                    definition.metadata.enclosing_symbol.clone(),
                    context_size,
                    span,
                    doc_score,
                    definition.metadata.is_external,
                    document.relative_path.clone(),
                );
                let params = symbol_index
                    .function_parameters
                    .get(&definition.symbol)
                    .cloned()
                    .unwrap_or_else(|| definition.metadata.parameters.clone());
                let node =
                    create_node_from_definition_with_params(core, &definition.metadata, &params)?;
                graph.add_node(definition.symbol.clone(), node);
            }
        }

        // Helper to resolve a symbol to the nearest ancestor that IS a node
        let resolve_to_node_symbol = |mut sym: String, graph: &ContextGraph| -> Option<String> {
            while !graph.symbol_to_node.contains_key(&sym) {
                if let Some(parent) = symbol_index.symbol_parent.get(&sym) {
                    sym = parent.clone();
                } else {
                    return None;
                }
            }
            Some(sym)
        };

        // Pass 2: Edge Wiring
        let mut state_writers: HashMap<String, Vec<petgraph::graph::NodeIndex>> = HashMap::new();
        let mut callers: HashMap<String, Vec<petgraph::graph::NodeIndex>> = HashMap::new();
        let mut readers: Vec<(petgraph::graph::NodeIndex, String)> = Vec::new();

        for document in &semantic_data.documents {
            for reference in &document.references {
                let resolved_source_sym =
                    resolve_to_node_symbol(reference.enclosing_symbol.clone(), &graph);
                let resolved_target_sym =
                    resolve_to_node_symbol(reference.symbol.clone(), &graph);

                if let (Some(source_sym), Some(target_sym)) =
                    (resolved_source_sym, resolved_target_sym)
                {
                    let source_idx = *graph.symbol_to_node.get(&source_sym).unwrap();
                    let target_idx = *graph.symbol_to_node.get(&target_sym).unwrap();

                    if source_idx == target_idx {
                        continue;
                    }

                    let edge_kind = infer_edge_kind(&reference.role, source_idx, target_idx);

                    if matches!(edge_kind, EdgeKind::Write) {
                        state_writers
                            .entry(target_sym.clone())
                            .or_default()
                            .push(source_idx);
                    }
                    if matches!(edge_kind, EdgeKind::Read) {
                        readers.push((source_idx, target_sym.clone()));
                    }
                    if matches!(edge_kind, EdgeKind::Call) {
                        callers
                            .entry(target_sym.clone())
                            .or_default()
                            .push(source_idx);
                    }

                    graph.add_edge(source_idx, target_idx, edge_kind);
                }
            }
        }

        // Pass 2.5: Process TypeDefinition relationships
        // Types are now in TypeRegistry, not graph nodes.
        // We fill in type references in FunctionNode and VariableNode.
        for document in &semantic_data.documents {
            for definition in &document.definitions {
                if let Some(&source_idx) = graph.symbol_to_node.get(&definition.symbol) {
                    for relationship in &definition.metadata.relationships {
                        if matches!(
                            relationship.kind,
                            crate::domain::semantic::RelationshipKind::TypeDefinition
                        ) {
                            // TypeDefinition means "source uses target as a type"
                            let target_type_id = &relationship.target_symbol;

                            // Only process if target is a registered type
                            if type_registry.contains(target_type_id) {
                                match graph.graph.node_weight_mut(source_idx) {
                                    Some(Node::Function(f)) => {
                                        f.return_type = Some(target_type_id.clone());
                                    }
                                    Some(Node::Variable(v)) => {
                                        v.var_type = Some(target_type_id.clone());
                                    }
                                    None => {}
                                }
                            }
                        }
                        // Implements/Inherits are stored in TypeRegistry, not as graph edges
                    }
                }
            }
        }

        // Pass 3: Dynamic Expansion Edges
        // 1. SharedStateWrite edges: Reader -> Writer
        for (reader_idx, state_symbol) in readers {
            if let Some(writers) = state_writers.get(&state_symbol) {
                for &writer_idx in writers {
                    if reader_idx != writer_idx {
                        graph.add_edge(reader_idx, writer_idx, EdgeKind::SharedStateWrite);
                    }
                }
            }
        }

        // 2. CallIn edges: Callee -> Caller
        let symbols: Vec<String> = graph.symbol_to_node.keys().cloned().collect();
        for callee_symbol in symbols {
            if let (Some(callee_idx), Some(caller_indices)) = (
                graph.get_node_by_symbol(&callee_symbol),
                callers.get(&callee_symbol),
            ) {
                for &caller_idx in caller_indices {
                    if callee_idx != caller_idx {
                        graph.add_edge(callee_idx, caller_idx, EdgeKind::CallIn);
                    }
                }
            }
        }
        graph.type_registry = type_registry;
        Ok(graph)
    }
}

fn infer_node_type_from_kind(kind: &SymbolKind) -> NodeType {
    match kind {
        SymbolKind::Function
        | SymbolKind::Method
        | SymbolKind::Constructor
        | SymbolKind::StaticMethod
        | SymbolKind::AbstractMethod => NodeType::Function,
        SymbolKind::Variable
        | SymbolKind::Field
        | SymbolKind::Constant
        | SymbolKind::Parameter
        | SymbolKind::Module
        | SymbolKind::Namespace
        | SymbolKind::Package
        | SymbolKind::Macro => NodeType::Variable,
        // Types are not nodes anymore (they go into TypeRegistry).
        // Return Variable so NodeInfo is valid for doc scoring; create_node_from_definition is never called for types.
        SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::TypeAlias
        | SymbolKind::Trait
        | SymbolKind::Protocol => NodeType::Variable,
        _ => NodeType::Variable,
    }
}

/// Creates a graph node from definition metadata and pre-resolved parameters (from SymbolIndex or metadata).
fn create_node_from_definition_with_params(
    core: NodeCore,
    metadata: &SymbolMetadata,
    params: &[crate::domain::semantic::Parameter],
) -> Result<Node> {
    use crate::domain::node::Parameter as NodeParameter;

    match infer_node_type_from_kind(&metadata.kind) {
        NodeType::Function => {
            let parameters: Vec<NodeParameter> = params
                .iter()
                .map(|p| NodeParameter {
                    name: p.name.clone(),
                    param_type: p.param_type.clone(),
                })
                .collect();

            Ok(Node::Function(FunctionNode {
                core,
                parameters,
                is_async: false,
                is_generator: false,
                visibility: Visibility::Public,
                return_type: metadata.return_type.clone(),
            }))
        }
        NodeType::Variable => Ok(Node::Variable(VariableNode {
            core,
            var_type: None, // Filled from relationships in Pass 2.5
            mutability: Mutability::Mutable,
            variable_kind: VariableKind::Global,
        })),
    }
}

/// Create TypeInfo for registering in TypeRegistry
fn create_type_info(
    kind: &SymbolKind,
    metadata: &SymbolMetadata,
    context_size: u32,
    doc_score: f32,
) -> TypeInfo {
    // Check if it's abstract based on kind
    let mut is_abstract = matches!(
        kind,
        SymbolKind::Interface | SymbolKind::Trait | SymbolKind::Protocol
    );

    // Python Protocol detection
    if matches!(kind, SymbolKind::Class) {
        is_abstract = metadata.relationships.iter().any(|r| {
            matches!(
                r.kind,
                crate::domain::semantic::RelationshipKind::Implements
            ) && r.target_symbol.contains("typing/Protocol#")
        });
    }

    let type_kind = match kind {
        SymbolKind::Class if is_abstract => TypeKind::Protocol,
        SymbolKind::Class => TypeKind::Class,
        SymbolKind::Interface => TypeKind::Interface,
        SymbolKind::Struct => TypeKind::Struct,
        SymbolKind::Enum => TypeKind::Enum,
        SymbolKind::TypeAlias => TypeKind::TypeAlias,
        SymbolKind::Trait => TypeKind::Protocol,
        SymbolKind::Protocol => TypeKind::Protocol,
        _ => TypeKind::Class,
    };

    TypeInfo {
        definition: TypeDefAttribute {
            type_kind,
            is_abstract,
            type_param_count: 0, // TODO: extract from signature
        },
        context_size,
        doc_score,
    }
}

fn infer_edge_kind(
    role: &ReferenceRole,
    _source: petgraph::graph::NodeIndex,
    _target: petgraph::graph::NodeIndex,
) -> EdgeKind {
    match role {
        ReferenceRole::Read => EdgeKind::Read,
        ReferenceRole::Write => EdgeKind::Write,
        ReferenceRole::Call => EdgeKind::Call,
        ReferenceRole::TypeUsage => EdgeKind::Call, // Types are in TypeRegistry, not graph
        ReferenceRole::Import => EdgeKind::Call,
        ReferenceRole::Unknown => EdgeKind::Call,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::semantic::SymbolMetadata;

    #[test]
    fn test_infer_node_type_from_kind() {
        assert_eq!(
            infer_node_type_from_kind(&SymbolKind::Function),
            NodeType::Function
        );
        assert_eq!(
            infer_node_type_from_kind(&SymbolKind::Variable),
            NodeType::Variable
        );
        assert_eq!(
            infer_node_type_from_kind(&SymbolKind::Unknown),
            NodeType::Variable
        );
    }

    #[test]
    fn test_create_type_info() {
        let metadata = SymbolMetadata {
            symbol: "MyClass#".into(),
            kind: SymbolKind::Class,
            display_name: "MyClass".into(),
            documentation: vec![],
            signature: None,
            parameters: vec![],
            return_type: None,
            relationships: vec![],
            enclosing_symbol: None,
            is_external: false,
            throws: vec![],
        };

        let type_info = create_type_info(&SymbolKind::Class, &metadata, 100, 0.8);
        assert_eq!(type_info.definition.type_kind, TypeKind::Class);
        assert!(!type_info.definition.is_abstract);
        assert_eq!(type_info.context_size, 100);
        assert_eq!(type_info.doc_score, 0.8);
    }

    #[test]
    fn test_create_type_info_protocol() {
        let metadata = SymbolMetadata {
            symbol: "MyProtocol#".into(),
            kind: SymbolKind::Class,
            display_name: "MyProtocol".into(),
            documentation: vec![],
            signature: None,
            parameters: vec![],
            return_type: None,
            relationships: vec![crate::domain::semantic::Relationship {
                target_symbol: "typing/Protocol#".into(),
                kind: crate::domain::semantic::RelationshipKind::Implements,
            }],
            enclosing_symbol: None,
            is_external: false,
            throws: vec![],
        };

        let type_info = create_type_info(&SymbolKind::Class, &metadata, 100, 0.8);
        assert_eq!(type_info.definition.type_kind, TypeKind::Protocol);
        assert!(type_info.definition.is_abstract);
    }
}
