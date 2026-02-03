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

    /// Check if a function is an "abstract factory" - returns an abstract type
    /// This identifies the Abstract Factory design pattern where a function/method
    /// returns an interface/protocol, hiding concrete implementation details
    fn is_abstract_factory(&self, function_node: &Node, graph: &ContextGraph) -> bool {
        if let Node::Function(f) = function_node {
            // Check if function has a return type annotation
            if let Some(return_type_id) = f.return_type_id() {
                // Look up the return type in the TypeRegistry
                if let Some(type_info) = graph.type_registry.get(return_type_id) {
                    // If return type is an abstract type (Protocol/Interface/Trait)
                    // with sufficient documentation, this is an abstract factory
                    if type_info.definition.is_abstract && type_info.doc_score >= self.doc_threshold
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

impl PruningPolicy for AcademicBaseline {
    fn evaluate(
        &self,
        source: &Node,
        target: &Node,
        edge_kind: &EdgeKind,
        graph: &ContextGraph,
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
                    let sig_complete = f.is_signature_complete();
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
            Node::Variable(_) => {
                // Regular variables are always transparent
                // (Types are no longer in the graph, they are in TypeRegistry)
                PruningDecision::Transparent
            }
            Node::Function(f) => {
                // Check if this function is an "abstract factory"
                // (returns an abstract type, following the Abstract Factory pattern)
                if self.is_abstract_factory(target, graph) {
                    // Abstract factories are boundaries - we care about the interface they return,
                    // not the concrete implementation details inside
                    return PruningDecision::Boundary;
                }

                // Function boundary: signature complete and well-documented
                let sig_complete = f.is_signature_complete();
                if sig_complete && f.core.doc_score >= self.doc_threshold {
                    PruningDecision::Boundary
                } else {
                    PruningDecision::Transparent
                }
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
        let core = make_core(0, "f", 0.8, false);
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

    #[test]
    fn test_abstract_factory_is_boundary() {
        let policy = AcademicBaseline::default();

        // Create a factory function that returns an abstract type
        let factory_core = make_core(0, "get_service", 0.8, false);
        let factory = Node::Function(FunctionNode {
            core: factory_core,
            parameters: Vec::new(),
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_type: Some("ServicePort#".to_string()),
        });

        // Register abstract type in TypeRegistry
        let mut graph = ContextGraph::new();
        graph.type_registry.register(
            "ServicePort#".to_string(),
            crate::domain::type_registry::TypeInfo {
                definition: crate::domain::type_registry::TypeDefAttribute {
                    type_kind: crate::domain::type_registry::TypeKind::Interface,
                    is_abstract: true,
                    type_param_count: 0,
                },
                context_size: 100,
                doc_score: 0.9,
            },
        );

        // Test: factory function should be recognized as boundary (abstract factory pattern)
        let caller = poorly_documented_function();
        let d = policy.evaluate(&caller, &factory, &EdgeKind::Call, &graph);
        assert!(matches!(d, PruningDecision::Boundary));
    }

    #[test]
    fn test_poorly_documented_factory_returning_abstract_becomes_boundary() {
        let policy = AcademicBaseline::default();

        // Create a factory function with incomplete docs that returns abstract type
        let factory_core = make_core(0, "get_service", 0.3, false);
        let factory = Node::Function(FunctionNode {
            core: factory_core,
            parameters: Vec::new(),
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_type: Some("ServicePort#".to_string()),
        });

        // Register abstract type in TypeRegistry
        let mut graph = ContextGraph::new();
        graph.type_registry.register(
            "ServicePort#".to_string(),
            crate::domain::type_registry::TypeInfo {
                definition: crate::domain::type_registry::TypeDefAttribute {
                    type_kind: crate::domain::type_registry::TypeKind::Interface,
                    is_abstract: true,
                    type_param_count: 0,
                },
                context_size: 100,
                doc_score: 0.9,
            },
        );

        // Test: Even though factory has poor docs, it becomes Boundary
        // because it returns an abstract type (abstract factory pattern)
        let caller = well_documented_function();
        let d = policy.evaluate(&caller, &factory, &EdgeKind::Call, &graph);
        assert!(matches!(d, PruningDecision::Boundary));
    }
}
