use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;
use crate::domain::node::Node;
use crate::domain::policy::{PruningDecision, PruningPolicy};

/// Strict pruning policy
/// Only trusts external + interface with high threshold
pub struct StrictPolicy {
    doc_threshold: f32,
}

impl Default for StrictPolicy {
    fn default() -> Self {
        Self::new(0.8) // Higher threshold
    }
}

impl StrictPolicy {
    pub fn new(doc_threshold: f32) -> Self {
        Self { doc_threshold }
    }
}

impl PruningPolicy for StrictPolicy {
    fn evaluate(
        &self,
        source: &Node,
        target: &Node,
        edge_kind: &EdgeKind,
        _graph: &ContextGraph,
    ) -> PruningDecision {
        // Special handling for dynamic expansion edges
        match edge_kind {
            EdgeKind::SharedStateWrite => {
                return PruningDecision::Transparent;
            }
            EdgeKind::CallIn => {
                // In strict mode, we might still want to check the source
                if let Node::Function(f) = source {
                    let sig_complete = f.typed_param_count == f.param_count && f.has_return_type;
                    if sig_complete && f.core.doc_score >= self.doc_threshold {
                        return PruningDecision::Boundary;
                    }
                }
                return PruningDecision::Transparent;
            }
            _ => {}
        }

        // External dependencies are always boundaries
        if target.core().is_external {
            return PruningDecision::Boundary;
        }

        match target {
            Node::Type(t) => {
                // Only abstract types with very high doc score
                if t.is_abstract && t.core.doc_score >= self.doc_threshold {
                    PruningDecision::Boundary
                } else {
                    PruningDecision::Transparent
                }
            }
            Node::Function(_) => {
                // Functions are always transparent in strict mode
                PruningDecision::Transparent
            }
            Node::Variable(_) => PruningDecision::Transparent,
        }
    }

    fn doc_threshold(&self) -> f32 {
        self.doc_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::edge::EdgeKind;
    use crate::domain::graph::ContextGraph;
    use crate::domain::node::{
        FunctionNode, Mutability, Node, NodeCore, SourceSpan, TypeKind, TypeNode, VariableKind,
        VariableNode, Visibility,
    };

    fn make_core(id: u32, name: &str, doc_score: f32, is_external: bool) -> NodeCore {
        NodeCore::new(
            id,
            name.to_string(),
            None,
            10,
            SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 1,
                end_column: 5,
            },
            doc_score,
            is_external,
            "test.py".to_string(),
        )
    }

    fn well_documented_function() -> Node {
        let core = make_core(0, "f", 0.9, false);
        Node::Function(FunctionNode {
            core,
            param_count: 2,
            typed_param_count: 2,
            has_return_type: true,
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
        })
    }

    fn poorly_documented_function() -> Node {
        let core = make_core(0, "g", 0.0, false);
        Node::Function(FunctionNode {
            core,
            param_count: 1,
            typed_param_count: 0,
            has_return_type: false,
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
        })
    }

    fn abstract_type_high_doc() -> Node {
        let core = make_core(0, "I", 0.9, false);
        Node::Type(TypeNode {
            core,
            type_kind: TypeKind::Interface,
            is_abstract: true,
            type_param_count: 0,
        })
    }

    fn abstract_type_low_doc() -> Node {
        let core = make_core(0, "I", 0.6, false);
        Node::Type(TypeNode {
            core,
            type_kind: TypeKind::Interface,
            is_abstract: true,
            type_param_count: 0,
        })
    }

    fn concrete_type() -> Node {
        let core = make_core(0, "C", 0.9, false);
        Node::Type(TypeNode {
            core,
            type_kind: TypeKind::Class,
            is_abstract: false,
            type_param_count: 0,
        })
    }

    fn variable_node() -> Node {
        let core = make_core(0, "v", 1.0, false);
        Node::Variable(VariableNode {
            core,
            has_type_annotation: true,
            mutability: Mutability::Immutable,
            variable_kind: VariableKind::Global,
        })
    }

    fn external_node() -> Node {
        let core = make_core(0, "ext", 0.0, true);
        Node::Function(FunctionNode {
            core,
            param_count: 0,
            typed_param_count: 0,
            has_return_type: false,
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
        })
    }

    fn empty_graph() -> ContextGraph {
        ContextGraph::new()
    }

    #[test]
    fn test_external_node_is_boundary() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let target = external_node();
        let source = well_documented_function();
        let edge = EdgeKind::Call;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Boundary));
    }

    #[test]
    fn test_abstract_type_high_doc_is_boundary() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let target = abstract_type_high_doc();
        let source = well_documented_function();
        let edge = EdgeKind::ParamType;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Boundary));
    }

    #[test]
    fn test_abstract_type_low_doc_is_transparent() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let target = abstract_type_low_doc();
        let source = well_documented_function();
        let edge = EdgeKind::ParamType;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_concrete_type_is_transparent() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let target = concrete_type();
        let source = well_documented_function();
        let edge = EdgeKind::ParamType;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_function_always_transparent() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let target = well_documented_function();
        let source = poorly_documented_function();
        let edge = EdgeKind::Call;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_variable_always_transparent() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let target = variable_node();
        let source = well_documented_function();
        let edge = EdgeKind::Read;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_shared_state_write_always_transparent() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let source = well_documented_function();
        let target = well_documented_function();
        let edge = EdgeKind::SharedStateWrite;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_call_in_with_complete_signature_is_boundary() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let edge = EdgeKind::CallIn;
        let callee = well_documented_function();
        let caller = poorly_documented_function();
        let d = policy.evaluate(&callee, &caller, &edge, &graph);
        assert!(matches!(d, PruningDecision::Boundary));
    }

    #[test]
    fn test_call_in_with_incomplete_signature_is_transparent() {
        let policy = StrictPolicy::default();
        let graph = empty_graph();
        let edge = EdgeKind::CallIn;
        let callee = poorly_documented_function();
        let caller = well_documented_function();
        let d = policy.evaluate(&callee, &caller, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_doc_threshold_configurable() {
        assert!((StrictPolicy::default().doc_threshold() - 0.8).abs() < 0.001);
        assert!((StrictPolicy::new(0.5).doc_threshold() - 0.5).abs() < 0.001);
    }
}
