use crate::adapters::policy::academic::AcademicBaseline;
use crate::adapters::test_detector::UniversalTestDetector;
use crate::domain::graph::ContextGraph;
use crate::domain::node::Node;
use crate::domain::ports::SourceReader;
use crate::domain::solver::CfSolver;
use anyhow::Result;
use petgraph::graph::NodeIndex;

pub fn compute_cf_for_symbols(graph: &ContextGraph, symbols: &[String]) -> Result<()> {
    println!("Computing CF for symbols: {:?}", symbols);

    let mut start_indices = Vec::new();
    for symbol in symbols {
        let node_idx = graph
            .get_node_by_symbol(symbol)
            .ok_or_else(|| anyhow::anyhow!("Symbol not found: {}", symbol))?;
        start_indices.push(node_idx);
    }

    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();
    let result = solver.compute_cf(graph, &start_indices, &policy, None);

    println!("CF Result:");
    println!("  Starting symbols: {}", symbols.len());
    println!("  Total context size: {} tokens", result.total_context_size);
    println!("  Reachable nodes: {}", result.reachable_set.len());

    Ok(())
}

pub fn display_top_cf_nodes(
    graph: &ContextGraph,
    limit: usize,
    node_type: &str,
    include_tests: bool,
) -> Result<()> {
    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();
    let test_detector = UniversalTestDetector::new();

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

        // Filter out test code if requested (default is to exclude)
        if !include_tests && test_detector.is_test_code(symbol, &node.core().file_path) {
            continue;
        }

        let result = solver.compute_cf(graph, &[node_idx], &policy, None);
        cf_results.push((symbol.clone(), result.total_context_size, type_str));
    }

    // Sort by CF (descending)
    cf_results.sort_by(|a, b| b.1.cmp(&a.1));

    let filter_msg = if !include_tests {
        " (excluding tests)"
    } else {
        ""
    };
    println!("\nTop {} nodes by Context Footprint{}:", limit, filter_msg);
    println!("{}", "=".repeat(80));

    for (i, (symbol, cf, node_type)) in cf_results.iter().take(limit).enumerate() {
        println!("{}. [{}] {} tokens", i + 1, node_type, cf);
        println!("   {}", symbol);
        println!();
    }

    Ok(())
}

pub fn search_symbols(
    graph: &ContextGraph,
    pattern: &str,
    with_cf: bool,
    limit: Option<usize>,
    include_tests: bool,
) -> Result<()> {
    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();
    let test_detector = UniversalTestDetector::new();

    println!("Searching for symbols matching: \"{}\"", pattern);
    println!("{}", "=".repeat(80));

    let pattern_lower = pattern.to_lowercase();
    let mut matches: Vec<(String, &str, u32)> = Vec::new();

    for (symbol, &node_idx) in &graph.symbol_to_node {
        let node = graph.node(node_idx);

        let type_str = match node {
            Node::Function(_) => "function",
            Node::Type(_) => "type",
            Node::Variable(_) => "variable",
        };

        // Simple substring match (case-insensitive)
        if symbol.to_lowercase().contains(&pattern_lower) {
            // Filter out test code if requested (default is to exclude)
            if !include_tests && test_detector.is_test_code(symbol, &node.core().file_path) {
                continue;
            }

            // Always compute CF for sorting, even if not displaying
            let result = solver.compute_cf(graph, &[node_idx], &policy, None);
            matches.push((symbol.clone(), type_str, result.total_context_size));
        }
    }

    // Sort by CF (descending)
    matches.sort_by(|a, b| b.2.cmp(&a.2));

    // Apply limit if specified
    let display_count = limit.unwrap_or(matches.len());
    let matches_to_show = &matches[..matches.len().min(display_count)];

    let filter_msg = if !include_tests {
        " (excluding tests)"
    } else {
        ""
    };
    println!(
        "Found {} matching symbol(s){}:\n",
        matches.len(),
        filter_msg
    );

    if let Some(lim) = limit.filter(|&lim| matches.len() > lim) {
        println!("Showing top {} by CF:\n", lim);
    }

    for (i, (symbol, node_type, cf)) in matches_to_show.iter().enumerate() {
        print!("{}. [{}] ", i + 1, node_type);
        if with_cf || limit.is_some() {
            print!("CF: {} tokens", cf);
        }
        println!("\n   {}", symbol);
        println!();
    }

    Ok(())
}

