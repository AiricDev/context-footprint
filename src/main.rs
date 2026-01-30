use anyhow::Result;
use clap::{Parser, Subcommand};
use context_footprint::app::engine::ContextEngine;
use context_footprint::cli;
use context_footprint::server;
use std::net::SocketAddr;

#[derive(Parser)]
#[command(name = "context-footprint")]
#[command(about = "Analyze code coupling via Context Footprint metric", long_about = None)]
struct Cli {
    /// Path to SCIP index file
    #[arg(default_value = "index.scip")]
    scip_path: String,

    /// Override the project root used to resolve `Document.relative_path`.
    ///
    /// By default, we trust `Index.metadata.project_root` from the SCIP index.
    /// Use this flag when the index was generated elsewhere or the sources live
    /// in a different location on disk.
    #[arg(long)]
    project_root: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compute CF for specific symbols (union)
    Compute {
        /// Symbols to analyze
        #[arg(required = true)]
        symbols: Vec<String>,
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
    /// Start an HTTP server for repeated queries
    Serve {
        /// Host to bind (e.g. 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind (e.g. 8080)
        #[arg(long, default_value = "8080")]
        port: u16,
    },
    /// Start an MCP server over stdio
    Mcp {},
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    println!("Loading SCIP index from: {}", cli.scip_path);

    println!("Building context graph...");
    let engine =
        ContextEngine::load_from_scip_with_project_root(&cli.scip_path, cli.project_root.as_deref())?;
    let health = engine.health();

    println!("Graph Summary:");
    println!("  Nodes: {}", health.node_count);
    println!("  Edges: {}", health.edge_count);
    println!();

    match &cli.command {
        Commands::Compute { symbols } => {
            cli::compute_cf_for_symbols(&engine, symbols)?;
        }
        Commands::Stats { include_tests } => {
            cli::compute_and_display_cf_stats(&engine, *include_tests)?;
        }
        Commands::Top {
            limit,
            node_type,
            include_tests,
        } => {
            cli::display_top_cf_nodes(&engine, *limit, node_type, *include_tests)?;
        }
        Commands::Search {
            pattern,
            with_cf,
            limit,
            include_tests,
        } => {
            cli::search_symbols(&engine, pattern, *with_cf, *limit, *include_tests)?;
        }
        Commands::Context {
            symbol,
            show_boundaries,
            max_tokens,
        } => {
            cli::display_context_code(&engine, symbol, *show_boundaries, *max_tokens)?;
        }
        Commands::Serve { host, port } => {
            let addr: SocketAddr = format!("{host}:{port}")
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid bind addr {host}:{port}: {e}"))?;
            println!("Starting HTTP server on http://{addr}");
            server::http::serve(engine, addr).await?;
        }
        Commands::Mcp {} => {
            println!("Starting MCP stdio server...");
            server::mcp::CfMcpServer::new(engine).serve_stdio().await?;
        }
    }

    Ok(())
}
