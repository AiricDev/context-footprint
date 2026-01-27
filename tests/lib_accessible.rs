//! Sanity check: library and test common module are accessible.

mod common;

use context_footprint::domain::graph::ContextGraph;
use context_footprint::domain::policy::SourceSpan;

#[test]
fn test_library_accessible() {
    let graph = ContextGraph::new();
    assert_eq!(graph.graph.node_count(), 0);
}

#[test]
fn test_mock_size_function() {
    use common::mock::MockSizeFunction;
    use context_footprint::domain::policy::SizeFunction;

    let f = MockSizeFunction::with_size(42);
    let span = SourceSpan {
        start_line: 0,
        start_column: 0,
        end_line: 1,
        end_column: 5,
    };
    assert_eq!(f.compute("hello", &span), 42);
}

#[test]
fn test_mock_doc_scorer() {
    use common::mock::MockDocScorer;
    use context_footprint::domain::policy::{DocumentationScorer, NodeInfo, NodeType};

    let s = MockDocScorer::with_score(0.8);
    let info = NodeInfo {
        node_type: NodeType::Function,
        name: "foo".into(),
        signature: None,
    };
    assert_eq!(s.score(&info, Some("doc")), 0.8);
    assert_eq!(s.score(&info, None), 0.0);
}

#[test]
fn test_mock_source_reader() {
    use common::mock::MockSourceReader;
    use context_footprint::domain::ports::SourceReader;
    use std::path::Path;

    let reader = MockSourceReader::new().with_file("/test/main.py", "def foo(): pass");
    let out = reader.read(Path::new("/test/main.py")).unwrap();
    assert_eq!(out, "def foo(): pass");
    assert!(reader.read(Path::new("/missing")).is_err());
}
