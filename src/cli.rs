use crate::adapters::doc_scorer::heuristic::HeuristicDocScorer;
use crate::adapters::size_function::tiktoken::TiktokenSizeFunction;
use crate::app::dto::{ComputeRequest, ContextRequest, PolicyKind};
use crate::app::engine::ContextEngine;
use crate::domain::builder::GraphBuilder;
use crate::domain::node::Node;
use crate::domain::ports::SourceReader;
use crate::domain::semantic::SemanticData;
use anyhow::{Context as _, Result};
use std::path::Path;

/// Build graph from SemanticData JSON file and print the graph structure as JSON.
pub fn debug_graph_data(json_path: &Path) -> Result<()> {
    let json_content = std::fs::read_to_string(json_path).context("Failed to read JSON file")?;
    let semantic_data: SemanticData =
        serde_json::from_str(&json_content).context("Failed to parse SemanticData JSON")?;

    struct SimpleSourceReader {
        project_root: String,
    }

    impl SourceReader for SimpleSourceReader {
        fn read(&self, path: &Path) -> Result<String> {
            let full_path = Path::new(&self.project_root).join(path);
            std::fs::read_to_string(&full_path)
                .with_context(|| format!("Failed to read source file: {}", full_path.display()))
        }

        fn read_lines(
            &self,
            path: &str,
            start_line: usize,
            end_line: usize,
        ) -> Result<Vec<String>> {
            let content = self.read(Path::new(path))?;
            let lines: Vec<String> = content
                .lines()
                .skip(start_line.saturating_sub(1))
                .take(end_line - start_line + 1)
                .map(String::from)
                .collect();
            Ok(lines)
        }
    }

    let source_reader = SimpleSourceReader {
        project_root: semantic_data.project_root.clone(),
    };

    let size_function = Box::new(TiktokenSizeFunction::new());
    let doc_scorer = Box::new(HeuristicDocScorer);
    let builder = GraphBuilder::new(size_function, doc_scorer);

    let graph = builder
        .build(semantic_data, &source_reader)
        .context("Failed to build context graph")?;

    let mut nodes = Vec::new();
    for idx in graph.graph.node_indices() {
        let node = graph.node(idx);
        let core = node.core();
        let node_type = match node {
            Node::Function(_) => "function",
            Node::Variable(_) => "variable",
        };

        let mut edges_out = Vec::new();
        for (target_idx, edge_kind) in graph.neighbors(idx) {
            let target_node = graph.node(target_idx);
            edges_out.push(serde_json::json!({
                "target": target_node.core().name,
                "target_symbol": graph.symbol_to_node.iter()
                    .find(|&(_, &v)| v == target_idx)
                    .map(|(k, _)| k.as_str())
                    .unwrap_or("unknown"),
                "kind": format!("{:?}", edge_kind),
            }));
        }

        let mut node_json = serde_json::json!({
            "id": core.id,
            "name": core.name,
            "type": node_type,
            "file": core.file_path,
            "span": format!("{}:{}-{}:{}", core.span.start_line, core.span.start_column, core.span.end_line, core.span.end_column),
            "context_size": core.context_size,
            "doc_score": core.doc_score,
            "is_external": core.is_external,
            "edges": edges_out,
        });

        if let Some(scope) = &core.scope {
            node_json["scope"] = serde_json::json!(scope);
        }

        if let Node::Function(f) = node {
            node_json["is_async"] = serde_json::json!(f.is_async);
            node_json["is_interface_method"] = serde_json::json!(f.is_interface_method);
            node_json["visibility"] = serde_json::json!(format!("{:?}", f.visibility));
            if !f.parameters.is_empty() {
                node_json["param_count"] = serde_json::json!(f.parameters.len());
            }
            if !f.return_types.is_empty() {
                node_json["return_types"] = serde_json::json!(f.return_types);
            }
        }

        let symbol = graph
            .symbol_to_node
            .iter()
            .find(|&(_, &v)| v == idx)
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        node_json["symbol"] = serde_json::json!(symbol);

        nodes.push(node_json);
    }

    let output = serde_json::json!({
        "node_count": graph.graph.node_count(),
        "edge_count": graph.graph.edge_count(),
        "type_registry_count": graph.type_registry.len(),
        "nodes": nodes,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

pub fn compute_cf_for_symbols(engine: &ContextEngine, symbols: &[String]) -> Result<()> {
    println!("Computing CF for symbols: {:?}", symbols);
    let result = engine.compute(ComputeRequest {
        symbols: symbols.to_vec(),
        policy: PolicyKind::Academic,
        max_tokens: None,
    })?;

    println!("CF Result:");
    println!("  Starting symbols: {}", symbols.len());
    println!("  Total context size: {} tokens", result.total_context_size);
    println!("  Reachable nodes: {}", result.reachable_node_count);

    Ok(())
}

pub fn display_top_cf_nodes(
    engine: &ContextEngine,
    limit: usize,
    node_type: &str,
    include_tests: bool,
) -> Result<()> {
    println!("Computing CF for all nodes...");
    let result = engine.top(limit, node_type, include_tests, PolicyKind::Academic)?;

    let filter_msg = if !include_tests {
        " (excluding tests)"
    } else {
        ""
    };
    println!("\nTop {} nodes by Context Footprint{}:", limit, filter_msg);
    println!("{}", "=".repeat(80));

    for (i, item) in result.items.iter().enumerate() {
        println!("{}. [{}] {} tokens", i + 1, item.node_type, item.cf);
        println!("   {}", item.symbol);
        println!();
    }

    Ok(())
}

pub fn search_symbols(
    engine: &ContextEngine,
    pattern: &str,
    with_cf: bool,
    limit: Option<usize>,
    include_tests: bool,
) -> Result<()> {
    println!("Searching for symbols matching: \"{}\"", pattern);
    println!("{}", "=".repeat(80));
    let result = engine.search(pattern, with_cf, limit, include_tests, PolicyKind::Academic)?;

    let filter_msg = if !include_tests {
        " (excluding tests)"
    } else {
        ""
    };
    println!(
        "Found {} matching symbol(s){}:\n",
        result.total_matches, filter_msg
    );

    if let Some(lim) = limit.filter(|&lim| result.total_matches > lim) {
        println!("Showing top {} by CF:\n", lim);
    }

    for (i, item) in result.items.iter().enumerate() {
        print!("{}. [{}] ", i + 1, item.node_type);
        if let Some(cf) = item.cf {
            print!("CF: {} tokens", cf);
        }
        println!("\n   {}", item.symbol);
        println!();
    }

    Ok(())
}

pub fn display_context_code(
    engine: &ContextEngine,
    symbol: &str,
    _show_boundaries: bool,
    max_tokens: Option<u32>,
) -> Result<()> {
    println!("Computing context for symbol: {}", symbol);
    let result = engine.context(ContextRequest {
        symbol: symbol.to_string(),
        policy: PolicyKind::Academic,
        max_tokens,
        include_code: true,
    })?;

    println!("\nContext Summary:");
    println!("  Total size: {} tokens", result.total_context_size);
    println!("  Reachable nodes: {}", result.reachable_node_count);
    if let Some(limit) = max_tokens {
        println!("  Max tokens: {}", limit);
    }
    println!("{}", "=".repeat(80));

    for layer in &result.layers {
        println!(
            "\n\u{1F310} Layer {}: {}",
            layer.depth,
            if layer.depth == 0 {
                "Observed Symbol"
            } else {
                "Direct Dependencies"
            }
        );
        println!("{}", "=".repeat(40));

        for file in &layer.files {
            println!("\n  \u{1F4C4} File: {}", file.file_path);
            for node in &file.nodes {
                let display = node.symbol.split('/').next_back().unwrap_or(&node.symbol);
                println!("    Symbol: {} ({} tokens)", display, node.context_size);
                println!(
                    "    Lines: {}-{}",
                    node.span.start_line_1based, node.span.end_line_1based
                );
                if let Some(lines) = &node.code {
                    println!("    Code:");
                    for l in lines {
                        println!("      {:4} | {}", l.line_number, l.text);
                    }
                }
            }
        }
    }

    Ok(())
}

pub fn compute_and_display_cf_stats(engine: &ContextEngine, include_tests: bool) -> Result<()> {
    let filter_msg = if !include_tests {
        " (excluding tests)"
    } else {
        ""
    };
    println!("Calculating CF stats{}...", filter_msg);
    let result = engine.stats(include_tests, PolicyKind::Academic)?;

    println!("\n{}", "=".repeat(60));
    print_distribution(&format!("Functions{}", filter_msg), &result.functions);
    println!("{}", "=".repeat(60));

    Ok(())
}

fn print_distribution(name: &str, dist: &crate::app::dto::CfDistribution) {
    println!("\n{} - Context Footprint Distribution:", name);
    println!("  Total count: {}", dist.count);

    println!("\n  Percentiles:");
    for p in &dist.percentiles {
        println!("    {:>3}%: {:>8} tokens", p.percentile, p.tokens);
    }

    println!("\n  Summary:");
    println!("    Average: {:>8} tokens", dist.average);
    println!("    Median:  {:>8} tokens", dist.median);
    println!("    Min:     {:>8} tokens", dist.min);
    println!("    Max:     {:>8} tokens", dist.max);
}
