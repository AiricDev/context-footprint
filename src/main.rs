use crate::graph::ScipGraph;
use anyhow::{Context, Result};
use memmap2::Mmap;
use prost::Message;
use std::fs::File;
use std::path::Path;

pub mod scip {
    include!(concat!(env!("OUT_DIR"), "/scip.rs"));
}

pub mod symbol;
pub mod graph;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let scip_path = if args.len() > 1 {
        &args[1]
    } else {
        "index.scip"
    };

    println!("Loading index from: {}", scip_path);
    
    let index = load_scip_index(scip_path)?;
    println!("Successfully loaded index. Building graph...");

    let mut graph = ScipGraph::new();
    graph.build(&index);

    println!("Graph Summary:");
    println!("  Nodes: {}", graph.graph.node_count());
    println!("  Edges: {}", graph.graph.edge_count());

    Ok(())
}

fn load_scip_index<P: AsRef<Path>>(path: P) -> Result<scip::Index> {
    let file = File::open(path).context("Failed to open index file")?;
    let mmap = unsafe { Mmap::map(&file).context("Failed to mmap index file")? };
    
    let index = scip::Index::decode(&mmap[..]).context("Failed to decode SCIP index")?;
    Ok(index)
}
