use crate::domain::graph::ContextGraph;
use crate::domain::node::NodeId;
use crate::domain::policy::PruningPolicy;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

/// CF computation result
#[derive(Debug, Clone)]
pub struct CfResult {
    pub reachable_set: HashSet<NodeId>,
    pub reachable_nodes_ordered: Vec<NodeId>,
    pub reachable_nodes_by_layer: Vec<Vec<NodeId>>,
    pub total_context_size: u32,
}

/// CF Solver - computes Context-Footprint for a given node
pub struct CfSolver;

/// Memoization cache for CF totals (and reachable sets) per start node.
///
/// This is intended for batch-style use cases like computing CF for every node (stats / ranking),
/// where repeatedly traversing overlapping subgraphs is expensive.
///
/// Notes:
/// - The cache is **policy-dependent**. Do not reuse a `CfMemo` across different policies.
/// - The cached reachable sets follow the same semantics as `compute_cf` when `max_tokens=None`:
///   boundary nodes are included but not expanded.
#[derive(Debug, Default)]
pub struct CfMemo {
    reachable: HashMap<NodeIndex, Vec<NodeIndex>>,
    total_context_size: HashMap<NodeIndex, u32>,
}

impl CfMemo {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for CfSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl CfSolver {
    pub fn new() -> Self {
        Self
    }

    /// Compute CF for a given set of starting nodes
    pub fn compute_cf(
        &self,
        graph: &ContextGraph,
        starts: &[NodeIndex],
        policy: &dyn PruningPolicy,
        max_tokens: Option<u32>,
    ) -> CfResult {
        // Build a reverse mapping once so neighbor ordering isn't O(|V|) per comparison.
        let mut idx_to_symbol: HashMap<NodeIndex, &str> =
            HashMap::with_capacity(graph.symbol_to_node.len());
        for (sym, &idx) in &graph.symbol_to_node {
            idx_to_symbol.insert(idx, sym.as_str());
        }

        let mut visited = HashSet::new();
        let mut ordered = Vec::new();
        let mut layers: Vec<Vec<NodeId>> = Vec::new();
        let mut queue = VecDeque::new();
        let mut total_size = 0;

        // queue stores (node_index, depth)
        for &start in starts {
            queue.push_back((start, 0));
        }

        while let Some((current, depth)) = queue.pop_front() {
            let current_node = graph.node(current);
            let current_id = current_node.core().id;

            if !visited.insert(current_id) {
                continue;
            }

            let node_size = current_node.core().context_size;
            total_size += node_size;
            ordered.push(current_id);

            // Add to layers
            while layers.len() <= depth {
                layers.push(Vec::new());
            }
            layers[depth].push(current_id);

            // Check if we exceeded max_tokens
            if let Some(limit) = max_tokens
                && total_size >= limit
            {
                break;
            }

            // Get neighbors and sort them by symbol for deterministic traversal
            let mut neighbors: Vec<_> = graph.neighbors(current).collect();
            neighbors.sort_by(|(a_idx, _), (b_idx, _)| {
                let a_sym = idx_to_symbol.get(a_idx).copied().unwrap_or("");
                let b_sym = idx_to_symbol.get(b_idx).copied().unwrap_or("");
                a_sym.cmp(b_sym)
            });

            for (neighbor, edge_kind) in neighbors {
                let neighbor_node = graph.node(neighbor);
                let neighbor_id = neighbor_node.core().id;
                let decision = policy.evaluate(current_node, neighbor_node, edge_kind, graph);

                if matches!(
                    decision,
                    crate::domain::policy::PruningDecision::Transparent
                ) {
                    queue.push_back((neighbor, depth + 1));
                } else {
                    // Boundary: count as reached and include its size, but do not traverse
                    if !visited.contains(&neighbor_id) {
                        let b_size = neighbor_node.core().context_size;

                        // Check if adding boundary node exceeds limit
                        if let Some(limit) = max_tokens
                            && total_size + b_size > limit
                        {
                            continue;
                        }

                        if visited.insert(neighbor_id) {
                            total_size += b_size;
                            ordered.push(neighbor_id);

                            let b_depth = depth + 1;
                            while layers.len() <= b_depth {
                                layers.push(Vec::new());
                            }
                            layers[b_depth].push(neighbor_id);
                        }
                    }
                }

                // Sub-early check after adding boundary
                if let Some(limit) = max_tokens
                    && total_size >= limit
                {
                    break;
                }
            }

            if let Some(limit) = max_tokens
                && total_size >= limit
            {
                break;
            }
        }

        CfResult {
            reachable_set: visited,
            reachable_nodes_ordered: ordered,
            reachable_nodes_by_layer: layers,
            total_context_size: total_size,
        }
    }

