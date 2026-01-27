use anyhow::Result;
use context_footprint::adapters::doc_scorer::simple::SimpleDocScorer;
use context_footprint::adapters::fs::reader::FileSourceReader;
use context_footprint::adapters::policy::academic::AcademicBaseline;
use context_footprint::adapters::scip::adapter::ScipDataSourceAdapter;
use context_footprint::adapters::size_function::tiktoken::TiktokenSizeFunction;
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::ports::SemanticDataSource;
use context_footprint::domain::solver::CfSolver;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let scip_path = if args.len() > 1 {
        args[1].clone()
    } else {
        "index.scip".to_string()
    };

    println!("Loading SCIP index from: {}", scip_path);

    let data_source = ScipDataSourceAdapter::new(&scip_path);
    let source_reader = FileSourceReader::new();
    let size_function = Box::new(TiktokenSizeFunction::new());
    let doc_scorer = Box::new(SimpleDocScorer::new());

    println!("Building context graph...");
    let semantic_data = data_source.load()?;
    let builder = GraphBuilder::new(size_function, doc_scorer);
    let graph = builder.build(semantic_data, &source_reader)?;

    println!("Graph Summary:");
    println!("  Nodes: {}", graph.graph.node_count());
    println!("  Edges: {}", graph.graph.edge_count());

    if args.len() > 2 {
        let target_symbol = &args[2];
        println!("\nComputing CF for symbol: {}", target_symbol);

        let node_idx = graph
            .get_node_by_symbol(target_symbol)
            .ok_or_else(|| anyhow::anyhow!("Symbol not found: {}", target_symbol))?;

        let policy = AcademicBaseline::default();
        let solver = CfSolver::new();
        let result = solver.compute_cf(&graph, node_idx, &policy);

        println!("CF Result:");
        println!("  Total context size: {}", result.total_context_size);
        println!("  Reachable nodes: {}", result.reachable_set.len());
    }

    Ok(())
}
