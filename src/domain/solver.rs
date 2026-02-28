use crate::domain::edge::EdgeKind;
use crate::domain::graph::ContextGraph;
use crate::domain::node::{Node, NodeId};
use crate::domain::policy::{
    PruningDecision, PruningParams, evaluate_forward, should_explore_callers,
};
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;

/// How the current node was reached (for edge-aware pruning and reverse exploration).
#[derive(Debug, Clone)]
enum ReachedVia {
    Start,
    Forward(EdgeKind),
    /// Reached by following incoming Call edges (call-in exploration).
    CallIn,
    /// Reached by following incoming Write edges (shared-state write exploration).
    SharedStateWrite,
}

/// Single step in BFS traversal: node plus the edge/decision that led to it.
#[derive(Debug, Clone)]
pub struct TraversalStep {
    pub node_id: NodeId,
    pub incoming_edge_kind: Option<EdgeKind>,
    pub decision: Option<PruningDecision>,
}

/// CF computation result
#[derive(Debug, Clone)]
pub struct CfResult {
    pub reachable_set: HashSet<NodeId>,
    pub reachable_nodes_ordered: Vec<NodeId>,
    pub reachable_nodes_by_layer: Vec<Vec<NodeId>>,
    /// Traversal steps in BFS order: for each node, the edge kind and decision that led to it (None for start nodes).
    pub traversal_steps: Vec<TraversalStep>,
    pub total_context_size: u32,
}

/// CF Solver - computes Context-Footprint for a given node.
///
/// Holds graph and pruning params (doc_threshold + mode).
pub struct CfSolver {
    graph: Arc<ContextGraph>,
    params: PruningParams,
}

impl CfSolver {
    pub fn new(graph: Arc<ContextGraph>, params: PruningParams) -> Self {
        Self {
            graph,
            params,
        }
    }

