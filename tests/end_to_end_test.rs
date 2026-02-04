//! End-to-end tests: load real SCIP index, build graph, compute CF.

mod common;

use std::path::Path;

use context_footprint::adapters::doc_scorer::heuristic::HeuristicDocScorer;
use context_footprint::adapters::fs::reader::FileSourceReader;
use context_footprint::adapters::scip::adapter::ScipDataSourceAdapter;
use context_footprint::adapters::size_function::tiktoken::TiktokenSizeFunction;
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::policy::PruningParams;
use context_footprint::domain::ports::SemanticDataSource;
use context_footprint::domain::solver::CfSolver;
use std::sync::Arc;

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
    let doc_scorer = Box::new(HeuristicDocScorer::new());
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
    let expected_min = graph.node(start).core().context_size;
    let mut solver = CfSolver::new(Arc::new(graph), PruningParams::academic(0.5));
    let result = solver.compute_cf(&[start], None);

    assert!(!result.reachable_set.is_empty());
    assert!(result.total_context_size >= expected_min);
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
    let doc_scorer = Box::new(HeuristicDocScorer::new());
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

    // Compute CF for every node to calculate statistics (uses internal memo for efficiency)
    let graph_arc = Arc::new(graph);
    let mut solver = CfSolver::new(Arc::clone(&graph_arc), PruningParams::academic(0.5));
    let node_count = graph_arc.graph.node_count();

    println!("Calculating CF for all {} nodes...", node_count);

    let mut function_cf: Vec<u32> = Vec::new();
    let mut variable_cf: Vec<u32> = Vec::new();
    let type_cf: Vec<u32> = Vec::new(); // Types are in TypeRegistry, not graph nodes
    let mut low_cf_examples: Vec<(String, String, u32, &'static str)> = Vec::new();

    // Invert symbol_to_node for better reporting
    let mut node_to_symbol: std::collections::HashMap<petgraph::graph::NodeIndex, String> =
        std::collections::HashMap::new();
    for (sym, idx) in &graph_arc.symbol_to_node {
        node_to_symbol.insert(*idx, sym.clone());
    }

    for idx in graph_arc.graph.node_indices() {
        let node = graph_arc.node(idx);
        let cf = solver.compute_cf_total(idx);

        // Types are in TypeRegistry, not graph nodes; only Function and Variable nodes exist
        let kind = match node {
            context_footprint::domain::node::Node::Function(_) => {
                function_cf.push(cf);
                "Function"
            }
            context_footprint::domain::node::Node::Variable(_) => {
                variable_cf.push(cf);
                "Variable"
            }
        };

        if cf <= 2 && low_cf_examples.len() < 40 {
            let symbol = node_to_symbol.get(&idx).cloned().unwrap_or_default();
            let name = if node.core().name.is_empty() {
                symbol.split(' ').next_back().unwrap_or("").to_string()
            } else {
                node.core().name.clone()
            };
            low_cf_examples.push((name, symbol, cf, kind));
        }

        let total_processed = function_cf.len() + variable_cf.len() + type_cf.len();
        if total_processed.is_multiple_of(1000) {
            println!("Processed {}/{} nodes...", total_processed, node_count);
        }
    }

    let print_stats = |name: &str, mut sizes: Vec<u32>| {
        if sizes.is_empty() {
            return;
        }
        sizes.sort_unstable();
        println!("\nContext Footprint Percentiles ({}):", name);
        println!("---------------------------------------");
        for i in 1..=20 {
            let p = i * 5;
            let index = (p * (sizes.len() - 1)) / 100;
            println!("{:>3}%: {} tokens", p, sizes[index]);
        }
        let node_count = sizes.len();
        let sum: u64 = sizes.iter().map(|&s| s as u64).sum();
        println!("---------------------------------------");
        println!("Count:   {}", node_count);
        println!("Average: {} tokens", sum / node_count as u64);
        println!("Max:     {} tokens", sizes.last().unwrap_or(&0));
    };

    print_stats("Functions", function_cf);
    print_stats("Variables", variable_cf);
    print_stats("Types", type_cf);

    println!("\nLow CF Examples (CF <= 2):");
    println!("---------------------------------------");
    println!("{:<10} | {:<4} | {:<20} | Symbol", "Kind", "CF", "Name");
    for (name, symbol, cf, kind) in low_cf_examples {
        println!("{:<10} | {:<4} | {:<20} | {}", kind, cf, name, symbol);
    }
    println!("---------------------------------------");

    println!("FastAPI E2E test passed!");
}
