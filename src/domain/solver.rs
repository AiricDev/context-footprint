use crate::domain::graph::ContextGraph;
use crate::domain::node::NodeId;
use crate::domain::policy::PruningPolicy;
use petgraph::graph::NodeIndex;
use std::collections::HashSet;
use std::collections::VecDeque;

/// CF computation result
#[derive(Debug, Clone)]
pub struct CfResult {
    pub reachable_set: HashSet<NodeId>,
    pub total_context_size: u32,
}

/// CF Solver - computes Context-Footprint for a given node
pub struct CfSolver;

impl CfSolver {
    pub fn new() -> Self {
        Self
    }
    
    /// Compute CF for a given starting node
    pub fn compute_cf(
        &self,
        graph: &ContextGraph,
        start: NodeIndex,
        policy: &dyn PruningPolicy,
    ) -> CfResult {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut total_size = 0;
        
        queue.push_back(start);
        
        while let Some(current) = queue.pop_front() {
            if !visited.insert(graph.node(current).core().id) {
                continue;
            }
            
            total_size += graph.node(current).core().context_size;
            
            for (neighbor, edge_kind) in graph.neighbors(current) {
                let source_node = graph.node(current);
                let neighbor_node = graph.node(neighbor);
                let decision = policy.evaluate(source_node, neighbor_node, edge_kind, graph);
                
                visited.insert(neighbor_node.core().id); // Boundary nodes also count
                
                if matches!(decision, crate::domain::policy::PruningDecision::Transparent) {
                    queue.push_back(neighbor); // Continue traversal
                }
            }
        }
        
        CfResult {
            reachable_set: visited,
            total_context_size: total_size,
        }
    }
}
