use crate::domain::policy::{PruningPolicy, PruningDecision};
use crate::domain::node::Node;
use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;

/// Academic baseline pruning policy
/// Uses type completeness + documentation presence check
pub struct AcademicBaseline {
    doc_threshold: f32,
}

impl AcademicBaseline {
    pub fn new(doc_threshold: f32) -> Self {
        Self { doc_threshold }
    }
    
    pub fn default() -> Self {
        Self::new(0.5)
    }
}

impl PruningPolicy for AcademicBaseline {
    fn evaluate(&self, source: &Node, target: &Node, edge_kind: &EdgeKind, _graph: &ContextGraph) -> PruningDecision {
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
