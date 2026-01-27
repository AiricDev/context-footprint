use crate::domain::node::Node;
use crate::domain::edge::EdgeKind;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

/// Symbol identifier (SCIP symbol string)
pub type SymbolId = String;

/// Context Graph - the core data structure
pub struct ContextGraph {
    /// The directed graph of nodes and edges
    pub graph: DiGraph<Node, EdgeKind>,
    
    /// Mapping from symbol to node index
    pub symbol_to_node: HashMap<SymbolId, NodeIndex>,
}

impl ContextGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            symbol_to_node: HashMap::new(),
        }
    }
    
    pub fn add_node(&mut self, symbol: SymbolId, node: Node) -> NodeIndex {
        let idx = self.graph.add_node(node);
        self.symbol_to_node.insert(symbol, idx);
        idx
    }
    
    pub fn add_edge(&mut self, source: NodeIndex, target: NodeIndex, kind: EdgeKind) {
        self.graph.add_edge(source, target, kind);
    }
    
    pub fn get_node_by_symbol(&self, symbol: &str) -> Option<NodeIndex> {
        self.symbol_to_node.get(symbol).copied()
    }
    
    pub fn node(&self, idx: NodeIndex) -> &Node {
        &self.graph[idx]
    }
    
    pub fn neighbors(&self, idx: NodeIndex) -> impl Iterator<Item = (NodeIndex, &EdgeKind)> {
        self.graph.neighbors_directed(idx, petgraph::Direction::Outgoing)
            .map(move |neighbor| {
                let edge = self.graph.find_edge(idx, neighbor).unwrap();
                (neighbor, self.graph.edge_weight(edge).unwrap())
            })
    }
}