    /// Compute CF total context size for a single start node, using memoization.
    ///
    /// This method is optimized for computing CF for many nodes under the same policy.
    /// It intentionally does **not** return traversal order / layers, and it ignores `max_tokens`.
    pub fn compute_cf_total_context_size_cached(
        &self,
        graph: &ContextGraph,
        start: NodeIndex,
        policy: &dyn PruningPolicy,
        memo: &mut CfMemo,
    ) -> u32 {
        if let Some(&cached) = memo.total_context_size.get(&start) {
            return cached;
        }

        // Per-call visited set, keyed by NodeIndex::index().
        // We keep an incrementally-updated total and a reachable list so we never have to scan
        // the whole bitset at the end.
        let node_count = graph.graph.node_count();
        let mut visited = vec![false; node_count];
        let mut reachable: Vec<NodeIndex> = Vec::new();
        let mut total_size: u32 = 0;

        let mut queue: VecDeque<NodeIndex> = VecDeque::new();

        // Helper to add a node exactly once (and keep totals in sync).
        let add_node = |idx: NodeIndex,
                        visited: &mut [bool],
                        reachable: &mut Vec<NodeIndex>,
                        total_size: &mut u32| {
            let pos = idx.index();
            if pos >= visited.len() {
                return;
            }
            if !visited[pos] {
                visited[pos] = true;
                *total_size = total_size.saturating_add(graph.node(idx).core().context_size);
                reachable.push(idx);
            }
        };

        add_node(start, &mut visited, &mut reachable, &mut total_size);
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            let current_node = graph.node(current);

            // For totals we don't need deterministic ordering; skip neighbor sorting to save time.
            for (neighbor, edge_kind) in graph.neighbors(current) {
                let neighbor_pos = neighbor.index();
                if neighbor_pos < visited.len() && visited[neighbor_pos] {
                    continue;
                }

                let neighbor_node = graph.node(neighbor);
                let decision = policy.evaluate(current_node, neighbor_node, edge_kind, graph);

                if matches!(
                    decision,
                    crate::domain::policy::PruningDecision::Transparent
                ) {
                    if let Some(cached_set) = memo.reachable.get(&neighbor) {
                        // Reuse cached reachable set when we are allowed to expand `neighbor`.
                        for &idx in cached_set {
                            add_node(idx, &mut visited, &mut reachable, &mut total_size);
                        }
                    } else {
                        // Expand normally (BFS) and cache later.
                        add_node(neighbor, &mut visited, &mut reachable, &mut total_size);
                        queue.push_back(neighbor);
                    }
                } else {
                    // Boundary: include its size, but do not traverse through it.
                    add_node(neighbor, &mut visited, &mut reachable, &mut total_size);
                }
            }
        }

