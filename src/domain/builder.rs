use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;
use crate::domain::node::{
    FunctionNode, Mutability, Node, NodeCore, SourceSpan, TypeKind, TypeNode, VariableKind,
    VariableNode, Visibility,
};
use crate::domain::policy::{DocumentationScorer, NodeInfo, NodeType, SizeFunction};
use crate::domain::ports::SourceReader;
use crate::domain::semantic::{ReferenceRole, SemanticData, SymbolKind, SymbolMetadata};
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

        // Pass 1: Node Allocation
        for document in &semantic_data.documents {
            let source_path = Path::new(&semantic_data.project_root).join(&document.relative_path);
            let source_code = source_reader.read(&source_path)?;

            for definition in &document.definitions {
                let node_id = graph.graph.node_count() as u32;

                // Compute context_size
                let span = SourceSpan {
                    start_line: definition.enclosing_range.start_line,
                    start_column: definition.enclosing_range.start_column,
                    end_line: definition.enclosing_range.end_line,
                    end_column: definition.enclosing_range.end_column,
                };
                let context_size = self.size_function.compute(&source_code, &span);

                // Compute doc_score
                let doc_text = definition
                    .metadata
                    .documentation
                    .first()
                    .map(|s| s.as_str());
                let node_info = NodeInfo {
                    node_type: infer_node_type_from_kind(&definition.metadata.kind),
                    name: definition.metadata.display_name.clone(),
                    signature: definition.metadata.signature.clone(),
                };
                let doc_score = self.doc_scorer.score(&node_info, doc_text);

                // Create NodeCore
                let core = NodeCore::new(
                    node_id,
                    definition.metadata.display_name.clone(),
                    definition.metadata.enclosing_symbol.clone(),
                    context_size,
                    SourceSpan {
                        start_line: definition.range.start_line,
                        start_column: definition.range.start_column,
                        end_line: definition.range.end_line,
                        end_column: definition.range.end_column,
                    },
                    doc_score,
                    definition.metadata.is_external,
                );

                // Create specific node type
                let node = create_node_from_definition(core, &definition.metadata)?;
                graph.add_node(definition.symbol.clone(), node);
            }
        }

        // Pass 2: Edge Wiring and Collection for Dynamic Expansion
        let mut state_writers: HashMap<String, Vec<petgraph::graph::NodeIndex>> = HashMap::new();
        let mut callers: HashMap<String, Vec<petgraph::graph::NodeIndex>> = HashMap::new();
        let mut readers: Vec<(petgraph::graph::NodeIndex, String)> = Vec::new();

        for document in &semantic_data.documents {
            for reference in &document.references {
                if let (Some(source_idx), Some(target_idx)) = (
                    graph.get_node_by_symbol(&reference.enclosing_symbol),
                    graph.get_node_by_symbol(&reference.symbol),
                ) {
                    let edge_kind = infer_edge_kind(&reference.role, source_idx, target_idx);

                    // Track writers for SharedStateWrite expansion
                    if matches!(edge_kind, EdgeKind::Write) {
                        state_writers
                            .entry(reference.symbol.clone())
                            .or_default()
                            .push(source_idx);
                    }

                    // Track readers for SharedStateWrite expansion
                    if matches!(edge_kind, EdgeKind::Read) {
                        readers.push((source_idx, reference.symbol.clone()));
                    }

                    // Track callers for CallIn expansion
                    if matches!(edge_kind, EdgeKind::Call) {
                        callers
                            .entry(reference.symbol.clone())
                            .or_default()
                            .push(source_idx);
                    }

                    graph.add_edge(source_idx, target_idx, edge_kind);
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
        SymbolKind::Variable | SymbolKind::Field | SymbolKind::Constant | SymbolKind::Parameter => {
            NodeType::Variable
        }
        SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::TypeAlias
        | SymbolKind::Trait
        | SymbolKind::Protocol => NodeType::Type,
        _ => NodeType::Function, // Default
    }
}

fn create_node_from_definition(core: NodeCore, metadata: &SymbolMetadata) -> Result<Node> {
    match infer_node_type_from_kind(&metadata.kind) {
        NodeType::Function => {
            // Extract signature information (simplified - would need actual parsing)
            Ok(Node::Function(FunctionNode {
                core,
                param_count: 0,       // TODO: extract from signature
                typed_param_count: 0, // TODO: extract from signature
                has_return_type: metadata.signature.is_some(), // Simplified
                is_async: false,      // TODO: extract from signature
                is_generator: false,  // TODO: extract from signature
                visibility: Visibility::Public, // TODO: extract from metadata
            }))
        }
        NodeType::Variable => {
            Ok(Node::Variable(VariableNode {
                core,
                has_type_annotation: metadata.signature.is_some(),
                mutability: Mutability::Mutable, // TODO: infer from context
                variable_kind: VariableKind::Global, // TODO: infer from context
            }))
        }
        NodeType::Type => {
            let is_abstract = matches!(
                metadata.kind,
                SymbolKind::Interface | SymbolKind::Trait | SymbolKind::Protocol
            );
            Ok(Node::Type(TypeNode {
                core,
                type_kind: match metadata.kind {
                    SymbolKind::Class => TypeKind::Class,
                    SymbolKind::Interface => TypeKind::Interface,
                    SymbolKind::Struct => TypeKind::Struct,
                    SymbolKind::Enum => TypeKind::Enum,
                    SymbolKind::TypeAlias => TypeKind::TypeAlias,
                    SymbolKind::Trait => TypeKind::Protocol, // Trait is similar to Protocol
                    SymbolKind::Protocol => TypeKind::Protocol,
                    _ => TypeKind::Class, // Default
                },
                is_abstract,
                type_param_count: 0, // TODO: extract from signature
            }))
        }
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
        ReferenceRole::TypeUsage => EdgeKind::ParamType, // Simplified
        ReferenceRole::Import => EdgeKind::Call,         // Simplified
        ReferenceRole::Unknown => EdgeKind::Call,        // Default
    }
}