    /// Compute CF for a given set of starting nodes (full result with layers, etc.).
    pub fn compute_cf(&self, starts: &[NodeIndex], max_tokens: Option<u32>) -> CfResult {
        let graph = self.graph.as_ref();
        let params = &self.params;
        // Build a reverse mapping once so neighbor ordering isn't O(|V|) per comparison.
        let mut idx_to_symbol: HashMap<NodeIndex, &str> =
            HashMap::with_capacity(graph.symbol_to_node.len());
        for (sym, &idx) in &graph.symbol_to_node {
            idx_to_symbol.insert(idx, sym.as_str());
        }

        let mut visited = HashSet::new();
        let mut ordered = Vec::new();
        let mut traversal_steps = Vec::new();
        let mut layers: Vec<Vec<NodeId>> = Vec::new();
        let mut queue: VecDeque<(NodeIndex, u32, ReachedVia, Option<PruningDecision>)> =
            VecDeque::new();
        let mut total_size = 0;

        for &start in starts {
            queue.push_back((start, 0, ReachedVia::Start, None));
        }

        while let Some((current, depth, reached_via, incoming_decision)) = queue.pop_front() {
            let current_node = graph.node(current);
            let current_id = current_node.core().id;

            if !visited.insert(current_id) {
                continue;
            }

            let node_size = current_node.core().context_size;
            total_size += node_size;
            let step_edge_kind = match &reached_via {
                ReachedVia::Forward(ek) => Some(ek.clone()),
                _ => None,
            };
            ordered.push(current_id);
            traversal_steps.push(TraversalStep {
                node_id: current_id,
                incoming_edge_kind: step_edge_kind,
                decision: incoming_decision,
            });

            // Add to layers
            while layers.len() <= depth as usize {
                layers.push(Vec::new());
            }
            layers[depth as usize].push(current_id);

            // Check if we exceeded max_tokens
            if let Some(limit) = max_tokens
                && total_size >= limit
            {
                break;
            }

            // === Stop exploring from CallIn nodes ===
            // If we reached this node just to understand how it calls something,
            // we only need its immediate context. We do not explore further from it.
            if matches!(reached_via, ReachedVia::CallIn) {
                continue;
            }

            // === Forward traversal: outgoing edges ===
            let mut out_edges: Vec<_> = graph.outgoing_edges(current).collect();
            out_edges.sort_by(|(a_idx, _), (b_idx, _)| {
                let a_sym = idx_to_symbol.get(a_idx).copied().unwrap_or("");
                let b_sym = idx_to_symbol.get(b_idx).copied().unwrap_or("");
                a_sym.cmp(b_sym)
            });

            for (neighbor, edge_kind) in out_edges {
                let neighbor_node = graph.node(neighbor);
                let neighbor_id = neighbor_node.core().id;
                let decision =
                    evaluate_forward(params, current_node, neighbor_node, edge_kind, graph);

                if matches!(decision, PruningDecision::Transparent) {
                    queue.push_back((
                        neighbor,
                        depth + 1,
                        ReachedVia::Forward(edge_kind.clone()),
                        Some(decision),
                    ));
                } else {
                    // Boundary: count as reached and include its size, but do not traverse
                    if !visited.contains(&neighbor_id) {
                        let b_size = neighbor_node.core().context_size;

                        // Check if adding boundary node exceeds limit
                        if let Some(limit) = max_tokens
                            && total_size + b_size > limit
                        {
                            break;
                        }

                        if visited.insert(neighbor_id) {
                            total_size += b_size;
                            ordered.push(neighbor_id);
                            traversal_steps.push(TraversalStep {
                                node_id: neighbor_id,
                                incoming_edge_kind: Some(edge_kind.clone()),
                                decision: Some(decision),
                            });

                            let b_depth = depth + 1;
                            while layers.len() <= b_depth as usize {
                                layers.push(Vec::new());
                            }
                            layers[b_depth as usize].push(neighbor_id);
                        }
                    }
                }
            }

            // === Reverse exploration: call-in (function) ===
            if let Node::Function(f) = current_node {
                let incoming_edge = match &reached_via {
                    ReachedVia::Forward(ek) => Some(ek),
                    _ => None,
                };
                if should_explore_callers(f, current, incoming_edge, params, graph) {
                    for (caller_idx, _) in graph.incoming_edges(current, Some(EdgeKind::Call)) {
                        let caller_id = graph.node(caller_idx).core().id;
                        if !visited.contains(&caller_id) {
                            queue.push_back((caller_idx, depth + 1, ReachedVia::CallIn, None));
                        }
                    }
                }
            }

            // === Reverse exploration: shared-state write (mutable variable reached via Read) ===
            if let Node::Variable(v) = current_node
                && v.mutability == crate::domain::node::Mutability::Mutable
                && matches!(reached_via, ReachedVia::Forward(EdgeKind::Read))
            {
                for (writer_idx, _) in graph.incoming_edges(current, Some(EdgeKind::Write)) {
                    let writer_id = graph.node(writer_idx).core().id;
                    if !visited.contains(&writer_id) {
                        queue.push_back((
                            writer_idx,
                            depth + 1,
                            ReachedVia::SharedStateWrite,
                            None,
                        ));
                    }
                }
            }

            if let Some(limit) = max_tokens
                && total_size >= limit
            {
                break;
            }
        }

        let result = CfResult {
            reachable_set: visited.clone(),
            reachable_nodes_ordered: ordered.clone(),
            reachable_nodes_by_layer: layers.clone(),
            traversal_steps,
            total_context_size: total_size,
        };

        result
    }

