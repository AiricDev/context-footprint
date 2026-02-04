use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;
use crate::domain::node::Node;
use crate::domain::type_registry::TypeRegistry;

/// Node type for documentation scoring
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Function,
    Variable,
}

/// Pruning decision
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PruningDecision {
    Boundary,    // Stop traversal here; node is a valid abstraction
    Transparent, // Continue traversal through this node
}

/// Pruning parameters for the CF solver.
/// Only [doc_threshold] is configurable; "document completeness" is defined by doc_score (from doc_scorer).
#[derive(Debug, Clone)]
pub struct PruningParams {
    /// Documentation score threshold: doc_score >= this value is "sufficient documentation".
    pub doc_threshold: f32,
    /// If true (Academic): internal function is Boundary when sig complete and doc_score >= doc_threshold.
    /// If false (Strict): only abstract factory is Boundary for internal functions.
    pub treat_typed_documented_function_as_boundary: bool,
}


impl Default for PruningParams {
    fn default() -> Self {
        Self::academic(0.5)
    }
}

impl PruningParams {
    /// Academic mode: internal function IS a boundary if typed + documented.
    pub fn academic(doc_threshold: f32) -> Self {
        Self {
            doc_threshold,
            treat_typed_documented_function_as_boundary: true,
        }
    }

    /// Strict mode: internal function is TRANSPARENT unless it's an abstract factory.
    pub fn strict(doc_threshold: f32) -> Self {
        Self {
            doc_threshold,
            treat_typed_documented_function_as_boundary: false,
        }
    }
}

// -----------------------------------------------------------------------------
// Core algorithm (domain layer)
// -----------------------------------------------------------------------------

/// Returns true if the function returns an abstract type (Protocol/Interface/Trait)
/// with sufficient documentation - i.e. "abstract factory" pattern.
pub fn is_abstract_factory(
    function_node: &Node,
    type_registry: &TypeRegistry,
    doc_threshold: f32,
) -> bool {
    let Node::Function(f) = function_node else {
        return false;
    };
    let Some(return_type_id) = f.return_type_id() else {
        return false;
    };
    let Some(type_info) = type_registry.get(return_type_id) else {
        return false;
    };
    type_info.definition.is_abstract && type_info.doc_score >= doc_threshold
}

fn call_in_source_decision(params: &PruningParams, source: &Node) -> PruningDecision {
    if let Node::Function(f) = source {
        if f.is_signature_complete() && f.core.doc_score >= params.doc_threshold {
            return PruningDecision::Boundary;
        }
    }
    PruningDecision::Transparent
}

/// Core pruning algorithm: evaluates Boundary vs Transparent.
/// Order: edge handling → external → node dispatch; uses [PruningParams] (doc_threshold + mode).
pub fn evaluate(
    params: &PruningParams,
    source: &Node,
    target: &Node,
    edge_kind: &EdgeKind,
    graph: &ContextGraph,
) -> PruningDecision {
    // 1. Dynamic expansion edges
    match edge_kind {
        EdgeKind::SharedStateWrite => return PruningDecision::Transparent,
        EdgeKind::CallIn => return call_in_source_decision(params, source),
        _ => {}
    }

    // 2. External dependencies are always boundaries
    if target.core().is_external {
        return PruningDecision::Boundary;
    }

    // 3. Node type dispatch
    match target {
        Node::Variable(_) => PruningDecision::Transparent,
        Node::Function(f) => {
            if is_abstract_factory(target, &graph.type_registry, params.doc_threshold) {
                return PruningDecision::Boundary;
            }
            if params.treat_typed_documented_function_as_boundary
                && f.is_signature_complete()
                && f.core.doc_score >= params.doc_threshold
            {
                return PruningDecision::Boundary;
            }
            PruningDecision::Transparent
        }
    }
}

// SourceSpan is defined in node.rs
pub use crate::domain::node::SourceSpan;

/// Size function trait - computes context size
pub trait SizeFunction: Send + Sync {
    /// Compute the context size for a given source code span,
    /// potentially excluding documentation to avoid "punishing" well-documented code.
    fn compute(&self, source: &str, span: &SourceSpan, doc_texts: &[String]) -> u32;
}

/// Documentation scorer trait - evaluates documentation quality
pub trait DocumentationScorer: Send + Sync {
    /// Evaluate documentation quality, returns [0.0, 1.0] score
    /// - 0.0: No documentation or meaningless documentation
    /// - 1.0: Complete, clear documentation
    fn score(&self, node_info: &NodeInfo, doc_text: Option<&str>) -> f32;
}

/// Node information for documentation scoring
pub struct NodeInfo {
    pub node_type: NodeType,
    pub name: String,
    pub signature: Option<String>, // Function signature
    pub language: Option<String>,  // Programming language
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::edge::EdgeKind;
    use crate::domain::graph::ContextGraph;
    use crate::domain::node::{FunctionNode, Node, NodeCore, SourceSpan, Visibility};

    fn test_node(doc_score: f32) -> Node {
        let core = NodeCore::new(
            0,
            "f".to_string(),
            None,
            10,
            SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 1,
                end_column: 5,
            },
            doc_score,
            false,
            "test.py".to_string(),
        );
        Node::Function(FunctionNode {
            core,
            parameters: vec![crate::domain::node::Parameter {
                name: "x".to_string(),
                param_type: Some("int#".to_string()),
            }],
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_type: Some("int#".to_string()),
        })
    }

    #[test]
    fn test_default_pruning_params() {
        let p = PruningParams::default();
        assert!((p.doc_threshold - 0.5).abs() < 1e-5);
        assert!(p.treat_typed_documented_function_as_boundary);
    }

    #[test]
    fn test_academic_vs_strict() {
        let graph = ContextGraph::new();
        let target = test_node(0.8);
        let source = test_node(0.0);
        let edge = EdgeKind::Call;
        let academic = PruningParams::default();
        let strict = PruningParams {
            doc_threshold: 0.5,
            treat_typed_documented_function_as_boundary: false,
        };
        assert!(matches!(
            evaluate(&academic, &source, &target, &edge, &graph),
            PruningDecision::Boundary
        ));
        assert!(matches!(
            evaluate(&strict, &source, &target, &edge, &graph),
            PruningDecision::Transparent
        ));
    }
}
