use crate::domain::node::Node;
use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;

/// Node type for documentation scoring
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Function,
    Variable,
    Type,
}

/// Pruning decision
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PruningDecision {
    Boundary,     // Stop traversal here; node is a valid abstraction
    Transparent,  // Continue traversal through this node
}

/// Pruning policy trait - determines if a node acts as a Context Boundary
pub trait PruningPolicy: Send + Sync {
    /// Evaluate if a node acts as a valid Context Boundary
    fn evaluate(&self, source: &Node, target: &Node, edge_kind: &EdgeKind, graph: &ContextGraph) -> PruningDecision;
    
    /// Documentation score threshold (exceeding this value is considered "sufficient documentation")
    fn doc_threshold(&self) -> f32 {
        0.5
    }
}

/// Size function trait - computes context size
pub trait SizeFunction: Send + Sync {
    /// Compute the context size for a given source code span
    fn compute(&self, source: &str, span: &SourceSpan) -> u32;
}

// SourceSpan is defined in node.rs
pub use crate::domain::node::SourceSpan;

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
    pub signature: Option<String>,  // Function signature
}
