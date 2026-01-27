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
