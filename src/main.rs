use anyhow::Result;
use clap::{Parser, Subcommand};
use context_footprint::adapters::doc_scorer::simple::SimpleDocScorer;
use context_footprint::adapters::fs::reader::FileSourceReader;
use context_footprint::adapters::policy::academic::AcademicBaseline;
use context_footprint::adapters::scip::adapter::ScipDataSourceAdapter;
use context_footprint::adapters::size_function::tiktoken::TiktokenSizeFunction;
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::graph::ContextGraph;
use context_footprint::domain::node::Node;
use context_footprint::domain::ports::{SemanticDataSource, SourceReader};
use context_footprint::domain::solver::CfSolver;
use petgraph::graph::NodeIndex;

#[derive(Parser)]
#[command(name = "context-footprint")]
#[command(about = "Analyze code coupling via Context Footprint metric", long_about = None)]
struct Cli {
    /// Path to SCIP index file
    #[arg(default_value = "index.scip")]
    scip_path: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compute CF for a specific symbol
    Compute {
        /// Symbol to analyze (e.g., "scip-python python myapp abc123 `module`/Class#method().")
        symbol: String,
    },
    /// Show CF distribution statistics across all nodes
    Stats,
    /// List nodes with highest CF
    Top {
        /// Number of nodes to display
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Filter by node type (function, type, or all)
        #[arg(short = 't', long, default_value = "all")]
        node_type: String,
    },
    /// Search for symbols by keyword
    Search {
        /// Keyword to search for in symbol names
        pattern: String,
        /// Show CF for each result
        #[arg(short, long)]
        with_cf: bool,
    },
    /// Print all context code for a symbol
    Context {
        /// Symbol to analyze
        symbol: String,
        /// Also show which nodes are boundaries (stop traversal)
        #[arg(short, long)]
        show_boundaries: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("Loading SCIP index from: {}", cli.scip_path);

    let data_source = ScipDataSourceAdapter::new(&cli.scip_path);
    let source_reader = FileSourceReader::new();
    let size_function = Box::new(TiktokenSizeFunction::new());
    let doc_scorer = Box::new(SimpleDocScorer::new());

    println!("Building context graph...");
    let mut semantic_data = data_source.load()?;

    // Override project_root: use SCIP file's parent directory
    if let Some(scip_parent) = std::path::Path::new(&cli.scip_path).parent() {
        let project_root = scip_parent.to_string_lossy().to_string();
        if !project_root.is_empty() {
            semantic_data.project_root = project_root;
        }
    }

    let builder = GraphBuilder::new(size_function, doc_scorer);
    let graph = builder.build(semantic_data, &source_reader)?;

    println!("Graph Summary:");
    println!("  Nodes: {}", graph.graph.node_count());
    println!("  Edges: {}", graph.graph.edge_count());
    println!();

    match &cli.command {
        Commands::Compute { symbol } => {
            compute_cf_for_symbol(&graph, symbol)?;
        }
        Commands::Stats => {
            compute_and_display_cf_stats(&graph)?;
        }
        Commands::Top { limit, node_type } => {
            display_top_cf_nodes(&graph, *limit, node_type)?;
        }
        Commands::Search { pattern, with_cf } => {
            search_symbols(&graph, pattern, *with_cf)?;
        }
        Commands::Context {
            symbol,
            show_boundaries,
        } => {
            display_context_code(&graph, symbol, *show_boundaries, &source_reader)?;
        }
    }

    Ok(())
}

fn compute_cf_for_symbol(graph: &ContextGraph, symbol: &str) -> Result<()> {
    println!("Computing CF for symbol: {}", symbol);

    let node_idx = graph
        .get_node_by_symbol(symbol)
        .ok_or_else(|| anyhow::anyhow!("Symbol not found: {}", symbol))?;

    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();
    let result = solver.compute_cf(graph, node_idx, &policy);

    println!("CF Result:");
    println!("  Total context size: {} tokens", result.total_context_size);
    println!("  Reachable nodes: {}", result.reachable_set.len());

    Ok(())
}

fn display_top_cf_nodes(graph: &ContextGraph, limit: usize, node_type: &str) -> Result<()> {
    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();

    println!("Computing CF for all nodes...");
    let mut cf_results: Vec<(String, u32, &str)> = Vec::new();

    for (symbol, &node_idx) in &graph.symbol_to_node {
        let node = graph.node(node_idx);

        let type_str = match node {
            Node::Function(_) => "function",
            Node::Type(_) => "type",
            Node::Variable(_) => "variable",
        };

        // Filter by node type if specified
        if node_type != "all" && node_type != type_str {
            continue;
        }

        let result = solver.compute_cf(graph, node_idx, &policy);
        cf_results.push((symbol.clone(), result.total_context_size, type_str));
    }

    // Sort by CF (descending)
    cf_results.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\nTop {} nodes by Context Footprint:", limit);
    println!("{}", "=".repeat(80));

    for (i, (symbol, cf, node_type)) in cf_results.iter().take(limit).enumerate() {
        println!("{}. [{}] {} tokens", i + 1, node_type, cf);
        println!("   {}", symbol);
        println!();
    }

    Ok(())
}

fn search_symbols(graph: &ContextGraph, pattern: &str, with_cf: bool) -> Result<()> {
    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();

    println!("Searching for symbols matching: \"{}\"", pattern);
    println!("{}", "=".repeat(80));

    let pattern_lower = pattern.to_lowercase();
    let mut matches: Vec<(String, &str, Option<u32>)> = Vec::new();

    for (symbol, &node_idx) in &graph.symbol_to_node {
        let node = graph.node(node_idx);

        let type_str = match node {
            Node::Function(_) => "function",
            Node::Type(_) => "type",
            Node::Variable(_) => "variable",
        };

        // Simple substring match (case-insensitive)
        if symbol.to_lowercase().contains(&pattern_lower) {
            let cf = if with_cf {
                let result = solver.compute_cf(graph, node_idx, &policy);
                Some(result.total_context_size)
            } else {
                None
            };
            matches.push((symbol.clone(), type_str, cf));
        }
    }

    println!("Found {} matching symbol(s):\n", matches.len());

    for (i, (symbol, node_type, cf)) in matches.iter().enumerate() {
        print!("{}. [{}] ", i + 1, node_type);
        if let Some(cf_value) = cf {
            print!("CF: {} tokens", cf_value);
        }
        println!("\n   {}", symbol);
        println!();
    }

    Ok(())
}

fn display_context_code(
    graph: &ContextGraph,
    symbol: &str,
    _show_boundaries: bool,
    source_reader: &dyn SourceReader,
) -> Result<()> {
    println!("Computing context for symbol: {}", symbol);

    let node_idx = graph
        .get_node_by_symbol(symbol)
        .ok_or_else(|| anyhow::anyhow!("Symbol not found: {}", symbol))?;

    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();
    let result = solver.compute_cf(graph, node_idx, &policy);

    println!("\nContext Summary:");
    println!("  Total size: {} tokens", result.total_context_size);
    println!("  Reachable nodes: {}", result.reachable_set.len());
    println!("{}", "=".repeat(80));

    // Group nodes by file for better organization
    let mut files_map: std::collections::HashMap<String, Vec<NodeIndex>> =
        std::collections::HashMap::new();

    for &node_id in &result.reachable_set {
        // Find the NodeIndex for this node_id
        let idx = graph
            .graph
            .node_indices()
            .find(|&idx| graph.node(idx).core().id == node_id)
            .unwrap();

        let node = graph.node(idx);
        let file_path = &node.core().file_path;

        files_map
            .entry(file_path.clone())
            .or_insert_with(Vec::new)
            .push(idx);
    }

    // Sort files for consistent output
    let mut file_list: Vec<_> = files_map.iter().collect();
    file_list.sort_by_key(|(path, _)| *path);

    for (file_path, nodes) in file_list {
        println!("\n📄 File: {}", file_path);
        println!("{}", "-".repeat(80));

        for node_idx in nodes {
            let node = graph.node(*node_idx);
            let core = node.core();
            
            // Get the symbol for this node
            let symbol = graph
                .symbol_to_node
                .iter()
                .find(|&(_, &idx)| idx == *node_idx)
                .map(|(s, _)| s.as_str())
                .unwrap_or(&core.name);

            println!(
                "\n  Symbol: {}",
                symbol.split('/').last().unwrap_or(symbol)
            );
            println!(
                "  Lines: {}-{}",
                core.span.start_line, core.span.end_line
            );

            // Read and display the code
            match source_reader.read_lines(
                file_path,
                core.span.start_line as usize,
                core.span.end_line as usize,
            ) {
                Ok(lines) => {
                    println!("  Code:");
                    for (i, line) in lines.iter().enumerate() {
                        let line_num = core.span.start_line as usize + i;
                        println!("    {:4} | {}", line_num, line);
                    }
                }
                Err(e) => {
                    println!("  [Error reading code: {}]", e);
                }
            }
        }
    }

    Ok(())
}

fn compute_and_display_cf_stats(graph: &ContextGraph) -> Result<()> {
    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();
    let node_count = graph.graph.node_count();

    let mut function_cf: Vec<u32> = Vec::new();
    let mut type_cf: Vec<u32> = Vec::new();

    println!("Calculating CF for {} nodes...", node_count);

    for (idx, node_idx) in graph.graph.node_indices().enumerate() {
        let node = graph.node(node_idx);
        let result = solver.compute_cf(graph, node_idx, &policy);
        let cf = result.total_context_size;

        match node {
            Node::Function(_) => function_cf.push(cf),
            Node::Type(_) => type_cf.push(cf),
            Node::Variable(_) => {} // Skip variables
        }

        if (idx + 1).is_multiple_of(1000) {
            println!("  Processed {}/{} nodes...", idx + 1, node_count);
        }
    }

    println!("\n{}", "=".repeat(60));
    print_cf_distribution("Functions", &mut function_cf);
    println!("{}", "=".repeat(60));
    print_cf_distribution("Types", &mut type_cf);
    println!("{}", "=".repeat(60));

    Ok(())
}

fn print_cf_distribution(name: &str, sizes: &mut [u32]) {
    if sizes.is_empty() {
        println!("\n{}: No nodes found", name);
        return;
    }

    sizes.sort_unstable();

    println!("\n{} - Context Footprint Distribution:", name);
    println!("  Total count: {}", sizes.len());

    // Print percentiles in 5% steps
    println!("\n  Percentiles:");
    for i in (5..=100).step_by(5) {
        let index = ((i * (sizes.len() - 1)) / 100).min(sizes.len() - 1);
        println!("    {:>3}%: {:>8} tokens", i, sizes[index]);
    }

    // Print summary stats
    let sum: u64 = sizes.iter().map(|&s| s as u64).sum();
    let avg = sum / sizes.len() as u64;
    let median_idx = sizes.len() / 2;
    let median = sizes[median_idx];

    println!("\n  Summary:");
    println!("    Average: {:>8} tokens", avg);
    println!("    Median:  {:>8} tokens", median);
    println!("    Min:     {:>8} tokens", sizes[0]);
    println!("    Max:     {:>8} tokens", sizes[sizes.len() - 1]);
}
