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
                    let sig_complete = f.is_signature_complete();
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
            Node::Variable(_) => {
                // Regular variables are always transparent
                // (Types are no longer in the graph, they are in TypeRegistry)
                PruningDecision::Transparent
            }
            Node::Function(_) => {
                // Functions are always transparent in strict mode
                PruningDecision::Transparent
            }
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
        FunctionNode, Mutability, Node, NodeCore, SourceSpan, VariableKind, VariableNode,
        Visibility,
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
            parameters: vec![
                crate::domain::node::Parameter {
                    name: "x".to_string(),
                    param_type: Some("int#".to_string()),
                },
                crate::domain::node::Parameter {
                    name: "y".to_string(),
                    param_type: Some("int#".to_string()),
                },
            ],
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_type: Some("int#".to_string()),
        })
    }

    fn poorly_documented_function() -> Node {
        let core = make_core(0, "g", 0.0, false);
        Node::Function(FunctionNode {
            core,
            parameters: vec![crate::domain::node::Parameter {
                name: "x".to_string(),
                param_type: None,
            }],
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_type: None,
        })
    }

    fn variable_node() -> Node {
        let core = make_core(0, "v", 1.0, false);
        Node::Variable(VariableNode {
            core,
            var_type: None,
            mutability: Mutability::Immutable,
            variable_kind: VariableKind::Global,
        })
    }

    fn external_node() -> Node {
        let core = make_core(0, "ext", 0.0, true);
        Node::Function(FunctionNode {
            core,
            parameters: Vec::new(),
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_type: None,
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
