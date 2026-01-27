//! GraphBuilder integration tests using mock data and fixtures.

mod common;

use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::edge::EdgeKind;

use common::fixtures::{
    create_semantic_data_simple, create_semantic_data_two_files, create_semantic_data_with_cycle,
    create_semantic_data_with_shared_state, source_reader_for_semantic_data,
};
use common::mock::{MockDocScorer, MockSizeFunction};

const DUMMY_SOURCE: &str = "def foo(): pass\n";

#[test]
fn test_build_graph_from_semantic_data_simple() {
    let semantic_data = create_semantic_data_simple();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    assert_eq!(graph.graph.node_count(), 2);
    assert!(graph.graph.edge_count() >= 1);
    assert!(graph.get_node_by_symbol("sym::func_a").is_some());
    assert!(graph.get_node_by_symbol("sym::func_b").is_some());
}

#[test]
fn test_build_graph_two_files() {
    let semantic_data = create_semantic_data_two_files();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    assert_eq!(graph.graph.node_count(), 2);
    assert!(graph.graph.edge_count() >= 1);
}

#[test]
fn test_three_pass_creates_nodes_then_edges() {
    let semantic_data = create_semantic_data_simple();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::with_size(5));
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    assert_eq!(graph.graph.node_count(), 2, "Pass 1: two nodes");
    assert!(graph.graph.edge_count() >= 1, "Pass 2/3: at least one edge");
    for node in graph.graph.node_weights() {
        assert_eq!(node.core().context_size, 5, "SizeFunction applied");
    }
}

#[test]
fn test_cycle_fixture_produces_cycle_edges() {
    let semantic_data = create_semantic_data_with_cycle();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    assert_eq!(graph.graph.node_count(), 3);
    assert!(graph.graph.edge_count() >= 3);
}

#[test]
fn test_shared_state_fixture_produces_shared_state_write_edges() {
    let semantic_data = create_semantic_data_with_shared_state();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    assert_eq!(graph.graph.node_count(), 4);
    let has_shared_state_write = graph
        .graph
        .edge_references()
        .any(|e| matches!(e.weight(), EdgeKind::SharedStateWrite));
    assert!(has_shared_state_write);
}