        memo.reachable.insert(start, reachable);
        memo.total_context_size.insert(start, total_size);
        total_size
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
            parameters: Vec::new(),
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_type: None,
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
        let result = solver.compute_cf(&graph, &[idx], &AlwaysTransparent, None);
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
        let result = solver.compute_cf(&graph, &[a], &AlwaysTransparent, None);
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
        let result = solver.compute_cf(&graph, &[a], &AlwaysTransparent, None);
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
        let result = solver.compute_cf(&graph, &[a], &AlwaysTransparent, None);
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
        let result = solver.compute_cf(&graph, &[a], &AlwaysBoundary, None);
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
        let result = solver.compute_cf(&graph, &[a], &AlwaysTransparent, None);
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
        let result = solver.compute_cf(&graph, &[r], &AlwaysTransparent, None);
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
        let result = solver.compute_cf(&graph, &[callee], &AlwaysTransparent, None);
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
        let res_trans = solver.compute_cf(&graph, &[a], &AlwaysTransparent, None);
        let res_bound = solver.compute_cf(&graph, &[a], &AlwaysBoundary, None);
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
        let result = solver.compute_cf(&graph, &[a], &AlwaysTransparent, None);
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
        graph.add_edge(a, c, EdgeKind::Call);
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, &[a], &AlwaysTransparent, None);
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
        let result = solver.compute_cf(&graph, &[b], &AlwaysTransparent, None);
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
        let result = solver.compute_cf(&graph, &[a], &AlwaysBoundary, None);
        assert!(result.reachable_set.contains(&0));
        assert!(result.reachable_set.contains(&1));
        assert_eq!(result.total_context_size, 10 + 99);
    }

    #[test]
    fn test_multi_node_union_cf() {
        let mut graph = ContextGraph::new();
        // A -> C, B -> C, C -> D. E is independent.
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        let d = graph.add_node("sym::d".into(), test_node(3, "d", 40));
        let _e = graph.add_node("sym::e".into(), test_node(4, "e", 50));

        graph.add_edge(a, c, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        graph.add_edge(c, d, EdgeKind::Call);

        let solver = CfSolver::new();
        let policy = AlwaysTransparent;

        // CF(A) = {A, C, D} = 10 + 30 + 40 = 80
        // CF(B) = {B, C, D} = 20 + 30 + 40 = 90
        // CF({A, B}) = {A, B, C, D} = 10 + 20 + 30 + 40 = 100
        let result = solver.compute_cf(&graph, &[a, b], &policy, None);

        assert_eq!(result.reachable_set.len(), 4);
        assert!(result.reachable_set.contains(&0)); // A
        assert!(result.reachable_set.contains(&1)); // B
        assert!(result.reachable_set.contains(&2)); // C
        assert!(result.reachable_set.contains(&3)); // D
        assert!(!result.reachable_set.contains(&4)); // E (not reachable)
        assert_eq!(result.total_context_size, 100);

        // Verify layers
        assert_eq!(result.reachable_nodes_by_layer[0].len(), 2); // {A, B}
        assert!(result.reachable_nodes_by_layer[0].contains(&0));
        assert!(result.reachable_nodes_by_layer[0].contains(&1));
        assert_eq!(result.reachable_nodes_by_layer[1].len(), 1); // {C}
        assert_eq!(result.reachable_nodes_by_layer[1][0], 2);
        assert_eq!(result.reachable_nodes_by_layer[2].len(), 1); // {D}
        assert_eq!(result.reachable_nodes_by_layer[2][0], 3);
    }

    #[test]
    fn test_cached_total_matches_compute_cf_for_each_node() {
        let mut graph = ContextGraph::new();
        // A -> B -> C, and A -> D (diamond-ish), plus a cycle E <-> F.
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        let d = graph.add_node("sym::d".into(), test_node(3, "d", 40));
        let e = graph.add_node("sym::e".into(), test_node(4, "e", 5));
        let f = graph.add_node("sym::f".into(), test_node(5, "f", 6));

        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        graph.add_edge(a, d, EdgeKind::Call);
        graph.add_edge(c, d, EdgeKind::Call);
        graph.add_edge(e, f, EdgeKind::Call);
        graph.add_edge(f, e, EdgeKind::Call);

        let solver = CfSolver::new();
        let policy = AlwaysTransparent;
        let mut memo = CfMemo::new();

        for idx in graph.graph.node_indices() {
            let expected = solver
                .compute_cf(&graph, &[idx], &policy, None)
                .total_context_size;
            let got = solver.compute_cf_total_context_size_cached(&graph, idx, &policy, &mut memo);
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn test_cached_total_respects_boundary_semantics() {
        let mut graph = ContextGraph::new();
        // A -> B -> C, but policy makes everything boundary, so only A and B count from A.
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);

        let solver = CfSolver::new();
        let policy = AlwaysBoundary;
        let mut memo = CfMemo::new();

        let expected = solver
            .compute_cf(&graph, &[a], &policy, None)
            .total_context_size;
        let got = solver.compute_cf_total_context_size_cached(&graph, a, &policy, &mut memo);
        assert_eq!(got, expected);
        assert_eq!(got, 10 + 20);
    }
}
