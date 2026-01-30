use crate::app::dto::{ComputeRequest, ContextRequest, PolicyKind};
use crate::app::engine::ContextEngine;
use anyhow::Result;

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
        result.total_matches,
        filter_msg
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
                println!(
                    "    Symbol: {} ({} tokens)",
                    display, node.context_size
                );
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
    print_distribution(&format!("Types{}", filter_msg), &result.types);
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
