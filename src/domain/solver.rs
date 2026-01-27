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

impl Default for CfSolver {
    fn default() -> Self {
        Self::new()
    }
}

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
            let current_id = graph.node(current).core().id;
            if !visited.insert(current_id) {
                continue;
            }
            total_size += graph.node(current).core().context_size;

            for (neighbor, edge_kind) in graph.neighbors(current) {
                let source_node = graph.node(current);
                let neighbor_node = graph.node(neighbor);
                let neighbor_id = neighbor_node.core().id;
                let decision = policy.evaluate(source_node, neighbor_node, edge_kind, graph);

                if matches!(
                    decision,
                    crate::domain::policy::PruningDecision::Transparent
                ) {
                    queue.push_back(neighbor);
                } else {
                    // Boundary: count as reached and include its size, but do not traverse
                    if visited.insert(neighbor_id) {
                        total_size += neighbor_node.core().context_size;
                    }
                }
            }
        }

        CfResult {
            reachable_set: visited,
            total_context_size: total_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::edge::EdgeKind;
    use crate::domain::graph::ContextGraph;
    use crate::domain::node::{FunctionNode, Node, NodeCore, SourceSpan, Visibility};
    use crate::domain::policy::{PruningDecision, PruningPolicy};

    fn test_node(id: u32, name: &str, context_size: u32) -> Node {
        let span = SourceSpan {
            start_line: 0,
            start_column: 0,
            end_line: 1,
            end_column: 10,
        };
        let core = NodeCore::new(
            id,
            name.to_string(),
            None,
            context_size,
            span,
            0.5,
            false,
            "test.py".to_string(),
        );
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

    /// Policy that never stops traversal.
    struct AlwaysTransparent;

    impl PruningPolicy for AlwaysTransparent {
        fn evaluate(
            &self,
            _source: &Node,
            _target: &Node,
            _edge_kind: &EdgeKind,
            _graph: &ContextGraph,
        ) -> PruningDecision {
            PruningDecision::Transparent
        }
    }

    /// Policy that always stops (treats every target as boundary).
    struct AlwaysBoundary;

    impl PruningPolicy for AlwaysBoundary {
        fn evaluate(
            &self,
            _source: &Node,
            _target: &Node,
            _edge_kind: &EdgeKind,
            _graph: &ContextGraph,
        ) -> PruningDecision {
            PruningDecision::Boundary
        }
    }

    #[test]
    fn test_single_node_cf() {
        let mut graph = ContextGraph::new();
        let idx = graph.add_node("sym::a".into(), test_node(0, "a", 100));
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, idx, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 1);
        assert!(result.reachable_set.contains(&0));
        assert_eq!(result.total_context_size, 100);
    }

    #[test]
    fn test_linear_dependency_chain() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, a, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 3);
        assert_eq!(result.total_context_size, 10 + 20 + 30);
    }

    #[test]
    fn test_diamond_dependency() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        let d = graph.add_node("sym::d".into(), test_node(3, "d", 40));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(a, c, EdgeKind::Call);
        graph.add_edge(b, d, EdgeKind::Call);
        graph.add_edge(c, d, EdgeKind::Call);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, a, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 4);
        assert_eq!(result.total_context_size, 10 + 20 + 30 + 40);
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        graph.add_edge(c, a, EdgeKind::Call);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, a, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 3);
        assert_eq!(result.total_context_size, 10 + 20 + 30);
    }

    #[test]
    fn test_boundary_stops_traversal() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, a, &AlwaysBoundary);
        assert_eq!(result.reachable_set.len(), 2); // a and b; c is not traversed
        assert_eq!(result.total_context_size, 10 + 20); // a and b both count
    }

    #[test]
    fn test_transparent_node_continues() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, a, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 3);
        assert_eq!(result.total_context_size, 60);
    }

    #[test]
    fn test_shared_state_write_expansion() {
        let mut graph = ContextGraph::new();
        let r = graph.add_node("sym::r".into(), test_node(0, "r", 10));
        let w1 = graph.add_node("sym::w1".into(), test_node(1, "w1", 20));
        let w2 = graph.add_node("sym::w2".into(), test_node(2, "w2", 30));
        graph.add_edge(r, w1, EdgeKind::SharedStateWrite);
        graph.add_edge(r, w2, EdgeKind::SharedStateWrite);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, r, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 3);
        assert_eq!(result.total_context_size, 10 + 20 + 30);
    }

    #[test]
    fn test_call_in_expansion() {
        let mut graph = ContextGraph::new();
        let callee = graph.add_node("sym::callee".into(), test_node(0, "callee", 10));
        let caller = graph.add_node("sym::caller".into(), test_node(1, "caller", 25));
        graph.add_edge(callee, caller, EdgeKind::CallIn);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, callee, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 2);
        assert_eq!(result.total_context_size, 10 + 25);
    }

    #[test]
    fn test_different_policies_different_results() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        let solver = CfSolver::new();
        let res_trans = solver.compute_cf(&graph, a, &AlwaysTransparent);
        let res_bound = solver.compute_cf(&graph, a, &AlwaysBoundary);
        assert_eq!(res_trans.total_context_size, 60);
        assert_eq!(res_bound.total_context_size, 30); // a and b, c not traversed
        assert_eq!(res_trans.reachable_set.len(), 3);
        assert_eq!(res_bound.reachable_set.len(), 2);
    }

    #[test]
    fn test_disconnected_component_not_reached() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let _b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        graph.add_edge(a, a, EdgeKind::Call); // self-loop only, b is disconnected
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, a, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 1);
        assert_eq!(result.total_context_size, 10);
    }

    #[test]
    fn test_multiple_edges_from_same_source() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(a, c, EdgeKind::ParamType);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, a, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 3);
        assert_eq!(result.total_context_size, 60);
    }

    #[test]
    fn test_start_at_middle_of_chain() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, b, &AlwaysTransparent);
        assert_eq!(result.reachable_set.len(), 2);
        assert_eq!(result.total_context_size, 20 + 30);
    }

    #[test]
    fn test_boundary_node_still_in_reachable_set() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 99));
        graph.add_edge(a, b, EdgeKind::Call);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, a, &AlwaysBoundary);
        assert!(result.reachable_set.contains(&0));
        assert!(result.reachable_set.contains(&1));
        assert_eq!(result.total_context_size, 10 + 99);
    }
}