    /// Compute CF total context size for a single start node.
    /// Does not return traversal order / layers; ignores max_tokens.
    pub fn compute_cf_total(&self, start: NodeIndex) -> u32 {
        let graph = self.graph.as_ref();
        let params = &self.params;
        let node_count = graph.graph.node_count();
        let mut visited = vec![false; node_count];
        let mut reachable: Vec<NodeIndex> = Vec::new();
        let mut total_size: u32 = 0;

        let mut queue: VecDeque<(NodeIndex, ReachedVia)> = VecDeque::new();

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
        queue.push_back((start, ReachedVia::Start));

        while let Some((current, reached_via)) = queue.pop_front() {
            // === Stop exploring from CallIn nodes ===
            // If we reached this node just to understand how it calls something,
            // we only need its immediate context. We do not explore further from it.
            if matches!(reached_via, ReachedVia::CallIn) {
                continue;
            }

            let current_node = graph.node(current);

            for (neighbor, edge_kind) in graph.outgoing_edges(current) {
                let neighbor_pos = neighbor.index();
                if neighbor_pos < visited.len() && visited[neighbor_pos] {
                    continue;
                }

                let neighbor_node = graph.node(neighbor);
                let decision =
                    evaluate_forward(params, current_node, neighbor_node, edge_kind, graph);

                if matches!(decision, PruningDecision::Transparent) {
                    add_node(neighbor, &mut visited, &mut reachable, &mut total_size);
                    queue.push_back((neighbor, ReachedVia::Forward(edge_kind.clone())));
                } else {
                    add_node(neighbor, &mut visited, &mut reachable, &mut total_size);
                }
            }

            if let Node::Function(f) = current_node {
                let incoming_edge = match &reached_via {
                    ReachedVia::Forward(ek) => Some(ek),
                    _ => None,
                };
                if should_explore_callers(f, current, incoming_edge, params, graph) {
                    for (caller_idx, _) in graph.incoming_edges(current, Some(EdgeKind::Call)) {
                        let caller_pos = caller_idx.index();
                        if caller_pos < visited.len() && !visited[caller_pos] {
                            add_node(caller_idx, &mut visited, &mut reachable, &mut total_size);
                            queue.push_back((caller_idx, ReachedVia::CallIn));
                        }
                    }
                }
            }

            if let Node::Variable(v) = current_node
                && v.mutability == crate::domain::node::Mutability::Mutable
                && matches!(reached_via, ReachedVia::Forward(EdgeKind::Read))
            {
                for (writer_idx, _) in graph.incoming_edges(current, Some(EdgeKind::Write)) {
                    let writer_pos = writer_idx.index();
                    if writer_pos < visited.len() && !visited[writer_pos] {
                        add_node(writer_idx, &mut visited, &mut reachable, &mut total_size);
                        queue.push_back((writer_idx, ReachedVia::SharedStateWrite));
                    }
                }
            }
        }

        total_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::edge::EdgeKind;
    use crate::domain::graph::ContextGraph;
    use crate::domain::node::{FunctionNode, Node, NodeCore, SourceSpan, Visibility};
    use crate::domain::policy::PruningParams;
    use std::sync::Arc;

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
            return_types: vec![],
            is_interface_method: false,
            is_constructor: false,
            is_di_wired: false,
        })
    }

    fn test_var_node(id: u32, name: &str, mutability: crate::domain::node::Mutability) -> Node {
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
            1,
            span,
            0.5,
            false,
            "test.py".to_string(),
        );
        Node::Variable(crate::domain::node::VariableNode {
            core,
            var_type: None,
            mutability,
            variable_kind: crate::domain::node::VariableKind::Global,
        })
    }

    /// Node that qualifies as boundary under academic(0.5): sig complete + doc_score >= 0.5.
    fn test_node_boundary(id: u32, name: &str, context_size: u32) -> Node {
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
            0.8,
            false,
            "test.py".to_string(),
        );
        Node::Function(FunctionNode {
            core,
            parameters: vec![crate::domain::node::Parameter {
                name: "x".to_string(),
                param_type: Some("int#".to_string()),
                is_high_freedom_type: false,
            }],
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
            return_types: vec!["int#".to_string()],
            is_interface_method: false,
            is_constructor: false,
            is_di_wired: false,
        })
    }

    #[test]
    fn test_single_node_cf() {
        let mut graph = ContextGraph::new();
        let idx = graph.add_node("sym::a".into(), test_node(0, "a", 100));
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[idx], None);
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
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[a], None);
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
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[a], None);
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
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[a], None);
        assert_eq!(result.reachable_set.len(), 3);
        assert_eq!(result.total_context_size, 10 + 20 + 30);
    }

    #[test]
    fn test_boundary_stops_traversal() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node_boundary(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        let solver = CfSolver::new(Arc::new(graph), PruningParams::academic(0.5));
        let result = solver.compute_cf(&[a], None);
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
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[a], None);
        assert_eq!(result.reachable_set.len(), 3);
        assert_eq!(result.total_context_size, 60);
    }

    #[test]
    fn test_shared_state_write_expansion() {
        // Reader R reads mutable var V; W1 and W2 write to V. Reverse exploration from V follows incoming Write to W1, W2.
        let mut graph = ContextGraph::new();
        let r = graph.add_node("sym::r".into(), test_node(0, "r", 10));
        let v_span = crate::domain::node::SourceSpan {
            start_line: 0,
            start_column: 0,
            end_line: 1,
            end_column: 5,
        };
        let v_core = crate::domain::node::NodeCore::new(
            1,
            "v".to_string(),
            None,
            1,
            v_span,
            0.0,
            false,
            "test.py".to_string(),
        );
        let var = Node::Variable(crate::domain::node::VariableNode {
            core: v_core,
            var_type: Some("int#".to_string()),
            mutability: crate::domain::node::Mutability::Mutable,
            variable_kind: crate::domain::node::VariableKind::Global,
        });
        let var_idx = graph.add_node("sym::v".into(), var);
        let w1 = graph.add_node("sym::w1".into(), test_node(2, "w1", 20));
        let w2 = graph.add_node("sym::w2".into(), test_node(3, "w2", 30));
        graph.add_edge(r, var_idx, EdgeKind::Read);
        graph.add_edge(w1, var_idx, EdgeKind::Write);
        graph.add_edge(w2, var_idx, EdgeKind::Write);
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[r], None);
        assert_eq!(result.reachable_set.len(), 4); // r, v, w1, w2
        assert_eq!(result.total_context_size, 10 + 1 + 20 + 30);
    }

    #[test]
    fn test_call_in_expansion() {
        // Caller --Call--> Callee. Start at Callee; call-in exploration follows incoming Call to Caller.
        let mut graph = ContextGraph::new();
        let callee = graph.add_node("sym::callee".into(), test_node(0, "callee", 10));
        let caller = graph.add_node("sym::caller".into(), test_node(1, "caller", 25));
        let var = graph.add_node("sym::var".into(), test_var_node(2, "var", crate::domain::node::Mutability::Mutable));
        graph.add_edge(caller, callee, EdgeKind::Call);
        // Make callee impure so call-in happens
        graph.add_edge(callee, var, EdgeKind::Write);
        
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[callee], None);
        assert_eq!(result.reachable_set.len(), 3); // callee, var (forward), caller (call-in)
        assert_eq!(result.total_context_size, 10 + 25 + 1);
    }

    #[test]
    fn test_different_policies_different_results() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node_boundary(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        let graph_arc = Arc::new(graph);
        let solver_trans = CfSolver::new(Arc::clone(&graph_arc), PruningParams::strict(0.5));
        let solver_bound = CfSolver::new(graph_arc, PruningParams::academic(0.5));
        let res_trans = solver_trans.compute_cf(&[a], None);
        let res_bound = solver_bound.compute_cf(&[a], None);
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
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[a], None);
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
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[a], None);
        assert_eq!(result.reachable_set.len(), 3);
        assert_eq!(result.total_context_size, 60);
    }

    #[test]
    fn test_start_at_middle_of_chain() {
        // A -> B -> C. Start at B. B has incomplete spec so call-in exploration reaches A.
        // B's context_size (250) is above LEAF_UTILITY_SIZE_THRESHOLD so call-in is explored.
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node(1, "b", 250));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        let var = graph.add_node("sym::var".into(), test_var_node(3, "var", crate::domain::node::Mutability::Mutable));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);
        // Make b impure so call-in happens
        graph.add_edge(b, var, EdgeKind::Write);
        
        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));
        let result = solver.compute_cf(&[b], None);
        assert_eq!(result.reachable_set.len(), 4); // B, then C and var (forward), A (call-in)
        assert_eq!(result.total_context_size, 10 + 250 + 30 + 1);
    }

    #[test]
    fn test_boundary_node_still_in_reachable_set() {
        let mut graph = ContextGraph::new();
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node_boundary(1, "b", 99));
        graph.add_edge(a, b, EdgeKind::Call);
        let solver = CfSolver::new(Arc::new(graph), PruningParams::academic(0.5));
        let result = solver.compute_cf(&[a], None);
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

        let solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.5));

        // CF(A) = {A, C, D} = 10 + 30 + 40 = 80
        // CF(B) = {B, C, D} = 20 + 30 + 40 = 90
        // CF({A, B}) = {A, B, C, D} = 10 + 20 + 30 + 40 = 100
        let result = solver.compute_cf(&[a, b], None);

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

        let graph_arc = Arc::new(graph);
        let solver = CfSolver::new(Arc::clone(&graph_arc), PruningParams::strict(0.5));

        for idx in graph_arc.graph.node_indices() {
            let expected = solver.compute_cf(&[idx], None).total_context_size;
            let got = solver.compute_cf_total(idx);
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn test_cached_total_respects_boundary_semantics() {
        let mut graph = ContextGraph::new();
        // A -> B -> C; B is boundary (sig complete + doc), so from A only A and B count.
        let a = graph.add_node("sym::a".into(), test_node(0, "a", 10));
        let b = graph.add_node("sym::b".into(), test_node_boundary(1, "b", 20));
        let c = graph.add_node("sym::c".into(), test_node(2, "c", 30));
        graph.add_edge(a, b, EdgeKind::Call);
        graph.add_edge(b, c, EdgeKind::Call);

        let solver = CfSolver::new(Arc::new(graph), PruningParams::academic(0.5));

        let expected = solver.compute_cf(&[a], None).total_context_size;
        let got = solver.compute_cf_total(a);
        assert_eq!(got, expected);
        assert_eq!(got, 10 + 20);
    }
}
