use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;
use crate::domain::node::{
    FunctionNode, Mutability, Node, NodeCore, SourceSpan, TypeDefAttribute, TypeKind, VariableKind,
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

        // 1. Pre-collect kinds and parentage for all definitions
        let mut symbol_to_kind: HashMap<String, SymbolKind> = HashMap::new();
        let mut symbol_to_parent: HashMap<String, String> = HashMap::new();
        let mut parameter_map: HashMap<String, Vec<crate::domain::node::Parameter>> =
            HashMap::new();

        for document in &semantic_data.documents {
            for definition in &document.definitions {
                symbol_to_kind.insert(definition.symbol.clone(), definition.metadata.kind.clone());
                if let Some(parent) = &definition.metadata.enclosing_symbol {
                    symbol_to_parent.insert(definition.symbol.clone(), parent.clone());
                }
            }
        }

        // Pass 1: Node Allocation
        for document in &semantic_data.documents {
            let source_path = Path::new(&semantic_data.project_root).join(&document.relative_path);
            let source_code = source_reader.read(&source_path)?;

            for definition in &document.definitions {
                let kind = &definition.metadata.kind;

                if matches!(kind, SymbolKind::Parameter) {
                    if let Some(parent) = &definition.metadata.enclosing_symbol {
                        let mut type_annotation = None;
                        // Try to find type definition relationship
                        for rel in &definition.metadata.relationships {
                            if matches!(
                                rel.kind,
                                crate::domain::semantic::RelationshipKind::TypeDefinition
                            ) {
                                type_annotation = Some(crate::domain::node::TypeRefAttribute {
                                    type_name: rel.target_symbol.clone(),
                                });
                                break;
                            }
                        }

                        let param = crate::domain::node::Parameter {
                            name: definition.metadata.display_name.clone(),
                            type_annotation,
                        };
                        parameter_map.entry(parent.clone()).or_default().push(param);
                    }
                    continue;
                }

                // Determine if this symbol should be an independent node
                let should_be_node = match kind {
                    // Always nodes
                    SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Constructor
                    | SymbolKind::StaticMethod
                    | SymbolKind::AbstractMethod
                    | SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Struct
                    | SymbolKind::Enum
                    | SymbolKind::TypeAlias
                    | SymbolKind::Trait
                    | SymbolKind::Protocol => true,

                    // Variable-like nodes: only if they are not parameters or local-like
                    SymbolKind::Variable | SymbolKind::Field | SymbolKind::Constant => definition
                        .metadata
                        .enclosing_symbol
                        .as_ref()
                        .and_then(|parent_sym| symbol_to_kind.get(parent_sym))
                        .is_none_or(|parent_kind| {
                            !matches!(
                                parent_kind,
                                SymbolKind::Function
                                    | SymbolKind::Method
                                    | SymbolKind::Constructor
                                    | SymbolKind::StaticMethod
                                    | SymbolKind::AbstractMethod
                            )
                        }),
                    _ => false, // Parameters, Modules, etc. are not independent nodes
                };

                if !should_be_node {
                    continue;
                }

                let node_id = graph.graph.node_count() as u32;

                // Extract documentation strings
                let doc_texts: Vec<String> = definition.metadata.documentation.clone();

                // Compute context_size
                let span = SourceSpan {
                    start_line: definition.enclosing_range.start_line,
                    start_column: definition.enclosing_range.start_column,
                    end_line: definition.enclosing_range.end_line,
                    end_column: definition.enclosing_range.end_column,
                };
                let context_size = self.size_function.compute(&source_code, &span, &doc_texts);

                // Compute doc_score
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

                // Create NodeCore
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

                // Create specific node type
                let node = create_node_from_definition(core, &definition.metadata)?;
                graph.add_node(definition.symbol.clone(), node);
            }
        }

        // Post-Pass 1: Attach parameters to functions and create ParamType edges
        for (func_symbol, params) in parameter_map {
            if let Some(&func_idx) = graph.symbol_to_node.get(&func_symbol) {
                // 1. Update FunctionNode
                if let Node::Function(f) = graph.graph.node_weight_mut(func_idx).unwrap() {
                    f.parameters = params.clone();
                }

                // 2. Add ParamType edges (Function -> Type) - Optional if we use attributes,
                // but checking relationships for edge creation is handled in Pass 2.5.
                // However, parameters aren't nodes anymore, so relationship from Parameter -> Type
                // won't be picked up by Pass 2.5.
                // We should add edges here for explicit dependency tracking if desired.
                // For now, relying on internal attributes is the goal, edges can be implicit.
            }
        }

        // Helper to resolve a symbol to the nearest ancestor that IS a node
        let resolve_to_node_symbol = |mut sym: String,
                                      graph: &ContextGraph,
                                      symbol_to_parent: &HashMap<String, String>|
         -> Option<String> {
            while !graph.symbol_to_node.contains_key(&sym) {
                if let Some(parent) = symbol_to_parent.get(&sym) {
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
                let resolved_source_sym = resolve_to_node_symbol(
                    reference.enclosing_symbol.clone(),
                    &graph,
                    &symbol_to_parent,
                );
                let resolved_target_sym =
                    resolve_to_node_symbol(reference.symbol.clone(), &graph, &symbol_to_parent);

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

        // Pass 2.5: Process relationships from definitions (e.g., return types, implements, inherits)
        for document in &semantic_data.documents {
            for definition in &document.definitions {
                if let Some(&source_idx) = graph.symbol_to_node.get(&definition.symbol) {
                    for relationship in &definition.metadata.relationships {
                        // Resolve target symbol to a node symbol
                        if let Some(target_idx) = resolve_to_node_symbol(
                            relationship.target_symbol.clone(),
                            &graph,
                            &symbol_to_parent,
                        )
                        .and_then(|resolved_target| {
                            graph.symbol_to_node.get(&resolved_target).copied()
                        }) {
                            if source_idx == target_idx {
                                continue;
                            }

                            // Convert relationship kind to edge kind based on source node type
                            let edge_kind = match relationship.kind {
                                crate::domain::semantic::RelationshipKind::TypeDefinition => {
                                    // TypeDefinition means "source uses target as a type"
                                    // The specific edge depends on what the source is
                                    match graph.node(source_idx) {
                                        crate::domain::node::Node::Function(_) => {
                                            EdgeKind::ReturnType
                                        }
                                        crate::domain::node::Node::Variable(_) => {
                                            EdgeKind::VariableType
                                        }
                                    }
                                }
                                crate::domain::semantic::RelationshipKind::Implements => {
                                    EdgeKind::Implements
                                }
                                crate::domain::semantic::RelationshipKind::Inherits => {
                                    EdgeKind::Inherits
                                }
                                crate::domain::semantic::RelationshipKind::References => {
                                    // Generic references - skip, handled by occurrences
                                    continue;
                                }
                            };

                            graph.add_edge(source_idx, target_idx, edge_kind);
                        }
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
        | SymbolKind::Module // Module __init__ treated as variable-like
        | SymbolKind::Namespace // Namespace/package treated as variable-like
        | SymbolKind::Package
        | SymbolKind::Macro => NodeType::Variable, // Macro definitions are declaration-like
        SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::TypeAlias
        | SymbolKind::Trait
        | SymbolKind::Protocol => NodeType::Type,
        _ => NodeType::Variable, // Default: treat unknown as variable (safer than function)
    }
}

fn create_node_from_definition(core: NodeCore, metadata: &SymbolMetadata) -> Result<Node> {
    match infer_node_type_from_kind(&metadata.kind) {
        NodeType::Function => {
            // Parse signature to extract parameters and return type
            let (parameters, return_type_annotation) =
                parse_function_signature(metadata.signature.as_deref());

            Ok(Node::Function(FunctionNode {
                core,
                parameters,
                is_async: false,                // TODO: extract from signature
                is_generator: false,            // TODO: extract from signature
                visibility: Visibility::Public, // TODO: extract from metadata
                return_type_annotation,
            }))
        }
        NodeType::Variable => Ok(Node::Variable(VariableNode {
            core,
            type_annotation: None, // TODO: extract from signature
            type_definition: None,
            mutability: Mutability::Mutable, // TODO: infer from context
            variable_kind: VariableKind::Global, // TODO: infer from context
        })),
        NodeType::Type => {
            // Check if it's abstract based on kind
            let mut is_abstract = matches!(
                metadata.kind,
                SymbolKind::Interface | SymbolKind::Trait | SymbolKind::Protocol
            );

            // Python Protocol detection: SCIP-python marks Protocols as Class but with Implements relationship to typing.Protocol
            if matches!(metadata.kind, SymbolKind::Class) {
                is_abstract = metadata.relationships.iter().any(|r| {
                    matches!(
                        r.kind,
                        crate::domain::semantic::RelationshipKind::Implements
                    ) && r.target_symbol.contains("typing/Protocol#")
                });
            }

            Ok(Node::Variable(VariableNode {
                core,
                type_annotation: None,
                type_definition: Some(TypeDefAttribute {
                    type_kind: match metadata.kind {
                        SymbolKind::Class if is_abstract => TypeKind::Protocol, // Python Protocol
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
                }),
                mutability: Mutability::Immutable, // Type definitions are mostly immutable
                variable_kind: VariableKind::TypeDef,
            }))
        }
    }
}

/// Parse a function signature string to extract parameters and return type
/// Supports formats like:
/// - "() -> int"
/// - "(x: int) -> int"
/// - "(x: int, y: str) -> bool"
/// - "(x, y)" (no types)
fn parse_function_signature(
    signature: Option<&str>,
) -> (
    Vec<crate::domain::node::Parameter>,
    Option<crate::domain::node::TypeRefAttribute>,
) {
    use crate::domain::node::{Parameter, TypeRefAttribute};

    let signature = match signature {
        Some(s) if !s.is_empty() => s,
        _ => return (Vec::new(), None),
    };

    // Find the arrow separating params and return type
    let (params_part, return_part) = match signature.split_once("->") {
        Some((params, ret)) => (params.trim(), Some(ret.trim())),
        None => (signature.trim(), None),
    };

    // Parse return type if present
    let return_type_annotation = return_part.and_then(|ret| {
        let ret = ret.trim().trim_end_matches(':'); // Remove trailing colon if present
        if ret.is_empty() {
            None
        } else {
            Some(TypeRefAttribute {
                type_name: ret.to_string(),
            })
        }
    });

    // Parse parameters - extract content between parentheses
    let parameters = if params_part.starts_with('(') && params_part.contains(')') {
        let params_content = params_part
            .trim_start_matches('(')
            .split(')')
            .next()
            .unwrap_or("")
            .trim();

        if params_content.is_empty() {
            Vec::new()
        } else {
            params_content
                .split(',')
                .map(|param| {
                    let param = param.trim();
                    // Check if param has type annotation (contains ':')
                    let (name, type_annotation) = match param.split_once(':') {
                        Some((name, type_str)) => {
                            let type_str = type_str.trim();
                            (
                                name.trim().to_string(),
                                if type_str.is_empty() {
                                    None
                                } else {
                                    Some(TypeRefAttribute {
                                        type_name: type_str.to_string(),
                                    })
                                },
                            )
                        }
                        None => (param.to_string(), None),
                    };
                    Parameter {
                        name,
                        type_annotation,
                    }
                })
                .collect()
        }
    } else {
        Vec::new()
    };

    (parameters, return_type_annotation)
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
            infer_node_type_from_kind(&SymbolKind::Class),
            NodeType::Type
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
    fn test_create_node_from_definition_class_vs_protocol() {
        let core = NodeCore::new(
            0,
            "MyClass".into(),
            None,
            10,
            SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            1.0,
            false,
            "file.py".into(),
        );

        let mut metadata = SymbolMetadata {
            symbol: "MyClass#".into(),
            kind: SymbolKind::Class,
            display_name: "MyClass".into(),
            documentation: vec![],
            signature: None,
            relationships: vec![],
            enclosing_symbol: None,
            is_external: false,
        };

        let node = create_node_from_definition(core.clone(), &metadata).unwrap();
        if let Node::Variable(v) = node {
            if let Some(td) = v.type_definition {
                assert_eq!(td.type_kind, TypeKind::Class);
            } else {
                panic!("Expected TypeDefinition attribute");
            }
        } else {
            panic!("Expected Variable node");
        }

        // Add Protocol relationship
        metadata
            .relationships
            .push(crate::domain::semantic::Relationship {
                target_symbol: "typing/Protocol#".into(),
                kind: crate::domain::semantic::RelationshipKind::Implements,
            });

        let node = create_node_from_definition(core.clone(), &metadata).unwrap();
        if let Node::Variable(v) = node {
            if let Some(td) = v.type_definition {
                assert_eq!(td.type_kind, TypeKind::Protocol);
            } else {
                panic!("Expected TypeDefinition attribute");
            }
        } else {
            panic!("Expected Variable node");
        }
    }
}
