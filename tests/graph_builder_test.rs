//! GraphBuilder integration tests using mock data and fixtures.

mod common;

use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::edge::EdgeKind;
use petgraph::visit::EdgeRef;

use common::fixtures::{
    create_semantic_data_empty_document, create_semantic_data_multiple_callers,
    create_semantic_data_simple, create_semantic_data_two_files,
    create_semantic_data_with_constructor_call, create_semantic_data_with_cycle,
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
fn test_shared_state_fixture_produces_read_and_write_edges() {
    // Builder produces Read (reader->var) and Write (w1->var, w2->var). Reverse exploration at query time follows incoming Write from var.
    let semantic_data = create_semantic_data_with_shared_state();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    assert_eq!(graph.graph.node_count(), 4);
    let read_count = graph
        .graph
        .edge_references()
        .filter(|e| matches!(e.weight(), EdgeKind::Read))
        .count();
    let write_count = graph
        .graph
        .edge_references()
        .filter(|e| matches!(e.weight(), EdgeKind::Write))
        .count();
    assert!(read_count >= 1, "Reader should have Read edge to variable");
    assert_eq!(
        write_count, 2,
        "W1 and W2 should have Write edges to variable"
    );
}

#[test]
fn test_empty_document_produces_no_nodes() {
    let semantic_data = create_semantic_data_empty_document();
    let reader = source_reader_for_semantic_data(&semantic_data, "");

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    assert_eq!(graph.graph.node_count(), 0);
    assert_eq!(graph.graph.edge_count(), 0);
}

#[test]
fn test_multiple_writers_all_connected_via_write_edges() {
    // Builder produces Write edges from W1 and W2 to the shared variable. CF query follows incoming Write from var at traversal time.
    let semantic_data = create_semantic_data_with_shared_state();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    let write_count = graph
        .graph
        .edge_references()
        .filter(|e| matches!(e.weight(), EdgeKind::Write))
        .count();
    assert_eq!(
        write_count, 2,
        "W1 and W2 should have Write edges to the shared variable"
    );
}

#[test]
fn test_multiple_callers_connected_via_call_edges() {
    // Builder produces Call edges A->C and B->C. CF query follows incoming Call from C at traversal time (call-in exploration).
    let semantic_data = create_semantic_data_multiple_callers();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    assert_eq!(graph.graph.node_count(), 3);
    let call_edges_to_callee = graph
        .graph
        .edge_references()
        .filter(|e| matches!(e.weight(), EdgeKind::Call))
        .count();
    assert!(
        call_edges_to_callee >= 2,
        "Callers A and B should have Call edges to callee C"
    );
}

#[test]
fn test_constructor_call_to_type_resolves_to_init() {
    let semantic_data = create_semantic_data_with_constructor_call();
    let reader = source_reader_for_semantic_data(&semantic_data, DUMMY_SOURCE);

    let size_fn = Box::new(MockSizeFunction::new());
    let doc_scorer = Box::new(MockDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);
    let graph = builder.build(semantic_data, &reader).unwrap();

    let caller_idx = graph.get_node_by_symbol("sym::caller").unwrap();
    let init_idx = graph.get_node_by_symbol("sym::MyClass.__init__").unwrap();

    let has_call_edge = graph.graph.edge_references().any(|e| {
        e.source() == caller_idx && e.target() == init_idx && matches!(e.weight(), EdgeKind::Call)
    });
    assert!(
        has_call_edge,
        "Constructor call to Type should resolve to __init__ Call edge"
    );
}