fn symbol_is_parameter(graph: &ContextGraph, node_idx: NodeIndex) -> bool {
    let symbol = graph
        .symbol_to_node
        .iter()
        .find(|&(_, &idx)| idx == node_idx)
        .map(|(s, _)| s.as_str())
        .unwrap_or("");

    // Python parameter pattern: .../func().(param)
    if symbol.contains("().(") && symbol.ends_with(')') {
        return true;
    }

    false
}

pub fn display_context_code(
    graph: &ContextGraph,
    symbol: &str,
    _show_boundaries: bool,
    source_reader: &dyn SourceReader,
    project_root: &str,
    max_tokens: Option<u32>,
) -> Result<()> {
    println!("Computing context for symbol: {}", symbol);

    let node_idx = graph
        .get_node_by_symbol(symbol)
        .ok_or_else(|| anyhow::anyhow!("Symbol not found: {}", symbol))?;

    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();
    let result = solver.compute_cf(graph, &[node_idx], &policy, max_tokens);

    println!("\nContext Summary:");
    println!("  Total size: {} tokens", result.total_context_size);
    println!("  Reachable nodes: {}", result.reachable_set.len());
    if let Some(limit) = max_tokens {
        println!("  Max tokens: {}", limit);
    }
    println!("{}", "=".repeat(80));

    for (depth, layer) in result.reachable_nodes_by_layer.iter().enumerate() {
        if layer.is_empty() {
            continue;
        }

        println!(
            "\n\u{1F310} Layer {}: {}",
            depth,
            if depth == 0 {
                "Observed Symbol"
            } else {
                "Direct Dependencies"
            }
        );
        println!("{}", "=".repeat(40));

        // Group nodes by file within this layer
        let mut files_map: std::collections::HashMap<String, Vec<NodeIndex>> =
            std::collections::HashMap::new();

        for &node_id in layer {
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

        // Sort files for consistent output within layer
        let mut file_list: Vec<_> = files_map.iter().collect();
        file_list.sort_by_key(|(path, _)| *path);

        for (file_path, nodes) in file_list {
            let full_path = std::path::Path::new(project_root).join(file_path);
            let full_path_str = full_path.to_string_lossy();

            println!("\n  \u{1F4C4} File: {}", file_path);

            // Sort nodes by start line within file
            let mut sorted_nodes = nodes.clone();
            sorted_nodes.sort_by_key(|&idx| graph.node(idx).core().span.start_line);

            // Filter out nodes that are contained within another node (e.g. nested functions)
            // unless we want to see everything.
            let mut top_level_nodes = Vec::new();
            for &idx in &sorted_nodes {
                let core = graph.node(idx).core();

                let is_sub_node = symbol_is_parameter(graph, idx);

                let is_contained = top_level_nodes.iter().any(|&prev_idx| {
                    let prev_core = graph.node(prev_idx).core();
                    core.span.start_line >= prev_core.span.start_line
                        && core.span.end_line <= prev_core.span.end_line
                        && idx != prev_idx
                });

                if !is_contained && !is_sub_node {
                    top_level_nodes.push(idx);
                }
            }

            for node_idx in top_level_nodes {
                let node = graph.node(node_idx);
                let core = node.core();

                // Get the symbol for this node
                let node_symbol = graph
                    .symbol_to_node
                    .iter()
                    .find(|&(_, &idx)| idx == node_idx)
                    .map(|(s, _)| s.as_str())
                    .unwrap_or(&core.name);

                println!(
                    "    Symbol: {} ({} tokens)",
                    node_symbol.split('/').next_back().unwrap_or(node_symbol),
                    core.context_size
                );
                println!(
                    "    Lines: {}-{}",
                    core.span.start_line + 1,
                    core.span.end_line + 1
                );

                // Read and display the code
                match source_reader.read_lines(
                    &full_path_str,
                    core.span.start_line as usize,
                    core.span.end_line as usize,
                ) {
                    Ok(lines) => {
                        println!("    Code:");
                        for (i, line) in lines.iter().enumerate() {
                            let line_num = core.span.start_line as usize + i + 1;
                            println!("      {:4} | {}", line_num, line);
                        }
                    }
                    Err(e) => {
                        println!("      [Error reading code: {}]", e);
                    }
                }
            }
        }
    }

    Ok(())
}

pub fn compute_and_display_cf_stats(graph: &ContextGraph, include_tests: bool) -> Result<()> {
    let policy = AcademicBaseline::default();
    let solver = CfSolver::new();
    let test_detector = UniversalTestDetector::new();
    let node_count = graph.graph.node_count();

    let mut function_cf: Vec<u32> = Vec::new();
    let mut type_cf: Vec<u32> = Vec::new();

    let filter_msg = if !include_tests {
        " (excluding tests)"
    } else {
        ""
    };
    println!("Calculating CF for {} nodes{}...", node_count, filter_msg);

    for (idx, node_idx) in graph.graph.node_indices().enumerate() {
        let node = graph.node(node_idx);

        if !include_tests {
            let symbol = graph
                .symbol_to_node
                .iter()
                .find(|&(_, &i)| i == node_idx)
                .map(|(s, _)| s.as_str())
                .unwrap_or("");

            if test_detector.is_test_code(symbol, &node.core().file_path) {
                continue;
            }
        }

        let result = solver.compute_cf(graph, &[node_idx], &policy, None);
        let cf = result.total_context_size;

        match node {
            Node::Function(_) => function_cf.push(cf),
            Node::Type(_) => type_cf.push(cf),
            Node::Variable(_) => {}
        }

        if (idx + 1) % 1000 == 0 {
            println!("  Processed {}/{} nodes...", idx + 1, node_count);
        }
    }

    println!("\n{}", "=".repeat(60));
    print_cf_distribution(&format!("Functions{}", filter_msg), &mut function_cf);
    println!("{}", "=".repeat(60));
    print_cf_distribution(&format!("Types{}", filter_msg), &mut type_cf);
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

    println!("\n  Percentiles:");
    for i in (5..=100).step_by(5) {
        let index = ((i * (sizes.len() - 1)) / 100).min(sizes.len() - 1);
        println!("    {:>3}%: {:>8} tokens", i, sizes[index]);
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::graph::ContextGraph;
    use crate::domain::node::{FunctionNode, NodeCore, SourceSpan, TypeNode, Visibility};
    use crate::domain::ports::SourceReader;
    use std::path::Path;

    struct MockReader;
    impl SourceReader for MockReader {
        fn read(&self, _path: &Path) -> Result<String> {
            Ok("line1\nline2\nline3\nline4\n".into())
        }
        fn read_lines(&self, _path: &str, start: usize, end: usize) -> Result<Vec<String>> {
            let lines = vec![
                "line1".to_string(),
                "line2".to_string(),
                "line3".to_string(),
                "line4".to_string(),
            ];
            Ok(lines[start..=end.min(lines.len() - 1)].to_vec())
        }
    }

    fn create_test_graph() -> ContextGraph {
        let mut graph = ContextGraph::new();
        let core1 = NodeCore::new(
            0,
            "func1".into(),
            None,
            10,
            SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            1.0,
            false,
            "file1.py".into(),
        );
        let node1 = Node::Function(FunctionNode {
            core: core1,
            param_count: 0,
            typed_param_count: 0,
            has_return_type: false,
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
        });
        graph.add_node("sym/func1().".into(), node1);

        let core2 = NodeCore::new(
            1,
            "Type1".into(),
            None,
            20,
            SourceSpan {
                start_line: 2,
                start_column: 0,
                end_line: 3,
                end_column: 0,
            },
            0.8,
            false,
            "file1.py".into(),
        );
        let node2 = Node::Type(TypeNode {
            core: core2,
            type_kind: crate::domain::node::TypeKind::Class,
            is_abstract: false,
            type_param_count: 0,
        });
        graph.add_node("sym/Type1#".into(), node2);

        let core3 = NodeCore::new(
            2,
            "test_func".into(),
            None,
            5,
            SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            1.0,
            false,
            "tests/test_file.py".into(),
        );
        let node3 = Node::Function(FunctionNode {
            core: core3,
            param_count: 0,
            typed_param_count: 0,
            has_return_type: false,
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
        });
        graph.add_node("sym/test_func().".into(), node3);

        graph
    }

    #[test]
    fn test_compute_cf_for_symbol_basic() {
        let graph = create_test_graph();
        assert!(compute_cf_for_symbols(&graph, &["sym/func1().".to_string()]).is_ok());
        assert!(compute_cf_for_symbols(&graph, &["nonexistent".to_string()]).is_err());
    }

    #[test]
    fn test_display_top_cf_nodes_filtering() {
        let graph = create_test_graph();

        // All nodes
        assert!(display_top_cf_nodes(&graph, 10, "all", true).is_ok());

        // Only functions
        assert!(display_top_cf_nodes(&graph, 10, "function", true).is_ok());

        // Only types
        assert!(display_top_cf_nodes(&graph, 10, "type", true).is_ok());

        // Exclude tests
        assert!(display_top_cf_nodes(&graph, 10, "all", false).is_ok());
    }

    #[test]
    fn test_search_symbols_variations() {
        let graph = create_test_graph();

        // Search with CF
        assert!(search_symbols(&graph, "func", true, None, true).is_ok());

        // Search with limit
        assert!(search_symbols(&graph, "func", false, Some(1), true).is_ok());

        // Search excluding tests
        assert!(search_symbols(&graph, "test", false, None, false).is_ok());

        // Search with no results
        assert!(search_symbols(&graph, "zzz", false, None, true).is_ok());
    }

    #[test]
    fn test_display_context_code_basic() {
        let graph = create_test_graph();
        let reader = MockReader;

        assert!(
            display_context_code(&graph, "sym/func1().", false, &reader, "/root", None).is_ok()
        );
        assert!(
            display_context_code(&graph, "nonexistent", false, &reader, "/root", None).is_err()
        );
    }

    #[test]
    fn test_symbol_is_parameter_logic() {
        let mut graph = ContextGraph::new();
        let core = NodeCore::new(
            0,
            "param".into(),
            None,
            1,
            SourceSpan {
                start_line: 0,
                start_column: 10,
                end_line: 0,
                end_column: 15,
            },
            0.0,
            false,
            "file.py".into(),
        );
        let node = Node::Variable(crate::domain::node::VariableNode {
            core,
            has_type_annotation: false,
            mutability: crate::domain::node::Mutability::Immutable,
            variable_kind: crate::domain::node::VariableKind::Global,
        });

        graph.add_node("sym/func().(param)".into(), node);
        let idx = graph.get_node_by_symbol("sym/func().(param)").unwrap();

        assert!(symbol_is_parameter(&graph, idx));

        // Non-parameter
        let core2 = NodeCore::new(
            1,
            "func".into(),
            None,
            10,
            SourceSpan {
                start_line: 0,
                start_column: 0,
                end_line: 1,
                end_column: 0,
            },
            1.0,
            false,
            "file.py".into(),
        );
        let node2 = Node::Function(FunctionNode {
            core: core2,
            param_count: 0,
            typed_param_count: 0,
            has_return_type: false,
            is_async: false,
            is_generator: false,
            visibility: Visibility::Public,
        });
        graph.add_node("sym/func().".into(), node2);
        let idx2 = graph.get_node_by_symbol("sym/func().").unwrap();

        assert!(!symbol_is_parameter(&graph, idx2));
    }

    #[test]
    fn test_compute_and_display_cf_stats_variations() {
        let graph = create_test_graph();
        assert!(compute_and_display_cf_stats(&graph, true).is_ok());
        assert!(compute_and_display_cf_stats(&graph, false).is_ok());
    }

    #[test]
    fn test_print_cf_distribution_logic() {
        let mut empty: Vec<u32> = vec![];
        print_cf_distribution("Empty", &mut empty);

        let mut data = vec![10, 20, 30, 40, 50, 100];
        print_cf_distribution("Test", &mut data);
    }
}
