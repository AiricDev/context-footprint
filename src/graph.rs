use crate::scip;
use crate::symbol::{ScipSymbol, SymbolType};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct NodeData {
    pub symbol: String,
    pub symbol_type: SymbolType,
}

pub struct ScipGraph {
    pub graph: DiGraph<NodeData, ()>,
    symbol_to_node: HashMap<String, NodeIndex>,
}

impl ScipGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            symbol_to_node: HashMap::new(),
        }
    }

    fn get_or_create_node(&mut self, symbol: &str) -> NodeIndex {
        if let Some(&idx) = self.symbol_to_node.get(symbol) {
            return idx;
        }

        let scip_symbol = ScipSymbol::parse(symbol);
        let symbol_type = scip_symbol
            .as_ref()
            .map(|s| s.infer_type())
            .unwrap_or(SymbolType::Other);

        let idx = self.graph.add_node(NodeData {
            symbol: symbol.to_string(),
            symbol_type,
        });
        self.symbol_to_node.insert(symbol.to_string(), idx);
        idx
    }

    pub fn build(&mut self, index: &scip::Index) {
        for document in &index.documents {
            self.process_document(document);
        }
    }

    fn process_document(&mut self, document: &scip::Document) {
        // 1. Identify all definitions in this document and their ranges
        // Map from range to symbol
        let mut defs: Vec<(Vec<i32>, &str)> = Vec::new();

        for occ in &document.occurrences {
            if (occ.symbol_roles & (scip::SymbolRole::Definition as i32)) != 0 {
                if !occ.symbol.is_empty() {
                    // Use enclosing_range if available for definitions, otherwise use range
                    let range = if !occ.enclosing_range.is_empty() {
                        &occ.enclosing_range
                    } else {
                        &occ.range
                    };
                    defs.push((range.clone(), &occ.symbol));
                }
            }
        }

        // 2. Process all references
        for occ in &document.occurrences {
            // If it's not a definition, or it's a reference (some can be both)
            // Actually, we want to find references to OTHER symbols.
            if (occ.symbol_roles & (scip::SymbolRole::Definition as i32)) == 0 {
                if occ.symbol.is_empty() || occ.symbol.starts_with("local ") {
                    continue;
                }

                // This is a reference. Find which definition it belongs to.
                // We use the reference's range and find the smallest definition range that encloses it.
                if let Some(caller_symbol) = find_enclosing_definition(&occ.range, &defs) {
                    let caller_idx = self.get_or_create_node(caller_symbol);
                    let callee_idx = self.get_or_create_node(&occ.symbol);
                    if caller_idx != callee_idx {
                        self.graph.add_edge(caller_idx, callee_idx, ());
                    }
                }
            }
        }
    }
}

/// Returns true if `inner` range is enclosed by `outer` range.
fn encloses(outer: &[i32], inner: &[i32]) -> bool {
    let (o_start_line, o_start_char, o_end_line, o_end_char) = parse_range(outer);
    let (i_start_line, i_start_char, i_end_line, i_end_char) = parse_range(inner);

    if o_start_line < i_start_line || (o_start_line == i_start_line && o_start_char <= i_start_char) {
        if o_end_line > i_end_line || (o_end_line == i_end_line && o_end_char >= i_end_char) {
            return true;
        }
    }
    false
}

fn parse_range(range: &[i32]) -> (i32, i32, i32, i32) {
    if range.len() == 3 {
        (range[0], range[1], range[0], range[2])
    } else if range.len() == 4 {
        (range[0], range[1], range[2], range[3])
    } else {
        (0, 0, 0, 0)
    }
}

fn find_enclosing_definition<'a>(
    ref_range: &[i32],
    defs: &[(Vec<i32>, &'a str)],
) -> Option<&'a str> {
    let mut best_def: Option<&str> = None;
    let mut best_range: Option<&[i32]> = None;

    for (def_range, symbol) in defs {
        if encloses(def_range, ref_range) {
            if let Some(prev_range) = best_range {
                // We want the SMALLEST enclosing range
                if encloses(prev_range, def_range) {
                    best_def = Some(symbol);
                    best_range = Some(def_range);
                }
            } else {
                best_def = Some(symbol);
                best_range = Some(def_range);
            }
        }
    }
    best_def
}
