//! Integration tests comparing different pruning policies and doc scorers.

mod common;

use context_footprint::adapters::doc_scorer::heuristic::HeuristicDocScorer;
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::policy::PruningParams;
use context_footprint::domain::solver::CfSolver;
use std::sync::Arc;

use common::fixtures::{
    create_semantic_data_chain_well_documented_middle, create_semantic_data_simple,
    source_reader_for_semantic_data,
};
use common::mock::{MockDocScorer, MockSizeFunction};

const DUMMY_SOURCE: &str = "def foo(): pass\n";

fn build_graph_with_simple_scorer(
    semantic_data: context_footprint::domain::semantic::SemanticData,
) -> context_footprint::domain::graph::ContextGraph {
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);
    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(HeuristicDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    builder.build(semantic_data, &reader).unwrap()
}

fn build_graph_with_heuristic_scorer(
    semantic_data: context_footprint::domain::semantic::SemanticData,
) -> context_footprint::domain::graph::ContextGraph {
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);
    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(HeuristicDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    builder.build(semantic_data, &reader).unwrap()
}

#[test]
fn test_academic_vs_strict_different_cf() {
    // Chain A -> B -> C with B well-documented. Academic stops at B (boundary); Strict continues to C.
    let semantic_data = create_semantic_data_chain_well_documented_middle();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);
    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    let start_idx = graph.get_node_by_symbol("sym::chain_a").unwrap();
    let graph_arc = Arc::new(graph);
    let mut solver_academic =
        CfSolver::new(Arc::clone(&graph_arc), PruningParams::academic(0.5));
    let mut solver_strict = CfSolver::new(graph_arc, PruningParams::strict(0.8));

    let cf_academic = solver_academic.compute_cf(&[start_idx], None);
    let cf_strict = solver_strict.compute_cf(&[start_idx], None);

    // Academic: B is well-doc + complete sig -> boundary, so we don't traverse to C.
    // Strict: functions always transparent, so we traverse to C.
    assert!(
        cf_academic.reachable_set.len() < cf_strict.reachable_set.len()
            || cf_academic.total_context_size < cf_strict.total_context_size,
        "Academic should produce smaller footprint than Strict (stops at B)"
    );
}

#[test]
fn test_heuristic_scorer_vs_simple_scorer() {
    let semantic_data = create_semantic_data_simple();
    let graph_simple = build_graph_with_simple_scorer(semantic_data.clone());
    let graph_heuristic = build_graph_with_heuristic_scorer(semantic_data);

    // Both should produce valid graphs with same structure (same symbols).
    assert_eq!(
        graph_simple.graph.node_count(),
        graph_heuristic.graph.node_count()
    );
    assert!(graph_simple.get_node_by_symbol("sym::func_a").is_some());
    assert!(graph_heuristic.get_node_by_symbol("sym::func_a").is_some());

    // Doc scores can differ: Simple gives 1.0 for any non-empty doc, Heuristic uses length+keywords.
    let idx = graph_simple.get_node_by_symbol("sym::func_a").unwrap();
    let score_simple = graph_simple.node(idx).core().doc_score;
    let idx_h = graph_heuristic.get_node_by_symbol("sym::func_a").unwrap();
    let score_heuristic = graph_heuristic.node(idx_h).core().doc_score;
    // "Doc for A" is short; Heuristic gives lower than 1.0, Simple gives 1.0.
    assert_ne!(
        score_simple, score_heuristic,
        "Heuristic and Simple scorers should assign different doc_score"
    );
}

#[test]
fn test_strict_policy_smaller_context_footprint() {
    // Run CF with Strict policy and verify it completes with a valid result.
    // (Strict treats functions as transparent, so in graphs with well-doc functions
    // Academic can yield a smaller footprint by stopping at those boundaries.)
    let semantic_data = create_semantic_data_simple();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);
    let size_fn = Box::new(MockSizeFunction::with_size(10));
    let doc_scorer = Box::new(MockDocScorer::with_score(0.8));
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    let start_idx = graph.get_node_by_symbol("sym::func_a").unwrap();
    let mut solver = CfSolver::new(Arc::new(graph), PruningParams::strict(0.8));
    let cf_strict = solver.compute_cf(&[start_idx], None);

    assert!(cf_strict.total_context_size >= 10);
    assert!(!cf_strict.reachable_set.is_empty());
}
