//! End-to-end tests: load real SCIP index, build graph, compute CF.

mod common;

use std::path::Path;

use context_footprint::adapters::doc_scorer::simple::SimpleDocScorer;
use context_footprint::adapters::fs::reader::FileSourceReader;
use context_footprint::adapters::policy::academic::AcademicBaseline;
use context_footprint::adapters::scip::adapter::ScipDataSourceAdapter;
use context_footprint::adapters::size_function::tiktoken::TiktokenSizeFunction;
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::ports::SemanticDataSource;
use context_footprint::domain::solver::CfSolver;

const SIMPLE_PYTHON_SCIP: &str = "tests/fixtures/simple_python/index.scip";
const FASTAPI_SCIP: &str = "tests/fixtures/fastapi/index.scip";

/// When `index.scip` exists (e.g. after `scip-python index .` in simple_python),
/// runs full pipeline: load SCIP → build graph → compute CF for a symbol.
#[test]
fn test_simple_python_project_when_scip_present() {
    if !Path::new(SIMPLE_PYTHON_SCIP).exists() {
        eprintln!(
            "Skipping E2E test: {} not found (run scip-python in tests/fixtures/simple_python to generate)",
            SIMPLE_PYTHON_SCIP
        );
        return;
    }

    let adapter = ScipDataSourceAdapter::new(SIMPLE_PYTHON_SCIP);
    let data = adapter.load().expect("load SCIP");
    assert!(!data.documents.is_empty(), "at least one document");

    let project_root = std::path::Path::new(SIMPLE_PYTHON_SCIP)
        .parent()
        .unwrap()
        .to_path_buf();
    let source_reader = FileSourceReader::new();
    let size_fn = Box::new(TiktokenSizeFunction::new());
    let doc_scorer = Box::new(SimpleDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);

    // Builder expects paths relative to project_root; our SemanticData has project_root
    // from the SCIP index. Override for test: set project_root to the fixture dir
    // so that source_reader can find main.py and utils.py.
    let mut data = data;
    data.project_root = project_root.to_string_lossy().into_owned();

    let graph = builder.build(data, &source_reader).expect("build graph");
    assert!(graph.graph.node_count() >= 1);
    assert!(!graph.symbol_to_node.is_empty());

    // Pick any symbol and compute CF
    let first_symbol = graph.symbol_to_node.keys().next().unwrap().clone();
    let start = graph.get_node_by_symbol(&first_symbol).unwrap();
    let solver = CfSolver::new();
    let policy = AcademicBaseline::default();
    let result = solver.compute_cf(&graph, start, &policy);

    assert!(!result.reachable_set.is_empty());
    assert!(result.total_context_size >= graph.node(start).core().context_size);
}

/// E2E test with a real-world project: FastAPI.
/// Run `tests/fixtures/setup_fastapi.sh` first to clone FastAPI and generate index.scip.
#[test]
fn test_fastapi_project() {
    if !Path::new(FASTAPI_SCIP).exists() {
        eprintln!("Skipping FastAPI E2E test: {} not found", FASTAPI_SCIP);
        eprintln!("Run `tests/fixtures/setup_fastapi.sh` to set up the fixture");
        return;
    }

    println!("Running E2E test on FastAPI...");

    let adapter = ScipDataSourceAdapter::new(FASTAPI_SCIP);
    let data = adapter.load().expect("load FastAPI SCIP index");

    println!(
        "FastAPI: {} documents, {} external symbols",
        data.documents.len(),
        data.external_symbols.len()
    );
    assert!(!data.documents.is_empty(), "FastAPI has source files");

    // Set project_root to fastapi/ so source files can be read
    let mut data = data;
    let fastapi_root = Path::new(FASTAPI_SCIP).parent().unwrap();
    data.project_root = fastapi_root.to_string_lossy().into_owned();

    let source_reader = FileSourceReader::new();
    let size_fn = Box::new(TiktokenSizeFunction::new());
    let doc_scorer = Box::new(SimpleDocScorer::new());
    let builder = GraphBuilder::new(size_fn, doc_scorer);

    println!("Building context graph for FastAPI...");
    let graph = builder
        .build(data, &source_reader)
        .expect("build FastAPI graph");

    println!(
        "FastAPI graph: {} nodes, {} edges",
        graph.graph.node_count(),
        graph.graph.edge_count()
    );
    assert!(
        graph.graph.node_count() >= 10,
        "FastAPI should have many symbols"
    );

    // Compute CF for the first few symbols to verify the full pipeline
    let symbols: Vec<_> = graph.symbol_to_node.keys().take(5).cloned().collect();
    let solver = CfSolver::new();
    let policy = AcademicBaseline::default();

    for symbol in symbols {
        let idx = graph.get_node_by_symbol(&symbol).unwrap();
        let result = solver.compute_cf(&graph, idx, &policy);
        println!(
            "CF for {}: {} nodes, {} tokens",
            symbol,
            result.reachable_set.len(),
            result.total_context_size
        );
        assert!(!result.reachable_set.is_empty());
        assert!(result.total_context_size > 0);
    }

    println!("FastAPI E2E test passed!");
}
