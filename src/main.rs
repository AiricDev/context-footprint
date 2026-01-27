use anyhow::Result;

pub mod scip {
    include!(concat!(env!("OUT_DIR"), "/scip.rs"));
}

pub mod domain;
pub mod adapters;

use crate::domain::builder::GraphBuilder;
use crate::domain::solver::CfSolver;
use crate::domain::ports::SemanticDataSource;
use crate::adapters::scip::adapter::ScipDataSourceAdapter;
use crate::adapters::fs::reader::FileSourceReader;
use crate::adapters::size_function::tiktoken::TiktokenSizeFunction;
use crate::adapters::doc_scorer::simple::SimpleDocScorer;
use crate::adapters::policy::academic::AcademicBaseline;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let scip_path = if args.len() > 1 {
        args[1].clone()
    } else {
        "index.scip".to_string()
    };

    println!("Loading SCIP index from: {}", scip_path);
    
    // 1. Setup Domain Services & Infrastructure Adapters
    let data_source = ScipDataSourceAdapter::new(&scip_path);
    let source_reader = FileSourceReader::new();
    let size_function = Box::new(TiktokenSizeFunction::new());
    let doc_scorer = Box::new(SimpleDocScorer::new());
    
    // 2. Build Context Graph (Orchestration)
    println!("Building context graph...");
    let semantic_data = data_source.load()?;
    let builder = GraphBuilder::new(size_function, doc_scorer);
    let graph = builder.build(semantic_data, &source_reader)?;

    println!("Graph Summary:");
    println!("  Nodes: {}", graph.graph.node_count());
    println!("  Edges: {}", graph.graph.edge_count());
    
    // 3. Compute CF if target symbol is provided (Orchestration)
    if args.len() > 2 {
        let target_symbol = &args[2];
        println!("\nComputing CF for symbol: {}", target_symbol);
        
        let node_idx = graph.get_node_by_symbol(target_symbol)
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
