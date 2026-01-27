use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;
use crate::domain::node::Node;
use crate::domain::policy::{PruningDecision, PruningPolicy};

/// Academic baseline pruning policy
/// Uses type completeness + documentation presence check
pub struct AcademicBaseline {
    doc_threshold: f32,
}

impl Default for AcademicBaseline {
    fn default() -> Self {
        Self::new(0.5)
    }
}

impl AcademicBaseline {
    pub fn new(doc_threshold: f32) -> Self {
        Self { doc_threshold }
    }
}

impl PruningPolicy for AcademicBaseline {
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
                // Shared-state write edges always traverse (no boundary stops them)
                return PruningDecision::Transparent;
            }
            EdgeKind::CallIn => {
                // Call-in edges traverse only when source lacks complete specification
                if let Node::Function(f) = source {
                    let sig_complete = f.typed_param_count == f.param_count && f.has_return_type;
                    if sig_complete && f.core.doc_score >= self.doc_threshold {
                        return PruningDecision::Boundary;
                    } else {
                        return PruningDecision::Transparent;
                    }
                }
                // If not a function, something is weird, but let's be conservative
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
                // Type boundary: must be abstract (interface/protocol) and well-documented
                if t.is_abstract && t.core.doc_score >= self.doc_threshold {
                    PruningDecision::Boundary
                } else {
                    PruningDecision::Transparent
                }
            }
            Node::Function(f) => {
                // Function boundary: signature complete and well-documented
                let sig_complete = f.typed_param_count == f.param_count && f.has_return_type;
                if sig_complete && f.core.doc_score >= self.doc_threshold {
                    PruningDecision::Boundary
                } else {
                    PruningDecision::Transparent
                }
            }
            Node::Variable(_) => {
                // Variables are always transparent (need to see type definition)
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
        let core = make_core(0, "f", 0.8, false);
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

    fn abstract_type_with_doc() -> Node {
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
        let policy = AcademicBaseline::default();
        let graph = empty_graph();
        let target = external_node();
        let source = well_documented_function();
        let edge = EdgeKind::Call;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Boundary));
    }

    #[test]
    fn test_well_documented_function_is_boundary() {
        let policy = AcademicBaseline::default();
        let graph = empty_graph();
        let target = well_documented_function();
        let source = poorly_documented_function();
        let edge = EdgeKind::Call;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Boundary));
    }

    #[test]
    fn test_poorly_documented_function_is_transparent() {
        let policy = AcademicBaseline::default();
        let graph = empty_graph();
        let target = poorly_documented_function();
        let source = well_documented_function();
        let edge = EdgeKind::Call;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_abstract_type_with_doc_is_boundary() {
        let policy = AcademicBaseline::default();
        let graph = empty_graph();
        let target = abstract_type_with_doc();
        let source = well_documented_function();
        let edge = EdgeKind::ParamType;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Boundary));
    }

    #[test]
    fn test_concrete_type_is_transparent() {
        let policy = AcademicBaseline::default();
        let graph = empty_graph();
        let target = concrete_type();
        let source = well_documented_function();
        let edge = EdgeKind::ParamType;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_variable_always_transparent() {
        let policy = AcademicBaseline::default();
        let graph = empty_graph();
        let target = variable_node();
        let source = well_documented_function();
        let edge = EdgeKind::Read;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_shared_state_write_always_transparent() {
        let policy = AcademicBaseline::default();
        let graph = empty_graph();
        let source = well_documented_function();
        let target = well_documented_function();
        let edge = EdgeKind::SharedStateWrite;
        let d = policy.evaluate(&source, &target, &edge, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }

    #[test]
    fn test_call_in_depends_on_signature_completeness() {
        let policy = AcademicBaseline::default();
        let graph = empty_graph();
        let edge = EdgeKind::CallIn;
        let well = well_documented_function();
        let poor = poorly_documented_function();
        let d_well = policy.evaluate(&well, &poor, &edge, &graph);
        let d_poor = policy.evaluate(&poor, &well, &edge, &graph);
        assert!(matches!(d_well, PruningDecision::Boundary));
        assert!(matches!(d_poor, PruningDecision::Transparent));
    }

    #[test]
    fn test_doc_threshold_below_returns_transparent() {
        let policy = AcademicBaseline::new(0.9);
        let mut target = well_documented_function();
        target.core_mut().doc_score = 0.6;
        let graph = empty_graph();
        let source = poorly_documented_function();
        let d = policy.evaluate(&source, &target, &EdgeKind::Call, &graph);
        assert!(matches!(d, PruningDecision::Transparent));
    }
}
