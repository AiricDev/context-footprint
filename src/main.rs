use anyhow::Result;
use clap::{Parser, Subcommand};
use context_footprint::adapters::doc_scorer::simple::SimpleDocScorer;
use context_footprint::adapters::fs::reader::FileSourceReader;
use context_footprint::adapters::scip::adapter::ScipDataSourceAdapter;
use context_footprint::adapters::size_function::tiktoken::TiktokenSizeFunction;
use context_footprint::cli;
use context_footprint::domain::builder::GraphBuilder;
use context_footprint::domain::ports::SemanticDataSource;

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
    Stats {
        /// Include test code (test_* functions and tests/ directory)
        #[arg(short, long)]
        include_tests: bool,
    },
    /// List nodes with highest CF
    Top {
        /// Number of nodes to display
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Filter by node type (function, type, or all)
        #[arg(short = 't', long, default_value = "all")]
        node_type: String,
        /// Include test code (test_* functions and tests/ directory)
        #[arg(short, long)]
        include_tests: bool,
    },
    /// Search for symbols by keyword
    Search {
        /// Keyword to search for in symbol names
        pattern: String,
        /// Show CF for each result
        #[arg(short, long)]
        with_cf: bool,
        /// Number of results to display (sorted by CF descending)
        #[arg(short, long)]
        limit: Option<usize>,
        /// Include test code (test_* functions and tests/ directory)
        #[arg(short, long)]
        include_tests: bool,
    },
    /// Print all context code for a symbol
    Context {
        /// Symbol to analyze
        symbol: String,
        /// Also show which nodes are boundaries (stop traversal)
        #[arg(short, long)]
        show_boundaries: bool,
        /// Max tokens to include in output
        #[arg(short, long)]
        max_tokens: Option<u32>,
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
    let graph = builder.build(semantic_data.clone(), &source_reader)?;

    println!("Graph Summary:");
    println!("  Nodes: {}", graph.graph.node_count());
    println!("  Edges: {}", graph.graph.edge_count());
    println!();

    match &cli.command {
        Commands::Compute { symbol } => {
            cli::compute_cf_for_symbol(&graph, symbol)?;
        }
        Commands::Stats { include_tests } => {
            cli::compute_and_display_cf_stats(&graph, *include_tests)?;
        }
        Commands::Top {
            limit,
            node_type,
            include_tests,
        } => {
            cli::display_top_cf_nodes(&graph, *limit, node_type, *include_tests)?;
        }
        Commands::Search {
            pattern,
            with_cf,
            limit,
            include_tests,
        } => {
            cli::search_symbols(&graph, pattern, *with_cf, *limit, *include_tests)?;
        }
        Commands::Context {
            symbol,
            show_boundaries,
            max_tokens,
        } => {
            cli::display_context_code(
                &graph,
                symbol,
                *show_boundaries,
                &source_reader,
                &semantic_data.project_root,
                *max_tokens,
            )?;
        }
    }

    Ok(())
}
