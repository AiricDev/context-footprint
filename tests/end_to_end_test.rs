//! End-to-end tests: build graph from SemanticData (fixture or JSON), compute CF.

mod common;

use std::sync::Arc;

use context_footprint::adapters::doc_scorer::heuristic::HeuristicDocScorer;
use context_footprint::adapters::size_function::tiktoken::TiktokenSizeFunction;
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::policy::PruningParams;
use context_footprint::domain::solver::CfSolver;

use common::fixtures::{create_semantic_data_two_files, source_reader_for_semantic_data};

/// Build graph from fixture SemanticData, then compute CF for a symbol.
#[test]
fn test_build_from_semantic_data_and_compute_cf() {
    let semantic_data = create_semantic_data_two_files();
    let reader = source_reader_for_semantic_data(&semantic_data, "def foo(): pass\n");

    let size_fn = Box::new(TiktokenSizeFunction::new());
    let doc_scorer = Box::new(HeuristicDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);

    let graph = builder.build(semantic_data, &reader).expect("build graph");
    assert!(graph.graph.node_count() >= 1);
    assert!(!graph.symbol_to_node.is_empty());

    let first_symbol = graph.symbol_to_node.keys().next().unwrap().clone();
    let start = graph.get_node_by_symbol(&first_symbol).unwrap();
    let expected_min = graph.node(start).core().context_size;
    let mut solver = CfSolver::new(Arc::new(graph), PruningParams::academic(0.5));
    let result = solver.compute_cf(&[start], None);

    assert!(!result.reachable_set.is_empty());
    assert!(result.total_context_size >= expected_min);
}
